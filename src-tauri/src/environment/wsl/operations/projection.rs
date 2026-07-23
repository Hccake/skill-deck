use tokio::time::Duration;

use crate::core::mutation::CancellationSignal;
use crate::environment::wsl::WslSession;
use crate::environment::wsl_protocol::{
    wsl_operation_with_features, WslExecutionFeature, WslOperationDescriptor, WslOperationExecutor,
    WslOperationRequest, DEFAULT_WSL_STDERR_LIMIT,
};
use crate::error::AppError;

const PROTOCOL_VERSION: &str = "2";
pub(crate) const PROJECT_TARGETS_SCRIPT: &str = include_str!("../scripts/projection.sh");
const PROJECT_TARGETS_OPERATION: WslOperationDescriptor = wsl_operation_with_features(
    "projection",
    "project-targets",
    PROJECT_TARGETS_SCRIPT,
    &[WslExecutionFeature::StableStat],
);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectedPosixTarget {
    pub index: u32,
    pub anchor_device: u64,
    pub anchor_inode: u64,
    pub physical_destination: String,
    pub relative_components: Vec<String>,
    pub storage_projection: String,
}

pub async fn project_targets(
    session: &WslSession,
    destinations: &[String],
    cancellation: Option<CancellationSignal>,
) -> Result<Vec<ProjectedPosixTarget>, AppError> {
    if destinations.is_empty() || destinations.iter().any(|path| !path.starts_with('/')) {
        return Err(AppError::Validation {
            field: Some("projection.destinations".to_string()),
            message: "WSL target projection requires absolute destinations".to_string(),
        });
    }
    let output = WslOperationExecutor::execute(
        &PROJECT_TARGETS_OPERATION,
        WslOperationRequest {
            session: session.clone(),
            args: destinations.to_vec(),
            stdin: Vec::new(),
            timeout: Duration::from_secs(10),
            stdout_limit: destinations
                .len()
                .saturating_mul(16 * 1024)
                .saturating_add(64),
            stderr_limit: DEFAULT_WSL_STDERR_LIMIT,
            cancellation,
        },
    )
    .await?;
    parse_projected_targets(&output.stdout, destinations.len())
}

pub fn parse_projected_targets(
    bytes: &[u8],
    expected: usize,
) -> Result<Vec<ProjectedPosixTarget>, AppError> {
    let mut fields = bytes.split(|byte| *byte == 0);
    if text(fields.next())? != PROTOCOL_VERSION {
        return Err(protocol_error());
    }
    let mut targets = Vec::with_capacity(expected);
    while let Some(tag) = fields.next() {
        if tag.is_empty() {
            continue;
        }
        if text(Some(tag))? != "P" {
            return Err(protocol_error());
        }
        let index = parse(fields.next())?;
        let anchor_device = parse(fields.next())?;
        let anchor_inode = parse(fields.next())?;
        let physical_destination = text(fields.next())?.to_string();
        let relative = text(fields.next())?;
        let storage_projection = text(fields.next())?.to_string();
        let relative_components = relative.split('/').map(str::to_string).collect::<Vec<_>>();
        if index as usize != targets.len()
            || !physical_destination.starts_with('/')
            || storage_projection.trim().is_empty()
            || relative_components.is_empty()
            || relative_components
                .iter()
                .any(|component| component.is_empty() || matches!(component.as_str(), "." | ".."))
        {
            return Err(protocol_error());
        }
        targets.push(ProjectedPosixTarget {
            index,
            anchor_device,
            anchor_inode,
            physical_destination,
            relative_components,
            storage_projection,
        });
    }
    if targets.len() != expected {
        return Err(protocol_error());
    }
    Ok(targets)
}

fn text(field: Option<&[u8]>) -> Result<&str, AppError> {
    std::str::from_utf8(field.ok_or_else(protocol_error)?).map_err(|_| protocol_error())
}

fn parse<T: std::str::FromStr>(field: Option<&[u8]>) -> Result<T, AppError> {
    text(field)?.parse().map_err(|_| protocol_error())
}

fn protocol_error() -> AppError {
    AppError::ConfigurationCorrupted {
        message: "invalid WSL target projection protocol response".to_string(),
    }
}

#[cfg(test)]
mod tests {
    #[cfg(target_os = "linux")]
    use std::fs;
    #[cfg(target_os = "linux")]
    use std::process::{Command, Stdio};

    #[cfg(target_os = "linux")]
    use tempfile::tempdir;

    use super::*;

    #[cfg(target_os = "linux")]
    fn wslpath_available() -> bool {
        Command::new("wslpath")
            .args(["-w", "/"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
    }

    #[test]
    fn parser_preserves_host_and_wsl_storage_projection_evidence() {
        let response = [
            "2",
            "P",
            "0",
            "7",
            "11",
            "/mnt/c/work/Foo",
            "Foo",
            r"C:\work",
            "P",
            "1",
            "8",
            "12",
            "/home/alice/foo",
            "foo",
            r"\\wsl.localhost\Ubuntu\home\alice",
        ]
        .join("\0");

        let projected = parse_projected_targets(response.as_bytes(), 2).unwrap();

        assert_eq!(projected[0].storage_projection, r"C:\work");
        assert_eq!(
            projected[1].storage_projection,
            r"\\wsl.localhost\Ubuntu\home\alice"
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn protocol_projects_missing_roots_from_the_resolved_existing_ancestor() {
        if !wslpath_available() {
            return;
        }
        let temp = tempdir().unwrap();
        let destination = temp.path().join(".custom/skills/demo");
        let output = Command::new("/bin/sh")
            .arg("-c")
            .arg(PROJECT_TARGETS_SCRIPT)
            .arg("--")
            .arg("project-targets")
            .arg(&destination)
            .output()
            .unwrap();

        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(output.stdout.starts_with(b"2\0"));
        let projected = parse_projected_targets(&output.stdout, 1).unwrap();
        assert_eq!(projected.len(), 1);
        assert_eq!(
            projected[0].physical_destination,
            destination.to_string_lossy()
        );
        assert_eq!(
            projected[0].relative_components,
            vec![".custom", "skills", "demo"]
        );
        assert!(!projected[0].storage_projection.is_empty());
        assert!(!destination.parent().unwrap().exists());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn protocol_resolves_an_existing_symlink_before_appending_missing_components() {
        if !wslpath_available() {
            return;
        }
        use std::os::unix::fs::symlink;

        let temp = tempdir().unwrap();
        let physical = temp.path().join("physical");
        let logical = temp.path().join("logical");
        fs::create_dir(&physical).unwrap();
        symlink(&physical, &logical).unwrap();
        let destination = logical.join("skills/demo");
        let output = Command::new("/bin/sh")
            .arg("-c")
            .arg(PROJECT_TARGETS_SCRIPT)
            .arg("--")
            .arg("project-targets")
            .arg(&destination)
            .output()
            .unwrap();

        assert!(output.status.success());
        let projected = parse_projected_targets(&output.stdout, 1).unwrap();
        assert_eq!(
            projected[0].physical_destination,
            physical.join("skills/demo").to_string_lossy()
        );
        assert_eq!(projected[0].relative_components, vec!["skills", "demo"]);
    }
}

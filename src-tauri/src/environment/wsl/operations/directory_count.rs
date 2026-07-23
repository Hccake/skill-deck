use tokio::time::Duration;

use crate::environment::wsl::WslSession;
use crate::environment::wsl_protocol::{
    decode_nul_records, wsl_operation, WslOperationDescriptor, WslOperationExecutor,
    WslOperationRequest, DEFAULT_WSL_STDERR_LIMIT,
};
use crate::error::AppError;

pub(crate) const DIRECTORY_COUNT_SCRIPT: &str = include_str!("../scripts/directory-count.sh");
const DIRECTORY_COUNT_OPERATION: WslOperationDescriptor =
    wsl_operation("directory-count", "inspect", DIRECTORY_COUNT_SCRIPT);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectoryCountFact {
    pub path: String,
    pub observed_count: Option<u32>,
    pub truncated: bool,
}

pub async fn inspect(
    session: &WslSession,
    paths: &[String],
    limit: u32,
) -> Result<Vec<DirectoryCountFact>, AppError> {
    if paths.is_empty() || limit == 0 || paths.iter().any(|path| !path.starts_with('/')) {
        return Err(AppError::Validation {
            field: Some("directoryCount".to_string()),
            message: "WSL directory count requires absolute paths and a positive limit".to_string(),
        });
    }
    let mut args = Vec::with_capacity(paths.len() + 1);
    args.push(limit.to_string());
    args.extend(paths.iter().cloned());
    let output = WslOperationExecutor::execute(
        &DIRECTORY_COUNT_OPERATION,
        WslOperationRequest {
            session: session.clone(),
            args,
            stdin: Vec::new(),
            timeout: Duration::from_secs(20),
            stdout_limit: paths.len().saturating_mul(16 * 1024).saturating_add(64),
            stderr_limit: DEFAULT_WSL_STDERR_LIMIT,
            cancellation: None,
        },
    )
    .await?;
    let facts = parse_directory_counts(&output.stdout, paths)?;
    if facts.len() != paths.len()
        || facts
            .iter()
            .zip(paths)
            .any(|(fact, path)| fact.path != *path)
    {
        return Err(protocol_error());
    }
    Ok(facts)
}

pub fn parse_directory_counts(
    bytes: &[u8],
    expected_paths: &[String],
) -> Result<Vec<DirectoryCountFact>, AppError> {
    let records = decode_nul_records(bytes);
    if records.first().map(String::as_str) != Some("1") {
        return Err(protocol_error());
    }
    let mut facts = Vec::new();
    let mut index = 1;
    while index < records.len() {
        if records.get(index).map(String::as_str) != Some("path") || index + 4 >= records.len() {
            return Err(protocol_error());
        }
        let path = records[index + 1].clone();
        let (observed_count, truncated) = match records[index + 2].as_str() {
            "none" if records[index + 3] == "0" && records[index + 4] == "0" => (None, false),
            "count" => {
                let count = records[index + 3]
                    .parse::<u32>()
                    .map_err(|_| protocol_error())?;
                let truncated = match records[index + 4].as_str() {
                    "0" => false,
                    "1" => true,
                    _ => return Err(protocol_error()),
                };
                (Some(count), truncated)
            }
            _ => return Err(protocol_error()),
        };
        facts.push(DirectoryCountFact {
            path,
            observed_count,
            truncated,
        });
        index += 5;
    }
    if facts.len() != expected_paths.len() {
        return Err(protocol_error());
    }
    Ok(facts)
}

fn protocol_error() -> AppError {
    AppError::ConfigurationCorrupted {
        message: "invalid WSL directory count protocol response".to_string(),
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use std::process::Command;

    use super::{parse_directory_counts, DIRECTORY_COUNT_SCRIPT};

    #[test]
    fn versioned_directory_count_isolates_missing_paths_and_reports_counts() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join("skills/one")).unwrap();
        std::fs::create_dir_all(temp.path().join("skills/two")).unwrap();
        let skills = temp.path().join("skills").to_string_lossy().into_owned();
        let missing = temp.path().join("missing").to_string_lossy().into_owned();
        let output = Command::new("/bin/sh")
            .arg("-c")
            .arg(DIRECTORY_COUNT_SCRIPT)
            .arg("--")
            .arg("inspect")
            .arg("10000")
            .arg(&skills)
            .arg(&missing)
            .output()
            .unwrap();

        assert!(output.status.success());
        let facts =
            parse_directory_counts(&output.stdout, &[skills.clone(), missing.clone()]).unwrap();
        assert_eq!(facts[0].path, skills);
        assert_eq!(facts[0].observed_count, Some(2));
        assert!(!facts[0].truncated);
        assert_eq!(facts[1].path, missing);
        assert_eq!(facts[1].observed_count, None);
    }
}

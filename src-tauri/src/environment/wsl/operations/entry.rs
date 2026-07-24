use sha2::{Digest, Sha256};
use tokio::time::Duration;

use crate::core::mutation::CancellationSignal;
use crate::environment::runtime::EntryFingerprint;
use crate::environment::wsl::WslSession;
use crate::environment::wsl_protocol::{
    wsl_operation_with_features, WslExecutionFeature, WslOperationDescriptor, WslOperationExecutor,
    WslOperationRequest, DEFAULT_WSL_STDERR_LIMIT,
};
use crate::error::AppError;

const PROTOCOL_VERSION: &str = "1";
pub(crate) const ENTRY_STATE_SCRIPT: &str = include_str!("../scripts/entry.sh");
const ENTRY_STATE_OPERATION: WslOperationDescriptor = wsl_operation_with_features(
    "entry-state",
    "inspect",
    ENTRY_STATE_SCRIPT,
    &[
        WslExecutionFeature::Sha256Sum,
        WslExecutionFeature::StableStat,
    ],
);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PosixEntryKind {
    Missing,
    File,
    Directory,
    Symlink,
    BrokenLink,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PosixEntryState {
    pub index: u32,
    pub kind: PosixEntryKind,
    pub fingerprint: EntryFingerprint,
    pub link_target: Option<String>,
}

pub async fn inspect_entries(
    session: &WslSession,
    paths: &[String],
    cancellation: Option<CancellationSignal>,
) -> Result<Vec<PosixEntryState>, AppError> {
    if paths.is_empty() || paths.iter().any(|path| !path.starts_with('/')) {
        return Err(AppError::Validation {
            field: Some("entry.paths".to_string()),
            message: "WSL entry inspection requires absolute paths".to_string(),
        });
    }
    let output = WslOperationExecutor::execute(
        &ENTRY_STATE_OPERATION,
        WslOperationRequest {
            session: session.clone(),
            args: paths.to_vec(),
            stdin: Vec::new(),
            timeout: Duration::from_secs(10),
            stdout_limit: paths.len().saturating_mul(1024).saturating_add(64),
            stderr_limit: DEFAULT_WSL_STDERR_LIMIT,
            cancellation,
        },
    )
    .await?;
    parse_entry_states(&output.stdout, paths.len())
}

pub fn parse_entry_states(bytes: &[u8], expected: usize) -> Result<Vec<PosixEntryState>, AppError> {
    let mut fields = bytes.split(|byte| *byte == 0);
    if text(fields.next())? != PROTOCOL_VERSION {
        return Err(protocol_error());
    }
    let mut states = Vec::with_capacity(expected);
    while let Some(tag) = fields.next() {
        if tag.is_empty() {
            continue;
        }
        if text(Some(tag))? != "S" {
            return Err(protocol_error());
        }
        let index = parse::<u32>(fields.next())?;
        let kind = match text(fields.next())? {
            "missing" => PosixEntryKind::Missing,
            "file" => PosixEntryKind::File,
            "directory" => PosixEntryKind::Directory,
            "symlink" => PosixEntryKind::Symlink,
            "brokenLink" => PosixEntryKind::BrokenLink,
            "other" => PosixEntryKind::Other,
            _ => return Err(protocol_error()),
        };
        let device = text(fields.next())?;
        let inode = text(fields.next())?;
        let mode = text(fields.next())?;
        let size = text(fields.next())?;
        let mtime_seconds = text(fields.next())?;
        let mtime_nanos = text(fields.next())?;
        let link_target = optional_text(fields.next())?;
        if index as usize != states.len() {
            return Err(protocol_error());
        }
        let fingerprint = if kind == PosixEntryKind::Missing {
            if [device, inode, mode, size, mtime_seconds, mtime_nanos]
                .iter()
                .any(|value| !value.is_empty())
                || link_target.is_some()
            {
                return Err(protocol_error());
            }
            EntryFingerprint("entry-v1-missing".to_string())
        } else {
            if [device, inode, mode, size, mtime_seconds, mtime_nanos]
                .iter()
                .any(|value| value.is_empty())
            {
                return Err(protocol_error());
            }
            let mut hasher = Sha256::new();
            hasher.update(b"skill-deck-wsl-entry-v1\0");
            for value in [device, inode, mode, size, mtime_seconds, mtime_nanos] {
                hasher.update(value.as_bytes());
                hasher.update([0]);
            }
            if let Some(target) = &link_target {
                hasher.update(target.as_bytes());
            }
            EntryFingerprint(format!("entry-v1-{:x}", hasher.finalize()))
        };
        states.push(PosixEntryState {
            index,
            kind,
            fingerprint,
            link_target,
        });
    }
    if states.len() != expected {
        return Err(protocol_error());
    }
    Ok(states)
}

fn text(field: Option<&[u8]>) -> Result<&str, AppError> {
    std::str::from_utf8(field.ok_or_else(protocol_error)?).map_err(|_| protocol_error())
}

fn optional_text(field: Option<&[u8]>) -> Result<Option<String>, AppError> {
    let value = text(field)?;
    Ok((!value.is_empty()).then(|| value.to_string()))
}

fn parse<T: std::str::FromStr>(field: Option<&[u8]>) -> Result<T, AppError> {
    text(field)?.parse().map_err(|_| protocol_error())
}

fn protocol_error() -> AppError {
    AppError::ConfigurationCorrupted {
        message: "invalid WSL entry state protocol response".to_string(),
    }
}

#[cfg(all(test, target_os = "linux"))]
#[allow(
    clippy::disallowed_methods,
    reason = "目录项协议测试需要直接运行待验证的 shell 测试脚本"
)]
mod tests {
    use std::fs;
    use std::os::unix::fs::symlink;
    use std::process::Command;

    use tempfile::tempdir;

    use super::*;

    fn inspect(paths: &[String]) -> Vec<PosixEntryState> {
        let output = Command::new("/bin/sh")
            .arg("-c")
            .arg(ENTRY_STATE_SCRIPT)
            .arg("--")
            .arg("inspect")
            .args(paths)
            .output()
            .expect("entry state script");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        parse_entry_states(&output.stdout, paths.len()).expect("parse")
    }

    #[test]
    fn fingerprint_is_stable_for_missing_and_changes_with_file_state() {
        let temp = tempdir().expect("temp");
        let missing = temp.path().join("missing");
        let file = temp.path().join("file");
        fs::write(&file, b"first").unwrap();
        let paths = vec![
            missing.to_string_lossy().into_owned(),
            file.to_string_lossy().into_owned(),
        ];
        let before = inspect(&paths);

        assert_eq!(before[0].kind, PosixEntryKind::Missing);
        assert_eq!(before[0].fingerprint.0, "entry-v1-missing");
        assert_eq!(before[1].kind, PosixEntryKind::File);

        fs::write(&file, b"second-longer").unwrap();
        let after = inspect(&paths);
        assert_ne!(before[1].fingerprint, after[1].fingerprint);
    }

    #[test]
    fn final_symlink_is_fingerprinted_without_following_its_target() {
        let temp = tempdir().expect("temp");
        let target = temp.path().join("target");
        let link = temp.path().join("link");
        fs::write(&target, b"first").unwrap();
        symlink(&target, &link).unwrap();
        let path = link.to_string_lossy().into_owned();
        let before = inspect(std::slice::from_ref(&path));

        fs::write(&target, b"target changed").unwrap();
        let after = inspect(&[path]);

        assert_eq!(before[0].kind, PosixEntryKind::Symlink);
        assert_eq!(
            before[0].link_target.as_deref(),
            Some(target.as_path().to_str().unwrap())
        );
        assert_eq!(before[0].fingerprint, after[0].fingerprint);
    }

    #[test]
    fn broken_final_symlink_has_an_explicit_kind() {
        let temp = tempdir().expect("temp");
        let link = temp.path().join("broken");
        symlink(temp.path().join("missing-target"), &link).unwrap();

        let state = inspect(&[link.to_string_lossy().into_owned()]);

        assert_eq!(state[0].kind, PosixEntryKind::BrokenLink);
    }
}

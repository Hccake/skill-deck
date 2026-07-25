#[cfg(all(test, target_os = "linux"))]
use std::path::{Path, PathBuf};

use tokio::time::Duration;

use crate::environment::types::{EnvironmentRef, ResourceLocator};
use crate::environment::wsl::WslSession;
use crate::environment::wsl_protocol::{
    wsl_operation, WslOperationDescriptor, WslOperationExecutor, WslOperationRequest,
    DEFAULT_WSL_STDERR_LIMIT, DEFAULT_WSL_STDOUT_LIMIT,
};
use crate::error::AppError;
use crate::storage::atomic_document::{AtomicDocumentIo, IoFuture};

const READ_SCRIPT: &str = include_str!("../scripts/atomic-file.sh");
pub(crate) const WRITE_SCRIPT: &str = include_str!("../scripts/atomic-file.sh");
const READ_OPERATION: WslOperationDescriptor = wsl_operation("atomic-file", "read", READ_SCRIPT);
const WRITE_OPERATION: WslOperationDescriptor = wsl_operation("atomic-file", "write", WRITE_SCRIPT);

pub struct WslAtomicDocumentIo {
    session: WslSession,
}

impl WslAtomicDocumentIo {
    pub fn new(session: WslSession) -> Self {
        Self { session }
    }

    async fn run(
        &self,
        operation: &WslOperationDescriptor,
        path: &str,
        stdin: Vec<u8>,
        stdout_limit: usize,
    ) -> Result<Vec<u8>, AppError> {
        let output = WslOperationExecutor::execute(
            operation,
            WslOperationRequest {
                session: self.session.clone(),
                args: vec![path.to_string()],
                stdin,
                timeout: Duration::from_secs(10),
                stdout_limit,
                stderr_limit: DEFAULT_WSL_STDERR_LIMIT,
                cancellation: None,
            },
        )
        .await?;
        Ok(output.stdout)
    }

    fn path<'a>(&self, target: &'a ResourceLocator) -> Result<&'a str, AppError> {
        match &target.environment {
            EnvironmentRef::Wsl { distro_name }
                if distro_name.eq_ignore_ascii_case(&self.session.distro_name)
                    && target.native_path.starts_with('/') =>
            {
                Ok(&target.native_path)
            }
            _ => Err(AppError::StorageUnsupported {
                path: target.native_path.clone(),
            }),
        }
    }
}

impl AtomicDocumentIo for WslAtomicDocumentIo {
    fn read_optional<'a>(
        &'a self,
        target: &'a ResourceLocator,
    ) -> IoFuture<'a, Result<Option<Vec<u8>>, AppError>> {
        Box::pin(async move {
            let output = self
                .run(
                    &READ_OPERATION,
                    self.path(target)?,
                    Vec::new(),
                    DEFAULT_WSL_STDOUT_LIMIT,
                )
                .await?;
            parse_read_response(&output)
        })
    }

    fn write_atomic<'a>(
        &'a self,
        target: &'a ResourceLocator,
        bytes: Vec<u8>,
    ) -> IoFuture<'a, Result<(), AppError>> {
        Box::pin(async move {
            let output = self
                .run(&WRITE_OPERATION, self.path(target)?, bytes, 32)
                .await?;
            parse_write_response(&output)
        })
    }
}

pub fn parse_read_response(bytes: &[u8]) -> Result<Option<Vec<u8>>, AppError> {
    let (version, rest) = take_field(bytes)?;
    let (exists, body) = take_field(rest)?;
    if version != b"1" {
        return Err(protocol_error());
    }
    match exists {
        b"0" if body.is_empty() => Ok(None),
        b"1" => Ok(Some(body.to_vec())),
        _ => Err(protocol_error()),
    }
}

pub fn parse_write_response(bytes: &[u8]) -> Result<(), AppError> {
    (bytes == b"1\0").then_some(()).ok_or_else(protocol_error)
}

fn take_field(bytes: &[u8]) -> Result<(&[u8], &[u8]), AppError> {
    let index = bytes
        .iter()
        .position(|byte| *byte == 0)
        .ok_or_else(protocol_error)?;
    Ok((&bytes[..index], &bytes[index + 1..]))
}

fn protocol_error() -> AppError {
    AppError::ConfigurationCorrupted {
        message: "invalid WSL atomic document protocol response".to_string(),
    }
}

#[cfg(all(test, target_os = "linux"))]
fn backup_path(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".bak");
    path.with_file_name(name)
}

#[cfg(all(test, target_os = "linux"))]
#[allow(
    clippy::disallowed_methods,
    reason = "原子文件协议测试需要直接运行待验证的 shell 测试脚本"
)]
mod tests {
    use std::fs;
    use std::io::Write;
    use std::process::{Command, Stdio};

    use tempfile::tempdir;

    use super::*;

    fn run_write(path: &Path, content: &[u8]) {
        let mut child = Command::new("/bin/sh")
            .arg("-c")
            .arg(WRITE_SCRIPT)
            .arg("--")
            .arg("write")
            .arg(path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("write script");
        child.stdin.take().unwrap().write_all(content).unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        parse_write_response(&output.stdout).expect("write response");
    }

    fn run_read(path: &Path) -> Vec<u8> {
        let output = Command::new("/bin/sh")
            .arg("-c")
            .arg(READ_SCRIPT)
            .arg("--")
            .arg("read")
            .arg(path)
            .output()
            .expect("read script");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        output.stdout
    }

    #[test]
    fn posix_atomic_write_leaves_no_sidecar() {
        let temp = tempdir().expect("temp");
        let path = temp.path().join("state/document.json");
        run_write(&path, &[0, 1, 2]);
        assert_eq!(fs::read(&path).unwrap(), [0, 1, 2]);
        assert!(!backup_path(&path).exists());

        fs::write(backup_path(&path), b"legacy backup").expect("legacy backup");
        run_write(&path, &[3, 0, 4]);
        assert_eq!(fs::read(&path).unwrap(), [3, 0, 4]);
        assert!(!backup_path(&path).exists());

        run_write(&path, &[5]);
        assert_eq!(fs::read(&path).unwrap(), [5]);
        assert!(!backup_path(&path).exists());
        assert_eq!(fs::read_dir(path.parent().unwrap()).unwrap().count(), 1);
    }

    #[test]
    fn optional_read_parser_preserves_binary_body_and_rejects_invalid_header() {
        assert_eq!(parse_read_response(b"1\0\x30\0").unwrap(), None);
        assert_eq!(
            parse_read_response(&[b'1', 0, b'1', 0, 0, 255, 1]).unwrap(),
            Some(vec![0, 255, 1])
        );
        assert!(parse_read_response(b"2\0\x31\0data").is_err());
    }

    #[test]
    fn optional_read_script_separates_protocol_fields_before_file_content() {
        let temp = tempdir().expect("temp");
        let path = temp.path().join("projects.json");
        fs::write(&path, br#"{"schemaVersion":1}"#).expect("fixture document");

        assert_eq!(
            parse_read_response(&run_read(&path)).expect("read response"),
            Some(br#"{"schemaVersion":1}"#.to_vec())
        );
        assert_eq!(
            parse_read_response(&run_read(&temp.path().join("missing.json")))
                .expect("missing response"),
            None
        );
    }

    #[cfg(unix)]
    #[test]
    fn atomic_write_does_not_replace_the_document_when_durability_fails() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempdir().expect("temp");
        let path = temp.path().join("state/document.json");
        fs::create_dir_all(path.parent().expect("parent")).expect("state directory");
        fs::write(&path, b"previous").expect("existing document");
        let bin = temp.path().join("bin");
        fs::create_dir(&bin).expect("bin");
        let sync = bin.join("sync");
        fs::write(&sync, b"#!/bin/sh\nexit 1\n").expect("failing sync");
        fs::set_permissions(&sync, fs::Permissions::from_mode(0o755)).expect("sync mode");
        let path_env = format!(
            "{}:{}",
            bin.display(),
            std::env::var("PATH").unwrap_or_default()
        );

        let mut child = Command::new("/bin/sh")
            .arg("-c")
            .arg(WRITE_SCRIPT)
            .arg("--")
            .arg("write")
            .arg(&path)
            .env("PATH", path_env)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("write script");
        child
            .stdin
            .take()
            .expect("stdin")
            .write_all(b"replacement")
            .expect("replacement");
        let output = child.wait_with_output().expect("write result");

        assert!(!output.status.success());
        assert_eq!(fs::read(&path).expect("preserved document"), b"previous");
    }
}

#[cfg(all(test, not(target_os = "linux")))]
mod portable_tests {
    use super::parse_read_response;

    #[test]
    fn optional_read_parser_preserves_binary_body_and_rejects_invalid_header() {
        assert_eq!(parse_read_response(b"1\0\x30\0").unwrap(), None);
        assert_eq!(
            parse_read_response(&[b'1', 0, b'1', 0, 0, 255, 1]).unwrap(),
            Some(vec![0, 255, 1])
        );
        assert!(parse_read_response(b"2\0\x31\0data").is_err());
    }
}

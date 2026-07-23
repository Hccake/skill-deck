use tokio::time::Duration;

use crate::core::mutation::CancellationSignal;
use crate::environment::wsl::WslSession;
use crate::environment::wsl_protocol::{
    wsl_operation, WslOperationDescriptor, WslOperationExecutor, WslOperationRequest,
    DEFAULT_WSL_STDERR_LIMIT,
};
use crate::error::AppError;

const MAP_HOST_PATH_SCRIPT: &str = include_str!("../scripts/path.sh");
const MAP_STORAGE_PATH_TO_HOST_SCRIPT: &str = include_str!("../scripts/path.sh");
const MAP_HOST_PATH_OPERATION: WslOperationDescriptor =
    wsl_operation("path", "map-host", MAP_HOST_PATH_SCRIPT);
const MAP_STORAGE_PATH_TO_HOST_OPERATION: WslOperationDescriptor =
    wsl_operation("path", "map-storage-host", MAP_STORAGE_PATH_TO_HOST_SCRIPT);

pub async fn map_host_bridge_path(
    session: &WslSession,
    host_path: &str,
    cancellation: Option<CancellationSignal>,
) -> Result<String, AppError> {
    if host_path.is_empty() || host_path.contains('\0') {
        return Err(AppError::Validation {
            field: Some("bridgePath".to_string()),
            message: "Host bridge path is invalid".to_string(),
        });
    }
    let output = WslOperationExecutor::execute(
        &MAP_HOST_PATH_OPERATION,
        WslOperationRequest {
            session: session.clone(),
            args: vec![host_path.to_string()],
            stdin: Vec::new(),
            timeout: Duration::from_secs(10),
            stdout_limit: 16 * 1024,
            stderr_limit: DEFAULT_WSL_STDERR_LIMIT,
            cancellation,
        },
    )
    .await?;
    parse_mapped_path(&output.stdout)
}

pub fn parse_mapped_path(bytes: &[u8]) -> Result<String, AppError> {
    let fields = bytes.split(|byte| *byte == 0).collect::<Vec<_>>();
    if fields.len() != 3 || fields[0] != b"1" || !fields[2].is_empty() {
        return Err(protocol_error());
    }
    let path = std::str::from_utf8(fields[1]).map_err(|_| protocol_error())?;
    if !path.starts_with('/') || path.contains('\0') {
        return Err(protocol_error());
    }
    Ok(path.to_string())
}

pub async fn map_storage_path_to_host(
    session: &WslSession,
    storage_path: &str,
    cancellation: Option<CancellationSignal>,
) -> Result<String, AppError> {
    if !storage_path.starts_with('/') || storage_path.contains('\0') {
        return Err(AppError::Validation {
            field: Some("storagePath".to_string()),
            message: "WSL storage path must be absolute".to_string(),
        });
    }
    let output = WslOperationExecutor::execute(
        &MAP_STORAGE_PATH_TO_HOST_OPERATION,
        WslOperationRequest {
            session: session.clone(),
            args: vec![storage_path.to_string()],
            stdin: Vec::new(),
            timeout: Duration::from_secs(10),
            stdout_limit: 16 * 1024,
            stderr_limit: DEFAULT_WSL_STDERR_LIMIT,
            cancellation,
        },
    )
    .await?;
    parse_host_storage_path(&output.stdout)
}

pub fn parse_host_storage_path(bytes: &[u8]) -> Result<String, AppError> {
    let fields = bytes.split(|byte| *byte == 0).collect::<Vec<_>>();
    if fields.len() != 3 || fields[0] != b"1" || !fields[2].is_empty() || fields[1].is_empty() {
        return Err(protocol_error());
    }
    let path = std::str::from_utf8(fields[1]).map_err(|_| protocol_error())?;
    if path.contains('\0') {
        return Err(protocol_error());
    }
    Ok(path.to_string())
}

fn protocol_error() -> AppError {
    AppError::ConfigurationCorrupted {
        message: "invalid WSL Host bridge path response".to_string(),
    }
}

#[cfg(test)]
mod tests {
    #[cfg(target_os = "linux")]
    use std::fs;
    #[cfg(target_os = "linux")]
    use std::process::Command;

    #[cfg(target_os = "linux")]
    use tempfile::tempdir;

    use super::*;

    #[cfg(target_os = "linux")]
    #[test]
    fn host_path_mapping_returns_only_a_versioned_absolute_posix_path() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempdir().unwrap();
        let tool = temp.path().join("wslpath");
        fs::write(&tool, "#!/bin/sh\nprintf '/custom/c/Bridge Path\\n'\n").unwrap();
        fs::set_permissions(&tool, fs::Permissions::from_mode(0o755)).unwrap();
        let output = Command::new("/bin/sh")
            .arg("-c")
            .arg(MAP_HOST_PATH_SCRIPT)
            .arg("--")
            .arg("map-host")
            .arg(r"C:\Temp\Bridge Path")
            .env("PATH", temp.path())
            .output()
            .unwrap();
        assert!(output.status.success());
        assert_eq!(
            parse_mapped_path(&output.stdout).unwrap(),
            "/custom/c/Bridge Path"
        );
        assert!(parse_mapped_path(b"1\0relative\0").is_err());
        assert!(parse_mapped_path(b"2\0/mnt/c/bridge\0").is_err());
    }

    #[test]
    fn storage_path_mapping_returns_only_one_versioned_windows_path() {
        assert_eq!(
            parse_host_storage_path(b"1\0C:\\Code\\App\0").unwrap(),
            r"C:\Code\App"
        );
        assert_eq!(
            parse_host_storage_path(b"1\0\\\\wsl.localhost\\Ubuntu\\home\\me\\app\0").unwrap(),
            r"\\wsl.localhost\Ubuntu\home\me\app"
        );
        assert!(parse_host_storage_path(b"2\0C:\\Code\\App\0").is_err());
        assert!(parse_host_storage_path(b"1\0\0").is_err());
    }
}

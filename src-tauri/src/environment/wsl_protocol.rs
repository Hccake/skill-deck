pub fn build_wsl_exec_args(
    distro_name: &str,
    user: &str,
    script: &str,
    positional_args: &[String],
) -> Vec<String> {
    let mut args = vec![
        "--distribution".to_string(),
        distro_name.to_string(),
        "--user".to_string(),
        user.to_string(),
        "--exec".to_string(),
        "/bin/sh".to_string(),
        "-c".to_string(),
        script.to_string(),
        "--".to_string(),
    ];
    args.extend_from_slice(positional_args);
    args
}

pub fn decode_nul_records(bytes: &[u8]) -> Vec<String> {
    bytes
        .split(|byte| *byte == 0)
        .filter(|record| !record.is_empty())
        .map(|record| String::from_utf8_lossy(record).into_owned())
        .collect()
}

#[cfg(target_os = "windows")]
pub async fn run_wsl_script(
    session: &WslSession,
    script: &'static str,
    positional_args: &[String],
    stdin_payload: Vec<u8>,
    timeout_duration: Duration,
) -> Result<Vec<u8>, AppError> {
    let args = build_wsl_exec_args(&session.distro_name, &session.user, script, positional_args);
    let mut command = Command::new("wsl.exe");
    command
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let mut child = command.spawn()?;
    let mut stdin = child.stdin.take().ok_or_else(|| AppError::Custom {
        message: "failed to open WSL stdin".to_string(),
    })?;
    let writer = tokio::spawn(async move {
        stdin.write_all(&stdin_payload).await?;
        stdin.shutdown().await
    });
    let output = timeout(timeout_duration, child.wait_with_output())
        .await
        .map_err(|_| AppError::Custom {
            message: "WSL command timed out".to_string(),
        })??;
    writer.await.map_err(|error| AppError::Custom {
        message: error.to_string(),
    })??;
    if !output.status.success() {
        return Err(AppError::Custom {
            message: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        });
    }
    Ok(output.stdout)
}

#[cfg(not(target_os = "windows"))]
pub async fn run_wsl_script(
    _session: &WslSession,
    _script: &'static str,
    _positional_args: &[String],
    _stdin_payload: Vec<u8>,
    _timeout_duration: Duration,
) -> Result<Vec<u8>, AppError> {
    Err(AppError::Custom {
        message: "WSL is only available on Windows".to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::{build_wsl_exec_args, decode_nul_records};

    #[test]
    fn user_values_are_positional_arguments_not_shell_source() {
        let args = build_wsl_exec_args(
            "Ubuntu Test",
            "alice",
            "printf '%s\\0' \"$1\"",
            &["$(touch /tmp/pwned)".to_string()],
        );

        assert_eq!(
            args[0..8],
            [
                "--distribution",
                "Ubuntu Test",
                "--user",
                "alice",
                "--exec",
                "/bin/sh",
                "-c",
                "printf '%s\\0' \"$1\"",
            ]
        );
        assert_eq!(args[8], "--");
        assert_eq!(args[9], "$(touch /tmp/pwned)");
    }

    #[test]
    fn decodes_nul_delimited_versioned_records() {
        assert_eq!(
            decode_nul_records(b"1\0first\0second\0"),
            vec!["1", "first", "second"]
        );
    }
}
use std::process::Stdio;

use tokio::io::AsyncWriteExt;
use tokio::process::Command;
use tokio::time::{timeout, Duration};

use crate::environment::wsl::WslSession;
use crate::error::AppError;

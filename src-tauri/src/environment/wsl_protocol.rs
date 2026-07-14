#![cfg_attr(not(target_os = "windows"), allow(dead_code))]

use std::future::pending;
#[cfg(target_os = "windows")]
use std::process::Stdio;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::process::Child;
#[cfg(target_os = "windows")]
use tokio::process::Command;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio::time::{Duration, Instant};

use crate::core::mutation::CancellationSignal;
use crate::environment::types::EnvironmentRef;
use crate::environment::wsl::WslSession;
use crate::error::AppError;

pub const DEFAULT_WSL_STDOUT_LIMIT: usize = 16 * 1024 * 1024;
pub const DEFAULT_WSL_STDERR_LIMIT: usize = 1024 * 1024;
const CANCELLATION_POLL_INTERVAL: Duration = Duration::from_millis(25);
const WSL_BOOTSTRAP_SCRIPT: &str = r#"printf 'skill-deck-wsl-shell-started-v1\n' >&2; script=$1; shift; exec /bin/sh -c "$script" -- "$@""#;
const WSL_SHELL_STARTED_MARKER: &[u8] = b"skill-deck-wsl-shell-started-v1\n";

pub struct WslCommandRequest {
    pub session: WslSession,
    pub script: &'static str,
    pub args: Vec<String>,
    pub stdin: Vec<u8>,
    pub timeout: Duration,
    pub stdout_limit: usize,
    pub stderr_limit: usize,
    pub cancellation: Option<CancellationSignal>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WslCommandOutput {
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub exit_code: Option<i32>,
}

pub struct WslCommandRunner;

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

fn build_wsl_runner_exec_args(request: &WslCommandRequest) -> Vec<String> {
    let mut args = Vec::with_capacity(request.args.len() + 1);
    args.push(request.script.to_string());
    args.extend(request.args.iter().cloned());
    build_wsl_exec_args(
        &request.session.distro_name,
        &request.session.user,
        WSL_BOOTSTRAP_SCRIPT,
        &args,
    )
}

async fn read_bounded<R>(
    mut reader: R,
    stream: &'static str,
    limit: usize,
    limit_tx: mpsc::Sender<AppError>,
) -> Result<Vec<u8>, AppError>
where
    R: AsyncRead + Unpin,
{
    let mut output = Vec::with_capacity(limit.min(8 * 1024));
    let mut chunk = [0u8; 8 * 1024];
    loop {
        let read = reader.read(&mut chunk).await?;
        if read == 0 {
            return Ok(output);
        }
        if output.len().saturating_add(read) > limit {
            let error = AppError::WslOutputLimitExceeded {
                stream: stream.to_string(),
                limit: u32::try_from(limit).unwrap_or(u32::MAX),
            };
            let _ = limit_tx.send(error.clone()).await;
            return Err(error);
        }
        output.extend_from_slice(&chunk[..read]);
    }
}

async fn terminate_child_and_tasks(
    child: &mut Child,
    writer: &mut JoinHandle<std::io::Result<()>>,
    stdout_reader: &mut JoinHandle<Result<Vec<u8>, AppError>>,
    stderr_reader: &mut JoinHandle<Result<Vec<u8>, AppError>>,
) {
    let _ = child.kill().await;
    let _ = child.wait().await;
    writer.abort();
    stdout_reader.abort();
    stderr_reader.abort();
}

async fn wait_for_cancellation(cancellation: Option<CancellationSignal>) {
    let Some(cancellation) = cancellation else {
        pending::<()>().await;
        return;
    };
    loop {
        if cancellation.is_cancelled() {
            return;
        }
        tokio::time::sleep(CANCELLATION_POLL_INTERVAL).await;
    }
}

async fn supervise_child(
    mut child: Child,
    stdin_payload: Vec<u8>,
    timeout_duration: Duration,
    stdout_limit: usize,
    stderr_limit: usize,
    cancellation: Option<CancellationSignal>,
) -> Result<WslCommandOutput, AppError> {
    let mut stdin = child.stdin.take().ok_or_else(|| AppError::Custom {
        message: "failed to open WSL stdin".to_string(),
    })?;
    let stdout = child.stdout.take().ok_or_else(|| AppError::Custom {
        message: "failed to open WSL stdout".to_string(),
    })?;
    let stderr = child.stderr.take().ok_or_else(|| AppError::Custom {
        message: "failed to open WSL stderr".to_string(),
    })?;
    let mut writer = tokio::spawn(async move {
        stdin.write_all(&stdin_payload).await?;
        stdin.shutdown().await
    });
    let (limit_tx, mut limit_rx) = mpsc::channel(2);
    let mut stdout_reader = tokio::spawn(read_bounded(
        stdout,
        "stdout",
        stdout_limit,
        limit_tx.clone(),
    ));
    let mut stderr_reader = tokio::spawn(read_bounded(stderr, "stderr", stderr_limit, limit_tx));
    let deadline = Instant::now() + timeout_duration;
    let deadline_sleep = tokio::time::sleep_until(deadline);
    tokio::pin!(deadline_sleep);
    let cancellation_wait = wait_for_cancellation(cancellation);
    tokio::pin!(cancellation_wait);
    let mut limit_channel_open = true;

    let status = loop {
        tokio::select! {
            status = child.wait() => match status {
                Ok(status) => break status,
                Err(error) => {
                    terminate_child_and_tasks(
                        &mut child,
                        &mut writer,
                        &mut stdout_reader,
                        &mut stderr_reader,
                    ).await;
                    return Err(error.into());
                }
            },
            limit_error = limit_rx.recv(), if limit_channel_open => {
                match limit_error {
                    Some(error) => {
                        terminate_child_and_tasks(
                            &mut child,
                            &mut writer,
                            &mut stdout_reader,
                            &mut stderr_reader,
                        ).await;
                        return Err(error);
                    }
                    None => limit_channel_open = false,
                }
            },
            _ = &mut deadline_sleep => {
                terminate_child_and_tasks(
                    &mut child,
                    &mut writer,
                    &mut stdout_reader,
                    &mut stderr_reader,
                ).await;
                return Err(AppError::WslCommandTimedOut);
            },
            _ = &mut cancellation_wait => {
                terminate_child_and_tasks(
                    &mut child,
                    &mut writer,
                    &mut stdout_reader,
                    &mut stderr_reader,
                ).await;
                return Err(AppError::MutationCancelled);
            },
        }
    };

    let drain_result = {
        let drain_tasks =
            async { tokio::join!(&mut writer, &mut stdout_reader, &mut stderr_reader) };
        tokio::pin!(drain_tasks);
        tokio::select! {
            joined = &mut drain_tasks => Ok(joined),
            Some(limit_error) = limit_rx.recv(), if limit_channel_open => Err(limit_error),
            _ = &mut deadline_sleep => Err(AppError::WslCommandTimedOut),
            _ = &mut cancellation_wait => Err(AppError::MutationCancelled),
        }
    };
    let (writer_result, stdout_result, stderr_result) = match drain_result {
        Ok(results) => results,
        Err(error) => {
            terminate_child_and_tasks(
                &mut child,
                &mut writer,
                &mut stdout_reader,
                &mut stderr_reader,
            )
            .await;
            return Err(error);
        }
    };
    let stdout = stdout_result.map_err(|error| AppError::Custom {
        message: error.to_string(),
    })??;
    let stderr = stderr_result.map_err(|error| AppError::Custom {
        message: error.to_string(),
    })??;
    let writer_result = writer_result.map_err(|error| AppError::Custom {
        message: error.to_string(),
    })?;
    if status.success() {
        writer_result?;
    }

    Ok(WslCommandOutput {
        stdout,
        stderr,
        exit_code: status.code(),
    })
}

fn interpret_wsl_command_output(
    session: &WslSession,
    mut output: WslCommandOutput,
) -> Result<WslCommandOutput, AppError> {
    if !output.stderr.starts_with(WSL_SHELL_STARTED_MARKER) {
        return Err(AppError::EnvironmentUnavailable {
            environment: EnvironmentRef::Wsl {
                distro_name: session.distro_name.clone(),
            },
            message: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        });
    }
    output.stderr.drain(..WSL_SHELL_STARTED_MARKER.len());
    if output.exit_code != Some(0) {
        return Err(AppError::WslCommandFailed {
            exit_code: output.exit_code,
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        });
    }
    Ok(output)
}

impl WslCommandRunner {
    #[cfg(target_os = "windows")]
    pub async fn run(request: WslCommandRequest) -> Result<WslCommandOutput, AppError> {
        let args = build_wsl_runner_exec_args(&request);
        let mut command = Command::new("wsl.exe");
        command
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        let child = command
            .spawn()
            .map_err(|error| AppError::EnvironmentUnavailable {
                environment: EnvironmentRef::Wsl {
                    distro_name: request.session.distro_name.clone(),
                },
                message: error.to_string(),
            })?;
        let session = request.session.clone();
        let output = supervise_child(
            child,
            request.stdin,
            request.timeout,
            request.stdout_limit,
            request.stderr_limit,
            request.cancellation,
        )
        .await?;
        interpret_wsl_command_output(&session, output)
    }

    #[cfg(not(target_os = "windows"))]
    pub async fn run(request: WslCommandRequest) -> Result<WslCommandOutput, AppError> {
        Err(AppError::EnvironmentUnavailable {
            environment: EnvironmentRef::Wsl {
                distro_name: request.session.distro_name,
            },
            message: "WSL is only available on Windows".to_string(),
        })
    }
}

pub async fn run_wsl_script(
    session: &WslSession,
    script: &'static str,
    positional_args: &[String],
    stdin_payload: Vec<u8>,
    timeout_duration: Duration,
) -> Result<Vec<u8>, AppError> {
    let output = WslCommandRunner::run(WslCommandRequest {
        session: session.clone(),
        script,
        args: positional_args.to_vec(),
        stdin: stdin_payload,
        timeout: timeout_duration,
        stdout_limit: DEFAULT_WSL_STDOUT_LIMIT,
        stderr_limit: DEFAULT_WSL_STDERR_LIMIT,
        cancellation: None,
    })
    .await?;
    Ok(output.stdout)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::process::Stdio;

    use tokio::process::{Child, Command};
    use tokio::time::{timeout, Duration};

    use super::{
        build_wsl_exec_args, decode_nul_records, interpret_wsl_command_output, supervise_child,
        WslCommandOutput, WslCommandRequest, WslCommandRunner, DEFAULT_WSL_STDERR_LIMIT,
        DEFAULT_WSL_STDOUT_LIMIT, WSL_SHELL_STARTED_MARKER,
    };
    use crate::core::mutation::CancellationSignal;
    use crate::environment::types::EnvironmentRef;
    use crate::environment::wsl::WslSession;
    use crate::error::AppError;

    fn test_session() -> WslSession {
        WslSession {
            distro_name: "Ubuntu".to_string(),
            user: "alice".to_string(),
            uid: 1000,
            home: "/home/alice".to_string(),
            xdg_state_home: None,
            config_home: "/home/alice/.config".to_string(),
            environment: BTreeMap::new(),
            git_available: true,
        }
    }

    #[cfg(unix)]
    fn spawn_shell(script: &str) -> (Child, u32) {
        let mut command = Command::new("/bin/sh");
        command
            .args(["-c", script])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        let child = command.spawn().expect("spawn test shell");
        let pid = child.id().expect("child pid");
        (child, pid)
    }

    #[cfg(unix)]
    async fn assert_process_stopped(pid: u32) {
        for _ in 0..20 {
            if !std::path::Path::new(&format!("/proc/{pid}")).exists() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("process {pid} was not reaped");
    }

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

    #[test]
    fn command_request_carries_bounded_defaults_and_cancellation() {
        let cancellation = CancellationSignal::default();
        let request = WslCommandRequest {
            session: test_session(),
            script: "printf ok",
            args: Vec::new(),
            stdin: Vec::new(),
            timeout: Duration::from_secs(1),
            stdout_limit: DEFAULT_WSL_STDOUT_LIMIT,
            stderr_limit: DEFAULT_WSL_STDERR_LIMIT,
            cancellation: Some(cancellation),
        };

        assert_eq!(request.stdout_limit, 16 * 1024 * 1024);
        assert_eq!(request.stderr_limit, 1024 * 1024);
        let _runner = WslCommandRunner;
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn supervisor_collects_stdout_stderr_and_exit_code() {
        let (child, _) = spawn_shell("printf stdout; printf stderr >&2; exit 7");
        let output = supervise_child(child, Vec::new(), Duration::from_secs(1), 1024, 1024, None)
            .await
            .expect("supervise child");

        assert_eq!(output.stdout, b"stdout");
        assert_eq!(output.stderr, b"stderr");
        assert_eq!(output.exit_code, Some(7));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn supervisor_rejects_stdout_and_stderr_over_their_limits() {
        for (script, expected_stream) in
            [("printf 12345", "stdout"), ("printf 12345 >&2", "stderr")]
        {
            let (child, _) = spawn_shell(script);
            let error = supervise_child(child, Vec::new(), Duration::from_secs(1), 4, 4, None)
                .await
                .expect_err("output must be bounded");

            assert!(matches!(
                error,
                AppError::WslOutputLimitExceeded { stream, limit }
                    if stream == expected_stream && limit == 4
            ));
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn supervisor_kills_and_reaps_a_timed_out_child_and_writer() {
        let (child, pid) = spawn_shell("exec sleep 5");
        let error = supervise_child(
            child,
            vec![b'x'; 32 * 1024 * 1024],
            Duration::from_millis(25),
            1024,
            1024,
            None,
        )
        .await
        .expect_err("command must time out");

        assert_eq!(error, AppError::WslCommandTimedOut);
        assert_process_stopped(pid).await;
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn supervisor_kills_and_reaps_a_cancelled_child() {
        let cancellation = CancellationSignal::default();
        let cancel_from_task = cancellation.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(25)).await;
            cancel_from_task.cancel();
        });
        let (child, pid) = spawn_shell("exec sleep 5");

        let error = supervise_child(
            child,
            Vec::new(),
            Duration::from_secs(1),
            1024,
            1024,
            Some(cancellation),
        )
        .await
        .expect_err("command must be cancelled");

        assert_eq!(error, AppError::MutationCancelled);
        assert_process_stopped(pid).await;
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn supervisor_does_not_deadlock_when_child_closes_stdin_early() {
        let (child, _) = spawn_shell("exec 0<&-; printf done");
        let result = timeout(
            Duration::from_secs(1),
            supervise_child(
                child,
                vec![b'x'; 32 * 1024 * 1024],
                Duration::from_secs(1),
                1024,
                1024,
                None,
            ),
        )
        .await;

        assert!(result.is_ok(), "stdin writer must be joined or aborted");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn supervisor_keeps_timeout_active_while_draining_process_pipes() {
        let (child, _) = spawn_shell("(sleep 0.3) & exit 0");

        let error = supervise_child(
            child,
            Vec::new(),
            Duration::from_millis(25),
            1024,
            1024,
            None,
        )
        .await
        .expect_err("open descendant pipes must not outlive the deadline");

        assert_eq!(error, AppError::WslCommandTimedOut);
    }

    #[test]
    fn shell_start_marker_distinguishes_session_and_business_failures() {
        let session = test_session();
        let environment = EnvironmentRef::Wsl {
            distro_name: "Ubuntu".to_string(),
        };
        let unavailable = interpret_wsl_command_output(
            &session,
            WslCommandOutput {
                stdout: Vec::new(),
                stderr: b"localized launcher failure".to_vec(),
                exit_code: Some(1),
            },
        )
        .expect_err("launcher failure");
        assert!(matches!(
            unavailable,
            AppError::EnvironmentUnavailable {
                environment: failed_environment,
                ..
            } if failed_environment == environment
        ));

        let mut marked_stderr = WSL_SHELL_STARTED_MARKER.to_vec();
        marked_stderr.extend_from_slice(b"permission denied");
        let business_failure = interpret_wsl_command_output(
            &session,
            WslCommandOutput {
                stdout: Vec::new(),
                stderr: marked_stderr,
                exit_code: Some(13),
            },
        )
        .expect_err("business failure");
        assert!(matches!(
            business_failure,
            AppError::WslCommandFailed {
                exit_code: Some(13),
                stderr,
            } if stderr == "permission denied"
        ));
    }

    #[test]
    fn shell_start_marker_is_removed_from_successful_stderr() {
        let mut stderr = WSL_SHELL_STARTED_MARKER.to_vec();
        stderr.extend_from_slice(b"warning");

        let output = interpret_wsl_command_output(
            &test_session(),
            WslCommandOutput {
                stdout: b"ok".to_vec(),
                stderr,
                exit_code: Some(0),
            },
        )
        .expect("successful command");

        assert_eq!(output.stdout, b"ok");
        assert_eq!(output.stderr, b"warning");
    }
}

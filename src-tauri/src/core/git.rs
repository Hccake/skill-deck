//! Git 克隆模块
//!
//! 功能：
//! - 克隆仓库到临时目录
//! - 支持分支/tag 指定
//! - 错误分类（认证、超时、权限、网络等）
//! - 支持进度事件发送到前端
//!
//! 与 CLI git.ts 行为一致

use crate::background_process::{
    attach_std_process_tree, configure_std_process_group, resume_std_process, std_command,
    terminate_std_process_tree,
};
use crate::core::mutation::CancellationSignal;
use crate::core::skill_paths::normalize_skill_folder_path;
use crate::error::AppError;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::Duration;
use tempfile::TempDir;

/// Git 克隆默认超时时间（秒）
pub const DEFAULT_CLONE_TIMEOUT_SECS: u64 = 120;
/// 允许的最小自定义超时时间（秒）
pub const MIN_CLONE_TIMEOUT_SECS: u64 = 30;
/// 允许的最大自定义超时时间（秒）
pub const MAX_CLONE_TIMEOUT_SECS: u64 = 3600;
const MAX_GIT_OUTPUT_CAPTURE_BYTES: usize = 256 * 1024;
const GIT_OUTPUT_DRAIN_TIMEOUT: Duration = Duration::from_secs(1);

/// 克隆进度阶段
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ClonePhase {
    /// 正在连接
    Connecting,
    /// 正在克隆
    Cloning,
    /// 克隆完成
    Done,
    /// 发生错误
    Error,
}

/// 克隆进度事件
#[derive(Debug, Clone, serde::Serialize)]
pub struct CloneProgress {
    /// 当前阶段
    pub phase: ClonePhase,
    /// 已用时间（秒）
    pub elapsed_secs: u64,
    /// 超时时间（秒）
    pub timeout_secs: u64,
    /// 可选的消息
    pub message: Option<String>,
}

/// 克隆结果，包含临时目录和仓库路径
pub struct CloneResult {
    /// RAII guard：drop 时自动清理临时目录。前缀下划线表示外部不应直接访问，
    /// 仅用于延长生命周期至 `repo_path` 使用结束。
    pub _temp_dir: TempDir,
    /// 仓库路径
    pub repo_path: PathBuf,
    /// clone 完成时捕获的 HEAD revision。transport 将其传递给后续 source workflow，
    /// 避免 consumer 再次依赖系统 Git 读取同一份 metadata。
    pub ref_revision: Option<String>,
}

pub(crate) fn clone_repo_with_progress_options<F>(
    url: &str,
    git_ref: Option<&str>,
    on_progress: F,
    cancellation: CancellationSignal,
    proxy: Option<&str>,
    timeout: Duration,
    display_timeout_secs: u64,
) -> Result<CloneResult, AppError>
where
    F: Fn(CloneProgress),
{
    let started_at = std::time::Instant::now();

    // 发送连接中状态
    on_progress(CloneProgress {
        phase: ClonePhase::Connecting,
        elapsed_secs: 0,
        timeout_secs: display_timeout_secs,
        message: None,
    });

    // 创建临时目录
    let temp_dir = TempDir::new().map_err(|e| AppError::GitCloneFailed {
        message: format!("Failed to create temp dir: {}", e),
    })?;

    let repo_path = temp_dir.path().to_path_buf();

    // 构建 git clone 命令，添加 --progress 以便 git 输出进度
    let mut cmd = std_command("git");
    apply_proxy_override(&mut cmd, proxy);
    cmd.arg("clone").arg("--depth").arg("1").arg("--progress");
    apply_clone_env(&mut cmd);

    // 如果指定了分支/tag
    if let Some(branch) = git_ref {
        cmd.arg("--branch").arg(branch);
    }

    cmd.arg(url).arg(&repo_path);

    // 执行克隆
    let result = execute_with_timeout_and_progress(&mut cmd, timeout, &on_progress, &cancellation);

    match result {
        Ok(output) => {
            if output.success {
                on_progress(CloneProgress {
                    phase: ClonePhase::Done,
                    elapsed_secs: output.elapsed_secs,
                    timeout_secs: display_timeout_secs,
                    message: None,
                });
                let ref_revision = compute_local_ref_revision(&repo_path);
                Ok(CloneResult {
                    _temp_dir: temp_dir,
                    repo_path,
                    ref_revision,
                })
            } else {
                // 分类错误
                let error = classify_git_command_error(&output, url, "clone");
                on_progress(CloneProgress {
                    phase: ClonePhase::Error,
                    elapsed_secs: output.elapsed_secs,
                    timeout_secs: display_timeout_secs,
                    message: Some(error.to_string()),
                });
                Err(error)
            }
        }
        Err(e) => {
            on_progress(build_error_progress(started_at, display_timeout_secs, &e));
            Err(e)
        }
    }
}

fn build_error_progress(
    started_at: std::time::Instant,
    timeout_secs: u64,
    err: &AppError,
) -> CloneProgress {
    CloneProgress {
        phase: ClonePhase::Error,
        elapsed_secs: started_at.elapsed().as_secs(),
        timeout_secs,
        message: Some(err.to_string()),
    }
}

fn normalize_clone_timeout_secs(value: u64) -> u64 {
    if value == 0 {
        DEFAULT_CLONE_TIMEOUT_SECS
    } else {
        value.clamp(MIN_CLONE_TIMEOUT_SECS, MAX_CLONE_TIMEOUT_SECS)
    }
}

pub(crate) fn resolve_clone_timeout_secs() -> u64 {
    crate::core::read_config()
        .map(|config| normalize_clone_timeout_secs(config.git_clone_timeout_secs.into()))
        .unwrap_or(DEFAULT_CLONE_TIMEOUT_SECS)
}

fn clone_env_pairs() -> [(&'static str, &'static str); 3] {
    [
        ("GIT_TERMINAL_PROMPT", "0"),
        ("GIT_LFS_SKIP_SMUDGE", "1"),
        ("LC_ALL", "C"),
    ]
}

fn apply_clone_env(cmd: &mut Command) {
    for (key, value) in clone_env_pairs() {
        cmd.env(key, value);
    }
}

fn apply_proxy_override(cmd: &mut Command, proxy: Option<&str>) {
    if let Some(proxy) = proxy {
        cmd.arg("-c").arg(format!("http.proxy={proxy}"));
    }
}

/// 命令执行结果
struct CommandOutput {
    success: bool,
    status_code: Option<i32>,
    stdout: String,
    stderr: String,
    elapsed_secs: u64,
}

struct RetainedOutput {
    bytes: Vec<u8>,
    truncated: bool,
}

fn spawn_output_reader<R>(mut reader: R) -> Receiver<std::io::Result<RetainedOutput>>
where
    R: std::io::Read + Send + 'static,
{
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let mut retained = Vec::with_capacity(MAX_GIT_OUTPUT_CAPTURE_BYTES);
        let mut truncated = false;
        let mut chunk = [0u8; 8 * 1024];
        let result = loop {
            let read = reader.read(&mut chunk)?;
            if read == 0 {
                break Ok(RetainedOutput {
                    bytes: retained,
                    truncated,
                });
            }
            retain_output_tail(&mut retained, &chunk[..read], &mut truncated);
        };
        let _ = sender.send(result);
        Ok::<(), std::io::Error>(())
    });
    receiver
}

fn retain_output_tail(retained: &mut Vec<u8>, chunk: &[u8], truncated: &mut bool) {
    if chunk.len() >= MAX_GIT_OUTPUT_CAPTURE_BYTES {
        *truncated = true;
        retained.clear();
        retained.extend_from_slice(&chunk[chunk.len() - MAX_GIT_OUTPUT_CAPTURE_BYTES..]);
        return;
    }

    let overflow = retained
        .len()
        .saturating_add(chunk.len())
        .saturating_sub(MAX_GIT_OUTPUT_CAPTURE_BYTES);
    if overflow > 0 {
        *truncated = true;
        retained.drain(..overflow);
    }
    retained.extend_from_slice(chunk);
}

fn receive_output_reader(
    reader: Receiver<std::io::Result<RetainedOutput>>,
    deadline: std::time::Instant,
    stream: &str,
    cancellation: &CancellationSignal,
) -> Result<String, AppError> {
    let retained = loop {
        if cancellation.is_cancelled() {
            return Err(AppError::MutationCancelled);
        }
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() {
            return Err(AppError::GitCloneFailed {
                message: format!("Git {stream} remained open after the Git process exited"),
            });
        }
        match reader.recv_timeout(remaining.min(Duration::from_millis(50))) {
            Ok(result) => break result,
            Err(RecvTimeoutError::Timeout) => continue,
            Err(RecvTimeoutError::Disconnected) => {
                return Err(AppError::GitCloneFailed {
                    message: format!("Git {stream} reader stopped unexpectedly"),
                });
            }
        }
    }
    .map_err(|error| AppError::GitCloneFailed {
        message: format!("Failed to read Git {stream}: {error}"),
    })?;
    Ok(decode_retained_output(retained))
}

fn decode_retained_output(mut retained: RetainedOutput) -> String {
    if retained.truncated {
        if let Some(newline) = retained.bytes.iter().position(|byte| *byte == b'\n') {
            retained.bytes.drain(..=newline);
        } else {
            retained.bytes.clear();
        }
    }
    String::from_utf8_lossy(&retained.bytes).into_owned()
}

/// 带超时和进度回调执行命令
fn execute_with_timeout_and_progress<F>(
    cmd: &mut Command,
    timeout: Duration,
    on_progress: &F,
    cancellation: &CancellationSignal,
) -> Result<CommandOutput, AppError>
where
    F: Fn(CloneProgress),
{
    use std::process::Stdio;

    // 设置 stderr 捕获
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    configure_std_process_group(cmd);

    if cancellation.is_cancelled() {
        return Err(AppError::MutationCancelled);
    }

    let start = std::time::Instant::now();
    let deadline = start + timeout;
    let mut child = cmd.spawn().map_err(|e| AppError::GitCloneFailed {
        message: format!("Failed to spawn git: {}", e),
    })?;
    let process_tree = attach_std_process_tree(&child).map_err(|error| {
        let _ = child.kill();
        let _ = child.wait();
        AppError::GitCloneFailed {
            message: format!("Failed to supervise Git process tree: {error}"),
        }
    })?;
    resume_std_process(&child).map_err(|error| {
        terminate_std_process_tree(&mut child, &process_tree);
        AppError::GitCloneFailed {
            message: format!("Failed to resume supervised Git process: {error}"),
        }
    })?;

    let stdout_reader =
        spawn_output_reader(
            child
                .stdout
                .take()
                .ok_or_else(|| AppError::GitCloneFailed {
                    message: "Failed to capture Git stdout".to_string(),
                })?,
        );
    let stderr_reader =
        spawn_output_reader(
            child
                .stderr
                .take()
                .ok_or_else(|| AppError::GitCloneFailed {
                    message: "Failed to capture Git stderr".to_string(),
                })?,
        );

    // 等待进程完成或超时；读取线程在等待期间持续排空管道。
    let mut last_progress_secs = 0u64;

    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                break Ok(status);
            }
            Ok(None) => {
                if cancellation.is_cancelled() {
                    terminate_std_process_tree(&mut child, &process_tree);
                    break Err(AppError::MutationCancelled);
                }

                // 进程仍在运行
                let elapsed = start.elapsed();
                let elapsed_secs = elapsed.as_secs();

                if elapsed > timeout {
                    // 超时，杀死进程
                    terminate_std_process_tree(&mut child, &process_tree);
                    break Err(AppError::GitTimeout {
                        timeout_secs: timeout.as_secs() as u32,
                    });
                }

                // 每秒发送一次进度更新
                if elapsed_secs > last_progress_secs {
                    last_progress_secs = elapsed_secs;
                    on_progress(CloneProgress {
                        phase: ClonePhase::Cloning,
                        elapsed_secs,
                        timeout_secs: timeout.as_secs(),
                        message: None,
                    });
                }

                // 短暂等待后重试
                std::thread::sleep(
                    Duration::from_millis(100)
                        .min(deadline.saturating_duration_since(std::time::Instant::now())),
                );
            }
            Err(e) => {
                terminate_std_process_tree(&mut child, &process_tree);
                break Err(AppError::GitCloneFailed {
                    message: format!("Failed to wait for git: {}", e),
                });
            }
        }
    };

    let status = status?;
    let drain_deadline = std::time::Instant::now() + GIT_OUTPUT_DRAIN_TIMEOUT;
    let stdout = receive_output_reader(stdout_reader, drain_deadline, "stdout", cancellation)
        .inspect_err(|_| terminate_std_process_tree(&mut child, &process_tree))?;
    let stderr = receive_output_reader(stderr_reader, drain_deadline, "stderr", cancellation)
        .inspect_err(|_| terminate_std_process_tree(&mut child, &process_tree))?;
    Ok(CommandOutput {
        success: status.success(),
        status_code: status.code(),
        stdout,
        stderr,
        elapsed_secs: start.elapsed().as_secs(),
    })
}

/// 在已 clone 的仓库里计算指定子目录的 tree SHA。
///
/// 等价于 GitHub Trees API 返回的 `sha` 字段——它们指向同一个 git tree object。
/// 让 update 流程可以直接复用 Git transport 已经下载的仓库，
/// 省掉一次 Trees API 调用，同时避免 API 偶发失败导致 lock 写入空 hash。
///
/// 失败返回 `None`：不在 git 仓库中、git 不可用、ref 不存在、目录不存在等。
pub fn compute_local_tree_sha(repo_path: &Path, folder_path: &str) -> Option<String> {
    let normalized = normalize_skill_folder_path(folder_path);
    let spec = if normalized.is_empty() {
        "HEAD^{tree}".to_string()
    } else {
        format!("HEAD:{}", normalized)
    };

    let mut cmd = std_command("git");
    cmd.arg("-C")
        .arg(repo_path)
        .arg("rev-parse")
        .arg("--verify")
        .arg(&spec);

    let output = cmd.output().ok()?;
    if !output.status.success() {
        return None;
    }
    normalize_git_object_id(String::from_utf8_lossy(&output.stdout).trim())
}

pub fn compute_local_ref_revision(repo_path: &Path) -> Option<String> {
    let output = std_command("git")
        .arg("-C")
        .arg(repo_path)
        .args(["rev-parse", "--verify", "HEAD"])
        .output()
        .ok()?;
    output.status.success().then(|| {
        String::from_utf8_lossy(&output.stdout)
            .trim()
            .to_ascii_lowercase()
    })
}

pub(crate) fn probe_remote_ref_revision_options(
    url: &str,
    git_ref: Option<&str>,
    cancellation: CancellationSignal,
    proxy: Option<&str>,
    timeout: Duration,
) -> Result<String, AppError> {
    let mut cmd = std_command("git");
    apply_proxy_override(&mut cmd, proxy);
    cmd.arg("ls-remote").arg("--exit-code").arg(url);
    match git_ref.filter(|value| !value.is_empty()) {
        Some(value) if value.starts_with("refs/") => {
            cmd.arg(value);
            if value.starts_with("refs/tags/") {
                cmd.arg(format!("{value}^{{}}"));
            }
        }
        Some(value) => {
            cmd.arg(format!("refs/heads/{value}"))
                .arg(format!("refs/tags/{value}^{{}}"))
                .arg(format!("refs/tags/{value}"));
        }
        None => {
            cmd.arg("HEAD");
        }
    }
    apply_clone_env(&mut cmd);

    let output = execute_with_timeout_and_progress(&mut cmd, timeout, &|_| {}, &cancellation)?;
    if !output.success {
        if output.status_code == Some(2) {
            return Err(AppError::GitRefNotFound {
                ref_name: git_ref.unwrap_or("HEAD").to_string(),
            });
        }
        return Err(classify_git_command_error(&output, url, "probe ref"));
    }

    let lines = output.stdout;
    let revision = lines
        .lines()
        .find(|line| line.ends_with("^{}"))
        .or_else(|| lines.lines().next())
        .and_then(|line| line.split_whitespace().next())
        .and_then(normalize_git_object_id)
        .ok_or_else(|| AppError::GitRefNotFound {
            ref_name: git_ref.unwrap_or("HEAD").to_string(),
        })?;
    Ok(revision)
}

fn normalize_git_object_id(value: &str) -> Option<String> {
    (matches!(value.len(), 40 | 64) && value.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .then(|| value.to_ascii_lowercase())
}

/// 分类 Git 错误（与 CLI 行为一致）
fn classify_git_error(stderr: &str, url: &str) -> AppError {
    let stderr_lower = stderr.to_lowercase();

    // 认证错误
    if stderr_lower.contains("authentication failed")
        || stderr_lower.contains("could not read username")
        || stderr_lower.contains("permission denied")
    {
        return AppError::GitAuthFailed {
            message: format!(
                "Authentication failed for {url}.\n\
                 - For private repos, ensure you have access\n\
                 - For SSH: Check your keys with 'ssh -T git@github.com'\n\
                 - For HTTPS: Run 'gh auth login' or configure git credentials"
            ),
        };
    }

    if stderr_lower.contains("could not resolve host")
        || stderr_lower.contains("could not resolve proxy")
        || stderr_lower.contains("unable to resolve")
        || stderr_lower.contains("name or service not known")
        || stderr_lower.contains("connection timed out")
        || stderr_lower.contains("connection refused")
        || stderr_lower.contains("failed to connect")
        || stderr_lower.contains("couldn't connect")
        || stderr_lower.contains("network is unreachable")
        || stderr_lower.contains("no route to host")
        || stderr_lower.contains("ssl certificate")
        || stderr_lower.contains("certificate verify failed")
        || stderr_lower.contains("ssl_error")
    {
        return AppError::GitNetworkError {
            message: format!("Git network request failed for {url}: {stderr}"),
        };
    }

    // 分支/tag 不存在（必须在 "repository not found" 检查之前）
    if stderr_lower.contains("remote branch")
        || stderr_lower.contains("did not match any")
        || stderr_lower.contains("not a valid ref")
        || (stderr_lower.contains("not found") && stderr_lower.contains("branch"))
    {
        return AppError::GitRefNotFound {
            ref_name: stderr.to_string(),
        };
    }

    // 仓库不存在
    if stderr_lower.contains("repository not found") || stderr_lower.contains("does not exist") {
        return AppError::GitRepoNotFound {
            repo: url.to_string(),
        };
    }

    // 通用错误
    AppError::GitCloneFailed {
        message: format!("Failed to clone {url}: {stderr}"),
    }
}

fn classify_git_command_error(output: &CommandOutput, url: &str, operation: &str) -> AppError {
    classify_git_failure(&output.stderr, url, operation, output.status_code)
}

pub(crate) fn classify_git_failure(
    stderr: &str,
    url: &str,
    operation: &str,
    status_code: Option<i32>,
) -> AppError {
    let classified = classify_git_error(stderr, url);
    if !matches!(classified, AppError::GitCloneFailed { .. }) {
        return classified;
    }

    let status = status_code
        .map(|code| code.to_string())
        .unwrap_or_else(|| "unknown".to_string());
    AppError::GitCloneFailed {
        message: format!(
            "Git {operation} failed with exit status {status}: {}",
            stderr.trim()
        ),
    }
}

#[cfg(test)]
#[allow(
    clippy::disallowed_methods,
    reason = "Git 测试夹具需要直接调用真实 Git 或启动可控子进程"
)]
mod tests {
    use super::*;

    const SUBPROCESS_OUTPUT_FIXTURE_ENV: &str = "SKILL_DECK_GIT_OUTPUT_FIXTURE";
    const SUBPROCESS_PID_FILE_ENV: &str = "SKILL_DECK_GIT_PROCESS_PID_FILE";
    const SUBPROCESS_DESCENDANT_PID_FILE_ENV: &str = "SKILL_DECK_GIT_DESCENDANT_PID_FILE";

    #[test]
    fn proxy_override_is_process_scoped_and_does_not_change_git_config() {
        let temp = tempfile::tempdir().expect("tempdir");
        let global_config = temp.path().join("gitconfig");
        let initial_proxy = "http://existing.example:8080";
        let injected_proxy = "http://runtime.example:7890";
        let status = Command::new("git")
            .args([
                "config",
                "--file",
                global_config.to_str().expect("config path"),
                "http.proxy",
                initial_proxy,
            ])
            .status()
            .expect("write fixture Git config");
        assert!(status.success());

        let mut command = Command::new("git");
        apply_proxy_override(&mut command, Some(injected_proxy));
        let args = command
            .get_args()
            .map(|argument| argument.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert_eq!(
            args,
            vec!["-c".to_string(), format!("http.proxy={injected_proxy}")]
        );
        assert!(args
            .iter()
            .all(|argument| !argument.starts_with("https.proxy=")));

        let persisted = Command::new("git")
            .args([
                "config",
                "--file",
                global_config.to_str().expect("config path"),
                "--get",
                "http.proxy",
            ])
            .output()
            .expect("read fixture Git config");
        assert!(persisted.status.success());
        assert_eq!(
            String::from_utf8_lossy(&persisted.stdout).trim(),
            initial_proxy
        );
    }

    fn subprocess_output_fixture_command(stream: &str) -> Command {
        let mut command = Command::new(std::env::current_exe().expect("current test executable"));
        command
            .args([
                "--exact",
                "core::git::tests::subprocess_output_fixture",
                "--nocapture",
            ])
            .env(SUBPROCESS_OUTPUT_FIXTURE_ENV, stream);
        command
    }

    fn cancel_after_pid_file_created(
        cancellation: CancellationSignal,
        pid_file: PathBuf,
    ) -> std::thread::JoinHandle<()> {
        std::thread::spawn(move || {
            let deadline = std::time::Instant::now() + Duration::from_secs(2);
            while !pid_file.is_file() {
                assert!(
                    std::time::Instant::now() < deadline,
                    "subprocess did not publish its PID"
                );
                std::thread::sleep(Duration::from_millis(10));
            }
            cancellation.cancel();
        })
    }

    #[cfg(unix)]
    fn assert_process_exited(pid: u32) {
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        loop {
            let result = unsafe { libc::kill(pid as i32, 0) };
            if result == -1 && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH) {
                return;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "descendant process {pid} remained alive"
            );
            std::thread::sleep(Duration::from_millis(25));
        }
    }

    #[cfg(target_os = "windows")]
    fn assert_process_exited(pid: u32) {
        use windows_sys::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0};
        use windows_sys::Win32::System::Threading::{OpenProcess, WaitForSingleObject};

        const SYNCHRONIZE_ACCESS: u32 = 0x0010_0000;

        let process = unsafe { OpenProcess(SYNCHRONIZE_ACCESS, 0, pid) };
        if process.is_null() {
            return;
        }
        let wait_result = unsafe { WaitForSingleObject(process, 2_000) };
        unsafe {
            CloseHandle(process);
        }
        assert_eq!(
            wait_result, WAIT_OBJECT_0,
            "descendant process {pid} remained alive"
        );
    }

    #[cfg(not(any(unix, target_os = "windows")))]
    fn assert_process_exited(_pid: u32) {}

    #[test]
    fn subprocess_output_fixture() {
        let Ok(stream) = std::env::var(SUBPROCESS_OUTPUT_FIXTURE_ENV) else {
            return;
        };
        if let Ok(pid_file) = std::env::var(SUBPROCESS_PID_FILE_ENV) {
            std::fs::write(pid_file, std::process::id().to_string()).expect("write process pid");
        }
        if stream == "sleep" {
            std::thread::sleep(Duration::from_secs(10));
            return;
        }
        if stream == "sleep-short" {
            std::thread::sleep(Duration::from_secs(4));
            return;
        }
        if stream == "spawn-descendant" {
            let descendant = subprocess_output_fixture_command("sleep-short")
                .spawn()
                .expect("spawn descendant holding output pipes");
            if let Ok(pid_file) = std::env::var(SUBPROCESS_DESCENDANT_PID_FILE_ENV) {
                std::fs::write(pid_file, descendant.id().to_string())
                    .expect("write descendant pid");
            }
            std::mem::forget(descendant);
            return;
        }
        if stream == "sensitive-stderr" {
            let secret = "TOP_SECRET_TOKEN".repeat(24 * 1024);
            std::io::Write::write_all(
                &mut std::io::stderr(),
                format!("Authorization: Bearer {secret}\nvisible diagnostic\n").as_bytes(),
            )
            .expect("write sensitive stderr fixture");
            return;
        }
        let output = format!("{}\n", "x".repeat(1023)).repeat(8 * 1024);
        match stream.as_str() {
            "stderr" => std::io::Write::write_all(&mut std::io::stderr(), output.as_bytes())
                .expect("write stderr fixture"),
            "stdout" => std::io::Write::write_all(&mut std::io::stdout(), output.as_bytes())
                .expect("write stdout fixture"),
            other => panic!("unknown subprocess output fixture: {other}"),
        }
    }

    #[test]
    fn test_classify_auth_error() {
        let err = classify_git_error("Authentication failed for ...", "https://example.com");
        assert!(matches!(err, AppError::GitAuthFailed { .. }));
    }

    #[test]
    fn test_classify_not_found_error() {
        let err = classify_git_error("Repository not found", "https://example.com");
        assert!(matches!(err, AppError::GitRepoNotFound { .. }));
    }

    #[test]
    fn test_classify_ref_not_found() {
        let err = classify_git_error("Remote branch 'foo' not found", "https://example.com");
        assert!(matches!(err, AppError::GitRefNotFound { .. }));
    }

    #[test]
    fn test_classify_generic_error() {
        let output = CommandOutput {
            success: false,
            status_code: Some(128),
            stdout: String::new(),
            stderr: "Some random error".to_string(),
            elapsed_secs: 0,
        };
        let err = classify_git_command_error(&output, "https://example.com", "clone");
        assert!(matches!(
            err,
            AppError::GitCloneFailed { message }
                if message.contains("clone")
                    && message.contains("exit status 128")
                    && message.contains("Some random error")
        ));
    }

    #[test]
    fn git_network_errors_use_one_generic_message_without_root_cause_inference() {
        for diagnostic in [
            "Could not resolve host: github.com",
            "Could not resolve proxy: localhost",
            "Connection timed out",
            "SSL certificate problem",
        ] {
            let err = classify_git_error(diagnostic, "https://github.com/acme/repo.git");
            assert!(matches!(
                err,
                AppError::GitNetworkError { message }
                    if message.contains("Git network request failed")
                        && message.contains(diagnostic)
                        && !message.contains("DNS")
                        && !message.contains("VPN")
                        && !message.contains("intercept")
            ));
        }
    }

    #[test]
    fn test_classify_connection_error() {
        let err = classify_git_error("Connection timed out", "https://github.com");
        assert!(matches!(err, AppError::GitNetworkError { .. }));

        let err = classify_git_error(
            "fatal: unable to access 'https://github.com/acme/repo.git/': Failed to connect to 127.0.0.1 port 7890 after 0 ms: Couldn't connect to server",
            "https://github.com/acme/repo.git",
        );
        assert!(matches!(err, AppError::GitNetworkError { .. }));
    }

    #[test]
    fn test_classify_ssl_error() {
        let err = classify_git_error("SSL certificate problem", "https://github.com");
        assert!(matches!(err, AppError::GitNetworkError { .. }));
    }

    #[test]
    fn git_error_messages_preserve_bounded_local_diagnostics() {
        let url =
            "https://alice:secret-token@github.com/acme/private.git?access_token=query-secret";
        let stderr = format!(
            "fatal: unexpected failure while accessing '{url}'\n\
             Authorization: Bearer header-secret"
        );

        let rendered = classify_git_error(&stderr, url).to_string();

        assert!(rendered.contains(url));
        assert!(rendered.contains(&stderr));
    }

    #[test]
    fn git_commands_use_stable_non_interactive_environment() {
        let envs = clone_env_pairs();
        assert!(envs.contains(&("GIT_TERMINAL_PROMPT", "0")));
        assert!(envs.contains(&("GIT_LFS_SKIP_SMUDGE", "1")));
        assert!(envs.contains(&("LC_ALL", "C")));
    }

    #[test]
    fn test_normalize_clone_timeout_uses_default_when_zero() {
        assert_eq!(normalize_clone_timeout_secs(0), 120);
    }

    #[test]
    fn test_normalize_clone_timeout_clamps_small_values() {
        assert_eq!(normalize_clone_timeout_secs(5), 30);
    }

    #[test]
    fn test_normalize_clone_timeout_clamps_large_values() {
        assert_eq!(normalize_clone_timeout_secs(7200), 3600);
    }

    #[test]
    fn test_git_timeout_error_includes_timeout_secs() {
        assert!(matches!(
            AppError::GitTimeout { timeout_secs: 300 },
            AppError::GitTimeout { timeout_secs: 300 }
        ));
    }

    /// 创建一个最小 git 仓库,在 `subdir/SKILL.md` 写入内容并 commit。
    /// 返回 (tempdir, repo_path, expected_subdir_tree_sha)。
    fn make_repo_with_skill(content: &str) -> Option<(TempDir, PathBuf, String)> {
        let tmp = TempDir::new().ok()?;
        let repo = tmp.path().to_path_buf();
        let run = |args: &[&str]| -> Option<()> {
            let status = Command::new("git")
                .arg("-C")
                .arg(&repo)
                .args(args)
                .status()
                .ok()?;
            if status.success() {
                Some(())
            } else {
                None
            }
        };
        run(&["init", "-q"])?;
        run(&["config", "user.email", "test@example.com"])?;
        run(&["config", "user.name", "Test"])?;
        run(&["config", "commit.gpgsign", "false"])?;
        std::fs::create_dir_all(repo.join("skills/demo")).ok()?;
        std::fs::write(repo.join("skills/demo/SKILL.md"), content).ok()?;
        run(&["add", "-A"])?;
        run(&["commit", "-q", "-m", "init"])?;
        let output = Command::new("git")
            .arg("-C")
            .arg(&repo)
            .args(["rev-parse", "HEAD:skills/demo"])
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        let sha = String::from_utf8_lossy(&output.stdout).trim().to_string();
        Some((tmp, repo, sha))
    }

    #[test]
    fn test_compute_local_tree_sha_matches_git_rev_parse() {
        let Some((_tmp, repo, expected)) = make_repo_with_skill("hello") else {
            eprintln!("git not available, skipping");
            return;
        };
        let actual = compute_local_tree_sha(&repo, "skills/demo/SKILL.md");
        assert_eq!(actual.as_deref(), Some(expected.as_str()));
    }

    #[test]
    fn test_compute_local_tree_sha_strips_skill_md_suffix() {
        // 同一目录,带与不带 SKILL.md 应得到同一个 tree SHA
        let Some((_tmp, repo, expected)) = make_repo_with_skill("hello") else {
            return;
        };
        let with_suffix = compute_local_tree_sha(&repo, "skills/demo/SKILL.md");
        let without_suffix = compute_local_tree_sha(&repo, "skills/demo");
        assert_eq!(with_suffix.as_deref(), Some(expected.as_str()));
        assert_eq!(without_suffix.as_deref(), Some(expected.as_str()));
    }

    #[test]
    fn test_compute_local_tree_sha_strips_lowercase_skill_md_suffix() {
        let Some((_tmp, repo, expected)) = make_repo_with_skill("hello") else {
            return;
        };
        let with_suffix = compute_local_tree_sha(&repo, "skills/demo/skill.md");
        assert_eq!(with_suffix.as_deref(), Some(expected.as_str()));
    }

    #[test]
    fn test_compute_local_tree_sha_returns_none_for_missing_path() {
        let Some((_tmp, repo, _)) = make_repo_with_skill("hello") else {
            return;
        };
        assert_eq!(compute_local_tree_sha(&repo, "skills/nope"), None);
    }

    #[test]
    fn test_compute_local_tree_sha_returns_none_outside_git_repo() {
        let tmp = TempDir::new().unwrap();
        assert_eq!(compute_local_tree_sha(tmp.path(), "skills/demo"), None);
    }

    #[test]
    fn git_object_id_validation_accepts_sha1_and_sha256_only() {
        assert_eq!(
            normalize_git_object_id(&"A".repeat(40)),
            Some("a".repeat(40))
        );
        assert_eq!(
            normalize_git_object_id(&"b".repeat(64)),
            Some("b".repeat(64))
        );
        assert_eq!(normalize_git_object_id(&"c".repeat(63)), None);
        assert_eq!(
            normalize_git_object_id(&format!("{}z", "d".repeat(39))),
            None
        );
    }

    #[test]
    fn test_build_error_progress_uses_real_elapsed_not_timeout() {
        let started = std::time::Instant::now();
        let err = AppError::GitCloneFailed {
            message: "boom".into(),
        };
        let progress = build_error_progress(started, 120, &err);
        assert!(progress.elapsed_secs < 120);
        assert!(matches!(progress.phase, ClonePhase::Error));
        assert_eq!(progress.timeout_secs, 120);
        assert_eq!(progress.message, Some(err.to_string()));
    }

    #[test]
    fn active_child_is_killed_and_waited_when_clone_is_cancelled() {
        let temp_dir = TempDir::new().expect("child pid temp dir");
        let pid_file = temp_dir.path().join("child.pid");
        let cancellation = CancellationSignal::default();
        let canceller = cancel_after_pid_file_created(cancellation.clone(), pid_file.clone());
        let mut command = subprocess_output_fixture_command("sleep");
        command.env(SUBPROCESS_PID_FILE_ENV, &pid_file);
        let started = std::time::Instant::now();

        let result = execute_with_timeout_and_progress(
            &mut command,
            Duration::from_secs(10),
            &|_| {},
            &cancellation,
        );
        canceller.join().unwrap();

        assert!(matches!(result, Err(AppError::MutationCancelled)));
        assert!(started.elapsed() < Duration::from_secs(2));
        let pid = std::fs::read_to_string(pid_file)
            .expect("read child pid")
            .parse::<u32>()
            .expect("parse child pid");
        assert_process_exited(pid);
    }

    #[test]
    fn active_child_is_killed_and_waited_when_git_times_out() {
        let temp_dir = TempDir::new().expect("child pid temp dir");
        let pid_file = temp_dir.path().join("child.pid");
        let mut command = subprocess_output_fixture_command("sleep");
        command.env(SUBPROCESS_PID_FILE_ENV, &pid_file);
        let started = std::time::Instant::now();

        let result = execute_with_timeout_and_progress(
            &mut command,
            Duration::from_secs(1),
            &|_| {},
            &CancellationSignal::default(),
        );

        assert!(matches!(
            result,
            Err(AppError::GitTimeout { timeout_secs: 1 })
        ));
        assert!(started.elapsed() < Duration::from_secs(3));
        let pid = std::fs::read_to_string(pid_file)
            .expect("read child pid")
            .parse::<u32>()
            .expect("parse child pid");
        assert_process_exited(pid);
    }

    #[test]
    fn inherited_output_pipe_failure_is_not_misreported_as_git_timeout() {
        let temp_dir = TempDir::new().expect("descendant pid temp dir");
        let pid_file = temp_dir.path().join("descendant.pid");
        let mut command = subprocess_output_fixture_command("spawn-descendant");
        command.env(SUBPROCESS_DESCENDANT_PID_FILE_ENV, &pid_file);
        let started = std::time::Instant::now();

        let result = execute_with_timeout_and_progress(
            &mut command,
            Duration::from_secs(1),
            &|_| {},
            &CancellationSignal::default(),
        );

        assert!(matches!(result, Err(AppError::GitCloneFailed { .. })));
        assert!(started.elapsed() < Duration::from_secs(3));
        let descendant_pid = std::fs::read_to_string(pid_file)
            .expect("read descendant pid")
            .parse::<u32>()
            .expect("parse descendant pid");
        assert_process_exited(descendant_pid);
    }

    #[test]
    fn cancellation_interrupts_inherited_output_pipe_drain() {
        let temp_dir = TempDir::new().expect("descendant pid temp dir");
        let pid_file = temp_dir.path().join("descendant.pid");
        let cancellation = CancellationSignal::default();
        let canceller = cancel_after_pid_file_created(cancellation.clone(), pid_file.clone());
        let mut command = subprocess_output_fixture_command("spawn-descendant");
        command.env(SUBPROCESS_DESCENDANT_PID_FILE_ENV, &pid_file);
        let started = std::time::Instant::now();

        let result = execute_with_timeout_and_progress(
            &mut command,
            Duration::from_secs(10),
            &|_| {},
            &cancellation,
        );
        canceller.join().unwrap();

        assert!(matches!(result, Err(AppError::MutationCancelled)));
        assert!(started.elapsed() < Duration::from_secs(2));
        let descendant_pid = std::fs::read_to_string(pid_file)
            .expect("read descendant pid")
            .parse::<u32>()
            .expect("parse descendant pid");
        assert_process_exited(descendant_pid);
    }

    #[test]
    fn truncated_sensitive_line_is_not_retained() {
        let mut command = subprocess_output_fixture_command("sensitive-stderr");
        let output = execute_with_timeout_and_progress(
            &mut command,
            Duration::from_secs(2),
            &|_| {},
            &CancellationSignal::default(),
        )
        .expect("sensitive output fixture");

        assert!(!output.stderr.contains("TOP_SECRET_TOKEN"));
        assert!(output.stderr.contains("visible diagnostic"));
    }

    #[test]
    fn large_output_streams_are_drained_while_subprocess_is_running() {
        for stream in ["stdout", "stderr"] {
            let mut command = subprocess_output_fixture_command(stream);
            let output = execute_with_timeout_and_progress(
                &mut command,
                Duration::from_secs(2),
                &|_| {},
                &CancellationSignal::default(),
            )
            .unwrap_or_else(|error| panic!("{stream} subprocess failed: {error:?}"));

            let retained = if stream == "stdout" {
                output.stdout
            } else {
                output.stderr
            };
            assert!(!retained.is_empty());
            assert!(retained.len() <= MAX_GIT_OUTPUT_CAPTURE_BYTES);
        }
    }
}

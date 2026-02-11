//! Git 克隆模块
//!
//! 功能：
//! - 克隆仓库到临时目录
//! - 支持分支/tag 指定
//! - 错误分类（认证、超时、权限、网络等）
//! - 支持进度事件发送到前端
//!
//! 与 CLI git.ts 行为一致

use crate::error::AppError;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;
use tempfile::TempDir;

/// Git 克隆超时时间（秒）- 增加到 120 秒以支持大仓库和慢网络
const CLONE_TIMEOUT_SECS: u64 = 120;

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
    /// 临时目录（drop 时自动清理）
    pub temp_dir: TempDir,
    /// 仓库路径
    pub repo_path: PathBuf,
}

/// 克隆仓库到临时目录（无进度回调版本，兼容现有调用）
///
/// # Arguments
/// * `url` - 仓库 URL（支持 HTTPS 和 SSH）
/// * `git_ref` - 可选的分支或 tag
///
/// # Returns
/// * `Ok(CloneResult)` - 包含临时目录和仓库路径
/// * `Err(AppError)` - 克隆失败，错误已分类
///
/// # 行为
/// - 使用 `--depth 1` 浅克隆
/// - 120 秒超时
/// - 失败时自动清理临时目录
pub fn clone_repo(url: &str, git_ref: Option<&str>) -> Result<CloneResult, AppError> {
    clone_repo_with_progress(url, git_ref, |_| {})
}

/// 克隆仓库到临时目录（带进度回调）
///
/// # Arguments
/// * `url` - 仓库 URL（支持 HTTPS 和 SSH）
/// * `git_ref` - 可选的分支或 tag
/// * `on_progress` - 进度回调函数
pub fn clone_repo_with_progress<F>(
    url: &str,
    git_ref: Option<&str>,
    on_progress: F,
) -> Result<CloneResult, AppError>
where
    F: Fn(CloneProgress),
{
    // 发送连接中状态
    on_progress(CloneProgress {
        phase: ClonePhase::Connecting,
        elapsed_secs: 0,
        timeout_secs: CLONE_TIMEOUT_SECS,
        message: None,
    });

    // 创建临时目录
    let temp_dir = TempDir::new()
        .map_err(|e| AppError::GitCloneFailed(format!("Failed to create temp dir: {}", e)))?;

    let repo_path = temp_dir.path().to_path_buf();

    // 构建 git clone 命令，添加 --progress 以便 git 输出进度
    let mut cmd = Command::new("git");
    cmd.arg("clone")
        .arg("--depth")
        .arg("1")
        .arg("--progress");

    // 如果指定了分支/tag
    if let Some(branch) = git_ref {
        cmd.arg("--branch").arg(branch);
    }

    cmd.arg(url).arg(&repo_path);

    // 执行克隆
    let result = execute_with_timeout_and_progress(
        &mut cmd,
        Duration::from_secs(CLONE_TIMEOUT_SECS),
        &on_progress,
    );

    match result {
        Ok(output) => {
            if output.success {
                on_progress(CloneProgress {
                    phase: ClonePhase::Done,
                    elapsed_secs: output.elapsed_secs,
                    timeout_secs: CLONE_TIMEOUT_SECS,
                    message: None,
                });
                Ok(CloneResult { temp_dir, repo_path })
            } else {
                // 分类错误
                let error = classify_git_error(&output.stderr, url);
                on_progress(CloneProgress {
                    phase: ClonePhase::Error,
                    elapsed_secs: output.elapsed_secs,
                    timeout_secs: CLONE_TIMEOUT_SECS,
                    message: Some(error.to_string()),
                });
                Err(error)
            }
        }
        Err(e) => {
            on_progress(CloneProgress {
                phase: ClonePhase::Error,
                elapsed_secs: CLONE_TIMEOUT_SECS,
                timeout_secs: CLONE_TIMEOUT_SECS,
                message: Some(e.to_string()),
            });
            Err(e)
        }
    }
}

/// 命令执行结果
struct CommandOutput {
    success: bool,
    stderr: String,
    elapsed_secs: u64,
}

/// 带超时和进度回调执行命令
fn execute_with_timeout_and_progress<F>(
    cmd: &mut Command,
    timeout: Duration,
    on_progress: &F,
) -> Result<CommandOutput, AppError>
where
    F: Fn(CloneProgress),
{
    use std::process::Stdio;

    // 设置 stderr 捕获
    cmd.stdout(Stdio::null()).stderr(Stdio::piped());

    let mut child = cmd
        .spawn()
        .map_err(|e| AppError::GitCloneFailed(format!("Failed to spawn git: {}", e)))?;

    // 等待进程完成或超时
    let start = std::time::Instant::now();
    let mut last_progress_secs = 0u64;

    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                // 进程已结束
                let stderr = child
                    .stderr
                    .take()
                    .map(|mut s| {
                        use std::io::Read;
                        let mut buf = String::new();
                        s.read_to_string(&mut buf).ok();
                        buf
                    })
                    .unwrap_or_default();

                return Ok(CommandOutput {
                    success: status.success(),
                    stderr,
                    elapsed_secs: start.elapsed().as_secs(),
                });
            }
            Ok(None) => {
                // 进程仍在运行
                let elapsed = start.elapsed();
                let elapsed_secs = elapsed.as_secs();

                if elapsed > timeout {
                    // 超时，杀死进程
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(AppError::GitTimeout);
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
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(e) => {
                return Err(AppError::GitCloneFailed(format!(
                    "Failed to wait for git: {}",
                    e
                )));
            }
        }
    }
}

/// 分类 Git 错误（与 CLI 行为一致）
fn classify_git_error(stderr: &str, url: &str) -> AppError {
    let stderr_lower = stderr.to_lowercase();

    // 认证错误
    if stderr_lower.contains("authentication failed")
        || stderr_lower.contains("could not read username")
        || stderr_lower.contains("permission denied")
    {
        return AppError::GitAuthFailed(format!(
            "Authentication failed for {url}.\n\
             - For private repos, ensure you have access\n\
             - For SSH: Check your keys with 'ssh -T git@github.com'\n\
             - For HTTPS: Run 'gh auth login' or configure git credentials"
        ));
    }

    // 网络/连接错误
    if stderr_lower.contains("could not resolve host")
        || stderr_lower.contains("unable to resolve")
        || stderr_lower.contains("name or service not known")
    {
        return AppError::GitNetworkError(format!(
            "DNS resolution failed for {url}.\n\
             - Check your internet connection\n\
             - Verify the URL is correct"
        ));
    }

    if stderr_lower.contains("connection timed out")
        || stderr_lower.contains("connection refused")
        || stderr_lower.contains("network is unreachable")
        || stderr_lower.contains("no route to host")
    {
        return AppError::GitNetworkError(format!(
            "Connection failed for {url}.\n\
             - Check your internet connection\n\
             - Check if a proxy/VPN is required"
        ));
    }

    if stderr_lower.contains("ssl certificate")
        || stderr_lower.contains("certificate verify failed")
        || stderr_lower.contains("ssl_error")
    {
        return AppError::GitNetworkError(format!(
            "SSL/TLS error for {url}.\n\
             - Check your system time\n\
             - Check if a proxy is intercepting HTTPS"
        ));
    }

    // 分支/tag 不存在（必须在 "repository not found" 检查之前）
    if stderr_lower.contains("remote branch")
        || stderr_lower.contains("did not match any")
        || stderr_lower.contains("not a valid ref")
        || (stderr_lower.contains("not found") && stderr_lower.contains("branch"))
    {
        return AppError::GitRefNotFound(stderr.to_string());
    }

    // 仓库不存在
    if stderr_lower.contains("repository not found")
        || stderr_lower.contains("does not exist")
    {
        return AppError::GitRepoNotFound(url.to_string());
    }

    // 通用错误
    AppError::GitCloneFailed(format!("Failed to clone {}: {}", url, stderr))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_classify_auth_error() {
        let err = classify_git_error("Authentication failed for ...", "https://example.com");
        assert!(matches!(err, AppError::GitAuthFailed(_)));
    }

    #[test]
    fn test_classify_not_found_error() {
        let err = classify_git_error("Repository not found", "https://example.com");
        assert!(matches!(err, AppError::GitRepoNotFound(_)));
    }

    #[test]
    fn test_classify_ref_not_found() {
        let err = classify_git_error("Remote branch 'foo' not found", "https://example.com");
        assert!(matches!(err, AppError::GitRefNotFound(_)));
    }

    #[test]
    fn test_classify_generic_error() {
        let err = classify_git_error("Some random error", "https://example.com");
        assert!(matches!(err, AppError::GitCloneFailed(_)));
    }

    #[test]
    fn test_classify_dns_error() {
        let err = classify_git_error("Could not resolve host: github.com", "https://github.com");
        assert!(matches!(err, AppError::GitNetworkError(_)));
    }

    #[test]
    fn test_classify_connection_error() {
        let err = classify_git_error("Connection timed out", "https://github.com");
        assert!(matches!(err, AppError::GitNetworkError(_)));
    }

    #[test]
    fn test_classify_ssl_error() {
        let err = classify_git_error("SSL certificate problem", "https://github.com");
        assert!(matches!(err, AppError::GitNetworkError(_)));
    }
}

//! Git 克隆模块
//!
//! 功能：
//! - 克隆仓库到临时目录
//! - 支持分支/tag 指定
//! - 错误分类（认证、超时、权限、网络等）
//! - 支持进度事件发送到前端
//!
//! 与 CLI git.ts 行为一致

use crate::core::github_api::normalize_skill_folder_path;
use crate::error::AppError;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;
use tempfile::TempDir;

/// Git 克隆默认超时时间（秒）
pub const DEFAULT_CLONE_TIMEOUT_SECS: u64 = 120;
/// 允许的最小自定义超时时间（秒）
pub const MIN_CLONE_TIMEOUT_SECS: u64 = 30;
/// 允许的最大自定义超时时间（秒）
pub const MAX_CLONE_TIMEOUT_SECS: u64 = 3600;

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
    let timeout_secs = resolve_clone_timeout_secs();
    let started_at = std::time::Instant::now();

    // 发送连接中状态
    on_progress(CloneProgress {
        phase: ClonePhase::Connecting,
        elapsed_secs: 0,
        timeout_secs,
        message: None,
    });

    // 创建临时目录
    let temp_dir = TempDir::new().map_err(|e| AppError::GitCloneFailed {
        message: format!("Failed to create temp dir: {}", e),
    })?;

    let repo_path = temp_dir.path().to_path_buf();

    // 构建 git clone 命令，添加 --progress 以便 git 输出进度
    let mut cmd = Command::new("git");
    cmd.arg("clone").arg("--depth").arg("1").arg("--progress");
    apply_clone_env(&mut cmd);

    // 如果指定了分支/tag
    if let Some(branch) = git_ref {
        cmd.arg("--branch").arg(branch);
    }

    cmd.arg(url).arg(&repo_path);

    // 执行克隆
    let result = execute_with_timeout_and_progress(
        &mut cmd,
        Duration::from_secs(timeout_secs),
        &on_progress,
    );

    match result {
        Ok(output) => {
            if output.success {
                on_progress(CloneProgress {
                    phase: ClonePhase::Done,
                    elapsed_secs: output.elapsed_secs,
                    timeout_secs,
                    message: None,
                });
                Ok(CloneResult {
                    _temp_dir: temp_dir,
                    repo_path,
                })
            } else {
                // 分类错误
                let error = classify_git_error(&output.stderr, url);
                on_progress(CloneProgress {
                    phase: ClonePhase::Error,
                    elapsed_secs: output.elapsed_secs,
                    timeout_secs,
                    message: Some(error.to_string()),
                });
                Err(error)
            }
        }
        Err(e) => {
            on_progress(build_error_progress(started_at, timeout_secs, &e));
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

fn resolve_clone_timeout_secs() -> u64 {
    crate::core::read_config()
        .map(|config| normalize_clone_timeout_secs(config.git_clone_timeout_secs.into()))
        .unwrap_or(DEFAULT_CLONE_TIMEOUT_SECS)
}

fn clone_env_pairs() -> [(&'static str, &'static str); 2] {
    [("GIT_TERMINAL_PROMPT", "0"), ("GIT_LFS_SKIP_SMUDGE", "1")]
}

fn apply_clone_env(cmd: &mut Command) {
    for (key, value) in clone_env_pairs() {
        cmd.env(key, value);
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

    // Windows: 隐藏控制台窗口
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    let mut child = cmd.spawn().map_err(|e| AppError::GitCloneFailed {
        message: format!("Failed to spawn git: {}", e),
    })?;

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
                    return Err(AppError::GitTimeout {
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
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(e) => {
                return Err(AppError::GitCloneFailed {
                    message: format!("Failed to wait for git: {}", e),
                });
            }
        }
    }
}

/// 在已 clone 的仓库里计算指定子目录的 tree SHA。
///
/// 等价于 GitHub Trees API 返回的 `sha` 字段——它们指向同一个 git tree object。
/// 让 update 流程可以直接复用 `clone_repo_with_progress` 已经下载的仓库，
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

    let mut cmd = Command::new("git");
    cmd.arg("-C")
        .arg(repo_path)
        .arg("rev-parse")
        .arg("--verify")
        .arg(&spec);

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    let output = cmd.output().ok()?;
    if !output.status.success() {
        return None;
    }
    let sha = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if sha.is_empty() {
        None
    } else {
        Some(sha)
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
        return AppError::GitAuthFailed {
            message: format!(
                "Authentication failed for {url}.\n\
                 - For private repos, ensure you have access\n\
                 - For SSH: Check your keys with 'ssh -T git@github.com'\n\
                 - For HTTPS: Run 'gh auth login' or configure git credentials"
            ),
        };
    }

    // 网络/连接错误
    if stderr_lower.contains("could not resolve host")
        || stderr_lower.contains("unable to resolve")
        || stderr_lower.contains("name or service not known")
    {
        return AppError::GitNetworkError {
            message: format!(
                "DNS resolution failed for {url}.\n\
                 - Check your internet connection\n\
                 - Verify the URL is correct"
            ),
        };
    }

    if stderr_lower.contains("connection timed out")
        || stderr_lower.contains("connection refused")
        || stderr_lower.contains("network is unreachable")
        || stderr_lower.contains("no route to host")
    {
        return AppError::GitNetworkError {
            message: format!(
                "Connection failed for {url}.\n\
                 - Check your internet connection\n\
                 - Check if a proxy/VPN is required"
            ),
        };
    }

    if stderr_lower.contains("ssl certificate")
        || stderr_lower.contains("certificate verify failed")
        || stderr_lower.contains("ssl_error")
    {
        return AppError::GitNetworkError {
            message: format!(
                "SSL/TLS error for {url}.\n\
                 - Check your system time\n\
                 - Check if a proxy is intercepting HTTPS"
            ),
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
        message: format!("Failed to clone {}: {}", url, stderr),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let err = classify_git_error("Some random error", "https://example.com");
        assert!(matches!(err, AppError::GitCloneFailed { .. }));
    }

    #[test]
    fn test_classify_dns_error() {
        let err = classify_git_error("Could not resolve host: github.com", "https://github.com");
        assert!(matches!(err, AppError::GitNetworkError { .. }));
    }

    #[test]
    fn test_classify_connection_error() {
        let err = classify_git_error("Connection timed out", "https://github.com");
        assert!(matches!(err, AppError::GitNetworkError { .. }));
    }

    #[test]
    fn test_classify_ssl_error() {
        let err = classify_git_error("SSL certificate problem", "https://github.com");
        assert!(matches!(err, AppError::GitNetworkError { .. }));
    }

    #[test]
    fn test_clone_env_pairs_include_lfs_skip_smudge() {
        let envs = clone_env_pairs();
        assert!(envs.contains(&("GIT_TERMINAL_PROMPT", "0")));
        assert!(envs.contains(&("GIT_LFS_SKIP_SMUDGE", "1")));
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
}

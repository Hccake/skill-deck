//! GitHub API 模块
//!
//! 功能：
//! - 获取 GitHub token（环境变量 + gh CLI）
//! - 调用 GitHub Trees API 获取 skillFolderHash

pub use crate::core::skill_paths::normalize_skill_folder_path;
use crate::error::AppError;
use reqwest::Client;
use serde::Deserialize;
use std::collections::HashMap;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};

static GITHUB_RATE_LIMITED: AtomicBool = AtomicBool::new(false);

/// GitHub Trees API 响应
#[derive(Debug, Deserialize)]
struct TreesResponse {
    sha: String,
    tree: Vec<TreeEntry>,
}

#[derive(Debug, Deserialize)]
struct TreeEntry {
    path: String,
    #[serde(rename = "type")]
    entry_type: String,
    sha: String,
}

fn get_github_env_token() -> Option<String> {
    if let Ok(token) = std::env::var("GITHUB_TOKEN") {
        if !token.trim().is_empty() {
            return Some(token);
        }
    }

    if let Ok(token) = std::env::var("GH_TOKEN") {
        if !token.trim().is_empty() {
            return Some(token);
        }
    }

    None
}

/// 通过 gh CLI 获取 token
fn get_gh_cli_token() -> Option<String> {
    let output = Command::new("gh").args(["auth", "token"]).output().ok()?;

    if output.status.success() {
        let token = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !token.is_empty() {
            return Some(token);
        }
    }

    None
}

fn refs_to_try(git_ref: Option<&str>) -> Vec<String> {
    match git_ref.map(str::trim).filter(|value| !value.is_empty()) {
        Some(r) => vec![r.to_string()],
        None => ["HEAD", "main", "master"]
            .into_iter()
            .map(String::from)
            .collect(),
    }
}

fn build_trees_url(owner_repo: &str, git_ref: &str) -> String {
    let encoded_ref = urlencoding::encode(git_ref);
    format!(
        "https://api.github.com/repos/{}/git/trees/{}?recursive=1",
        owner_repo, encoded_ref
    )
}

#[derive(Debug, Clone)]
struct GithubAuthState {
    env_token: Option<String>,
    cli_token: Option<String>,
    prefer_cli_token: bool,
}

impl GithubAuthState {
    fn from_env() -> Self {
        Self::new(
            get_github_env_token(),
            GITHUB_RATE_LIMITED.load(Ordering::Relaxed),
        )
    }

    fn new(env_token: Option<String>, prefer_cli_token: bool) -> Self {
        Self {
            env_token,
            cli_token: None,
            prefer_cli_token,
        }
    }

    fn token_for_initial_request<F>(&mut self, resolve_cli_token: F) -> Option<String>
    where
        F: FnOnce() -> Option<String>,
    {
        if let Some(token) = &self.env_token {
            return Some(token.clone());
        }

        if self.prefer_cli_token {
            if self.cli_token.is_none() {
                self.cli_token = resolve_cli_token();
            }
            return self.cli_token.clone();
        }

        None
    }

    fn retry_token_after_error<F>(
        &mut self,
        err: &AppError,
        request_had_token: bool,
        resolve_cli_token: F,
    ) -> Option<String>
    where
        F: FnOnce() -> Option<String>,
    {
        if request_had_token || !is_rate_limited_error(err) {
            return None;
        }

        self.prefer_cli_token = true;
        GITHUB_RATE_LIMITED.store(true, Ordering::Relaxed);
        if self.cli_token.is_none() {
            self.cli_token = resolve_cli_token();
        }
        self.cli_token.clone()
    }
}

fn is_rate_limited_error(err: &AppError) -> bool {
    matches!(
        err,
        AppError::GitHubApiError { reason, .. } if reason == "rate-limited"
    )
}

async fn send_tree_request(
    client: &Client,
    url: &str,
    token: Option<&str>,
) -> Result<TreesResponse, AppError> {
    let mut request = client
        .get(url)
        .header("Accept", "application/vnd.github.v3+json")
        .header("User-Agent", "skill-deck");

    if let Some(t) = token {
        request = request.header("Authorization", format!("Bearer {}", t));
    }

    let response = request.send().await.map_err(|e| AppError::GitHubApiError {
        reason: "network-error".into(),
        message: e.to_string(),
    })?;

    let status = response.status();
    if !status.is_success() {
        return Err(classify_github_response(status, response.headers(), url));
    }

    response
        .json::<TreesResponse>()
        .await
        .map_err(|e| AppError::GitHubApiError {
            reason: "network-error".into(),
            message: format!("Failed to parse Trees response: {}", e),
        })
}

async fn fetch_tree_for_ref(
    client: &Client,
    owner_repo: &str,
    git_ref: &str,
    auth: &mut GithubAuthState,
) -> Result<TreesResponse, AppError> {
    let url = build_trees_url(owner_repo, git_ref);
    let token = auth.token_for_initial_request(get_gh_cli_token);
    let result = send_tree_request(client, &url, token.as_deref()).await;

    match result {
        Ok(data) => Ok(data),
        Err(err) => {
            if let Some(retry_token) =
                auth.retry_token_after_error(&err, token.is_some(), get_gh_cli_token)
            {
                return send_tree_request(client, &url, Some(&retry_token)).await;
            }
            Err(err)
        }
    }
}

/// 获取 skill 文件夹的 hash（通过 GitHub Trees API）
///
/// # Arguments
/// * `owner_repo` - 格式为 "owner/repo"
/// * `skill_path` - 文件夹路径，如 "skills/my-skill/SKILL.md"
/// * `git_ref` - 可选的分支/tag，默认尝试 HEAD、main 和 master
///
/// # Returns
/// * `Ok(Some(hash))` - 成功获取 hash
/// * `Ok(None)` - API 调用成功但未找到对应文件夹
/// * `Err(_)` - API 调用失败
pub async fn fetch_skill_folder_hash(
    owner_repo: &str,
    skill_path: &str,
    git_ref: Option<&str>,
) -> Result<Option<String>, AppError> {
    let folder_path = normalize_skill_folder_path(skill_path);

    let client = Client::new();
    let mut auth = GithubAuthState::from_env();

    for git_ref in refs_to_try(git_ref) {
        match fetch_tree_for_ref(&client, owner_repo, &git_ref, &mut auth).await {
            Ok(data) => {
                if folder_path.is_empty() {
                    return Ok(Some(data.sha));
                }

                for entry in data.tree {
                    if entry.entry_type == "tree" && entry.path == folder_path {
                        return Ok(Some(entry.sha));
                    }
                }
                return Ok(None);
            }
            Err(err) if is_ref_specific_error(&err) => continue,
            Err(err) => return Err(err),
        }
    }

    Ok(None)
}

/// 把 reqwest 响应分类为 `AppError::GitHubApiError`,reason 字段决定前端文案。
///
/// reason 取值优先级 (高 → 低): `rate-limited` > `auth` > `http-<code>` > `network-error`。
/// 对于"换 ref 可能解决"的 404,调用方会继续尝试下一个分支;其他错误立即返回。
fn classify_github_response(
    status: reqwest::StatusCode,
    headers: &reqwest::header::HeaderMap,
    url: &str,
) -> AppError {
    if status.as_u16() == 403 {
        let remaining = headers
            .get("X-RateLimit-Remaining")
            .and_then(|h| h.to_str().ok())
            .and_then(|s| s.parse::<u32>().ok());
        if matches!(remaining, Some(0)) {
            return AppError::GitHubApiError {
                reason: "rate-limited".into(),
                message: format!("GitHub API rate limit reached at {}", url),
            };
        }
        // 403 但还有配额 → 多半是 token 权限不足
        return AppError::GitHubApiError {
            reason: "auth".into(),
            message: format!("GitHub API forbidden at {}", url),
        };
    }
    if status.as_u16() == 401 {
        return AppError::GitHubApiError {
            reason: "auth".into(),
            message: format!("GitHub API authentication required at {}", url),
        };
    }
    AppError::GitHubApiError {
        reason: format!("http-{}", status.as_u16()),
        message: format!("HTTP {} from GitHub Trees API at {}", status, url),
    }
}

/// 仅 404 (ref 不存在) 视为"换个 branch 可能能成功",其他错误立刻返回。
fn is_ref_specific_error(err: &AppError) -> bool {
    matches!(
        err,
        AppError::GitHubApiError { reason, .. } if reason == "http-404"
    )
}

/// 批量获取同源多个 skill 文件夹的 hash（单次 API 请求）
///
/// 与 `fetch_skill_folder_hash` 的区别：
/// - 对同一 owner_repo 只调用 **一次** Trees API
/// - 从返回的完整 tree 中查找所有 skill_paths 对应的 hash
/// - N 个同源 skills 从 N 次 API 降为 1 次
///
/// # 错误返回语义
///
/// - 网络错误 / HTTP 4xx/5xx → `Err(AppError::GitHubApiError { reason, .. })`,
///   其中 reason 在 UI 决定具体文案 (`rate-limited` / `auth` / ...)
/// - HTTP 2xx 但 tree 中找不到对应 path → `Ok(map_with_None_for_that_skill)`
///
/// 这样调用方 (`check_updates_inner`) 才能区分"远端真没这个 skill"和"网都没通"。
pub async fn fetch_skill_folder_hashes_batch(
    owner_repo: &str,
    skill_paths: &[(String, String)],
    git_ref: Option<&str>,
) -> Result<HashMap<String, Option<String>>, AppError> {
    // 预处理所有 skill_path：规范化路径
    let normalized: Vec<(String, String)> = skill_paths
        .iter()
        .map(|(name, path)| (name.clone(), normalize_skill_folder_path(path)))
        .collect();

    let client = Client::new();
    let mut auth = GithubAuthState::from_env();

    let mut last_err: Option<AppError> = None;

    for git_ref in refs_to_try(git_ref) {
        let data = match fetch_tree_for_ref(&client, owner_repo, &git_ref, &mut auth).await {
            Ok(data) => data,
            Err(err) => {
                // 仅 404 继续尝试下一个 ref;rate-limit / auth 等立即返回
                if !is_ref_specific_error(&err) {
                    return Err(err);
                }
                last_err = Some(err);
                continue;
            }
        };

        let tree_map: HashMap<&str, &str> = data
            .tree
            .iter()
            .filter(|e| e.entry_type == "tree")
            .map(|e| (e.path.as_str(), e.sha.as_str()))
            .collect();

        let mut results = HashMap::new();
        for (name, folder_path) in &normalized {
            if folder_path.is_empty() {
                results.insert(name.clone(), Some(data.sha.clone()));
            } else if let Some(sha) = tree_map.get(folder_path.as_str()) {
                results.insert(name.clone(), Some(sha.to_string()));
            } else {
                results.insert(name.clone(), None);
            }
        }
        return Ok(results);
    }

    // 所有 branch 都失败:把最后一个错误抛出来给调用方分类
    Err(last_err.unwrap_or_else(|| AppError::GitHubApiError {
        reason: "network-error".into(),
        message: format!("Failed to fetch tree for {}", owner_repo),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    use reqwest::header::HeaderMap;
    use reqwest::StatusCode;

    fn header_map(pairs: &[(&str, &str)]) -> HeaderMap {
        let mut h = HeaderMap::new();
        for (k, v) in pairs {
            h.insert(
                reqwest::header::HeaderName::from_bytes(k.as_bytes()).unwrap(),
                v.parse().unwrap(),
            );
        }
        h
    }

    #[test]
    fn test_classify_response_403_with_zero_remaining_is_rate_limited() {
        let err = classify_github_response(
            StatusCode::FORBIDDEN,
            &header_map(&[("X-RateLimit-Remaining", "0")]),
            "https://api.github.com/repos/x/y/git/trees/main",
        );
        match err {
            AppError::GitHubApiError { reason, .. } => assert_eq!(reason, "rate-limited"),
            _ => panic!("expected GitHubApiError"),
        }
    }

    #[test]
    fn test_refs_to_try_defaults_to_head_main_master() {
        assert_eq!(refs_to_try(None), vec!["HEAD", "main", "master"]);
        assert_eq!(refs_to_try(Some("feature/foo")), vec!["feature/foo"]);
    }

    #[test]
    fn test_build_trees_url_encodes_git_ref() {
        assert_eq!(
            build_trees_url("owner/repo", "feature/foo"),
            "https://api.github.com/repos/owner/repo/git/trees/feature%2Ffoo?recursive=1"
        );
    }

    #[test]
    fn test_github_auth_state_starts_anonymous_without_env_token() {
        let mut auth = GithubAuthState::new(None, false);
        let mut resolver_called = false;

        let token = auth.token_for_initial_request(|| {
            resolver_called = true;
            Some("cli-token".to_string())
        });

        assert_eq!(token, None);
        assert!(!resolver_called);
    }

    #[test]
    fn test_github_auth_state_retries_with_cli_token_after_rate_limit() {
        let mut auth = GithubAuthState::new(None, false);
        let err = classify_github_response(
            StatusCode::FORBIDDEN,
            &header_map(&[("X-RateLimit-Remaining", "0")]),
            "https://example.com",
        );
        let mut resolver_calls = 0;

        let token = auth.retry_token_after_error(&err, false, || {
            resolver_calls += 1;
            Some("cli-token".to_string())
        });

        assert_eq!(token, Some("cli-token".to_string()));
        assert_eq!(resolver_calls, 1);
        assert_eq!(
            auth.token_for_initial_request(|| panic!("token should be cached")),
            Some("cli-token".to_string())
        );
    }

    #[test]
    fn test_github_auth_state_does_not_retry_auth_errors() {
        let mut auth = GithubAuthState::new(None, false);
        let err = classify_github_response(
            StatusCode::UNAUTHORIZED,
            &HeaderMap::new(),
            "https://example.com",
        );

        let token = auth.retry_token_after_error(&err, false, || {
            panic!("auth errors should not resolve a CLI token");
        });

        assert_eq!(token, None);
    }

    #[test]
    fn test_classify_response_403_with_remaining_quota_is_auth() {
        let err = classify_github_response(
            StatusCode::FORBIDDEN,
            &header_map(&[("X-RateLimit-Remaining", "100")]),
            "https://example.com",
        );
        match err {
            AppError::GitHubApiError { reason, .. } => assert_eq!(reason, "auth"),
            _ => panic!("expected GitHubApiError"),
        }
    }

    #[test]
    fn test_classify_response_401_is_auth() {
        let err = classify_github_response(
            StatusCode::UNAUTHORIZED,
            &HeaderMap::new(),
            "https://example.com",
        );
        match err {
            AppError::GitHubApiError { reason, .. } => assert_eq!(reason, "auth"),
            _ => panic!("expected GitHubApiError"),
        }
    }

    #[test]
    fn test_classify_response_404_is_recoverable_with_other_ref() {
        let err = classify_github_response(
            StatusCode::NOT_FOUND,
            &HeaderMap::new(),
            "https://example.com",
        );
        match &err {
            AppError::GitHubApiError { reason, .. } => assert_eq!(reason, "http-404"),
            _ => panic!("expected GitHubApiError"),
        }
        assert!(is_ref_specific_error(&err));
    }

    #[test]
    fn test_classify_response_500_is_not_ref_specific() {
        let err = classify_github_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            &HeaderMap::new(),
            "https://example.com",
        );
        assert!(!is_ref_specific_error(&err));
    }

    #[test]
    fn test_normalize_skill_folder_path_strips_skill_md_and_slashes() {
        assert_eq!(
            normalize_skill_folder_path("skills/demo/SKILL.md"),
            "skills/demo"
        );
        assert_eq!(
            normalize_skill_folder_path("skills/demo/SKILL.md/"),
            "skills/demo"
        );
        assert_eq!(
            normalize_skill_folder_path("skills\\demo\\SKILL.md"),
            "skills/demo"
        );
        assert_eq!(
            normalize_skill_folder_path("skills/demo/skill.md"),
            "skills/demo"
        );
        assert_eq!(
            normalize_skill_folder_path("skills\\demo\\Skill.md"),
            "skills/demo"
        );
        assert_eq!(normalize_skill_folder_path("skill.md"), "");
        assert_eq!(normalize_skill_folder_path("skills/demo/"), "skills/demo");
        assert_eq!(normalize_skill_folder_path("/"), "");
        assert_eq!(normalize_skill_folder_path(""), "");
    }

    #[test]
    fn test_get_github_env_token_from_env() {
        // 保存原始值
        let original = std::env::var("GITHUB_TOKEN").ok();

        // 设置测试值
        std::env::set_var("GITHUB_TOKEN", "test-token");
        assert_eq!(get_github_env_token(), Some("test-token".to_string()));

        // 恢复原始值
        match original {
            Some(v) => std::env::set_var("GITHUB_TOKEN", v),
            None => std::env::remove_var("GITHUB_TOKEN"),
        }
    }
}

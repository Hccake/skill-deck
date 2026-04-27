//! GitHub API 模块
//!
//! 功能：
//! - 获取 GitHub token（环境变量 + gh CLI）
//! - 调用 GitHub Trees API 获取 skillFolderHash

use crate::error::AppError;
use reqwest::Client;
use serde::Deserialize;
use std::collections::HashMap;
use std::process::Command;

/// 规范化 skill 路径：去除 `SKILL.md` 后缀、统一斜杠、去除首尾斜杠。
///
/// Lock 中 `skill_path` 通常是 `skills/foo/SKILL.md`，但在向 GitHub Trees API 或
/// 本地 `git rev-parse` 提供子目录时只能用 `skills/foo`。该函数同时被
/// `fetch_skill_folder_hash` / `fetch_skill_folder_hashes_batch` /
/// `compute_local_tree_sha` 复用，避免三处实现走偏。
pub fn normalize_skill_folder_path(skill_path: &str) -> String {
    // 统一斜杠 + 先 trim 尾随 /,这样 `skills/demo/SKILL.md/` 也能正确剥掉 SKILL.md
    let trimmed = skill_path.replace('\\', "/");
    let trimmed = trimmed.trim_end_matches('/');
    if trimmed == "SKILL.md" {
        return String::new();
    }
    if let Some(stripped) = trimmed.strip_suffix("/SKILL.md") {
        return stripped.trim_end_matches('/').to_string();
    }
    trimmed.to_string()
}

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

/// 获取 GitHub token
///
/// 优先级：
/// 1. GITHUB_TOKEN 环境变量
/// 2. GH_TOKEN 环境变量
/// 3. gh auth token 命令
pub fn get_github_token() -> Option<String> {
    // 1. 检查 GITHUB_TOKEN
    if let Ok(token) = std::env::var("GITHUB_TOKEN") {
        if !token.is_empty() {
            return Some(token);
        }
    }

    // 2. 检查 GH_TOKEN
    if let Ok(token) = std::env::var("GH_TOKEN") {
        if !token.is_empty() {
            return Some(token);
        }
    }

    // 3. 尝试 gh auth token
    get_gh_cli_token()
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

/// 获取 skill 文件夹的 hash（通过 GitHub Trees API）
///
/// # Arguments
/// * `owner_repo` - 格式为 "owner/repo"
/// * `skill_path` - 文件夹路径，如 "skills/my-skill/SKILL.md"
/// * `git_ref` - 可选的分支/tag，默认尝试 main 和 master
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

    let token = get_github_token();
    let client = Client::new();

    // 如果指定了 git_ref，只尝试该分支；否则尝试 main 和 master
    let branches: Vec<&str> = match git_ref {
        Some(r) => vec![r],
        None => vec!["main", "master"],
    };

    for branch in branches {
        let url = format!(
            "https://api.github.com/repos/{}/git/trees/{}?recursive=1",
            owner_repo, branch
        );

        let mut request = client
            .get(&url)
            .header("Accept", "application/vnd.github.v3+json")
            .header("User-Agent", "skill-deck");

        if let Some(ref t) = token {
            request = request.header("Authorization", format!("Bearer {}", t));
        }

        let response = request.send().await;

        match response {
            Ok(resp) if resp.status().is_success() => {
                if let Ok(data) = resp.json::<TreesResponse>().await {
                    // 如果 folder_path 为空，返回根 tree SHA
                    if folder_path.is_empty() {
                        return Ok(Some(data.sha));
                    }

                    // 查找对应的 tree entry
                    for entry in data.tree {
                        if entry.entry_type == "tree" && entry.path == folder_path {
                            return Ok(Some(entry.sha));
                        }
                    }
                }
            }
            _ => continue,
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

    let token = get_github_token();
    let client = Client::new();

    let branches: Vec<&str> = match git_ref {
        Some(r) => vec![r],
        None => vec!["main", "master"],
    };

    let mut last_err: Option<AppError> = None;

    for branch in branches {
        let url = format!(
            "https://api.github.com/repos/{}/git/trees/{}?recursive=1",
            owner_repo, branch
        );

        let mut request = client
            .get(&url)
            .header("Accept", "application/vnd.github.v3+json")
            .header("User-Agent", "skill-deck");

        if let Some(ref t) = token {
            request = request.header("Authorization", format!("Bearer {}", t));
        }

        let response = match request.send().await {
            Ok(resp) => resp,
            Err(e) => {
                last_err = Some(AppError::GitHubApiError {
                    reason: "network-error".into(),
                    message: e.to_string(),
                });
                continue;
            }
        };

        let status = response.status();
        if !status.is_success() {
            let err = classify_github_response(status, response.headers(), &url);
            // 仅 404 继续尝试下一个 branch;rate-limit / auth 等立即返回
            if !is_ref_specific_error(&err) {
                return Err(err);
            }
            last_err = Some(err);
            continue;
        }

        // HTTP 2xx
        let data = match response.json::<TreesResponse>().await {
            Ok(d) => d,
            Err(e) => {
                last_err = Some(AppError::GitHubApiError {
                    reason: "network-error".into(),
                    message: format!("Failed to parse Trees response: {}", e),
                });
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
        assert_eq!(normalize_skill_folder_path("skills/demo/"), "skills/demo");
        assert_eq!(normalize_skill_folder_path("/"), "");
        assert_eq!(normalize_skill_folder_path(""), "");
    }

    #[test]
    fn test_get_github_token_from_env() {
        // 保存原始值
        let original = std::env::var("GITHUB_TOKEN").ok();

        // 设置测试值
        std::env::set_var("GITHUB_TOKEN", "test-token");
        assert_eq!(get_github_token(), Some("test-token".to_string()));

        // 恢复原始值
        match original {
            Some(v) => std::env::set_var("GITHUB_TOKEN", v),
            None => std::env::remove_var("GITHUB_TOKEN"),
        }
    }
}

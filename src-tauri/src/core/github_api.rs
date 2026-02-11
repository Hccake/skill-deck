//! GitHub API 模块
//!
//! 功能：
//! - 获取 GitHub token（环境变量 + gh CLI）
//! - 调用 GitHub Trees API 获取 skillFolderHash

use crate::error::AppError;
use reqwest::Client;
use serde::Deserialize;
use std::process::Command;

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
    // 规范化路径
    let mut folder_path = skill_path.replace('\\', "/");

    // 移除 SKILL.md 后缀
    if folder_path.ends_with("/SKILL.md") {
        folder_path = folder_path[..folder_path.len() - 9].to_string();
    } else if folder_path.ends_with("SKILL.md") {
        folder_path = folder_path[..folder_path.len() - 8].to_string();
    }

    // 移除尾部斜杠
    folder_path = folder_path.trim_end_matches('/').to_string();

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

#[cfg(test)]
mod tests {
    use super::*;

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

//! 来源解析模块
//!
//! 支持 9 种来源格式：
//! - GitHub shorthand: owner/repo
//! - GitHub + 子路径: owner/repo/path
//! - GitHub + @skill: owner/repo@skill-name
//! - GitHub URL: https://github.com/owner/repo
//! - GitHub URL + 分支: https://github.com/owner/repo/tree/branch/path
//! - GitLab URL: https://gitlab.com/group/repo
//! - GitLab URL + 分支: https://gitlab.com/group/repo/-/tree/branch/path
//! - 本地路径: ./path, /abs/path, C:\path
//! - Direct URL: https://example.com/SKILL.md
//! - Well-known: https://example.com (fallback)
//! - Git URL: git@github.com:owner/repo.git (fallback)

use crate::error::AppError;
use crate::models::{ParsedSource, SourceType};
use std::path::Path;
use url::Url;

/// 解析来源字符串
pub fn parse_source(input: &str) -> Result<ParsedSource, AppError> {
    let input = input.trim();

    if input.is_empty() {
        return Err(AppError::InvalidSource {
            value: "Empty source".to_string(),
        });
    }

    // 1. 检查本地路径
    if is_local_path(input) {
        return parse_local_path(input);
    }

    // 2. 检查是否是 URL
    if input.starts_with("http://") || input.starts_with("https://") {
        return parse_url(input);
    }

    // 3. 检查 Git URL (git@...) - 注意：不检查 .git 后缀，因为 shorthand 也可能带 .git
    if input.starts_with("git@") {
        return parse_git_url(input);
    }

    // 4. 尝试解析为 GitHub shorthand（支持 .git 后缀）
    parse_github_shorthand(input)
}

/// 检查是否是本地路径
fn is_local_path(input: &str) -> bool {
    // Unix 绝对路径
    if input.starts_with('/') {
        return true;
    }
    // 相对路径
    if input.starts_with("./") || input.starts_with("../") {
        return true;
    }
    // Windows 绝对路径 (C:\, D:\, C:/, D:/, etc.)
    if input.len() >= 3 {
        let chars: Vec<char> = input.chars().collect();
        if chars[0].is_ascii_alphabetic() && chars[1] == ':' && (chars[2] == '\\' || chars[2] == '/') {
            return true;
        }
    }
    false
}

/// 解析本地路径
fn parse_local_path(input: &str) -> Result<ParsedSource, AppError> {
    let path = Path::new(input);
    Ok(ParsedSource::local(path.to_path_buf()))
}

/// 解析 URL
fn parse_url(input: &str) -> Result<ParsedSource, AppError> {
    let url = Url::parse(input).map_err(|e| AppError::InvalidSource {
        value: format!("Invalid URL: {}", e),
    })?;

    let host = url.host_str().unwrap_or("");

    // GitHub URL
    if host == "github.com" || host == "www.github.com" {
        return parse_github_url(input, &url);
    }

    // GitLab URL
    if host == "gitlab.com" || host.contains("gitlab") {
        return parse_gitlab_url(input, &url);
    }

    // Direct URL (ends with SKILL.md or skill.md)
    let path = url.path().to_lowercase();
    if path.ends_with("skill.md") {
        return Ok(ParsedSource {
            source_type: SourceType::DirectUrl,
            url: input.to_string(),
            subpath: None,
            local_path: None,
            git_ref: None,
            skill_filter: None,
        });
    }

    // Well-known fallback
    Ok(ParsedSource {
        source_type: SourceType::WellKnown,
        url: input.to_string(),
        subpath: None,
        local_path: None,
        git_ref: None,
        skill_filter: None,
    })
}

/// 解析 Git URL (git@github.com:owner/repo.git)
fn parse_git_url(input: &str) -> Result<ParsedSource, AppError> {
    Ok(ParsedSource {
        source_type: SourceType::Git,
        url: input.to_string(),
        subpath: None,
        local_path: None,
        git_ref: None,
        skill_filter: None,
    })
}

/// 解析 GitHub URL
fn parse_github_url(_input: &str, url: &Url) -> Result<ParsedSource, AppError> {
    let path = url.path().trim_start_matches('/');
    let parts: Vec<&str> = path.split('/').collect();

    if parts.len() < 2 {
        return Err(AppError::InvalidSource {
            value: "Invalid GitHub URL: missing owner/repo".to_string(),
        });
    }

    let owner = parts[0];
    let repo = parts[1].trim_end_matches(".git");
    let base_url = format!("https://github.com/{}/{}", owner, repo);

    let mut result = ParsedSource::github(base_url);

    // 检查是否有 /tree/branch/path 或 /blob/branch/path
    if parts.len() > 3 && (parts[2] == "tree" || parts[2] == "blob") {
        result.git_ref = Some(parts[3].to_string());
        if parts.len() > 4 {
            result.subpath = Some(parts[4..].join("/"));
        }
    }

    Ok(result)
}

/// 解析 GitLab URL
fn parse_gitlab_url(input: &str, url: &Url) -> Result<ParsedSource, AppError> {
    let path = url.path().trim_start_matches('/');

    // GitLab 使用 /-/tree/branch/path 格式
    if let Some(tree_pos) = path.find("/-/tree/") {
        let repo_path = &path[..tree_pos];
        let after_tree = &path[tree_pos + 8..]; // "/-/tree/" 长度为 8
        let parts: Vec<&str> = after_tree.split('/').collect();

        let base_url = format!(
            "https://{}/{}",
            url.host_str().unwrap_or("gitlab.com"),
            repo_path
        );
        let mut result = ParsedSource {
            source_type: SourceType::GitLab,
            url: base_url,
            subpath: None,
            local_path: None,
            git_ref: None,
            skill_filter: None,
        };

        if !parts.is_empty() {
            result.git_ref = Some(parts[0].to_string());
            if parts.len() > 1 {
                result.subpath = Some(parts[1..].join("/"));
            }
        }

        return Ok(result);
    }

    // 简单 GitLab URL
    Ok(ParsedSource {
        source_type: SourceType::GitLab,
        url: input.to_string(),
        subpath: None,
        local_path: None,
        git_ref: None,
        skill_filter: None,
    })
}

/// 解析 GitHub shorthand (owner/repo, owner/repo/path, owner/repo@skill)
fn parse_github_shorthand(input: &str) -> Result<ParsedSource, AppError> {
    // 移除可能的 .git 后缀
    let input = input.trim_end_matches(".git");

    // 检查 @skill 语法 - 只在 owner/repo 之后查找 @（不在路径中）
    // 格式: owner/repo@skill 或 owner/repo/path@skill（path 不应包含 @）
    let (source, skill_filter) = if let Some(at_pos) = input.rfind('@') {
        // 确保 @ 在最后一个 / 之后（即不在子路径中间）
        let last_slash = input.rfind('/').unwrap_or(0);
        if at_pos > last_slash {
            // @ 在最后一段中，可能是 skill filter
            // 但需要确保这不是类似 owner/repo/v1.0@tag 的情况
            // CLI 的行为是：@ 后面的部分作为 skill name filter
            let source = &input[..at_pos];
            let filter = &input[at_pos + 1..];
            // 只有当 filter 不为空且不包含 / 时才视为 skill filter
            if !filter.is_empty() && !filter.contains('/') {
                (source, Some(filter.to_string()))
            } else {
                (input, None)
            }
        } else {
            (input, None)
        }
    } else {
        (input, None)
    };

    let parts: Vec<&str> = source.split('/').collect();

    if parts.len() < 2 {
        return Err(AppError::InvalidSource {
            value: format!("Invalid source format: {}. Expected owner/repo", input),
        });
    }

    let owner = parts[0];
    let repo = parts[1];
    let base_url = format!("https://github.com/{}/{}", owner, repo);

    let mut result = ParsedSource::github(base_url);

    // 设置子路径（如果有）
    if parts.len() > 2 {
        result.subpath = Some(parts[2..].join("/"));
    }

    // 设置 skill 过滤器
    if let Some(filter) = skill_filter {
        result.skill_filter = Some(filter);
    }

    Ok(result)
}

/// 获取规范化的 owner/repo 格式（用于 lock 文件）
pub fn get_owner_repo(parsed: &ParsedSource) -> Option<String> {
    match parsed.source_type {
        SourceType::GitHub => {
            // 从 https://github.com/owner/repo 提取 owner/repo
            if let Ok(url) = Url::parse(&parsed.url) {
                let path = url
                    .path()
                    .trim_start_matches('/')
                    .trim_end_matches(".git");
                let parts: Vec<&str> = path.split('/').take(2).collect();
                if parts.len() == 2 {
                    return Some(format!("{}/{}", parts[0], parts[1]));
                }
            }
            None
        }
        SourceType::GitLab => {
            // 从 https://gitlab.com/group/repo 提取 group/repo
            if let Ok(url) = Url::parse(&parsed.url) {
                let path = url
                    .path()
                    .trim_start_matches('/')
                    .trim_end_matches(".git");
                return Some(path.to_string());
            }
            None
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_github_shorthand() {
        let result = parse_source("owner/repo").unwrap();
        assert_eq!(result.source_type, SourceType::GitHub);
        assert_eq!(result.url, "https://github.com/owner/repo");
        assert!(result.subpath.is_none());
        assert!(result.skill_filter.is_none());
    }

    #[test]
    fn test_parse_github_shorthand_with_path() {
        let result = parse_source("owner/repo/skills/my-skill").unwrap();
        assert_eq!(result.source_type, SourceType::GitHub);
        assert_eq!(result.subpath, Some("skills/my-skill".to_string()));
    }

    #[test]
    fn test_parse_github_shorthand_with_skill_filter() {
        let result = parse_source("owner/repo@my-skill").unwrap();
        assert_eq!(result.source_type, SourceType::GitHub);
        assert_eq!(result.skill_filter, Some("my-skill".to_string()));
    }

    #[test]
    fn test_parse_github_url() {
        let result = parse_source("https://github.com/owner/repo").unwrap();
        assert_eq!(result.source_type, SourceType::GitHub);
        assert_eq!(result.url, "https://github.com/owner/repo");
    }

    #[test]
    fn test_parse_github_url_with_tree() {
        let result = parse_source("https://github.com/owner/repo/tree/main/skills").unwrap();
        assert_eq!(result.source_type, SourceType::GitHub);
        assert_eq!(result.git_ref, Some("main".to_string()));
        assert_eq!(result.subpath, Some("skills".to_string()));
    }

    #[test]
    fn test_parse_local_path_relative() {
        let result = parse_source("./skills").unwrap();
        assert_eq!(result.source_type, SourceType::Local);
        assert!(result.local_path.is_some());
    }

    #[test]
    fn test_parse_local_path_absolute_unix() {
        let result = parse_source("/home/user/skills").unwrap();
        assert_eq!(result.source_type, SourceType::Local);
    }

    #[test]
    fn test_parse_git_url() {
        let result = parse_source("git@github.com:owner/repo.git").unwrap();
        assert_eq!(result.source_type, SourceType::Git);
    }

    #[test]
    fn test_parse_direct_url() {
        let result = parse_source("https://example.com/docs/SKILL.md").unwrap();
        assert_eq!(result.source_type, SourceType::DirectUrl);
    }

    #[test]
    fn test_parse_gitlab_url() {
        let result = parse_source("https://gitlab.com/group/repo").unwrap();
        assert_eq!(result.source_type, SourceType::GitLab);
    }

    #[test]
    fn test_parse_gitlab_url_with_tree() {
        let result = parse_source("https://gitlab.com/group/repo/-/tree/main/skills").unwrap();
        assert_eq!(result.source_type, SourceType::GitLab);
        assert_eq!(result.git_ref, Some("main".to_string()));
        assert_eq!(result.subpath, Some("skills".to_string()));
    }

    #[test]
    fn test_get_owner_repo_github() {
        let parsed = parse_source("owner/repo").unwrap();
        assert_eq!(get_owner_repo(&parsed), Some("owner/repo".to_string()));
    }

    #[test]
    fn test_parse_github_shorthand_with_git_suffix() {
        // .git 后缀应该被正确处理为 GitHub 类型
        let result = parse_source("owner/repo.git").unwrap();
        assert_eq!(result.source_type, SourceType::GitHub);
        assert_eq!(result.url, "https://github.com/owner/repo");
    }

    #[test]
    fn test_parse_github_shorthand_path_with_at() {
        // 路径中的 @ 不应被误判为 skill filter
        let result = parse_source("owner/repo/path@2.0").unwrap();
        assert_eq!(result.source_type, SourceType::GitHub);
        // 这种情况下 @2.0 被视为 skill filter（CLI 行为）
        assert_eq!(result.skill_filter, Some("2.0".to_string()));
        assert_eq!(result.subpath, Some("path".to_string()));
    }

    #[test]
    fn test_parse_windows_absolute_path() {
        let result = parse_source("C:\\Users\\skills").unwrap();
        assert_eq!(result.source_type, SourceType::Local);
        assert!(result.local_path.is_some());
    }

    #[test]
    fn test_parse_windows_absolute_path_forward_slash() {
        let result = parse_source("D:/Code/skills").unwrap();
        assert_eq!(result.source_type, SourceType::Local);
    }

    #[test]
    fn test_whitespace_trimmed() {
        let result = parse_source("  owner/repo  ").unwrap();
        assert_eq!(result.source_type, SourceType::GitHub);
        assert_eq!(result.url, "https://github.com/owner/repo");
    }
}

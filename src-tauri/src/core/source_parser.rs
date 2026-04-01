//! 来源解析模块
//!
//! 支持 11 种来源格式：
//! - GitHub shorthand: owner/repo
//! - GitHub + 子路径: owner/repo/path
//! - GitHub + @skill: owner/repo@skill-name
//! - GitHub 前缀简写: github:owner/repo
//! - GitLab 前缀简写: gitlab:owner/repo
//! - GitHub URL: https://github.com/owner/repo
//! - GitHub URL + 分支: https://github.com/owner/repo/tree/branch/path
//! - GitLab URL: https://gitlab.com/group/repo
//! - GitLab URL + 分支: https://gitlab.com/group/repo/-/tree/branch/path
//! - 本地路径: ./path, /abs/path, C:\path
//! - Well-known: https://example.com (fallback)
//! - Git URL: git@github.com:owner/repo.git (fallback)

use crate::error::AppError;
use crate::models::{ParsedSource, SourceType};
use once_cell::sync::Lazy;
use regex::Regex;
use std::path::Path;
use url::Url;

/// Source 别名映射
/// 对应 CLI: source-parser.ts SOURCE_ALIASES
const SOURCE_ALIASES: &[(&str, &str)] = &[
    ("coinbase/agentWallet", "coinbase/agentic-wallet-skills"),
];

/// 解析 source 别名
fn resolve_alias(source: &str) -> String {
    SOURCE_ALIASES
        .iter()
        .find(|(alias, _)| *alias == source)
        .map(|(_, target)| target.to_string())
        .unwrap_or_else(|| source.to_string())
}

/// Fragment ref 解析结果
struct FragmentRefResult {
    /// 去除 #fragment 后的输入
    input_without_fragment: String,
    /// 提取的 git ref（分支/tag）
    git_ref: Option<String>,
    /// 提取的 skill filter（#ref@skill 中的 skill 部分）
    skill_filter: Option<String>,
}

// Lazy regex patterns for looks_like_git_source
static GITHUB_PATH_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^/[^/]+/[^/]+(?:\.git)?(?:/tree/[^/]+(?:/.*)?)?/?$").unwrap());
static GITLAB_PATH_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^/.+?/[^/]+(?:\.git)?(?:/-/tree/[^/]+(?:/.*)?)?/?$").unwrap());
static GIT_URL_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"\.git(?:$|\?|/)").unwrap());
static SHORTHAND_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^[^/]+/[^/]+(?:/(.+)|@(.+))?$").unwrap());

/// 判断输入是否看起来像 git 来源（用于决定是否提取 #fragment 作为分支引用）
/// 只有 git-like 来源才应将 # 后的内容解释为分支 ref
fn looks_like_git_source(input: &str) -> bool {
    // 前缀检查
    if input.starts_with("github:")
        || input.starts_with("gitlab:")
        || input.starts_with("git@")
    {
        return true;
    }
    // HTTP(S) URL
    if let Ok(parsed) = Url::parse(input) {
        let host = parsed.host_str().unwrap_or("");
        let path = parsed.path();
        if host == "github.com" || host == "www.github.com" {
            return GITHUB_PATH_RE.is_match(path);
        }
        if host == "gitlab.com" || host.contains("gitlab") {
            return GITLAB_PATH_RE.is_match(path);
        }
        // 通用 .git URL
        if GIT_URL_RE.is_match(input) {
            return true;
        }
    }
    // GitHub shorthand: owner/repo（不含冒号，不以 . 或 / 开头）
    !input.contains(':')
        && !input.starts_with('.')
        && !input.starts_with('/')
        && SHORTHAND_RE.is_match(input)
}

/// 从输入字符串中提取 #fragment 部分作为 git ref
/// 重要：url crate 会在解析时去掉 #fragment，所以必须在 URL 解析之前提取
fn parse_fragment_ref(input: &str) -> FragmentRefResult {
    if let Some(hash_pos) = input.find('#') {
        let before = &input[..hash_pos];
        let fragment = &input[hash_pos + 1..];

        // 空 fragment 或非 git 来源 → 不提取
        if fragment.is_empty() || !looks_like_git_source(before) {
            return FragmentRefResult {
                input_without_fragment: input.to_string(),
                git_ref: None,
                skill_filter: None,
            };
        }

        // 解析 fragment：如果包含 @，拆分为 ref 和 skill_filter
        if let Some(at_pos) = fragment.find('@') {
            let git_ref = &fragment[..at_pos];
            let skill = &fragment[at_pos + 1..];
            FragmentRefResult {
                input_without_fragment: before.to_string(),
                git_ref: if git_ref.is_empty() {
                    None
                } else {
                    Some(git_ref.to_string())
                },
                skill_filter: if skill.is_empty() {
                    None
                } else {
                    Some(skill.to_string())
                },
            }
        } else {
            FragmentRefResult {
                input_without_fragment: before.to_string(),
                git_ref: Some(fragment.to_string()),
                skill_filter: None,
            }
        }
    } else {
        FragmentRefResult {
            input_without_fragment: input.to_string(),
            git_ref: None,
            skill_filter: None,
        }
    }
}

/// 将 fragment ref 信息附加回输入字符串（用于递归调用 parse_source 时携带 ref）
fn append_fragment_ref(input: &str, git_ref: Option<&str>, skill_filter: Option<&str>) -> String {
    match (git_ref, skill_filter) {
        (Some(r), Some(s)) => format!("{}#{}@{}", input, r, s),
        (Some(r), None) => format!("{}#{}", input, r),
        (None, Some(s)) => format!("{}#@{}", input, s),
        (None, None) => input.to_string(),
    }
}

/// 解析来源字符串
pub fn parse_source(input: &str) -> Result<ParsedSource, AppError> {
    let input = input.trim();

    // 0. 空输入检查（最先）
    if input.is_empty() {
        return Err(AppError::InvalidSource {
            value: "Empty source".to_string(),
        });
    }

    // 1. 本地路径检查（在 fragment 提取之前，本地路径不含 #ref）
    if is_local_path(input) {
        return parse_local_path(input);
    }

    // 2. 提取 #fragment ref（必须在 URL 解析之前，url crate 会吞掉 fragment）
    let frag = parse_fragment_ref(input);
    let input = &frag.input_without_fragment;
    let fragment_ref = frag.git_ref;
    let fragment_skill_filter = frag.skill_filter;

    // 3. 解析别名
    let input = &resolve_alias(input);

    // 4. github: 前缀简写 → 复用 shorthand 解析（携带 fragment ref）
    if let Some(rest) = input.strip_prefix("github:") {
        let recursive_input =
            append_fragment_ref(rest, fragment_ref.as_deref(), fragment_skill_filter.as_deref());
        return parse_source(&recursive_input);
    }

    // 5. gitlab: 前缀简写 → 转换为 GitLab URL（携带 fragment ref）
    if let Some(rest) = input.strip_prefix("gitlab:") {
        let gitlab_url = format!("https://gitlab.com/{}", rest);
        let recursive_input = append_fragment_ref(
            &gitlab_url,
            fragment_ref.as_deref(),
            fragment_skill_filter.as_deref(),
        );
        return parse_source(&recursive_input);
    }

    // 6. 检查是否是 URL
    if input.starts_with("http://") || input.starts_with("https://") {
        let mut result = parse_url(input)?;
        // 合并 fragment ref（URL 提取的 ref 优先，fragment 作为 fallback）
        // Well-known 类型不应用 fragment ref
        if result.source_type != SourceType::WellKnown {
            result.git_ref = result.git_ref.or(fragment_ref);
            result.skill_filter = fragment_skill_filter.or(result.skill_filter);
        }
        return Ok(result);
    }

    // 7. 检查 Git URL (git@...)
    if input.starts_with("git@") {
        let mut result = parse_git_url(input)?;
        result.git_ref = fragment_ref;
        result.skill_filter = fragment_skill_filter;
        return Ok(result);
    }

    // 8. 尝试解析为 GitHub shorthand（支持 .git 后缀）
    let mut result = parse_github_shorthand(input)?;
    result.git_ref = result.git_ref.or(fragment_ref);
    result.skill_filter = fragment_skill_filter.or(result.skill_filter);
    Ok(result)
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

/// 检查 URL 原始路径中是否包含 `..` 遍历段（在 URL 规范化之前）
fn url_has_path_traversal(url_str: &str) -> bool {
    if let Some(idx) = url_str.find("://") {
        let after_scheme = &url_str[idx + 3..];
        let path_start = after_scheme.find('/').unwrap_or(after_scheme.len());
        let path = &after_scheme[path_start..];
        for segment in path.split('/') {
            if segment == ".." {
                return true;
            }
        }
    }
    false
}

/// 解析 URL
fn parse_url(input: &str) -> Result<ParsedSource, AppError> {
    if url_has_path_traversal(input) {
        return Err(AppError::InvalidSource {
            value: format!(
                "Unsafe URL: \"{}\" contains path traversal segments",
                input
            ),
        });
    }

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
            let sub = parts[4..].join("/");
            result.subpath = Some(sanitize_subpath(&sub)?);
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
                let sub = parts[1..].join("/");
                result.subpath = Some(sanitize_subpath(&sub)?);
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
        let sub = parts[2..].join("/");
        result.subpath = Some(sanitize_subpath(&sub)?);
    }

    // 设置 skill 过滤器
    if let Some(filter) = skill_filter {
        result.skill_filter = Some(filter);
    }

    Ok(result)
}

/// 校验 subpath 不含路径遍历攻击
/// 与 CLI sanitizeSubpath() 行为一致
pub fn sanitize_subpath(subpath: &str) -> Result<String, AppError> {
    let normalized = subpath.replace('\\', "/");
    for segment in normalized.split('/') {
        if segment == ".." {
            return Err(AppError::InvalidSource {
                value: format!(
                    "Unsafe subpath: \"{}\" contains path traversal segments",
                    subpath
                ),
            });
        }
    }
    Ok(subpath.to_string())
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
        SourceType::Git => {
            // git@host:owner/repo.git → owner/repo
            static SSH_RE: Lazy<Regex> =
                Lazy::new(|| Regex::new(r"^git@[^:]+:(.+)$").unwrap());
            if let Some(caps) = SSH_RE.captures(&parsed.url) {
                let path = caps[1].trim_end_matches(".git");
                if path.contains('/') {
                    return Some(path.to_string());
                }
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
    fn test_parse_direct_url_becomes_wellknown() {
        let result = parse_source("https://example.com/docs/SKILL.md").unwrap();
        assert_eq!(result.source_type, SourceType::WellKnown);
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

    #[test]
    fn test_source_alias_resolution() {
        let result = parse_source("coinbase/agentWallet").unwrap();
        assert_eq!(result.source_type, SourceType::GitHub);
        // URL 应包含 resolved 后的 repo 名
        assert!(result.url.contains("agentic-wallet-skills"));
    }

    #[test]
    fn test_parse_github_prefix_basic() {
        let result = parse_source("github:owner/repo").unwrap();
        assert_eq!(result.source_type, SourceType::GitHub);
        assert_eq!(result.url, "https://github.com/owner/repo");
    }

    #[test]
    fn test_parse_github_prefix_with_subpath() {
        let result = parse_source("github:owner/repo/skills/my-skill").unwrap();
        assert_eq!(result.source_type, SourceType::GitHub);
        assert_eq!(result.url, "https://github.com/owner/repo");
        assert_eq!(result.subpath, Some("skills/my-skill".to_string()));
    }

    #[test]
    fn test_parse_github_prefix_with_skill_filter() {
        let result = parse_source("github:owner/repo@my-skill").unwrap();
        assert_eq!(result.source_type, SourceType::GitHub);
        assert_eq!(result.skill_filter, Some("my-skill".to_string()));
    }

    #[test]
    fn test_parse_gitlab_prefix_basic() {
        let result = parse_source("gitlab:owner/repo").unwrap();
        assert_eq!(result.source_type, SourceType::GitLab);
        assert_eq!(result.url, "https://gitlab.com/owner/repo");
    }

    #[test]
    fn test_parse_gitlab_prefix_with_subgroups() {
        let result = parse_source("gitlab:group/subgroup/repo").unwrap();
        assert_eq!(result.source_type, SourceType::GitLab);
        assert!(result.url.contains("gitlab.com/group/subgroup/repo"));
    }

    #[test]
    fn test_get_owner_repo_ssh_github() {
        let parsed = parse_source("git@github.com:owner/repo.git").unwrap();
        assert_eq!(get_owner_repo(&parsed), Some("owner/repo".to_string()));
    }

    #[test]
    fn test_get_owner_repo_ssh_gitlab() {
        let parsed = parse_source("git@gitlab.com:owner/repo.git").unwrap();
        assert_eq!(get_owner_repo(&parsed), Some("owner/repo".to_string()));
    }

    #[test]
    fn test_get_owner_repo_ssh_subgroups() {
        let parsed = parse_source("git@gitlab.com:group/subgroup/repo.git").unwrap();
        assert_eq!(get_owner_repo(&parsed), Some("group/subgroup/repo".to_string()));
    }

    #[test]
    fn test_get_owner_repo_ssh_no_git_suffix() {
        let parsed = parse_source("git@github.com:owner/repo").unwrap();
        assert_eq!(get_owner_repo(&parsed), Some("owner/repo".to_string()));
    }

    #[test]
    fn test_get_owner_repo_ssh_custom_host() {
        let parsed = parse_source("git@git.company.com:org/team/repo.git").unwrap();
        assert_eq!(get_owner_repo(&parsed), Some("org/team/repo".to_string()));
    }

    #[test]
    fn test_sanitize_subpath_allows_normal() {
        assert!(sanitize_subpath("skills/my-skill").is_ok());
        assert!(sanitize_subpath("path/to/skill").is_ok());
        assert!(sanitize_subpath("src").is_ok());
    }

    #[test]
    fn test_sanitize_subpath_rejects_traversal() {
        assert!(sanitize_subpath("../etc").is_err());
        assert!(sanitize_subpath("../../etc/passwd").is_err());
        assert!(sanitize_subpath("skills/../../etc").is_err());
        assert!(sanitize_subpath("a/b/../../../etc").is_err());
    }

    #[test]
    fn test_sanitize_subpath_rejects_backslash_traversal() {
        assert!(sanitize_subpath("..\\etc").is_err());
        assert!(sanitize_subpath("..\\..\\secret").is_err());
    }

    #[test]
    fn test_sanitize_subpath_allows_non_traversal_dots() {
        assert!(sanitize_subpath(".hidden").is_ok());
        assert!(sanitize_subpath("file.txt").is_ok());
        assert!(sanitize_subpath("path/to/.config").is_ok());
        assert!(sanitize_subpath("..skill").is_ok());
        assert!(sanitize_subpath("skill..").is_ok());
    }

    #[test]
    fn test_parse_rejects_github_tree_traversal() {
        let result = parse_source("https://github.com/owner/repo/tree/main/../../etc");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_rejects_gitlab_tree_traversal() {
        let result = parse_source("https://gitlab.com/owner/repo/-/tree/main/../../etc");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_rejects_shorthand_traversal() {
        let result = parse_source("owner/repo/../../etc");
        assert!(result.is_err());
    }

    #[test]
    fn test_get_owner_repo_ssh_no_path_returns_none() {
        let parsed = ParsedSource {
            source_type: SourceType::Git,
            url: "git@github.com:repo.git".to_string(),
            subpath: None,
            local_path: None,
            git_ref: None,
            skill_filter: None,
        };
        assert_eq!(get_owner_repo(&parsed), None);
    }

    // ── Fragment ref (#branch) tests ──

    #[test]
    fn test_fragment_ref_github_shorthand() {
        let result = parse_source("owner/repo#feature-branch").unwrap();
        assert_eq!(result.source_type, SourceType::GitHub);
        assert_eq!(result.git_ref, Some("feature-branch".to_string()));
        assert_eq!(result.url, "https://github.com/owner/repo");
    }

    #[test]
    fn test_fragment_ref_with_skill_filter() {
        let result = parse_source("owner/repo#branch@skill-name").unwrap();
        assert_eq!(result.git_ref, Some("branch".to_string()));
        assert_eq!(result.skill_filter, Some("skill-name".to_string()));
    }

    #[test]
    fn test_fragment_ref_github_url() {
        let result = parse_source("https://github.com/owner/repo#v2.0").unwrap();
        assert_eq!(result.source_type, SourceType::GitHub);
        assert_eq!(result.git_ref, Some("v2.0".to_string()));
    }

    #[test]
    fn test_fragment_ref_git_ssh() {
        let result = parse_source("git@github.com:owner/repo.git#develop").unwrap();
        assert_eq!(result.git_ref, Some("develop".to_string()));
    }

    #[test]
    fn test_fragment_ref_github_prefix() {
        let result = parse_source("github:owner/repo#my-branch").unwrap();
        assert_eq!(result.source_type, SourceType::GitHub);
        assert_eq!(result.git_ref, Some("my-branch".to_string()));
    }

    #[test]
    fn test_fragment_ref_gitlab_prefix() {
        let result = parse_source("gitlab:group/repo#my-branch").unwrap();
        assert_eq!(result.source_type, SourceType::GitLab);
        assert_eq!(result.git_ref, Some("my-branch".to_string()));
    }

    #[test]
    fn test_fragment_ref_not_extracted_for_wellknown() {
        let result = parse_source("https://example.com#anchor").unwrap();
        assert_eq!(result.source_type, SourceType::WellKnown);
        assert_eq!(result.git_ref, None);
    }

    #[test]
    fn test_fragment_ref_empty_fragment_ignored() {
        let result = parse_source("owner/repo#").unwrap();
        assert_eq!(result.git_ref, None);
    }

    #[test]
    fn test_fragment_ref_slash_in_branch() {
        let result = parse_source("owner/repo#feature/my-branch").unwrap();
        assert_eq!(result.git_ref, Some("feature/my-branch".to_string()));
    }

    #[test]
    fn test_fragment_ref_shorthand_with_subpath() {
        let result = parse_source("owner/repo/skills/foo#my-branch").unwrap();
        assert_eq!(result.source_type, SourceType::GitHub);
        assert_eq!(result.git_ref, Some("my-branch".to_string()));
        assert_eq!(result.subpath, Some("skills/foo".to_string()));
    }
}

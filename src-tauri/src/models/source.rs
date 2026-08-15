//! 来源解析相关类型定义

use serde::{Deserialize, Serialize};
use specta::Type;
use std::path::PathBuf;

/// 来源类型枚举
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "lowercase")]
#[specta(rename_all = "lowercase")]
pub enum SourceType {
    GitHub,
    GitLab,
    Git,
    Local,
    WellKnown,
    Download,
}

impl<'de> Deserialize<'de> for SourceType {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        match s.as_str() {
            "github" => Ok(SourceType::GitHub),
            "gitlab" => Ok(SourceType::GitLab),
            "git" => Ok(SourceType::Git),
            "local" => Ok(SourceType::Local),
            "well-known" | "wellknown" => Ok(SourceType::WellKnown),
            "download" => Ok(SourceType::Download),
            "direct-url" | "directurl" => Ok(SourceType::WellKnown),
            other => Err(serde::de::Error::unknown_variant(
                other,
                &["github", "gitlab", "git", "local", "well-known", "download"],
            )),
        }
    }
}

impl std::fmt::Display for SourceType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SourceType::GitHub => write!(f, "github"),
            SourceType::GitLab => write!(f, "gitlab"),
            SourceType::Git => write!(f, "git"),
            SourceType::Local => write!(f, "local"),
            SourceType::WellKnown => write!(f, "well-known"),
            SourceType::Download => write!(f, "download"),
        }
    }
}

/// 解析后的来源信息
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct ParsedSource {
    /// 来源类型
    pub source_type: SourceType,
    /// 规范化后的 URL
    pub url: String,
    /// 仓库内子路径
    pub subpath: Option<String>,
    /// 本地路径（仅 Local 类型）
    pub local_path: Option<PathBuf>,
    /// Git 分支/tag
    pub git_ref: Option<String>,
    /// @skill 语法提取的 skill 名称
    pub skill_filter: Option<String>,
}

impl ParsedSource {
    /// 创建 GitHub 类型的 ParsedSource
    pub fn github(url: String) -> Self {
        Self {
            source_type: SourceType::GitHub,
            url,
            subpath: None,
            local_path: None,
            git_ref: None,
            skill_filter: None,
        }
    }

    /// 创建 Local 类型的 ParsedSource
    pub fn local(path: PathBuf) -> Self {
        Self {
            source_type: SourceType::Local,
            url: String::new(),
            subpath: None,
            local_path: Some(path),
            git_ref: None,
            skill_filter: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deserialize_direct_url_as_wellknown() {
        let json = r#""direct-url""#;
        let st: SourceType = serde_json::from_str(json).unwrap();
        assert_eq!(st, SourceType::WellKnown);
    }

    #[test]
    fn test_deserialize_all_source_types() {
        let cases = vec![
            ("\"github\"", SourceType::GitHub),
            ("\"gitlab\"", SourceType::GitLab),
            ("\"git\"", SourceType::Git),
            ("\"local\"", SourceType::Local),
            ("\"well-known\"", SourceType::WellKnown),
            ("\"download\"", SourceType::Download),
            ("\"wellknown\"", SourceType::WellKnown),
            ("\"direct-url\"", SourceType::WellKnown),
            ("\"directurl\"", SourceType::WellKnown),
        ];
        for (json, expected) in cases {
            let st: SourceType = serde_json::from_str(json)
                .unwrap_or_else(|e| panic!("Failed to deserialize {}: {}", json, e));
            assert_eq!(st, expected, "Mismatch for {}", json);
        }
    }

    #[test]
    fn test_deserialize_unknown_type_errors() {
        let json = r#""foobar""#;
        let result: Result<SourceType, _> = serde_json::from_str(json);
        assert!(result.is_err());
    }
}

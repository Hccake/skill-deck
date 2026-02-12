//! 来源解析相关类型定义

use serde::{Deserialize, Serialize};
use specta::Type;
use std::path::PathBuf;

/// 来源类型枚举
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "lowercase")]
#[specta(rename_all = "lowercase")]
pub enum SourceType {
    GitHub,
    GitLab,
    Git,
    Local,
    DirectUrl,
    WellKnown,
}

impl std::fmt::Display for SourceType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SourceType::GitHub => write!(f, "github"),
            SourceType::GitLab => write!(f, "gitlab"),
            SourceType::Git => write!(f, "git"),
            SourceType::Local => write!(f, "local"),
            SourceType::DirectUrl => write!(f, "direct-url"),
            SourceType::WellKnown => write!(f, "well-known"),
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

    /// 设置子路径
    pub fn with_subpath(mut self, subpath: String) -> Self {
        self.subpath = Some(subpath);
        self
    }

    /// 设置 Git ref
    pub fn with_ref(mut self, git_ref: String) -> Self {
        self.git_ref = Some(git_ref);
        self
    }

    /// 设置 skill 过滤器
    pub fn with_skill_filter(mut self, filter: String) -> Self {
        self.skill_filter = Some(filter);
        self
    }
}

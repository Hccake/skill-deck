//! 安装相关类型定义

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::application::payload_session::DiscoverySessionHandle;
use crate::core::agent_definition::AgentId;

/// 安装范围
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "lowercase")]
#[specta(rename_all = "lowercase")]
pub enum Scope {
    Global,
    Project,
}

/// 安装模式
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "lowercase")]
#[specta(rename_all = "lowercase")]
pub enum InstallMode {
    Symlink,
    Copy,
}

/// 具体安装目标展示信息，供前端确认页、完成页和目标选择使用。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct InstallTargetInfo {
    pub target_id: String,
    pub agent: AgentId,
    pub display_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subagent: Option<String>,
    pub path: String,
}

/// Agent 对某个 skill 的安装/可用状态
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "kebab-case")]
#[specta(rename_all = "kebab-case")]
pub enum AgentSkillPresence {
    DefaultActive,
    RequiresPrivateInstall,
    DuplicateCopy,
    PrivateOnly,
    NotInstalled,
}

/// 可用的 Skill 信息（fetch_available 返回）
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct AvailableSkill {
    /// Skill 名称
    pub name: String,
    /// 安装时使用的目录名
    pub install_dir_name: String,
    /// 描述
    pub description: String,
    /// 仓库内相对路径
    pub relative_path: String,
    /// 所属 plugin 名称（来自 .claude-plugin/ manifest）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plugin_name: Option<String>,
    /// Well-known discovery protocol version, if applicable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub well_known_version: Option<String>,
    /// Well-known entry type: legacy, skill-md, or archive.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub well_known_entry_type: Option<String>,
    /// Hostname of the fetched artifact URL for v2 entries.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artifact_url_host: Option<String>,
    /// Whether the v2 artifact digest was verified.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub digest_verified: Option<bool>,
    /// Compact trust reason for UI display.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trust_reason: Option<String>,
}

/// Agent presence returned by read-only skill inspection commands.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct SkillAgentPresenceInfo {
    pub agent: AgentId,
    pub display_name: String,
    pub presence: AgentSkillPresence,
    pub standard_path: String,
    pub private_path: Option<String>,
    pub can_cleanup_private_copy: bool,
}

/// Install target returned by read-only skill inspection commands.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct SkillInstallTargetInfo {
    pub target_id: String,
    pub agent: AgentId,
    pub display_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subagent: Option<String>,
    pub path: String,
}

/// fetch_available 返回结果
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct FetchResult {
    /// Opaque source snapshot shared by discovery, preview and execute.
    pub discovery_session: DiscoverySessionHandle,
    /// 来源类型
    pub source_type: String,
    /// 规范化 URL
    pub source_url: String,
    /// Git ref（branch/tag）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub git_ref: Option<String>,
    /// @skill 语法提取的名称（用于预选）
    pub skill_filter: Option<String>,
    /// 安装前风险策略
    pub risk_policy: InstallRiskPolicy,
    /// 可用的 skills 列表
    pub skills: Vec<AvailableSkill>,
}

/// 安装风险策略
#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct InstallRiskPolicy {
    pub kind: InstallRiskKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
}

/// 风险策略种类
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
#[specta(rename_all = "kebab-case")]
pub enum InstallRiskKind {
    None,
    RequireConfirmation,
}

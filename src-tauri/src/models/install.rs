//! 安装相关类型定义

use serde::{Deserialize, Serialize};
use specta::Type;
use std::path::PathBuf;

use crate::core::agents::AgentType;

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

/// 安装参数
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct InstallParams {
    /// 原始来源字符串
    pub source: String,
    /// 选中的 skill 名称列表
    pub skills: Vec<String>,
    /// 目标 agents
    pub agents: Vec<String>,
    /// 安装范围
    pub scope: Scope,
    /// Project scope 时的项目路径
    pub project_path: Option<String>,
    /// 安装模式
    pub mode: InstallMode,
    /// 是否为重试模式（仅重试指定 skills + agents）
    #[serde(default)]
    pub retry: bool,
    /// 是否按每个已安装 Agent 的现有 copy/symlink 模式重新安装
    #[serde(default)]
    pub preserve_existing_modes: bool,
    /// 是否已确认风险来源（如 OpenClaw）
    #[serde(default)]
    pub acknowledge_risk: bool,
}

/// 单个 skill 的安装结果
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct InstallResult {
    /// Skill 名称
    pub skill_name: String,
    /// Agent 名称
    pub agent: String,
    /// 是否成功
    pub success: bool,
    /// 安装路径
    pub path: PathBuf,
    /// Canonical 路径（symlink 模式）
    pub canonical_path: Option<PathBuf>,
    /// 实际使用的安装模式
    pub mode: InstallMode,
    /// symlink 是否失败并降级为 copy
    pub symlink_failed: bool,
    /// project scope 中因目标 agent 根目录不存在而跳过
    pub skipped: bool,
    /// 错误信息
    pub error: Option<String>,
}

/// 安装结果汇总
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct InstallResults {
    /// 成功的安装
    pub successful: Vec<InstallResult>,
    /// 失败的安装
    pub failed: Vec<InstallResult>,
    /// symlink 失败降级为 copy 的 agents
    pub symlink_fallback_agents: Vec<String>,
}

/// 可用的 Skill 信息（fetch_available 返回）
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct AvailableSkill {
    /// Skill 名称
    pub name: String,
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

/// 需要独立安装记录的 Agent 详情
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct IndependentAgentInfo {
    /// Agent 类型
    pub agent: AgentType,
    /// Agent 显示名称
    pub display_name: String,
    /// 安装路径
    pub path: String,
    /// 是否是 symlink（false 表示 copy 模式安装）
    pub is_symlink: bool,
}

/// Skill 的 Agent 安装详情（用于智能删除对话框）
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct SkillAgentDetails {
    /// Skill 名称
    pub skill_name: String,
    /// 安装范围
    pub scope: Scope,
    /// Canonical 目录路径
    pub canonical_path: String,
    /// 自动读取 shared canonical 目录的 Agents（带显示名称）
    pub automatic_agents: Vec<(AgentType, String)>,
    /// 有独立 symlink 或 copy 的 Agents
    pub independent_agents: Vec<IndependentAgentInfo>,
    // 注意：不设 has_independent_agents 字段，前端直接用 independent_agents.length > 0 推导（YAGNI）
}

/// 单个 skill 的删除结果
/// 对应 CLI: remove.ts 第 148-195 行的 results 数组元素
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct RemoveResult {
    /// Skill 名称
    pub skill_name: String,
    /// 是否成功
    pub success: bool,
    /// 删除的 agent 目录路径列表
    pub removed_paths: Vec<String>,
    /// 来源信息（从 lock file 读取，仅 Global）
    pub source: Option<String>,
    /// 来源类型
    pub source_type: Option<String>,
    /// 错误信息
    pub error: Option<String>,
}

/// fetch_available 返回结果
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct FetchResult {
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

#[cfg(test)]
mod tests {
    use super::InstallParams;

    #[test]
    fn test_install_params_defaults_preserve_existing_modes_to_false() {
        let params: InstallParams = serde_json::from_str(
            r#"{
                "source": "https://github.com/owner/repo",
                "skills": ["toolkit"],
                "agents": ["claude-code"],
                "scope": "global",
                "projectPath": null,
                "mode": "copy"
            }"#,
        )
        .unwrap();

        assert!(!params.preserve_existing_modes);
    }
}

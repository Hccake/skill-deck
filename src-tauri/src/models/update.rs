//! 更新结果相关类型定义

use crate::models::InstallMode;
use serde::{Deserialize, Serialize};
use specta::Type;

/// 单个 skill 的更新状态
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "lowercase")]
#[specta(rename_all = "lowercase")]
pub enum UpdateSkillStatus {
    Success,
    Partial,
    Failed,
    Skipped,
}

/// 单个 agent 的更新状态
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "lowercase")]
#[specta(rename_all = "lowercase")]
pub enum UpdateSkillAgentStatus {
    Success,
    Failed,
    Skipped,
}

/// agent 级更新结果
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct UpdateSkillAgentResult {
    pub agent: String,
    pub status: UpdateSkillAgentStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<InstallMode>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u32>,
}

/// skill 级更新结果
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct UpdateSkillItemResult {
    pub name: String,
    pub status: UpdateSkillStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub git_ref: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skill_path: Option<String>,
    #[serde(default)]
    pub warnings: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u32>,
    #[serde(default)]
    pub agent_results: Vec<UpdateSkillAgentResult>,
}

/// 更新汇总
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct UpdateSkillSummary {
    pub total: u32,
    pub succeeded: u32,
    pub partial: u32,
    pub failed: u32,
    pub skipped: u32,
}

/// 更新命令返回结果
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct UpdateSkillResponse {
    pub results: Vec<UpdateSkillItemResult>,
    pub summary: UpdateSkillSummary,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_update_skill_response_serde_shape() {
        let resp = UpdateSkillResponse {
            results: vec![UpdateSkillItemResult {
                name: "demo".to_string(),
                status: UpdateSkillStatus::Partial,
                error: None,
                reason: Some("agent-failed".to_string()),
                source: Some("owner/repo".to_string()),
                source_url: Some("https://github.com/owner/repo".to_string()),
                git_ref: Some("main".to_string()),
                skill_path: Some("skills/demo/SKILL.md".to_string()),
                warnings: vec!["lock write failed".to_string()],
                duration_ms: Some(12),
                agent_results: vec![UpdateSkillAgentResult {
                    agent: "cursor".to_string(),
                    status: UpdateSkillAgentStatus::Failed,
                    mode: Some(InstallMode::Copy),
                    error: Some("permission denied".to_string()),
                    duration_ms: Some(3),
                }],
            }],
            summary: UpdateSkillSummary {
                total: 1,
                succeeded: 0,
                partial: 1,
                failed: 0,
                skipped: 0,
            },
        };

        let value = serde_json::to_value(resp).expect("serialize");
        assert_eq!(value["results"][0]["status"], "partial");
        assert_eq!(value["results"][0]["reason"], "agent-failed");
        assert_eq!(
            value["results"][0]["sourceUrl"],
            "https://github.com/owner/repo"
        );
        assert_eq!(value["results"][0]["gitRef"], "main");
        assert_eq!(value["results"][0]["agentResults"][0]["mode"], "copy");
        assert!(value["results"][0]["agentResults"].is_array());
    }
}

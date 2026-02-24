//! Security Audit API
//!
//! 对应 CLI: telemetry.ts fetchAuditData
//! 调用 Vercel 的 audit API 获取 skill 风险等级

use serde::{Deserialize, Serialize};
use specta::Type;
use std::collections::HashMap;

const AUDIT_URL: &str = "https://add-skill.vercel.sh/audit";
const AUDIT_TIMEOUT_SECS: u64 = 3;

/// 风险等级
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "lowercase")]
#[specta(rename_all = "lowercase")]
pub enum RiskLevel {
    Safe,
    Low,
    Medium,
    High,
    Critical,
    Unknown,
}

/// Skill 审计数据
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct SkillAuditData {
    pub risk: RiskLevel,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alerts: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub score: Option<f64>,
    pub analyzed_at: String,
}

/// 获取 skill 的安全审计数据
///
/// 对应 CLI: fetchAuditData (telemetry.ts)
/// 3 秒超时，失败返回 None（graceful degradation）
pub async fn fetch_audit_data(
    source: &str,
    skills: &[String],
) -> Option<HashMap<String, SkillAuditData>> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(AUDIT_TIMEOUT_SECS))
        .build()
        .ok()?;

    let skills_param = skills.join(",");

    let response = client
        .get(AUDIT_URL)
        .query(&[("source", source), ("skills", &skills_param)])
        .send()
        .await
        .ok()?;

    if !response.status().is_success() {
        return None;
    }

    response
        .json::<HashMap<String, SkillAuditData>>()
        .await
        .ok()
}

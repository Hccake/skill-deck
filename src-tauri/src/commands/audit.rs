use crate::core::audit::{fetch_audit_data, SkillAuditData};
use crate::error::AppError;
use std::collections::HashMap;

/// 检查 skill 的安全审计数据
#[tauri::command]
#[specta::specta]
pub async fn check_skill_audit(
    source: String,
    skills: Vec<String>,
) -> Result<Option<HashMap<String, SkillAuditData>>, AppError> {
    Ok(fetch_audit_data(&source, &skills).await)
}

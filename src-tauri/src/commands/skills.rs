// list_skills command

use serde::Deserialize;

use crate::core::skill::{list_installed_skills, ListSkillsResult, SkillScope};
use crate::error::AppError;

/// list_skills 参数
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListSkillsParams {
    /// 范围: "global" | "project" | null (返回全部)
    pub scope: Option<String>,
    /// 项目路径（用于 project scope）
    pub project_path: Option<String>,
}

/// 列出已安装的 skills
/// 对应前端调用: invoke('list_skills', { params })
#[tauri::command]
pub fn list_skills(params: ListSkillsParams) -> Result<ListSkillsResult, AppError> {
    let scope = match params.scope.as_deref() {
        Some("global") => Some(SkillScope::Global),
        Some("project") => Some(SkillScope::Project),
        _ => None,
    };

    let cwd = params
        .project_path
        .unwrap_or_else(|| std::env::current_dir()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| ".".to_string()));

    // 检查路径存在性
    let path_exists = match scope {
        Some(SkillScope::Project) => std::path::Path::new(&cwd).is_dir(),
        _ => true, // global 始终为 true
    };

    let skills = if path_exists {
        list_installed_skills(scope, &cwd)?
    } else {
        Vec::new()
    };

    Ok(ListSkillsResult {
        skills,
        path_exists,
    })
}

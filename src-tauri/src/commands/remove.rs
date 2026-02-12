//! 删除相关的 Tauri Command
//!
//! 提供命令：
//! - remove_skill: 删除指定 skill（从所有已安装 agents + canonical 目录 + lock file）
//!
//! 对应 CLI: remove.ts 的 removeCommand()
//! GUI 简化：单个删除，无交互式选择，无确认提示（前端负责确认交互）

use crate::core::uninstaller;
use crate::error::AppError;
use crate::models::{RemoveResult, Scope};

/// 删除指定 skill
///
/// # Arguments
/// * `scope` - 删除范围（global/project）
/// * `name` - skill 名称
/// * `project_path` - Project scope 时的项目路径
///
/// # Returns
/// * `RemoveResult` - 删除结果
#[tauri::command]
#[specta::specta]
pub async fn remove_skill(
    scope: Scope,
    name: String,
    project_path: Option<String>,
) -> Result<RemoveResult, AppError> {
    uninstaller::remove_skill(&name, &scope, project_path.as_deref())
}

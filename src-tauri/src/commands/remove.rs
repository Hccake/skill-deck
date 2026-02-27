//! 删除相关的 Tauri Command
//!
//! 提供命令：
//! - remove_skill: 删除指定 skill（支持完全删除和部分移除）
//!
//! 对应 CLI: remove.ts 的 removeCommand()
//! GUI 增强：支持 full_removal（完全删除）和 agents 指定（部分移除）

use crate::core::agents::AgentType;
use crate::core::uninstaller;
use crate::error::AppError;
use crate::models::{RemoveResult, Scope};

/// 删除指定 skill
///
/// # Arguments
/// * `scope` - 删除范围（global/project）
/// * `name` - skill 名称
/// * `project_path` - Project scope 时的项目路径
/// * `agents` - 部分移除时指定的 agent 列表（None 或空 = 完全删除）
/// * `full_removal` - 是否完全删除（true = 删除一切，false = 仅删除指定 agents 的 symlink）
#[tauri::command]
#[specta::specta]
pub async fn remove_skill(
    scope: Scope,
    name: String,
    project_path: Option<String>,
    agents: Option<Vec<AgentType>>,
    full_removal: Option<bool>,
) -> Result<RemoveResult, AppError> {
    let full = full_removal.unwrap_or(true);
    let target_agents = agents.filter(|a| !a.is_empty());

    uninstaller::remove_skill(&name, &scope, project_path.as_deref(), full, target_agents.as_deref())
}

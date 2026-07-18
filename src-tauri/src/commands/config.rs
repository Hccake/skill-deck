use tauri::State;

use crate::application::default_agents;
use crate::commands::agents::AgentCommandError;
use crate::core::{read_config, skill_lock, write_config};
use crate::environment::types::ContextRef;
use crate::error::AppError;
use crate::models::SkillDeckConfig;
use crate::runtime::RuntimeServiceGraph;

#[tauri::command]
#[specta::specta]
pub fn get_config() -> Result<SkillDeckConfig, AppError> {
    read_config()
}

#[tauri::command]
#[specta::specta]
pub fn save_config(config: SkillDeckConfig) -> Result<(), AppError> {
    write_config(&config)
}

#[tauri::command]
#[specta::specta]
pub async fn get_default_target_agents(
    context: ContextRef,
    runtime: State<'_, RuntimeServiceGraph>,
) -> Result<Option<skill_lock::DefaultTargetAgents>, AppError> {
    default_agents::get_default_target_agents(context, runtime.environments(), runtime.agents())
        .await
}

#[tauri::command]
#[specta::specta]
pub async fn save_default_target_agents(
    context: ContextRef,
    defaults: skill_lock::DefaultTargetAgents,
    expected_registry_revision: String,
    runtime: State<'_, RuntimeServiceGraph>,
) -> Result<(), AgentCommandError> {
    default_agents::save_default_target_agents(
        context,
        defaults,
        expected_registry_revision,
        runtime.environments(),
        runtime.agents(),
        runtime.mutation(),
    )
    .await
}

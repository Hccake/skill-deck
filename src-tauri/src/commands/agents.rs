use tauri::State;

use crate::application::agent_selection::{resolve_agent_selection_groups, AgentSelectionGroups};
use crate::application::agents::{
    self, AgentDeleteImpact, AgentDeleteResult, CustomAgentDraftValidation,
};
pub use crate::application::agents::{AgentCommandError, ManagedAgentRegistry};
use crate::core::agent_definition::{AgentId, CustomAgentDefinition};
use crate::core::agent_settings::AgentSettingsSnapshot;
use crate::environment::agent_environment::AgentRuntimeSnapshot;
use crate::environment::planning::RuntimeTargetFactResolver;
use crate::environment::types::ContextRef;
use crate::models::InstallTargetInfo;
use crate::runtime::RuntimeServiceGraph;

#[tauri::command]
#[specta::specta]
pub async fn list_agents(
    context: ContextRef,
    runtime: State<'_, RuntimeServiceGraph>,
) -> Result<AgentRuntimeSnapshot, AgentCommandError> {
    agents::list_agents(context, runtime.environments(), runtime.agents()).await
}

#[tauri::command]
#[specta::specta]
pub async fn list_agent_selection_groups(
    context: ContextRef,
    runtime: State<'_, RuntimeServiceGraph>,
) -> Result<AgentSelectionGroups, AgentCommandError> {
    let agent_runtime =
        agents::list_agents(context.clone(), runtime.environments(), runtime.agents()).await?;
    let targets = RuntimeTargetFactResolver::new(runtime.environments_arc());
    resolve_agent_selection_groups(&context, &agent_runtime, &targets)
        .await
        .map_err(AgentCommandError::from)
}

#[tauri::command]
#[specta::specta]
pub fn get_agent_settings_snapshot(
    context: ContextRef,
    runtime: State<'_, RuntimeServiceGraph>,
) -> AgentSettingsSnapshot {
    agents::get_agent_settings_snapshot(context, runtime.agents())
}

#[tauri::command]
#[specta::specta]
pub async fn validate_custom_agent_draft(
    context: ContextRef,
    draft: CustomAgentDefinition,
    runtime: State<'_, RuntimeServiceGraph>,
) -> Result<CustomAgentDraftValidation, AgentCommandError> {
    agents::validate_custom_agent_draft(context, draft, runtime.environments(), runtime.agents())
        .await
}

#[tauri::command]
#[specta::specta]
pub fn save_custom_agent(
    context: ContextRef,
    draft: CustomAgentDefinition,
    original_id: Option<AgentId>,
    expected_registry_revision: String,
    runtime: State<'_, RuntimeServiceGraph>,
) -> Result<AgentSettingsSnapshot, AgentCommandError> {
    agents::save_custom_agent(
        context,
        draft,
        original_id,
        expected_registry_revision,
        runtime.agents(),
        runtime.admission(),
    )
}

#[tauri::command]
#[specta::specta]
pub async fn delete_custom_agent(
    context: ContextRef,
    id: AgentId,
    expected_registry_revision: String,
    runtime: State<'_, RuntimeServiceGraph>,
) -> Result<AgentDeleteResult, AgentCommandError> {
    agents::delete_custom_agent(
        context,
        id,
        expected_registry_revision,
        runtime.agents(),
        runtime.admission(),
        runtime.environments(),
    )
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn delete_invalid_custom_agent(
    context: ContextRef,
    index: u32,
    expected_registry_revision: String,
    runtime: State<'_, RuntimeServiceGraph>,
) -> Result<AgentDeleteResult, AgentCommandError> {
    agents::delete_invalid_custom_agent(
        context,
        index,
        expected_registry_revision,
        runtime.agents(),
        runtime.admission(),
    )
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn preview_custom_agent_delete(
    context: ContextRef,
    id: AgentId,
    expected_registry_revision: String,
    runtime: State<'_, RuntimeServiceGraph>,
) -> Result<AgentDeleteImpact, AgentCommandError> {
    agents::preview_custom_agent_delete(
        context,
        id,
        expected_registry_revision,
        runtime.agents(),
        runtime.environments(),
    )
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn list_eve_install_targets(
    context: ContextRef,
    runtime: State<'_, RuntimeServiceGraph>,
) -> Result<Vec<InstallTargetInfo>, crate::error::AppError> {
    agents::list_eve_install_targets(context, runtime.environments()).await
}

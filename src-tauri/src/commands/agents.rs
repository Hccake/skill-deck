use tauri::State;

use crate::application::agents::{
    self, AgentDeleteImpact, AgentDeleteResult, CustomAgentDraftValidation,
};
pub use crate::application::agents::{AgentCommandError, ManagedAgentRegistry};
use crate::application::skill_libraries::LibraryUsage;
use crate::core::agent_definition::{AgentId, CustomAgentDefinition};
use crate::core::agent_settings::AgentSettingsSnapshot;
use crate::environment::agent_environment::AgentRuntimeSnapshot;
use crate::environment::types::SkillLocationRef;
use crate::runtime::RuntimeServiceGraph;

#[tauri::command]
#[specta::specta]
pub async fn list_agents(
    context: SkillLocationRef,
    runtime: State<'_, RuntimeServiceGraph>,
) -> Result<AgentRuntimeSnapshot, AgentCommandError> {
    agents::list_agents(context, runtime.wsl(), runtime.agents()).await
}

#[tauri::command]
#[specta::specta]
pub fn get_agent_settings_snapshot(
    context: SkillLocationRef,
    runtime: State<'_, RuntimeServiceGraph>,
) -> AgentSettingsSnapshot {
    agents::get_agent_settings_snapshot(context, runtime.agents())
}

#[tauri::command]
#[specta::specta]
pub async fn validate_custom_agent_draft(
    context: SkillLocationRef,
    draft: CustomAgentDefinition,
    runtime: State<'_, RuntimeServiceGraph>,
) -> Result<CustomAgentDraftValidation, AgentCommandError> {
    agents::validate_custom_agent_draft(context, draft, runtime.wsl(), runtime.agents()).await
}

#[tauri::command]
#[specta::specta]
pub async fn save_custom_agent(
    context: SkillLocationRef,
    draft: CustomAgentDefinition,
    original_id: Option<AgentId>,
    expected_registry_revision: String,
    runtime: State<'_, RuntimeServiceGraph>,
) -> Result<AgentSettingsSnapshot, AgentCommandError> {
    if let Some(original_id) = original_id.as_ref() {
        let usages = runtime
            .library_usages()
            .agent_usages(&context.environment, original_id)
            .await?;
        let changes_read_paths = runtime
            .agents()
            .active_custom_definition(original_id)
            .is_some_and(|current| {
                current.id != draft.id
                    || current.global != draft.global
                    || current.project != draft.project
            });
        if changes_read_paths && !usages.is_empty() {
            return Err(crate::error::AppError::LibraryReferenceConflict {
                usages: usages.into_iter().map(|usage| usage.context).collect(),
            }
            .into());
        }
    }
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
    context: SkillLocationRef,
    id: AgentId,
    expected_registry_revision: String,
    runtime: State<'_, RuntimeServiceGraph>,
) -> Result<AgentDeleteResult, AgentCommandError> {
    let usages = runtime
        .library_usages()
        .agent_usages(&context.environment, &id)
        .await?;
    if !usages.is_empty() {
        return Err(crate::error::AppError::LibraryReferenceConflict {
            usages: usages.into_iter().map(|usage| usage.context).collect(),
        }
        .into());
    }
    agents::delete_custom_agent(
        context,
        id.clone(),
        expected_registry_revision,
        runtime.agents(),
        runtime.admission(),
    )
}

#[tauri::command]
#[specta::specta]
pub async fn delete_invalid_custom_agent(
    context: SkillLocationRef,
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
    context: SkillLocationRef,
    id: AgentId,
    expected_registry_revision: String,
    runtime: State<'_, RuntimeServiceGraph>,
) -> Result<AgentDeleteImpact, AgentCommandError> {
    let environment = context.environment.clone();
    let mut impact = agents::preview_custom_agent_delete(
        context,
        id.clone(),
        expected_registry_revision,
        runtime.agents(),
        runtime.wsl(),
    )
    .await?;
    impact.library_usages = runtime
        .library_usages()
        .agent_usages(&environment, &id)
        .await?;
    Ok(impact)
}

#[tauri::command]
#[specta::specta]
pub async fn get_agent_library_usages(
    environment: crate::environment::types::EnvironmentRef,
    id: AgentId,
    runtime: State<'_, RuntimeServiceGraph>,
) -> Result<Vec<LibraryUsage>, AgentCommandError> {
    Ok(runtime
        .library_usages()
        .agent_usages(&environment, &id)
        .await?)
}

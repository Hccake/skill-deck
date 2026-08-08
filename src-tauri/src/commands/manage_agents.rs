use tauri::State;

use crate::application::manage_agents::{
    ManageAgentSelectionSnapshot, ManageAgentsPreviewOutcome, ManageAgentsPreviewRequest,
    ManageAgentsRequest, ManageAgentsResponse,
};
use crate::core::mutation::{MutationKind, MutationPhase};
use crate::environment::types::SkillLocationRef;
use crate::error::AppError;
use crate::runtime::RuntimeServiceGraph;

#[tauri::command]
#[specta::specta]
pub async fn preview_manage_skill_agents(
    request: ManageAgentsPreviewRequest,
    runtime: State<'_, RuntimeServiceGraph>,
) -> Result<ManageAgentsPreviewOutcome, AppError> {
    runtime.manage_agents().preview(&request).await
}

#[tauri::command]
#[specta::specta]
pub async fn get_manage_agent_selection(
    context: SkillLocationRef,
    skill_name: String,
    runtime: State<'_, RuntimeServiceGraph>,
) -> Result<ManageAgentSelectionSnapshot, AppError> {
    runtime
        .manage_agents()
        .selection(&context, &skill_name)
        .await
}

#[tauri::command]
#[specta::specta]
pub async fn manage_skill_agents(
    request: ManageAgentsRequest,
    runtime: State<'_, RuntimeServiceGraph>,
) -> Result<ManageAgentsResponse, AppError> {
    let guard = runtime
        .admission()
        .begin_mutation(MutationKind::ManageAgents, request.context.clone())?;
    guard.transition(MutationPhase::Preparing, None, false);
    runtime
        .manage_agents()
        .execute(&request, guard.cancellation())
        .await
}

use tauri::State;

use crate::application::manage_agents::{
    ManageAgentsPreview, ManageAgentsPreviewRequest, ManageAgentsRequest, ManageAgentsResponse,
};
use crate::core::mutation::{MutationKind, MutationPhase};
use crate::error::AppError;
use crate::runtime::RuntimeServiceGraph;

#[tauri::command]
#[specta::specta]
pub async fn preview_manage_skill_agents(
    request: ManageAgentsPreviewRequest,
    runtime: State<'_, RuntimeServiceGraph>,
) -> Result<ManageAgentsPreview, AppError> {
    runtime.manage_agents().preview(&request).await
}

#[tauri::command]
#[specta::specta]
pub async fn manage_skill_agents(
    request: ManageAgentsRequest,
    runtime: State<'_, RuntimeServiceGraph>,
) -> Result<ManageAgentsResponse, AppError> {
    let guard = runtime
        .mutation()
        .begin(MutationKind::ManageAgents, request.context.clone())?;
    guard.transition(MutationPhase::Preparing, None, false);
    runtime
        .manage_agents()
        .execute(&request, guard.cancellation())
        .await
}

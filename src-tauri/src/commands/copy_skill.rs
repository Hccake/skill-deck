use tauri::State;

use crate::application::copy::{
    CopyExecutionRequest, CopyPreviewOutcome, CopyRequest, CopyResponse,
};
use crate::core::mutation::{MutationKind, MutationPhase};
use crate::error::AppError;
use crate::runtime::RuntimeServiceGraph;

#[tauri::command]
#[specta::specta]
pub async fn preview_copy_skill_to_projects(
    request: CopyRequest,
    runtime: State<'_, RuntimeServiceGraph>,
) -> Result<CopyPreviewOutcome, AppError> {
    runtime.copy().preview(&request).await
}

#[tauri::command]
#[specta::specta]
pub async fn copy_skill_to_projects(
    request: CopyExecutionRequest,
    runtime: State<'_, RuntimeServiceGraph>,
) -> Result<CopyResponse, AppError> {
    let context = request.request.source.clone();
    let guard = runtime
        .admission()
        .begin_mutation(MutationKind::Copy, context.clone())?;
    guard.transition(MutationPhase::Preparing, None, false);
    runtime.copy().execute(&request, guard.cancellation()).await
}

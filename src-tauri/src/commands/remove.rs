use tauri::State;

use crate::application::remove::{RemovePreview, RemoveRequest, RemoveResponse};
use crate::core::mutation::{MutationKind, MutationPhase};
use crate::environment::types::ContextRef;
use crate::error::AppError;
use crate::runtime::RuntimeServiceGraph;

#[tauri::command]
#[specta::specta]
pub async fn preview_remove(
    context: ContextRef,
    skill_name: String,
    runtime: State<'_, RuntimeServiceGraph>,
) -> Result<RemovePreview, AppError> {
    runtime.remove().preview(&context, &skill_name).await
}

#[tauri::command]
#[specta::specta]
pub async fn remove_skill(
    request: RemoveRequest,
    runtime: State<'_, RuntimeServiceGraph>,
) -> Result<RemoveResponse, AppError> {
    let guard = runtime
        .mutation()
        .begin(MutationKind::Remove, request.context.clone())?;
    guard.transition(MutationPhase::Preparing, None, false);
    runtime
        .remove()
        .execute(&request, guard.cancellation())
        .await
}

use tauri::State;

use crate::application::copy::{CopyExecutionRequest, CopyPreview, CopyRequest, CopyResponse};
use crate::core::mutation::{MutationKind, MutationPhase};
use crate::error::AppError;
use crate::runtime::RuntimeServiceGraph;

#[tauri::command]
#[specta::specta]
pub async fn preview_copy_skill_to_projects(
    request: CopyRequest,
    runtime: State<'_, RuntimeServiceGraph>,
) -> Result<CopyPreview, AppError> {
    let result = runtime.copy().preview(&request).await;
    crate::diagnostics::record_command_result(
        crate::diagnostics::DiagnosticOperation::Copy,
        &result,
        &request.source,
    );
    result
}

#[tauri::command]
#[specta::specta]
pub async fn copy_skill_to_projects(
    request: CopyExecutionRequest,
    runtime: State<'_, RuntimeServiceGraph>,
) -> Result<CopyResponse, AppError> {
    let context = request.request.source.clone();
    let result = async {
        let guard = runtime
            .mutation()
            .begin(MutationKind::Copy, context.clone())?;
        guard.transition(MutationPhase::Preparing, None, false);
        runtime.copy().execute(&request, guard.cancellation()).await
    }
    .await;
    crate::diagnostics::record_command_result(
        crate::diagnostics::DiagnosticOperation::Copy,
        &result,
        &context,
    );
    result
}

use tauri::{State, WebviewWindow};

use crate::application::update::{
    PreparedUpdatePreview, UpdateCheckRequest, UpdateCheckResponse, UpdateExecutionProgress,
    UpdateExecutionStage, UpdateRequest, UpdateResponse,
};
use crate::application::update_preparation::{PreparationTarget, PreparedUpdates};
use crate::core::mutation::{MutationKind, MutationPhase, MutationProgress};
use crate::environment::runtime::ObservedEntryId;
use crate::error::AppError;
use crate::runtime::RuntimeServiceGraph;

#[tauri::command]
#[specta::specta]
pub async fn check_updates(
    request: UpdateCheckRequest,
    runtime: State<'_, RuntimeServiceGraph>,
) -> Result<UpdateCheckResponse, AppError> {
    runtime.update_check().check(&request).await
}

#[tauri::command]
#[specta::specta]
pub async fn prepare_update(
    operation_id: String,
    request: UpdateRequest,
    window: WebviewWindow,
    runtime: State<'_, RuntimeServiceGraph>,
) -> Result<PreparedUpdatePreview, AppError> {
    let ticket = runtime
        .update_preparations()
        .begin(operation_id, window.label())?;
    let prepared = runtime
        .update()
        .prepare(&request, ticket.cancellation.clone())
        .await?;
    let preview = prepared.preview.clone();
    ticket.publish(PreparedUpdates::Direct(prepared))?;
    Ok(preview)
}

#[tauri::command]
#[specta::specta]
pub async fn cancel_update_preparation(
    operation_id: String,
    window: WebviewWindow,
    runtime: State<'_, RuntimeServiceGraph>,
) -> Result<(), AppError> {
    runtime
        .update_preparations()
        .cancel(&operation_id, window.label())
}

#[tauri::command]
#[specta::specta]
pub async fn execute_update(
    operation_id: String,
    selected_copy_entries: Vec<ObservedEntryId>,
    window: WebviewWindow,
    runtime: State<'_, RuntimeServiceGraph>,
) -> Result<UpdateResponse, AppError> {
    let PreparationTarget::Direct(context) = runtime
        .update_preparations()
        .target(&operation_id, window.label())?
    else {
        return Err(AppError::StaleContext);
    };
    let guard = runtime
        .admission()
        .begin_mutation(MutationKind::Update, context.clone())?;
    let PreparedUpdates::Direct(prepared) = runtime
        .update_preparations()
        .take(&operation_id, window.label())?
    else {
        return Err(AppError::StaleContext);
    };
    guard.transition(MutationPhase::Validating, None, true);
    let result = runtime
        .update()
        .execute_prepared(
            prepared,
            &selected_copy_entries,
            guard.cancellation(),
            |event| {
                let UpdateExecutionProgress {
                    stage,
                    subject,
                    current,
                    total,
                } = event;
                guard.transition(
                    match stage {
                        UpdateExecutionStage::Validating => MutationPhase::Validating,
                        UpdateExecutionStage::Updating => MutationPhase::Committing,
                    },
                    Some(MutationProgress {
                        subject,
                        current,
                        total,
                    }),
                    matches!(stage, UpdateExecutionStage::Validating),
                );
            },
        )
        .await;
    guard.transition(MutationPhase::Finishing, None, false);
    result
}

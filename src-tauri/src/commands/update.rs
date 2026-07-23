use tauri::State;

use crate::application::mutation::plan::PreviewToken;
use crate::application::update::{
    UpdateCheckRequest, UpdateCheckResponse, UpdateExecutionProgress, UpdateExecutionRequest,
    UpdateExecutionStage, UpdatePreview, UpdateRequest, UpdateResponse,
};
use crate::core::mutation::{MutationKind, MutationPhase, MutationProgress};
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
pub async fn preview_update(
    request: UpdateRequest,
    runtime: State<'_, RuntimeServiceGraph>,
) -> Result<UpdatePreview, AppError> {
    runtime.update().preview(&request).await
}

#[tauri::command]
#[specta::specta]
pub async fn update_skill(
    execution: UpdateExecutionRequest,
    expected_token: PreviewToken,
    runtime: State<'_, RuntimeServiceGraph>,
) -> Result<UpdateResponse, AppError> {
    if execution.request.skill_names.len() != 1 {
        return Err(AppError::Validation {
            field: Some("skillNames".to_string()),
            message: "single-Skill update requires exactly one Skill".to_string(),
        });
    }
    execute_update(execution, expected_token, runtime).await
}

#[tauri::command]
#[specta::specta]
pub async fn update_skills_batch(
    execution: UpdateExecutionRequest,
    expected_token: PreviewToken,
    runtime: State<'_, RuntimeServiceGraph>,
) -> Result<UpdateResponse, AppError> {
    execute_update(execution, expected_token, runtime).await
}

async fn execute_update(
    execution: UpdateExecutionRequest,
    expected_token: PreviewToken,
    runtime: State<'_, RuntimeServiceGraph>,
) -> Result<UpdateResponse, AppError> {
    let guard = runtime
        .mutation()
        .begin(MutationKind::Update, execution.request.context.clone())?;
    guard.transition(MutationPhase::Acquiring, None, true);
    let result = runtime
        .update()
        .execute_with_stage_observer(&execution, expected_token, guard.cancellation(), |event| {
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
        })
        .await;
    guard.transition(MutationPhase::Finishing, None, false);
    result
}

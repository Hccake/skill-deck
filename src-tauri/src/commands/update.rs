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
    validate_single_skill_update(&execution)?;
    execute_update(execution, expected_token, runtime).await
}

fn validate_single_skill_update(execution: &UpdateExecutionRequest) -> Result<(), AppError> {
    if execution.request.skill_names.len() == 1 {
        return Ok(());
    }
    Err(AppError::Validation {
        field: Some("skillNames".to_string()),
        message: "single-Skill update requires exactly one Skill".to_string(),
    })
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
    let context = execution.request.context.clone();
    let guard = runtime
        .admission()
        .begin_mutation(MutationKind::Update, context.clone())?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::update::UpdateRequest;
    use crate::environment::types::{EnvironmentRef, SkillLocation, SkillLocationRef};

    #[test]
    fn single_skill_update_rejects_batch_requests() {
        let execution = UpdateExecutionRequest {
            request: UpdateRequest {
                context: SkillLocationRef {
                    environment: EnvironmentRef::Native,
                    scope: SkillLocation::Global,
                },
                skill_names: vec!["alpha".to_string(), "beta".to_string()],
            },
            overwrite_private_entries: Vec::new(),
        };

        assert!(matches!(
            validate_single_skill_update(&execution),
            Err(AppError::Validation {
                field: Some(field),
                ..
            }) if field == "skillNames"
        ));
    }
}

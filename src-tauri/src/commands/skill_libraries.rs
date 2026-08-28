use tauri::State;

use crate::application::library_application::{
    ApplyLibraryApplicationRequest, LibraryAgentOptions, LibraryApplicationDraft,
    LibraryApplicationPreview, LibraryApplicationResponse, LibraryApplicationSummary,
};
use crate::application::library_update::{
    ExecuteLibraryUpdateRequest, LibraryUpdateExecutionOutcome, LibraryUpdateExecutionStage,
    LibraryUpdatePreview,
};
use crate::application::skill_libraries::{
    ExecuteAddLibrarySkillsRequest, LibraryAddPreview, LibraryAddResponse, LibraryId,
    LibraryWorkspaceSnapshot, PreviewAddLibrarySkillsRequest, RemoveLibrarySkillRequest,
    SkillLibraryDetail, UpdateLibrarySkillsRequest,
};
use crate::application::update::{UpdateCheckMode, UpdateCheckResponse};
use crate::core::mutation::{MutationKind, MutationPhase};
use crate::environment::types::EnvironmentRef;
use crate::environment::types::SkillLocationRef;
use crate::error::AppError;
use crate::runtime::RuntimeServiceGraph;

#[tauri::command]
#[specta::specta]
pub async fn list_skill_libraries(
    environment: EnvironmentRef,
    runtime: State<'_, RuntimeServiceGraph>,
) -> Result<LibraryWorkspaceSnapshot, AppError> {
    runtime.skill_libraries().workspace(environment).await
}

#[tauri::command]
#[specta::specta]
pub async fn create_skill_library(
    environment: EnvironmentRef,
    name: String,
    runtime: State<'_, RuntimeServiceGraph>,
) -> Result<LibraryWorkspaceSnapshot, AppError> {
    let _permit = runtime.admission().begin_exclusive_action()?;
    runtime.skill_libraries().create(environment, name).await
}

#[tauri::command]
#[specta::specta]
pub async fn rename_skill_library(
    environment: EnvironmentRef,
    library_id: LibraryId,
    name: String,
    runtime: State<'_, RuntimeServiceGraph>,
) -> Result<LibraryWorkspaceSnapshot, AppError> {
    let _permit = runtime.admission().begin_exclusive_action()?;
    runtime
        .skill_libraries()
        .rename(environment, library_id, name)
        .await
}

#[tauri::command]
#[specta::specta]
pub async fn get_skill_library(
    environment: EnvironmentRef,
    library_id: LibraryId,
    runtime: State<'_, RuntimeServiceGraph>,
) -> Result<SkillLibraryDetail, AppError> {
    runtime
        .skill_libraries()
        .detail(environment, library_id)
        .await
}

#[tauri::command]
#[specta::specta]
pub async fn read_library_skill_content(
    environment: EnvironmentRef,
    library_id: LibraryId,
    skill_name: String,
    runtime: State<'_, RuntimeServiceGraph>,
) -> Result<String, AppError> {
    runtime
        .skill_libraries()
        .read_skill_content(environment, library_id, skill_name)
        .await
}

#[tauri::command]
#[specta::specta]
pub async fn add_skills_to_library(
    request: ExecuteAddLibrarySkillsRequest,
    runtime: State<'_, RuntimeServiceGraph>,
) -> Result<LibraryAddResponse, AppError> {
    let _permit = runtime.admission().begin_exclusive_action()?;
    runtime
        .skill_libraries()
        .execute_add_skills(
            runtime.payloads(),
            runtime.agent_selection_targets(),
            request,
        )
        .await
}

#[tauri::command]
#[specta::specta]
pub async fn preview_add_library_skills(
    request: PreviewAddLibrarySkillsRequest,
    runtime: State<'_, RuntimeServiceGraph>,
) -> Result<LibraryAddPreview, AppError> {
    runtime
        .skill_libraries()
        .preview_add_skills(
            runtime.payloads(),
            runtime.agent_selection_targets(),
            request,
        )
        .await
}

#[tauri::command]
#[specta::specta]
pub async fn check_library_skill_updates(
    environment: EnvironmentRef,
    library_id: LibraryId,
    runtime: State<'_, RuntimeServiceGraph>,
) -> Result<UpdateCheckResponse, AppError> {
    let names = runtime
        .skill_libraries()
        .detail(environment.clone(), library_id.clone())
        .await?
        .skills
        .into_iter()
        .map(|skill| skill.name)
        .collect();
    runtime
        .library_update_check()
        .check(environment, library_id, UpdateCheckMode::Force, names)
        .await
}

#[tauri::command]
#[specta::specta]
pub async fn preview_library_skill_updates(
    request: UpdateLibrarySkillsRequest,
    runtime: State<'_, RuntimeServiceGraph>,
) -> Result<LibraryUpdatePreview, AppError> {
    runtime.library_update().preview(&request).await
}

#[tauri::command]
#[specta::specta]
pub async fn update_library_skills(
    request: ExecuteLibraryUpdateRequest,
    runtime: State<'_, RuntimeServiceGraph>,
) -> Result<LibraryUpdateExecutionOutcome, AppError> {
    let permit = runtime.admission().begin_library_mutation(
        MutationKind::ManageLibraries,
        request.request.environment.clone(),
        request.request.library_id.as_str().to_string(),
    )?;
    permit.transition(MutationPhase::Acquiring, None, true);
    let result = runtime
        .library_update()
        .execute_with_stage_observer(&request, permit.cancellation(), |stage| {
            let (phase, cancelable) = match stage {
                LibraryUpdateExecutionStage::Acquiring => (MutationPhase::Acquiring, true),
                LibraryUpdateExecutionStage::Validating => (MutationPhase::Validating, true),
                LibraryUpdateExecutionStage::Committing => (MutationPhase::Committing, false),
            };
            permit.transition(phase, None, cancelable);
        })
        .await;
    permit.transition(MutationPhase::Finishing, None, false);
    result
}

#[tauri::command]
#[specta::specta]
pub async fn remove_library_skill(
    request: RemoveLibrarySkillRequest,
    runtime: State<'_, RuntimeServiceGraph>,
) -> Result<SkillLibraryDetail, AppError> {
    let _permit = runtime.admission().begin_exclusive_action()?;
    runtime
        .skill_libraries()
        .remove_skill(runtime.agent_selection_targets(), request)
        .await
}

#[tauri::command]
#[specta::specta]
pub async fn delete_skill_library(
    environment: EnvironmentRef,
    library_id: LibraryId,
    runtime: State<'_, RuntimeServiceGraph>,
) -> Result<LibraryWorkspaceSnapshot, AppError> {
    let _permit = runtime.admission().begin_exclusive_action()?;
    runtime
        .skill_libraries()
        .delete(environment, library_id)
        .await
}

#[tauri::command]
#[specta::specta]
pub async fn get_library_application(
    context: SkillLocationRef,
    runtime: State<'_, RuntimeServiceGraph>,
) -> Result<LibraryApplicationSummary, AppError> {
    runtime.library_application().read(context).await
}

#[tauri::command]
#[specta::specta]
pub async fn get_library_agent_options(
    context: SkillLocationRef,
    runtime: State<'_, RuntimeServiceGraph>,
) -> Result<LibraryAgentOptions, AppError> {
    runtime.library_application().agent_options(context).await
}

#[tauri::command]
#[specta::specta]
pub async fn preview_library_application(
    draft: LibraryApplicationDraft,
    runtime: State<'_, RuntimeServiceGraph>,
) -> Result<LibraryApplicationPreview, AppError> {
    runtime.library_application().preview(draft).await
}

#[tauri::command]
#[specta::specta]
pub async fn apply_library_application(
    request: ApplyLibraryApplicationRequest,
    runtime: State<'_, RuntimeServiceGraph>,
) -> Result<LibraryApplicationResponse, AppError> {
    let permit = runtime
        .admission()
        .begin_mutation(MutationKind::ManageLibraries, request.draft.context.clone())?;
    runtime
        .library_application()
        .apply(request, permit.cancellation())
        .await
}

#[tauri::command]
#[specta::specta]
pub async fn retry_library_application(
    context: SkillLocationRef,
    runtime: State<'_, RuntimeServiceGraph>,
) -> Result<LibraryApplicationResponse, AppError> {
    let permit = runtime
        .admission()
        .begin_mutation(MutationKind::ManageLibraries, context.clone())?;
    runtime
        .library_application()
        .retry_pending(context, permit.cancellation())
        .await
}

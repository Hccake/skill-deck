use tauri::State;

use crate::application::install::{InstallPreview, InstallRequest, InstallResponse};
use crate::application::mutation::plan::PreviewToken;
use crate::core::mutation::{MutationKind, MutationPhase};
use crate::error::AppError;
use crate::runtime::RuntimeServiceGraph;

#[tauri::command]
#[specta::specta]
pub async fn preview_install(
    request: InstallRequest,
    runtime: State<'_, RuntimeServiceGraph>,
) -> Result<InstallPreview, AppError> {
    runtime.install().preview(&request).await
}

#[tauri::command]
#[specta::specta]
pub async fn install_skills(
    request: InstallRequest,
    expected_token: PreviewToken,
    runtime: State<'_, RuntimeServiceGraph>,
) -> Result<InstallResponse, AppError> {
    async {
        let guard = runtime
            .mutation()
            .begin(MutationKind::Install, request.context.clone())?;
        guard.transition(MutationPhase::Acquiring, None, true);
        runtime
            .install()
            .execute(&request, expected_token, guard.cancellation())
            .await
    }
    .await
}

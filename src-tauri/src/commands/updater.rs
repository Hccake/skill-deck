use std::sync::Arc;

use tauri::ipc::Channel;
use tauri::{AppHandle, State};

use crate::application::application_update::{
    ApplicationUpdateCoordinator, ApplicationUpdateInfo, ApplicationUpdateProgress,
    ApplicationUpdateResult,
};
use crate::error::AppError;
use crate::runtime::RuntimeServiceGraph;

#[tauri::command]
#[specta::specta]
pub async fn check_application_update(
    app: AppHandle,
    runtime: State<'_, RuntimeServiceGraph>,
) -> Result<Option<ApplicationUpdateInfo>, AppError> {
    let updater = runtime.application_updater(app);
    ApplicationUpdateCoordinator::new(runtime.admission())
        .check(&updater)
        .await
}

#[tauri::command]
#[specta::specta]
pub async fn download_and_install_application_update(
    app: AppHandle,
    runtime: State<'_, RuntimeServiceGraph>,
    expected_version: String,
    progress: Channel<ApplicationUpdateProgress>,
) -> Result<ApplicationUpdateResult, AppError> {
    let updater = runtime.application_updater(app);
    ApplicationUpdateCoordinator::new(runtime.admission())
        .download_and_install(
            &updater,
            &expected_version,
            Arc::new(move |event| {
                let _ = progress.send(event);
            }),
        )
        .await
}

#[tauri::command]
#[specta::specta]
pub fn cancel_application_update_download(
    runtime: State<'_, RuntimeServiceGraph>,
) -> Result<bool, AppError> {
    runtime.admission().request_cancel_lifecycle()
}

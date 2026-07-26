use tauri::State;

use crate::application::recovery::RecoveryResourceStatus;
use crate::error::{AppError, RecoveryResourceId};
use crate::runtime::RuntimeServiceGraph;

#[tauri::command]
#[specta::specta]
pub async fn list_recovery_resources(
    runtime: State<'_, RuntimeServiceGraph>,
) -> Result<Vec<RecoveryResourceStatus>, AppError> {
    runtime.recovery().list().await
}

#[tauri::command]
#[specta::specta]
pub async fn get_recovery_resource_status(
    resource_id: RecoveryResourceId,
    runtime: State<'_, RuntimeServiceGraph>,
) -> Result<RecoveryResourceStatus, AppError> {
    runtime.recovery().status(&resource_id).await
}

#[tauri::command]
#[specta::specta]
pub async fn confirm_recovery_resource_resolved(
    resource_id: RecoveryResourceId,
    expected_revision: String,
    runtime: State<'_, RuntimeServiceGraph>,
) -> Result<(), AppError> {
    runtime
        .recovery()
        .confirm_resolved(&resource_id, &expected_revision)
        .await
}

#[tauri::command]
#[specta::specta]
pub async fn open_recovery_resource(
    resource_id: RecoveryResourceId,
    runtime: State<'_, RuntimeServiceGraph>,
) -> Result<(), AppError> {
    let target = runtime.recovery().open_target(&resource_id)?;
    crate::environment::opener::open_authorized_resource(&target)
}

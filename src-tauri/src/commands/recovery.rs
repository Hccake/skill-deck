use tauri::State;

use crate::application::recovery::{RecoveryResourceStatus, RecoveryResourcesSnapshot};
use crate::environment::maintenance::RuntimeMaintenanceStatus;
use crate::environment::types::EnvironmentRef;
use crate::error::{AppError, RecoveryResourceId};
use crate::runtime::RuntimeServiceGraph;

#[tauri::command]
#[specta::specta]
pub async fn list_recovery_resources(
    runtime: State<'_, RuntimeServiceGraph>,
) -> Result<RecoveryResourcesSnapshot, AppError> {
    Ok(RecoveryResourcesSnapshot {
        maintenance: runtime.maintenance().statuses()?,
        resources: runtime.recovery().list().await?,
    })
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

#[tauri::command]
#[specta::specta]
pub async fn retry_runtime_maintenance(
    environment: EnvironmentRef,
    runtime: State<'_, RuntimeServiceGraph>,
) -> Result<RuntimeMaintenanceStatus, AppError> {
    runtime.maintenance().retry(environment).await
}

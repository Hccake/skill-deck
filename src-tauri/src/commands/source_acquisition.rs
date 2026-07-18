use std::sync::Arc;

use tauri::State;

use crate::application::payload_session::AcquiredPayloadHandle;
use crate::application::source_acquisition::{
    AcquireSelectedPayloadsRequest, SelectedPayloadAcquisitionService,
};
use crate::error::AppError;
use crate::runtime::RuntimeServiceGraph;

#[tauri::command]
#[specta::specta]
pub async fn acquire_selected_payloads(
    request: AcquireSelectedPayloadsRequest,
    runtime: State<'_, RuntimeServiceGraph>,
) -> Result<Vec<AcquiredPayloadHandle>, AppError> {
    SelectedPayloadAcquisitionService::new(Arc::new(runtime.payloads().clone()))
        .acquire(request)
        .await
}

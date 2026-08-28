use std::sync::Arc;

use tauri::{Emitter, State, WebviewWindow};

use crate::application::payload_session::AcquiredPayloadHandle;
use crate::application::source_acquisition::{
    AcquireSelectedPayloadsRequest, FetchResult, SelectedPayloadAcquisitionService,
    SourceSelectionIntent,
};
use crate::core::CloneProgress;
use crate::environment::types::EnvironmentRef;
use crate::error::AppError;
use crate::runtime::RuntimeServiceGraph;

#[derive(Debug, serde::Serialize)]
struct SourceDiscoveryProgressEvent {
    operation_id: String,
    #[serde(flatten)]
    progress: CloneProgress,
}

#[tauri::command]
#[specta::specta]
pub async fn discover_skill_source(
    window: WebviewWindow,
    environment: EnvironmentRef,
    source: String,
    operation_id: String,
    selection_intent: SourceSelectionIntent,
    runtime: State<'_, RuntimeServiceGraph>,
) -> Result<FetchResult, AppError> {
    let window = window.clone();
    runtime
        .source_discovery()
        .discover_with_selection(environment, source, selection_intent, move |progress| {
            let _ = window.emit(
                "clone-progress",
                &SourceDiscoveryProgressEvent {
                    operation_id: operation_id.clone(),
                    progress,
                },
            );
        })
        .await
}

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

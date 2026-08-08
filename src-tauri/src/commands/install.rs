use std::sync::Arc;

use tauri::{Emitter, State, WebviewWindow};

use crate::application::source_acquisition::SourceDiscoveryService;
use crate::core::CloneProgress;
use crate::environment::types::SkillLocationRef;
use crate::error::AppError;
use crate::models::FetchResult;
use crate::runtime::RuntimeServiceGraph;

#[derive(Debug, serde::Serialize)]
struct SourceFetchProgressEvent {
    operation_id: String,
    #[serde(flatten)]
    progress: CloneProgress,
}

#[tauri::command]
#[specta::specta]
pub async fn fetch_available(
    window: WebviewWindow,
    context: SkillLocationRef,
    source: String,
    operation_id: String,
    runtime: State<'_, RuntimeServiceGraph>,
) -> Result<FetchResult, AppError> {
    let window = window.clone();
    SourceDiscoveryService::new(Arc::new(runtime.payloads().clone()), runtime.wsl())
        .discover(context, source, move |progress| {
            let _ = window.emit(
                "clone-progress",
                &SourceFetchProgressEvent {
                    operation_id: operation_id.clone(),
                    progress,
                },
            );
        })
        .await
}

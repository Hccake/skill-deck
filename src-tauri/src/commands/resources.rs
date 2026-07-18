use tauri::State;

use crate::application::resources::{ConfigResourceKind, SkillIdentity};
use crate::environment::types::ContextRef;
use crate::error::AppError;
use crate::runtime::RuntimeServiceGraph;

#[tauri::command]
#[specta::specta]
pub async fn open_skill_resource(
    identity: SkillIdentity,
    runtime: State<'_, RuntimeServiceGraph>,
) -> Result<(), AppError> {
    runtime.resources().open_skill(&identity).await
}

#[tauri::command]
#[specta::specta]
pub async fn open_config_resource(
    context: ContextRef,
    kind: ConfigResourceKind,
    runtime: State<'_, RuntimeServiceGraph>,
) -> Result<(), AppError> {
    runtime.resources().open_config(&context, kind).await
}

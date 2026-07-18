use tauri::State;

use crate::application::resources::SkillIdentity;
use crate::core::skill::ListSkillsResult;
use crate::environment::types::ContextRef;
use crate::error::AppError;
use crate::runtime::RuntimeServiceGraph;

#[tauri::command]
#[specta::specta]
pub async fn list_skills(
    context: ContextRef,
    runtime: State<'_, RuntimeServiceGraph>,
) -> Result<ListSkillsResult, AppError> {
    crate::application::skills::list_skills(context, runtime.environments(), runtime.agents()).await
}

#[tauri::command]
#[specta::specta]
pub async fn read_skill_content(
    identity: SkillIdentity,
    runtime: State<'_, RuntimeServiceGraph>,
) -> Result<String, AppError> {
    runtime.resources().read_skill(&identity).await
}

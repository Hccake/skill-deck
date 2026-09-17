use tauri::State;

use crate::application::resources::SkillIdentity;
use crate::application::skill_read::ListSkillsResult;
use crate::environment::types::SkillLocationRef;
use crate::error::AppError;
use crate::runtime::RuntimeServiceGraph;

#[tauri::command]
#[specta::specta]
pub async fn list_skills(
    context: SkillLocationRef,
    runtime: State<'_, RuntimeServiceGraph>,
) -> Result<ListSkillsResult, AppError> {
    let mut result = crate::application::skills::list_skills(
        context.clone(),
        runtime.wsl(),
        runtime.agents(),
        runtime.skill_libraries().repository(),
        runtime.agent_selection_targets(),
    )
    .await?;
    runtime
        .update_check()
        .qualify_skills(&context, &mut result.skills)
        .await?;
    result.library_application = runtime.library_application().read(context).await?;
    Ok(result)
}

#[tauri::command]
#[specta::specta]
pub async fn read_skill_content(
    identity: SkillIdentity,
    runtime: State<'_, RuntimeServiceGraph>,
) -> Result<String, AppError> {
    runtime.resources().read_skill(&identity).await
}

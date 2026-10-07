use crate::models::translation::*;
use crate::runtime::RuntimeServiceGraph;
use tauri::{State, WebviewWindow};

fn authorize(window: &WebviewWindow) -> Result<(), TranslationError> {
    if window.label() == "main" {
        Ok(())
    } else {
        Err(TranslationError::InvalidSettings)
    }
}

#[tauri::command]
#[specta::specta]
pub fn get_translation_settings(
    window: WebviewWindow,
    runtime: State<'_, RuntimeServiceGraph>,
) -> Result<TranslationSettings, TranslationError> {
    authorize(&window)?;
    runtime.translation().settings()
}

#[tauri::command]
#[specta::specta]
pub async fn save_translation_settings(
    window: WebviewWindow,
    draft: TranslationSettingsDraft,
    runtime: State<'_, RuntimeServiceGraph>,
) -> Result<TranslationSettings, TranslationError> {
    authorize(&window)?;
    let _permit = runtime
        .admission()
        .begin_mutation(
            crate::core::mutation::MutationKind::UpdateSettings,
            crate::environment::types::SkillLocationRef {
                environment: crate::environment::types::EnvironmentRef::Native,
                scope: crate::environment::types::SkillLocation::Global,
            },
        )
        .map_err(|_| TranslationError::InvalidSettings)?;
    runtime.translation().save(draft).await
}

#[tauri::command]
#[specta::specta]
pub async fn test_translation_service(
    window: WebviewWindow,
    service: TranslationServiceRef,
    profile: Option<TranslationProfileDraft>,
    runtime: State<'_, RuntimeServiceGraph>,
) -> Result<String, TranslationError> {
    authorize(&window)?;
    runtime.translation().test(service, profile).await
}

#[tauri::command]
#[specta::specta]
pub fn begin_translation(
    window: WebviewWindow,
    revision: String,
    runtime: State<'_, RuntimeServiceGraph>,
) -> Result<String, TranslationError> {
    authorize(&window)?;
    runtime.translation().begin(&revision)
}

#[tauri::command]
#[specta::specta]
pub async fn translate_units(
    window: WebviewWindow,
    session_id: String,
    units: Vec<TranslationUnit>,
    runtime: State<'_, RuntimeServiceGraph>,
) -> Result<Vec<TranslationResult>, TranslationError> {
    authorize(&window)?;
    runtime.translation().units(&session_id, units).await
}

#[tauri::command]
#[specta::specta]
pub fn cancel_translation(
    window: WebviewWindow,
    session_id: String,
    runtime: State<'_, RuntimeServiceGraph>,
) -> Result<(), TranslationError> {
    authorize(&window)?;
    runtime.translation().cancel(&session_id);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn get_translation_cache_info(
    window: WebviewWindow,
    runtime: State<'_, RuntimeServiceGraph>,
) -> Result<TranslationCacheInfo, TranslationError> {
    authorize(&window)?;
    Ok(runtime.translation().cache_info())
}

#[tauri::command]
#[specta::specta]
pub fn clear_translation_cache(
    window: WebviewWindow,
    runtime: State<'_, RuntimeServiceGraph>,
) -> Result<TranslationCacheInfo, TranslationError> {
    authorize(&window)?;
    runtime.translation().clear_cache()
}

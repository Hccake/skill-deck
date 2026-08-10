use tauri::State;

use crate::application::network_settings;
use crate::core::mutation::MutationKind;
use crate::environment::types::{EnvironmentRef, SkillLocation, SkillLocationRef};
use crate::error::AppError;
use crate::models::NetworkProxySettings;
use crate::runtime::RuntimeServiceGraph;

#[tauri::command]
#[specta::specta]
pub fn get_proxy_settings() -> Result<NetworkProxySettings, AppError> {
    network_settings::get_proxy_settings()
}

#[tauri::command]
#[specta::specta]
pub fn save_proxy_settings(
    settings: NetworkProxySettings,
    runtime: State<'_, RuntimeServiceGraph>,
) -> Result<NetworkProxySettings, AppError> {
    let _permit = runtime.admission().begin_mutation(
        MutationKind::UpdateSettings,
        SkillLocationRef {
            environment: EnvironmentRef::Native,
            scope: SkillLocation::Global,
        },
    )?;
    network_settings::save_proxy_settings(settings, |settings| {
        runtime.activate_network_settings(settings)
    })
}

use tauri::State;

use crate::core::mutation::MutationKind;
use crate::environment::types::{EnvironmentRef, SkillLocation, SkillLocationRef};
use crate::error::AppError;
use crate::models::{NetworkProxySettings, ProxySettingsSnapshot};
use crate::runtime::network_connection::ProxyConnectionTestResult;
use crate::runtime::RuntimeServiceGraph;

#[tauri::command]
#[specta::specta]
pub fn get_proxy_settings(
    reload: Option<bool>,
    runtime: State<'_, RuntimeServiceGraph>,
) -> Result<ProxySettingsSnapshot, AppError> {
    if reload.unwrap_or(false) {
        let _permit = runtime.admission().begin_mutation(
            MutationKind::UpdateSettings,
            SkillLocationRef {
                environment: EnvironmentRef::Native,
                scope: SkillLocation::Global,
            },
        )?;
        Ok(runtime.config().reload())
    } else {
        Ok(runtime.config().proxy_snapshot())
    }
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
    runtime.config().save_proxy(settings)
}

#[tauri::command]
#[specta::specta]
pub async fn test_proxy_connection(
    settings: NetworkProxySettings,
    wsl_distros: Vec<String>,
    runtime: State<'_, RuntimeServiceGraph>,
) -> Result<ProxyConnectionTestResult, AppError> {
    runtime.connection_probe().run(settings, wsl_distros).await
}

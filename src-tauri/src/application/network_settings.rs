use crate::core::{read_config, update_config};
use crate::error::AppError;
use crate::models::NetworkProxySettings;

pub(crate) fn get_proxy_settings() -> Result<NetworkProxySettings, AppError> {
    Ok(read_config()?.network_proxy)
}

pub(crate) fn save_proxy_settings(
    settings: NetworkProxySettings,
    activate: impl FnOnce(NetworkProxySettings),
) -> Result<NetworkProxySettings, AppError> {
    save_proxy_settings_with(settings, activate, |normalized| {
        let normalized = normalized.clone();
        update_config(move |config| config.network_proxy = normalized)?;
        Ok(())
    })
}

fn save_proxy_settings_with(
    settings: NetworkProxySettings,
    activate: impl FnOnce(NetworkProxySettings),
    persist: impl FnOnce(&NetworkProxySettings) -> Result<(), AppError>,
) -> Result<NetworkProxySettings, AppError> {
    let normalized =
        settings
            .validate_and_normalize()
            .map_err(|error| AppError::InvalidProxySettings {
                code: error.code().to_string(),
            })?;
    persist(&normalized)?;
    activate(normalized.clone());
    Ok(normalized)
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use crate::error::AppError;
    use crate::models::{NetworkProxySettings, ProxyMode};

    use super::save_proxy_settings_with;

    #[test]
    fn runtime_settings_change_only_after_proxy_settings_are_persisted() {
        let activated = Mutex::new(Vec::new());
        let custom = NetworkProxySettings {
            mode: ProxyMode::Custom,
            custom_proxy_url: Some("http://127.0.0.1:7890".to_string()),
            ..NetworkProxySettings::default()
        };

        let error = save_proxy_settings_with(
            custom.clone(),
            |settings| activated.lock().expect("settings").push(settings),
            |_| {
                Err(AppError::Io {
                    message: "write failed".to_string(),
                })
            },
        )
        .expect_err("persist failure");
        assert!(matches!(error, AppError::Io { .. }));
        assert!(activated.lock().expect("settings").is_empty());

        let saved = save_proxy_settings_with(
            custom,
            |settings| activated.lock().expect("settings").push(settings),
            |_| Ok(()),
        )
        .expect("persisted settings");
        assert_eq!(saved.mode, ProxyMode::Custom);
        assert_eq!(activated.lock().expect("settings").as_slice(), [saved]);
    }
}

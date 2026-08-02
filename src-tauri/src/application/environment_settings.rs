use crate::application::install_wizard_session::InstallWizardSessionController;
use crate::core::mutation::SingleMutationController;
use crate::core::update_config;
use crate::environment::project_service::{self, EnvironmentDiscoverySnapshot};
use crate::environment::wsl::EnvironmentRegistry;
use crate::error::AppError;
use crate::models::SkillDeckConfig;

fn persist_wsl_integration_setting_with<Persist>(
    enabled: bool,
    environments: &EnvironmentRegistry,
    persist: Persist,
) -> Result<(), AppError>
where
    Persist: FnOnce() -> Result<(), AppError>,
{
    persist()?;
    environments.set_wsl_integration_enabled(enabled);
    Ok(())
}

fn merge_config_preserving_wsl_setting(
    mut config: SkillDeckConfig,
    persisted: &mut SkillDeckConfig,
) {
    config.wsl_integration_enabled = persisted.wsl_integration_enabled;
    *persisted = config;
}

pub fn save_config_preserving_wsl_setting(config: SkillDeckConfig) -> Result<(), AppError> {
    update_config(move |persisted| merge_config_preserving_wsl_setting(config, persisted))?;
    Ok(())
}

pub async fn set_wsl_integration_enabled(
    enabled: bool,
    environments: &EnvironmentRegistry,
    mutation: &SingleMutationController,
    install_wizard_session: &InstallWizardSessionController,
) -> Result<EnvironmentDiscoverySnapshot, AppError> {
    if enabled && !cfg!(target_os = "windows") {
        return Err(AppError::CapabilityUnavailable {
            capability: "wslIntegration".to_string(),
            path: None,
        });
    }
    if install_wizard_session.is_active() {
        return Err(AppError::MutationBusy);
    }

    mutation
        .with_idle(|| {
            persist_wsl_integration_setting_with(enabled, environments, || {
                update_config(|config| config.wsl_integration_enabled = enabled)?;
                Ok(())
            })
        })
        .map_err(|_| AppError::MutationBusy)??;

    project_service::list_environments(environments).await
}

#[cfg(test)]
mod tests {
    use super::{merge_config_preserving_wsl_setting, persist_wsl_integration_setting_with};
    use crate::environment::wsl::EnvironmentRegistry;
    use crate::error::AppError;
    use crate::models::SkillDeckConfig;

    #[test]
    fn wsl_setting_updates_runtime_only_after_config_is_persisted() {
        let registry = EnvironmentRegistry::default();

        persist_wsl_integration_setting_with(false, &registry, || {
            assert!(registry.wsl_integration_enabled());
            Ok(())
        })
        .expect("persist setting");

        assert!(!registry.wsl_integration_enabled());
    }

    #[test]
    fn failed_wsl_setting_write_keeps_runtime_state() {
        let registry = EnvironmentRegistry::default();

        let error = persist_wsl_integration_setting_with(false, &registry, || {
            Err(AppError::Io {
                message: "write failed".to_string(),
            })
        })
        .expect_err("write failure");

        assert!(matches!(error, AppError::Io { .. }));
        assert!(registry.wsl_integration_enabled());
    }

    #[test]
    fn generic_config_save_cannot_change_the_wsl_setting() {
        let incoming = SkillDeckConfig {
            wsl_integration_enabled: true,
            ..SkillDeckConfig::default()
        };
        let mut persisted = SkillDeckConfig::default();

        merge_config_preserving_wsl_setting(incoming, &mut persisted);

        assert!(!persisted.wsl_integration_enabled);
    }
}

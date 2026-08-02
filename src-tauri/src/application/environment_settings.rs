use crate::application::runtime_admission::{AdmissionDenied, RuntimeAdmissionCoordinator};
use crate::core::update_config;
use crate::environment::project_service::{self, EnvironmentDiscoverySnapshot};
use crate::environment::wsl::EnvironmentRegistry;
use crate::error::{AppError, WslIntegrationBusyReason};
use crate::models::SkillDeckConfig;

const WSL_QUIESCENCE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

async fn apply_wsl_integration_setting_with<Persist>(
    enabled: bool,
    environments: &EnvironmentRegistry,
    admission: &RuntimeAdmissionCoordinator,
    quiescence_timeout: std::time::Duration,
    persist: Persist,
) -> Result<(), AppError>
where
    Persist: FnOnce(bool) -> Result<(), AppError>,
{
    let _permit = admission
        .begin_wsl_integration_change()
        .map_err(map_admission_error)?;
    if environments.wsl_integration_enabled() == enabled {
        return persist(enabled);
    }
    if enabled {
        let transition = environments.begin_enable()?;
        persist(true)?;
        transition.commit_enabled();
        return Ok(());
    }

    let transition = environments.begin_disable()?;
    if transition
        .wait_for_quiescence(quiescence_timeout)
        .await
        .is_err()
    {
        return Err(AppError::WslIntegrationBusy {
            reason: WslIntegrationBusyReason::WslOperation,
        });
    }
    persist(false)?;
    transition.commit_disabled();
    Ok(())
}

fn map_admission_error(error: AdmissionDenied) -> AppError {
    match error {
        AdmissionDenied::Mutation => AppError::WslIntegrationBusy {
            reason: WslIntegrationBusyReason::Mutation,
        },
        AdmissionDenied::Lifecycle => AppError::WslIntegrationBusy {
            reason: WslIntegrationBusyReason::Lifecycle,
        },
        AdmissionDenied::InstallWizard => AppError::WslIntegrationBusy {
            reason: WslIntegrationBusyReason::InstallWizard,
        },
        AdmissionDenied::WslSettingChange => AppError::WslIntegrationBusy {
            reason: WslIntegrationBusyReason::WslOperation,
        },
        AdmissionDenied::ApplicationTerminating => AppError::ApplicationTerminating,
    }
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
    admission: &RuntimeAdmissionCoordinator,
) -> Result<EnvironmentDiscoverySnapshot, AppError> {
    if enabled && !cfg!(target_os = "windows") {
        return Err(AppError::CapabilityUnavailable {
            capability: "wslIntegration".to_string(),
            path: None,
        });
    }
    apply_wsl_integration_setting_with(
        enabled,
        environments,
        admission,
        WSL_QUIESCENCE_TIMEOUT,
        |enabled| {
            update_config(|config| config.wsl_integration_enabled = enabled)?;
            Ok(())
        },
    )
    .await?;

    project_service::list_environments(environments).await
}

#[cfg(test)]
mod tests {
    use super::{apply_wsl_integration_setting_with, merge_config_preserving_wsl_setting};
    use crate::application::runtime_admission::RuntimeAdmissionCoordinator;
    use crate::environment::wsl::EnvironmentRegistry;
    use crate::error::AppError;
    use crate::models::SkillDeckConfig;

    #[tokio::test]
    async fn wsl_setting_updates_runtime_only_after_config_is_persisted() {
        let registry = EnvironmentRegistry::default();
        let admission = RuntimeAdmissionCoordinator::default();

        apply_wsl_integration_setting_with(
            false,
            &registry,
            &admission,
            std::time::Duration::from_secs(1),
            |_| {
                assert!(registry.wsl_integration_enabled());
                Ok(())
            },
        )
        .await
        .expect("persist setting");

        assert!(!registry.wsl_integration_enabled());
        assert_eq!(registry.capability_revision(), 1);
    }

    #[tokio::test]
    async fn failed_wsl_setting_write_keeps_runtime_state() {
        let registry = EnvironmentRegistry::default();
        let admission = RuntimeAdmissionCoordinator::default();

        let error = apply_wsl_integration_setting_with(
            false,
            &registry,
            &admission,
            std::time::Duration::from_secs(1),
            |_| {
                Err(AppError::Io {
                    message: "write failed".to_string(),
                })
            },
        )
        .await
        .expect_err("write failure");

        assert!(matches!(error, AppError::Io { .. }));
        assert!(registry.wsl_integration_enabled());
    }

    #[tokio::test]
    async fn failed_enable_write_rolls_runtime_back_to_disabled() {
        let registry = EnvironmentRegistry::new(false);
        let admission = RuntimeAdmissionCoordinator::default();

        let error = apply_wsl_integration_setting_with(
            true,
            &registry,
            &admission,
            std::time::Duration::from_secs(1),
            |_| {
                Err(AppError::Io {
                    message: "write failed".to_string(),
                })
            },
        )
        .await
        .expect_err("write failure");

        assert!(matches!(error, AppError::Io { .. }));
        assert!(!registry.wsl_integration_enabled());
        assert_eq!(registry.capability_revision(), 0);
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

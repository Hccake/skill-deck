use crate::application::payload_session::PayloadSessionManager;
use crate::application::runtime_admission::{AdmissionDenied, RuntimeAdmissionCoordinator};
use crate::core::update_config;
use crate::environment::project_service::{self, EnvironmentDiscoverySnapshot};
use crate::environment::wsl::WslRuntime;
use crate::error::{AppError, WslIntegrationBusyReason};
use crate::models::SkillDeckConfig;

const WSL_QUIESCENCE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

async fn apply_wsl_integration_setting_with<Persist>(
    enabled: bool,
    environments: &WslRuntime,
    quiescence_timeout: std::time::Duration,
    persist: Persist,
) -> Result<(), AppError>
where
    Persist: FnOnce(bool) -> Result<(), AppError>,
{
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

async fn set_wsl_integration_enabled_with<T, Persist, Retire, Snapshot, SnapshotFuture>(
    enabled: bool,
    environments: &WslRuntime,
    admission: &RuntimeAdmissionCoordinator,
    quiescence_timeout: std::time::Duration,
    persist: Persist,
    retire: Retire,
    snapshot: Snapshot,
) -> Result<T, AppError>
where
    Persist: FnOnce(bool) -> Result<(), AppError>,
    Retire: FnOnce(),
    Snapshot: FnOnce() -> SnapshotFuture,
    SnapshotFuture: std::future::Future<Output = Result<T, AppError>>,
{
    let _permit = admission
        .begin_wsl_integration_change()
        .map_err(map_admission_error)?;

    if !enabled && environments.wsl_integration_enabled() {
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
        retire();
        if transition
            .wait_for_source_retirement(quiescence_timeout)
            .await
            .is_err()
        {
            return Err(AppError::WslIntegrationBusy {
                reason: WslIntegrationBusyReason::WslOperation,
            });
        }
        persist(false)?;
        transition.flush_deferred_source_cleanups().await;
        transition.commit_disabled();
    } else {
        apply_wsl_integration_setting_with(enabled, environments, quiescence_timeout, persist)
            .await?;
        if !enabled {
            retire();
        }
    }
    snapshot().await
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
    config.network_proxy = persisted.network_proxy.clone();
    *persisted = config;
}

pub fn save_config_preserving_wsl_setting(config: SkillDeckConfig) -> Result<(), AppError> {
    update_config(move |persisted| merge_config_preserving_wsl_setting(config, persisted))?;
    Ok(())
}

pub struct WslIntegrationSettings<'a> {
    runtime: &'a WslRuntime,
    admission: &'a RuntimeAdmissionCoordinator,
    payloads: &'a PayloadSessionManager,
}

impl<'a> WslIntegrationSettings<'a> {
    pub fn new(
        runtime: &'a WslRuntime,
        admission: &'a RuntimeAdmissionCoordinator,
        payloads: &'a PayloadSessionManager,
    ) -> Self {
        Self {
            runtime,
            admission,
            payloads,
        }
    }

    pub async fn set_enabled(
        &self,
        enabled: bool,
    ) -> Result<EnvironmentDiscoverySnapshot, AppError> {
        if enabled && !cfg!(target_os = "windows") {
            return Err(AppError::CapabilityUnavailable {
                capability: "wslIntegration".to_string(),
                path: None,
            });
        }
        set_wsl_integration_enabled_with(
            enabled,
            self.runtime,
            self.admission,
            WSL_QUIESCENCE_TIMEOUT,
            |enabled| {
                update_config(|config| config.wsl_integration_enabled = enabled)?;
                Ok(())
            },
            || {
                self.payloads.retire_wsl_sessions();
            },
            || project_service::list_environments(self.runtime),
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::{
        apply_wsl_integration_setting_with, map_admission_error,
        merge_config_preserving_wsl_setting, set_wsl_integration_enabled_with,
    };
    use crate::application::runtime_admission::AdmissionDenied;
    use crate::application::runtime_admission::RuntimeAdmissionCoordinator;
    use crate::environment::wsl::WslRuntime;
    use crate::error::{AppError, WslIntegrationBusyReason};
    use crate::models::SkillDeckConfig;

    #[tokio::test]
    async fn wsl_setting_updates_runtime_only_after_config_is_persisted() {
        let registry = WslRuntime::default();
        apply_wsl_integration_setting_with(
            false,
            &registry,
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
        let registry = WslRuntime::default();
        let error = apply_wsl_integration_setting_with(
            false,
            &registry,
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
        let registry = WslRuntime::new(false);
        let error = apply_wsl_integration_setting_with(
            true,
            &registry,
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

    #[test]
    fn generic_config_save_cannot_change_proxy_settings() {
        use crate::models::{NetworkProxySettings, ProxyMode};

        let incoming = SkillDeckConfig {
            network_proxy: NetworkProxySettings {
                mode: ProxyMode::Direct,
                ..NetworkProxySettings::default()
            },
            ..SkillDeckConfig::default()
        };
        let expected = NetworkProxySettings {
            mode: ProxyMode::Custom,
            custom_proxy_url: Some("http://127.0.0.1:7890".to_string()),
            ..NetworkProxySettings::default()
        };
        let mut persisted = SkillDeckConfig {
            network_proxy: expected.clone(),
            ..SkillDeckConfig::default()
        };

        merge_config_preserving_wsl_setting(incoming, &mut persisted);

        assert_eq!(persisted.network_proxy, expected);
    }

    #[test]
    fn admission_errors_have_stable_public_error_mappings() {
        let cases = [
            (
                AdmissionDenied::Mutation,
                AppError::WslIntegrationBusy {
                    reason: WslIntegrationBusyReason::Mutation,
                },
            ),
            (
                AdmissionDenied::Lifecycle,
                AppError::WslIntegrationBusy {
                    reason: WslIntegrationBusyReason::Lifecycle,
                },
            ),
            (
                AdmissionDenied::InstallWizard,
                AppError::WslIntegrationBusy {
                    reason: WslIntegrationBusyReason::InstallWizard,
                },
            ),
            (
                AdmissionDenied::WslSettingChange,
                AppError::WslIntegrationBusy {
                    reason: WslIntegrationBusyReason::WslOperation,
                },
            ),
            (
                AdmissionDenied::ApplicationTerminating,
                AppError::ApplicationTerminating,
            ),
        ];

        for (denied, expected) in cases {
            assert_eq!(map_admission_error(denied), expected);
        }
    }

    #[tokio::test]
    async fn setting_permit_is_held_until_the_authoritative_snapshot_returns() {
        let environments = WslRuntime::default();
        let admission = RuntimeAdmissionCoordinator::default();
        let task_environments = environments.clone();
        let task_admission = admission.clone();
        let (snapshot_started_tx, snapshot_started_rx) = tokio::sync::oneshot::channel();
        let (release_snapshot_tx, release_snapshot_rx) = tokio::sync::oneshot::channel();

        let task = tokio::spawn(async move {
            set_wsl_integration_enabled_with(
                false,
                &task_environments,
                &task_admission,
                std::time::Duration::from_secs(1),
                |_| Ok(()),
                || {},
                || async move {
                    snapshot_started_tx.send(()).expect("snapshot started");
                    release_snapshot_rx.await.expect("release snapshot");
                    Ok(())
                },
            )
            .await
        });

        snapshot_started_rx.await.expect("snapshot phase reached");
        assert_eq!(
            admission.begin_wsl_integration_change().unwrap_err(),
            AdmissionDenied::WslSettingChange
        );
        release_snapshot_tx.send(()).expect("release snapshot");
        task.await.expect("setting task").expect("setting change");
        assert!(admission.begin_wsl_integration_change().is_ok());
    }

    #[tokio::test]
    async fn disabling_retires_wsl_payloads_before_committing_the_runtime_cycle() {
        let environments = WslRuntime::default();
        let admission = RuntimeAdmissionCoordinator::default();

        set_wsl_integration_enabled_with(
            false,
            &environments,
            &admission,
            std::time::Duration::from_secs(1),
            |_| Ok(()),
            || assert!(environments.wsl_integration_enabled()),
            || async {
                assert!(!environments.wsl_integration_enabled());
                Ok(())
            },
        )
        .await
        .expect("disable WSL integration");
    }
}

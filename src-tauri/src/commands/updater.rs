use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use serde::Serialize;
use specta::Type;
use tauri::ipc::Channel;
use tauri::{AppHandle, State};
use tauri_plugin_updater::UpdaterExt;

use crate::application::runtime_admission::RuntimeAdmissionCoordinator;
use crate::core::mutation::LifecycleLeaseKind;
use crate::error::AppError;
use crate::runtime::RuntimeServiceGraph;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct ApplicationUpdateInfo {
    pub version: String,
    pub body: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(tag = "event", content = "data", rename_all = "camelCase")]
#[specta(tag = "event", content = "data", rename_all = "camelCase")]
pub enum ApplicationUpdateProgress {
    Started { content_length: Option<u64> },
    Progress { chunk_length: u64 },
    Finished,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct ApplicationUpdateResult {
    pub version: String,
    pub installed: bool,
}

pub type UpdaterFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

pub trait ApplicationUpdater: Send + Sync {
    fn check<'a>(&'a self) -> UpdaterFuture<'a, Result<Option<ApplicationUpdateInfo>, AppError>>;

    fn download_and_install<'a>(
        &'a self,
        expected_version: &'a str,
        progress: Arc<dyn Fn(ApplicationUpdateProgress) + Send + Sync>,
    ) -> UpdaterFuture<'a, Result<(), AppError>>;
}

struct TauriApplicationUpdater {
    app: AppHandle,
}

impl ApplicationUpdater for TauriApplicationUpdater {
    fn check<'a>(&'a self) -> UpdaterFuture<'a, Result<Option<ApplicationUpdateInfo>, AppError>> {
        Box::pin(async move {
            let updater = self.app.updater().map_err(updater_error)?;
            Ok(updater
                .check()
                .await
                .map_err(updater_error)?
                .map(|update| ApplicationUpdateInfo {
                    version: update.version,
                    body: update.body,
                }))
        })
    }

    fn download_and_install<'a>(
        &'a self,
        expected_version: &'a str,
        progress: Arc<dyn Fn(ApplicationUpdateProgress) + Send + Sync>,
    ) -> UpdaterFuture<'a, Result<(), AppError>> {
        Box::pin(async move {
            let updater = self.app.updater().map_err(updater_error)?;
            let update = updater
                .check()
                .await
                .map_err(updater_error)?
                .ok_or_else(no_update)?;
            if update.version != expected_version {
                return Err(AppError::Validation {
                    field: Some("expectedVersion".to_string()),
                    message: "available application update changed".to_string(),
                });
            }
            let started = Arc::new(AtomicBool::new(false));
            let chunk_started = Arc::clone(&started);
            let chunk_progress = Arc::clone(&progress);
            let finish_started = Arc::clone(&started);
            let finish_progress = Arc::clone(&progress);
            update
                .download_and_install(
                    move |chunk_length, content_length| {
                        if !chunk_started.swap(true, Ordering::AcqRel) {
                            chunk_progress(ApplicationUpdateProgress::Started { content_length });
                        }
                        chunk_progress(ApplicationUpdateProgress::Progress {
                            chunk_length: chunk_length.try_into().unwrap_or(u64::MAX),
                        });
                    },
                    move || {
                        if !finish_started.swap(true, Ordering::AcqRel) {
                            finish_progress(ApplicationUpdateProgress::Started {
                                content_length: None,
                            });
                        }
                        finish_progress(ApplicationUpdateProgress::Finished);
                    },
                )
                .await
                .map_err(updater_error)
        })
    }
}

#[tauri::command]
#[specta::specta]
pub async fn check_application_update(
    app: AppHandle,
) -> Result<Option<ApplicationUpdateInfo>, AppError> {
    TauriApplicationUpdater { app }.check().await
}

#[tauri::command]
#[specta::specta]
pub async fn download_and_install_application_update(
    app: AppHandle,
    runtime: State<'_, RuntimeServiceGraph>,
    expected_version: String,
    progress: Channel<ApplicationUpdateProgress>,
) -> Result<ApplicationUpdateResult, AppError> {
    download_and_install_with(
        runtime.admission(),
        &TauriApplicationUpdater { app },
        &expected_version,
        Arc::new(move |event| {
            let _ = progress.send(event);
        }),
    )
    .await
}

async fn download_and_install_with<U: ApplicationUpdater>(
    controller: &RuntimeAdmissionCoordinator,
    updater: &U,
    expected_version: &str,
    progress: Arc<dyn Fn(ApplicationUpdateProgress) + Send + Sync>,
) -> Result<ApplicationUpdateResult, AppError> {
    let _lease = controller.begin_lifecycle(LifecycleLeaseKind::ApplicationUpdate)?;
    let update = updater.check().await?.ok_or_else(no_update)?;
    if update.version != expected_version {
        return Err(AppError::Validation {
            field: Some("expectedVersion".to_string()),
            message: "available application update changed".to_string(),
        });
    }
    updater
        .download_and_install(expected_version, progress)
        .await?;
    Ok(ApplicationUpdateResult {
        version: update.version,
        installed: true,
    })
}

fn no_update() -> AppError {
    AppError::Validation {
        field: Some("expectedVersion".to_string()),
        message: "no application update is currently available".to_string(),
    }
}

fn updater_error(error: impl std::fmt::Display) -> AppError {
    AppError::ExecutionFailed {
        message: error.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    use super::*;
    use crate::application::runtime_admission::{
        RuntimeAdmissionCoordinator, WizardAdmission, WizardWindowPresence,
    };
    use crate::core::mutation::MutationKind;
    use crate::environment::types::{EnvironmentRef, SkillLocation, SkillLocationRef};

    struct FakeUpdater {
        version: String,
        installed: Arc<Mutex<bool>>,
        observed_lease: Arc<Mutex<bool>>,
        check_calls: Arc<AtomicUsize>,
        download_calls: Arc<AtomicUsize>,
        controller: Arc<RuntimeAdmissionCoordinator>,
    }

    impl ApplicationUpdater for FakeUpdater {
        fn check<'a>(
            &'a self,
        ) -> UpdaterFuture<'a, Result<Option<ApplicationUpdateInfo>, AppError>> {
            Box::pin(async move {
                self.check_calls.fetch_add(1, Ordering::SeqCst);
                Ok(Some(ApplicationUpdateInfo {
                    version: self.version.clone(),
                    body: None,
                }))
            })
        }

        fn download_and_install<'a>(
            &'a self,
            expected_version: &'a str,
            _progress: Arc<dyn Fn(ApplicationUpdateProgress) + Send + Sync>,
        ) -> UpdaterFuture<'a, Result<(), AppError>> {
            Box::pin(async move {
                self.download_calls.fetch_add(1, Ordering::SeqCst);
                assert_eq!(expected_version, self.version);
                *self.observed_lease.lock().unwrap() =
                    self.controller.activity_snapshot().lifecycle.is_some();
                assert!(matches!(
                    self.controller
                        .begin_mutation(MutationKind::Install, native_global()),
                    Err(AppError::MutationBusy)
                ));
                *self.installed.lock().unwrap() = true;
                Ok(())
            })
        }
    }

    #[tokio::test]
    async fn combined_update_holds_one_backend_lease_until_install_returns() {
        let controller = Arc::new(RuntimeAdmissionCoordinator::default());
        let installed = Arc::new(Mutex::new(false));
        let observed_lease = Arc::new(Mutex::new(false));
        let updater = FakeUpdater {
            version: "2.0.0".to_string(),
            installed: Arc::clone(&installed),
            observed_lease: Arc::clone(&observed_lease),
            check_calls: Arc::new(AtomicUsize::new(0)),
            download_calls: Arc::new(AtomicUsize::new(0)),
            controller: Arc::clone(&controller),
        };

        let result =
            download_and_install_with(controller.as_ref(), &updater, "2.0.0", Arc::new(|_| {}))
                .await
                .expect("update");

        assert!(result.installed);
        assert!(*installed.lock().unwrap());
        assert!(*observed_lease.lock().unwrap());
        assert!(controller.activity_snapshot().lifecycle.is_none());
    }

    #[tokio::test]
    async fn version_mismatch_does_not_install() {
        let controller = Arc::new(RuntimeAdmissionCoordinator::default());
        let installed = Arc::new(Mutex::new(false));
        let updater = FakeUpdater {
            version: "2.0.1".to_string(),
            installed: Arc::clone(&installed),
            observed_lease: Arc::new(Mutex::new(false)),
            check_calls: Arc::new(AtomicUsize::new(0)),
            download_calls: Arc::new(AtomicUsize::new(0)),
            controller: Arc::clone(&controller),
        };

        assert!(download_and_install_with(
            controller.as_ref(),
            &updater,
            "2.0.0",
            Arc::new(|_| {}),
        )
        .await
        .is_err());
        assert!(!*installed.lock().unwrap());
        assert!(controller.activity_snapshot().lifecycle.is_none());
    }

    #[tokio::test]
    async fn wizard_session_rejects_update_before_check_or_download() {
        let controller = Arc::new(RuntimeAdmissionCoordinator::default());
        let WizardAdmission::Reserved(_reservation) = controller
            .admit_install_wizard(WizardWindowPresence::Absent)
            .expect("wizard reservation")
        else {
            panic!("expected wizard reservation");
        };
        let check_calls = Arc::new(AtomicUsize::new(0));
        let download_calls = Arc::new(AtomicUsize::new(0));
        let updater = FakeUpdater {
            version: "2.0.0".to_string(),
            installed: Arc::new(Mutex::new(false)),
            observed_lease: Arc::new(Mutex::new(false)),
            check_calls: Arc::clone(&check_calls),
            download_calls: Arc::clone(&download_calls),
            controller: Arc::clone(&controller),
        };

        let error =
            download_and_install_with(controller.as_ref(), &updater, "2.0.0", Arc::new(|_| {}))
                .await
                .expect_err("wizard must block application update");

        assert_eq!(error, AppError::InstallWizardActive);
        assert_eq!(check_calls.load(Ordering::SeqCst), 0);
        assert_eq!(download_calls.load(Ordering::SeqCst), 0);
    }

    fn native_global() -> SkillLocationRef {
        SkillLocationRef {
            environment: EnvironmentRef::Native,
            scope: SkillLocation::Global,
        }
    }
}

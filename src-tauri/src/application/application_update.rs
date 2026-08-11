use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use serde::Serialize;
use specta::Type;

use crate::application::runtime_admission::RuntimeAdmissionCoordinator;
use crate::core::mutation::LifecycleLeaseKind;
use crate::error::AppError;

const UPDATE_CHECK_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);
const UPDATE_DOWNLOAD_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30 * 60);

#[derive(Clone, Copy)]
pub(crate) struct ApplicationUpdateLimits {
    pub(crate) check_timeout: std::time::Duration,
    pub(crate) download_timeout: std::time::Duration,
}

impl Default for ApplicationUpdateLimits {
    fn default() -> Self {
        Self {
            check_timeout: UPDATE_CHECK_TIMEOUT,
            download_timeout: UPDATE_DOWNLOAD_TIMEOUT,
        }
    }
}

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
    Downloaded,
    Installing,
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

pub(crate) trait ApplicationUpdater: Send + Sync {
    fn check<'a>(
        &'a self,
        limits: ApplicationUpdateLimits,
    ) -> UpdaterFuture<'a, Result<Option<ApplicationUpdateInfo>, AppError>>;

    fn download_and_install<'a>(
        &'a self,
        expected_version: &'a str,
        progress: Arc<dyn Fn(ApplicationUpdateProgress) + Send + Sync>,
        limits: ApplicationUpdateLimits,
    ) -> UpdaterFuture<'a, Result<ApplicationUpdateInfo, AppError>>;
}

pub(crate) struct ApplicationUpdateCoordinator<'a> {
    admission: &'a RuntimeAdmissionCoordinator,
    limits: ApplicationUpdateLimits,
}

impl<'a> ApplicationUpdateCoordinator<'a> {
    pub fn new(admission: &'a RuntimeAdmissionCoordinator) -> Self {
        Self {
            admission,
            limits: ApplicationUpdateLimits::default(),
        }
    }

    pub async fn check<U: ApplicationUpdater>(
        &self,
        updater: &U,
    ) -> Result<Option<ApplicationUpdateInfo>, AppError> {
        updater.check(self.limits).await
    }

    pub async fn download_and_install<U: ApplicationUpdater>(
        &self,
        updater: &U,
        expected_version: &str,
        progress: Arc<dyn Fn(ApplicationUpdateProgress) + Send + Sync>,
    ) -> Result<ApplicationUpdateResult, AppError> {
        let lease = self
            .admission
            .begin_cancelable_lifecycle(LifecycleLeaseKind::ApplicationUpdate)?;
        let cancellation = lease.cancellation();
        let lifecycle = self.admission.clone();
        let client_progress = progress;
        let progress = Arc::new(move |event: ApplicationUpdateProgress| {
            match &event {
                ApplicationUpdateProgress::Started { .. }
                | ApplicationUpdateProgress::Progress { .. } => {
                    lifecycle.set_application_update_cancelable(true);
                }
                ApplicationUpdateProgress::Downloaded
                | ApplicationUpdateProgress::Installing
                | ApplicationUpdateProgress::Finished => {
                    lifecycle.set_application_update_cancelable(false);
                }
            }
            client_progress(event);
        });
        self.admission.set_application_update_cancelable(true);
        let download = updater.download_and_install(expected_version, progress, self.limits);
        tokio::pin!(download);
        let update = tokio::select! {
            result = &mut download => result,
            () = cancellation.cancelled() => Err(AppError::MutationCancelled),
        };
        self.admission.set_application_update_cancelable(false);
        let update = update?;
        Ok(ApplicationUpdateResult {
            version: update.version,
            installed: true,
        })
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
        download_calls: Arc<AtomicUsize>,
        controller: Arc<RuntimeAdmissionCoordinator>,
    }

    impl ApplicationUpdater for FakeUpdater {
        fn check<'a>(
            &'a self,
            _limits: ApplicationUpdateLimits,
        ) -> UpdaterFuture<'a, Result<Option<ApplicationUpdateInfo>, AppError>> {
            Box::pin(async move {
                Ok(Some(ApplicationUpdateInfo {
                    version: self.version.clone(),
                    body: None,
                }))
            })
        }

        fn download_and_install<'a>(
            &'a self,
            expected_version: &'a str,
            progress: Arc<dyn Fn(ApplicationUpdateProgress) + Send + Sync>,
            _limits: ApplicationUpdateLimits,
        ) -> UpdaterFuture<'a, Result<ApplicationUpdateInfo, AppError>> {
            Box::pin(async move {
                self.download_calls.fetch_add(1, Ordering::SeqCst);
                if expected_version != self.version {
                    return Err(AppError::Validation {
                        field: Some("expectedVersion".to_string()),
                        message: "available application update changed".to_string(),
                    });
                }
                *self.observed_lease.lock().unwrap() =
                    self.controller.activity_snapshot().lifecycle.is_some();
                assert!(
                    self.controller
                        .activity_snapshot()
                        .lifecycle
                        .expect("application update lifecycle")
                        .cancelable
                );
                assert!(matches!(
                    self.controller
                        .begin_mutation(MutationKind::Install, native_global()),
                    Err(AppError::MutationBusy)
                ));
                progress(ApplicationUpdateProgress::Started {
                    content_length: Some(100),
                });
                progress(ApplicationUpdateProgress::Downloaded);
                assert!(
                    !self
                        .controller
                        .activity_snapshot()
                        .lifecycle
                        .expect("application update lifecycle")
                        .cancelable
                );
                *self.installed.lock().unwrap() = true;
                Ok(ApplicationUpdateInfo {
                    version: self.version.clone(),
                    body: None,
                })
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
            download_calls: Arc::new(AtomicUsize::new(0)),
            controller: Arc::clone(&controller),
        };

        let result = ApplicationUpdateCoordinator::new(controller.as_ref())
            .download_and_install(&updater, "2.0.0", Arc::new(|_| {}))
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
            download_calls: Arc::new(AtomicUsize::new(0)),
            controller: Arc::clone(&controller),
        };

        assert!(ApplicationUpdateCoordinator::new(controller.as_ref())
            .download_and_install(&updater, "2.0.0", Arc::new(|_| {}))
            .await
            .is_err());
        assert!(!*installed.lock().unwrap());
        assert!(controller.activity_snapshot().lifecycle.is_none());
    }

    #[tokio::test]
    async fn wizard_session_rejects_update_before_download() {
        let controller = Arc::new(RuntimeAdmissionCoordinator::default());
        let WizardAdmission::Reserved(_reservation) = controller
            .admit_install_wizard(WizardWindowPresence::Absent)
            .expect("wizard reservation")
        else {
            panic!("expected wizard reservation");
        };
        let download_calls = Arc::new(AtomicUsize::new(0));
        let updater = FakeUpdater {
            version: "2.0.0".to_string(),
            installed: Arc::new(Mutex::new(false)),
            observed_lease: Arc::new(Mutex::new(false)),
            download_calls: Arc::clone(&download_calls),
            controller: Arc::clone(&controller),
        };

        let error = ApplicationUpdateCoordinator::new(controller.as_ref())
            .download_and_install(&updater, "2.0.0", Arc::new(|_| {}))
            .await
            .expect_err("wizard must block application update");

        assert_eq!(error, AppError::InstallWizardActive);
        assert_eq!(download_calls.load(Ordering::SeqCst), 0);
    }

    struct PendingUpdater;

    impl ApplicationUpdater for PendingUpdater {
        fn check<'a>(
            &'a self,
            _limits: ApplicationUpdateLimits,
        ) -> UpdaterFuture<'a, Result<Option<ApplicationUpdateInfo>, AppError>> {
            Box::pin(async { Ok(None) })
        }

        fn download_and_install<'a>(
            &'a self,
            _expected_version: &'a str,
            _progress: Arc<dyn Fn(ApplicationUpdateProgress) + Send + Sync>,
            _limits: ApplicationUpdateLimits,
        ) -> UpdaterFuture<'a, Result<ApplicationUpdateInfo, AppError>> {
            Box::pin(std::future::pending())
        }
    }

    #[tokio::test]
    async fn download_can_be_cancelled_before_the_first_progress_event() {
        let controller = Arc::new(RuntimeAdmissionCoordinator::default());
        let cancellation_controller = Arc::clone(&controller);
        let cancel = tokio::spawn(async move {
            tokio::task::yield_now().await;
            assert!(cancellation_controller
                .request_cancel_lifecycle()
                .expect("cancel request"));
        });

        let result = ApplicationUpdateCoordinator::new(controller.as_ref())
            .download_and_install(&PendingUpdater, "2.0.0", Arc::new(|_| {}))
            .await;

        cancel.await.expect("cancel task");
        assert_eq!(result, Err(AppError::MutationCancelled));
        assert!(controller.activity_snapshot().lifecycle.is_none());
    }

    fn native_global() -> SkillLocationRef {
        SkillLocationRef {
            environment: EnvironmentRef::Native,
            scope: SkillLocation::Global,
        }
    }
}

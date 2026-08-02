use std::sync::{Arc, Mutex};

use crate::application::install_wizard_session::InstallWizardSessionSnapshot;
use crate::application::runtime_admission::{
    AdmissionDenied, RuntimeAdmissionCoordinator, WizardAdmission, WizardWindowObservation,
    WizardWindowPresence,
};
use crate::error::AppError;

pub struct InstallWizardWindowRequest {
    pub query: String,
}

pub trait InstallWizardWindowAdapter {
    fn current_instance(&self) -> Option<String>;
    fn focus(&self, instance_id: &str) -> Result<bool, AppError>;
    fn create(
        &self,
        request: InstallWizardWindowRequest,
        instance_id: &str,
        on_destroyed: Arc<dyn Fn() + Send + Sync>,
    ) -> Result<(), AppError>;
}

pub struct InstallWizardWorkflow {
    admission: Arc<RuntimeAdmissionCoordinator>,
    tracked_instance: Mutex<Option<String>>,
}

impl InstallWizardWorkflow {
    pub fn new(admission: Arc<RuntimeAdmissionCoordinator>) -> Self {
        Self {
            admission,
            tracked_instance: Mutex::new(None),
        }
    }

    pub fn tracked_instance_id(&self) -> Option<String> {
        self.tracked_instance
            .lock()
            .expect("install wizard instance lock poisoned")
            .clone()
    }

    pub fn open_or_focus_install_wizard(
        self: &Arc<Self>,
        adapter: &dyn InstallWizardWindowAdapter,
        request: InstallWizardWindowRequest,
    ) -> Result<(), AppError> {
        let mut request = Some(request);
        for attempt in 0..=1 {
            let observed = adapter
                .current_instance()
                .map_or(WizardWindowPresence::Absent, |instance_id| {
                    WizardWindowPresence::Present { instance_id }
                });
            match self
                .admission
                .admit_install_wizard(observed)
                .map_err(admission_error)?
            {
                WizardAdmission::Existing { instance_id } => {
                    self.set_tracked_instance(Some(instance_id.clone()));
                    if adapter.focus(&instance_id)? {
                        return Ok(());
                    }
                    self.observe_destroyed(&instance_id);
                    if attempt == 1 {
                        return Err(AppError::Io {
                            message: "Install wizard window disappeared while focusing".to_string(),
                        });
                    }
                }
                WizardAdmission::Reserved(reservation) => {
                    let instance_id = uuid::Uuid::new_v4().to_string();
                    let workflow = Arc::clone(self);
                    let destroyed_instance = instance_id.clone();
                    self.set_tracked_instance(Some(instance_id.clone()));
                    if let Err(error) = adapter.create(
                        request.take().expect("wizard request consumed once"),
                        &instance_id,
                        Arc::new(move || {
                            workflow.observe_destroyed(&destroyed_instance);
                        }),
                    ) {
                        self.set_tracked_instance(None);
                        return Err(error);
                    }
                    if self.tracked_instance_id().as_deref() != Some(instance_id.as_str()) {
                        return Err(AppError::Io {
                            message: "Install wizard window closed during creation".to_string(),
                        });
                    }
                    reservation.activate(instance_id);
                    return Ok(());
                }
            }
        }
        unreachable!("install wizard workflow retries at most once")
    }

    pub fn reconcile_window(
        &self,
        observed_instance: Option<String>,
    ) -> InstallWizardSessionSnapshot {
        match observed_instance {
            Some(instance_id) => {
                self.set_tracked_instance(Some(instance_id.clone()));
                self.admission
                    .observe_install_wizard_window(WizardWindowObservation::Present { instance_id })
            }
            None => {
                let Some(instance_id) = self.tracked_instance_id() else {
                    return self.admission.install_wizard_snapshot();
                };
                self.observe_destroyed(&instance_id)
            }
        }
    }

    pub fn observe_destroyed(&self, instance_id: &str) -> InstallWizardSessionSnapshot {
        let mut tracked = self
            .tracked_instance
            .lock()
            .expect("install wizard instance lock poisoned");
        if tracked.as_deref() == Some(instance_id) {
            *tracked = None;
        }
        drop(tracked);
        self.admission
            .observe_install_wizard_window(WizardWindowObservation::Destroyed {
                instance_id: instance_id.to_string(),
            })
    }

    fn set_tracked_instance(&self, instance_id: Option<String>) {
        *self
            .tracked_instance
            .lock()
            .expect("install wizard instance lock poisoned") = instance_id;
    }
}

fn admission_error(error: AdmissionDenied) -> AppError {
    match error {
        AdmissionDenied::ApplicationTerminating => AppError::ApplicationTerminating,
        _ => AppError::MutationBusy,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;

    #[derive(Default)]
    struct FakeWindow {
        instance: Mutex<Option<String>>,
        focus_results: Mutex<Vec<bool>>,
        create_error: Mutex<Option<AppError>>,
        destroyed: Mutex<Option<Arc<dyn Fn() + Send + Sync>>>,
    }

    impl InstallWizardWindowAdapter for FakeWindow {
        fn current_instance(&self) -> Option<String> {
            self.instance.lock().expect("instance lock").clone()
        }

        fn focus(&self, _instance_id: &str) -> Result<bool, AppError> {
            let mut results = self.focus_results.lock().expect("focus results lock");
            let focused = if results.is_empty() {
                true
            } else {
                results.remove(0)
            };
            if !focused {
                *self.instance.lock().expect("instance lock") = None;
            }
            Ok(focused)
        }

        fn create(
            &self,
            _request: InstallWizardWindowRequest,
            instance_id: &str,
            on_destroyed: Arc<dyn Fn() + Send + Sync>,
        ) -> Result<(), AppError> {
            if let Some(error) = self.create_error.lock().expect("create error lock").take() {
                return Err(error);
            }
            *self.instance.lock().expect("instance lock") = Some(instance_id.to_string());
            *self.destroyed.lock().expect("destroyed lock") = Some(on_destroyed);
            Ok(())
        }
    }

    fn workflow() -> Arc<InstallWizardWorkflow> {
        Arc::new(InstallWizardWorkflow::new(Arc::new(
            RuntimeAdmissionCoordinator::default(),
        )))
    }

    #[test]
    fn failed_creation_releases_reservation() {
        let workflow = workflow();
        let adapter = FakeWindow::default();
        *adapter.create_error.lock().expect("create error lock") = Some(AppError::Io {
            message: "build failed".to_string(),
        });

        let error = workflow
            .open_or_focus_install_wizard(
                &adapter,
                InstallWizardWindowRequest {
                    query: "entryPoint=test".to_string(),
                },
            )
            .expect_err("creation fails");

        assert!(matches!(error, AppError::Io { .. }));
        assert!(!workflow.admission.install_wizard_snapshot().active);
        assert!(workflow.admission.begin_wsl_integration_change().is_ok());
    }

    #[test]
    fn stale_existing_window_is_reconciled_then_created_once() {
        let workflow = workflow();
        let adapter = FakeWindow::default();
        *adapter.instance.lock().expect("instance lock") = Some("stale".to_string());
        *adapter.focus_results.lock().expect("focus results lock") = vec![false];

        workflow
            .open_or_focus_install_wizard(
                &adapter,
                InstallWizardWindowRequest {
                    query: "entryPoint=test".to_string(),
                },
            )
            .expect("retry creates a window");

        assert_ne!(workflow.tracked_instance_id().as_deref(), Some("stale"));
        assert!(workflow.admission.install_wizard_snapshot().active);
    }

    #[test]
    fn old_destroy_callback_cannot_close_replacement_session() {
        let workflow = workflow();
        let first = FakeWindow::default();
        workflow
            .open_or_focus_install_wizard(
                &first,
                InstallWizardWindowRequest {
                    query: "first".to_string(),
                },
            )
            .expect("create first");
        let old_callback = first
            .destroyed
            .lock()
            .expect("destroyed lock")
            .clone()
            .expect("destroy callback");
        let old_instance = workflow.tracked_instance_id().expect("first instance");
        workflow.observe_destroyed(&old_instance);

        let second = FakeWindow::default();
        workflow
            .open_or_focus_install_wizard(
                &second,
                InstallWizardWindowRequest {
                    query: "second".to_string(),
                },
            )
            .expect("create second");
        old_callback();

        assert!(workflow.admission.install_wizard_snapshot().active);
        assert_ne!(
            workflow.tracked_instance_id().as_deref(),
            Some(old_instance.as_str())
        );
    }
}

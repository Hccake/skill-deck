use std::sync::{Arc, Mutex};

use serde::Serialize;
use specta::Type;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type, tauri_specta::Event)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct InstallWizardSessionSnapshot {
    pub revision: u32,
    pub active: bool,
}

#[derive(Default)]
struct SessionState {
    revision: u32,
    active: bool,
}

type SessionListener = Arc<dyn Fn(InstallWizardSessionSnapshot) + Send + Sync>;

#[derive(Default)]
pub struct InstallWizardSessionController {
    state: Mutex<SessionState>,
    listener: Mutex<Option<SessionListener>>,
}

impl InstallWizardSessionController {
    #[cfg(test)]
    fn snapshot(&self) -> InstallWizardSessionSnapshot {
        snapshot_from_state(
            &self
                .state
                .lock()
                .expect("install wizard session lock poisoned"),
        )
    }

    pub fn activate(&self) -> InstallWizardSessionSnapshot {
        self.transition(true)
    }

    pub fn deactivate(&self) -> InstallWizardSessionSnapshot {
        self.transition(false)
    }

    pub fn reconcile_window_presence(&self, window_exists: bool) -> InstallWizardSessionSnapshot {
        self.transition(window_exists)
    }

    pub fn set_listener(
        &self,
        listener: impl Fn(InstallWizardSessionSnapshot) + Send + Sync + 'static,
    ) {
        *self
            .listener
            .lock()
            .expect("install wizard session listener lock poisoned") = Some(Arc::new(listener));
    }

    fn transition(&self, active: bool) -> InstallWizardSessionSnapshot {
        let mut state = self
            .state
            .lock()
            .expect("install wizard session lock poisoned");
        if state.active == active {
            return snapshot_from_state(&state);
        }
        state.active = active;
        state.revision = state
            .revision
            .checked_add(1)
            .expect("install wizard session revision exhausted during one application run");
        let snapshot = snapshot_from_state(&state);
        drop(state);
        self.publish(snapshot.clone());
        snapshot
    }

    fn publish(&self, snapshot: InstallWizardSessionSnapshot) {
        let listener = self
            .listener
            .lock()
            .expect("install wizard session listener lock poisoned")
            .clone();
        if let Some(listener) = listener {
            listener(snapshot);
        }
    }
}

fn snapshot_from_state(state: &SessionState) -> InstallWizardSessionSnapshot {
    InstallWizardSessionSnapshot {
        revision: state.revision,
        active: state.active,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::{InstallWizardSessionController, InstallWizardSessionSnapshot};

    #[test]
    fn session_transitions_publish_revisioned_snapshots_once() {
        let controller = InstallWizardSessionController::default();
        let observed = Arc::new(Mutex::new(Vec::new()));
        let listener_observed = Arc::clone(&observed);
        controller.set_listener(move |snapshot| {
            listener_observed
                .lock()
                .expect("observed snapshots lock")
                .push(snapshot);
        });

        assert_eq!(
            controller.snapshot(),
            InstallWizardSessionSnapshot {
                revision: 0,
                active: false,
            }
        );

        controller.activate();
        controller.activate();
        controller.deactivate();
        controller.deactivate();

        assert_eq!(
            *observed.lock().expect("observed snapshots lock"),
            vec![
                InstallWizardSessionSnapshot {
                    revision: 1,
                    active: true,
                },
                InstallWizardSessionSnapshot {
                    revision: 2,
                    active: false,
                },
            ]
        );
        assert_eq!(controller.snapshot().revision, 2);
        assert!(!controller.snapshot().active);
    }

    #[test]
    fn window_presence_reconciliation_heals_missed_lifecycle_transitions() {
        let controller = InstallWizardSessionController::default();

        assert_eq!(
            controller.reconcile_window_presence(true),
            InstallWizardSessionSnapshot {
                revision: 1,
                active: true,
            }
        );
        assert_eq!(
            controller.reconcile_window_presence(false),
            InstallWizardSessionSnapshot {
                revision: 2,
                active: false,
            }
        );
        assert_eq!(
            controller.reconcile_window_presence(false),
            InstallWizardSessionSnapshot {
                revision: 2,
                active: false,
            }
        );
    }
}

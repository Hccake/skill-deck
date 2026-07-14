use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use serde::Serialize;
use specta::Type;

use crate::environment::types::ContextRef;
use crate::error::AppError;

pub const MUTATION_STATE_CHANGED_EVENT: &str = "mutation-state-changed";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub enum MutationKind {
    Install,
    Update,
    Remove,
    Copy,
    ManageAgents,
    DuplicateCleanup,
    #[allow(dead_code)]
    Repair,
    SaveAgentDefaults,
    BatchUpdate,
    ProjectMigration,
    AddProject,
    RemoveProject,
    UpdateProjectPreference,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub enum MutationPhase {
    Preparing,
    Acquiring,
    Materializing,
    Committing,
    #[allow(dead_code)]
    Finishing,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct MutationProgress {
    pub subject: Option<String>,
    pub current: Option<u32>,
    pub total: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct ActiveMutation {
    pub id: String,
    pub kind: MutationKind,
    pub context: ContextRef,
    pub phase: MutationPhase,
    pub progress: Option<MutationProgress>,
    pub cancelable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct MutationSnapshot {
    pub revision: u32,
    pub active: Option<ActiveMutation>,
}

#[derive(Clone, Default)]
pub struct CancellationSignal(Arc<AtomicBool>);

impl CancellationSignal {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Release);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

struct MutationState {
    active: ActiveMutation,
    cancellation: CancellationSignal,
}

#[derive(Default)]
struct ControllerState {
    revision: u32,
    mutation: Option<MutationState>,
}

#[derive(Default)]
pub struct SingleMutationController {
    state: Mutex<ControllerState>,
    listener: Mutex<Option<MutationListener>>,
}

type MutationListener = Arc<dyn Fn(MutationSnapshot) + Send + Sync>;

impl SingleMutationController {
    pub fn begin(
        &self,
        kind: MutationKind,
        context: ContextRef,
    ) -> Result<MutationGuard<'_>, AppError> {
        let mut state = self
            .state
            .lock()
            .expect("mutation controller lock poisoned");
        if state.mutation.is_some() {
            return Err(AppError::MutationBusy);
        }
        let cancellation = CancellationSignal::default();
        state.revision = next_revision(state.revision);
        state.mutation = Some(MutationState {
            active: ActiveMutation {
                id: uuid::Uuid::new_v4().to_string(),
                kind,
                context,
                phase: MutationPhase::Preparing,
                progress: None,
                cancelable: false,
            },
            cancellation: cancellation.clone(),
        });
        let snapshot = snapshot_from_state(&state);
        drop(state);
        self.publish(snapshot);
        Ok(MutationGuard {
            controller: self,
            cancellation,
        })
    }

    #[cfg(test)]
    pub fn active(&self) -> Option<ActiveMutation> {
        self.state
            .lock()
            .expect("mutation controller lock poisoned")
            .mutation
            .as_ref()
            .map(|state| state.active.clone())
    }

    pub fn snapshot(&self) -> MutationSnapshot {
        let state = self
            .state
            .lock()
            .expect("mutation controller lock poisoned");
        snapshot_from_state(&state)
    }

    pub fn set_listener(&self, listener: impl Fn(MutationSnapshot) + Send + Sync + 'static) {
        *self
            .listener
            .lock()
            .expect("mutation listener lock poisoned") = Some(Arc::new(listener));
    }

    pub fn request_cancel(&self) -> Result<bool, AppError> {
        let state = self
            .state
            .lock()
            .expect("mutation controller lock poisoned");
        let Some(state) = state.mutation.as_ref() else {
            return Ok(false);
        };
        if !state.active.cancelable {
            return Ok(false);
        }
        state.cancellation.cancel();
        Ok(true)
    }

    fn publish(&self, snapshot: MutationSnapshot) {
        let listener = self
            .listener
            .lock()
            .expect("mutation listener lock poisoned")
            .clone();
        if let Some(listener) = listener {
            listener(snapshot);
        }
    }
}

fn snapshot_from_state(state: &ControllerState) -> MutationSnapshot {
    MutationSnapshot {
        revision: state.revision,
        active: state
            .mutation
            .as_ref()
            .map(|mutation| mutation.active.clone()),
    }
}

fn next_revision(revision: u32) -> u32 {
    revision
        .checked_add(1)
        .expect("mutation revision exhausted during one application run")
}

pub struct MutationGuard<'a> {
    controller: &'a SingleMutationController,
    cancellation: CancellationSignal,
}

impl MutationGuard<'_> {
    pub fn cancellation(&self) -> CancellationSignal {
        self.cancellation.clone()
    }

    pub fn transition(
        &self,
        phase: MutationPhase,
        progress: Option<MutationProgress>,
        cancelable: bool,
    ) {
        let cancelable =
            cancelable && !matches!(phase, MutationPhase::Committing | MutationPhase::Finishing);
        let mut state = self
            .controller
            .state
            .lock()
            .expect("mutation controller lock poisoned");
        let Some(mutation) = state.mutation.as_mut() else {
            return;
        };
        mutation.active.phase = phase;
        mutation.active.progress = progress;
        mutation.active.cancelable = cancelable;
        state.revision = next_revision(state.revision);
        let snapshot = snapshot_from_state(&state);
        drop(state);
        self.controller.publish(snapshot);
    }
}

impl Drop for MutationGuard<'_> {
    fn drop(&mut self) {
        let mut state = self
            .controller
            .state
            .lock()
            .expect("mutation controller lock poisoned");
        if state.mutation.take().is_some() {
            state.revision = next_revision(state.revision);
            let snapshot = snapshot_from_state(&state);
            drop(state);
            self.controller.publish(snapshot);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::{MutationKind, MutationPhase, MutationProgress, SingleMutationController};
    use crate::environment::types::{ContextRef, ContextScope, EnvironmentRef};

    fn host_global() -> ContextRef {
        ContextRef {
            environment: EnvironmentRef::Host,
            scope: ContextScope::Global,
        }
    }

    #[test]
    fn snapshot_revision_tracks_the_mutation_lifecycle() {
        let controller = SingleMutationController::default();
        assert_eq!(
            controller.snapshot(),
            super::MutationSnapshot {
                revision: 0,
                active: None,
            }
        );

        let guard = controller
            .begin(MutationKind::Install, host_global())
            .expect("begin mutation");
        let started = controller.snapshot();
        assert_eq!(started.revision, 1);
        let active = started.active.expect("active mutation");
        assert_eq!(active.phase, MutationPhase::Preparing);
        assert_eq!(active.progress, None);

        guard.transition(
            MutationPhase::Materializing,
            Some(MutationProgress {
                subject: Some("demo".to_string()),
                current: Some(1),
                total: Some(3),
            }),
            false,
        );
        let updated = controller.snapshot();
        assert_eq!(updated.revision, 2);
        assert_eq!(
            updated.active.expect("active mutation").progress,
            Some(MutationProgress {
                subject: Some("demo".to_string()),
                current: Some(1),
                total: Some(3),
            })
        );

        drop(guard);
        assert_eq!(
            controller.snapshot(),
            super::MutationSnapshot {
                revision: 3,
                active: None,
            }
        );
    }

    #[test]
    fn rejected_mutation_does_not_advance_snapshot_revision() {
        let controller = SingleMutationController::default();
        let _guard = controller
            .begin(MutationKind::Install, host_global())
            .expect("begin mutation");

        assert!(controller
            .begin(MutationKind::Remove, host_global())
            .is_err());
        assert_eq!(controller.snapshot().revision, 1);
    }

    #[test]
    fn lifecycle_changes_publish_complete_snapshots() {
        let controller = SingleMutationController::default();
        let revisions = Arc::new(Mutex::new(Vec::new()));
        let observed = Arc::clone(&revisions);
        controller.set_listener(move |snapshot| {
            observed.lock().expect("observed revisions lock").push((
                snapshot.revision,
                snapshot.active.as_ref().map(|active| active.phase),
            ));
        });

        let guard = controller
            .begin(MutationKind::Install, host_global())
            .expect("begin mutation");
        guard.transition(MutationPhase::Committing, None, false);
        drop(guard);

        assert_eq!(
            *revisions.lock().expect("revisions lock"),
            vec![
                (1, Some(MutationPhase::Preparing)),
                (2, Some(MutationPhase::Committing)),
                (3, None),
            ]
        );
    }

    #[test]
    fn only_one_mutation_can_be_active_and_guard_releases_it() {
        let controller = SingleMutationController::default();
        {
            let _guard = controller
                .begin(MutationKind::Install, host_global())
                .expect("begin mutation");
            assert!(controller
                .begin(MutationKind::Remove, host_global())
                .is_err());
            assert_eq!(
                controller.active().expect("active mutation").kind,
                MutationKind::Install
            );
        }
        assert!(controller.active().is_none());
    }

    #[test]
    fn mutation_starts_non_cancelable() {
        let controller = SingleMutationController::default();
        let _guard = controller
            .begin(MutationKind::Install, host_global())
            .expect("begin mutation");

        assert!(!controller.active().expect("active mutation").cancelable);
    }

    #[test]
    fn transition_updates_all_fields_and_publishes_once() {
        let controller = SingleMutationController::default();
        let observed = Arc::new(Mutex::new(Vec::new()));
        let listener_observed = Arc::clone(&observed);
        controller.set_listener(move |snapshot| {
            listener_observed
                .lock()
                .expect("observed snapshots lock")
                .push(snapshot);
        });
        let guard = controller
            .begin(MutationKind::Install, host_global())
            .expect("begin mutation");
        observed.lock().expect("observed snapshots lock").clear();
        let progress = MutationProgress {
            subject: Some("toolkit".to_string()),
            current: Some(1),
            total: Some(2),
        };

        guard.transition(MutationPhase::Acquiring, Some(progress.clone()), true);

        let snapshots = observed.lock().expect("observed snapshots lock");
        assert_eq!(snapshots.len(), 1);
        assert_eq!(snapshots[0].revision, 2);
        assert_eq!(
            snapshots[0].active.as_ref().expect("active").phase,
            MutationPhase::Acquiring
        );
        assert_eq!(
            snapshots[0].active.as_ref().expect("active").progress,
            Some(progress)
        );
        assert!(snapshots[0].active.as_ref().expect("active").cancelable);
    }

    #[test]
    fn irreversible_phases_force_non_cancelable_state() {
        let controller = SingleMutationController::default();
        let guard = controller
            .begin(MutationKind::Install, host_global())
            .expect("begin mutation");

        guard.transition(MutationPhase::Committing, None, true);
        assert!(!controller.active().expect("active").cancelable);

        guard.transition(MutationPhase::Finishing, None, true);
        assert!(!controller.active().expect("active").cancelable);
    }

    #[test]
    fn cancel_request_sets_signal_only_when_cancelable() {
        let controller = SingleMutationController::default();
        let guard = controller
            .begin(MutationKind::Install, host_global())
            .expect("begin mutation");

        assert!(!controller.request_cancel().expect("request cancel"));
        guard.transition(MutationPhase::Acquiring, None, true);
        assert!(controller.request_cancel().expect("request cancel"));
        assert!(guard.cancellation().is_cancelled());

        guard.transition(MutationPhase::Committing, None, true);
        assert!(!controller.request_cancel().expect("request cancel"));
    }
}

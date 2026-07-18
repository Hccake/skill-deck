use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use serde::Serialize;
use specta::Type;
use tokio::sync::Notify;

use crate::environment::types::{same_environment_identity, ContextRef, EnvironmentRef};
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
    ManageAgentDefinitions,
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
    Validating,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub enum LifecycleLeaseKind {
    ApplicationUpdate,
    RuntimeMaintenance,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct ActiveLifecycleLease {
    pub id: String,
    pub kind: LifecycleLeaseKind,
    pub cancelable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct BackendActivitySnapshot {
    pub revision: u32,
    pub mutation: Option<ActiveMutation>,
    pub lifecycle: Option<ActiveLifecycleLease>,
}

#[derive(Default)]
struct CancellationState {
    cancelled: AtomicBool,
    notification: Notify,
}

#[derive(Clone, Default)]
pub struct CancellationSignal(Arc<CancellationState>);

impl CancellationSignal {
    pub fn cancel(&self) {
        if !self.0.cancelled.swap(true, Ordering::AcqRel) {
            self.0.notification.notify_waiters();
        }
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.cancelled.load(Ordering::Acquire)
    }

    pub async fn cancelled(&self) {
        if self.is_cancelled() {
            return;
        }
        let notified = self.0.notification.notified();
        tokio::pin!(notified);
        notified.as_mut().enable();
        if self.is_cancelled() {
            return;
        }
        notified.await;
    }
}

struct MutationState {
    active: ActiveMutation,
    cancellation: CancellationSignal,
}

struct LifecycleState {
    active: ActiveLifecycleLease,
}

#[derive(Default)]
struct ControllerState {
    revision: u32,
    mutation: Option<MutationState>,
    lifecycle: Option<LifecycleState>,
    termination_requested: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TerminationAdmission {
    Acquired,
    AlreadyRequested,
    Blocked(BackendActivitySnapshot),
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
        if state.termination_requested {
            return Err(AppError::ApplicationTerminating);
        }
        if state.mutation.is_some() || state.lifecycle.is_some() {
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

    pub fn begin_lifecycle(
        &self,
        kind: LifecycleLeaseKind,
    ) -> Result<LifecycleLeaseGuard<'_>, AppError> {
        let mut state = self
            .state
            .lock()
            .expect("mutation controller lock poisoned");
        if state.termination_requested {
            return Err(AppError::ApplicationTerminating);
        }
        if state.mutation.is_some() || state.lifecycle.is_some() {
            return Err(AppError::MutationBusy);
        }
        state.revision = next_revision(state.revision);
        state.lifecycle = Some(LifecycleState {
            active: ActiveLifecycleLease {
                id: uuid::Uuid::new_v4().to_string(),
                kind,
                cancelable: false,
            },
        });
        let snapshot = snapshot_from_state(&state);
        drop(state);
        self.publish(snapshot);
        Ok(LifecycleLeaseGuard { controller: self })
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

    pub fn active_for_environment(&self, environment: &EnvironmentRef) -> bool {
        self.state
            .lock()
            .expect("mutation controller lock poisoned")
            .mutation
            .as_ref()
            .is_some_and(|mutation| {
                same_environment_identity(&mutation.active.context.environment, environment)
            })
    }

    pub fn activity_snapshot(&self) -> BackendActivitySnapshot {
        let state = self
            .state
            .lock()
            .expect("mutation controller lock poisoned");
        activity_snapshot_from_state(&state)
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

    pub fn request_termination(&self) -> TerminationAdmission {
        let mut state = self
            .state
            .lock()
            .expect("mutation controller lock poisoned");
        if state.termination_requested {
            return TerminationAdmission::AlreadyRequested;
        }
        if state.mutation.is_some() || state.lifecycle.is_some() {
            return TerminationAdmission::Blocked(activity_snapshot_from_state(&state));
        }
        state.termination_requested = true;
        TerminationAdmission::Acquired
    }

    pub fn with_idle<T>(
        &self,
        action: impl FnOnce() -> T,
    ) -> Result<T, Box<BackendActivitySnapshot>> {
        let state = self
            .state
            .lock()
            .expect("mutation controller lock poisoned");
        if state.mutation.is_some() || state.lifecycle.is_some() {
            return Err(Box::new(activity_snapshot_from_state(&state)));
        }
        Ok(action())
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

fn activity_snapshot_from_state(state: &ControllerState) -> BackendActivitySnapshot {
    BackendActivitySnapshot {
        revision: state.revision,
        mutation: state
            .mutation
            .as_ref()
            .map(|mutation| mutation.active.clone()),
        lifecycle: state
            .lifecycle
            .as_ref()
            .map(|lifecycle| lifecycle.active.clone()),
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

pub struct LifecycleLeaseGuard<'a> {
    controller: &'a SingleMutationController,
}

impl Drop for LifecycleLeaseGuard<'_> {
    fn drop(&mut self) {
        let mut state = self
            .controller
            .state
            .lock()
            .expect("mutation controller lock poisoned");
        if state.lifecycle.take().is_some() {
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

    use super::{
        LifecycleLeaseKind, MutationKind, MutationPhase, MutationProgress,
        SingleMutationController, TerminationAdmission,
    };
    use crate::environment::types::{ContextRef, ContextScope, EnvironmentRef};
    use crate::error::AppError;

    fn host_global() -> ContextRef {
        ContextRef {
            environment: EnvironmentRef::Host,
            scope: ContextScope::Global,
        }
    }

    #[test]
    fn updater_lease_and_skill_mutation_are_mutually_exclusive() {
        let controller = SingleMutationController::default();
        let lease = controller
            .begin_lifecycle(LifecycleLeaseKind::ApplicationUpdate)
            .expect("begin lifecycle");
        assert_eq!(controller.activity_snapshot().revision, 1);
        assert!(matches!(
            controller.begin(MutationKind::Install, host_global()),
            Err(AppError::MutationBusy)
        ));
        assert!(matches!(
            controller.request_termination(),
            TerminationAdmission::Blocked(snapshot)
                if snapshot.lifecycle.is_some() && snapshot.mutation.is_none()
        ));
        drop(lease);
        assert_eq!(controller.activity_snapshot().revision, 2);

        let mutation = controller
            .begin(MutationKind::Install, host_global())
            .expect("begin mutation");
        assert!(matches!(
            controller.begin_lifecycle(LifecycleLeaseKind::ApplicationUpdate),
            Err(AppError::MutationBusy)
        ));
        drop(mutation);
    }

    #[test]
    fn runtime_maintenance_lease_blocks_mutation_start() {
        let controller = SingleMutationController::default();
        let lease = controller
            .begin_lifecycle(LifecycleLeaseKind::RuntimeMaintenance)
            .expect("begin maintenance");
        assert!(matches!(
            controller.begin(MutationKind::Install, host_global()),
            Err(AppError::MutationBusy)
        ));
        drop(lease);
        controller
            .begin(MutationKind::Install, host_global())
            .expect("mutation after maintenance");
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
            MutationPhase::Acquiring,
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
        controller
            .begin(MutationKind::Update, host_global())
            .expect("begin update after prior guard release");
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

    #[test]
    fn active_mutation_blocks_application_termination() {
        let controller = SingleMutationController::default();
        let guard = controller
            .begin(MutationKind::Install, host_global())
            .expect("begin mutation");

        let admission = controller.request_termination();

        let TerminationAdmission::Blocked(snapshot) = admission else {
            panic!("active mutation must block application termination");
        };
        assert_eq!(
            snapshot.mutation.as_ref().map(|active| active.kind),
            Some(MutationKind::Install)
        );

        drop(guard);
        assert_eq!(
            controller.request_termination(),
            TerminationAdmission::Acquired
        );
    }

    #[test]
    fn termination_admission_rejects_new_mutations() {
        let controller = SingleMutationController::default();
        assert_eq!(
            controller.request_termination(),
            TerminationAdmission::Acquired
        );

        let result = controller.begin(MutationKind::Update, host_global());

        assert!(matches!(result, Err(AppError::ApplicationTerminating)));
    }

    #[test]
    fn repeated_termination_request_is_idempotent() {
        let controller = SingleMutationController::default();

        assert_eq!(
            controller.request_termination(),
            TerminationAdmission::Acquired
        );
        assert_eq!(
            controller.request_termination(),
            TerminationAdmission::AlreadyRequested
        );
    }

    #[test]
    fn idle_action_runs_only_without_an_active_mutation() {
        let controller = SingleMutationController::default();
        let guard = controller
            .begin(MutationKind::Remove, host_global())
            .expect("begin mutation");
        let mut performed = false;

        let blocked = controller.with_idle(|| performed = true);

        assert!(blocked.is_err());
        assert!(!performed);

        drop(guard);
        assert_eq!(controller.with_idle(|| 42), Ok(42));
    }
}

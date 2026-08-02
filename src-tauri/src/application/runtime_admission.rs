use std::fmt;
use std::sync::{Arc, Mutex};

use crate::application::install_wizard_session::InstallWizardSessionSnapshot;
use crate::core::mutation::{
    ActiveLifecycleLease, ActiveMutation, BackendActivitySnapshot, CancellationSignal,
    LifecycleLeaseKind, MutationKind, MutationPhase, MutationProgress, MutationSnapshot,
    TerminationAdmission,
};
use crate::environment::types::{same_environment_identity, ContextRef, EnvironmentRef};
use crate::error::AppError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdmissionDenied {
    Mutation,
    Lifecycle,
    InstallWizard,
    WslSettingChange,
    ApplicationTerminating,
}

impl AdmissionDenied {
    fn as_legacy_error(self) -> AppError {
        match self {
            Self::ApplicationTerminating => AppError::ApplicationTerminating,
            _ => AppError::MutationBusy,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WizardWindowPresence {
    Absent,
    Present { instance_id: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WizardWindowObservation {
    Present { instance_id: String },
    Destroyed { instance_id: String },
}

pub enum WizardAdmission {
    Existing { instance_id: String },
    Reserved(WizardReservation),
}

impl fmt::Debug for WizardAdmission {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Existing { instance_id } => formatter
                .debug_struct("Existing")
                .field("instance_id", instance_id)
                .finish(),
            Self::Reserved(_) => formatter.write_str("Reserved(..)"),
        }
    }
}

struct MutationState {
    token: u64,
    active: ActiveMutation,
    cancellation: CancellationSignal,
}

struct LifecycleState {
    token: u64,
    active: ActiveLifecycleLease,
}

enum WizardState {
    Idle,
    Reserved { token: u64 },
    Active { instance_id: String },
}

struct AdmissionState {
    revision: u32,
    next_token: u64,
    mutation: Option<MutationState>,
    lifecycle: Option<LifecycleState>,
    wizard: WizardState,
    setting_token: Option<u64>,
    termination_requested: bool,
}

impl Default for AdmissionState {
    fn default() -> Self {
        Self {
            revision: 0,
            next_token: 0,
            mutation: None,
            lifecycle: None,
            wizard: WizardState::Idle,
            setting_token: None,
            termination_requested: false,
        }
    }
}

type MutationListener = Arc<dyn Fn(MutationSnapshot) + Send + Sync>;
type WizardListener = Arc<dyn Fn(InstallWizardSessionSnapshot) + Send + Sync>;

#[derive(Default)]
struct AdmissionInner {
    state: Mutex<AdmissionState>,
    mutation_listener: Mutex<Option<MutationListener>>,
    wizard_listener: Mutex<Option<WizardListener>>,
}

#[derive(Clone, Default)]
pub struct RuntimeAdmissionCoordinator {
    inner: Arc<AdmissionInner>,
}

impl RuntimeAdmissionCoordinator {
    #[cfg(test)]
    pub fn begin(
        &self,
        kind: MutationKind,
        context: ContextRef,
    ) -> Result<MutationPermit, AppError> {
        self.begin_mutation(kind, context)
    }

    pub fn begin_mutation(
        &self,
        kind: MutationKind,
        context: ContextRef,
    ) -> Result<MutationPermit, AppError> {
        let mut state = self.lock_state();
        self.ensure_process_running(&state)
            .map_err(AdmissionDenied::as_legacy_error)?;
        if state.mutation.is_some() || state.lifecycle.is_some() || state.setting_token.is_some() {
            return Err(AppError::MutationBusy);
        }
        let token = next_token(&mut state);
        let cancellation = CancellationSignal::default();
        state.revision = next_revision(state.revision);
        state.mutation = Some(MutationState {
            token,
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
        let snapshot = mutation_snapshot_from_state(&state);
        drop(state);
        self.publish_mutation(snapshot);
        Ok(MutationPermit {
            inner: Arc::clone(&self.inner),
            token,
            cancellation,
        })
    }

    pub fn begin_lifecycle(&self, kind: LifecycleLeaseKind) -> Result<LifecyclePermit, AppError> {
        let mut state = self.lock_state();
        self.ensure_process_running(&state)
            .map_err(AdmissionDenied::as_legacy_error)?;
        if state.mutation.is_some() || state.lifecycle.is_some() || state.setting_token.is_some() {
            return Err(AppError::MutationBusy);
        }
        let token = next_token(&mut state);
        state.revision = next_revision(state.revision);
        state.lifecycle = Some(LifecycleState {
            token,
            active: ActiveLifecycleLease {
                id: uuid::Uuid::new_v4().to_string(),
                kind,
                cancelable: false,
            },
        });
        Ok(LifecyclePermit {
            inner: Arc::clone(&self.inner),
            token,
        })
    }

    pub fn admit_install_wizard(
        &self,
        observed_window: WizardWindowPresence,
    ) -> Result<WizardAdmission, AdmissionDenied> {
        let mut state = self.lock_state();
        self.ensure_process_running(&state)?;
        if state.setting_token.is_some() {
            return Err(AdmissionDenied::WslSettingChange);
        }

        match observed_window {
            WizardWindowPresence::Present { instance_id } => {
                let changed = !matches!(
                    &state.wizard,
                    WizardState::Active { instance_id: active } if active == &instance_id
                );
                if changed {
                    state.wizard = WizardState::Active {
                        instance_id: instance_id.clone(),
                    };
                    state.revision = next_revision(state.revision);
                }
                let snapshot = wizard_snapshot_from_state(&state);
                drop(state);
                if changed {
                    self.publish_wizard(snapshot);
                }
                Ok(WizardAdmission::Existing { instance_id })
            }
            WizardWindowPresence::Absent => match state.wizard {
                WizardState::Reserved { .. } => Err(AdmissionDenied::InstallWizard),
                WizardState::Idle | WizardState::Active { .. } => {
                    let token = next_token(&mut state);
                    state.wizard = WizardState::Reserved { token };
                    state.revision = next_revision(state.revision);
                    let snapshot = wizard_snapshot_from_state(&state);
                    drop(state);
                    self.publish_wizard(snapshot);
                    Ok(WizardAdmission::Reserved(WizardReservation {
                        inner: Arc::clone(&self.inner),
                        token,
                        finished: false,
                    }))
                }
            },
        }
    }

    pub fn observe_install_wizard_window(
        &self,
        observation: WizardWindowObservation,
    ) -> InstallWizardSessionSnapshot {
        let mut state = self.lock_state();
        let changed = match observation {
            WizardWindowObservation::Present { instance_id } => {
                if matches!(
                    &state.wizard,
                    WizardState::Active { instance_id: active } if active == &instance_id
                ) {
                    false
                } else {
                    state.wizard = WizardState::Active { instance_id };
                    true
                }
            }
            WizardWindowObservation::Destroyed { instance_id } => {
                if matches!(
                    &state.wizard,
                    WizardState::Active { instance_id: active } if active == &instance_id
                ) {
                    state.wizard = WizardState::Idle;
                    true
                } else {
                    false
                }
            }
        };
        if changed {
            state.revision = next_revision(state.revision);
        }
        let snapshot = wizard_snapshot_from_state(&state);
        drop(state);
        if changed {
            self.publish_wizard(snapshot.clone());
        }
        snapshot
    }

    pub fn begin_wsl_integration_change(&self) -> Result<SettingPermit, AdmissionDenied> {
        let mut state = self.lock_state();
        self.ensure_process_running(&state)?;
        if state.mutation.is_some() {
            return Err(AdmissionDenied::Mutation);
        }
        if state.lifecycle.is_some() {
            return Err(AdmissionDenied::Lifecycle);
        }
        if !matches!(state.wizard, WizardState::Idle) {
            return Err(AdmissionDenied::InstallWizard);
        }
        if state.setting_token.is_some() {
            return Err(AdmissionDenied::WslSettingChange);
        }
        let token = next_token(&mut state);
        state.setting_token = Some(token);
        state.revision = next_revision(state.revision);
        Ok(SettingPermit {
            inner: Arc::clone(&self.inner),
            token,
        })
    }

    pub fn mutation_snapshot(&self) -> MutationSnapshot {
        mutation_snapshot_from_state(&self.lock_state())
    }

    pub fn snapshot(&self) -> MutationSnapshot {
        self.mutation_snapshot()
    }

    pub fn install_wizard_snapshot(&self) -> InstallWizardSessionSnapshot {
        wizard_snapshot_from_state(&self.lock_state())
    }

    #[cfg(test)]
    pub fn activity_snapshot(&self) -> BackendActivitySnapshot {
        activity_snapshot_from_state(&self.lock_state())
    }

    pub fn active_for_environment(&self, environment: &EnvironmentRef) -> bool {
        self.lock_state().mutation.as_ref().is_some_and(|mutation| {
            same_environment_identity(&mutation.active.context.environment, environment)
        })
    }

    pub fn request_cancel(&self) -> Result<bool, AppError> {
        let state = self.lock_state();
        let Some(mutation) = state.mutation.as_ref() else {
            return Ok(false);
        };
        if !mutation.active.cancelable {
            return Ok(false);
        }
        mutation.cancellation.cancel();
        Ok(true)
    }

    pub fn request_termination(&self) -> TerminationAdmission {
        let mut state = self.lock_state();
        if state.termination_requested {
            return TerminationAdmission::AlreadyRequested;
        }
        if state.mutation.is_some()
            || state.lifecycle.is_some()
            || state.setting_token.is_some()
            || matches!(state.wizard, WizardState::Reserved { .. })
        {
            return TerminationAdmission::Blocked(activity_snapshot_from_state(&state));
        }
        state.termination_requested = true;
        TerminationAdmission::Acquired
    }

    pub fn with_idle<T>(
        &self,
        action: impl FnOnce() -> T,
    ) -> Result<T, Box<BackendActivitySnapshot>> {
        let state = self.lock_state();
        if state.mutation.is_some() || state.lifecycle.is_some() || state.setting_token.is_some() {
            return Err(Box::new(activity_snapshot_from_state(&state)));
        }
        Ok(action())
    }

    pub fn set_mutation_listener(
        &self,
        listener: impl Fn(MutationSnapshot) + Send + Sync + 'static,
    ) {
        *self
            .inner
            .mutation_listener
            .lock()
            .expect("mutation listener lock poisoned") = Some(Arc::new(listener));
    }

    #[cfg(test)]
    pub fn set_listener(&self, listener: impl Fn(MutationSnapshot) + Send + Sync + 'static) {
        self.set_mutation_listener(listener);
    }

    #[cfg(test)]
    pub fn active(&self) -> Option<ActiveMutation> {
        self.lock_state()
            .mutation
            .as_ref()
            .map(|mutation| mutation.active.clone())
    }

    pub fn set_install_wizard_listener(
        &self,
        listener: impl Fn(InstallWizardSessionSnapshot) + Send + Sync + 'static,
    ) {
        *self
            .inner
            .wizard_listener
            .lock()
            .expect("install wizard listener lock poisoned") = Some(Arc::new(listener));
    }

    #[cfg(test)]
    fn wizard_instance_id(&self) -> Option<String> {
        match &self.lock_state().wizard {
            WizardState::Active { instance_id } => Some(instance_id.clone()),
            _ => None,
        }
    }

    fn ensure_process_running(&self, state: &AdmissionState) -> Result<(), AdmissionDenied> {
        if state.termination_requested {
            Err(AdmissionDenied::ApplicationTerminating)
        } else {
            Ok(())
        }
    }

    fn lock_state(&self) -> std::sync::MutexGuard<'_, AdmissionState> {
        self.inner
            .state
            .lock()
            .expect("runtime admission lock poisoned")
    }

    fn publish_mutation(&self, snapshot: MutationSnapshot) {
        let listener = self
            .inner
            .mutation_listener
            .lock()
            .expect("mutation listener lock poisoned")
            .clone();
        if let Some(listener) = listener {
            listener(snapshot);
        }
    }

    fn publish_wizard(&self, snapshot: InstallWizardSessionSnapshot) {
        let listener = self
            .inner
            .wizard_listener
            .lock()
            .expect("install wizard listener lock poisoned")
            .clone();
        if let Some(listener) = listener {
            listener(snapshot);
        }
    }
}

fn next_token(state: &mut AdmissionState) -> u64 {
    state.next_token = state
        .next_token
        .checked_add(1)
        .expect("runtime admission token exhausted during one application run");
    state.next_token
}

fn next_revision(revision: u32) -> u32 {
    revision
        .checked_add(1)
        .expect("runtime admission revision exhausted during one application run")
}

fn mutation_snapshot_from_state(state: &AdmissionState) -> MutationSnapshot {
    MutationSnapshot {
        revision: state.revision,
        active: state
            .mutation
            .as_ref()
            .map(|mutation| mutation.active.clone()),
    }
}

fn activity_snapshot_from_state(state: &AdmissionState) -> BackendActivitySnapshot {
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

fn wizard_snapshot_from_state(state: &AdmissionState) -> InstallWizardSessionSnapshot {
    InstallWizardSessionSnapshot {
        revision: state.revision,
        active: !matches!(state.wizard, WizardState::Idle),
    }
}

pub struct MutationPermit {
    inner: Arc<AdmissionInner>,
    token: u64,
    cancellation: CancellationSignal,
}

impl fmt::Debug for MutationPermit {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("MutationPermit")
            .finish_non_exhaustive()
    }
}

impl MutationPermit {
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
        let coordinator = RuntimeAdmissionCoordinator {
            inner: Arc::clone(&self.inner),
        };
        let mut state = coordinator.lock_state();
        let Some(mutation) = state
            .mutation
            .as_mut()
            .filter(|mutation| mutation.token == self.token)
        else {
            return;
        };
        mutation.active.phase = phase;
        mutation.active.progress = progress;
        mutation.active.cancelable = cancelable;
        state.revision = next_revision(state.revision);
        let snapshot = mutation_snapshot_from_state(&state);
        drop(state);
        coordinator.publish_mutation(snapshot);
    }
}

impl Drop for MutationPermit {
    fn drop(&mut self) {
        let coordinator = RuntimeAdmissionCoordinator {
            inner: Arc::clone(&self.inner),
        };
        let mut state = coordinator.lock_state();
        if state
            .mutation
            .as_ref()
            .is_none_or(|mutation| mutation.token != self.token)
        {
            return;
        }
        state.mutation = None;
        state.revision = next_revision(state.revision);
        let snapshot = mutation_snapshot_from_state(&state);
        drop(state);
        coordinator.publish_mutation(snapshot);
    }
}

pub struct LifecyclePermit {
    inner: Arc<AdmissionInner>,
    token: u64,
}

impl fmt::Debug for LifecyclePermit {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LifecyclePermit")
            .finish_non_exhaustive()
    }
}

impl Drop for LifecyclePermit {
    fn drop(&mut self) {
        let coordinator = RuntimeAdmissionCoordinator {
            inner: Arc::clone(&self.inner),
        };
        let mut state = coordinator.lock_state();
        if state
            .lifecycle
            .as_ref()
            .is_some_and(|lifecycle| lifecycle.token == self.token)
        {
            state.lifecycle = None;
            state.revision = next_revision(state.revision);
        }
    }
}

pub struct SettingPermit {
    inner: Arc<AdmissionInner>,
    token: u64,
}

impl fmt::Debug for SettingPermit {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SettingPermit")
            .finish_non_exhaustive()
    }
}

impl Drop for SettingPermit {
    fn drop(&mut self) {
        let coordinator = RuntimeAdmissionCoordinator {
            inner: Arc::clone(&self.inner),
        };
        let mut state = coordinator.lock_state();
        if state.setting_token == Some(self.token) {
            state.setting_token = None;
            state.revision = next_revision(state.revision);
        }
    }
}

pub struct WizardReservation {
    inner: Arc<AdmissionInner>,
    token: u64,
    finished: bool,
}

impl WizardReservation {
    pub fn activate(mut self, instance_id: String) -> InstallWizardSessionSnapshot {
        let coordinator = RuntimeAdmissionCoordinator {
            inner: Arc::clone(&self.inner),
        };
        let mut state = coordinator.lock_state();
        if matches!(state.wizard, WizardState::Reserved { token } if token == self.token) {
            state.wizard = WizardState::Active { instance_id };
            state.revision = next_revision(state.revision);
        }
        let snapshot = wizard_snapshot_from_state(&state);
        drop(state);
        self.finished = true;
        coordinator.publish_wizard(snapshot.clone());
        snapshot
    }
}

impl Drop for WizardReservation {
    fn drop(&mut self) {
        if self.finished {
            return;
        }
        let coordinator = RuntimeAdmissionCoordinator {
            inner: Arc::clone(&self.inner),
        };
        let mut state = coordinator.lock_state();
        if matches!(state.wizard, WizardState::Reserved { token } if token == self.token) {
            state.wizard = WizardState::Idle;
            state.revision = next_revision(state.revision);
            let snapshot = wizard_snapshot_from_state(&state);
            drop(state);
            coordinator.publish_wizard(snapshot);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::mutation::{LifecycleLeaseKind, MutationKind};
    use crate::environment::types::{ContextRef, ContextScope, EnvironmentRef};

    fn host_global() -> ContextRef {
        ContextRef {
            environment: EnvironmentRef::Host,
            scope: ContextScope::Global,
        }
    }

    #[test]
    fn setting_change_conflicts_with_mutation_and_lifecycle() {
        let admission = RuntimeAdmissionCoordinator::default();
        let mutation = admission
            .begin_mutation(MutationKind::Install, host_global())
            .expect("mutation admitted");
        assert_eq!(
            admission.begin_wsl_integration_change().unwrap_err(),
            AdmissionDenied::Mutation
        );
        drop(mutation);

        let lifecycle = admission
            .begin_lifecycle(LifecycleLeaseKind::RuntimeMaintenance)
            .expect("lifecycle admitted");
        assert_eq!(
            admission.begin_wsl_integration_change().unwrap_err(),
            AdmissionDenied::Lifecycle
        );
        drop(lifecycle);
        assert!(admission.begin_wsl_integration_change().is_ok());
    }

    #[test]
    fn wizard_reservation_blocks_setting_change_until_creation_finishes() {
        let admission = RuntimeAdmissionCoordinator::default();
        let WizardAdmission::Reserved(reservation) = admission
            .admit_install_wizard(WizardWindowPresence::Absent)
            .expect("wizard reservation admitted")
        else {
            panic!("expected reservation");
        };

        assert_eq!(
            admission.begin_wsl_integration_change().unwrap_err(),
            AdmissionDenied::InstallWizard
        );
        drop(reservation);
        assert!(admission.begin_wsl_integration_change().is_ok());
    }

    #[test]
    fn late_destroyed_event_does_not_close_a_new_wizard_instance() {
        let admission = RuntimeAdmissionCoordinator::default();
        let WizardAdmission::Reserved(reservation) = admission
            .admit_install_wizard(WizardWindowPresence::Absent)
            .expect("first reservation admitted")
        else {
            panic!("expected reservation");
        };
        reservation.activate("wizard-1".to_string());
        admission.observe_install_wizard_window(WizardWindowObservation::Destroyed {
            instance_id: "wizard-1".to_string(),
        });

        let WizardAdmission::Reserved(reservation) = admission
            .admit_install_wizard(WizardWindowPresence::Absent)
            .expect("second reservation admitted")
        else {
            panic!("expected reservation");
        };
        reservation.activate("wizard-2".to_string());
        let snapshot =
            admission.observe_install_wizard_window(WizardWindowObservation::Destroyed {
                instance_id: "wizard-1".to_string(),
            });

        assert!(snapshot.active);
        assert_eq!(admission.wizard_instance_id().as_deref(), Some("wizard-2"));
    }

    #[test]
    fn observed_existing_window_heals_stale_session_atomically() {
        let admission = RuntimeAdmissionCoordinator::default();
        let result = admission
            .admit_install_wizard(WizardWindowPresence::Present {
                instance_id: "wizard-1".to_string(),
            })
            .expect("existing wizard admitted");

        assert!(matches!(result, WizardAdmission::Existing { .. }));
        assert!(admission.install_wizard_snapshot().active);
        assert_eq!(admission.wizard_instance_id().as_deref(), Some("wizard-1"));
    }
}

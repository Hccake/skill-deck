use std::fmt;
use std::sync::{Arc, Mutex};

use crate::application::install_wizard_session::InstallWizardSessionSnapshot;
use crate::core::mutation::{
    ActiveLifecycleLease, ActiveMutation, BackendActivitySnapshot, CancellationSignal,
    LifecycleLeaseKind, MutationKind, MutationPhase, MutationProgress, MutationSnapshot,
    MutationTargetRef, TerminationAdmission,
};
use crate::environment::types::{same_environment_identity, EnvironmentRef, SkillLocationRef};
use crate::error::AppError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdmissionDenied {
    Mutation,
    Lifecycle,
    InstallWizard,
    WslSettingChange,
    ApplicationTerminating,
}

#[derive(Clone, Copy)]
enum AdmissionIntent {
    Mutation,
    InstallWizardMutation,
    Lifecycle(LifecycleLeaseKind),
    InstallWizard,
    WslSettingChange,
    ExclusiveAction,
}

impl AdmissionDenied {
    pub(crate) fn into_legacy_error(self) -> AppError {
        match self {
            Self::ApplicationTerminating => AppError::ApplicationTerminating,
            Self::InstallWizard => AppError::InstallWizardActive,
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

#[derive(Debug, Clone, PartialEq, Eq)]
enum MutationOwner {
    Standalone,
    InstallWizard { instance_id: String },
}

struct MutationState {
    token: u64,
    active: ActiveMutation,
    cancellation: CancellationSignal,
    owner: MutationOwner,
}

struct LifecycleState {
    token: u64,
    active: ActiveLifecycleLease,
    cancellation: Option<CancellationSignal>,
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
    exclusive_token: Option<u64>,
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
            exclusive_token: None,
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
    pub fn begin_mutation(
        &self,
        kind: MutationKind,
        context: SkillLocationRef,
    ) -> Result<MutationPermit, AppError> {
        let state = self.lock_state();
        self.denial_for(&state, AdmissionIntent::Mutation)
            .map_or(Ok(()), |denied| Err(denied.into_legacy_error()))?;
        Ok(self.register_mutation(
            state,
            kind,
            MutationTargetRef::SkillLocation {
                environment: context.environment,
                scope: context.scope,
            },
            MutationOwner::Standalone,
        ))
    }

    pub fn begin_library_mutation(
        &self,
        kind: MutationKind,
        environment: EnvironmentRef,
        library_id: String,
    ) -> Result<MutationPermit, AppError> {
        let state = self.lock_state();
        self.denial_for(&state, AdmissionIntent::Mutation)
            .map_or(Ok(()), |denied| Err(denied.into_legacy_error()))?;
        Ok(self.register_mutation(
            state,
            kind,
            MutationTargetRef::Library {
                environment,
                library_id,
            },
            MutationOwner::Standalone,
        ))
    }

    pub fn begin_install_from_active_wizard(
        &self,
        context: SkillLocationRef,
    ) -> Result<MutationPermit, AppError> {
        let state = self.lock_state();
        if let Some(denied) = self.denial_for(&state, AdmissionIntent::InstallWizardMutation) {
            return Err(match denied {
                AdmissionDenied::InstallWizard => AppError::InstallWizardSessionUnavailable,
                other => other.into_legacy_error(),
            });
        }
        let WizardState::Active { instance_id } = &state.wizard else {
            unreachable!("wizard mutation admission requires an active session");
        };
        let owner = MutationOwner::InstallWizard {
            instance_id: instance_id.clone(),
        };
        Ok(self.register_mutation(
            state,
            MutationKind::Install,
            MutationTargetRef::SkillLocation {
                environment: context.environment,
                scope: context.scope,
            },
            owner,
        ))
    }

    fn register_mutation(
        &self,
        mut state: std::sync::MutexGuard<'_, AdmissionState>,
        kind: MutationKind,
        target: MutationTargetRef,
        owner: MutationOwner,
    ) -> MutationPermit {
        let token = next_token(&mut state);
        let cancellation = CancellationSignal::default();
        state.revision = next_revision(state.revision);
        state.mutation = Some(MutationState {
            token,
            active: ActiveMutation {
                id: uuid::Uuid::new_v4().to_string(),
                kind,
                target,
                phase: MutationPhase::Preparing,
                progress: None,
                cancelable: false,
            },
            cancellation: cancellation.clone(),
            owner: owner.clone(),
        });
        let snapshot = mutation_snapshot_from_state(&state);
        drop(state);
        self.publish_mutation(snapshot);
        MutationPermit {
            inner: Arc::clone(&self.inner),
            token,
            cancellation,
            owner,
        }
    }

    pub fn begin_lifecycle(&self, kind: LifecycleLeaseKind) -> Result<LifecyclePermit, AppError> {
        self.register_lifecycle(kind, false)
    }

    pub fn begin_cancelable_lifecycle(
        &self,
        kind: LifecycleLeaseKind,
    ) -> Result<LifecyclePermit, AppError> {
        self.register_lifecycle(kind, true)
    }

    fn register_lifecycle(
        &self,
        kind: LifecycleLeaseKind,
        supports_cancellation: bool,
    ) -> Result<LifecyclePermit, AppError> {
        let mut state = self.lock_state();
        self.denial_for(&state, AdmissionIntent::Lifecycle(kind))
            .map_or(Ok(()), |denied| Err(denied.into_legacy_error()))?;
        let token = next_token(&mut state);
        let cancellation = supports_cancellation.then(CancellationSignal::default);
        state.revision = next_revision(state.revision);
        state.lifecycle = Some(LifecycleState {
            token,
            active: ActiveLifecycleLease {
                id: uuid::Uuid::new_v4().to_string(),
                kind,
                cancelable: false,
            },
            cancellation: cancellation.clone(),
        });
        Ok(LifecyclePermit {
            inner: Arc::clone(&self.inner),
            token,
            cancellation,
        })
    }

    pub fn admit_install_wizard(
        &self,
        observed_window: WizardWindowPresence,
    ) -> Result<WizardAdmission, AdmissionDenied> {
        let mut state = self.lock_state();
        if let Some(denied) = self.denial_for(&state, AdmissionIntent::InstallWizard) {
            return Err(denied);
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
        if let Some(denied) = self.denial_for(&state, AdmissionIntent::WslSettingChange) {
            return Err(denied);
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
            same_environment_identity(mutation.active.target.environment(), environment)
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

    pub fn request_cancel_lifecycle(&self) -> Result<bool, AppError> {
        let state = self.lock_state();
        let Some(lifecycle) = state.lifecycle.as_ref() else {
            return Ok(false);
        };
        if !lifecycle.active.cancelable {
            return Ok(false);
        }
        let Some(cancellation) = lifecycle.cancellation.as_ref() else {
            return Ok(false);
        };
        cancellation.cancel();
        Ok(true)
    }

    pub fn set_application_update_cancelable(&self, cancelable: bool) {
        let mut state = self.lock_state();
        let Some(lifecycle) = state.lifecycle.as_mut().filter(|lifecycle| {
            lifecycle.active.kind == LifecycleLeaseKind::ApplicationUpdate
                && lifecycle.cancellation.is_some()
        }) else {
            return;
        };
        lifecycle.active.cancelable = cancelable;
        state.revision = next_revision(state.revision);
    }

    pub fn request_termination(&self) -> TerminationAdmission {
        let mut state = self.lock_state();
        if state.termination_requested {
            return TerminationAdmission::AlreadyRequested;
        }
        if state.mutation.is_some()
            || state.lifecycle.is_some()
            || state.setting_token.is_some()
            || state.exclusive_token.is_some()
            || matches!(state.wizard, WizardState::Reserved { .. })
        {
            return TerminationAdmission::Blocked(activity_snapshot_from_state(&state));
        }
        state.termination_requested = true;
        state.revision = next_revision(state.revision);
        TerminationAdmission::Acquired
    }

    pub fn with_idle<T>(
        &self,
        action: impl FnOnce() -> T,
    ) -> Result<T, Box<BackendActivitySnapshot>> {
        let mut state = self.lock_state();
        if self
            .denial_for(&state, AdmissionIntent::ExclusiveAction)
            .is_some()
        {
            return Err(Box::new(activity_snapshot_from_state(&state)));
        }
        let token = next_token(&mut state);
        state.exclusive_token = Some(token);
        state.revision = next_revision(state.revision);
        drop(state);
        let _permit = ExclusivePermit {
            inner: Arc::clone(&self.inner),
            token,
        };
        Ok(action())
    }

    pub fn begin_exclusive_action(&self) -> Result<ExclusivePermit, AppError> {
        let mut state = self.lock_state();
        self.denial_for(&state, AdmissionIntent::ExclusiveAction)
            .map_or(Ok(()), |denied| Err(denied.into_legacy_error()))?;
        let token = next_token(&mut state);
        state.exclusive_token = Some(token);
        state.revision = next_revision(state.revision);
        Ok(ExclusivePermit {
            inner: Arc::clone(&self.inner),
            token,
        })
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

    fn denial_for(
        &self,
        state: &AdmissionState,
        intent: AdmissionIntent,
    ) -> Option<AdmissionDenied> {
        if state.termination_requested {
            return Some(AdmissionDenied::ApplicationTerminating);
        }
        match intent {
            AdmissionIntent::Mutation
            | AdmissionIntent::Lifecycle(LifecycleLeaseKind::ApplicationUpdate)
            | AdmissionIntent::WslSettingChange => {
                if state.mutation.is_some() {
                    Some(AdmissionDenied::Mutation)
                } else if !matches!(state.wizard, WizardState::Idle) {
                    Some(AdmissionDenied::InstallWizard)
                } else if state.lifecycle.is_some() || state.exclusive_token.is_some() {
                    Some(AdmissionDenied::Lifecycle)
                } else if state.setting_token.is_some() {
                    Some(AdmissionDenied::WslSettingChange)
                } else {
                    None
                }
            }
            AdmissionIntent::InstallWizardMutation => {
                if !matches!(state.wizard, WizardState::Active { .. }) {
                    Some(AdmissionDenied::InstallWizard)
                } else if state.mutation.is_some() {
                    Some(AdmissionDenied::Mutation)
                } else if state.lifecycle.is_some() || state.exclusive_token.is_some() {
                    Some(AdmissionDenied::Lifecycle)
                } else if state.setting_token.is_some() {
                    Some(AdmissionDenied::WslSettingChange)
                } else {
                    None
                }
            }
            AdmissionIntent::Lifecycle(LifecycleLeaseKind::RuntimeMaintenance) => {
                if state.mutation.is_some() {
                    Some(AdmissionDenied::Mutation)
                } else if state.lifecycle.is_some() || state.exclusive_token.is_some() {
                    Some(AdmissionDenied::Lifecycle)
                } else if state.setting_token.is_some() {
                    Some(AdmissionDenied::WslSettingChange)
                } else {
                    None
                }
            }
            AdmissionIntent::InstallWizard => {
                if state.mutation.is_some() {
                    Some(AdmissionDenied::Mutation)
                } else if state.lifecycle.as_ref().is_some_and(|lifecycle| {
                    lifecycle.active.kind == LifecycleLeaseKind::ApplicationUpdate
                }) {
                    Some(AdmissionDenied::Lifecycle)
                } else if state.setting_token.is_some() {
                    Some(AdmissionDenied::WslSettingChange)
                } else if state.exclusive_token.is_some() {
                    Some(AdmissionDenied::Lifecycle)
                } else {
                    None
                }
            }
            AdmissionIntent::ExclusiveAction => {
                if state.mutation.is_some() {
                    Some(AdmissionDenied::Mutation)
                } else if state.lifecycle.is_some() || state.exclusive_token.is_some() {
                    Some(AdmissionDenied::Lifecycle)
                } else if state.setting_token.is_some() {
                    Some(AdmissionDenied::WslSettingChange)
                } else {
                    None
                }
            }
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
    owner: MutationOwner,
}

impl fmt::Debug for MutationPermit {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("MutationPermit")
            .field("owner", &self.owner)
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
        let Some(mutation) = state
            .mutation
            .as_ref()
            .filter(|mutation| mutation.token == self.token)
        else {
            return;
        };
        debug_assert_eq!(mutation.owner, self.owner);
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
    cancellation: Option<CancellationSignal>,
}

impl fmt::Debug for LifecyclePermit {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LifecyclePermit")
            .finish_non_exhaustive()
    }
}

impl LifecyclePermit {
    pub fn cancellation(&self) -> CancellationSignal {
        self.cancellation
            .clone()
            .expect("lifecycle does not support cancellation")
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

pub struct ExclusivePermit {
    inner: Arc<AdmissionInner>,
    token: u64,
}

impl Drop for ExclusivePermit {
    fn drop(&mut self) {
        let coordinator = RuntimeAdmissionCoordinator {
            inner: Arc::clone(&self.inner),
        };
        let mut state = coordinator.lock_state();
        if state.exclusive_token == Some(self.token) {
            state.exclusive_token = None;
            state.revision = next_revision(state.revision);
        }
    }
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
    use crate::core::mutation::{
        LifecycleLeaseKind, MutationKind, MutationPhase, MutationTargetRef,
    };
    use crate::environment::types::{EnvironmentRef, SkillLocation, SkillLocationRef};

    fn native_global() -> SkillLocationRef {
        SkillLocationRef {
            environment: EnvironmentRef::Native,
            scope: SkillLocation::Global,
        }
    }

    #[test]
    fn library_mutation_binds_its_target_and_uses_the_active_cancellation_signal() {
        let admission = RuntimeAdmissionCoordinator::default();
        let permit = admission
            .begin_library_mutation(
                MutationKind::ManageLibraries,
                EnvironmentRef::Native,
                "library-1".to_string(),
            )
            .expect("Library mutation admitted");
        permit.transition(MutationPhase::Acquiring, None, true);

        assert_eq!(
            admission.snapshot().active.unwrap().target,
            MutationTargetRef::Library {
                environment: EnvironmentRef::Native,
                library_id: "library-1".to_string(),
            }
        );
        assert_eq!(admission.request_cancel(), Ok(true));
        assert!(permit.cancellation().is_cancelled());
    }

    #[test]
    fn setting_change_conflicts_with_mutation_and_lifecycle() {
        let admission = RuntimeAdmissionCoordinator::default();
        let mutation = admission
            .begin_mutation(MutationKind::Install, native_global())
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
    fn active_wizard_can_start_its_own_install() {
        let admission = RuntimeAdmissionCoordinator::default();
        let WizardAdmission::Reserved(reservation) = admission
            .admit_install_wizard(WizardWindowPresence::Absent)
            .expect("wizard reservation admitted")
        else {
            panic!("expected reservation");
        };
        reservation.activate("wizard-1".to_string());

        let install = admission
            .begin_install_from_active_wizard(native_global())
            .expect("wizard install admitted");

        assert_eq!(
            admission.active().map(|mutation| mutation.kind),
            Some(MutationKind::Install)
        );
        assert!(admission.install_wizard_snapshot().active);

        assert_eq!(
            admission
                .begin_install_from_active_wizard(native_global())
                .unwrap_err(),
            AppError::MutationBusy
        );
        drop(install);

        let retry = admission
            .begin_install_from_active_wizard(native_global())
            .expect("wizard install retry admitted");
        drop(retry);
    }

    #[test]
    fn wizard_install_requires_an_active_session() {
        let admission = RuntimeAdmissionCoordinator::default();

        assert_eq!(
            admission
                .begin_install_from_active_wizard(native_global())
                .unwrap_err(),
            AppError::InstallWizardSessionUnavailable
        );

        let external_mutation = admission
            .begin_mutation(MutationKind::Remove, native_global())
            .expect("external mutation admitted");
        assert_eq!(
            admission
                .begin_install_from_active_wizard(native_global())
                .unwrap_err(),
            AppError::InstallWizardSessionUnavailable
        );
        drop(external_mutation);

        let WizardAdmission::Reserved(_reservation) = admission
            .admit_install_wizard(WizardWindowPresence::Absent)
            .expect("wizard reservation admitted")
        else {
            panic!("expected reservation");
        };
        assert_eq!(
            admission
                .begin_install_from_active_wizard(native_global())
                .unwrap_err(),
            AppError::InstallWizardSessionUnavailable
        );
    }

    #[test]
    fn wizard_session_blocks_external_business_mutations_and_application_updates() {
        let admission = RuntimeAdmissionCoordinator::default();
        let WizardAdmission::Reserved(reservation) = admission
            .admit_install_wizard(WizardWindowPresence::Absent)
            .expect("wizard reservation admitted")
        else {
            panic!("expected reservation");
        };

        assert_eq!(
            admission
                .begin_mutation(MutationKind::Install, native_global())
                .unwrap_err(),
            AppError::InstallWizardActive
        );
        assert_eq!(
            admission
                .begin_lifecycle(LifecycleLeaseKind::ApplicationUpdate)
                .unwrap_err(),
            AppError::InstallWizardActive
        );

        reservation.activate("wizard-1".to_string());
        assert_eq!(
            admission
                .begin_mutation(MutationKind::Remove, native_global())
                .unwrap_err(),
            AppError::InstallWizardActive
        );
        let maintenance = admission
            .begin_lifecycle(LifecycleLeaseKind::RuntimeMaintenance)
            .expect("runtime maintenance admitted");
        assert_eq!(
            admission
                .begin_mutation(MutationKind::Remove, native_global())
                .unwrap_err(),
            AppError::InstallWizardActive
        );
        drop(maintenance);
    }

    #[test]
    fn active_business_work_blocks_a_new_wizard_session() {
        let admission = RuntimeAdmissionCoordinator::default();
        let mutation = admission
            .begin_mutation(MutationKind::Install, native_global())
            .expect("mutation admitted");
        assert_eq!(
            admission
                .admit_install_wizard(WizardWindowPresence::Absent)
                .unwrap_err(),
            AdmissionDenied::Mutation
        );
        drop(mutation);

        let lifecycle = admission
            .begin_lifecycle(LifecycleLeaseKind::ApplicationUpdate)
            .expect("lifecycle admitted");
        assert_eq!(
            admission
                .admit_install_wizard(WizardWindowPresence::Absent)
                .unwrap_err(),
            AdmissionDenied::Lifecycle
        );
        drop(lifecycle);

        let maintenance = admission
            .begin_lifecycle(LifecycleLeaseKind::RuntimeMaintenance)
            .expect("runtime maintenance admitted");
        assert!(admission
            .admit_install_wizard(WizardWindowPresence::Absent)
            .is_ok());
        drop(maintenance);
    }

    #[test]
    fn application_update_lifecycle_is_cancelable_only_when_enabled() {
        let admission = RuntimeAdmissionCoordinator::default();
        let lifecycle = admission
            .begin_cancelable_lifecycle(LifecycleLeaseKind::ApplicationUpdate)
            .expect("application update admitted");

        assert!(!admission.activity_snapshot().lifecycle.unwrap().cancelable);
        assert_eq!(admission.request_cancel_lifecycle(), Ok(false));

        admission.set_application_update_cancelable(true);
        assert!(admission.activity_snapshot().lifecycle.unwrap().cancelable);
        assert_eq!(admission.request_cancel_lifecycle(), Ok(true));
        assert!(lifecycle.cancellation().is_cancelled());

        admission.set_application_update_cancelable(false);
        assert!(!admission.activity_snapshot().lifecycle.unwrap().cancelable);
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

    #[test]
    fn idle_action_runs_without_holding_the_admission_lock() {
        let admission = RuntimeAdmissionCoordinator::default();

        let result = admission.with_idle(|| {
            assert!(matches!(
                admission.begin_mutation(MutationKind::Install, native_global()),
                Err(AppError::MutationBusy)
            ));
            admission.mutation_snapshot().revision
        });

        assert_eq!(result, Ok(1));
        assert_eq!(admission.mutation_snapshot().revision, 2);
        admission
            .begin_mutation(MutationKind::Install, native_global())
            .expect("exclusive action released admission");
    }

    #[test]
    fn exclusive_action_and_setting_change_conflict_in_both_directions() {
        let admission = RuntimeAdmissionCoordinator::default();
        admission
            .with_idle(|| {
                assert_eq!(
                    admission.begin_wsl_integration_change().unwrap_err(),
                    AdmissionDenied::Lifecycle
                );
            })
            .expect("exclusive action");

        let _setting = admission
            .begin_wsl_integration_change()
            .expect("setting change");
        assert!(admission.with_idle(|| ()).is_err());
    }
}

#[cfg(test)]
mod mutation_tests;

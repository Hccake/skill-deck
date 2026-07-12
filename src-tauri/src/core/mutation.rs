use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use serde::Serialize;
use specta::Type;

use crate::environment::types::ContextRef;
use crate::error::AppError;

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
    Repair,
    SaveAgentDefaults,
    BatchUpdate,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct ActiveMutation {
    pub kind: MutationKind,
    pub context: ContextRef,
    pub status_text: String,
    pub cancelable: bool,
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
pub struct SingleMutationController {
    state: Mutex<Option<MutationState>>,
}

impl SingleMutationController {
    pub fn begin(
        &self,
        kind: MutationKind,
        context: ContextRef,
        status_text: impl Into<String>,
    ) -> Result<MutationGuard<'_>, AppError> {
        let mut state = self
            .state
            .lock()
            .expect("mutation controller lock poisoned");
        if state.is_some() {
            return Err(AppError::MutationBusy);
        }
        let cancellation = CancellationSignal::default();
        *state = Some(MutationState {
            active: ActiveMutation {
                kind,
                context,
                status_text: status_text.into(),
                cancelable: true,
            },
            cancellation: cancellation.clone(),
        });
        Ok(MutationGuard {
            controller: self,
            cancellation,
        })
    }

    pub fn active(&self) -> Option<ActiveMutation> {
        self.state
            .lock()
            .expect("mutation controller lock poisoned")
            .as_ref()
            .map(|state| state.active.clone())
    }

    pub fn request_cancel(&self) -> Result<bool, AppError> {
        let state = self
            .state
            .lock()
            .expect("mutation controller lock poisoned");
        let Some(state) = state.as_ref() else {
            return Ok(false);
        };
        if !state.active.cancelable {
            return Ok(false);
        }
        state.cancellation.cancel();
        Ok(true)
    }
}

pub struct MutationGuard<'a> {
    controller: &'a SingleMutationController,
    cancellation: CancellationSignal,
}

impl MutationGuard<'_> {
    pub fn cancellation(&self) -> CancellationSignal {
        self.cancellation.clone()
    }

    pub fn set_cancelable(&self, cancelable: bool) {
        if let Some(state) = self
            .controller
            .state
            .lock()
            .expect("mutation controller lock poisoned")
            .as_mut()
        {
            state.active.cancelable = cancelable;
        }
    }
}

impl Drop for MutationGuard<'_> {
    fn drop(&mut self) {
        self.controller
            .state
            .lock()
            .expect("mutation controller lock poisoned")
            .take();
    }
}

#[cfg(test)]
mod tests {
    use super::{MutationKind, SingleMutationController};
    use crate::environment::types::{ContextRef, ContextScope, EnvironmentRef};

    fn host_global() -> ContextRef {
        ContextRef {
            environment: EnvironmentRef::Host,
            scope: ContextScope::Global,
        }
    }

    #[test]
    fn only_one_mutation_can_be_active_and_guard_releases_it() {
        let controller = SingleMutationController::default();
        {
            let _guard = controller
                .begin(MutationKind::Install, host_global(), "Installing")
                .expect("begin mutation");
            assert!(controller
                .begin(MutationKind::Remove, host_global(), "Removing")
                .is_err());
            assert_eq!(
                controller.active().expect("active mutation").kind,
                MutationKind::Install
            );
        }
        assert!(controller.active().is_none());
    }

    #[test]
    fn cancel_request_sets_signal_only_when_cancelable() {
        let controller = SingleMutationController::default();
        let guard = controller
            .begin(MutationKind::Install, host_global(), "Preparing")
            .expect("begin mutation");

        assert!(controller.request_cancel().expect("request cancel"));
        assert!(guard.cancellation().is_cancelled());

        guard.set_cancelable(false);
        assert!(!controller.request_cancel().expect("request cancel"));
    }
}

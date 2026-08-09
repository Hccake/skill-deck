use std::future::Future;
use std::pin::Pin;

use crate::application::mutation::coordinator::MutationUnitObserver;
use crate::application::mutation::plan::MutationPlan;
use crate::application::mutation::result::MutationUnitResult;
use crate::core::mutation::CancellationSignal;

pub type MutationFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

pub trait MutationPlanExecutor: Send + Sync {
    fn execute<'a>(
        &'a self,
        plan: MutationPlan,
        cancellation: CancellationSignal,
    ) -> MutationFuture<'a, Vec<MutationUnitResult>>;

    fn execute_with_observer<'a>(
        &'a self,
        plan: MutationPlan,
        cancellation: CancellationSignal,
        _observer: MutationUnitObserver<'a>,
    ) -> MutationFuture<'a, Vec<MutationUnitResult>> {
        self.execute(plan, cancellation)
    }
}

#[cfg(test)]
mod tests {
    use super::MutationPlanExecutor;
    use crate::application::mutation::coordinator::MutationUnitObserver;
    use crate::application::mutation::plan::MutationPlan;
    use crate::application::mutation::planning::{assemble_plan, MutationPlanDraft};
    use crate::application::mutation::result::{
        ErrorReport, MutationUnitResult, MutationUnitStatus, OperationErrorCode,
    };
    use crate::core::mutation::{CancellationSignal, MutationKind};
    use crate::environment::types::{EnvironmentRef, SkillLocation, SkillLocationRef};
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct RecordingExecutor {
        calls: AtomicUsize,
        cancelled_calls: AtomicUsize,
    }

    impl MutationPlanExecutor for RecordingExecutor {
        fn execute<'a>(
            &'a self,
            _plan: MutationPlan,
            cancellation: CancellationSignal,
        ) -> super::MutationFuture<'a, Vec<MutationUnitResult>> {
            Box::pin(async move {
                self.calls.fetch_add(1, Ordering::SeqCst);
                if cancellation.is_cancelled() {
                    self.cancelled_calls.fetch_add(1, Ordering::SeqCst);
                }
                vec![failed_result()]
            })
        }
    }

    fn plan() -> MutationPlan {
        assemble_plan(MutationPlanDraft {
            kind: MutationKind::Remove,
            payloads: Default::default(),
            units: Vec::new(),
        })
    }

    fn failed_result() -> MutationUnitResult {
        MutationUnitResult {
            unit_id: "remove:demo".to_string(),
            skill_name: "demo".to_string(),
            source: None,
            target: SkillLocationRef {
                environment: EnvironmentRef::Native,
                scope: SkillLocation::Global,
            },
            status: MutationUnitStatus::Failed,
            retryable: false,
            lock_committed: false,
            actual_mode: None,
            fallback_reason: None,
            agent_targets: Vec::new(),
            warnings: Vec::new(),
            error: Some(ErrorReport::new(OperationErrorCode::ExecutionFailed)),
            recovery: None,
        }
    }

    #[tokio::test]
    async fn normal_and_default_observer_execution_preserve_cancellation_and_results() {
        let executor = RecordingExecutor {
            calls: AtomicUsize::new(0),
            cancelled_calls: AtomicUsize::new(0),
        };
        let direct = executor
            .execute(plan(), CancellationSignal::default())
            .await;
        let cancellation = CancellationSignal::default();
        cancellation.cancel();
        let observer: MutationUnitObserver<'_> = std::sync::Arc::new(|_| {});
        let observed = executor
            .execute_with_observer(plan(), cancellation, observer)
            .await;

        assert_eq!(direct, observed);
        assert_eq!(direct[0].status, MutationUnitStatus::Failed);
        assert_eq!(
            direct[0].error.as_ref().map(|error| error.code),
            Some(OperationErrorCode::ExecutionFailed)
        );
        assert_eq!(executor.calls.load(Ordering::SeqCst), 2);
        assert_eq!(executor.cancelled_calls.load(Ordering::SeqCst), 1);
    }
}

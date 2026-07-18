use std::sync::{Arc, LazyLock};

use tokio::sync::{OwnedSemaphorePermit, Semaphore};

use crate::core::mutation::CancellationSignal;
use crate::error::AppError;

pub const SOURCE_CLONE_LIMIT: usize = 2;

static SHARED_SOURCE_CLONE_GATE: LazyLock<SourceCloneGate> =
    LazyLock::new(SourceCloneGate::default);

pub fn shared_source_clone_gate() -> &'static SourceCloneGate {
    &SHARED_SOURCE_CLONE_GATE
}

pub struct SourceCloneGate {
    permits: Arc<Semaphore>,
}

impl Default for SourceCloneGate {
    fn default() -> Self {
        Self::new(SOURCE_CLONE_LIMIT)
    }
}

impl SourceCloneGate {
    pub fn new(limit: usize) -> Self {
        assert!(limit > 0, "source clone limit must be positive");
        Self {
            permits: Arc::new(Semaphore::new(limit)),
        }
    }

    pub async fn acquire(
        &self,
        cancellation: &CancellationSignal,
    ) -> Result<OwnedSemaphorePermit, AppError> {
        tokio::select! {
            permit = self.permits.clone().acquire_owned() => permit.map_err(|_| {
                AppError::ExecutionFailed {
                    message: "source clone gate closed".to_string(),
                }
            }),
            () = cancellation.cancelled() => Err(AppError::MutationCancelled),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use super::*;
    use crate::core::mutation::CancellationSignal;

    #[tokio::test]
    async fn third_clone_waits_until_a_permit_is_released() {
        let gate = Arc::new(SourceCloneGate::new(2));
        let first = gate.acquire(&CancellationSignal::default()).await.unwrap();
        let _second = gate.acquire(&CancellationSignal::default()).await.unwrap();
        let third_gate = gate.clone();
        let mut third =
            tokio::spawn(async move { third_gate.acquire(&CancellationSignal::default()).await });

        assert!(tokio::time::timeout(Duration::from_millis(40), &mut third)
            .await
            .is_err());
        drop(first);
        let _third_permit = tokio::time::timeout(Duration::from_millis(200), third)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
    }

    #[tokio::test]
    async fn queued_clone_returns_when_cancelled() {
        let gate = Arc::new(SourceCloneGate::new(1));
        let _held = gate.acquire(&CancellationSignal::default()).await.unwrap();
        let cancellation = CancellationSignal::default();
        let queued_gate = gate.clone();
        let queued_cancellation = cancellation.clone();
        let queued = tokio::spawn(async move { queued_gate.acquire(&queued_cancellation).await });

        cancellation.cancel();
        let error = tokio::time::timeout(Duration::from_millis(200), queued)
            .await
            .unwrap()
            .unwrap()
            .unwrap_err();
        assert!(matches!(error, crate::error::AppError::MutationCancelled));
    }
}

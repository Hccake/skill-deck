use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use tokio::sync::watch;

use crate::application::payload_session::{
    PayloadCleanupReport, PayloadSessionMaintenance, PayloadSessionManager,
};
use crate::application::runtime_admission::RuntimeAdmissionCoordinator;
use crate::core::mutation::LifecycleLeaseKind;
use crate::environment::maintenance::{
    MaintenanceIssueCode, RuntimeMaintenanceState, RuntimeMaintenanceStatus,
};
use crate::environment::native::acquire::NativePayloadSessionStorage;
use crate::environment::types::{EnvironmentKey, EnvironmentRef};
use crate::environment::wsl::operations::acquire::WslPayloadSessionStorage;
use crate::environment::wsl::WslRuntime;
use crate::error::AppError;
use crate::runtime::recovery::RuntimeRecoveryGraph;

pub struct MaintenanceTaskOutcome {
    pub payload: Result<PayloadCleanupReport, AppError>,
    pub recovery: Result<(), AppError>,
}

pub type MaintenanceFuture<'a> = Pin<Box<dyn Future<Output = MaintenanceTaskOutcome> + Send + 'a>>;

pub trait RuntimeMaintenanceBackend: Send + Sync {
    fn run<'a>(&'a self, environment: &'a EnvironmentRef) -> MaintenanceFuture<'a>;
}

pub struct RuntimeMaintenanceTasks {
    payloads: Arc<PayloadSessionManager>,
    native_payload_storage: Arc<NativePayloadSessionStorage>,
    recovery: Arc<RuntimeRecoveryGraph>,
    environments: Arc<WslRuntime>,
    mutation: Arc<RuntimeAdmissionCoordinator>,
}

impl RuntimeMaintenanceTasks {
    pub fn new(
        payloads: Arc<PayloadSessionManager>,
        native_payload_storage: Arc<NativePayloadSessionStorage>,
        recovery: Arc<RuntimeRecoveryGraph>,
        environments: Arc<WslRuntime>,
        mutation: Arc<RuntimeAdmissionCoordinator>,
    ) -> Self {
        Self {
            payloads,
            native_payload_storage,
            recovery,
            environments,
            mutation,
        }
    }

    async fn run_native(&self) -> MaintenanceTaskOutcome {
        if self
            .mutation
            .active_for_environment(&EnvironmentRef::Native)
        {
            return MaintenanceTaskOutcome {
                payload: Err(AppError::MutationBusy),
                recovery: Err(AppError::MutationBusy),
            };
        }
        let payload = match self.payloads.protected_session_ids(&EnvironmentRef::Native) {
            Ok(protected) => self.native_payload_storage.sweep_orphans(&protected).await,
            Err(error) => Err(error),
        };
        let recovery = self.recovery.reindex_native().await;
        MaintenanceTaskOutcome { payload, recovery }
    }

    async fn run_wsl(
        &self,
        environment: &EnvironmentRef,
        distro_name: &str,
    ) -> MaintenanceTaskOutcome {
        if self.mutation.active_for_environment(environment) {
            return MaintenanceTaskOutcome {
                payload: Err(AppError::MutationBusy),
                recovery: Err(AppError::MutationBusy),
            };
        }
        let workspace = match self.environments.workspace(distro_name) {
            Ok(workspace) => workspace,
            Err(error) => {
                return MaintenanceTaskOutcome {
                    payload: Err(maintenance_environment_error(environment, &error)),
                    recovery: Err(maintenance_environment_error(environment, &error)),
                };
            }
        };
        let payload = match self.payloads.protected_session_ids(environment) {
            Ok(protected) => {
                WslPayloadSessionStorage::new(workspace.clone())
                    .sweep_orphans(&protected)
                    .await
            }
            Err(error) => Err(error),
        };
        let recovery = self.recovery.reindex_wsl(workspace).await;
        MaintenanceTaskOutcome { payload, recovery }
    }
}

impl RuntimeMaintenanceBackend for RuntimeMaintenanceTasks {
    fn run<'a>(&'a self, environment: &'a EnvironmentRef) -> MaintenanceFuture<'a> {
        Box::pin(async move {
            let _lease = match self
                .mutation
                .begin_lifecycle(LifecycleLeaseKind::RuntimeMaintenance)
            {
                Ok(lease) => lease,
                Err(error) => {
                    return MaintenanceTaskOutcome {
                        payload: Err(error.clone()),
                        recovery: Err(error),
                    };
                }
            };
            match environment {
                EnvironmentRef::Native => self.run_native().await,
                EnvironmentRef::Wsl { distro_name } => self.run_wsl(environment, distro_name).await,
            }
        })
    }
}

fn maintenance_environment_error(environment: &EnvironmentRef, error: &AppError) -> AppError {
    AppError::EnvironmentUnavailable {
        environment: environment.clone(),
        message: error.to_string(),
    }
}

struct MaintenanceEntry {
    connection_revision: Option<u64>,
    generation: u64,
    status: RuntimeMaintenanceStatus,
    running: Option<watch::Sender<bool>>,
}

pub struct RuntimeMaintenanceCoordinator {
    payloads: Arc<PayloadSessionManager>,
    backend: Arc<dyn RuntimeMaintenanceBackend>,
    entries: Mutex<HashMap<EnvironmentKey, MaintenanceEntry>>,
}

impl RuntimeMaintenanceCoordinator {
    pub fn new(
        payloads: Arc<PayloadSessionManager>,
        backend: Arc<dyn RuntimeMaintenanceBackend>,
    ) -> Self {
        Self {
            payloads,
            backend,
            entries: Mutex::new(HashMap::new()),
        }
    }

    pub fn register(
        &self,
        environment: EnvironmentRef,
    ) -> Result<RuntimeMaintenanceStatus, AppError> {
        let key = EnvironmentKey::from_ref(&environment);
        let mut entries = self.lock_entries()?;
        let status = entries
            .entry(key)
            .or_insert_with(|| MaintenanceEntry {
                connection_revision: None,
                generation: 0,
                status: RuntimeMaintenanceStatus {
                    environment,
                    state: RuntimeMaintenanceState::Pending,
                    issues: Vec::new(),
                },
                running: None,
            })
            .status
            .clone();
        Ok(status)
    }

    pub async fn start(
        &self,
        environment: EnvironmentRef,
        connection_revision: u64,
    ) -> Result<RuntimeMaintenanceStatus, AppError> {
        self.run(environment, connection_revision).await
    }

    async fn run(
        &self,
        environment: EnvironmentRef,
        connection_revision: u64,
    ) -> Result<RuntimeMaintenanceStatus, AppError> {
        let key = EnvironmentKey::from_ref(&environment);
        let generation = loop {
            let waiting = {
                let mut entries = self.lock_entries()?;
                let entry = entries
                    .entry(key.clone())
                    .or_insert_with(|| MaintenanceEntry {
                        connection_revision: None,
                        generation: 0,
                        status: RuntimeMaintenanceStatus {
                            environment: environment.clone(),
                            state: RuntimeMaintenanceState::Pending,
                            issues: Vec::new(),
                        },
                        running: None,
                    });
                if let Some(running) = &entry.running {
                    Some(running.subscribe())
                } else if entry.status.state != RuntimeMaintenanceState::Failed
                    && entry
                        .connection_revision
                        .is_some_and(|revision| revision >= connection_revision)
                {
                    return Ok(entry.status.clone());
                } else {
                    entry.connection_revision = Some(connection_revision);
                    entry.generation = entry.generation.saturating_add(1);
                    entry.status.state = RuntimeMaintenanceState::Pending;
                    entry.status.issues.clear();
                    let (completion, _) = watch::channel(false);
                    entry.running = Some(completion);
                    let generation = entry.generation;
                    drop(entries);
                    if let Err(error) = self.payloads.begin_maintenance(&environment) {
                        let completion = {
                            let mut entries = self.lock_entries()?;
                            let entry = entries.get_mut(&key).ok_or_else(state_error)?;
                            if entry.generation != generation {
                                return Ok(entry.status.clone());
                            }
                            entry.status.state = RuntimeMaintenanceState::Failed;
                            entry.status.issues = vec![MaintenanceIssueCode::PayloadSweepFailed];
                            entry.running.take()
                        };
                        if let Some(completion) = completion {
                            let _ = completion.send(true);
                        }
                        return Err(error);
                    }
                    break generation;
                }
            };
            if let Some(mut waiting) = waiting {
                while !*waiting.borrow() {
                    waiting.changed().await.map_err(|_| state_error())?;
                }
            }
        };

        let outcome = self.backend.run(&environment).await;
        let mut issues = Vec::new();
        match outcome.payload {
            Ok(report) => {
                if self
                    .payloads
                    .record_maintenance_report(&environment, &report)
                    .is_err()
                {
                    issues.push(MaintenanceIssueCode::PayloadSweepFailed);
                }
            }
            Err(error) => {
                log::warn!("Payload maintenance failed for {environment:?}: {error}");
                issues.push(MaintenanceIssueCode::PayloadSweepFailed);
            }
        }
        if let Err(error) = outcome.recovery {
            log::warn!("Recovery maintenance failed for {environment:?}: {error}");
            issues.push(MaintenanceIssueCode::RecoveryReindexFailed);
        }
        issues.sort();
        issues.dedup();

        let state = if issues.is_empty() {
            match self.payloads.complete_maintenance(&environment) {
                Ok(()) => RuntimeMaintenanceState::Ready,
                Err(error) => {
                    log::warn!(
                        "Failed to complete Payload maintenance for {environment:?}: {error}"
                    );
                    issues.push(MaintenanceIssueCode::PayloadSweepFailed);
                    let _ = self.payloads.fail_maintenance(&environment);
                    RuntimeMaintenanceState::Failed
                }
            }
        } else {
            if let Err(error) = self.payloads.fail_maintenance(&environment) {
                log::warn!("Failed to close Payload maintenance gate for {environment:?}: {error}");
            }
            RuntimeMaintenanceState::Failed
        };
        let (status, notify) = {
            let mut entries = self.lock_entries()?;
            let entry = entries.get_mut(&key).ok_or_else(state_error)?;
            if entry.generation != generation {
                return Ok(entry.status.clone());
            }
            entry.status.state = state;
            entry.status.issues = issues;
            let notify = entry.running.take();
            (entry.status.clone(), notify)
        };
        if let Some(completion) = notify {
            let _ = completion.send(true);
        }
        Ok(status)
    }

    fn lock_entries(
        &self,
    ) -> Result<std::sync::MutexGuard<'_, HashMap<EnvironmentKey, MaintenanceEntry>>, AppError>
    {
        self.entries.lock().map_err(|_| state_error())
    }
}

fn state_error() -> AppError {
    AppError::Io {
        message: "runtime maintenance state is unavailable".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    use tokio::sync::{Notify, Semaphore};

    use super::*;
    use crate::application::payload_session::{PayloadSessionLimits, PayloadSessionManager};
    use crate::environment::types::EnvironmentRef;

    struct FakeBackend {
        calls: AtomicUsize,
        failing_calls: BTreeSet<usize>,
        block_calls: bool,
        started: Notify,
        release: Semaphore,
    }

    impl FakeBackend {
        fn new(failing_calls: impl IntoIterator<Item = usize>) -> Self {
            Self {
                calls: AtomicUsize::new(0),
                failing_calls: failing_calls.into_iter().collect(),
                block_calls: false,
                started: Notify::new(),
                release: Semaphore::new(0),
            }
        }

        fn blocked() -> Self {
            Self {
                block_calls: true,
                ..Self::new([])
            }
        }

        async fn wait_for_calls(&self, expected: usize) {
            loop {
                let started = self.started.notified();
                if self.calls.load(Ordering::SeqCst) >= expected {
                    return;
                }
                started.await;
            }
        }

        fn release_one(&self) {
            self.release.add_permits(1);
        }
    }

    impl RuntimeMaintenanceBackend for FakeBackend {
        fn run<'a>(&'a self, _environment: &'a EnvironmentRef) -> MaintenanceFuture<'a> {
            Box::pin(async move {
                let call = self.calls.fetch_add(1, Ordering::SeqCst);
                self.started.notify_waiters();
                if self.block_calls {
                    self.release
                        .acquire()
                        .await
                        .expect("test release semaphore remains open")
                        .forget();
                }
                if self.failing_calls.contains(&call) {
                    MaintenanceTaskOutcome {
                        payload: Err(crate::error::AppError::Io {
                            message: "payload failed".to_string(),
                        }),
                        recovery: Err(crate::error::AppError::Io {
                            message: "recovery failed".to_string(),
                        }),
                    }
                } else {
                    MaintenanceTaskOutcome {
                        payload: Ok(
                            crate::application::payload_session::PayloadCleanupReport::default(),
                        ),
                        recovery: Ok(()),
                    }
                }
            })
        }
    }

    fn payloads() -> Arc<PayloadSessionManager> {
        Arc::new(PayloadSessionManager::in_memory(
            PayloadSessionLimits {
                ttl_ms: 1_000,
                max_sessions: 8,
                max_bytes: 1_024,
            },
            || 1,
        ))
    }

    #[tokio::test]
    async fn same_revision_ready_does_not_run_again() {
        let payloads = payloads();
        let backend = Arc::new(FakeBackend::new([]));
        let coordinator = RuntimeMaintenanceCoordinator::new(payloads.clone(), backend.clone());

        let first = coordinator.start(EnvironmentRef::Native, 0).await.unwrap();
        let second = coordinator.start(EnvironmentRef::Native, 0).await.unwrap();

        assert_eq!(first.state, RuntimeMaintenanceState::Ready);
        assert_eq!(second.state, RuntimeMaintenanceState::Ready);
        assert_eq!(backend.calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn same_revision_failed_reenters_after_explicit_start() {
        let payloads = payloads();
        let backend = Arc::new(FakeBackend::new([0]));
        let coordinator = RuntimeMaintenanceCoordinator::new(payloads.clone(), backend.clone());

        let failed = coordinator.start(EnvironmentRef::Native, 0).await.unwrap();
        assert_eq!(failed.state, RuntimeMaintenanceState::Failed);
        assert_eq!(
            failed.issues,
            vec![
                MaintenanceIssueCode::PayloadSweepFailed,
                MaintenanceIssueCode::RecoveryReindexFailed,
            ]
        );
        assert!(payloads
            .discover(EnvironmentRef::Native, "blocked")
            .await
            .is_err());

        let repeated = coordinator.start(EnvironmentRef::Native, 0).await.unwrap();
        assert_eq!(repeated.state, RuntimeMaintenanceState::Ready);
        assert_eq!(backend.calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn newer_revision_reinitializes_failed_maintenance() {
        let payloads = payloads();
        let backend = Arc::new(FakeBackend::new([0]));
        let coordinator = RuntimeMaintenanceCoordinator::new(payloads.clone(), backend.clone());

        let failed = coordinator.start(EnvironmentRef::Native, 0).await.unwrap();
        assert_eq!(failed.state, RuntimeMaintenanceState::Failed);

        let ready = coordinator.start(EnvironmentRef::Native, 1).await.unwrap();
        assert_eq!(ready.state, RuntimeMaintenanceState::Ready);
        payloads
            .discover(EnvironmentRef::Native, "ready")
            .await
            .expect("payload gate reopened");
    }

    #[tokio::test]
    async fn concurrent_case_alias_starts_share_one_revision_task() {
        let backend = Arc::new(FakeBackend::blocked());
        let coordinator = Arc::new(RuntimeMaintenanceCoordinator::new(
            payloads(),
            backend.clone(),
        ));
        let first_coordinator = coordinator.clone();
        let first = tokio::spawn(async move {
            first_coordinator
                .start(
                    EnvironmentRef::Wsl {
                        distro_name: "Ubuntu".to_string(),
                    },
                    7,
                )
                .await
        });
        backend.wait_for_calls(1).await;
        let second_coordinator = coordinator.clone();
        let second = tokio::spawn(async move {
            second_coordinator
                .start(
                    EnvironmentRef::Wsl {
                        distro_name: "ubuntu".to_string(),
                    },
                    7,
                )
                .await
        });
        backend.release_one();

        let (first, second) = tokio::join!(first, second);
        let first = first.unwrap().unwrap();
        let second = second.unwrap().unwrap();
        assert_eq!(first.state, RuntimeMaintenanceState::Ready);
        assert_eq!(second.state, RuntimeMaintenanceState::Ready);
        assert_eq!(backend.calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn newer_revision_waits_for_the_running_revision_then_runs() {
        let backend = Arc::new(FakeBackend::blocked());
        let coordinator = Arc::new(RuntimeMaintenanceCoordinator::new(
            payloads(),
            backend.clone(),
        ));
        let first_coordinator = coordinator.clone();
        let first =
            tokio::spawn(async move { first_coordinator.start(EnvironmentRef::Native, 3).await });
        backend.wait_for_calls(1).await;

        let second_coordinator = coordinator.clone();
        let second =
            tokio::spawn(async move { second_coordinator.start(EnvironmentRef::Native, 4).await });
        backend.release_one();
        backend.wait_for_calls(2).await;
        backend.release_one();

        assert_eq!(
            first.await.unwrap().unwrap().state,
            RuntimeMaintenanceState::Ready
        );
        assert_eq!(
            second.await.unwrap().unwrap().state,
            RuntimeMaintenanceState::Ready
        );
        assert_eq!(backend.calls.load(Ordering::SeqCst), 2);
    }
}

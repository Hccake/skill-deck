use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use tokio::sync::watch;

use crate::application::payload_session::{
    PayloadCleanupReport, PayloadSessionMaintenance, PayloadSessionManager,
};
use crate::application::recovery_runtime::RuntimeRecoveryGraph;
use crate::core::mutation::{LifecycleLeaseKind, SingleMutationController};
use crate::environment::maintenance::{
    MaintenanceIssueCode, RuntimeMaintenanceState, RuntimeMaintenanceStatus,
};
use crate::environment::native::acquire::NativePayloadSessionStorage;
use crate::environment::types::{EnvironmentKey, EnvironmentRef};
use crate::environment::wsl::operations::acquire::WslPayloadSessionStorage;
use crate::environment::wsl::EnvironmentRegistry;
use crate::error::AppError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MaintenanceTaskSelection {
    pub payload: bool,
    pub recovery: bool,
}

impl MaintenanceTaskSelection {
    pub const fn all() -> Self {
        Self {
            payload: true,
            recovery: true,
        }
    }

    fn from_issues(issues: &[MaintenanceIssueCode]) -> Self {
        Self {
            payload: issues.contains(&MaintenanceIssueCode::PayloadSweepFailed),
            recovery: issues.contains(&MaintenanceIssueCode::RecoveryReindexFailed),
        }
    }
}

pub struct MaintenanceTaskOutcome {
    pub payload: Option<Result<PayloadCleanupReport, AppError>>,
    pub recovery: Option<Result<(), AppError>>,
}

pub type MaintenanceFuture<'a> = Pin<Box<dyn Future<Output = MaintenanceTaskOutcome> + Send + 'a>>;

pub trait RuntimeMaintenanceBackend: Send + Sync {
    fn run<'a>(
        &'a self,
        environment: &'a EnvironmentRef,
        selection: MaintenanceTaskSelection,
    ) -> MaintenanceFuture<'a>;
}

pub struct RuntimeMaintenanceTasks {
    payloads: Arc<PayloadSessionManager>,
    native_payload_storage: Arc<NativePayloadSessionStorage>,
    recovery: Arc<RuntimeRecoveryGraph>,
    environments: Arc<EnvironmentRegistry>,
    mutation: Arc<SingleMutationController>,
}

impl RuntimeMaintenanceTasks {
    pub fn new(
        payloads: Arc<PayloadSessionManager>,
        native_payload_storage: Arc<NativePayloadSessionStorage>,
        recovery: Arc<RuntimeRecoveryGraph>,
        environments: Arc<EnvironmentRegistry>,
        mutation: Arc<SingleMutationController>,
    ) -> Self {
        Self {
            payloads,
            native_payload_storage,
            recovery,
            environments,
            mutation,
        }
    }

    async fn run_host(&self, selection: MaintenanceTaskSelection) -> MaintenanceTaskOutcome {
        if self.mutation.active_for_environment(&EnvironmentRef::Host) {
            return MaintenanceTaskOutcome {
                payload: selection.payload.then_some(Err(AppError::MutationBusy)),
                recovery: selection.recovery.then_some(Err(AppError::MutationBusy)),
            };
        }
        let payload = if selection.payload {
            Some(
                match self.payloads.protected_session_ids(&EnvironmentRef::Host) {
                    Ok(protected) => self.native_payload_storage.sweep_orphans(&protected).await,
                    Err(error) => Err(error),
                },
            )
        } else {
            None
        };
        let recovery = if selection.recovery {
            Some(self.recovery.reindex_host().await)
        } else {
            None
        };
        MaintenanceTaskOutcome { payload, recovery }
    }

    async fn run_wsl(
        &self,
        environment: &EnvironmentRef,
        distro_name: &str,
        selection: MaintenanceTaskSelection,
    ) -> MaintenanceTaskOutcome {
        if self.mutation.active_for_environment(environment) {
            return MaintenanceTaskOutcome {
                payload: selection.payload.then_some(Err(AppError::MutationBusy)),
                recovery: selection.recovery.then_some(Err(AppError::MutationBusy)),
            };
        }
        let payloads = Arc::clone(&self.payloads);
        let recovery = Arc::clone(&self.recovery);
        let operation_environment = environment.clone();
        let result = self
            .environments
            .with_session(distro_name, move |session| {
                let payloads = Arc::clone(&payloads);
                let recovery = Arc::clone(&recovery);
                let environment = operation_environment.clone();
                async move {
                    let payload = if selection.payload {
                        Some(match payloads.protected_session_ids(&environment) {
                            Ok(protected) => {
                                WslPayloadSessionStorage::new(session.clone())
                                    .sweep_orphans(&protected)
                                    .await
                            }
                            Err(error) => Err(error),
                        })
                    } else {
                        None
                    };
                    let recovery = if selection.recovery {
                        Some(recovery.reindex_wsl(session).await)
                    } else {
                        None
                    };
                    Ok(MaintenanceTaskOutcome { payload, recovery })
                }
            })
            .await;
        match result {
            Ok(outcome) => outcome,
            Err(error) => MaintenanceTaskOutcome {
                payload: selection
                    .payload
                    .then_some(Err(maintenance_environment_error(environment, &error))),
                recovery: selection
                    .recovery
                    .then_some(Err(maintenance_environment_error(environment, &error))),
            },
        }
    }
}

impl RuntimeMaintenanceBackend for RuntimeMaintenanceTasks {
    fn run<'a>(
        &'a self,
        environment: &'a EnvironmentRef,
        selection: MaintenanceTaskSelection,
    ) -> MaintenanceFuture<'a> {
        Box::pin(async move {
            let _lease = match self
                .mutation
                .begin_lifecycle(LifecycleLeaseKind::RuntimeMaintenance)
            {
                Ok(lease) => lease,
                Err(error) => {
                    return MaintenanceTaskOutcome {
                        payload: selection.payload.then_some(Err(error.clone())),
                        recovery: selection.recovery.then_some(Err(error)),
                    };
                }
            };
            match environment {
                EnvironmentRef::Host => self.run_host(selection).await,
                EnvironmentRef::Wsl { distro_name } => {
                    self.run_wsl(environment, distro_name, selection).await
                }
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
    generation: u64,
    status: RuntimeMaintenanceStatus,
    running: Option<watch::Sender<bool>>,
}

type MaintenanceListener = Arc<dyn Fn(RuntimeMaintenanceStatus) + Send + Sync>;

pub struct RuntimeMaintenanceCoordinator {
    payloads: Arc<PayloadSessionManager>,
    backend: Arc<dyn RuntimeMaintenanceBackend>,
    entries: Mutex<HashMap<EnvironmentKey, MaintenanceEntry>>,
    listener: Mutex<Option<MaintenanceListener>>,
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
            listener: Mutex::new(None),
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

    pub fn statuses(&self) -> Result<Vec<RuntimeMaintenanceStatus>, AppError> {
        let entries = self.lock_entries()?;
        let mut statuses = entries
            .iter()
            .map(|(key, entry)| (key.clone(), entry.status.clone()))
            .collect::<Vec<_>>();
        statuses.sort_by(|left, right| left.0.cmp(&right.0));
        Ok(statuses.into_iter().map(|(_, status)| status).collect())
    }

    pub fn set_listener(
        &self,
        listener: impl Fn(RuntimeMaintenanceStatus) + Send + Sync + 'static,
    ) -> Result<(), AppError> {
        *self.listener.lock().map_err(|_| state_error())? = Some(Arc::new(listener));
        Ok(())
    }

    pub async fn start(
        &self,
        environment: EnvironmentRef,
    ) -> Result<RuntimeMaintenanceStatus, AppError> {
        self.run(environment, false).await
    }

    pub async fn retry(
        &self,
        environment: EnvironmentRef,
    ) -> Result<RuntimeMaintenanceStatus, AppError> {
        self.run(environment, true).await
    }

    async fn run(
        &self,
        environment: EnvironmentRef,
        retry: bool,
    ) -> Result<RuntimeMaintenanceStatus, AppError> {
        let key = EnvironmentKey::from_ref(&environment);
        let (generation, selection) = loop {
            let waiting = {
                let mut entries = self.lock_entries()?;
                let entry = entries
                    .entry(key.clone())
                    .or_insert_with(|| MaintenanceEntry {
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
                } else if retry && entry.status.state == RuntimeMaintenanceState::Ready {
                    return Ok(entry.status.clone());
                } else {
                    let selection =
                        if retry && entry.status.state == RuntimeMaintenanceState::Failed {
                            MaintenanceTaskSelection::from_issues(&entry.status.issues)
                        } else {
                            MaintenanceTaskSelection::all()
                        };
                    entry.generation = entry.generation.saturating_add(1);
                    entry.status.state = RuntimeMaintenanceState::Pending;
                    entry.status.issues.clear();
                    let (completion, _) = watch::channel(false);
                    entry.running = Some(completion);
                    let status = entry.status.clone();
                    let generation = entry.generation;
                    drop(entries);
                    if let Err(error) = self.payloads.begin_maintenance(&environment) {
                        let (status, completion) = {
                            let mut entries = self.lock_entries()?;
                            let entry = entries.get_mut(&key).ok_or_else(state_error)?;
                            if entry.generation != generation {
                                return Ok(entry.status.clone());
                            }
                            entry.status.state = RuntimeMaintenanceState::Failed;
                            entry.status.issues = vec![MaintenanceIssueCode::PayloadSweepFailed];
                            (entry.status.clone(), entry.running.take())
                        };
                        if let Some(completion) = completion {
                            let _ = completion.send(true);
                        }
                        self.publish(status);
                        return Err(error);
                    }
                    self.publish(status);
                    break (generation, selection);
                }
            };
            if let Some(mut waiting) = waiting {
                while !*waiting.borrow() {
                    waiting.changed().await.map_err(|_| state_error())?;
                }
                return self
                    .lock_entries()?
                    .get(&key)
                    .map(|entry| entry.status.clone())
                    .ok_or_else(state_error);
            }
        };

        let outcome = self.backend.run(&environment, selection).await;
        let mut issues = Vec::new();
        if let Some(result) = outcome.payload {
            match result {
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
        }
        if let Some(Err(error)) = outcome.recovery {
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
        self.publish(status.clone());
        Ok(status)
    }

    fn publish(&self, status: RuntimeMaintenanceStatus) {
        let listener = self
            .listener
            .lock()
            .ok()
            .and_then(|listener| listener.clone());
        if let Some(listener) = listener {
            listener(status);
        }
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
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    use super::*;
    use crate::application::payload_session::{PayloadSessionLimits, PayloadSessionManager};
    use crate::environment::types::EnvironmentRef;

    struct FakeBackend {
        calls: AtomicUsize,
        selections: Mutex<Vec<MaintenanceTaskSelection>>,
    }

    impl RuntimeMaintenanceBackend for FakeBackend {
        fn run<'a>(
            &'a self,
            _environment: &'a EnvironmentRef,
            selection: MaintenanceTaskSelection,
        ) -> MaintenanceFuture<'a> {
            Box::pin(async move {
                let call = self.calls.fetch_add(1, Ordering::SeqCst);
                self.selections.lock().unwrap().push(selection);
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                if call == 0 {
                    MaintenanceTaskOutcome {
                        payload: selection.payload.then(|| {
                            Err(crate::error::AppError::Io {
                                message: "payload failed".to_string(),
                            })
                        }),
                        recovery: selection.recovery.then(|| {
                            Err(crate::error::AppError::Io {
                                message: "recovery failed".to_string(),
                            })
                        }),
                    }
                } else {
                    MaintenanceTaskOutcome {
                        payload: selection.payload.then(|| {
                            Ok(
                                crate::application::payload_session::PayloadCleanupReport::default(
                                ),
                            )
                        }),
                        recovery: selection.recovery.then_some(Ok(())),
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
    async fn failed_maintenance_is_retryable_and_retries_only_failed_tasks() {
        let payloads = payloads();
        let backend = Arc::new(FakeBackend {
            calls: AtomicUsize::new(0),
            selections: Mutex::new(Vec::new()),
        });
        let coordinator = RuntimeMaintenanceCoordinator::new(payloads.clone(), backend.clone());

        let failed = coordinator.start(EnvironmentRef::Host).await.unwrap();
        assert_eq!(failed.state, RuntimeMaintenanceState::Failed);
        assert_eq!(
            failed.issues,
            vec![
                MaintenanceIssueCode::PayloadSweepFailed,
                MaintenanceIssueCode::RecoveryReindexFailed,
            ]
        );
        assert!(payloads
            .discover(EnvironmentRef::Host, "blocked")
            .await
            .is_err());

        let ready = coordinator.retry(EnvironmentRef::Host).await.unwrap();
        assert_eq!(ready.state, RuntimeMaintenanceState::Ready);
        assert_eq!(
            backend.selections.lock().unwrap().as_slice(),
            [
                MaintenanceTaskSelection::all(),
                MaintenanceTaskSelection::all(),
            ]
        );
        payloads
            .discover(EnvironmentRef::Host, "ready")
            .await
            .expect("payload gate reopened");
    }

    #[tokio::test]
    async fn concurrent_case_alias_starts_share_one_in_flight_task() {
        let backend = Arc::new(FakeBackend {
            calls: AtomicUsize::new(1),
            selections: Mutex::new(Vec::new()),
        });
        let coordinator = Arc::new(RuntimeMaintenanceCoordinator::new(
            payloads(),
            backend.clone(),
        ));
        let first = coordinator.start(EnvironmentRef::Wsl {
            distro_name: "Ubuntu".to_string(),
        });
        let second = coordinator.start(EnvironmentRef::Wsl {
            distro_name: "ubuntu".to_string(),
        });

        let (first, second) = tokio::join!(first, second);
        let first = first.unwrap();
        let second = second.unwrap();
        assert_eq!(first.state, RuntimeMaintenanceState::Ready);
        assert_eq!(second.state, RuntimeMaintenanceState::Ready);
        assert_eq!(backend.calls.load(Ordering::SeqCst), 2);
        assert_eq!(coordinator.statuses().unwrap().len(), 1);
        assert_eq!(
            coordinator.statuses().unwrap()[0].environment,
            EnvironmentRef::Wsl {
                distro_name: "Ubuntu".to_string(),
            }
        );
    }
}

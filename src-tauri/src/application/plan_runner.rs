use std::sync::{Arc, Mutex};

use crate::application::install::{InstallFuture, InstallPlanExecutor};
use crate::application::mutation::coordinator::{
    BoxFuture, MutationCoordinator, MutationUnitObserver, PreparedLockCommitter,
    RuntimeRevisionSnapshot, RuntimeRevisionSource,
};
use crate::application::mutation::plan::{MutationPlan, RuntimeRevisions};
use crate::application::mutation::result::{ErrorReport, MutationUnitResult, MutationUnitStatus};
use crate::application::recovery_runtime::{RuntimeRecoveryGraph, RuntimeRecoveryService};
use crate::core::mutation::CancellationSignal;
use crate::environment::native::atomic_file::NativeAtomicDocumentIo;
use crate::environment::native::materialize::NativePreparedEntryExecutor;
use crate::environment::recovery::RecoveryMarkerStore;
use crate::environment::runtime::ExecutionBackend;
use crate::environment::types::{
    normalized_wsl_distro_name, same_environment_identity, ContextRef, EnvironmentKey,
    EnvironmentRef,
};
use crate::environment::wsl::operations::atomic_file::WslAtomicDocumentIo;
use crate::environment::wsl::operations::materialize::WslPreparedEntryExecutor;
use crate::environment::wsl::WslRuntime;
use crate::error::AppError;
use crate::storage::lock_plan::{LockCommitReceipt, LockPlanCommitter, PreparedLockMutation};

pub struct RuntimeLockCommitter {
    environments: Arc<WslRuntime>,
}

impl RuntimeLockCommitter {
    pub fn new(environments: Arc<WslRuntime>) -> Self {
        Self { environments }
    }
}

impl PreparedLockCommitter for RuntimeLockCommitter {
    fn commit<'a>(
        &'a self,
        mutation: &'a PreparedLockMutation,
    ) -> BoxFuture<'a, Result<LockCommitReceipt, AppError>> {
        Box::pin(async move {
            match &mutation.target.environment {
                EnvironmentRef::Host => {
                    LockPlanCommitter::new(Arc::new(NativeAtomicDocumentIo))
                        .commit(mutation.clone())
                        .await
                }
                EnvironmentRef::Wsl { distro_name } => {
                    let mutation = mutation.clone();
                    let workspace = self.environments.workspace(distro_name)?;
                    LockPlanCommitter::new(Arc::new(WslAtomicDocumentIo::new(workspace)))
                        .commit(mutation)
                        .await
                }
            }
        })
    }
}

pub struct RuntimePlanExecutor {
    environments: Arc<WslRuntime>,
    native_recovery: Arc<dyn RecoveryMarkerStore>,
    locks: Arc<dyn PreparedLockCommitter>,
    revisions: Arc<dyn RuntimeRevisionSource>,
    recovery_graph: Option<Arc<RuntimeRecoveryGraph>>,
}

#[derive(Clone)]
pub struct RuntimeExecutionDependencies {
    recovery: Arc<RuntimeRecoveryGraph>,
    locks: Arc<dyn PreparedLockCommitter>,
}

impl RuntimeExecutionDependencies {
    pub fn new(
        environments: Arc<WslRuntime>,
        recovery_root: std::path::PathBuf,
    ) -> Result<Self, AppError> {
        Ok(Self {
            recovery: Arc::new(RuntimeRecoveryGraph::new(
                environments.clone(),
                recovery_root,
            )?),
            locks: Arc::new(RuntimeLockCommitter::new(environments)),
        })
    }

    pub fn executor(
        &self,
        environments: Arc<WslRuntime>,
        revisions: Arc<dyn RuntimeRevisionSource>,
    ) -> RuntimePlanExecutor {
        RuntimePlanExecutor::with_recovery_graph(
            environments,
            Arc::clone(&self.recovery),
            Arc::clone(&self.locks),
            revisions,
        )
    }

    pub fn recovery_service(&self) -> RuntimeRecoveryService {
        self.recovery.service()
    }

    pub fn recovery_graph(&self) -> Arc<RuntimeRecoveryGraph> {
        Arc::clone(&self.recovery)
    }
}

impl RuntimePlanExecutor {
    #[cfg(test)]
    pub fn new(
        environments: Arc<WslRuntime>,
        native_recovery: Arc<dyn RecoveryMarkerStore>,
        locks: Arc<dyn PreparedLockCommitter>,
        revisions: Arc<dyn RuntimeRevisionSource>,
    ) -> Self {
        Self {
            environments,
            native_recovery,
            locks,
            revisions,
            recovery_graph: None,
        }
    }

    pub fn with_recovery_graph(
        environments: Arc<WslRuntime>,
        recovery_graph: Arc<RuntimeRecoveryGraph>,
        locks: Arc<dyn PreparedLockCommitter>,
        revisions: Arc<dyn RuntimeRevisionSource>,
    ) -> Self {
        Self {
            environments,
            native_recovery: recovery_graph.native_store(),
            locks,
            revisions,
            recovery_graph: Some(recovery_graph),
        }
    }

    async fn run(
        &self,
        plan: MutationPlan,
        cancellation: CancellationSignal,
        observer: MutationUnitObserver<'_>,
    ) -> Vec<MutationUnitResult> {
        let environment = match single_plan_environment(&plan) {
            Ok(environment) => environment,
            Err(error) => return failed_plan(&plan, error),
        };
        match environment {
            EnvironmentRef::Host => {
                let backend = native_backend();
                if let Err(error) = validate_entry_backends(&plan, &backend) {
                    return failed_plan(&plan, error);
                }
                let entries = NativePreparedEntryExecutor::new(
                    backend,
                    plan.operation_id.clone(),
                    Arc::clone(&self.native_recovery),
                );
                MutationCoordinator::new(
                    entries,
                    SharedLocks(Arc::clone(&self.locks)),
                    SharedRevisions(Arc::clone(&self.revisions)),
                )
                .execute_with_observer(plan, cancellation, observer)
                .await
            }
            EnvironmentRef::Wsl { distro_name } => {
                let workspace = match self.environments.workspace(&distro_name) {
                    Ok(workspace) => workspace,
                    Err(error) => return failed_plan(&plan, error),
                };
                let backend = ExecutionBackend::WslPosix {
                    distro_name: normalized_wsl_distro_name(&distro_name),
                };
                if let Err(error) = validate_entry_backends(&plan, &backend) {
                    return failed_plan(&plan, error);
                }
                let failure_units = failure_units(&plan);
                let plan = Arc::new(Mutex::new(Some(plan)));
                let locks = Arc::clone(&self.locks);
                let revisions = Arc::clone(&self.revisions);
                let recovery_graph = self.recovery_graph.clone();
                let cancellation_for_run = cancellation.clone();
                let observer_for_run = Arc::clone(&observer);
                match self
                    .environments
                    .with_session(&distro_name, move |session| {
                        let workspace = workspace.clone();
                        let plan = plan.lock().expect("WSL plan handoff lock poisoned").take();
                        let locks = Arc::clone(&locks);
                        let revisions = Arc::clone(&revisions);
                        let recovery_graph = recovery_graph.clone();
                        let cancellation = cancellation_for_run.clone();
                        let observer = Arc::clone(&observer_for_run);
                        async move {
                            let plan = plan.ok_or_else(|| AppError::ExecutionFailed {
                                message: "WSL mutation plan was consumed more than once"
                                    .to_string(),
                            })?;
                            let entries = match &recovery_graph {
                                Some(graph) => {
                                    let store = graph.active_wsl_store(session.clone())?;
                                    WslPreparedEntryExecutor::with_recovery_store(
                                        session,
                                        plan.operation_id.clone(),
                                        store,
                                    )
                                }
                                None => WslPreparedEntryExecutor::new(
                                    session,
                                    plan.operation_id.clone(),
                                ),
                            };
                            let results = MutationCoordinator::new(
                                entries,
                                SharedLocks(locks),
                                SharedRevisions(revisions),
                            )
                            .execute_with_observer(plan, cancellation, observer)
                            .await;
                            if let Some(graph) = recovery_graph {
                                graph.wsl_store(workspace)?;
                            }
                            Ok(results)
                        }
                    })
                    .await
                {
                    Ok(results) => results,
                    Err(error) => failed_units(&failure_units, error),
                }
            }
        }
    }
}

impl InstallPlanExecutor for RuntimePlanExecutor {
    fn execute<'a>(
        &'a self,
        plan: MutationPlan,
        cancellation: CancellationSignal,
    ) -> InstallFuture<'a, Vec<MutationUnitResult>> {
        Box::pin(async move { self.run(plan, cancellation, Arc::new(|_| {})).await })
    }

    fn execute_with_observer<'a>(
        &'a self,
        plan: MutationPlan,
        cancellation: CancellationSignal,
        observer: MutationUnitObserver<'a>,
    ) -> InstallFuture<'a, Vec<MutationUnitResult>> {
        Box::pin(async move { self.run(plan, cancellation, observer).await })
    }
}

struct SharedLocks(Arc<dyn PreparedLockCommitter>);

impl PreparedLockCommitter for SharedLocks {
    fn commit<'a>(
        &'a self,
        mutation: &'a PreparedLockMutation,
    ) -> BoxFuture<'a, Result<LockCommitReceipt, AppError>> {
        self.0.commit(mutation)
    }
}

struct SharedRevisions(Arc<dyn RuntimeRevisionSource>);

impl RuntimeRevisionSource for SharedRevisions {
    fn current<'a>(
        &'a self,
        context: &'a ContextRef,
    ) -> BoxFuture<'a, Result<RuntimeRevisions, AppError>> {
        self.0.current(context)
    }

    fn snapshot<'a>(
        &'a self,
        context: &'a ContextRef,
    ) -> BoxFuture<'a, Result<RuntimeRevisionSnapshot, AppError>> {
        self.0.snapshot(context)
    }
}

fn single_plan_environment(plan: &MutationPlan) -> Result<EnvironmentRef, AppError> {
    let Some(first) = plan.units.first() else {
        return Err(AppError::Validation {
            field: Some("units".to_string()),
            message: "mutation plan has no units".to_string(),
        });
    };
    if plan
        .units
        .iter()
        .any(|unit| !same_environment_identity(&first.target.environment, &unit.target.environment))
    {
        return Err(AppError::StaleEnvironment);
    }
    Ok(first.target.environment.clone())
}

fn validate_entry_backends(
    plan: &MutationPlan,
    expected: &ExecutionBackend,
) -> Result<(), AppError> {
    if plan.units.iter().any(|unit| {
        unit.canonical_entry
            .iter()
            .chain(&unit.required_agent_entries)
            .any(|entry| !same_backend(&entry.key.backend, expected))
    }) {
        return Err(AppError::StaleEnvironment);
    }
    Ok(())
}

fn same_backend(left: &ExecutionBackend, right: &ExecutionBackend) -> bool {
    match (left, right) {
        (ExecutionBackend::NativeWindows, ExecutionBackend::NativeWindows)
        | (ExecutionBackend::NativeUnix, ExecutionBackend::NativeUnix) => true,
        (
            ExecutionBackend::WslPosix { distro_name: left },
            ExecutionBackend::WslPosix { distro_name: right },
        ) => EnvironmentKey::wsl(left) == EnvironmentKey::wsl(right),
        _ => false,
    }
}

fn native_backend() -> ExecutionBackend {
    if cfg!(windows) {
        ExecutionBackend::NativeWindows
    } else {
        ExecutionBackend::NativeUnix
    }
}

#[derive(Clone)]
struct FailureUnit {
    id: String,
    skill_name: String,
    source: Option<ContextRef>,
    target: ContextRef,
}

fn failure_units(plan: &MutationPlan) -> Vec<FailureUnit> {
    plan.units
        .iter()
        .map(|unit| FailureUnit {
            id: unit.id.clone(),
            skill_name: unit.skill_name.clone(),
            source: unit.source.clone(),
            target: unit.target.clone(),
        })
        .collect()
}

fn failed_plan(plan: &MutationPlan, error: AppError) -> Vec<MutationUnitResult> {
    failed_units(&failure_units(plan), error)
}

fn failed_units(units: &[FailureUnit], error: AppError) -> Vec<MutationUnitResult> {
    units
        .iter()
        .map(|unit| {
            let report = ErrorReport::from_app_error(error.clone(), Some(unit.target.clone()));
            MutationUnitResult {
                unit_id: unit.id.clone(),
                skill_name: unit.skill_name.clone(),
                source: unit.source.clone(),
                target: unit.target.clone(),
                status: MutationUnitStatus::Failed,
                retryable: report.retryable,
                lock_committed: false,
                actual_mode: None,
                fallback_reason: None,
                agent_targets: Vec::new(),
                warnings: Vec::new(),
                error: Some(report),
                recovery: None,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::fs;
    use std::sync::Arc;

    use tempfile::tempdir;

    use super::*;
    use crate::application::install::InstallPlanExecutor;
    use crate::application::mutation::coordinator::{
        BoxFuture, PreparedLockCommitter, RuntimeRevisionSource,
    };
    use crate::application::mutation::plan::{
        ExecutionUnit, ExpectedTargetEntry, MutationPlan, PreparedEntryAction,
        PreparedEntryMutation, RuntimeRevisions,
    };
    use crate::application::payload_session::{PayloadSessionLimits, PayloadSessionManager};
    use crate::core::lossless_lock::{LockSchema, LosslessLockDocument};
    use crate::core::mutation::CancellationSignal;
    use crate::core::skill_payload::build_skill_payload;
    use crate::environment::native::recovery::NativeRecoveryMarkerStore;
    use crate::environment::native::tree::project_target;
    use crate::environment::runtime::{ContextSnapshotRevision, ExecutionBackend};
    use crate::environment::types::{ContextRef, ContextScope, EnvironmentRef, ResourceLocator};
    use crate::environment::wsl::WslRuntime;
    use crate::error::AppError;
    use crate::models::InstallMode;
    use crate::storage::lock_plan::{LockCommitReceipt, LockExpectedState, PreparedLockMutation};

    struct NoLocks;

    impl PreparedLockCommitter for NoLocks {
        fn commit<'a>(
            &'a self,
            _mutation: &'a PreparedLockMutation,
        ) -> BoxFuture<'a, Result<LockCommitReceipt, AppError>> {
            Box::pin(async { panic!("test plan has no lock mutation") })
        }
    }

    struct Revisions(RuntimeRevisions);

    impl RuntimeRevisionSource for Revisions {
        fn current<'a>(
            &'a self,
            _context: &'a ContextRef,
        ) -> BoxFuture<'a, Result<RuntimeRevisions, AppError>> {
            Box::pin(async move { Ok(self.0.clone()) })
        }
    }

    #[tokio::test]
    async fn native_runner_executes_a_generic_plan_through_the_real_coordinator() {
        let temp = tempdir().unwrap();
        let source = temp.path().join("source");
        fs::create_dir(&source).unwrap();
        fs::write(source.join("SKILL.md"), b"new").unwrap();
        let payload = build_skill_payload(&source).unwrap();
        let payload_id = payload.payload_id.clone();
        let manager = PayloadSessionManager::in_memory(
            PayloadSessionLimits {
                ttl_ms: 60_000,
                max_sessions: 4,
                max_bytes: 1_000_000,
            },
            || 1_000,
        );
        let discovery = manager
            .discover(EnvironmentRef::Host, "source")
            .await
            .unwrap();
        let handle = manager
            .acquire_payload(&discovery, "demo", payload)
            .await
            .unwrap();
        let lease = manager.pin_verified(&handle).await.unwrap();
        let destination = temp.path().join("target/demo");
        let backend = if cfg!(windows) {
            ExecutionBackend::NativeWindows
        } else {
            ExecutionBackend::NativeUnix
        };
        let projected = project_target(&destination, backend).unwrap();
        let context = ContextRef {
            environment: EnvironmentRef::Host,
            scope: ContextScope::Global,
        };
        let revisions = RuntimeRevisions {
            registry: "registry-1".to_string(),
            environment: "environment-1".to_string(),
            context: ContextSnapshotRevision::parse("context-v1-runner").unwrap(),
        };
        let entry = PreparedEntryMutation {
            key: projected.key.clone(),
            destination: ResourceLocator {
                environment: EnvironmentRef::Host,
                native_path: projected
                    .physical_destination
                    .to_string_lossy()
                    .into_owned(),
            },
            action: PreparedEntryAction::Replace {
                payload_id: payload_id.clone(),
                requested_mode: InstallMode::Copy,
            },
            owner_agent_ids: Vec::new(),
        };
        let plan = MutationPlan {
            operation_id: "operation-native-runner".to_string(),
            payloads: BTreeMap::from([(payload_id, lease)]),
            units: vec![ExecutionUnit {
                id: "install:demo".to_string(),
                skill_name: "demo".to_string(),
                source: None,
                target: context,
                expected_revisions: revisions.clone(),
                canonical_entry: Some(entry),
                required_agent_entries: Vec::new(),
                lock_mutation: None,
                expected_targets: vec![ExpectedTargetEntry {
                    key: projected.key,
                    fingerprint: projected.fingerprint,
                    expected_content_manifest_hash: None,
                }],
            }],
        };
        let recovery =
            Arc::new(NativeRecoveryMarkerStore::new(temp.path().join("recovery")).unwrap());
        let runner = RuntimePlanExecutor::new(
            Arc::new(WslRuntime::default()),
            recovery,
            Arc::new(NoLocks),
            Arc::new(Revisions(revisions)),
        );

        let results = runner.execute(plan, CancellationSignal::default()).await;

        assert_eq!(results.len(), 1);
        assert_eq!(
            results[0].status,
            crate::application::mutation::result::MutationUnitStatus::Succeeded
        );
        assert_eq!(fs::read(destination.join("SKILL.md")).unwrap(), b"new");
    }

    #[tokio::test]
    async fn runtime_lock_committer_routes_host_documents_through_atomic_lock_io() {
        let temp = tempdir().unwrap();
        let target = ResourceLocator {
            environment: EnvironmentRef::Host,
            native_path: temp
                .path()
                .join("skills-lock.json")
                .to_string_lossy()
                .into_owned(),
        };
        let document = LosslessLockDocument::empty(LockSchema::Project);
        let mutation = PreparedLockMutation {
            target: target.clone(),
            legacy_target: None,
            schema: LockSchema::Project,
            skill_name: "demo".to_string(),
            replacement: Some(serde_json::json!({
                "source": "owner/repo",
                "computedHash": "computed"
            })),
            root_replacements: BTreeMap::new(),
            expected: LockExpectedState::capture(&document, ["demo"], std::iter::empty::<&str>()),
        };
        let committer = RuntimeLockCommitter::new(Arc::new(WslRuntime::default()));

        committer.commit(&mutation).await.unwrap();

        let written: serde_json::Value =
            serde_json::from_slice(&fs::read(target.native_path).unwrap()).unwrap();
        assert_eq!(written["skills"]["demo"]["computedHash"], "computed");
    }
}

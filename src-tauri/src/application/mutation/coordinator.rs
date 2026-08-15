use std::collections::{BTreeMap, BTreeSet};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use crate::application::mutation::plan::{ExecutionUnit, MutationPlan, RuntimeRevisions};
use crate::application::mutation::result::{
    ErrorReport, MutationUnitResult, MutationUnitStatus, MutationWarning, MutationWarningCode,
};
use crate::application::payload_session::PinnedPayloadLease;
use crate::core::mutation::CancellationSignal;
use crate::core::skill_payload::PayloadId;
use crate::environment::types::SkillLocationRef;
use crate::error::AppError;
use crate::storage::lock_plan::{LockCommitReceipt, PreparedLockMutation};

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MutationUnitProgress {
    pub skill_name: String,
    pub current: u32,
    pub total: u32,
}

pub type MutationUnitObserver<'a> = Arc<dyn Fn(MutationUnitProgress) + Send + Sync + 'a>;

pub trait PreparedEntryExecutor: Send + Sync {
    type Staged: Send;

    fn stage<'a>(
        &'a self,
        unit: &'a ExecutionUnit,
        payloads: &'a BTreeMap<PayloadId, PinnedPayloadLease>,
        cancellation: CancellationSignal,
    ) -> BoxFuture<'a, Result<Self::Staged, AppError>>;

    fn recheck_entries<'a>(
        &'a self,
        staged: &'a Self::Staged,
    ) -> BoxFuture<'a, Result<(), AppError>>;

    fn swap<'a>(&'a self, staged: &'a mut Self::Staged) -> BoxFuture<'a, Result<(), AppError>>;

    fn verify<'a>(&'a self, staged: &'a Self::Staged) -> BoxFuture<'a, Result<(), AppError>>;

    fn restore<'a>(&'a self, staged: &'a mut Self::Staged) -> BoxFuture<'a, Result<(), AppError>>;

    fn cleanup<'a>(
        &'a self,
        staged: Self::Staged,
    ) -> BoxFuture<'a, Result<Vec<MutationWarning>, AppError>>;
}

pub trait PreparedLockCommitter: Send + Sync {
    fn commit<'a>(
        &'a self,
        mutation: &'a PreparedLockMutation,
    ) -> BoxFuture<'a, Result<LockCommitReceipt, AppError>>;
}

pub trait RuntimeRevisionSource: Send + Sync {
    fn current<'a>(
        &'a self,
        context: &'a SkillLocationRef,
    ) -> BoxFuture<'a, Result<RuntimeRevisions, AppError>>;

    fn snapshot<'a>(
        &'a self,
        context: &'a SkillLocationRef,
    ) -> BoxFuture<'a, Result<RuntimeRevisionSnapshot, AppError>> {
        Box::pin(async move {
            let revisions = self.current(context).await?;
            Ok(RuntimeRevisionSnapshot {
                authority: RuntimeAuthorityRevisions::from_runtime(&revisions),
                revisions,
            })
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeAuthorityRevisions {
    pub registry: String,
    pub environment: String,
    pub context: String,
}

impl RuntimeAuthorityRevisions {
    fn from_runtime(revisions: &RuntimeRevisions) -> Self {
        Self {
            registry: revisions.registry.clone(),
            environment: revisions.environment.clone(),
            context: revisions.context.as_str().to_string(),
        }
    }
}

pub struct RuntimeRevisionSnapshot {
    pub revisions: RuntimeRevisions,
    pub authority: RuntimeAuthorityRevisions,
}

pub struct MutationCoordinator<E, L, R> {
    entries: E,
    locks: L,
    revisions: R,
}

impl<E, L, R> MutationCoordinator<E, L, R>
where
    E: PreparedEntryExecutor,
    L: PreparedLockCommitter,
    R: RuntimeRevisionSource,
{
    pub fn new(entries: E, locks: L, revisions: R) -> Self {
        Self {
            entries,
            locks,
            revisions,
        }
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub async fn execute(
        &self,
        plan: MutationPlan,
        cancellation: CancellationSignal,
    ) -> Vec<MutationUnitResult> {
        self.execute_with_observer(plan, cancellation, Arc::new(|_| {}))
            .await
    }

    pub async fn execute_with_observer(
        &self,
        mut plan: MutationPlan,
        cancellation: CancellationSignal,
        observer: MutationUnitObserver<'_>,
    ) -> Vec<MutationUnitResult> {
        let total = u32::try_from(plan.units.len()).unwrap_or(u32::MAX);
        let mut staged_units = BTreeMap::new();
        for index in 0..plan.units.len() {
            let unit = plan.units[index].clone();
            let preflight = async {
                if cancellation.is_cancelled() {
                    return Err(AppError::MutationCancelled);
                }
                let runtime = self.revisions.snapshot(&unit.target).await?;
                validate_runtime_revisions(&runtime.revisions, &unit.expected_revisions)?;
                let staged = self
                    .entries
                    .stage(&unit, &plan.payloads, cancellation.clone())
                    .await?;
                Ok((staged, runtime.authority))
            }
            .await;
            staged_units.insert(index, preflight);
        }

        if cancellation.is_cancelled() {
            let mut results = Vec::with_capacity(plan.units.len());
            for (index, unit) in plan.units.iter().enumerate() {
                match staged_units
                    .remove(&index)
                    .expect("every unit has a preflight result")
                {
                    Ok((staged, _)) => {
                        let _ = self.entries.cleanup(staged).await;
                        results.push(failed_result(unit, AppError::MutationCancelled, false));
                    }
                    Err(error) => {
                        results.push(failed_result(unit, error_for_preflight(&error), false));
                    }
                }
            }
            return results;
        }

        let mut results = Vec::with_capacity(plan.units.len());
        let mut blocked_targets = BTreeSet::new();
        for index in 0..plan.units.len() {
            let unit = plan.units[index].clone();
            observer(MutationUnitProgress {
                skill_name: unit.skill_name.clone(),
                current: u32::try_from(index + 1).unwrap_or(u32::MAX),
                total,
            });
            let staged = staged_units
                .remove(&index)
                .expect("every unit has a preflight result");
            let (mut staged, expected_authority) = match staged {
                Ok(staged) => staged,
                Err(error) => {
                    results.push(failed_result(&unit, error_for_preflight(&error), false));
                    continue;
                }
            };
            if unit
                .expected_targets
                .iter()
                .any(|target| blocked_targets.contains(&target.key))
            {
                let _ = self.entries.cleanup(staged).await;
                results.push(not_run(&unit, AppError::StaleTarget));
                continue;
            }
            if cancellation.is_cancelled() {
                let _ = self.entries.cleanup(staged).await;
                results.push(failed_result(&unit, AppError::MutationCancelled, false));
                continue;
            }

            let phase_result = async {
                self.recheck_runtime_authority(&unit, &expected_authority)
                    .await?;
                self.entries.recheck_entries(&staged).await?;
                self.entries.swap(&mut staged).await?;
                self.entries.verify(&staged).await?;
                let receipt = match &unit.lock_mutation {
                    Some(mutation) => Some(self.locks.commit(mutation).await?),
                    None => None,
                };
                Ok::<_, AppError>(receipt)
            }
            .await;

            let receipt = match phase_result {
                Ok(receipt) => receipt,
                Err(primary) => {
                    let restore = self.entries.restore(&mut staged).await;
                    let _ = self.entries.cleanup(staged).await;
                    let error = restore.err().unwrap_or(primary);
                    if matches!(error, AppError::RecoveryRequired { .. }) {
                        blocked_targets.extend(
                            unit.expected_targets
                                .iter()
                                .map(|target| target.key.clone()),
                        );
                    }
                    results.push(failed_result(&unit, error, false));
                    continue;
                }
            };

            if let Some(receipt) = &receipt {
                advance_future_lock_expectations(&mut plan.units[index + 1..], &unit, receipt);
            }
            let lock_committed = unit.lock_mutation.is_some();
            match self.entries.cleanup(staged).await {
                Ok(warnings) => results.push(success_result(&unit, lock_committed, warnings)),
                Err(error) => {
                    results.push(success_result(
                        &unit,
                        lock_committed,
                        vec![MutationWarning {
                            code: MutationWarningCode::BackupCleanupFailed,
                            parameters: BTreeMap::new(),
                            technical_details: Some(error.to_string().chars().take(4096).collect()),
                        }],
                    ));
                }
            }
        }
        results
    }

    async fn recheck_runtime_authority(
        &self,
        unit: &ExecutionUnit,
        expected: &RuntimeAuthorityRevisions,
    ) -> Result<(), AppError> {
        let actual = self.revisions.snapshot(&unit.target).await?.authority;
        if &actual == expected {
            return Ok(());
        }
        Err(AppError::StaleAgentRuntime {
            expected_registry_revision: expected.registry.clone(),
            actual_registry_revision: actual.registry,
            expected_environment_revision: expected.environment.clone(),
            actual_environment_revision: actual.environment,
        })
    }
}

fn validate_runtime_revisions(
    actual: &RuntimeRevisions,
    expected: &RuntimeRevisions,
) -> Result<(), AppError> {
    if actual == expected {
        return Ok(());
    }
    Err(AppError::StaleAgentRuntime {
        expected_registry_revision: expected.registry.clone(),
        actual_registry_revision: actual.registry.clone(),
        expected_environment_revision: expected.environment.clone(),
        actual_environment_revision: actual.environment.clone(),
    })
}

fn error_for_preflight(error: &AppError) -> AppError {
    match error {
        AppError::MutationCancelled => AppError::MutationCancelled,
        AppError::StaleContext => AppError::StaleContext,
        AppError::StaleRegistry => AppError::StaleRegistry,
        AppError::StaleEnvironment => AppError::StaleEnvironment,
        AppError::StalePayload => AppError::StalePayload,
        AppError::StaleTarget => AppError::StaleTarget,
        AppError::SelfCopy => AppError::SelfCopy,
        _ => AppError::ExecutionFailed {
            message: error.to_string(),
        },
    }
}

fn advance_future_lock_expectations(
    future: &mut [ExecutionUnit],
    committed_unit: &ExecutionUnit,
    receipt: &LockCommitReceipt,
) {
    let Some(committed) = &committed_unit.lock_mutation else {
        return;
    };
    for unit in future {
        let Some(next) = unit.lock_mutation.as_mut() else {
            continue;
        };
        if next.target == committed.target {
            next.expected.advance(receipt);
        }
    }
}

fn success_result(
    unit: &ExecutionUnit,
    lock_committed: bool,
    warnings: Vec<MutationWarning>,
) -> MutationUnitResult {
    MutationUnitResult {
        unit_id: unit.id.clone(),
        skill_name: unit.skill_name.clone(),
        source: unit.source.clone(),
        target: unit.target.clone(),
        status: MutationUnitStatus::Succeeded,
        retryable: false,
        lock_committed,
        actual_mode: None,
        fallback_reason: None,
        agent_targets: Vec::new(),
        warnings,
        error: None,
        recovery: None,
    }
}

fn not_run(unit: &ExecutionUnit, error: AppError) -> MutationUnitResult {
    let mut result = failed_result(unit, error, false);
    result.status = MutationUnitStatus::NotRun;
    result.retryable = false;
    if let Some(error) = result.error.as_mut() {
        error.retryable = false;
    }
    result
}

fn failed_result(
    unit: &ExecutionUnit,
    error: AppError,
    lock_committed: bool,
) -> MutationUnitResult {
    let status = match &error {
        AppError::RecoveryRequired { .. } => MutationUnitStatus::RecoveryRequired,
        AppError::MutationCancelled => MutationUnitStatus::Cancelled,
        _ => MutationUnitStatus::Failed,
    };
    let report = ErrorReport::from_app_error(error, Some(unit.target.clone()));
    if status == MutationUnitStatus::RecoveryRequired {
        let mut result = MutationUnitResult::recovery_required(
            unit.id.clone(),
            unit.skill_name.clone(),
            unit.target.clone(),
            report,
        );
        result.source = unit.source.clone();
        return result;
    }
    MutationUnitResult {
        unit_id: unit.id.clone(),
        skill_name: unit.skill_name.clone(),
        source: unit.source.clone(),
        target: unit.target.clone(),
        status,
        retryable: report.retryable,
        lock_committed,
        actual_mode: None,
        fallback_reason: None,
        agent_targets: Vec::new(),
        warnings: Vec::new(),
        error: Some(report),
        recovery: None,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    use super::*;
    use crate::application::mutation::plan::{ExpectedTargetEntry, RuntimeRevisions};
    use crate::core::lossless_lock::{LockSchema, LosslessLockDocument};
    use crate::environment::runtime::{
        ContextSnapshotRevision, EntryFingerprint, ExecutionBackend, PhysicalParentIdentity,
        PhysicalTargetKey,
    };
    use crate::environment::types::{EnvironmentRef, ResourceLocator, SkillLocation};
    use crate::error::RecoveryResourceId;
    use crate::storage::lock_plan::{LockExpectedState, PreparedLockMutation};

    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Failure {
        None,
        StageSecond,
        CancelAfterSecond,
        Lock,
        Cleanup,
        LockAndRestoreRequired,
    }

    struct FakeEntryExecutor {
        log: Arc<Mutex<Vec<String>>>,
        failure: Failure,
    }

    struct FakeStaged {
        unit_id: String,
    }

    impl PreparedEntryExecutor for FakeEntryExecutor {
        type Staged = FakeStaged;

        fn stage<'a>(
            &'a self,
            unit: &'a ExecutionUnit,
            _payloads: &'a BTreeMap<PayloadId, PinnedPayloadLease>,
            cancellation: CancellationSignal,
        ) -> BoxFuture<'a, Result<Self::Staged, AppError>> {
            Box::pin(async move {
                if self.failure == Failure::StageSecond && unit.id == "second" {
                    return Err(AppError::ExecutionFailed {
                        message: "second target cannot be staged".to_string(),
                    });
                }
                self.log.lock().unwrap().push(format!("stage:{}", unit.id));
                self.log.lock().unwrap().push(format!("marker:{}", unit.id));
                if self.failure == Failure::CancelAfterSecond && unit.id == "second" {
                    cancellation.cancel();
                }
                Ok(FakeStaged {
                    unit_id: unit.id.clone(),
                })
            })
        }

        fn recheck_entries<'a>(
            &'a self,
            staged: &'a Self::Staged,
        ) -> BoxFuture<'a, Result<(), AppError>> {
            Box::pin(async move {
                self.log
                    .lock()
                    .unwrap()
                    .push(format!("recheck:{}", staged.unit_id));
                Ok(())
            })
        }

        fn swap<'a>(&'a self, staged: &'a mut Self::Staged) -> BoxFuture<'a, Result<(), AppError>> {
            Box::pin(async move {
                self.log
                    .lock()
                    .unwrap()
                    .push(format!("swap:{}", staged.unit_id));
                Ok(())
            })
        }

        fn verify<'a>(&'a self, staged: &'a Self::Staged) -> BoxFuture<'a, Result<(), AppError>> {
            Box::pin(async move {
                self.log
                    .lock()
                    .unwrap()
                    .push(format!("verify:{}", staged.unit_id));
                Ok(())
            })
        }

        fn restore<'a>(
            &'a self,
            staged: &'a mut Self::Staged,
        ) -> BoxFuture<'a, Result<(), AppError>> {
            Box::pin(async move {
                self.log
                    .lock()
                    .unwrap()
                    .push(format!("restore:{}", staged.unit_id));
                if self.failure == Failure::LockAndRestoreRequired {
                    return Err(AppError::RecoveryRequired {
                        recovery_resource_id: RecoveryResourceId::parse(format!(
                            "recovery-{}",
                            staged.unit_id
                        ))
                        .unwrap(),
                        message: "restore failed".to_string(),
                    });
                }
                Ok(())
            })
        }

        fn cleanup<'a>(
            &'a self,
            staged: Self::Staged,
        ) -> BoxFuture<'a, Result<Vec<MutationWarning>, AppError>> {
            Box::pin(async move {
                self.log
                    .lock()
                    .unwrap()
                    .push(format!("cleanup:{}", staged.unit_id));
                if self.failure == Failure::Cleanup {
                    Err(AppError::ExecutionFailed {
                        message: "backup remains".to_string(),
                    })
                } else {
                    Ok(Vec::new())
                }
            })
        }
    }

    struct FakeLockCommitter {
        log: Arc<Mutex<Vec<String>>>,
        failure: Failure,
    }

    struct RecordingLockCommitter {
        expected_entries: Arc<Mutex<Vec<Vec<String>>>>,
    }

    impl PreparedLockCommitter for FakeLockCommitter {
        fn commit<'a>(
            &'a self,
            mutation: &'a PreparedLockMutation,
        ) -> BoxFuture<'a, Result<LockCommitReceipt, AppError>> {
            Box::pin(async move {
                self.log
                    .lock()
                    .unwrap()
                    .push(format!("lock:{}", mutation.skill_name()));
                if self.failure == Failure::Lock
                    || (self.failure == Failure::LockAndRestoreRequired
                        && mutation.skill_name() == "first")
                {
                    Err(AppError::ExecutionFailed {
                        message: "lock failed".to_string(),
                    })
                } else {
                    Ok(LockCommitReceipt {
                        entry_snapshots: mutation.expected.entry_snapshots.clone(),
                        root_snapshots: mutation.expected.root_snapshots.clone(),
                    })
                }
            })
        }
    }

    impl PreparedLockCommitter for RecordingLockCommitter {
        fn commit<'a>(
            &'a self,
            mutation: &'a PreparedLockMutation,
        ) -> BoxFuture<'a, Result<LockCommitReceipt, AppError>> {
            Box::pin(async move {
                self.expected_entries
                    .lock()
                    .unwrap()
                    .push(mutation.expected.entry_snapshots.keys().cloned().collect());
                Ok(LockCommitReceipt {
                    entry_snapshots: mutation.expected.entry_snapshots.clone(),
                    root_snapshots: mutation.expected.root_snapshots.clone(),
                })
            })
        }
    }

    struct FakeRevisions {
        log: Arc<Mutex<Vec<String>>>,
        revisions: RuntimeRevisions,
    }

    struct ChangingRootRevisions {
        calls: Arc<AtomicUsize>,
        change_authority: bool,
    }

    impl RuntimeRevisionSource for ChangingRootRevisions {
        fn current<'a>(
            &'a self,
            _context: &'a SkillLocationRef,
        ) -> BoxFuture<'a, Result<RuntimeRevisions, AppError>> {
            Box::pin(async { Ok(revisions()) })
        }

        fn snapshot<'a>(
            &'a self,
            _context: &'a SkillLocationRef,
        ) -> BoxFuture<'a, Result<RuntimeRevisionSnapshot, AppError>> {
            let call = self.calls.fetch_add(1, Ordering::SeqCst);
            let change_authority = self.change_authority;
            Box::pin(async move {
                let mut current = revisions();
                if call > 0 {
                    current.context =
                        ContextSnapshotRevision::parse("context-v1-created-root").unwrap();
                }
                Ok(RuntimeRevisionSnapshot {
                    revisions: current,
                    authority: RuntimeAuthorityRevisions {
                        registry: "registry-1".to_string(),
                        environment: "environment-1".to_string(),
                        context: if call > 0 && change_authority {
                            "binding-2".to_string()
                        } else {
                            "binding-1".to_string()
                        },
                    },
                })
            })
        }
    }

    impl RuntimeRevisionSource for FakeRevisions {
        fn current<'a>(
            &'a self,
            context: &'a SkillLocationRef,
        ) -> BoxFuture<'a, Result<RuntimeRevisions, AppError>> {
            Box::pin(async move {
                self.log.lock().unwrap().push(format!(
                    "runtime:{}",
                    match context.scope {
                        SkillLocation::Global => "global",
                        SkillLocation::Project { .. } => "project",
                    }
                ));
                Ok(self.revisions.clone())
            })
        }
    }

    fn context() -> SkillLocationRef {
        SkillLocationRef {
            environment: EnvironmentRef::Native,
            scope: SkillLocation::Global,
        }
    }

    fn revisions() -> RuntimeRevisions {
        RuntimeRevisions {
            registry: "registry-1".to_string(),
            environment: "environment-1".to_string(),
            context: ContextSnapshotRevision::parse("context-v1-test").unwrap(),
        }
    }

    fn key(name: &str) -> PhysicalTargetKey {
        PhysicalTargetKey {
            backend: ExecutionBackend::NativeUnix,
            physical_parent: PhysicalParentIdentity::Unix {
                device: 1,
                inode: 2,
            },
            normalized_final_child_name: name.to_string(),
        }
    }

    fn lock_mutation(unit_id: &str) -> PreparedLockMutation {
        let document = LosslessLockDocument::empty(LockSchema::Project);
        PreparedLockMutation {
            target: ResourceLocator {
                environment: EnvironmentRef::Native,
                native_path: "/work/skills-lock.json".to_string(),
            },
            legacy_target: None,
            schema: LockSchema::Project,
            entry: crate::storage::lock_plan::LockEntryMutation::Replace {
                key: unit_id.to_string(),
                replacement: serde_json::json!({
                    "source": "test",
                    "computedHash": "hash"
                }),
            },
            root_replacements: BTreeMap::new(),
            expected: LockExpectedState::capture(&document, [unit_id], std::iter::empty::<&str>()),
        }
    }

    fn unit(id: &str, target: PhysicalTargetKey) -> ExecutionUnit {
        ExecutionUnit {
            id: id.to_string(),
            skill_name: id.to_string(),
            source: None,
            target: context(),
            expected_revisions: revisions(),
            canonical_entry: None,
            required_agent_entries: Vec::new(),
            lock_mutation: Some(lock_mutation(id)),
            expected_targets: vec![ExpectedTargetEntry {
                key: target,
                fingerprint: EntryFingerprint("entry-v1".to_string()),
                expected_content_manifest_hash: None,
            }],
        }
    }

    fn coordinator(
        failure: Failure,
        log: Arc<Mutex<Vec<String>>>,
    ) -> MutationCoordinator<FakeEntryExecutor, FakeLockCommitter, FakeRevisions> {
        MutationCoordinator::new(
            FakeEntryExecutor {
                log: log.clone(),
                failure,
            },
            FakeLockCommitter {
                log: log.clone(),
                failure,
            },
            FakeRevisions {
                log,
                revisions: revisions(),
            },
        )
    }

    fn plan(units: Vec<ExecutionUnit>) -> MutationPlan {
        MutationPlan {
            kind: crate::core::mutation::MutationKind::Install,
            operation_id: "operation-1".to_string(),
            payloads: BTreeMap::new(),
            units,
        }
    }

    #[tokio::test]
    async fn successful_unit_uses_the_exact_transaction_order() {
        let log = Arc::new(Mutex::new(Vec::new()));
        let results = coordinator(Failure::None, log.clone())
            .execute(
                plan(vec![unit("one", key("one"))]),
                CancellationSignal::default(),
            )
            .await;
        assert_eq!(results[0].status, MutationUnitStatus::Succeeded);
        assert_eq!(results[0].skill_name, "one");
        assert_eq!(
            *log.lock().unwrap(),
            vec![
                "runtime:global",
                "stage:one",
                "marker:one",
                "runtime:global",
                "recheck:one",
                "swap:one",
                "verify:one",
                "lock:one",
                "cleanup:one",
            ]
        );
    }

    #[tokio::test]
    async fn observer_reports_each_unit_before_its_destructive_phase() {
        let log = Arc::new(Mutex::new(Vec::new()));
        let progress = Arc::new(Mutex::new(Vec::new()));
        let observed = Arc::clone(&progress);
        coordinator(Failure::None, log)
            .execute_with_observer(
                plan(vec![unit("one", key("one")), unit("two", key("two"))]),
                CancellationSignal::default(),
                Arc::new(move |item| observed.lock().unwrap().push(item)),
            )
            .await;

        assert_eq!(
            *progress.lock().unwrap(),
            vec![
                MutationUnitProgress {
                    skill_name: "one".to_string(),
                    current: 1,
                    total: 2,
                },
                MutationUnitProgress {
                    skill_name: "two".to_string(),
                    current: 2,
                    total: 2,
                },
            ]
        );
    }

    #[tokio::test]
    async fn lock_failure_restores_before_cleanup() {
        let log = Arc::new(Mutex::new(Vec::new()));
        let results = coordinator(Failure::Lock, log.clone())
            .execute(
                plan(vec![unit("one", key("one"))]),
                CancellationSignal::default(),
            )
            .await;
        assert_eq!(results[0].status, MutationUnitStatus::Failed);
        let log = log.lock().unwrap();
        assert!(log
            .windows(3)
            .any(|window| { window == ["lock:one", "restore:one", "cleanup:one"] }));
    }

    #[tokio::test]
    async fn cleanup_failure_after_lock_is_success_with_typed_warning() {
        let log = Arc::new(Mutex::new(Vec::new()));
        let results = coordinator(Failure::Cleanup, log)
            .execute(
                plan(vec![unit("one", key("one"))]),
                CancellationSignal::default(),
            )
            .await;
        assert_eq!(results[0].status, MutationUnitStatus::Succeeded);
        assert!(results[0].lock_committed);
        assert_eq!(
            results[0].warnings[0].code,
            MutationWarningCode::BackupCleanupFailed
        );
    }

    #[tokio::test]
    async fn recovery_required_blocks_only_later_overlapping_units() {
        let log = Arc::new(Mutex::new(Vec::new()));
        let shared = key("shared");
        let results = coordinator(Failure::LockAndRestoreRequired, log)
            .execute(
                plan(vec![
                    unit("first", shared.clone()),
                    unit("overlap", shared),
                    unit("independent", key("independent")),
                ]),
                CancellationSignal::default(),
            )
            .await;
        assert_eq!(results[0].status, MutationUnitStatus::RecoveryRequired);
        assert_eq!(results[1].status, MutationUnitStatus::NotRun);
        assert_eq!(results[2].status, MutationUnitStatus::Succeeded);
    }

    #[tokio::test]
    async fn one_staging_failure_does_not_abort_independent_units() {
        let log = Arc::new(Mutex::new(Vec::new()));
        let results = coordinator(Failure::StageSecond, log.clone())
            .execute(
                plan(vec![
                    unit("first", key("first")),
                    unit("second", key("second")),
                    unit("third", key("third")),
                ]),
                CancellationSignal::default(),
            )
            .await;

        assert_eq!(results[0].status, MutationUnitStatus::Succeeded);
        assert_eq!(results[1].status, MutationUnitStatus::Failed);
        assert_eq!(results[2].status, MutationUnitStatus::Succeeded);
        let log = log.lock().unwrap();
        let stage_third = log.iter().position(|entry| entry == "stage:third").unwrap();
        let swap_first = log.iter().position(|entry| entry == "swap:first").unwrap();
        assert!(stage_third < swap_first);
    }

    #[tokio::test]
    async fn cancellation_before_commit_cleans_every_staged_unit() {
        let log = Arc::new(Mutex::new(Vec::new()));
        let results = coordinator(Failure::CancelAfterSecond, log.clone())
            .execute(
                plan(vec![
                    unit("first", key("first")),
                    unit("second", key("second")),
                    unit("third", key("third")),
                ]),
                CancellationSignal::default(),
            )
            .await;

        assert!(results
            .iter()
            .all(|result| result.status != MutationUnitStatus::Succeeded));
        let log = log.lock().unwrap();
        assert!(log.iter().any(|entry| entry == "cleanup:first"));
        assert!(log.iter().any(|entry| entry == "cleanup:second"));
        assert!(!log.iter().any(|entry| entry.starts_with("swap:")));
    }

    #[tokio::test]
    async fn successful_lock_receipt_advances_later_unit_expectations() {
        let log = Arc::new(Mutex::new(Vec::new()));
        let expected_entries = Arc::new(Mutex::new(Vec::new()));
        let coordinator = MutationCoordinator::new(
            FakeEntryExecutor {
                log: log.clone(),
                failure: Failure::None,
            },
            RecordingLockCommitter {
                expected_entries: Arc::clone(&expected_entries),
            },
            FakeRevisions {
                log,
                revisions: revisions(),
            },
        );

        let results = coordinator
            .execute(
                plan(vec![
                    unit("first", key("first")),
                    unit("second", key("second")),
                ]),
                CancellationSignal::default(),
            )
            .await;

        assert!(results
            .iter()
            .all(|result| result.status == MutationUnitStatus::Succeeded));
        assert_eq!(
            *expected_entries.lock().unwrap(),
            vec![
                vec!["first".to_string()],
                vec!["first".to_string(), "second".to_string()],
            ]
        );
    }

    #[tokio::test]
    async fn app_created_root_revision_is_allowed_when_context_authority_is_unchanged() {
        let log = Arc::new(Mutex::new(Vec::new()));
        let calls = Arc::new(AtomicUsize::new(0));
        let coordinator = MutationCoordinator::new(
            FakeEntryExecutor {
                log: log.clone(),
                failure: Failure::None,
            },
            FakeLockCommitter {
                log,
                failure: Failure::None,
            },
            ChangingRootRevisions {
                calls: calls.clone(),
                change_authority: false,
            },
        );

        let results = coordinator
            .execute(
                plan(vec![unit("one", key("one"))]),
                CancellationSignal::default(),
            )
            .await;

        assert_eq!(results[0].status, MutationUnitStatus::Succeeded);
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn selected_binding_change_is_rejected_after_staging_before_swap() {
        let log = Arc::new(Mutex::new(Vec::new()));
        let calls = Arc::new(AtomicUsize::new(0));
        let coordinator = MutationCoordinator::new(
            FakeEntryExecutor {
                log: log.clone(),
                failure: Failure::None,
            },
            FakeLockCommitter {
                log: log.clone(),
                failure: Failure::None,
            },
            ChangingRootRevisions {
                calls,
                change_authority: true,
            },
        );

        let results = coordinator
            .execute(
                plan(vec![unit("one", key("one"))]),
                CancellationSignal::default(),
            )
            .await;

        assert_eq!(results[0].status, MutationUnitStatus::Failed);
        assert!(!log.lock().unwrap().iter().any(|entry| entry == "swap:one"));
    }
}

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use sha2::{Digest, Sha256};

#[cfg(test)]
use crate::application::mutation::coordinator::PreparedEntryTestDriver;
use crate::application::mutation::coordinator::{
    BoxFuture, PreparedLockCommitter, PreparedUnitExecutor, UnitTransactionReceipt,
};
use crate::application::mutation::plan::{
    ExecutionUnit, PreparedEntryAction, PreparedEntryMutation,
};
use crate::application::mutation::result::{MutationWarning, MutationWarningCode};
use crate::application::payload_session::{PayloadLocalSource, PinnedPayloadLease};
use crate::core::mutation::CancellationSignal;
use crate::core::skill_payload::{PayloadId, SkillPayload};
use crate::environment::native::entry::{
    cleanup_entry_set, planned_recovery_paths, preflight_entry_writes, recheck_entry_set,
    restore_entry_set, stage_entry_set, swap_entry_set, verify_entry_set, NativeEntryAction,
    NativeEntryIntent, NativeEntrySet,
};
use crate::environment::recovery::{
    RecoveryEntryPhase, RecoveryMarker, RecoveryMarkerEntry, RecoveryMarkerKind, RecoveryMarkerRef,
    RecoveryMarkerStore, RecoverySubject, RECOVERY_MARKER_SCHEMA_VERSION,
};
use crate::environment::runtime::ExecutionBackend;
use crate::environment::types::{EnvironmentRef, ResourceLocator};
use crate::error::{AppError, RecoveryResourceId};
use crate::models::InstallMode;
use crate::storage::atomic_document::{DocumentWriteFailure, PublicationState};
use crate::storage::lock_plan::PreparedLockMutation;

pub struct NativePreparedEntrySet {
    entries: NativeEntrySet,
    recovery: Option<NativePreparedRecovery>,
    // In-memory safety latch also protects evidence when persisting the
    // RecoveryRequired marker itself fails.
    retain_recovery: bool,
}

struct NativePreparedRecovery {
    recovery_store: Arc<dyn RecoveryMarkerStore>,
    recovery_marker: Mutex<RecoveryMarker>,
    recovery_ref: RecoveryMarkerRef,
}

pub struct NativePreparedEntryExecutor {
    backend: ExecutionBackend,
    operation_id: String,
    operation_kind: crate::core::mutation::MutationKind,
    recovery_store: Arc<dyn RecoveryMarkerStore>,
}

pub struct NativePreparedUnitExecutor<L> {
    entries: NativePreparedEntryExecutor,
    locks: L,
}

pub struct PreparedNativeUnit {
    unit: ExecutionUnit,
    intents: Vec<NativeEntryIntent>,
}

impl<L> NativePreparedUnitExecutor<L> {
    pub fn new(entries: NativePreparedEntryExecutor, locks: L) -> Self {
        Self { entries, locks }
    }
}

impl<L> PreparedUnitExecutor for NativePreparedUnitExecutor<L>
where
    L: PreparedLockCommitter,
{
    type Prepared = PreparedNativeUnit;

    fn prepare<'a>(
        &'a self,
        unit: &'a ExecutionUnit,
        payloads: &'a BTreeMap<PayloadId, PinnedPayloadLease>,
        cancellation: CancellationSignal,
    ) -> BoxFuture<'a, Result<Self::Prepared, AppError>> {
        Box::pin(async move {
            let mut loaded = BTreeMap::new();
            for entry in unit
                .primary_entry
                .iter()
                .chain(unit.additional_entries.iter())
            {
                let PreparedEntryAction::Replace {
                    payload_id,
                    requested_mode: InstallMode::Copy,
                } = &entry.action
                else {
                    continue;
                };
                if loaded.contains_key(payload_id) {
                    continue;
                }
                if cancellation.is_cancelled() {
                    return Err(AppError::MutationCancelled);
                }
                let lease = payloads.get(payload_id).ok_or(AppError::StalePayload)?;
                match lease.local_source()? {
                    PayloadLocalSource::InProcess | PayloadLocalSource::NativeManaged { .. } => {}
                    PayloadLocalSource::WslManaged { .. } => {
                        return Err(AppError::CapabilityUnavailable {
                            capability: "backendLocalPayload".to_string(),
                            path: None,
                        });
                    }
                }
                loaded.insert(payload_id.clone(), Arc::new(lease.load_payload().await?));
            }
            if cancellation.is_cancelled() {
                return Err(AppError::MutationCancelled);
            }
            let intents = prepare_native_mutations(unit, &loaded, self.entries.backend.clone())?;
            preflight_entry_writes(&intents)?;
            Ok(PreparedNativeUnit {
                unit: unit.clone(),
                intents,
            })
        })
    }

    fn execute<'a>(
        &'a self,
        prepared: Self::Prepared,
        lock: Option<&'a PreparedLockMutation>,
        cancellation: CancellationSignal,
    ) -> BoxFuture<'a, Result<UnitTransactionReceipt, AppError>> {
        Box::pin(async move {
            if cancellation.is_cancelled() {
                return Err(AppError::MutationCancelled);
            }
            // Staging creates directories and temporary entries. Keep it in the
            // transaction future so dropping the caller cannot detach writes
            // after RuntimeAdmission has released its permit.
            let entries = stage_entry_set(&prepared.intents)?;
            let recovery = if planned_recovery_paths(&entries).is_empty() {
                None
            } else {
                let marker = native_recovery_marker(
                    &self.entries.operation_id,
                    &prepared.unit.id,
                    RecoverySubject {
                        operation_kind: self.entries.operation_kind,
                        skill_name: prepared.unit.skill_name.clone(),
                        context: prepared.unit.target.clone(),
                    },
                    &entries,
                    now_epoch_ms(),
                )?;
                let recovery_ref = match self.entries.recovery_store.create(&marker).await {
                    Ok(marker_ref) => marker_ref,
                    Err(error) => {
                        let cleanup = cleanup_entry_set(entries)?;
                        if cleanup.is_empty() {
                            return Err(error);
                        }
                        return Err(AppError::ExecutionFailed {
                            message: format!(
                                "{error}; native staging cleanup failed: {}",
                                cleanup.join("; ")
                            ),
                        });
                    }
                };
                Some(NativePreparedRecovery {
                    recovery_store: Arc::clone(&self.entries.recovery_store),
                    recovery_marker: Mutex::new(marker),
                    recovery_ref,
                })
            };
            let mut staged = NativePreparedEntrySet {
                entries,
                recovery,
                retain_recovery: false,
            };
            enum TransactionFailure {
                Entry(AppError),
                Lock(DocumentWriteFailure),
            }

            let transaction = async {
                if cancellation.is_cancelled() {
                    return Err(TransactionFailure::Entry(AppError::MutationCancelled));
                }
                self.entries
                    .recheck_entries(&staged)
                    .await
                    .map_err(TransactionFailure::Entry)?;
                self.entries
                    .swap(&mut staged)
                    .await
                    .map_err(TransactionFailure::Entry)?;
                self.entries
                    .verify(&staged)
                    .await
                    .map_err(TransactionFailure::Entry)?;
                match lock {
                    Some(lock) => self
                        .locks
                        .commit(lock)
                        .await
                        .map(Some)
                        .map_err(TransactionFailure::Lock),
                    None => Ok(None),
                }
            }
            .await;
            match transaction {
                Ok(lock) => match self.entries.cleanup(staged).await {
                    Ok(warnings) => Ok(UnitTransactionReceipt { lock, warnings }),
                    Err(error) => Ok(UnitTransactionReceipt {
                        lock,
                        warnings: vec![MutationWarning {
                            code: MutationWarningCode::BackupCleanupFailed,
                            parameters: BTreeMap::new(),
                            technical_details: Some(error.to_string().chars().take(4096).collect()),
                        }],
                    }),
                },
                Err(TransactionFailure::Lock(
                    failure @ DocumentWriteFailure {
                        publication:
                            PublicationState::PublishedUnconfirmed | PublicationState::OutcomeUnknown,
                        ..
                    },
                )) => staged
                    .recovery_required(format!(
                        "lock publication is not confirmed: {}",
                        failure.error
                    ))
                    .await
                    .map(|()| unreachable!("recovery_required always returns an error")),
                Err(TransactionFailure::Entry(primary @ AppError::RecoveryRequired { .. })) => {
                    Err(primary)
                }
                Err(TransactionFailure::Entry(primary))
                | Err(TransactionFailure::Lock(DocumentWriteFailure {
                    error: primary,
                    publication: PublicationState::NotPublished,
                    ..
                })) => {
                    // swap may already have attempted and failed to restore.
                    // Do not retry a destructive compensation after that
                    // boundary, or turn retained evidence into cleanup work.
                    if staged.retain_recovery {
                        return Err(primary);
                    }
                    // A failed restore leaves the only recoverable copy in
                    // backup. Cleanup must not relabel or delete that evidence.
                    self.entries.restore(&mut staged).await?;
                    let _ = self.entries.cleanup(staged).await;
                    Err(primary)
                }
            }
        })
    }
}

impl NativePreparedEntryExecutor {
    #[cfg(test)]
    pub fn new(
        backend: ExecutionBackend,
        operation_id: impl Into<String>,
        recovery_store: Arc<dyn RecoveryMarkerStore>,
    ) -> Self {
        Self::for_operation(
            backend,
            operation_id,
            crate::core::mutation::MutationKind::Install,
            recovery_store,
        )
    }

    pub fn for_operation(
        backend: ExecutionBackend,
        operation_id: impl Into<String>,
        operation_kind: crate::core::mutation::MutationKind,
        recovery_store: Arc<dyn RecoveryMarkerStore>,
    ) -> Self {
        Self {
            backend,
            operation_id: operation_id.into(),
            operation_kind,
            recovery_store,
        }
    }
}

impl NativePreparedEntryExecutor {
    #[cfg(test)]
    fn stage<'a>(
        &'a self,
        unit: &'a ExecutionUnit,
        payloads: &'a BTreeMap<PayloadId, PinnedPayloadLease>,
        cancellation: CancellationSignal,
    ) -> BoxFuture<'a, Result<NativePreparedEntrySet, AppError>> {
        Box::pin(async move {
            let mut loaded = BTreeMap::new();
            for entry in unit
                .primary_entry
                .iter()
                .chain(unit.additional_entries.iter())
            {
                let PreparedEntryAction::Replace {
                    payload_id,
                    requested_mode: InstallMode::Copy,
                } = &entry.action
                else {
                    continue;
                };
                if loaded.contains_key(payload_id) {
                    continue;
                }
                if cancellation.is_cancelled() {
                    return Err(AppError::MutationCancelled);
                }
                let lease = payloads.get(payload_id).ok_or(AppError::StalePayload)?;
                match lease.local_source()? {
                    PayloadLocalSource::InProcess | PayloadLocalSource::NativeManaged { .. } => {}
                    PayloadLocalSource::WslManaged { .. } => {
                        return Err(AppError::CapabilityUnavailable {
                            capability: "backendLocalPayload".to_string(),
                            path: None,
                        })
                    }
                }
                loaded.insert(payload_id.clone(), Arc::new(lease.load_payload().await?));
            }
            if cancellation.is_cancelled() {
                return Err(AppError::MutationCancelled);
            }
            let intents = prepare_native_mutations(unit, &loaded, self.backend.clone())?;
            let entries = stage_entry_set(&intents)?;
            let recovery = if planned_recovery_paths(&entries).is_empty() {
                None
            } else {
                let marker = native_recovery_marker(
                    &self.operation_id,
                    &unit.id,
                    RecoverySubject {
                        operation_kind: self.operation_kind,
                        skill_name: unit.skill_name.clone(),
                        context: unit.target.clone(),
                    },
                    &entries,
                    now_epoch_ms(),
                )?;
                let recovery_ref = match self.recovery_store.create(&marker).await {
                    Ok(marker_ref) => marker_ref,
                    Err(error) => {
                        let cleanup = cleanup_entry_set(entries)?;
                        if cleanup.is_empty() {
                            return Err(error);
                        }
                        return Err(AppError::ExecutionFailed {
                            message: format!(
                                "{error}; native staging cleanup failed: {}",
                                cleanup.join("; ")
                            ),
                        });
                    }
                };
                Some(NativePreparedRecovery {
                    recovery_store: Arc::clone(&self.recovery_store),
                    recovery_marker: Mutex::new(marker),
                    recovery_ref,
                })
            };
            Ok(NativePreparedEntrySet {
                entries,
                recovery,
                retain_recovery: false,
            })
        })
    }

    fn recheck_entries<'a>(
        &'a self,
        staged: &'a NativePreparedEntrySet,
    ) -> BoxFuture<'a, Result<(), AppError>> {
        let entries = staged.entries.clone();
        Box::pin(async move {
            tokio::task::spawn_blocking(move || recheck_entry_set(&entries))
                .await
                .map_err(native_task_error)?
        })
    }

    fn swap<'a>(
        &'a self,
        staged: &'a mut NativePreparedEntrySet,
    ) -> BoxFuture<'a, Result<(), AppError>> {
        Box::pin(async move {
            match swap_entry_set(&mut staged.entries) {
                Ok(()) => {
                    staged
                        .update_recovery(
                            RecoveryMarkerKind::InProgress,
                            Some(RecoveryEntryPhase::Swapped),
                        )
                        .await
                }
                Err(AppError::RestoreFailed { message }) => staged.recovery_required(message).await,
                Err(error) => Err(error),
            }
        })
    }

    fn verify<'a>(
        &'a self,
        staged: &'a NativePreparedEntrySet,
    ) -> BoxFuture<'a, Result<(), AppError>> {
        let entries = staged.entries.clone();
        Box::pin(async move {
            tokio::task::spawn_blocking(move || verify_entry_set(&entries))
                .await
                .map_err(native_task_error)??;
            staged
                .update_recovery(
                    RecoveryMarkerKind::InProgress,
                    Some(RecoveryEntryPhase::Verified),
                )
                .await
        })
    }

    fn restore<'a>(
        &'a self,
        staged: &'a mut NativePreparedEntrySet,
    ) -> BoxFuture<'a, Result<(), AppError>> {
        Box::pin(async move {
            match restore_entry_set(&mut staged.entries) {
                Ok(()) => {
                    staged
                        .update_recovery(RecoveryMarkerKind::CleanupOnly, None)
                        .await?;
                    staged.retain_recovery = false;
                    Ok(())
                }
                Err(error) => staged.recovery_required(error.to_string()).await,
            }
        })
    }

    fn cleanup<'a>(
        &'a self,
        staged: NativePreparedEntrySet,
    ) -> BoxFuture<'a, Result<Vec<MutationWarning>, AppError>> {
        Box::pin(async move {
            if staged.retain_recovery {
                return Err(match &staged.recovery {
                    Some(recovery) => AppError::RecoveryRequired {
                        recovery_resource_id: recovery.recovery_ref.resource_id.clone(),
                        message:
                            "native recovery evidence cannot be cleaned before a confirmed restore"
                                .to_string(),
                    },
                    None => AppError::RestoreFailed {
                        message: "native restore is not confirmed".to_string(),
                    },
                });
            }
            staged
                .update_recovery(RecoveryMarkerKind::CleanupOnly, None)
                .await?;
            let warnings = cleanup_entry_set(staged.entries)?;
            if warnings.is_empty() {
                if let Some(recovery) = &staged.recovery {
                    recovery
                        .recovery_store
                        .remove(&recovery.recovery_ref)
                        .await?;
                }
                return Ok(Vec::new());
            }
            let mut result = warnings
                .into_iter()
                .map(|details| MutationWarning {
                    code: MutationWarningCode::BackupCleanupFailed,
                    parameters: BTreeMap::new(),
                    technical_details: Some(details),
                })
                .collect::<Vec<_>>();
            if staged.recovery.is_some() {
                result.push(MutationWarning {
                    code: MutationWarningCode::CleanupMarkerRetained,
                    parameters: BTreeMap::new(),
                    technical_details: None,
                });
            }
            Ok(result)
        })
    }
}

#[cfg(test)]
impl PreparedEntryTestDriver for NativePreparedEntryExecutor {
    type Staged = NativePreparedEntrySet;

    fn stage<'a>(
        &'a self,
        unit: &'a ExecutionUnit,
        payloads: &'a BTreeMap<PayloadId, PinnedPayloadLease>,
        cancellation: CancellationSignal,
    ) -> BoxFuture<'a, Result<Self::Staged, AppError>> {
        NativePreparedEntryExecutor::stage(self, unit, payloads, cancellation)
    }

    fn recheck_entries<'a>(
        &'a self,
        staged: &'a Self::Staged,
    ) -> BoxFuture<'a, Result<(), AppError>> {
        NativePreparedEntryExecutor::recheck_entries(self, staged)
    }

    fn swap<'a>(&'a self, staged: &'a mut Self::Staged) -> BoxFuture<'a, Result<(), AppError>> {
        NativePreparedEntryExecutor::swap(self, staged)
    }

    fn verify<'a>(&'a self, staged: &'a Self::Staged) -> BoxFuture<'a, Result<(), AppError>> {
        NativePreparedEntryExecutor::verify(self, staged)
    }

    fn restore<'a>(&'a self, staged: &'a mut Self::Staged) -> BoxFuture<'a, Result<(), AppError>> {
        NativePreparedEntryExecutor::restore(self, staged)
    }

    fn cleanup<'a>(
        &'a self,
        staged: Self::Staged,
    ) -> BoxFuture<'a, Result<Vec<MutationWarning>, AppError>> {
        NativePreparedEntryExecutor::cleanup(self, staged)
    }
}

fn native_task_error(error: tokio::task::JoinError) -> AppError {
    AppError::ExecutionFailed {
        message: format!("native mutation task failed: {error}"),
    }
}

impl NativePreparedEntrySet {
    async fn update_recovery(
        &self,
        kind: RecoveryMarkerKind,
        phase: Option<RecoveryEntryPhase>,
    ) -> Result<(), AppError> {
        let Some(recovery) = &self.recovery else {
            return Ok(());
        };
        let mut updated = recovery
            .recovery_marker
            .lock()
            .map_err(|_| AppError::Io {
                message: "native recovery marker state is unavailable".to_string(),
            })?
            .clone();
        updated.kind = kind;
        if let Some(phase) = phase {
            for entry in &mut updated.entries {
                entry.phase = phase;
            }
        }
        recovery
            .recovery_store
            .update(&recovery.recovery_ref, &updated)
            .await?;
        *recovery.recovery_marker.lock().map_err(|_| AppError::Io {
            message: "native recovery marker state is unavailable".to_string(),
        })? = updated;
        Ok(())
    }

    async fn recovery_required(&mut self, message: String) -> Result<(), AppError> {
        self.retain_recovery = true;
        let Some(recovery) = &self.recovery else {
            return Err(AppError::RestoreFailed { message });
        };
        match self
            .update_recovery(
                RecoveryMarkerKind::RecoveryRequired,
                Some(RecoveryEntryPhase::RestoreFailed),
            )
            .await
        {
            Ok(()) => Err(AppError::RecoveryRequired {
                recovery_resource_id: recovery.recovery_ref.resource_id.clone(),
                message,
            }),
            Err(error) => Err(AppError::RestoreFailed {
                message: format!("{message}; failed to persist recovery marker: {error}"),
            }),
        }
    }
}

fn native_recovery_marker(
    operation_id: &str,
    unit_id: &str,
    subject: RecoverySubject,
    entries: &NativeEntrySet,
    created_at_epoch_ms: u64,
) -> Result<RecoveryMarker, AppError> {
    let resource_id = operation_owner_id(operation_id, unit_id);
    let marker_entries = planned_recovery_paths(entries)
        .into_iter()
        .map(|entry| {
            Ok(RecoveryMarkerEntry {
                physical_target_digest: format!(
                    "target-v1-{:x}",
                    Sha256::digest(serde_json::to_vec(&entry.target)?)
                ),
                destination: ResourceLocator {
                    environment: EnvironmentRef::Native,
                    native_path: entry.destination.to_string_lossy().into_owned(),
                },
                backup: Some(ResourceLocator {
                    environment: EnvironmentRef::Native,
                    native_path: entry.backup.to_string_lossy().into_owned(),
                }),
                expected_state: entry.expected_state,
                original_fingerprint: entry.original_fingerprint.0,
                phase: RecoveryEntryPhase::Staged,
            })
        })
        .collect::<Result<Vec<_>, AppError>>()?;
    Ok(RecoveryMarker {
        schema_version: RECOVERY_MARKER_SCHEMA_VERSION,
        resource_id,
        kind: RecoveryMarkerKind::InProgress,
        environment: EnvironmentRef::Native,
        operation_id: operation_id.to_string(),
        unit_id: unit_id.to_string(),
        subject: Some(subject),
        created_at_epoch_ms,
        entries: marker_entries,
    })
}

fn operation_owner_id(operation_id: &str, unit_id: &str) -> RecoveryResourceId {
    RecoveryResourceId::parse(format!(
        "{:x}",
        Sha256::digest(format!("skill-deck-operation-v1\0{operation_id}\0{unit_id}").as_bytes())
    ))
    .expect("SHA-256 recovery IDs are valid")
}

fn now_epoch_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

pub fn prepare_native_mutations(
    unit: &ExecutionUnit,
    payloads: &BTreeMap<PayloadId, Arc<SkillPayload>>,
    backend: ExecutionBackend,
) -> Result<Vec<NativeEntryIntent>, AppError> {
    if unit.target.environment != EnvironmentRef::Native {
        return Err(AppError::StaleEnvironment);
    }
    let canonical_path = unit
        .primary_entry
        .as_ref()
        .map(|entry| PathBuf::from(&entry.destination.native_path));
    let expected = unit
        .expected_targets
        .iter()
        .map(|entry| (&entry.key, entry))
        .collect::<BTreeMap<_, _>>();
    let all_remove = unit
        .primary_entry
        .iter()
        .chain(unit.additional_entries.iter())
        .all(|entry| entry.action == PreparedEntryAction::Remove);
    let entries = if all_remove {
        unit.additional_entries
            .iter()
            .chain(unit.primary_entry.iter())
            .collect::<Vec<_>>()
    } else {
        unit.primary_entry
            .iter()
            .chain(unit.additional_entries.iter())
            .collect::<Vec<_>>()
    };
    let mut intents = Vec::new();
    for entry in entries {
        validate_native_entry(entry, &backend)?;
        let expected = expected
            .get(&entry.key)
            .copied()
            .ok_or(AppError::StaleTarget)?;
        let action = match &entry.action {
            PreparedEntryAction::Keep => NativeEntryAction::Keep,
            PreparedEntryAction::Remove => NativeEntryAction::Remove,
            PreparedEntryAction::Replace {
                payload_id,
                requested_mode: InstallMode::Copy,
            } => {
                let payload = payloads.get(payload_id).ok_or(AppError::StalePayload)?;
                if &payload.payload_id != payload_id {
                    return Err(AppError::StalePayload);
                }
                NativeEntryAction::Materialize {
                    payload: Arc::clone(payload),
                }
            }
            PreparedEntryAction::Replace {
                requested_mode: InstallMode::Symlink,
                ..
            } => {
                let target = canonical_path.clone().ok_or_else(|| AppError::Validation {
                    field: Some("canonicalEntry".to_string()),
                    message: "Native symlink entry requires a canonical entry".to_string(),
                })?;
                if target == Path::new(&entry.destination.native_path) {
                    return Err(AppError::SelfCopy);
                }
                NativeEntryAction::Symlink { target }
            }
            PreparedEntryAction::Link { target } => {
                if target.environment != EnvironmentRef::Native
                    || !Path::new(&target.native_path).is_absolute()
                {
                    return Err(AppError::StaleEnvironment);
                }
                let target = PathBuf::from(&target.native_path);
                if target == Path::new(&entry.destination.native_path) {
                    return Err(AppError::SelfCopy);
                }
                NativeEntryAction::Symlink { target }
            }
        };
        intents.push(NativeEntryIntent {
            target: entry.key.clone(),
            destination: PathBuf::from(&entry.destination.native_path),
            expected_fingerprint: expected.fingerprint.clone(),
            expected_content_manifest_hash: expected.expected_content_manifest_hash.clone(),
            action,
        });
    }
    if intents.is_empty() {
        return Err(AppError::Validation {
            field: Some("entrySet".to_string()),
            message: "Native entry set must not be empty".to_string(),
        });
    }
    Ok(intents)
}

fn validate_native_entry(
    entry: &PreparedEntryMutation,
    backend: &ExecutionBackend,
) -> Result<(), AppError> {
    if &entry.key.backend != backend
        || entry.destination.environment != EnvironmentRef::Native
        || !Path::new(&entry.destination.native_path).is_absolute()
    {
        return Err(AppError::StaleTarget);
    }
    match backend {
        ExecutionBackend::NativeWindows | ExecutionBackend::NativeUnix => Ok(()),
        ExecutionBackend::WslPosix { .. } => Err(AppError::StaleEnvironment),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::fs;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    use tempfile::tempdir;

    use super::*;
    use crate::application::mutation::plan::{
        ExecutionUnit, ExpectedTargetEntry, PreparedEntryAction, PreparedEntryMutation,
        RuntimeRevisions,
    };
    use crate::application::payload_session::{PayloadSessionLimits, PayloadSessionManager};
    use crate::core::agent_definition::AgentId;
    use crate::core::mutation::CancellationSignal;
    use crate::core::skill_payload::{build_skill_payload, SkillPayload};
    use crate::environment::native::acquire::NativePayloadSessionStorage;
    use crate::environment::native::entry::NativeEntryAction;
    use crate::environment::native::recovery::NativeRecoveryMarkerStore;
    use crate::environment::native::tree::{inspect_entry_no_follow, physical_parent_identity};
    use crate::environment::recovery::{
        RecoveryEntryPhase, RecoveryMarkerKind, RecoveryMarkerLoad, RecoveryMarkerStore,
    };
    use crate::environment::runtime::{
        physical_target_key, ContextSnapshotRevision, ExecutionBackend, PhysicalTargetKey,
    };
    use crate::environment::types::{
        EnvironmentRef, ResourceLocator, SkillLocation, SkillLocationRef,
    };
    use crate::models::InstallMode;

    #[test]
    fn generic_unit_maps_to_one_native_entry_set() {
        let temp = tempdir().expect("temp");
        let canonical = temp.path().join("shared/demo");
        let agent = temp.path().join("agent/demo");
        fs::create_dir_all(canonical.parent().unwrap()).expect("canonical parent");
        fs::create_dir_all(agent.parent().unwrap()).expect("agent parent");
        let payload = payload(temp.path());
        let canonical_mutation = mutation(&canonical, PreparedEntryAction::Keep);
        let agent_mutation = mutation(
            &agent,
            PreparedEntryAction::Replace {
                payload_id: payload.payload_id.clone(),
                requested_mode: InstallMode::Symlink,
            },
        );
        let unit = unit(canonical_mutation, agent_mutation);

        let mapped = prepare_native_mutations(
            &unit,
            &BTreeMap::from([(payload.payload_id.clone(), Arc::new(payload))]),
            native_backend(),
        )
        .expect("mapped");

        assert_eq!(mapped.len(), 2);
        assert!(matches!(mapped[0].action, NativeEntryAction::Keep));
        assert!(matches!(
            &mapped[1].action,
            NativeEntryAction::Symlink { target } if target == &canonical
        ));
    }

    #[test]
    fn generic_unit_maps_an_explicit_managed_directory_link() {
        let temp = tempdir().expect("temp");
        let library_skill = temp.path().join("library/demo");
        let canonical = temp.path().join("shared/demo");
        let agent = temp.path().join("agent/demo");
        fs::create_dir_all(&library_skill).expect("library skill");
        fs::create_dir_all(canonical.parent().unwrap()).expect("canonical parent");
        fs::create_dir_all(agent.parent().unwrap()).expect("agent parent");
        let unit = unit(
            mutation(
                &canonical,
                PreparedEntryAction::Link {
                    target: ResourceLocator {
                        environment: EnvironmentRef::Native,
                        native_path: library_skill.to_string_lossy().into_owned(),
                    },
                },
            ),
            mutation(&agent, PreparedEntryAction::Keep),
        );

        let mapped =
            prepare_native_mutations(&unit, &BTreeMap::new(), native_backend()).expect("mapped");

        assert!(matches!(
            &mapped[0].action,
            NativeEntryAction::Symlink { target } if target == &library_skill
        ));
    }

    #[test]
    fn remove_unit_stages_agent_entry_before_primary_entry() {
        let temp = tempdir().expect("temp");
        let canonical = temp.path().join("shared/demo");
        let agent = temp.path().join("agent/demo");
        fs::create_dir_all(&canonical).expect("canonical");
        fs::create_dir_all(&agent).expect("agent");
        let unit = unit(
            mutation(&canonical, PreparedEntryAction::Remove),
            mutation(&agent, PreparedEntryAction::Remove),
        );

        let mapped =
            prepare_native_mutations(&unit, &BTreeMap::new(), native_backend()).expect("mapped");

        assert_eq!(mapped.len(), 2);
        assert_eq!(mapped[0].destination, agent);
        assert_eq!(mapped[1].destination, canonical);
    }

    #[tokio::test]
    async fn keep_only_executor_does_not_create_a_recovery_marker() {
        let temp = tempdir().expect("temp");
        let physical_root = fs::canonicalize(temp.path()).expect("physical temp root");
        let canonical = physical_root.join("shared/demo");
        let agent = physical_root.join("agent/demo");
        fs::create_dir_all(&canonical).expect("canonical");
        fs::create_dir_all(&agent).expect("agent");
        let unit = unit(
            mutation(&canonical, PreparedEntryAction::Keep),
            mutation(&agent, PreparedEntryAction::Keep),
        );
        let recovery_root = temp.path().join("recovery");
        let recovery_store =
            Arc::new(NativeRecoveryMarkerStore::new(&recovery_root).expect("recovery store"));
        let executor = NativePreparedEntryExecutor::new(
            native_backend(),
            "operation-keep-only",
            recovery_store.clone(),
        );

        let staged = executor
            .stage(&unit, &BTreeMap::new(), CancellationSignal::default())
            .await
            .expect("stage Keep-only unit");

        assert!(recovery_store
            .enumerate()
            .await
            .expect("markers")
            .is_empty());
        assert_eq!(
            fs::read_dir(&recovery_root).expect("recovery root").count(),
            0
        );
        assert!(executor.cleanup(staged).await.expect("cleanup").is_empty());
        assert_eq!(
            fs::read_dir(&recovery_root).expect("recovery root").count(),
            0
        );
    }

    #[tokio::test]
    async fn mixed_executor_recovery_contains_only_real_changes() {
        let temp = tempdir().expect("temp");
        let physical_root = fs::canonicalize(temp.path()).expect("physical temp root");
        let canonical = physical_root.join("shared/demo");
        let agent = physical_root.join("agent/demo");
        fs::create_dir_all(&canonical).expect("canonical");
        fs::create_dir_all(&agent).expect("agent");
        let unit = unit(
            mutation(&canonical, PreparedEntryAction::Keep),
            mutation(&agent, PreparedEntryAction::Remove),
        );
        let recovery_store = Arc::new(
            NativeRecoveryMarkerStore::new(temp.path().join("recovery")).expect("recovery store"),
        );
        let executor = NativePreparedEntryExecutor::new(
            native_backend(),
            "operation-mixed",
            recovery_store.clone(),
        );

        let staged = executor
            .stage(&unit, &BTreeMap::new(), CancellationSignal::default())
            .await
            .expect("stage mixed unit");
        let markers = recovery_store.enumerate().await.expect("markers");

        assert!(matches!(
            markers.as_slice(),
            [RecoveryMarkerLoad::Valid { marker, .. }]
                if matches!(marker.entries.as_slice(), [entry]
                    if entry.destination.native_path == agent.to_string_lossy())
        ));
        assert!(executor.cleanup(staged).await.expect("cleanup").is_empty());
        assert!(recovery_store
            .enumerate()
            .await
            .expect("markers after cleanup")
            .is_empty());
    }

    #[tokio::test]
    async fn executor_persists_repair_identity_before_swap_and_cleans_after_restore() {
        let temp = tempdir().expect("temp");
        let physical_root = fs::canonicalize(temp.path()).expect("physical temp root");
        let canonical = physical_root.join("shared/demo");
        let agent = physical_root.join("agent/demo");
        fs::create_dir_all(&canonical).expect("canonical");
        fs::write(canonical.join("SKILL.md"), b"old").expect("old skill");
        fs::create_dir_all(agent.parent().unwrap()).expect("agent parent");
        let payload = payload(temp.path());
        let payload_id = payload.payload_id.clone();
        let storage = Arc::new(
            NativePayloadSessionStorage::new(temp.path().join("payloads")).expect("storage"),
        );
        let manager = PayloadSessionManager::new(
            storage,
            PayloadSessionLimits {
                ttl_ms: 60_000,
                max_sessions: 4,
                max_bytes: 1_000_000,
            },
            || 1_000,
        );
        let discovery = manager
            .discover(EnvironmentRef::Native, "source-1")
            .await
            .expect("discover");
        let handle = manager
            .acquire_payload(&discovery, "demo", payload)
            .await
            .expect("acquire");
        let lease = manager.pin_verified(&handle).await.expect("pin");
        let canonical_mutation = mutation(
            &canonical,
            PreparedEntryAction::Replace {
                payload_id: payload_id.clone(),
                requested_mode: InstallMode::Copy,
            },
        );
        let agent_mutation = mutation(
            &agent,
            PreparedEntryAction::Replace {
                payload_id: payload_id.clone(),
                requested_mode: InstallMode::Symlink,
            },
        );
        let unit = unit(canonical_mutation, agent_mutation);
        let recovery_store = Arc::new(
            NativeRecoveryMarkerStore::new(temp.path().join("recovery")).expect("recovery store"),
        );
        let executor = NativePreparedEntryExecutor::for_operation(
            native_backend(),
            "operation-1",
            crate::core::mutation::MutationKind::Repair,
            recovery_store.clone(),
        );

        let mut staged = executor
            .stage(
                &unit,
                &BTreeMap::from([(payload_id, lease)]),
                CancellationSignal::default(),
            )
            .await
            .expect("stage");
        let loads = recovery_store.enumerate().await.expect("markers");
        assert!(matches!(
            loads.as_slice(),
            [RecoveryMarkerLoad::Valid { marker, .. }]
                if marker.kind == RecoveryMarkerKind::InProgress
                    && marker.subject.as_ref().is_some_and(|subject| {
                        subject.operation_kind == crate::core::mutation::MutationKind::Repair
                            && subject.skill_name == "demo"
                    })
                    && marker.entries.iter().all(|entry| {
                        entry.phase == RecoveryEntryPhase::Staged
                            && entry.backup.as_ref().is_some_and(|backup| {
                                !std::path::Path::new(&backup.native_path).exists()
                            })
                    })
        ));

        executor.recheck_entries(&staged).await.expect("recheck");
        executor.swap(&mut staged).await.expect("swap");
        executor.verify(&staged).await.expect("verify");
        assert_eq!(fs::read(canonical.join("SKILL.md")).unwrap(), b"new");
        assert!(agent
            .symlink_metadata()
            .expect("agent link")
            .file_type()
            .is_symlink());

        executor.restore(&mut staged).await.expect("restore");
        assert_eq!(fs::read(canonical.join("SKILL.md")).unwrap(), b"old");
        assert!(agent.symlink_metadata().is_err());
        assert!(executor.cleanup(staged).await.expect("cleanup").is_empty());
        assert!(recovery_store
            .enumerate()
            .await
            .expect("markers")
            .is_empty());
    }

    #[tokio::test]
    async fn executor_installs_a_new_primary_entry_before_activating_its_symlink() {
        let temp = tempdir().expect("temp");
        let physical_root = fs::canonicalize(temp.path()).expect("physical temp root");
        let canonical = physical_root.join("shared/demo");
        let agent = physical_root.join("agent/demo");
        fs::create_dir_all(canonical.parent().unwrap()).expect("canonical parent");
        fs::create_dir_all(agent.parent().unwrap()).expect("agent parent");
        let payload = payload(temp.path());
        let payload_id = payload.payload_id.clone();
        let storage = Arc::new(
            NativePayloadSessionStorage::new(temp.path().join("payloads")).expect("storage"),
        );
        let manager = PayloadSessionManager::new(
            storage,
            PayloadSessionLimits {
                ttl_ms: 60_000,
                max_sessions: 4,
                max_bytes: 1_000_000,
            },
            || 1_000,
        );
        let discovery = manager
            .discover(EnvironmentRef::Native, "source-1")
            .await
            .expect("discover");
        let handle = manager
            .acquire_payload(&discovery, "demo", payload)
            .await
            .expect("acquire");
        let lease = manager.pin_verified(&handle).await.expect("pin");
        let unit = unit(
            mutation(
                &canonical,
                PreparedEntryAction::Replace {
                    payload_id: payload_id.clone(),
                    requested_mode: InstallMode::Copy,
                },
            ),
            mutation(
                &agent,
                PreparedEntryAction::Replace {
                    payload_id: payload_id.clone(),
                    requested_mode: InstallMode::Symlink,
                },
            ),
        );
        let recovery_store = Arc::new(
            NativeRecoveryMarkerStore::new(temp.path().join("recovery")).expect("recovery store"),
        );
        let executor = NativePreparedEntryExecutor::new(
            native_backend(),
            "operation-new-install",
            recovery_store.clone(),
        );

        let mut staged = executor
            .stage(
                &unit,
                &BTreeMap::from([(payload_id, lease)]),
                CancellationSignal::default(),
            )
            .await
            .expect("stage fresh install");
        executor.recheck_entries(&staged).await.expect("recheck");
        executor.swap(&mut staged).await.expect("swap");
        executor.verify(&staged).await.expect("verify");

        assert_eq!(fs::read(canonical.join("SKILL.md")).unwrap(), b"new");
        assert_eq!(fs::read(agent.join("SKILL.md")).unwrap(), b"new");
        assert!(agent
            .symlink_metadata()
            .expect("agent link")
            .file_type()
            .is_symlink());
        assert!(executor.cleanup(staged).await.expect("cleanup").is_empty());
        assert!(recovery_store
            .enumerate()
            .await
            .expect("markers")
            .is_empty());
    }

    fn payload(root: &std::path::Path) -> SkillPayload {
        let source = root.join("source");
        fs::create_dir_all(&source).expect("source");
        fs::write(source.join("SKILL.md"), b"new").expect("skill");
        build_skill_payload(&source).expect("payload")
    }

    fn mutation(path: &std::path::Path, action: PreparedEntryAction) -> PreparedEntryMutation {
        let parent = path.parent().unwrap();
        PreparedEntryMutation {
            key: target_key(parent, path.file_name().unwrap().to_str().unwrap()),
            destination: ResourceLocator {
                environment: EnvironmentRef::Native,
                native_path: path.to_string_lossy().into_owned(),
            },
            action,
            reader_agent_ids: vec![AgentId::parse("claude-code").expect("agent")],
        }
    }

    fn unit(
        primary_entry: PreparedEntryMutation,
        agent_entry: PreparedEntryMutation,
    ) -> ExecutionUnit {
        let expected_targets = [&primary_entry, &agent_entry]
            .into_iter()
            .map(|entry| ExpectedTargetEntry {
                key: entry.key.clone(),
                fingerprint: inspect_entry_no_follow(std::path::Path::new(
                    &entry.destination.native_path,
                ))
                .expect("inspect")
                .fingerprint,
                expected_content_manifest_hash: None,
            })
            .collect();
        ExecutionUnit {
            id: "unit-1".to_string(),
            skill_name: "demo".to_string(),
            source: None,
            target: SkillLocationRef {
                environment: EnvironmentRef::Native,
                scope: SkillLocation::Global,
            },
            expected_revisions: RuntimeRevisions {
                registry: "registry-1".to_string(),
                environment: "environment-1".to_string(),
                context: ContextSnapshotRevision::parse("context-v1-native-test").unwrap(),
            },
            primary_entry: Some(primary_entry),
            additional_entries: vec![agent_entry],
            lock_mutation: None,
            expected_targets,
        }
    }

    fn target_key(parent: &std::path::Path, name: &str) -> PhysicalTargetKey {
        physical_target_key(
            native_backend(),
            physical_parent_identity(parent).expect("parent identity"),
            name,
            !cfg!(windows),
        )
        .expect("target key")
    }

    fn native_backend() -> ExecutionBackend {
        if cfg!(windows) {
            ExecutionBackend::NativeWindows
        } else {
            ExecutionBackend::NativeUnix
        }
    }

    struct FailingLockCommitter {
        destination: PathBuf,
        block_restore: bool,
    }

    impl PreparedLockCommitter for FailingLockCommitter {
        fn commit<'a>(
            &'a self,
            _mutation: &'a PreparedLockMutation,
        ) -> BoxFuture<
            'a,
            Result<
                crate::storage::lock_plan::LockCommitReceipt,
                crate::storage::atomic_document::DocumentWriteFailure,
            >,
        > {
            Box::pin(async move {
                if self.block_restore {
                    // The production entry set has already moved the original
                    // directory into its backup. A nonempty replacement makes
                    // the restore rename fail without deleting that backup.
                    fs::create_dir(&self.destination)
                        .map_err(AppError::from)
                        .map_err(
                            crate::storage::atomic_document::DocumentWriteFailure::not_published,
                        )?;
                    fs::write(self.destination.join("external.txt"), b"external")
                        .map_err(AppError::from)
                        .map_err(
                            crate::storage::atomic_document::DocumentWriteFailure::not_published,
                        )?;
                }
                Err(
                    crate::storage::atomic_document::DocumentWriteFailure::not_published(
                        AppError::ExecutionFailed {
                            message: "injected lock failure".to_string(),
                        },
                    ),
                )
            })
        }
    }

    struct PublishedUnconfirmedDocumentIo;

    struct MarkerUpdateUnconfirmedDocumentIo {
        writes: AtomicUsize,
    }

    impl crate::storage::atomic_document::AtomicDocumentIo for MarkerUpdateUnconfirmedDocumentIo {
        fn observe<'a>(
            &'a self,
            target: &'a ResourceLocator,
            max_bytes: u64,
        ) -> crate::storage::atomic_document::IoFuture<
            'a,
            Result<crate::storage::atomic_document::DocumentSnapshot, AppError>,
        > {
            crate::environment::native::atomic_file::NativeAtomicDocumentIo
                .observe(target, max_bytes)
        }

        fn replace<'a>(
            &'a self,
            target: &'a ResourceLocator,
            expected: crate::storage::atomic_document::DocumentSnapshot,
            bytes: Vec<u8>,
        ) -> crate::storage::atomic_document::IoFuture<
            'a,
            Result<
                crate::storage::atomic_document::DocumentCommitReceipt,
                crate::storage::atomic_document::DocumentWriteFailure,
            >,
        > {
            Box::pin(async move {
                let receipt = crate::environment::native::atomic_file::NativeAtomicDocumentIo
                    .replace(target, expected, bytes)
                    .await?;
                if self.writes.fetch_add(1, Ordering::SeqCst) > 0 {
                    return Err(crate::storage::atomic_document::DocumentWriteFailure {
                        error: AppError::Io {
                            message: "injected marker sync failure".to_string(),
                        },
                        phase: crate::storage::atomic_document::WritePhase::Confirming,
                        publication:
                            crate::storage::atomic_document::PublicationState::PublishedUnconfirmed,
                    });
                }
                Ok(receipt)
            })
        }

        fn remove<'a>(
            &'a self,
            target: &'a ResourceLocator,
            expected: crate::storage::atomic_document::DocumentSnapshot,
        ) -> crate::storage::atomic_document::IoFuture<
            'a,
            Result<(), crate::storage::atomic_document::DocumentWriteFailure>,
        > {
            crate::environment::native::atomic_file::NativeAtomicDocumentIo.remove(target, expected)
        }
    }

    impl crate::storage::atomic_document::AtomicDocumentIo for PublishedUnconfirmedDocumentIo {
        fn observe<'a>(
            &'a self,
            target: &'a ResourceLocator,
            max_bytes: u64,
        ) -> crate::storage::atomic_document::IoFuture<
            'a,
            Result<crate::storage::atomic_document::DocumentSnapshot, AppError>,
        > {
            Box::pin(async move {
                let max_bytes = usize::try_from(max_bytes).unwrap();
                let bytes = environment_engine::atomic_document::read_optional_bounded(
                    Path::new(&target.native_path),
                    max_bytes,
                )?;
                Ok(crate::storage::atomic_document::DocumentSnapshot {
                    bytes,
                    generation: None,
                })
            })
        }

        fn replace<'a>(
            &'a self,
            target: &'a ResourceLocator,
            expected: crate::storage::atomic_document::DocumentSnapshot,
            bytes: Vec<u8>,
        ) -> crate::storage::atomic_document::IoFuture<
            'a,
            Result<
                crate::storage::atomic_document::DocumentCommitReceipt,
                crate::storage::atomic_document::DocumentWriteFailure,
            >,
        > {
            Box::pin(async move {
                environment_engine::atomic_document::replace_if_unchanged(
                    Path::new(&target.native_path),
                    expected.bytes.as_deref(),
                    &bytes,
                )
                .map_err(crate::storage::atomic_document::DocumentWriteFailure::from_engine)?;
                Err(crate::storage::atomic_document::DocumentWriteFailure {
                    error: AppError::Io {
                        message: "injected parent sync failure".to_string(),
                    },
                    phase: crate::storage::atomic_document::WritePhase::Confirming,
                    publication:
                        crate::storage::atomic_document::PublicationState::PublishedUnconfirmed,
                })
            })
        }

        fn remove<'a>(
            &'a self,
            _target: &'a ResourceLocator,
            _expected: crate::storage::atomic_document::DocumentSnapshot,
        ) -> crate::storage::atomic_document::IoFuture<
            'a,
            Result<(), crate::storage::atomic_document::DocumentWriteFailure>,
        > {
            Box::pin(async { panic!("materialization test does not remove documents") })
        }
    }

    #[tokio::test]
    async fn native_unit_retains_backup_and_marker_when_restore_fails() {
        exercise_native_restore_failure(true).await;
    }

    #[tokio::test]
    async fn native_unit_cleans_after_confirmed_successful_restore() {
        exercise_native_restore_failure(false).await;
    }

    #[tokio::test]
    async fn native_unit_does_not_restore_entries_after_an_unconfirmed_lock_publish() {
        use crate::core::lossless_lock::{LockSchema, LosslessLockDocument};
        use crate::storage::lock_plan::{LockEntryMutation, LockExpectedState};

        let temp = tempdir().unwrap();
        let root = fs::canonicalize(temp.path()).unwrap();
        let canonical = root.join("shared/demo");
        let agent = root.join("agent/demo");
        fs::create_dir_all(&canonical).unwrap();
        fs::create_dir_all(&agent).unwrap();
        fs::write(canonical.join("SKILL.md"), b"old").unwrap();
        let unit = unit(
            mutation(&canonical, PreparedEntryAction::Remove),
            mutation(&agent, PreparedEntryAction::Keep),
        );
        let recovery = Arc::new(NativeRecoveryMarkerStore::new(root.join("recovery")).unwrap());
        let executor = NativePreparedUnitExecutor::new(
            NativePreparedEntryExecutor::new(
                native_backend(),
                "unconfirmed-lock",
                recovery.clone(),
            ),
            crate::runtime::plan_runner::RuntimeLockCommitter::with_io(Arc::new(
                PublishedUnconfirmedDocumentIo,
            )),
        );
        let lock_path = root.join("skills-lock.json");
        let lock = PreparedLockMutation {
            target: ResourceLocator {
                environment: EnvironmentRef::Native,
                native_path: lock_path.to_string_lossy().into_owned(),
            },
            legacy_target: None,
            schema: LockSchema::Project,
            entry: LockEntryMutation::Remove {
                key: "demo".to_string(),
            },
            root_replacements: BTreeMap::new(),
            expected: LockExpectedState::capture(
                &LosslessLockDocument::empty(LockSchema::Project),
                ["demo"],
                std::iter::empty::<&str>(),
            ),
        };
        let prepared = executor
            .prepare(&unit, &BTreeMap::new(), CancellationSignal::default())
            .await
            .unwrap();

        let error = executor
            .execute(prepared, Some(&lock), CancellationSignal::default())
            .await
            .unwrap_err();

        assert!(matches!(error, AppError::RecoveryRequired { .. }));
        assert!(!canonical.exists());
        let lock: serde_json::Value =
            serde_json::from_slice(&fs::read(lock_path).unwrap()).unwrap();
        assert_eq!(lock["version"], 1);
        let markers = recovery.enumerate().await.unwrap();
        let [RecoveryMarkerLoad::Valid { marker, .. }] = markers.as_slice() else {
            panic!("unconfirmed lock publication must retain recovery evidence");
        };
        assert_eq!(marker.kind, RecoveryMarkerKind::RecoveryRequired);
        let backup = marker
            .entries
            .iter()
            .find(|entry| entry.destination.native_path == canonical.to_string_lossy())
            .unwrap()
            .backup
            .as_ref()
            .unwrap();
        assert_eq!(
            fs::read(Path::new(&backup.native_path).join("SKILL.md")).unwrap(),
            b"old"
        );
    }

    #[tokio::test]
    async fn native_unit_does_not_restore_after_an_unconfirmed_marker_publish() {
        let temp = tempdir().unwrap();
        let root = fs::canonicalize(temp.path()).unwrap();
        let canonical = root.join("shared/demo");
        let agent = root.join("agent/demo");
        fs::create_dir_all(&canonical).unwrap();
        fs::create_dir_all(&agent).unwrap();
        fs::write(canonical.join("SKILL.md"), b"old").unwrap();
        let unit = unit(
            mutation(&canonical, PreparedEntryAction::Remove),
            mutation(&agent, PreparedEntryAction::Keep),
        );
        let recovery = Arc::new(
            NativeRecoveryMarkerStore::with_io(
                root.join("recovery"),
                Arc::new(MarkerUpdateUnconfirmedDocumentIo {
                    writes: AtomicUsize::new(0),
                }),
            )
            .unwrap(),
        );
        let executor = NativePreparedUnitExecutor::new(
            NativePreparedEntryExecutor::new(
                native_backend(),
                "unconfirmed-marker",
                recovery.clone(),
            ),
            FailingLockCommitter {
                destination: canonical.clone(),
                block_restore: false,
            },
        );
        let prepared = executor
            .prepare(&unit, &BTreeMap::new(), CancellationSignal::default())
            .await
            .unwrap();

        let error = executor
            .execute(prepared, None, CancellationSignal::default())
            .await
            .unwrap_err();

        assert!(matches!(error, AppError::RecoveryRequired { .. }));
        assert!(!canonical.exists());
        assert_eq!(recovery.enumerate().await.unwrap().len(), 1);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn native_prepare_rejects_a_target_parent_that_is_already_read_only() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempdir().unwrap();
        let root = fs::canonicalize(temp.path()).unwrap();
        let canonical = root.join("shared/demo");
        let agent = root.join("agent/demo");
        fs::create_dir_all(canonical.parent().unwrap()).unwrap();
        fs::create_dir_all(agent.parent().unwrap()).unwrap();
        let unit = unit(
            mutation(&canonical, PreparedEntryAction::Keep),
            mutation(
                &agent,
                PreparedEntryAction::Link {
                    target: ResourceLocator {
                        environment: EnvironmentRef::Native,
                        native_path: canonical.to_string_lossy().into_owned(),
                    },
                },
            ),
        );
        fs::set_permissions(agent.parent().unwrap(), fs::Permissions::from_mode(0o500)).unwrap();
        let recovery = Arc::new(NativeRecoveryMarkerStore::new(root.join("recovery")).unwrap());
        let executor = NativePreparedUnitExecutor::new(
            NativePreparedEntryExecutor::new(native_backend(), "read-only-preflight", recovery),
            FailingLockCommitter {
                destination: canonical,
                block_restore: false,
            },
        );

        let result = executor
            .prepare(&unit, &BTreeMap::new(), CancellationSignal::default())
            .await;

        fs::set_permissions(agent.parent().unwrap(), fs::Permissions::from_mode(0o700)).unwrap();
        assert!(result.is_err());
        assert!(!agent.exists());
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn native_prepare_rejects_a_parent_that_denies_child_rename() {
        use std::os::windows::ffi::OsStrExt;

        use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
        use windows_sys::Win32::Storage::FileSystem::{
            CreateFileW, FILE_FLAG_BACKUP_SEMANTICS, FILE_GENERIC_READ, FILE_SHARE_READ,
            OPEN_EXISTING,
        };

        let temp = tempdir().unwrap();
        let root = fs::canonicalize(temp.path()).unwrap();
        let canonical = root.join("shared/demo");
        let agent = root.join("agent/demo");
        fs::create_dir_all(canonical.parent().unwrap()).unwrap();
        fs::create_dir_all(agent.parent().unwrap()).unwrap();
        let unit = unit(
            mutation(&canonical, PreparedEntryAction::Keep),
            mutation(
                &agent,
                PreparedEntryAction::Link {
                    target: ResourceLocator {
                        environment: EnvironmentRef::Native,
                        native_path: canonical.to_string_lossy().into_owned(),
                    },
                },
            ),
        );
        let wide = agent
            .parent()
            .unwrap()
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect::<Vec<_>>();
        let handle = unsafe {
            CreateFileW(
                wide.as_ptr(),
                FILE_GENERIC_READ,
                FILE_SHARE_READ,
                std::ptr::null(),
                OPEN_EXISTING,
                FILE_FLAG_BACKUP_SEMANTICS,
                std::ptr::null_mut(),
            )
        };
        assert_ne!(handle, INVALID_HANDLE_VALUE);
        let recovery = Arc::new(NativeRecoveryMarkerStore::new(root.join("recovery")).unwrap());
        let executor = NativePreparedUnitExecutor::new(
            NativePreparedEntryExecutor::new(native_backend(), "locked-parent-preflight", recovery),
            FailingLockCommitter {
                destination: canonical,
                block_restore: false,
            },
        );

        let result = executor
            .prepare(&unit, &BTreeMap::new(), CancellationSignal::default())
            .await;

        unsafe { CloseHandle(handle) };
        assert!(result.is_err());
        assert!(!agent.exists());
    }

    async fn exercise_native_restore_failure(block_restore: bool) {
        use crate::core::lossless_lock::LockSchema;
        use crate::storage::lock_plan::{LockEntryMutation, LockExpectedState};

        let temp = tempdir().unwrap();
        let root = fs::canonicalize(temp.path()).unwrap();
        let canonical = root.join("shared/demo");
        let agent = root.join("agent/demo");
        fs::create_dir_all(&canonical).unwrap();
        fs::create_dir_all(&agent).unwrap();
        fs::write(canonical.join("SKILL.md"), b"old").unwrap();
        let unit = unit(
            mutation(&canonical, PreparedEntryAction::Remove),
            mutation(&agent, PreparedEntryAction::Keep),
        );
        let recovery = Arc::new(NativeRecoveryMarkerStore::new(root.join("recovery")).unwrap());
        let executor = NativePreparedUnitExecutor::new(
            NativePreparedEntryExecutor::new(
                native_backend(),
                "restore-regression",
                recovery.clone(),
            ),
            FailingLockCommitter {
                destination: canonical.clone(),
                block_restore,
            },
        );
        let lock = PreparedLockMutation {
            target: ResourceLocator {
                environment: EnvironmentRef::Native,
                native_path: root.join("skills-lock.json").to_string_lossy().into_owned(),
            },
            legacy_target: None,
            schema: LockSchema::Project,
            entry: LockEntryMutation::Remove {
                key: "demo".to_string(),
            },
            root_replacements: BTreeMap::new(),
            expected: LockExpectedState {
                entry_snapshots: BTreeMap::new(),
                root_snapshots: BTreeMap::new(),
            },
        };
        let prepared = executor
            .prepare(&unit, &BTreeMap::new(), CancellationSignal::default())
            .await
            .unwrap();
        let error = executor
            .execute(prepared, Some(&lock), CancellationSignal::default())
            .await
            .unwrap_err();
        let markers = recovery.enumerate().await.unwrap();
        if block_restore {
            assert!(matches!(error, AppError::RecoveryRequired { .. }));
            let [RecoveryMarkerLoad::Valid { marker, .. }] = markers.as_slice() else {
                panic!("restore failure must retain a valid recovery marker");
            };
            assert_eq!(marker.kind, RecoveryMarkerKind::RecoveryRequired);
            let backup = marker
                .entries
                .iter()
                .find(|entry| entry.destination.native_path == canonical.to_string_lossy())
                .unwrap()
                .backup
                .as_ref()
                .unwrap();
            assert_eq!(
                fs::read(Path::new(&backup.native_path).join("SKILL.md")).unwrap(),
                b"old"
            );
            assert_eq!(
                fs::read(canonical.join("external.txt")).unwrap(),
                b"external"
            );
        } else {
            assert!(matches!(error, AppError::ExecutionFailed { .. }));
            assert!(markers.is_empty());
            assert_eq!(fs::read(canonical.join("SKILL.md")).unwrap(), b"old");
        }
    }
}

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use sha2::{Digest, Sha256};

use crate::application::mutation::coordinator::{BoxFuture, PreparedEntryExecutor};
use crate::application::mutation::plan::{
    ExecutionUnit, PreparedEntryAction, PreparedEntryMutation,
};
use crate::application::mutation::result::{MutationWarning, MutationWarningCode};
use crate::application::payload_session::{PayloadLocalSource, PinnedPayloadLease};
use crate::core::mutation::CancellationSignal;
use crate::core::skill_payload::{PayloadId, SkillPayload};
use crate::environment::native::entry::{
    cleanup_entry_set, planned_recovery_paths, recheck_entry_set, restore_entry_set,
    stage_entry_set, swap_entry_set, verify_entry_set, NativeEntryAction, NativeEntryIntent,
    NativeEntrySet,
};
use crate::environment::recovery::{
    RecoveryEntryPhase, RecoveryMarker, RecoveryMarkerEntry, RecoveryMarkerKind, RecoveryMarkerRef,
    RecoveryMarkerStore, RecoverySubject, RECOVERY_MARKER_SCHEMA_VERSION,
};
use crate::environment::runtime::ExecutionBackend;
use crate::environment::types::{EnvironmentRef, ResourceLocator};
use crate::error::{AppError, RecoveryResourceId};
use crate::models::InstallMode;

pub struct NativePreparedEntrySet {
    entries: NativeEntrySet,
    recovery: Option<NativePreparedRecovery>,
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

impl PreparedEntryExecutor for NativePreparedEntryExecutor {
    type Staged = NativePreparedEntrySet;

    fn stage<'a>(
        &'a self,
        unit: &'a ExecutionUnit,
        payloads: &'a BTreeMap<PayloadId, PinnedPayloadLease>,
        cancellation: CancellationSignal,
    ) -> BoxFuture<'a, Result<Self::Staged, AppError>> {
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
            let entries = tokio::task::spawn_blocking(move || stage_entry_set(&intents))
                .await
                .map_err(native_task_error)??;
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
            Ok(NativePreparedEntrySet { entries, recovery })
        })
    }

    fn recheck_entries<'a>(
        &'a self,
        staged: &'a Self::Staged,
    ) -> BoxFuture<'a, Result<(), AppError>> {
        let entries = staged.entries.clone();
        Box::pin(async move {
            tokio::task::spawn_blocking(move || recheck_entry_set(&entries))
                .await
                .map_err(native_task_error)?
        })
    }

    fn swap<'a>(&'a self, staged: &'a mut Self::Staged) -> BoxFuture<'a, Result<(), AppError>> {
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

    fn verify<'a>(&'a self, staged: &'a Self::Staged) -> BoxFuture<'a, Result<(), AppError>> {
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

    fn restore<'a>(&'a self, staged: &'a mut Self::Staged) -> BoxFuture<'a, Result<(), AppError>> {
        Box::pin(async move {
            match restore_entry_set(&mut staged.entries) {
                Ok(()) => {
                    staged
                        .update_recovery(RecoveryMarkerKind::CleanupOnly, None)
                        .await
                }
                Err(error) => staged.recovery_required(error.to_string()).await,
            }
        })
    }

    fn cleanup<'a>(
        &'a self,
        staged: Self::Staged,
    ) -> BoxFuture<'a, Result<Vec<MutationWarning>, AppError>> {
        Box::pin(async move {
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

    async fn recovery_required(&self, message: String) -> Result<(), AppError> {
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
    use std::sync::Arc;

    use tempfile::tempdir;

    use super::*;
    use crate::application::mutation::coordinator::PreparedEntryExecutor;
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
}

use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};

use sha2::{Digest, Sha256};
use tokio::time::Duration;

use crate::application::mutation::coordinator::{
    BoxFuture, PreparedUnitExecutor, UnitTransactionReceipt,
};
use crate::application::mutation::plan::{
    ExecutionUnit, PreparedEntryAction, PreparedEntryMutation,
};
use crate::application::mutation::result::{MutationWarning, MutationWarningCode};
use crate::application::payload_session::{PayloadLocalSource, PinnedPayloadLease};
use crate::core::mutation::CancellationSignal;
use crate::core::skill_payload::{PayloadId, SkillPayloadManifest};
use crate::environment::content_manifest::ContentManifestHash;
use crate::environment::recovery::{
    RecoveryEntryPhase, RecoveryExpectedEntryState, RecoveryMarker, RecoveryMarkerEntry,
    RecoveryMarkerKind, RecoverySubject, RECOVERY_MARKER_SCHEMA_VERSION,
};
use crate::environment::runtime::posix_relative_target;
use crate::environment::runtime::{EntryFingerprint, ExecutionBackend, PhysicalParentIdentity};
use crate::environment::types::{EnvironmentRef, ResourceLocator};
use crate::environment::wsl::WslSession;
use crate::error::{AppError, RecoveryResourceId};
use crate::storage::lock_plan::{LockCommitReceipt, PreparedLockMutation};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WslEntryAction {
    Keep,
    Materialize,
    Symlink { target: String },
    Remove,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WslEntryMutation {
    pub physical_target_digest: String,
    pub destination: String,
    pub expected_fingerprint: EntryFingerprint,
    pub expected_content_manifest_hash: Option<ContentManifestHash>,
    pub action: WslEntryAction,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WslPayloadBinding {
    pub source: PayloadLocalSource,
    pub manifest: SkillPayloadManifest,
}

pub fn prepare_wsl_mutations(
    unit: &ExecutionUnit,
    payloads: &BTreeMap<PayloadId, WslPayloadBinding>,
    distro_name: &str,
    worker_generation: u64,
) -> Result<Vec<WslEntryMutation>, AppError> {
    let environment_matches = matches!(
        &unit.target.environment,
        EnvironmentRef::Wsl { distro_name: actual }
            if actual.eq_ignore_ascii_case(distro_name)
    );
    if !environment_matches {
        return Err(AppError::StaleEnvironment);
    }
    let canonical_path = unit
        .primary_entry
        .as_ref()
        .map(|entry| entry.destination.native_path.clone());
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
    let mut mutations = Vec::new();
    for entry in entries {
        validate_wsl_entry(entry, distro_name)?;
        let expected = expected
            .get(&entry.key)
            .copied()
            .ok_or(AppError::StaleTarget)?;
        let action = match &entry.action {
            PreparedEntryAction::Keep => WslEntryAction::Keep,
            PreparedEntryAction::Remove => WslEntryAction::Remove,
            PreparedEntryAction::Replace {
                payload_id,
                requested_mode: crate::models::InstallMode::Copy,
            } => {
                let binding = payloads.get(payload_id).ok_or(AppError::StalePayload)?;
                if binding.manifest.payload_id() != payload_id {
                    return Err(AppError::StalePayload);
                }
                match &binding.source {
                    PayloadLocalSource::WslManaged {
                        distro_name: source_distro,
                        worker_generation: source_generation,
                        ..
                    } if source_distro.eq_ignore_ascii_case(distro_name)
                        && *source_generation == worker_generation => {}
                    PayloadLocalSource::WslManaged {
                        distro_name: source_distro,
                        ..
                    } if source_distro.eq_ignore_ascii_case(distro_name) => {
                        return Err(AppError::StalePayload);
                    }
                    _ => {
                        return Err(AppError::CapabilityUnavailable {
                            capability: "backendLocalPayload".to_string(),
                            path: None,
                        })
                    }
                }
                WslEntryAction::Materialize
            }
            PreparedEntryAction::Replace {
                requested_mode: crate::models::InstallMode::Symlink,
                ..
            } => {
                let target = canonical_path.clone().ok_or_else(|| AppError::Validation {
                    field: Some("canonicalEntry".to_string()),
                    message: "WSL symlink entry requires a canonical entry".to_string(),
                })?;
                if target == entry.destination.native_path {
                    return Err(AppError::SelfCopy);
                }
                let parent = entry
                    .destination
                    .native_path
                    .rsplit_once('/')
                    .map(|(parent, _)| parent)
                    .filter(|parent| !parent.is_empty())
                    .ok_or(AppError::StaleTarget)?;
                let relative_target = posix_relative_target(parent, &target)?;
                WslEntryAction::Symlink {
                    target: relative_target,
                }
            }
            PreparedEntryAction::Link { target } => {
                let target_path = match &target.environment {
                    EnvironmentRef::Wsl {
                        distro_name: target_distro,
                    } if target_distro.eq_ignore_ascii_case(distro_name)
                        && target.native_path.starts_with('/') =>
                    {
                        &target.native_path
                    }
                    _ => return Err(AppError::StaleEnvironment),
                };
                if target_path == &entry.destination.native_path {
                    return Err(AppError::SelfCopy);
                }
                let parent = entry
                    .destination
                    .native_path
                    .rsplit_once('/')
                    .map(|(parent, _)| parent)
                    .filter(|parent| !parent.is_empty())
                    .ok_or(AppError::StaleTarget)?;
                WslEntryAction::Symlink {
                    target: posix_relative_target(parent, target_path)?,
                }
            }
        };
        mutations.push(WslEntryMutation {
            physical_target_digest: format!(
                "target-v1-{:x}",
                Sha256::digest(serde_json::to_vec(&entry.key)?)
            ),
            destination: entry.destination.native_path.clone(),
            expected_fingerprint: expected.fingerprint.clone(),
            expected_content_manifest_hash: expected.expected_content_manifest_hash.clone(),
            action,
        });
    }
    Ok(mutations)
}

pub fn recovery_marker_for_entry_set(
    operation_id: &str,
    unit_id: &str,
    resource_id: &str,
    environment: &EnvironmentRef,
    subject: RecoverySubject,
    entries: &[WslEntryMutation],
    created_at_epoch_ms: u64,
) -> Result<RecoveryMarker, AppError> {
    let resource_id = RecoveryResourceId::parse(resource_id.to_string()).map_err(|error| {
        AppError::Validation {
            field: Some("recoveryResourceId".to_string()),
            message: error.to_string(),
        }
    })?;
    let marker_entries = entries
        .iter()
        .enumerate()
        .filter_map(|(index, entry)| {
            if matches!(entry.action, WslEntryAction::Keep) {
                return None;
            }
            let parent = entry
                .destination
                .rsplit_once('/')
                .map(|(parent, _)| parent)
                .filter(|parent| !parent.is_empty())
                .unwrap_or("/");
            let separator = if parent == "/" { "" } else { "/" };
            Some(RecoveryMarkerEntry {
                physical_target_digest: entry.physical_target_digest.clone(),
                destination: ResourceLocator {
                    environment: environment.clone(),
                    native_path: entry.destination.clone(),
                },
                backup: Some(ResourceLocator {
                    environment: environment.clone(),
                    native_path: format!(
                        "{parent}{separator}.skill-deck-backup-{}-{index:06}",
                        resource_id.as_str()
                    ),
                }),
                expected_state: match entry.action {
                    WslEntryAction::Remove => RecoveryExpectedEntryState::Missing,
                    WslEntryAction::Materialize | WslEntryAction::Symlink { .. } => {
                        RecoveryExpectedEntryState::Present
                    }
                    WslEntryAction::Keep => unreachable!("Keep entries are filtered"),
                },
                original_fingerprint: entry.expected_fingerprint.0.clone(),
                phase: RecoveryEntryPhase::Staged,
            })
        })
        .collect::<Vec<_>>();
    Ok(RecoveryMarker {
        schema_version: RECOVERY_MARKER_SCHEMA_VERSION,
        resource_id,
        kind: RecoveryMarkerKind::InProgress,
        environment: environment.clone(),
        operation_id: operation_id.to_string(),
        unit_id: unit_id.to_string(),
        subject: Some(subject),
        created_at_epoch_ms,
        entries: marker_entries,
    })
}

fn add_lock_recovery_evidence(
    marker: &mut RecoveryMarker,
    entries: &[WslEntryMutation],
    environment: &EnvironmentRef,
) -> Result<(), AppError> {
    if !marker.entries.is_empty() {
        return Ok(());
    }
    let evidence = entries.first().ok_or_else(|| AppError::Validation {
        field: Some("entrySet".to_string()),
        message: "WSL lock mutation requires target evidence".to_string(),
    })?;
    marker.entries.push(RecoveryMarkerEntry {
        physical_target_digest: evidence.physical_target_digest.clone(),
        destination: ResourceLocator {
            environment: environment.clone(),
            native_path: evidence.destination.clone(),
        },
        backup: None,
        expected_state: if evidence.expected_fingerprint.0 == "entry-v1-missing" {
            RecoveryExpectedEntryState::Missing
        } else {
            RecoveryExpectedEntryState::Present
        },
        original_fingerprint: evidence.expected_fingerprint.0.clone(),
        phase: RecoveryEntryPhase::Staged,
    });
    Ok(())
}

fn validate_wsl_entry(entry: &PreparedEntryMutation, distro_name: &str) -> Result<(), AppError> {
    let backend_matches = matches!(
        &entry.key.backend,
        ExecutionBackend::WslPosix { distro_name: actual }
            if actual.eq_ignore_ascii_case(distro_name)
    );
    let environment_matches = matches!(
        &entry.destination.environment,
        EnvironmentRef::Wsl { distro_name: actual }
            if actual.eq_ignore_ascii_case(distro_name)
    );
    if !backend_matches || !environment_matches || !entry.destination.native_path.starts_with('/') {
        return Err(AppError::StaleTarget);
    }
    Ok(())
}

pub struct WslPreparedUnitExecutor {
    session: WslSession,
    workspace: crate::environment::wsl::WslWorkspace,
    operation_id: String,
    operation_kind: crate::core::mutation::MutationKind,
}

pub struct PreparedWslUnit {
    generation: u64,
    resource_id: String,
    request: environment_protocol::MutationUnitRequest,
}

impl WslPreparedUnitExecutor {
    pub fn for_operation(
        session: WslSession,
        workspace: crate::environment::wsl::WslWorkspace,
        operation_id: impl Into<String>,
        operation_kind: crate::core::mutation::MutationKind,
    ) -> Self {
        Self {
            session,
            workspace,
            operation_id: operation_id.into(),
            operation_kind,
        }
    }
}

impl PreparedUnitExecutor for WslPreparedUnitExecutor {
    type Prepared = PreparedWslUnit;

    fn prepare<'a>(
        &'a self,
        unit: &'a ExecutionUnit,
        payloads: &'a BTreeMap<PayloadId, PinnedPayloadLease>,
        cancellation: CancellationSignal,
    ) -> BoxFuture<'a, Result<Self::Prepared, AppError>> {
        Box::pin(async move {
            if cancellation.is_cancelled() {
                return Err(AppError::MutationCancelled);
            }
            let bindings = payloads
                .iter()
                .map(|(id, lease)| {
                    Ok((
                        id.clone(),
                        WslPayloadBinding {
                            source: lease.local_source()?,
                            manifest: lease.manifest().clone(),
                        },
                    ))
                })
                .collect::<Result<BTreeMap<_, _>, AppError>>()?;
            let mutations = prepare_wsl_mutations(
                unit,
                &bindings,
                &self.session.distro_name,
                self.session.runtime_generation,
            )?;
            let resource_id = operation_owner_id(&self.operation_id, &unit.id);
            let mut marker = recovery_marker_for_entry_set(
                &self.operation_id,
                &unit.id,
                &resource_id,
                &unit.target.environment,
                RecoverySubject {
                    operation_kind: self.operation_kind,
                    skill_name: unit.skill_name.clone(),
                    context: unit.target.clone(),
                },
                &mutations,
                now_epoch_ms(),
            )?;
            if marker.entries.is_empty() && unit.lock_mutation.is_some() {
                add_lock_recovery_evidence(&mut marker, &mutations, &unit.target.environment)?;
            }
            let initial_marker_json = if marker.entries.is_empty() {
                Vec::new()
            } else {
                serde_json::to_vec_pretty(&marker)?
            };
            let entries = mutations
                .iter()
                .map(|mutation| {
                    let planned = unit
                        .primary_entry
                        .iter()
                        .chain(&unit.additional_entries)
                        .find(|entry| entry.destination.native_path == mutation.destination)
                        .ok_or(AppError::StaleTarget)?;
                    let (expected_anchor_device, expected_anchor_inode) =
                        match &planned.key.physical_parent {
                            PhysicalParentIdentity::Wsl {
                                distro_name,
                                device,
                                inode,
                            } if distro_name.eq_ignore_ascii_case(&self.session.distro_name) => {
                                (*device, *inode)
                            }
                            _ => return Err(AppError::StaleTarget),
                        };
                    let action = match &mutation.action {
                        WslEntryAction::Keep => environment_protocol::MutationEntryAction::Keep,
                        WslEntryAction::Remove => environment_protocol::MutationEntryAction::Remove,
                        WslEntryAction::Symlink { target } => {
                            environment_protocol::MutationEntryAction::Symlink {
                                target: target.clone(),
                            }
                        }
                        WslEntryAction::Materialize => {
                            let PreparedEntryAction::Replace { payload_id, .. } = &planned.action
                            else {
                                return Err(AppError::StalePayload);
                            };
                            let binding = bindings.get(payload_id).ok_or(AppError::StalePayload)?;
                            let payload_id = match &binding.source {
                                PayloadLocalSource::WslManaged {
                                    worker_generation,
                                    worker_payload_id,
                                    ..
                                } if *worker_generation == self.session.runtime_generation => {
                                    *worker_payload_id
                                }
                                _ => return Err(AppError::StalePayload),
                            };
                            environment_protocol::MutationEntryAction::Materialize { payload_id }
                        }
                    };
                    Ok(environment_protocol::MutationEntry {
                        destination: mutation.destination.clone(),
                        expected_anchor_device,
                        expected_anchor_inode,
                        expected_fingerprint: mutation.expected_fingerprint.0.clone(),
                        expected_content_hash: mutation
                            .expected_content_manifest_hash
                            .as_ref()
                            .map(|hash| hash.as_str().to_string()),
                        action,
                    })
                })
                .collect::<Result<Vec<_>, AppError>>()?;
            Ok(PreparedWslUnit {
                generation: self.session.runtime_generation,
                resource_id: resource_id.clone(),
                request: environment_protocol::MutationUnitRequest {
                    resource_id,
                    operation_id: self.operation_id.clone(),
                    unit_id: unit.id.clone(),
                    initial_marker_json,
                    entries,
                    lock: None,
                    deadline_millis: 120_000,
                },
            })
        })
    }

    fn execute<'a>(
        &'a self,
        mut prepared: Self::Prepared,
        lock: Option<&'a PreparedLockMutation>,
        cancellation: CancellationSignal,
    ) -> BoxFuture<'a, Result<UnitTransactionReceipt, AppError>> {
        Box::pin(async move {
            prepared.request.lock = lock
                .map(|lock| wire_lock(lock, &self.session.distro_name))
                .transpose()?;
            let outcome = self
                .workspace
                .execute_worker_mutation(
                    prepared.generation,
                    &prepared.resource_id,
                    &prepared.request,
                    cancellation,
                )
                .await?;
            match outcome {
                environment_protocol::MutationUnitOutcome::Succeeded { lock, cleanup } => {
                    let mut warnings = Vec::new();
                    if let Some(cleanup) = cleanup {
                        let acknowledged = self
                            .workspace
                            .request_worker_control_for_generation(
                                prepared.generation,
                                environment_protocol::Message::AcknowledgeMutationUnit {
                                    cleanup: cleanup.clone(),
                                },
                                None,
                                Duration::from_secs(30),
                            )
                            .await;
                        if !matches!(
                            acknowledged,
                            Ok(environment_protocol::Message::MutationAcknowledged {
                                ref resource_id
                            }) if resource_id == &cleanup.resource_id
                        ) {
                            warnings.push(MutationWarning {
                                code: MutationWarningCode::BackupCleanupFailed,
                                parameters: BTreeMap::new(),
                                technical_details: acknowledged
                                    .as_ref()
                                    .err()
                                    .map(ToString::to_string),
                            });
                            warnings.push(MutationWarning {
                                code: MutationWarningCode::CleanupMarkerRetained,
                                parameters: BTreeMap::new(),
                                technical_details: None,
                            });
                        }
                    }
                    Ok(UnitTransactionReceipt {
                        lock: lock.map(host_lock_receipt).transpose()?,
                        warnings,
                    })
                }
                environment_protocol::MutationUnitOutcome::Failed {
                    code,
                    phase,
                    parameters,
                    message,
                } => Err(match code.as_str() {
                    "staleTarget" => AppError::StaleTarget,
                    "stalePayload" => AppError::StalePayload,
                    "deadlineExceeded" => AppError::WslCommandTimedOut,
                    "lockConflictSkill" => mutation_parameter(&parameters, "skillName")
                        .map(|skill_name| AppError::LockConflict {
                            target: crate::error::LockConflictTarget::Skill {
                                skill_name: skill_name.to_string(),
                            },
                        })
                        .unwrap_or_else(invalid_mutation_response),
                    "lockConflictRoot" => mutation_parameter(&parameters, "field")
                        .map(|field| AppError::LockConflict {
                            target: crate::error::LockConflictTarget::RootField {
                                field: field.to_string(),
                            },
                        })
                        .unwrap_or_else(invalid_mutation_response),
                    _ => AppError::ExecutionFailed {
                        message: format!("WSL mutation failed during {phase}: {message}"),
                    },
                }),
                environment_protocol::MutationUnitOutcome::Cancelled => {
                    Err(AppError::MutationCancelled)
                }
                environment_protocol::MutationUnitOutcome::RecoveryRequired {
                    resource_id,
                    message,
                } => Err(AppError::RecoveryRequired {
                    recovery_resource_id: RecoveryResourceId::parse(resource_id).map_err(
                        |error| AppError::ConfigurationCorrupted {
                            message: error.to_string(),
                        },
                    )?,
                    message,
                }),
            }
        })
    }
}

fn wire_lock(
    lock: &PreparedLockMutation,
    distro_name: &str,
) -> Result<environment_protocol::MutationLock, AppError> {
    if !matches!(
        &lock.target.environment,
        EnvironmentRef::Wsl { distro_name: target }
            if target.eq_ignore_ascii_case(distro_name)
    ) || !lock.target.native_path.starts_with('/')
        || lock.legacy_target.as_ref().is_some_and(|target| {
            !matches!(
                &target.environment,
                EnvironmentRef::Wsl { distro_name: legacy }
                    if legacy.eq_ignore_ascii_case(distro_name)
            ) || !target.native_path.starts_with('/')
        })
    {
        return Err(AppError::StaleEnvironment);
    }
    let entry = match &lock.entry {
        crate::storage::lock_plan::LockEntryMutation::Replace { key, replacement } => {
            environment_protocol::MutationLockEntry::Replace {
                key: key.clone(),
                replacement_json: serde_json::to_vec(replacement)?,
            }
        }
        crate::storage::lock_plan::LockEntryMutation::Remove { key } => {
            environment_protocol::MutationLockEntry::Remove { key: key.clone() }
        }
        crate::storage::lock_plan::LockEntryMutation::MoveAndReplace {
            from,
            to,
            replacement,
        } => environment_protocol::MutationLockEntry::MoveAndReplace {
            from: from.clone(),
            to: to.clone(),
            replacement_json: serde_json::to_vec(replacement)?,
        },
    };
    Ok(environment_protocol::MutationLock {
        target: lock.target.native_path.clone(),
        legacy_target: lock
            .legacy_target
            .as_ref()
            .map(|target| target.native_path.clone()),
        schema: match lock.schema {
            crate::core::lossless_lock::LockSchema::Global => {
                environment_protocol::MutationLockSchema::Global
            }
            crate::core::lossless_lock::LockSchema::Project => {
                environment_protocol::MutationLockSchema::Project
            }
        },
        entry,
        root_replacements_json: lock
            .root_replacements
            .iter()
            .map(|(field, value)| Ok((field.clone(), serde_json::to_vec(value)?)))
            .collect::<Result<_, AppError>>()?,
        expected_entries_json: lock
            .expected
            .entry_snapshots
            .iter()
            .map(|(key, snapshot)| {
                Ok((
                    key.clone(),
                    snapshot.value().map(serde_json::to_vec).transpose()?,
                ))
            })
            .collect::<Result<_, AppError>>()?,
        expected_roots_json: lock
            .expected
            .root_snapshots
            .iter()
            .map(|(field, snapshot)| {
                Ok((
                    field.clone(),
                    snapshot.value().map(serde_json::to_vec).transpose()?,
                ))
            })
            .collect::<Result<_, AppError>>()?,
    })
}

fn host_lock_receipt(
    receipt: environment_protocol::MutationLockReceipt,
) -> Result<LockCommitReceipt, AppError> {
    Ok(LockCommitReceipt {
        entry_snapshots: receipt
            .entries_json
            .into_iter()
            .map(|(key, value)| {
                value
                    .map(|bytes| serde_json::from_slice(&bytes))
                    .transpose()
                    .map(crate::core::lossless_lock::LockEntrySnapshot::from_value)
                    .map(|snapshot| (key, snapshot))
            })
            .collect::<Result<_, _>>()?,
        root_snapshots: receipt
            .roots_json
            .into_iter()
            .map(|(field, value)| {
                value
                    .map(|bytes| serde_json::from_slice(&bytes))
                    .transpose()
                    .map(crate::core::lossless_lock::LockRootSnapshot::from_value)
                    .map(|snapshot| (field, snapshot))
            })
            .collect::<Result<_, _>>()?,
    })
}

fn mutation_parameter<'a>(parameters: &'a [(String, String)], name: &str) -> Option<&'a str> {
    parameters
        .iter()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value.as_str())
}

fn invalid_mutation_response() -> AppError {
    AppError::ConfigurationCorrupted {
        message: "WSL Worker mutation response is missing an error parameter".to_string(),
    }
}

fn operation_owner_id(operation_id: &str, unit_id: &str) -> String {
    format!(
        "{:x}",
        Sha256::digest(format!("skill-deck-operation-v1\0{operation_id}\0{unit_id}").as_bytes())
    )
}

fn now_epoch_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::mutation::MutationKind;
    use crate::environment::types::{SkillLocation, SkillLocationRef};

    #[test]
    fn lock_only_unit_uses_one_keep_entry_as_recovery_evidence() {
        let environment = EnvironmentRef::Wsl {
            distro_name: "Ubuntu".to_string(),
        };
        let subject = RecoverySubject {
            operation_kind: MutationKind::Install,
            skill_name: "demo".to_string(),
            context: SkillLocationRef {
                environment: environment.clone(),
                scope: SkillLocation::Global,
            },
        };
        let entries = vec![WslEntryMutation {
            physical_target_digest: "target-1".to_string(),
            destination: "/home/alice/.agents/skills/demo".to_string(),
            expected_fingerprint: EntryFingerprint("entry-v1-current".to_string()),
            expected_content_manifest_hash: None,
            action: WslEntryAction::Keep,
        }];

        let mut marker = recovery_marker_for_entry_set(
            "operation-1",
            "unit-1",
            &"f".repeat(64),
            &environment,
            subject,
            &entries,
            1,
        )
        .unwrap();

        assert!(marker.entries.is_empty());
        add_lock_recovery_evidence(&mut marker, &entries, &environment).unwrap();
        assert_eq!(marker.entries.len(), 1);
        assert!(marker.entries[0].backup.is_none());
        assert_eq!(
            marker.entries[0].expected_state,
            RecoveryExpectedEntryState::Present
        );
    }
}

#[cfg(all(test, target_os = "windows"))]
#[allow(
    clippy::disallowed_methods,
    reason = "真实 WSL 2 Mutation 门禁需要同步启动 wsl.exe 清理测试 fixture"
)]
mod windows_worker_mutation_tests {
    use std::collections::BTreeMap;
    use std::process::Stdio;

    use crate::application::mutation::coordinator::PreparedUnitExecutor;
    use crate::application::mutation::plan::{
        ExecutionUnit, ExpectedTargetEntry, PreparedEntryAction, PreparedEntryMutation,
        RuntimeRevisions,
    };
    use crate::core::mutation::{CancellationSignal, MutationKind};
    use crate::environment::runtime::{
        ContextSnapshotRevision, ExecutionBackend, PhysicalParentIdentity, PhysicalTargetKey,
    };
    use crate::environment::types::{
        normalized_wsl_distro_name, EnvironmentRef, ResourceLocator, SkillLocation,
        SkillLocationRef,
    };
    use crate::environment::wsl::operations::entry::inspect_entries;
    use crate::environment::wsl::operations::projection::project_targets;
    use crate::environment::wsl::WslRuntime;

    #[tokio::test]
    #[ignore = "requires Windows with a WSL 2 distribution"]
    async fn real_wsl2_worker_executes_and_acknowledges_one_remove_transaction() {
        let distro =
            std::env::var("SKILL_DECK_TEST_WSL_DISTRO").unwrap_or_else(|_| "Ubuntu".to_string());
        let fixture = format!(
            "/tmp/skill-deck-mutation-gate-{}",
            uuid::Uuid::new_v4().simple()
        );
        run_fixture(
            &distro,
            "set -eu; mkdir -p \"$1/demo\"; printf old > \"$1/demo/SKILL.md\"",
            &fixture,
        )
        .await;
        let _cleanup = FixtureCleanup {
            distro: distro.clone(),
            path: fixture.clone(),
        };
        let runtime = WslRuntime::for_wsl_test();
        let workspace = runtime.workspace(&distro).unwrap();
        let session = runtime.connect(&distro).await.unwrap();
        let destination = format!("{fixture}/demo");
        let projection = project_targets(&workspace, std::slice::from_ref(&destination), None)
            .await
            .unwrap()
            .remove(0);
        let fingerprint = inspect_entries(&workspace, std::slice::from_ref(&destination), None)
            .await
            .unwrap()
            .remove(0)
            .fingerprint;
        let key = PhysicalTargetKey {
            backend: ExecutionBackend::WslPosix {
                distro_name: normalized_wsl_distro_name(&distro),
            },
            physical_parent: PhysicalParentIdentity::Wsl {
                distro_name: normalized_wsl_distro_name(&distro),
                device: projection.anchor_device,
                inode: projection.anchor_inode,
            },
            normalized_final_child_name: "demo".to_string(),
        };
        let environment = EnvironmentRef::Wsl {
            distro_name: distro.clone(),
        };
        let unit = ExecutionUnit {
            id: "remove-demo".to_string(),
            skill_name: "demo".to_string(),
            source: None,
            target: SkillLocationRef {
                environment: environment.clone(),
                scope: SkillLocation::Global,
            },
            expected_revisions: RuntimeRevisions {
                registry: "test".to_string(),
                environment: "test".to_string(),
                context: ContextSnapshotRevision::parse("context-v1-test").unwrap(),
            },
            primary_entry: Some(PreparedEntryMutation {
                key: key.clone(),
                destination: ResourceLocator {
                    environment,
                    native_path: destination.clone(),
                },
                action: PreparedEntryAction::Remove,
                reader_agent_ids: Vec::new(),
            }),
            additional_entries: Vec::new(),
            lock_mutation: None,
            expected_targets: vec![ExpectedTargetEntry {
                key,
                fingerprint,
                expected_content_manifest_hash: None,
            }],
        };
        let executor = super::WslPreparedUnitExecutor::for_operation(
            session,
            workspace.clone(),
            "mutation-gate",
            MutationKind::Remove,
        );

        let prepared = executor
            .prepare(&unit, &BTreeMap::new(), CancellationSignal::default())
            .await
            .unwrap();
        let receipt = executor
            .execute(prepared, None, CancellationSignal::default())
            .await
            .unwrap();

        assert!(receipt.warnings.is_empty());
        assert!(matches!(
            inspect_entries(&workspace, &[destination], None)
                .await
                .unwrap()[0]
                .kind,
            crate::environment::wsl::operations::entry::PosixEntryKind::Missing
        ));
    }

    async fn run_fixture(distro: &str, script: &str, path: &str) {
        let status = crate::environment::wsl::wsl_command()
            .args([
                "--distribution",
                distro,
                "--exec",
                "/bin/sh",
                "-c",
                script,
                "--",
                path,
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .status()
            .await
            .unwrap();
        assert!(status.success());
    }

    struct FixtureCleanup {
        distro: String,
        path: String,
    }

    impl Drop for FixtureCleanup {
        fn drop(&mut self) {
            let _ = std::process::Command::new("wsl.exe")
                .args([
                    "--distribution",
                    &self.distro,
                    "--exec",
                    "/bin/rm",
                    "-rf",
                    "--",
                    &self.path,
                ])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }
    }
}

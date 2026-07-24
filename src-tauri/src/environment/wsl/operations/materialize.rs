use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use sha2::{Digest, Sha256};
use tokio::time::Duration;

use crate::application::mutation::coordinator::{BoxFuture, PreparedEntryExecutor};
use crate::application::mutation::plan::{
    ExecutionUnit, PreparedEntryAction, PreparedEntryMutation,
};
use crate::application::mutation::result::MutationWarning;
use crate::application::payload_session::{PayloadLocalSource, PinnedPayloadLease};
use crate::core::mutation::CancellationSignal;
use crate::core::skill_payload::{PayloadEntryKind, PayloadId, SkillPayloadManifest};
use crate::environment::content_manifest::ContentManifestHash;
use crate::environment::recovery::{
    RecoveryEntryPhase, RecoveryExpectedEntryState, RecoveryMarker, RecoveryMarkerEntry,
    RecoveryMarkerKind, RecoveryMarkerRef, RecoveryMarkerStore, RECOVERY_MARKER_SCHEMA_VERSION,
};
use crate::environment::runtime::posix_relative_target;
use crate::environment::runtime::{EntryFingerprint, ExecutionBackend};
use crate::environment::types::{EnvironmentRef, ResourceLocator};
use crate::environment::wsl::operations::content_manifest::inspect_path as inspect_content_manifest;
use crate::environment::wsl::operations::entry::inspect_entries;
use crate::environment::wsl::operations::recovery::WslRecoveryMarkerStore;
use crate::environment::wsl::WslSession;
use crate::environment::wsl_protocol::{
    wsl_operation, wsl_operation_with_features, WslExecutionFeature, WslOperationDescriptor,
    WslOperationExecutor, WslOperationRequest, DEFAULT_WSL_STDERR_LIMIT,
};
use crate::error::{AppError, RecoveryResourceId};

const MATERIALIZE_SCRIPT: &str = include_str!("../scripts/materialize.sh");
const STAGE_OPERATION: WslOperationDescriptor = wsl_operation_with_features(
    "materialize",
    "stage",
    MATERIALIZE_SCRIPT,
    &[
        WslExecutionFeature::NulSafeXargs,
        WslExecutionFeature::Sha256Sum,
        WslExecutionFeature::StableStat,
    ],
);
const MATERIALIZE_MUTATION_FEATURES: &[WslExecutionFeature] = &[
    WslExecutionFeature::Sha256Sum,
    WslExecutionFeature::StableStat,
];
const SWAP_OPERATION: WslOperationDescriptor = wsl_operation_with_features(
    "materialize",
    "swap",
    MATERIALIZE_SCRIPT,
    MATERIALIZE_MUTATION_FEATURES,
);
const VERIFY_OPERATION: WslOperationDescriptor = wsl_operation_with_features(
    "materialize",
    "verify",
    MATERIALIZE_SCRIPT,
    MATERIALIZE_MUTATION_FEATURES,
);
const RESTORE_OPERATION: WslOperationDescriptor = wsl_operation_with_features(
    "materialize",
    "restore",
    MATERIALIZE_SCRIPT,
    MATERIALIZE_MUTATION_FEATURES,
);
const CLEANUP_OPERATION: WslOperationDescriptor =
    wsl_operation("materialize", "cleanup", MATERIALIZE_SCRIPT);
const MAX_MATERIALIZE_ENTRY_COUNT: usize = 100_000;
const MAX_MATERIALIZE_RECORD_COUNT: usize = 1_000_000;
const MAX_MATERIALIZE_STAGE_REQUEST_BYTES: usize = 16 * 1024 * 1024;

fn materialize_request_size_error() -> AppError {
    AppError::CapabilityUnavailable {
        capability: "wslMaterializeRequestSize".to_string(),
        path: None,
    }
}

fn append_stage_record(request: &mut Vec<u8>, fields: [&str; 7]) -> Result<(), AppError> {
    if fields.iter().any(|field| field.as_bytes().contains(&0)) {
        return Err(AppError::Validation {
            field: Some("materializeStageRequest".to_string()),
            message: "WSL materialize stage fields must not contain NUL".to_string(),
        });
    }
    let record_bytes = fields.iter().try_fold(0usize, |total, field| {
        total.checked_add(field.len())?.checked_add(1)
    });
    if record_bytes
        .and_then(|record_bytes| request.len().checked_add(record_bytes))
        .is_none_or(|total| total > MAX_MATERIALIZE_STAGE_REQUEST_BYTES)
    {
        return Err(materialize_request_size_error());
    }
    for field in fields {
        request.extend_from_slice(field.as_bytes());
        request.push(0);
    }
    Ok(())
}

fn materialize_stage_request(entries: &[WslEntryMutation]) -> Result<Vec<u8>, AppError> {
    if entries.len() > MAX_MATERIALIZE_ENTRY_COUNT {
        return Err(materialize_request_size_error());
    }
    let manifest_records = entries
        .iter()
        .try_fold(0usize, |count, entry| {
            count.checked_add(match &entry.action {
                WslEntryAction::Materialize { manifest, .. } => manifest.entries.len(),
                _ => 0,
            })
        })
        .ok_or_else(materialize_request_size_error)?;
    let total_records = 1usize
        .checked_add(entries.len())
        .and_then(|count| count.checked_add(manifest_records))
        .ok_or_else(materialize_request_size_error)?;
    if total_records > MAX_MATERIALIZE_RECORD_COUNT {
        return Err(materialize_request_size_error());
    }
    let total_records = total_records.to_string();
    let entry_count = entries.len().to_string();
    let mut request = Vec::new();
    append_stage_record(
        &mut request,
        ["H", "1", &total_records, &entry_count, "", "", ""],
    )?;
    for (index, entry) in entries.iter().enumerate() {
        let index = format!("{index:06}");
        let (action, source, manifest) = match &entry.action {
            WslEntryAction::Keep => ("keep", "", None),
            WslEntryAction::Materialize {
                payload_root,
                manifest,
            } => ("materialize", payload_root.as_str(), Some(manifest)),
            WslEntryAction::Symlink { target } => ("symlink", target.as_str(), None),
            WslEntryAction::Remove => ("remove", "", None),
        };
        let manifest_count = manifest
            .map_or(0, |manifest| manifest.entries.len())
            .to_string();
        append_stage_record(
            &mut request,
            [
                "E",
                &index,
                &entry.destination,
                action,
                source,
                &entry.expected_fingerprint.0,
                &manifest_count,
            ],
        )?;
        if let Some(manifest) = manifest {
            for manifest_entry in &manifest.entries {
                let (kind, blob_id) = match manifest_entry.kind {
                    PayloadEntryKind::Directory => ("directory", ""),
                    PayloadEntryKind::File => {
                        ("file", manifest_entry.blob_id.as_deref().unwrap_or(""))
                    }
                };
                let executable = if manifest_entry.executable { "1" } else { "0" };
                let expected_size = manifest_entry.size.to_string();
                append_stage_record(
                    &mut request,
                    [
                        "M",
                        &index,
                        kind,
                        &manifest_entry.relative_path,
                        blob_id,
                        executable,
                        &expected_size,
                    ],
                )?;
            }
        }
    }
    Ok(request)
}

fn parse_unit_response(bytes: &[u8]) -> Result<(), AppError> {
    (bytes == b"1\0")
        .then_some(())
        .ok_or_else(|| AppError::ConfigurationCorrupted {
            message: "invalid WSL entry-set protocol response".to_string(),
        })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WslEntryAction {
    Keep,
    Materialize {
        payload_root: String,
        manifest: SkillPayloadManifest,
    },
    Symlink {
        target: String,
    },
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
        .canonical_entry
        .as_ref()
        .map(|entry| entry.destination.native_path.clone());
    let expected = unit
        .expected_targets
        .iter()
        .map(|entry| (&entry.key, entry))
        .collect::<BTreeMap<_, _>>();
    let all_remove = unit
        .canonical_entry
        .iter()
        .chain(unit.required_agent_entries.iter())
        .all(|entry| entry.action == PreparedEntryAction::Remove);
    let entries = if all_remove {
        unit.required_agent_entries
            .iter()
            .chain(unit.canonical_entry.iter())
            .collect::<Vec<_>>()
    } else {
        unit.canonical_entry
            .iter()
            .chain(unit.required_agent_entries.iter())
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
                let payload_root = match &binding.source {
                    PayloadLocalSource::WslManaged {
                        distro_name: source_distro,
                        payload_root,
                    } if source_distro.eq_ignore_ascii_case(distro_name) => payload_root.clone(),
                    _ => {
                        return Err(AppError::CapabilityUnavailable {
                            capability: "backendLocalPayload".to_string(),
                            path: None,
                        })
                    }
                };
                WslEntryAction::Materialize {
                    payload_root,
                    manifest: binding.manifest.clone(),
                }
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
                    WslEntryAction::Materialize { .. } | WslEntryAction::Symlink { .. } => {
                        RecoveryExpectedEntryState::Present
                    }
                    WslEntryAction::Keep => unreachable!("Keep entries are filtered"),
                },
                original_fingerprint: entry.expected_fingerprint.0.clone(),
                phase: RecoveryEntryPhase::Staged,
            })
        })
        .collect();
    Ok(RecoveryMarker {
        schema_version: RECOVERY_MARKER_SCHEMA_VERSION,
        resource_id,
        kind: RecoveryMarkerKind::InProgress,
        environment: environment.clone(),
        operation_id: operation_id.to_string(),
        unit_id: unit_id.to_string(),
        created_at_epoch_ms,
        entries: marker_entries,
    })
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

pub struct WslPreparedEntrySet {
    session: WslSession,
    owner_id: String,
    operation_root: String,
    entries: Vec<WslEntryMutation>,
    recovery_store: Arc<dyn RecoveryMarkerStore>,
    recovery_marker: Mutex<Option<RecoveryMarker>>,
    recovery_ref: Option<RecoveryMarkerRef>,
}

pub struct WslPreparedEntryExecutor {
    session: WslSession,
    operation_id: String,
    recovery_store: Arc<dyn RecoveryMarkerStore>,
}

impl WslPreparedEntryExecutor {
    pub fn new(session: WslSession, operation_id: impl Into<String>) -> Self {
        let recovery_store = Arc::new(WslRecoveryMarkerStore::new(session.clone()));
        Self::with_recovery_store(session, operation_id, recovery_store)
    }

    pub fn with_recovery_store(
        session: WslSession,
        operation_id: impl Into<String>,
        recovery_store: Arc<dyn RecoveryMarkerStore>,
    ) -> Self {
        Self {
            session,
            operation_id: operation_id.into(),
            recovery_store,
        }
    }
}

impl PreparedEntryExecutor for WslPreparedEntryExecutor {
    type Staged = WslPreparedEntrySet;

    fn stage<'a>(
        &'a self,
        unit: &'a ExecutionUnit,
        payloads: &'a BTreeMap<PayloadId, PinnedPayloadLease>,
        cancellation: CancellationSignal,
    ) -> BoxFuture<'a, Result<Self::Staged, AppError>> {
        Box::pin(async move {
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
            let mutations = prepare_wsl_mutations(unit, &bindings, &self.session.distro_name)?;
            stage_entry_set(
                &self.session,
                &self.operation_id,
                &unit.id,
                mutations,
                cancellation,
                Arc::clone(&self.recovery_store),
            )
            .await
        })
    }

    fn recheck_entries<'a>(
        &'a self,
        staged: &'a Self::Staged,
    ) -> BoxFuture<'a, Result<(), AppError>> {
        Box::pin(async move { staged.recheck().await })
    }

    fn swap<'a>(&'a self, staged: &'a mut Self::Staged) -> BoxFuture<'a, Result<(), AppError>> {
        Box::pin(async move { staged.swap().await })
    }

    fn verify<'a>(&'a self, staged: &'a Self::Staged) -> BoxFuture<'a, Result<(), AppError>> {
        Box::pin(async move { staged.verify().await })
    }

    fn restore<'a>(&'a self, staged: &'a mut Self::Staged) -> BoxFuture<'a, Result<(), AppError>> {
        Box::pin(async move { staged.restore().await })
    }

    fn cleanup<'a>(
        &'a self,
        staged: Self::Staged,
    ) -> BoxFuture<'a, Result<Vec<MutationWarning>, AppError>> {
        Box::pin(async move {
            staged.cleanup().await?;
            Ok(Vec::new())
        })
    }
}

pub async fn stage_entry_set(
    session: &WslSession,
    operation_id: &str,
    unit_id: &str,
    entries: Vec<WslEntryMutation>,
    cancellation: CancellationSignal,
    recovery_store: Arc<dyn RecoveryMarkerStore>,
) -> Result<WslPreparedEntrySet, AppError> {
    if entries.is_empty() {
        return Err(AppError::Validation {
            field: Some("entrySet".to_string()),
            message: "WSL entry set must not be empty".to_string(),
        });
    }
    if let Some(entry) = entries
        .iter()
        .find(|entry| !entry.destination.starts_with('/'))
    {
        return Err(AppError::UnsafePath {
            path: entry.destination.clone(),
            reason: "WSL destination must be an absolute POSIX path".to_string(),
        });
    }
    recheck_content_manifests(session, &entries, Some(cancellation.clone())).await?;
    let request = materialize_stage_request(&entries)?;
    let owner_id = operation_owner_id(operation_id, unit_id);
    let operation_root = format!("/tmp/skill-deck-operation-{owner_id}");
    let marker = recovery_marker_for_entry_set(
        operation_id,
        unit_id,
        &owner_id,
        &EnvironmentRef::Wsl {
            distro_name: session.distro_name.clone(),
        },
        &entries,
        now_epoch_ms(),
    )?;
    let marker_ref = recovery_store.create(&marker).await?;
    let prepared = WslPreparedEntrySet {
        session: session.clone(),
        owner_id,
        operation_root,
        entries,
        recovery_store,
        recovery_marker: Mutex::new(Some(marker)),
        recovery_ref: Some(marker_ref),
    };
    if let Err(error) = run(
        &prepared.session,
        &STAGE_OPERATION,
        vec![prepared.operation_root.clone(), prepared.owner_id.clone()],
        request,
        Duration::from_secs(60),
        Some(cancellation),
    )
    .await
    {
        let _ = prepared.cleanup().await;
        return Err(error);
    }
    Ok(prepared)
}

impl WslPreparedEntrySet {
    pub async fn recheck(&self) -> Result<(), AppError> {
        let paths = self
            .entries
            .iter()
            .map(|entry| entry.destination.clone())
            .collect::<Vec<_>>();
        let actual = inspect_entries(&self.session, &paths, None).await?;
        if !actual
            .iter()
            .zip(&self.entries)
            .all(|(actual, expected)| actual.fingerprint == expected.expected_fingerprint)
        {
            return Err(AppError::StaleTarget);
        }
        recheck_content_manifests(&self.session, &self.entries, None).await
    }

    pub async fn swap(&mut self) -> Result<(), AppError> {
        self.run_static(&SWAP_OPERATION).await?;
        self.update_recovery(
            RecoveryMarkerKind::InProgress,
            Some(RecoveryEntryPhase::Swapped),
        )
        .await
    }

    pub async fn verify(&self) -> Result<(), AppError> {
        self.run_static(&VERIFY_OPERATION).await?;
        self.update_recovery(
            RecoveryMarkerKind::InProgress,
            Some(RecoveryEntryPhase::Verified),
        )
        .await
    }

    pub async fn restore(&mut self) -> Result<(), AppError> {
        match self.run_static(&RESTORE_OPERATION).await {
            Ok(()) => {
                self.update_recovery(RecoveryMarkerKind::CleanupOnly, None)
                    .await
            }
            Err(primary) => {
                if self
                    .update_recovery(
                        RecoveryMarkerKind::RecoveryRequired,
                        Some(RecoveryEntryPhase::RestoreFailed),
                    )
                    .await
                    .is_ok()
                {
                    let resource_id = RecoveryResourceId::parse(self.owner_id.clone())
                        .expect("operation owner is a SHA-256 recovery ID");
                    Err(AppError::RecoveryRequired {
                        recovery_resource_id: resource_id,
                        message: primary.to_string(),
                    })
                } else {
                    Err(AppError::RestoreFailed {
                        message: primary.to_string(),
                    })
                }
            }
        }
    }

    pub async fn cleanup(self) -> Result<(), AppError> {
        self.update_recovery(RecoveryMarkerKind::CleanupOnly, None)
            .await?;
        self.cleanup_files().await?;
        if let Some(marker_ref) = &self.recovery_ref {
            self.recovery_store.remove(marker_ref).await?;
        }
        Ok(())
    }

    async fn run_static(&self, operation: &WslOperationDescriptor) -> Result<(), AppError> {
        run(
            &self.session,
            operation,
            vec![self.operation_root.clone(), self.owner_id.clone()],
            Vec::new(),
            Duration::from_secs(30),
            None,
        )
        .await
    }

    async fn cleanup_files(&self) -> Result<(), AppError> {
        self.run_static(&CLEANUP_OPERATION).await
    }

    async fn update_recovery(
        &self,
        kind: RecoveryMarkerKind,
        phase: Option<RecoveryEntryPhase>,
    ) -> Result<(), AppError> {
        let Some(marker_ref) = &self.recovery_ref else {
            return Ok(());
        };
        let marker = self
            .recovery_marker
            .lock()
            .map_err(|_| AppError::Io {
                message: "WSL recovery marker state is unavailable".to_string(),
            })?
            .clone();
        let Some(marker) = marker else {
            return Ok(());
        };
        let updated = next_recovery_marker(&marker, kind, phase);
        self.recovery_store.update(marker_ref, &updated).await?;
        *self.recovery_marker.lock().map_err(|_| AppError::Io {
            message: "WSL recovery marker state is unavailable".to_string(),
        })? = Some(updated);
        Ok(())
    }
}

async fn recheck_content_manifests(
    session: &WslSession,
    entries: &[WslEntryMutation],
    cancellation: Option<CancellationSignal>,
) -> Result<(), AppError> {
    for entry in entries {
        let Some(expected) = &entry.expected_content_manifest_hash else {
            continue;
        };
        let actual = inspect_content_manifest(session, &entry.destination, cancellation.clone())
            .await
            .map_err(|_| AppError::StaleTarget)?;
        if actual.hash() != expected {
            return Err(AppError::StaleTarget);
        }
    }
    Ok(())
}

fn next_recovery_marker(
    marker: &RecoveryMarker,
    kind: RecoveryMarkerKind,
    phase: Option<RecoveryEntryPhase>,
) -> RecoveryMarker {
    let mut updated = marker.clone();
    updated.kind = kind;
    if let Some(phase) = phase {
        for entry in &mut updated.entries {
            entry.phase = phase;
        }
    }
    updated
}

async fn run(
    session: &WslSession,
    operation: &WslOperationDescriptor,
    args: Vec<String>,
    stdin: Vec<u8>,
    timeout: Duration,
    cancellation: Option<CancellationSignal>,
) -> Result<(), AppError> {
    let output = WslOperationExecutor::execute(
        operation,
        WslOperationRequest {
            session: session.clone(),
            args,
            stdin,
            timeout,
            stdout_limit: 32,
            stderr_limit: DEFAULT_WSL_STDERR_LIMIT,
            cancellation,
        },
    )
    .await?;
    parse_unit_response(&output.stdout)
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
    use std::collections::BTreeMap;
    use std::fs;
    #[cfg(target_os = "linux")]
    use std::io::Write;
    #[cfg(target_os = "linux")]
    use std::process::{Command, Output, Stdio};
    use std::sync::atomic::{AtomicUsize, Ordering};

    use tempfile::tempdir;

    use super::*;
    use crate::application::mutation::plan::{
        ExecutionUnit, ExpectedTargetEntry, PreparedEntryAction, PreparedEntryMutation,
        RuntimeRevisions,
    };
    use crate::application::payload_session::PayloadLocalSource;
    use crate::core::agent_definition::AgentId;
    use crate::core::skill_payload::{build_skill_payload, SkillPayload};
    use crate::environment::recovery::RecoveryMarkerKind;
    use crate::environment::recovery::{RecoveryFuture, RecoveryMarkerLoad};
    use crate::environment::runtime::{
        ContextSnapshotRevision, ExecutionBackend, PhysicalParentIdentity, PhysicalTargetKey,
    };
    use crate::environment::types::{ContextRef, ContextScope, EnvironmentRef, ResourceLocator};
    #[cfg(target_os = "linux")]
    use crate::environment::wsl::operations::entry::{parse_entry_states, ENTRY_STATE_SCRIPT};
    use crate::environment::wsl_protocol::WslExecutionProfile;
    use crate::models::InstallMode;

    fn payload_fixture(root: &std::path::Path) -> (SkillPayload, std::path::PathBuf) {
        let source = root.join("source");
        fs::create_dir_all(source.join("scripts")).unwrap();
        fs::write(source.join("SKILL.md"), b"new").unwrap();
        fs::write(source.join("scripts/run.sh"), b"#!/bin/sh\n").unwrap();
        let payload = build_skill_payload(&source).unwrap();
        let managed = root.join("payload");
        fs::create_dir_all(managed.join("blobs")).unwrap();
        for (id, blob) in &payload.blobs {
            fs::write(managed.join("blobs").join(id), blob).unwrap();
        }
        (payload, managed)
    }

    #[cfg(target_os = "linux")]
    fn fingerprint(path: &std::path::Path) -> String {
        let output = Command::new("/bin/sh")
            .arg("-c")
            .arg(ENTRY_STATE_SCRIPT)
            .arg("--")
            .arg("inspect")
            .arg(path)
            .output()
            .unwrap();
        parse_entry_states(&output.stdout, 1).unwrap()[0]
            .fingerprint
            .0
            .clone()
    }

    #[cfg(target_os = "linux")]
    fn run(script: &str, args: &[String], stdin: &[u8]) -> Output {
        let mut child = Command::new("/bin/sh")
            .arg("-c")
            .arg(script)
            .arg("--")
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(stdin).unwrap();
        child.wait_with_output().unwrap()
    }

    #[test]
    fn materialize_stage_request_uses_fixed_width_tagged_records() {
        let temp = tempdir().unwrap();
        let (payload, managed) = payload_fixture(temp.path());
        let request = materialize_stage_request(&[WslEntryMutation {
            physical_target_digest: "target-v1-demo".to_string(),
            destination: "/home/alice/.agents/skills/demo".to_string(),
            expected_fingerprint: EntryFingerprint("entry-v1-missing".to_string()),
            expected_content_manifest_hash: None,
            action: WslEntryAction::Materialize {
                payload_root: managed.to_string_lossy().into_owned(),
                manifest: payload.manifest(),
            },
        }])
        .expect("encode stage request");
        let mut fields = request
            .split(|byte| *byte == 0)
            .map(|field| String::from_utf8(field.to_vec()).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(fields.pop().as_deref(), Some(""));

        assert_eq!(fields[0], "H");
        assert_eq!(fields[1], "1");
        assert_eq!(fields[3], "1");
        assert_eq!(fields.len() % 7, 0);
        assert_eq!(fields[7], "E");
        assert_eq!(fields[8], "000000");
        assert!(fields
            .chunks_exact(7)
            .skip(2)
            .all(|record| record[0] == "M"));
    }

    #[test]
    fn materialize_stage_request_rejects_oversized_stdin_before_transport() {
        let destination = format!("/{}", "a".repeat(16 * 1024 * 1024));

        let result = materialize_stage_request(&[WslEntryMutation {
            physical_target_digest: "target-v1-oversized".to_string(),
            destination,
            expected_fingerprint: EntryFingerprint("entry-v1-missing".to_string()),
            expected_content_manifest_hash: None,
            action: WslEntryAction::Remove,
        }]);
        let error = match result {
            Err(error) => error,
            Ok(_) => panic!("oversized stage request was accepted"),
        };

        assert!(matches!(
            error,
            AppError::CapabilityUnavailable { capability, path: None }
                if capability == "wslMaterializeRequestSize"
        ));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn materialize_stage_rejects_a_truncated_fixed_width_header() {
        let temp = tempdir().unwrap();
        let operation_root = temp.path().join("skill-deck-operation-truncated");
        initialize_operation_root(&operation_root, "truncated");

        let output = run(
            MATERIALIZE_SCRIPT,
            &operation_args("stage", &operation_root, "truncated"),
            b"H\0\x31\0\x31\0\x30\0",
        );

        assert!(
            !output.status.success(),
            "truncated fixed-width header was accepted"
        );
    }

    #[tokio::test]
    async fn oversized_stage_request_does_not_create_a_recovery_marker() {
        let store = Arc::new(CountingRecoveryStore::default());
        let recovery_store: Arc<dyn RecoveryMarkerStore> = store.clone();
        let destination = format!("/{}", "a".repeat(16 * 1024 * 1024));

        let result = stage_entry_set(
            &test_session(),
            "oversized-operation",
            "oversized-unit",
            vec![WslEntryMutation {
                physical_target_digest: "target-v1-oversized".to_string(),
                destination,
                expected_fingerprint: EntryFingerprint("entry-v1-missing".to_string()),
                expected_content_manifest_hash: None,
                action: WslEntryAction::Remove,
            }],
            CancellationSignal::default(),
            recovery_store,
        )
        .await;
        let error = match result {
            Err(error) => error,
            Ok(_) => panic!("oversized stage request was accepted"),
        };

        assert!(matches!(
            error,
            AppError::CapabilityUnavailable { capability, path: None }
                if capability == "wslMaterializeRequestSize"
        ));
        assert_eq!(store.create_count.load(Ordering::SeqCst), 0);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn entry_set_stages_swaps_verifies_restores_and_cleans_full_payload() {
        let temp = tempdir().unwrap();
        let (payload, managed) = payload_fixture(temp.path());
        let destination = temp.path().join("targets/demo");
        fs::create_dir_all(&destination).unwrap();
        fs::write(destination.join("SKILL.md"), b"old").unwrap();
        let operation_root = temp.path().join("skill-deck-operation-op-1");
        initialize_operation_root(&operation_root, "op-1");
        let entries = vec![WslEntryMutation {
            physical_target_digest: "target-v1-demo".to_string(),
            destination: destination.to_string_lossy().into_owned(),
            expected_fingerprint: EntryFingerprint(fingerprint(&destination)),
            expected_content_manifest_hash: None,
            action: WslEntryAction::Materialize {
                payload_root: managed.to_string_lossy().into_owned(),
                manifest: payload.manifest(),
            },
        }];

        let output = run(
            MATERIALIZE_SCRIPT,
            &operation_args("stage", &operation_root, "op-1"),
            &materialize_stage_request(&entries).unwrap(),
        );
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(fs::read(destination.join("SKILL.md")).unwrap(), b"old");

        assert!(run(
            MATERIALIZE_SCRIPT,
            &operation_args("swap", &operation_root, "op-1"),
            &[]
        )
        .status
        .success());
        let verify_output = run(
            MATERIALIZE_SCRIPT,
            &operation_args("verify", &operation_root, "op-1"),
            &[],
        );
        assert!(
            verify_output.status.success(),
            "{}",
            String::from_utf8_lossy(&verify_output.stderr)
        );
        assert_eq!(fs::read(destination.join("SKILL.md")).unwrap(), b"new");
        assert_eq!(
            fs::read(destination.join("scripts/run.sh")).unwrap(),
            b"#!/bin/sh\n"
        );

        assert!(run(
            MATERIALIZE_SCRIPT,
            &operation_args("restore", &operation_root, "op-1"),
            &[]
        )
        .status
        .success());
        assert_eq!(fs::read(destination.join("SKILL.md")).unwrap(), b"old");
        assert!(run(
            MATERIALIZE_SCRIPT,
            &operation_args("cleanup", &operation_root, "op-1"),
            &[]
        )
        .status
        .success());
        assert!(!operation_root.exists());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn stale_entry_recheck_prevents_every_final_write_in_the_set() {
        let temp = tempdir().unwrap();
        let (payload, managed) = payload_fixture(temp.path());
        let operation_root = temp.path().join("skill-deck-operation-op-2");
        let destinations = [
            temp.path().join("targets/first"),
            temp.path().join("targets/second"),
        ];
        for destination in &destinations {
            fs::create_dir_all(destination).unwrap();
            fs::write(destination.join("SKILL.md"), b"old").unwrap();
        }
        initialize_operation_root(&operation_root, "op-2");
        let entries = destinations
            .iter()
            .enumerate()
            .map(|(index, destination)| WslEntryMutation {
                physical_target_digest: format!("target-v1-{index}"),
                destination: destination.to_string_lossy().into_owned(),
                expected_fingerprint: EntryFingerprint(fingerprint(destination)),
                expected_content_manifest_hash: None,
                action: WslEntryAction::Materialize {
                    payload_root: managed.to_string_lossy().into_owned(),
                    manifest: payload.manifest(),
                },
            })
            .collect::<Vec<_>>();
        assert!(run(
            MATERIALIZE_SCRIPT,
            &operation_args("stage", &operation_root, "op-2"),
            &materialize_stage_request(&entries).unwrap()
        )
        .status
        .success());
        fs::write(destinations[1].join("external"), b"changed").unwrap();

        let output = run(
            MATERIALIZE_SCRIPT,
            &operation_args("swap", &operation_root, "op-2"),
            &[],
        );

        assert!(!output.status.success());
        for destination in &destinations {
            assert_eq!(fs::read(destination.join("SKILL.md")).unwrap(), b"old");
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn existing_child_content_change_is_detected_before_wsl_swap() {
        let temp = tempdir().unwrap();
        let (payload, managed) = payload_fixture(temp.path());
        let destination = temp.path().join("targets/demo");
        fs::create_dir_all(&destination).unwrap();
        fs::write(destination.join("SKILL.md"), b"old").unwrap();
        let operation_root = temp.path().join("skill-deck-operation-op-content");
        initialize_operation_root(&operation_root, "op-content");
        let entries = vec![WslEntryMutation {
            physical_target_digest: "target-v1-content".to_string(),
            destination: destination.to_string_lossy().into_owned(),
            expected_fingerprint: EntryFingerprint(fingerprint(&destination)),
            expected_content_manifest_hash: None,
            action: WslEntryAction::Materialize {
                payload_root: managed.to_string_lossy().into_owned(),
                manifest: payload.manifest(),
            },
        }];
        assert!(run(
            MATERIALIZE_SCRIPT,
            &operation_args("stage", &operation_root, "op-content"),
            &materialize_stage_request(&entries).unwrap()
        )
        .status
        .success());
        fs::write(destination.join("SKILL.md"), b"locally changed").unwrap();

        assert!(!run(
            MATERIALIZE_SCRIPT,
            &operation_args("swap", &operation_root, "op-content"),
            &[]
        )
        .status
        .success());
        assert_eq!(
            fs::read(destination.join("SKILL.md")).unwrap(),
            b"locally changed"
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn tampered_stage_is_rejected_before_any_wsl_final_write() {
        let temp = tempdir().unwrap();
        let (payload, managed) = payload_fixture(temp.path());
        let operation_root = temp.path().join("skill-deck-operation-op-tampered");
        let destination = temp.path().join("targets/demo");
        fs::create_dir_all(&destination).unwrap();
        fs::write(destination.join("SKILL.md"), b"old").unwrap();
        initialize_operation_root(&operation_root, "op-tampered");
        let entries = vec![WslEntryMutation {
            physical_target_digest: "target-v1-demo".to_string(),
            destination: destination.to_string_lossy().into_owned(),
            expected_fingerprint: EntryFingerprint(fingerprint(&destination)),
            expected_content_manifest_hash: None,
            action: WslEntryAction::Materialize {
                payload_root: managed.to_string_lossy().into_owned(),
                manifest: payload.manifest(),
            },
        }];
        assert!(run(
            MATERIALIZE_SCRIPT,
            &operation_args("stage", &operation_root, "op-tampered"),
            &materialize_stage_request(&entries).unwrap()
        )
        .status
        .success());
        let stage = destination
            .parent()
            .unwrap()
            .join(".skill-deck-stage-op-tampered-000000");
        fs::write(stage.join("unexpected.txt"), b"tampered").unwrap();

        assert!(!run(
            MATERIALIZE_SCRIPT,
            &operation_args("swap", &operation_root, "op-tampered"),
            &[]
        )
        .status
        .success());
        assert_eq!(fs::read(destination.join("SKILL.md")).unwrap(), b"old");
        assert!(!destination
            .parent()
            .unwrap()
            .join(".skill-deck-backup-op-tampered-000000")
            .exists());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn symlink_and_remove_share_the_same_swap_and_restore_boundary() {
        let temp = tempdir().unwrap();
        let operation_root = temp.path().join("skill-deck-operation-op-3");
        let canonical = temp.path().join("canonical/demo");
        let link = temp.path().join("agents/demo");
        let removed = temp.path().join("duplicates/demo");
        fs::create_dir_all(&canonical).unwrap();
        fs::create_dir_all(link.parent().unwrap()).unwrap();
        fs::create_dir_all(&removed).unwrap();
        fs::write(canonical.join("SKILL.md"), b"canonical").unwrap();
        fs::write(removed.join("SKILL.md"), b"private").unwrap();
        initialize_operation_root(&operation_root, "op-3");
        let entries = vec![
            WslEntryMutation {
                physical_target_digest: "target-v1-link".to_string(),
                destination: link.to_string_lossy().into_owned(),
                expected_fingerprint: EntryFingerprint(fingerprint(&link)),
                expected_content_manifest_hash: None,
                action: WslEntryAction::Symlink {
                    target: "../canonical/demo".to_string(),
                },
            },
            WslEntryMutation {
                physical_target_digest: "target-v1-remove".to_string(),
                destination: removed.to_string_lossy().into_owned(),
                expected_fingerprint: EntryFingerprint(fingerprint(&removed)),
                expected_content_manifest_hash: None,
                action: WslEntryAction::Remove,
            },
        ];
        assert!(run(
            MATERIALIZE_SCRIPT,
            &operation_args("stage", &operation_root, "op-3"),
            &materialize_stage_request(&entries).unwrap()
        )
        .status
        .success());

        assert!(run(
            MATERIALIZE_SCRIPT,
            &operation_args("swap", &operation_root, "op-3"),
            &[]
        )
        .status
        .success());
        assert!(run(
            MATERIALIZE_SCRIPT,
            &operation_args("verify", &operation_root, "op-3"),
            &[]
        )
        .status
        .success());
        assert!(link.symlink_metadata().unwrap().file_type().is_symlink());
        assert_eq!(fs::read(link.join("SKILL.md")).unwrap(), b"canonical");
        assert!(!removed.exists());

        assert!(run(
            MATERIALIZE_SCRIPT,
            &operation_args("restore", &operation_root, "op-3"),
            &[]
        )
        .status
        .success());
        assert!(link.symlink_metadata().is_err());
        assert_eq!(fs::read(removed.join("SKILL.md")).unwrap(), b"private");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn stage_creates_a_missing_configured_root_before_staging_the_skill_child() {
        let temp = tempdir().unwrap();
        let (payload, managed) = payload_fixture(temp.path());
        let destination = temp.path().join(".custom/skills/demo");
        let operation_root = temp.path().join("skill-deck-operation-op-missing-root");
        initialize_operation_root(&operation_root, "op-missing-root");
        let entries = vec![WslEntryMutation {
            physical_target_digest: "target-v1-missing-root".to_string(),
            destination: destination.to_string_lossy().into_owned(),
            expected_fingerprint: EntryFingerprint("entry-v1-missing".to_string()),
            expected_content_manifest_hash: None,
            action: WslEntryAction::Materialize {
                payload_root: managed.to_string_lossy().into_owned(),
                manifest: payload.manifest(),
            },
        }];

        let output = run(
            MATERIALIZE_SCRIPT,
            &operation_args("stage", &operation_root, "op-missing-root"),
            &materialize_stage_request(&entries).unwrap(),
        );

        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(destination.parent().unwrap().is_dir());
        assert!(!destination.exists());
        assert!(run(
            MATERIALIZE_SCRIPT,
            &operation_args("swap", &operation_root, "op-missing-root"),
            &[]
        )
        .status
        .success());
        assert!(destination.join("SKILL.md").is_file());
    }

    #[test]
    fn generic_unit_maps_to_one_wsl_entry_set_without_backend_branches_in_services() {
        let temp = tempdir().unwrap();
        let (payload, _) = payload_fixture(temp.path());
        let environment = EnvironmentRef::Wsl {
            distro_name: "Ubuntu".to_string(),
        };
        let canonical = mutation(
            "canonical",
            "/home/alice/.agents/skills/demo",
            PreparedEntryAction::Keep,
            &environment,
        );
        let agent = mutation(
            "agent",
            "/home/alice/.claude/skills/demo",
            PreparedEntryAction::Replace {
                payload_id: payload.payload_id.clone(),
                requested_mode: InstallMode::Symlink,
            },
            &environment,
        );
        let expected_targets = [&canonical, &agent]
            .into_iter()
            .map(|entry| ExpectedTargetEntry {
                key: entry.key.clone(),
                fingerprint: EntryFingerprint(format!(
                    "entry-v1-{}",
                    entry.key.normalized_final_child_name
                )),
                expected_content_manifest_hash: None,
            })
            .collect();
        let unit = ExecutionUnit {
            id: "unit-1".to_string(),
            skill_name: "demo".to_string(),
            source: None,
            target: ContextRef {
                environment: environment.clone(),
                scope: ContextScope::Global,
            },
            expected_revisions: RuntimeRevisions {
                registry: "registry-1".to_string(),
                environment: "environment-1".to_string(),
                context: ContextSnapshotRevision::parse("context-v1-wsl-test").unwrap(),
            },
            canonical_entry: Some(canonical),
            required_agent_entries: vec![agent],
            lock_mutation: None,
            expected_targets,
        };
        let bindings = BTreeMap::from([(
            payload.payload_id.clone(),
            WslPayloadBinding {
                source: PayloadLocalSource::WslManaged {
                    distro_name: "Ubuntu".to_string(),
                    payload_root: "/tmp/skill-deck-source-session/payload-demo".to_string(),
                },
                manifest: payload.manifest(),
            },
        )]);

        let mapped = prepare_wsl_mutations(&unit, &bindings, "Ubuntu").unwrap();

        assert_eq!(mapped.len(), 2);
        assert!(matches!(mapped[0].action, WslEntryAction::Keep));
        assert_eq!(
            mapped[1].action,
            WslEntryAction::Symlink {
                target: "../../.agents/skills/demo".to_string(),
            }
        );
        let recovery = recovery_marker_for_entry_set(
            "operation-1",
            "unit-1",
            "recovery-id",
            &environment,
            &mapped,
            123,
        )
        .unwrap();
        assert_eq!(recovery.kind, RecoveryMarkerKind::InProgress);
        assert_eq!(recovery.entries.len(), 1);
        assert!(recovery.entries.iter().all(|entry| {
            entry.backup.as_ref().is_some_and(|backup| {
                backup
                    .native_path
                    .contains(".skill-deck-backup-recovery-id-")
            })
        }));
        let verified = next_recovery_marker(
            &recovery,
            RecoveryMarkerKind::InProgress,
            Some(RecoveryEntryPhase::Verified),
        );
        let cleanup = next_recovery_marker(&verified, RecoveryMarkerKind::CleanupOnly, None);
        assert_eq!(cleanup.kind, RecoveryMarkerKind::CleanupOnly);
        assert!(cleanup
            .entries
            .iter()
            .all(|entry| entry.phase == RecoveryEntryPhase::Verified));
    }

    #[test]
    fn remove_unit_stages_agent_entry_before_canonical_entry() {
        let environment = EnvironmentRef::Wsl {
            distro_name: "Ubuntu".to_string(),
        };
        let canonical = mutation(
            "canonical",
            "/home/alice/.agents/skills/demo",
            PreparedEntryAction::Remove,
            &environment,
        );
        let agent = mutation(
            "agent",
            "/home/alice/.claude/skills/demo",
            PreparedEntryAction::Remove,
            &environment,
        );
        let expected_targets = [&canonical, &agent]
            .into_iter()
            .map(|entry| ExpectedTargetEntry {
                key: entry.key.clone(),
                fingerprint: EntryFingerprint(format!(
                    "entry-v1-{}",
                    entry.key.normalized_final_child_name
                )),
                expected_content_manifest_hash: None,
            })
            .collect();
        let unit = ExecutionUnit {
            id: "remove-demo".to_string(),
            skill_name: "demo".to_string(),
            source: None,
            target: ContextRef {
                environment,
                scope: ContextScope::Global,
            },
            expected_revisions: RuntimeRevisions {
                registry: "registry-1".to_string(),
                environment: "environment-1".to_string(),
                context: ContextSnapshotRevision::parse("context-v1-wsl-remove").unwrap(),
            },
            canonical_entry: Some(canonical),
            required_agent_entries: vec![agent],
            lock_mutation: None,
            expected_targets,
        };

        let mapped = prepare_wsl_mutations(&unit, &BTreeMap::new(), "Ubuntu").unwrap();

        assert_eq!(mapped.len(), 2);
        assert_eq!(mapped[0].destination, "/home/alice/.claude/skills/demo");
        assert_eq!(mapped[1].destination, "/home/alice/.agents/skills/demo");
    }

    fn mutation(
        name: &str,
        path: &str,
        action: PreparedEntryAction,
        environment: &EnvironmentRef,
    ) -> PreparedEntryMutation {
        PreparedEntryMutation {
            key: PhysicalTargetKey {
                backend: ExecutionBackend::WslPosix {
                    distro_name: "Ubuntu".to_string(),
                },
                physical_parent: PhysicalParentIdentity::Wsl {
                    distro_name: "Ubuntu".to_string(),
                    device: 1,
                    inode: if name == "canonical" { 1 } else { 2 },
                },
                normalized_final_child_name: name.to_string(),
            },
            destination: ResourceLocator {
                environment: environment.clone(),
                native_path: path.to_string(),
            },
            action,
            owner_agent_ids: vec![AgentId::parse("claude-code").unwrap()],
        }
    }

    #[cfg(target_os = "linux")]
    fn initialize_operation_root(root: &std::path::Path, id: &str) {
        fs::create_dir_all(root).unwrap();
        fs::write(root.join(".skill-deck-owner"), format!("1\n{id}\n")).unwrap();
        fs::write(root.join("recovery.json"), b"{}").unwrap();
    }

    #[cfg(target_os = "linux")]
    fn operation_args(subcommand: &str, root: &std::path::Path, id: &str) -> Vec<String> {
        vec![
            subcommand.to_string(),
            root.to_string_lossy().into_owned(),
            id.to_string(),
        ]
    }

    fn test_session() -> WslSession {
        WslSession {
            distro_name: "Ubuntu".to_string(),
            user: "alice".to_string(),
            uid: 1000,
            home: "/home/alice".to_string(),
            xdg_state_home: None,
            config_home: "/home/alice/.config".to_string(),
            environment: BTreeMap::new(),
            git_available: true,
            execution_profile: WslExecutionProfile::all_supported(),
            runtime_generation: 0,
        }
    }

    #[derive(Default)]
    struct CountingRecoveryStore {
        create_count: AtomicUsize,
    }

    impl RecoveryMarkerStore for CountingRecoveryStore {
        fn environment(&self) -> EnvironmentRef {
            EnvironmentRef::Wsl {
                distro_name: "Ubuntu".to_string(),
            }
        }

        fn create<'a>(
            &'a self,
            marker: &'a RecoveryMarker,
        ) -> RecoveryFuture<'a, Result<RecoveryMarkerRef, AppError>> {
            self.create_count.fetch_add(1, Ordering::SeqCst);
            Box::pin(async move {
                Ok(RecoveryMarkerRef {
                    resource_id: marker.resource_id.clone(),
                    environment: marker.environment.clone(),
                    managed_root: ResourceLocator {
                        environment: marker.environment.clone(),
                        native_path: format!(
                            "/tmp/skill-deck-operation-{}",
                            marker.resource_id.as_str()
                        ),
                    },
                })
            })
        }

        fn update<'a>(
            &'a self,
            _marker_ref: &'a RecoveryMarkerRef,
            _marker: &'a RecoveryMarker,
        ) -> RecoveryFuture<'a, Result<(), AppError>> {
            Box::pin(async { Ok(()) })
        }

        fn enumerate<'a>(
            &'a self,
        ) -> RecoveryFuture<'a, Result<Vec<RecoveryMarkerLoad>, AppError>> {
            Box::pin(async { Ok(Vec::new()) })
        }

        fn remove<'a>(
            &'a self,
            _marker_ref: &'a RecoveryMarkerRef,
        ) -> RecoveryFuture<'a, Result<(), AppError>> {
            Box::pin(async { Ok(()) })
        }

        fn cleanup<'a>(
            &'a self,
            _marker_ref: &'a RecoveryMarkerRef,
            _marker: &'a RecoveryMarker,
        ) -> RecoveryFuture<'a, Result<(), AppError>> {
            Box::pin(async { Ok(()) })
        }
    }
}

use std::fmt;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use environment_engine::linux_mutation::{
    validate_intents, EntryAction, EntryIntent, MutationError as EngineMutationError,
    ParentIdentity, StagedMutation,
};
use environment_engine::lock::{
    self as engine_lock, EntryMutation as EngineLockEntry, LockMutation as EngineLockMutation,
    LockSchema as EngineLockSchema,
};
use environment_protocol::{
    MutationCleanupToken, MutationEntryAction, MutationLock, MutationLockEntry,
    MutationLockReceipt, MutationLockSchema, MutationUnitOutcome, MutationUnitRequest,
    MAX_REQUEST_DEADLINE_MILLIS,
};

use crate::payload::PayloadManager;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoveryFile {
    pub resource_id: String,
    pub managed_root: PathBuf,
    pub marker_bytes: Option<Vec<u8>>,
    pub unsafe_root: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MutationRecoveryError {
    InvalidBase,
    InvalidResource,
    UnsafeRoot,
    StaleMarker,
    Io { message: String },
}

impl fmt::Display for MutationRecoveryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for MutationRecoveryError {}

impl From<std::io::Error> for MutationRecoveryError {
    fn from(error: std::io::Error) -> Self {
        Self::Io {
            message: error.to_string(),
        }
    }
}

pub struct MutationRecoveryStore {
    namespace: PathBuf,
}

pub struct MutationManager {
    recovery: MutationRecoveryStore,
}

pub struct AcceptedMutation {
    request: MutationUnitRequest,
    intents: Vec<EntryIntent>,
    marker: Option<serde_json::Value>,
}

#[derive(Debug)]
pub enum WorkerMutationError {
    InvalidRequest,
    Payload,
    Engine(EngineMutationError),
    Lock(engine_lock::LockError),
    Recovery(MutationRecoveryError),
    LockCommitUncertain { message: String },
    Io(std::io::Error),
    Json(serde_json::Error),
}

impl fmt::Display for WorkerMutationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for WorkerMutationError {}

impl From<MutationRecoveryError> for WorkerMutationError {
    fn from(error: MutationRecoveryError) -> Self {
        Self::Recovery(error)
    }
}

impl From<EngineMutationError> for WorkerMutationError {
    fn from(error: EngineMutationError) -> Self {
        Self::Engine(error)
    }
}

impl From<std::io::Error> for WorkerMutationError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<serde_json::Error> for WorkerMutationError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

impl MutationManager {
    pub fn new(namespace: PathBuf) -> Result<Self, WorkerMutationError> {
        Ok(Self {
            recovery: MutationRecoveryStore::new(namespace)?,
        })
    }

    pub fn accept<F>(
        &self,
        request: MutationUnitRequest,
        payloads: &PayloadManager,
        is_cancelled: F,
    ) -> Result<AcceptedMutation, WorkerMutationError>
    where
        F: Fn() -> bool,
    {
        if request.entries.is_empty()
            || request.resource_id.is_empty()
            || request.operation_id.is_empty()
            || request.unit_id.is_empty()
            || request.deadline_millis == 0
            || request.deadline_millis > MAX_REQUEST_DEADLINE_MILLIS
        {
            return Err(WorkerMutationError::InvalidRequest);
        }
        let intents = request
            .entries
            .iter()
            .map(|entry| {
                let action = match &entry.action {
                    MutationEntryAction::Keep => EntryAction::Keep,
                    MutationEntryAction::Materialize { payload_id } => EntryAction::Materialize {
                        payload_root: payloads
                            .payload_root(*payload_id)
                            .map_err(|_| WorkerMutationError::Payload)?,
                    },
                    MutationEntryAction::Symlink { target } => {
                        let target = PathBuf::from(target);
                        if target.is_absolute() || target.as_os_str().is_empty() {
                            return Err(WorkerMutationError::InvalidRequest);
                        }
                        EntryAction::Symlink { target }
                    }
                    MutationEntryAction::Remove => EntryAction::Remove,
                };
                Ok(EntryIntent {
                    destination: PathBuf::from(&entry.destination),
                    expected_parent: ParentIdentity {
                        device: entry.expected_anchor_device,
                        inode: entry.expected_anchor_inode,
                    },
                    expected_fingerprint: entry.expected_fingerprint.clone(),
                    expected_content_hash: entry.expected_content_hash.clone(),
                    action,
                })
            })
            .collect::<Result<Vec<_>, WorkerMutationError>>()?;
        validate_intents(&intents, is_cancelled).map_err(WorkerMutationError::Engine)?;
        let requires_recovery = request.lock.is_some()
            || request
                .entries
                .iter()
                .any(|entry| !matches!(entry.action, MutationEntryAction::Keep));
        let marker = if requires_recovery {
            let marker: serde_json::Value = serde_json::from_slice(&request.initial_marker_json)?;
            validate_initial_marker(&request, &marker)?;
            self.recovery
                .create(&request.resource_id, &request.initial_marker_json)?;
            Some(marker)
        } else {
            None
        };
        Ok(AcceptedMutation {
            request,
            intents,
            marker,
        })
    }

    pub fn execute<F>(
        &self,
        mut accepted: AcceptedMutation,
        is_cancelled: F,
    ) -> Result<MutationUnitOutcome, WorkerMutationError>
    where
        F: Fn() -> bool,
    {
        let intents = std::mem::take(&mut accepted.intents);
        let mut staged =
            match StagedMutation::stage(&accepted.request.resource_id, intents, &is_cancelled) {
                Ok(staged) => staged,
                Err(error) => {
                    if accepted.marker.is_some() {
                        self.remove_recovery(&accepted.request.resource_id)?;
                    }
                    return Ok(engine_failure("stage", error));
                }
            };
        let mut lock_committed = false;
        let transaction = (|| {
            staged.swap(&is_cancelled)?;
            update_optional_marker_phase(&mut accepted.marker, "inProgress", Some("swapped"));
            self.write_marker(&accepted)?;
            staged.verify(|| false)?;
            update_optional_marker_phase(&mut accepted.marker, "inProgress", Some("verified"));
            self.write_marker(&accepted)?;
            let lock = match accepted.request.lock.as_ref().map(apply_lock).transpose() {
                Ok(lock) => {
                    lock_committed = lock.is_some();
                    lock
                }
                Err(error @ WorkerMutationError::LockCommitUncertain { .. }) => {
                    lock_committed = true;
                    return Err(error);
                }
                Err(error) => return Err(error),
            };
            update_optional_marker_phase(&mut accepted.marker, "inProgress", Some("lockCommitted"));
            self.write_marker(&accepted)?;
            Ok::<_, WorkerMutationError>(lock)
        })();
        let lock = match transaction {
            Ok(lock) => lock,
            Err(error) if lock_committed => {
                let stage_cleanup = staged.cleanup_stages();
                update_optional_marker_phase(
                    &mut accepted.marker,
                    "recoveryRequired",
                    Some("lockCommitted"),
                );
                let marker_error = self.write_marker(&accepted).err();
                let mut message = marker_error.map_or_else(
                    || error.to_string(),
                    |marker_error| format!("{error}; {marker_error}"),
                );
                if !stage_cleanup.is_empty() {
                    message.push_str(&format!(
                        "; stage cleanup failed: {}",
                        stage_cleanup.join("; ")
                    ));
                }
                return Ok(MutationUnitOutcome::RecoveryRequired {
                    resource_id: accepted.request.resource_id,
                    message,
                });
            }
            Err(error) => {
                let primary = error.to_string();
                let (code, parameters) = transaction_error_fields(&error);
                return match staged.restore() {
                    Ok(()) => {
                        let _ = staged.cleanup();
                        if accepted.marker.is_some() {
                            self.remove_recovery(&accepted.request.resource_id)?;
                        }
                        Ok(MutationUnitOutcome::Failed {
                            code,
                            phase: "commit".to_string(),
                            parameters,
                            message: primary,
                        })
                    }
                    Err(restore) => {
                        let stage_cleanup = staged.cleanup_stages();
                        update_optional_marker_phase(
                            &mut accepted.marker,
                            "recoveryRequired",
                            Some("restoreFailed"),
                        );
                        if accepted.marker.is_some() {
                            self.write_marker(&accepted)?;
                            let mut message = format!("{primary}; {restore}");
                            if !stage_cleanup.is_empty() {
                                message.push_str(&format!(
                                    "; stage cleanup failed: {}",
                                    stage_cleanup.join("; ")
                                ));
                            }
                            Ok(MutationUnitOutcome::RecoveryRequired {
                                resource_id: accepted.request.resource_id,
                                message,
                            })
                        } else {
                            Ok(MutationUnitOutcome::Failed {
                                code: "restoreFailed".to_string(),
                                phase: "restore".to_string(),
                                parameters: Vec::new(),
                                message: format!("{primary}; {restore}"),
                            })
                        }
                    }
                };
            }
        };
        update_optional_marker_phase(&mut accepted.marker, "cleanupOnly", None);
        self.write_marker(&accepted)?;
        let cleanup = if accepted.marker.is_some() {
            Some(MutationCleanupToken {
                resource_id: accepted.request.resource_id.clone(),
                marker_sha256: self.recovery.marker_digest(&accepted.request.resource_id)?,
            })
        } else {
            None
        };
        Ok(MutationUnitOutcome::Succeeded { lock, cleanup })
    }

    pub fn acknowledge(&self, cleanup: &MutationCleanupToken) -> Result<(), WorkerMutationError> {
        self.recovery
            .acknowledge(&cleanup.resource_id, &cleanup.marker_sha256)?;
        Ok(())
    }

    pub fn requires_acceptance(accepted: &AcceptedMutation) -> bool {
        accepted.marker.is_some()
    }

    pub fn recovery_store(&self) -> &MutationRecoveryStore {
        &self.recovery
    }

    fn write_marker(&self, accepted: &AcceptedMutation) -> Result<(), WorkerMutationError> {
        let Some(marker) = &accepted.marker else {
            return Ok(());
        };
        self.recovery.update(
            &accepted.request.resource_id,
            &serde_json::to_vec_pretty(marker)?,
        )?;
        Ok(())
    }

    fn remove_recovery(&self, resource_id: &str) -> Result<(), WorkerMutationError> {
        self.recovery.remove(resource_id)?;
        Ok(())
    }
}

impl MutationRecoveryStore {
    pub fn new(namespace: PathBuf) -> Result<Self, MutationRecoveryError> {
        if !namespace.is_absolute() {
            return Err(MutationRecoveryError::InvalidBase);
        }
        fs::create_dir_all(&namespace)?;
        let namespace = fs::canonicalize(namespace)?;
        if !namespace.is_dir() {
            return Err(MutationRecoveryError::InvalidBase);
        }
        Ok(Self { namespace })
    }

    pub fn list(&self) -> Result<Vec<RecoveryFile>, MutationRecoveryError> {
        let mut entries = fs::read_dir(&self.namespace)?
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with("skill-deck-operation-")
            })
            .collect::<Vec<_>>();
        entries.sort_by_key(fs::DirEntry::file_name);
        Ok(entries
            .into_iter()
            .map(|entry| {
                let root = entry.path();
                let resource_id = entry
                    .file_name()
                    .to_string_lossy()
                    .trim_start_matches("skill-deck-operation-")
                    .to_string();
                let safe = self.validate_root(&root, &resource_id).is_ok();
                let marker_bytes = safe
                    .then(|| fs::read(root.join("recovery.json")))
                    .transpose()
                    .ok()
                    .flatten();
                RecoveryFile {
                    resource_id,
                    managed_root: root,
                    marker_bytes,
                    unsafe_root: !safe,
                }
            })
            .collect())
    }

    pub fn create(
        &self,
        resource_id: &str,
        marker_bytes: &[u8],
    ) -> Result<PathBuf, MutationRecoveryError> {
        validate_resource_id(resource_id)?;
        if marker_bytes.is_empty() {
            return Err(MutationRecoveryError::StaleMarker);
        }
        let root = self.root(resource_id);
        if fs::symlink_metadata(&root).is_ok() {
            return Err(MutationRecoveryError::StaleMarker);
        }
        fs::create_dir(&root)?;
        set_private_directory(&root)?;
        let result = (|| {
            fs::write(
                root.join(".skill-deck-owner"),
                format!("1\n{resource_id}\n"),
            )?;
            write_atomic(&root.join("recovery.json"), marker_bytes)
        })();
        if result.is_err() {
            let _ = fs::remove_dir_all(&root);
        }
        result?;
        Ok(root)
    }

    pub fn update(
        &self,
        resource_id: &str,
        marker_bytes: &[u8],
    ) -> Result<(), MutationRecoveryError> {
        let root = self.root(resource_id);
        self.validate_root(&root, resource_id)?;
        if marker_bytes.is_empty() {
            return Err(MutationRecoveryError::StaleMarker);
        }
        write_atomic(&root.join("recovery.json"), marker_bytes)
    }

    pub fn cleanup(
        &self,
        resource_id: &str,
        expected_marker: &[u8],
        backups: &[PathBuf],
    ) -> Result<(), MutationRecoveryError> {
        let root = self.root(resource_id);
        self.validate_root(&root, resource_id)?;
        let marker = fs::read(root.join("recovery.json"))?;
        if marker != expected_marker {
            return Err(MutationRecoveryError::StaleMarker);
        }
        let expected_backups = marker_backups(&marker)?;
        if expected_backups
            .iter()
            .map(|(_, backup)| backup)
            .ne(backups.iter())
        {
            return Err(MutationRecoveryError::StaleMarker);
        }
        for (destination, backup) in &expected_backups {
            validate_backup(resource_id, destination, backup, &root)?;
        }
        for (_, backup) in &expected_backups {
            remove_no_follow(&stage_path_for_backup(resource_id, backup)?)?;
        }
        for (_, backup) in expected_backups {
            remove_no_follow(&backup)?;
        }
        fs::remove_dir_all(root)?;
        Ok(())
    }

    pub fn remove(&self, resource_id: &str) -> Result<(), MutationRecoveryError> {
        let root = self.root(resource_id);
        self.validate_root(&root, resource_id)?;
        fs::remove_dir_all(root)?;
        Ok(())
    }

    pub fn marker_digest(&self, resource_id: &str) -> Result<String, MutationRecoveryError> {
        let root = self.root(resource_id);
        self.validate_root(&root, resource_id)?;
        Ok(format!(
            "sha256:{:x}",
            Sha256::digest(fs::read(root.join("recovery.json"))?)
        ))
    }

    pub fn acknowledge(
        &self,
        resource_id: &str,
        expected_digest: &str,
    ) -> Result<(), MutationRecoveryError> {
        let root = self.root(resource_id);
        self.validate_root(&root, resource_id)?;
        let marker = fs::read(root.join("recovery.json"))?;
        let actual = format!("sha256:{:x}", Sha256::digest(&marker));
        if actual != expected_digest {
            return Err(MutationRecoveryError::StaleMarker);
        }
        let backups = marker_backups(&marker)?
            .into_iter()
            .map(|(_, backup)| backup)
            .collect::<Vec<_>>();
        self.cleanup(resource_id, &marker, &backups)
    }

    fn root(&self, resource_id: &str) -> PathBuf {
        self.namespace
            .join(format!("skill-deck-operation-{resource_id}"))
    }

    fn validate_root(&self, root: &Path, resource_id: &str) -> Result<(), MutationRecoveryError> {
        validate_resource_id(resource_id)?;
        if root != self.root(resource_id) {
            return Err(MutationRecoveryError::UnsafeRoot);
        }
        let metadata = fs::symlink_metadata(root)?;
        let owner = root.join(".skill-deck-owner");
        let owner_metadata = fs::symlink_metadata(&owner)?;
        if !metadata.is_dir()
            || metadata.file_type().is_symlink()
            || !owner_metadata.is_file()
            || owner_metadata.file_type().is_symlink()
            || fs::read_to_string(owner)? != format!("1\n{resource_id}\n")
        {
            return Err(MutationRecoveryError::UnsafeRoot);
        }
        Ok(())
    }
}

fn validate_initial_marker(
    request: &MutationUnitRequest,
    marker: &serde_json::Value,
) -> Result<(), WorkerMutationError> {
    if marker.get("resourceId").and_then(serde_json::Value::as_str) != Some(&request.resource_id)
        || marker
            .get("operationId")
            .and_then(serde_json::Value::as_str)
            != Some(&request.operation_id)
        || marker.get("unitId").and_then(serde_json::Value::as_str) != Some(&request.unit_id)
        || marker.get("kind").and_then(serde_json::Value::as_str) != Some("inProgress")
    {
        return Err(WorkerMutationError::InvalidRequest);
    }
    let marker_entries = marker
        .get("entries")
        .and_then(serde_json::Value::as_array)
        .ok_or(WorkerMutationError::InvalidRequest)?;
    let changed = request
        .entries
        .iter()
        .enumerate()
        .filter(|(_, entry)| !matches!(entry.action, MutationEntryAction::Keep))
        .collect::<Vec<_>>();
    let evidenced = if changed.is_empty() && request.lock.is_some() {
        request.entries.iter().enumerate().take(1).collect()
    } else {
        changed
    };
    if marker_entries.len() != evidenced.len() {
        return Err(WorkerMutationError::InvalidRequest);
    }
    for (marker_entry, (index, request_entry)) in marker_entries.iter().zip(evidenced) {
        let destination = marker_entry
            .get("destination")
            .and_then(|value| value.get("nativePath"))
            .and_then(serde_json::Value::as_str);
        let backup = marker_entry
            .get("backup")
            .and_then(|value| value.get("nativePath"))
            .and_then(serde_json::Value::as_str);
        let destination_path = Path::new(&request_entry.destination);
        let expected_backup = if matches!(request_entry.action, MutationEntryAction::Keep) {
            None
        } else {
            destination_path.parent().map(|parent| {
                parent.join(format!(
                    ".skill-deck-backup-{}-{index:06}",
                    request.resource_id
                ))
            })
        };
        if destination != Some(request_entry.destination.as_str())
            || backup.map(Path::new) != expected_backup.as_deref()
        {
            return Err(WorkerMutationError::InvalidRequest);
        }
    }
    Ok(())
}

fn update_marker_phase(marker: &mut serde_json::Value, kind: &str, phase: Option<&str>) {
    marker["kind"] = serde_json::Value::String(kind.to_string());
    if let (Some(phase), Some(entries)) = (
        phase,
        marker
            .get_mut("entries")
            .and_then(serde_json::Value::as_array_mut),
    ) {
        for entry in entries {
            entry["phase"] = serde_json::Value::String(phase.to_string());
        }
    }
}

fn update_optional_marker_phase(
    marker: &mut Option<serde_json::Value>,
    kind: &str,
    phase: Option<&str>,
) {
    if let Some(marker) = marker {
        update_marker_phase(marker, kind, phase);
    }
}

fn apply_lock(lock: &MutationLock) -> Result<MutationLockReceipt, WorkerMutationError> {
    let target = Path::new(&lock.target);
    if !target.is_absolute()
        || lock
            .legacy_target
            .as_deref()
            .is_some_and(|path| !Path::new(path).is_absolute())
    {
        return Err(WorkerMutationError::InvalidRequest);
    }
    let current = read_optional(target)?;
    let legacy = match (&current, &lock.legacy_target) {
        (None, Some(path)) => read_optional(Path::new(path))?,
        _ => None,
    };
    let mutation = engine_lock_mutation(lock)?;
    let applied = engine_lock::apply(current.as_deref(), legacy.as_deref(), &mutation)
        .map_err(WorkerMutationError::Lock)?;
    write_document_atomic(target, &applied.bytes)?;
    Ok(MutationLockReceipt {
        entries_json: applied
            .receipt
            .entries
            .into_iter()
            .map(|(key, value)| {
                value
                    .map(|value| serde_json::to_vec(&value))
                    .transpose()
                    .map(|value| (key, value))
            })
            .collect::<Result<_, _>>()?,
        roots_json: applied
            .receipt
            .roots
            .into_iter()
            .map(|(field, value)| {
                value
                    .map(|value| serde_json::to_vec(&value))
                    .transpose()
                    .map(|value| (field, value))
            })
            .collect::<Result<_, _>>()?,
    })
}

fn engine_lock_mutation(lock: &MutationLock) -> Result<EngineLockMutation, WorkerMutationError> {
    let parse = |bytes: &[u8]| serde_json::from_slice(bytes).map_err(WorkerMutationError::from);
    Ok(EngineLockMutation {
        schema: match lock.schema {
            MutationLockSchema::Global => EngineLockSchema::Global,
            MutationLockSchema::Project => EngineLockSchema::Project,
        },
        entry: match &lock.entry {
            MutationLockEntry::Replace {
                key,
                replacement_json,
            } => EngineLockEntry::Replace {
                key: key.clone(),
                replacement: parse(replacement_json)?,
            },
            MutationLockEntry::Remove { key } => EngineLockEntry::Remove { key: key.clone() },
            MutationLockEntry::MoveAndReplace {
                from,
                to,
                replacement_json,
            } => EngineLockEntry::MoveAndReplace {
                from: from.clone(),
                to: to.clone(),
                replacement: parse(replacement_json)?,
            },
        },
        root_replacements: lock
            .root_replacements_json
            .iter()
            .map(|(field, bytes)| parse(bytes).map(|value| (field.clone(), value)))
            .collect::<Result<_, _>>()?,
        expected_entries: lock
            .expected_entries_json
            .iter()
            .map(|(key, bytes)| {
                bytes
                    .as_deref()
                    .map(parse)
                    .transpose()
                    .map(|value| (key.clone(), value))
            })
            .collect::<Result<_, _>>()?,
        expected_roots: lock
            .expected_roots_json
            .iter()
            .map(|(field, bytes)| {
                bytes
                    .as_deref()
                    .map(parse)
                    .transpose()
                    .map(|value| (field.clone(), value))
            })
            .collect::<Result<_, _>>()?,
    })
}

fn read_optional(path: &Path) -> Result<Option<Vec<u8>>, WorkerMutationError> {
    match fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

fn write_document_atomic(path: &Path, bytes: &[u8]) -> Result<(), WorkerMutationError> {
    let parent = path.parent().ok_or(WorkerMutationError::InvalidRequest)?;
    fs::create_dir_all(parent)?;
    let legacy_backup = PathBuf::from(format!("{}.bak", path.display()));
    match fs::remove_file(&legacy_backup) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    let temporary = parent.join(format!(".skill-deck-document.{}", std::process::id()));
    if fs::symlink_metadata(&temporary).is_ok() {
        return Err(WorkerMutationError::InvalidRequest);
    }
    let mut committed = false;
    let result = (|| {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        set_private_file(&temporary)?;
        use std::io::Write;
        file.write_all(bytes)?;
        file.sync_all()?;
        fs::rename(&temporary, path)?;
        committed = true;
        fs::File::open(parent)?.sync_all()?;
        Ok::<_, std::io::Error>(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    match result {
        Ok(()) => Ok(()),
        Err(error) if committed => Err(WorkerMutationError::LockCommitUncertain {
            message: error.to_string(),
        }),
        Err(error) => Err(error.into()),
    }
}

fn engine_failure(phase: &str, error: EngineMutationError) -> MutationUnitOutcome {
    if error == EngineMutationError::Cancelled {
        MutationUnitOutcome::Cancelled
    } else {
        MutationUnitOutcome::Failed {
            code: match error {
                EngineMutationError::StaleTarget => "staleTarget",
                EngineMutationError::InvalidPayload => "stalePayload",
                _ => "executionFailed",
            }
            .to_string(),
            phase: phase.to_string(),
            parameters: Vec::new(),
            message: error.to_string(),
        }
    }
}

fn transaction_error_fields(error: &WorkerMutationError) -> (String, Vec<(String, String)>) {
    match error {
        WorkerMutationError::Lock(engine_lock::LockError::EntryConflict { key }) => (
            "lockConflictSkill".to_string(),
            vec![("skillName".to_string(), key.clone())],
        ),
        WorkerMutationError::Lock(engine_lock::LockError::RootConflict { field }) => (
            "lockConflictRoot".to_string(),
            vec![("field".to_string(), field.clone())],
        ),
        _ => ("transactionFailed".to_string(), Vec::new()),
    }
}

fn validate_resource_id(resource_id: &str) -> Result<(), MutationRecoveryError> {
    if resource_id.is_empty()
        || resource_id.len() > 128
        || !resource_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        Err(MutationRecoveryError::InvalidResource)
    } else {
        Ok(())
    }
}

fn validate_backup(
    resource_id: &str,
    destination: &Path,
    backup: &Path,
    root: &Path,
) -> Result<(), MutationRecoveryError> {
    let name = backup
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or(MutationRecoveryError::UnsafeRoot)?;
    if !backup.is_absolute()
        || !destination.is_absolute()
        || unsafe_lexical_path(backup)
        || unsafe_lexical_path(destination)
        || backup == root
        || backup == destination
        || backup.parent() != destination.parent()
        || !name.starts_with(&format!(".skill-deck-backup-{resource_id}-"))
    {
        return Err(MutationRecoveryError::UnsafeRoot);
    }
    Ok(())
}

fn marker_backups(marker: &[u8]) -> Result<Vec<(PathBuf, PathBuf)>, MutationRecoveryError> {
    let value: serde_json::Value =
        serde_json::from_slice(marker).map_err(|_| MutationRecoveryError::StaleMarker)?;
    value
        .get("entries")
        .and_then(serde_json::Value::as_array)
        .ok_or(MutationRecoveryError::StaleMarker)?
        .iter()
        .filter_map(|entry| {
            let backup = entry.get("backup")?;
            (!backup.is_null()).then_some((entry, backup))
        })
        .map(|(entry, backup)| {
            let destination = entry
                .get("destination")
                .and_then(|value| value.get("nativePath"))
                .and_then(serde_json::Value::as_str)
                .ok_or(MutationRecoveryError::StaleMarker)?;
            let backup = backup
                .get("nativePath")
                .and_then(serde_json::Value::as_str)
                .ok_or(MutationRecoveryError::StaleMarker)?;
            Ok((PathBuf::from(destination), PathBuf::from(backup)))
        })
        .collect()
}

fn stage_path_for_backup(
    resource_id: &str,
    backup: &Path,
) -> Result<PathBuf, MutationRecoveryError> {
    let name = backup
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or(MutationRecoveryError::UnsafeRoot)?;
    let suffix = name
        .strip_prefix(&format!(".skill-deck-backup-{resource_id}-"))
        .filter(|suffix| !suffix.is_empty())
        .ok_or(MutationRecoveryError::UnsafeRoot)?;
    Ok(backup.with_file_name(format!(".skill-deck-stage-{resource_id}-{suffix}")))
}

fn unsafe_lexical_path(path: &Path) -> bool {
    path.components().any(|component| {
        matches!(
            component,
            std::path::Component::CurDir | std::path::Component::ParentDir
        )
    })
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), MutationRecoveryError> {
    let parent = path.parent().ok_or(MutationRecoveryError::UnsafeRoot)?;
    let temporary = parent.join(format!(".recovery-{}", std::process::id()));
    if fs::symlink_metadata(&temporary).is_ok() {
        return Err(MutationRecoveryError::StaleMarker);
    }
    let result = (|| {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        set_private_file(&temporary)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        fs::rename(&temporary, path)?;
        fs::File::open(parent)?.sync_all()?;
        Ok::<_, std::io::Error>(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result.map_err(Into::into)
}

fn remove_no_follow(path: &Path) -> Result<(), MutationRecoveryError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {
            fs::remove_dir_all(path)?
        }
        Ok(_) => fs::remove_file(path)?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    Ok(())
}

#[cfg(unix)]
fn set_private_directory(path: &Path) -> Result<(), MutationRecoveryError> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    Ok(())
}

#[cfg(not(unix))]
fn set_private_directory(_path: &Path) -> Result<(), MutationRecoveryError> {
    Ok(())
}

#[cfg(unix)]
fn set_private_file(path: &Path) -> Result<(), std::io::Error> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
}

#[cfg(not(unix))]
fn set_private_file(_path: &Path) -> Result<(), std::io::Error> {
    Ok(())
}

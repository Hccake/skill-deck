use std::collections::BTreeSet;
use std::fmt;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};
use unicode_normalization::UnicodeNormalization;

use crate::entry::{inspect_entries, EntryKind, EntryRequest};
use crate::manifest::{build_manifest, ManifestKind, ManifestRequest, ManifestResponse};
#[cfg(target_os = "linux")]
use crate::payload::read_blob;
use crate::payload::{verify_payload, PayloadEntryKind, PayloadManifest};
#[cfg(target_os = "linux")]
use crate::projection::{project_targets, ProjectionRequest};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParentIdentity {
    pub device: u64,
    pub inode: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EntryAction {
    Keep,
    Materialize { payload_root: PathBuf },
    Symlink { target: PathBuf },
    Remove,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryIntent {
    pub destination: PathBuf,
    pub expected_parent: ParentIdentity,
    pub expected_fingerprint: String,
    pub expected_content_hash: Option<String>,
    pub action: EntryAction,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoveryEntry {
    pub destination: PathBuf,
    pub backup: PathBuf,
    pub expected_present: bool,
    pub original_fingerprint: String,
}

#[derive(Debug)]
struct StagedEntry {
    intent: EntryIntent,
    parent_identity: Option<ParentIdentity>,
    stage: Option<PathBuf>,
    backup: PathBuf,
    backup_created: bool,
    installed: bool,
}

#[derive(Debug)]
pub struct StagedMutation {
    entries: Vec<StagedEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MutationError {
    UnsupportedPlatform,
    InvalidRequest,
    StaleTarget,
    InvalidPayload,
    VerificationFailed,
    Cancelled,
    RestoreFailed { message: String },
    Io { message: String },
}

impl fmt::Display for MutationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for MutationError {}

impl From<std::io::Error> for MutationError {
    fn from(error: std::io::Error) -> Self {
        Self::Io {
            message: error.to_string(),
        }
    }
}

pub fn parent_identity(path: &Path) -> Result<ParentIdentity, MutationError> {
    parent_identity_platform(path)
}

pub fn fingerprint_path(path: &Path) -> Result<String, MutationError> {
    let fact = inspect_entries(&EntryRequest {
        paths: vec![path.to_path_buf()],
    })
    .map_err(|_| MutationError::Io {
        message: format!("failed to inspect {}", path.display()),
    })?
    .facts
    .into_iter()
    .next()
    .ok_or(MutationError::InvalidRequest)?;
    if fact.kind == EntryKind::Missing {
        return Ok("entry-v1-missing".to_string());
    }
    let metadata = fact.metadata.ok_or(MutationError::InvalidRequest)?;
    let values = [
        metadata.device.to_string(),
        metadata.inode.to_string(),
        format!("{:x}", metadata.mode),
        metadata.size.to_string(),
        metadata.mtime_seconds.to_string(),
        format!("{:09}", metadata.mtime_nanos),
    ];
    let mut hasher = Sha256::new();
    hasher.update(b"skill-deck-wsl-entry-v1\0");
    for value in values {
        hasher.update(value.as_bytes());
        hasher.update([0]);
    }
    if let Some(target) = fact.link_target {
        hasher.update(target.as_os_str().as_encoded_bytes());
    }
    Ok(format!("entry-v1-{:x}", hasher.finalize()))
}

pub fn validate_intents<F>(intents: &[EntryIntent], is_cancelled: F) -> Result<(), MutationError>
where
    F: Fn() -> bool,
{
    validate_intents_platform(intents, &is_cancelled)
}

impl StagedMutation {
    pub fn stage<F>(
        operation_id: &str,
        intents: Vec<EntryIntent>,
        is_cancelled: F,
    ) -> Result<Self, MutationError>
    where
        F: Fn() -> bool,
    {
        stage_platform(operation_id, intents, &is_cancelled)
    }

    pub fn recheck<F>(&self, is_cancelled: F) -> Result<(), MutationError>
    where
        F: Fn() -> bool,
    {
        for entry in &self.entries {
            check_cancelled(&is_cancelled)?;
            recheck_entry(entry)?;
        }
        Ok(())
    }

    pub fn swap<F>(&mut self, is_cancelled: F) -> Result<(), MutationError>
    where
        F: Fn() -> bool,
    {
        self.recheck(&is_cancelled)?;
        for index in 0..self.entries.len() {
            if let Err(primary) = swap_one(&mut self.entries[index]) {
                if let Err(restore) = self.restore() {
                    return Err(MutationError::RestoreFailed {
                        message: format!("{primary}; {restore}"),
                    });
                }
                return Err(primary);
            }
        }
        Ok(())
    }

    pub fn verify<F>(&self, is_cancelled: F) -> Result<(), MutationError>
    where
        F: Fn() -> bool,
    {
        for entry in &self.entries {
            check_cancelled(&is_cancelled)?;
            let valid = match &entry.intent.action {
                EntryAction::Keep => {
                    fingerprint_path(&entry.intent.destination)?
                        == entry.intent.expected_fingerprint
                        && entry
                            .intent
                            .expected_content_hash
                            .as_ref()
                            .is_none_or(|expected| {
                                content_hash_path(&entry.intent.destination).as_ref()
                                    == Ok(expected)
                            })
                }
                EntryAction::Materialize { payload_root } => {
                    verify_materialized(payload_root, &entry.intent.destination).is_ok()
                }
                EntryAction::Symlink { target } => {
                    fs::symlink_metadata(&entry.intent.destination)
                        .is_ok_and(|metadata| metadata.file_type().is_symlink())
                        && fs::read_link(&entry.intent.destination)
                            .is_ok_and(|actual| &actual == target)
                }
                EntryAction::Remove => {
                    fingerprint_path(&entry.intent.destination).as_deref() == Ok("entry-v1-missing")
                }
            };
            if !valid {
                return Err(MutationError::VerificationFailed);
            }
        }
        Ok(())
    }

    pub fn restore(&mut self) -> Result<(), MutationError> {
        for entry in self.entries.iter_mut().rev() {
            if entry.installed {
                remove_no_follow(&entry.intent.destination)?;
                entry.installed = false;
            }
            if entry.backup_created {
                fs::rename(&entry.backup, &entry.intent.destination)?;
                entry.backup_created = false;
            }
        }
        Ok(())
    }

    pub fn recovery_entries(&self) -> Vec<RecoveryEntry> {
        self.entries
            .iter()
            .filter(|entry| !matches!(entry.intent.action, EntryAction::Keep))
            .map(|entry| RecoveryEntry {
                destination: entry.intent.destination.clone(),
                backup: entry.backup.clone(),
                expected_present: !matches!(entry.intent.action, EntryAction::Remove),
                original_fingerprint: entry.intent.expected_fingerprint.clone(),
            })
            .collect()
    }

    pub fn cleanup_stages(&mut self) -> Vec<String> {
        let mut warnings = Vec::new();
        for entry in &mut self.entries {
            if let Some(path) = entry.stage.clone() {
                if let Err(error) = remove_no_follow(&path) {
                    warnings.push(format!("{}: {error}", path.display()));
                } else {
                    entry.stage = None;
                }
            }
        }
        warnings
    }

    pub fn cleanup(mut self) -> Result<Vec<String>, MutationError> {
        let mut warnings = self.cleanup_stages();
        for entry in self.entries {
            if entry.backup_created {
                if let Err(error) = remove_no_follow(&entry.backup) {
                    warnings.push(format!("{}: {error}", entry.backup.display()));
                }
            }
        }
        Ok(warnings)
    }
}

#[cfg(not(target_os = "linux"))]
fn parent_identity_platform(_path: &Path) -> Result<ParentIdentity, MutationError> {
    Err(MutationError::UnsupportedPlatform)
}

#[cfg(target_os = "linux")]
fn parent_identity_platform(path: &Path) -> Result<ParentIdentity, MutationError> {
    use std::os::unix::fs::MetadataExt;
    let metadata = fs::metadata(path)?;
    if !metadata.is_dir() {
        return Err(MutationError::InvalidRequest);
    }
    Ok(ParentIdentity {
        device: metadata.dev(),
        inode: metadata.ino(),
    })
}

#[cfg(not(target_os = "linux"))]
fn stage_platform(
    _operation_id: &str,
    _intents: Vec<EntryIntent>,
    _is_cancelled: &impl Fn() -> bool,
) -> Result<StagedMutation, MutationError> {
    Err(MutationError::UnsupportedPlatform)
}

#[cfg(target_os = "linux")]
fn stage_platform(
    operation_id: &str,
    intents: Vec<EntryIntent>,
    is_cancelled: &impl Fn() -> bool,
) -> Result<StagedMutation, MutationError> {
    if intents.is_empty()
        || operation_id.is_empty()
        || !operation_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(MutationError::InvalidRequest);
    }
    validate_intents_platform(&intents, is_cancelled)?;
    let mut entries = Vec::with_capacity(intents.len());
    for (index, intent) in intents.into_iter().enumerate() {
        let mut current_stage = None;
        let staged = (|| {
            check_cancelled(is_cancelled)?;
            let parent = intent
                .destination
                .parent()
                .ok_or(MutationError::InvalidRequest)?;
            let parent_identity = match &intent.action {
                EntryAction::Keep | EntryAction::Remove => None,
                EntryAction::Materialize { .. } | EntryAction::Symlink { .. } => {
                    fs::create_dir_all(parent)?;
                    Some(parent_identity(parent)?)
                }
            };
            let stage = sibling(&intent.destination, "stage", operation_id, index)?;
            let backup = sibling(&intent.destination, "backup", operation_id, index)?;
            if fs::symlink_metadata(&stage).is_ok() || fs::symlink_metadata(&backup).is_ok() {
                return Err(MutationError::StaleTarget);
            }
            current_stage = Some(stage.clone());
            let staged_path = match &intent.action {
                EntryAction::Keep | EntryAction::Remove => None,
                EntryAction::Materialize { payload_root } => {
                    materialize(payload_root, &stage)?;
                    Some(stage)
                }
                EntryAction::Symlink { target } => {
                    use std::os::unix::fs::symlink;
                    symlink(target, &stage)?;
                    Some(stage)
                }
            };
            let staged = StagedEntry {
                intent,
                parent_identity,
                stage: staged_path,
                backup,
                backup_created: false,
                installed: false,
            };
            verify_stage(&staged)?;
            Ok(staged)
        })();
        match staged {
            Ok(staged) => entries.push(staged),
            Err(error) => {
                if let Some(stage) = current_stage {
                    let _ = remove_no_follow(&stage);
                }
                cleanup_partial(&mut entries);
                return Err(error);
            }
        }
    }
    Ok(StagedMutation { entries })
}

#[cfg(not(target_os = "linux"))]
fn validate_intents_platform(
    _intents: &[EntryIntent],
    _is_cancelled: &impl Fn() -> bool,
) -> Result<(), MutationError> {
    Err(MutationError::UnsupportedPlatform)
}

#[cfg(target_os = "linux")]
fn validate_intents_platform(
    intents: &[EntryIntent],
    is_cancelled: &impl Fn() -> bool,
) -> Result<(), MutationError> {
    if intents.is_empty() {
        return Err(MutationError::InvalidRequest);
    }
    let mut destinations = BTreeSet::new();
    for intent in intents {
        check_cancelled(is_cancelled)?;
        if !intent.destination.is_absolute() || !destinations.insert(&intent.destination) {
            return Err(MutationError::InvalidRequest);
        }
        validate_intent_platform(intent)?;
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn validate_intent_platform(intent: &EntryIntent) -> Result<(), MutationError> {
    let projection = project_targets(&ProjectionRequest {
        destinations: vec![intent.destination.clone()],
    })
    .map_err(|_| MutationError::StaleTarget)?
    .targets
    .into_iter()
    .next()
    .ok_or(MutationError::StaleTarget)?;
    if projection.anchor_device != intent.expected_parent.device
        || projection.anchor_inode != intent.expected_parent.inode
        || projection.physical_destination != intent.destination
        || fingerprint_path(&intent.destination)? != intent.expected_fingerprint
    {
        return Err(MutationError::StaleTarget);
    }
    if let Some(expected) = &intent.expected_content_hash {
        if content_hash_path(&intent.destination).as_ref() != Ok(expected) {
            return Err(MutationError::StaleTarget);
        }
    }
    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn validate_intent_platform(_intent: &EntryIntent) -> Result<(), MutationError> {
    Err(MutationError::UnsupportedPlatform)
}

fn recheck_entry(entry: &StagedEntry) -> Result<(), MutationError> {
    if entry.parent_identity.is_none() {
        validate_intent_platform(&entry.intent)?;
        return verify_stage(entry);
    }
    let parent = entry
        .intent
        .destination
        .parent()
        .ok_or(MutationError::InvalidRequest)?;
    if Some(parent_identity(parent)?) != entry.parent_identity
        || fingerprint_path(&entry.intent.destination)? != entry.intent.expected_fingerprint
        || fs::symlink_metadata(&entry.backup).is_ok()
    {
        return Err(MutationError::StaleTarget);
    }
    if let Some(expected) = &entry.intent.expected_content_hash {
        if content_hash_path(&entry.intent.destination).as_ref() != Ok(expected) {
            return Err(MutationError::StaleTarget);
        }
    }
    verify_stage(entry)
}

fn verify_stage(entry: &StagedEntry) -> Result<(), MutationError> {
    match (&entry.intent.action, &entry.stage) {
        (EntryAction::Materialize { payload_root }, Some(stage)) => {
            verify_materialized(payload_root, stage)
        }
        (EntryAction::Symlink { target }, Some(stage))
            if fs::symlink_metadata(stage).is_ok_and(|value| value.file_type().is_symlink())
                && fs::read_link(stage).is_ok_and(|actual| &actual == target) =>
        {
            Ok(())
        }
        (EntryAction::Keep | EntryAction::Remove, None) => Ok(()),
        _ => Err(MutationError::VerificationFailed),
    }
}

fn swap_one(entry: &mut StagedEntry) -> Result<(), MutationError> {
    if matches!(entry.intent.action, EntryAction::Keep) {
        return Ok(());
    }
    if fingerprint_path(&entry.intent.destination)?.as_str() != "entry-v1-missing" {
        fs::rename(&entry.intent.destination, &entry.backup)?;
        entry.backup_created = true;
    }
    if let Some(stage) = entry.stage.take() {
        fs::rename(stage, &entry.intent.destination)?;
        entry.installed = true;
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn sibling(
    destination: &Path,
    kind: &str,
    operation_id: &str,
    index: usize,
) -> Result<PathBuf, MutationError> {
    let parent = destination.parent().ok_or(MutationError::InvalidRequest)?;
    Ok(parent.join(format!(".skill-deck-{kind}-{operation_id}-{index:06}")))
}

#[cfg(target_os = "linux")]
fn materialize(payload_root: &Path, destination: &Path) -> Result<(), MutationError> {
    let manifest = verify_payload(payload_root).map_err(|_| MutationError::InvalidPayload)?;
    fs::create_dir(destination)?;
    for entry in &manifest.entries {
        let path = destination.join(&entry.relative_path);
        match entry.kind {
            PayloadEntryKind::Directory => fs::create_dir_all(path)?,
            PayloadEntryKind::File => {
                if let Some(parent) = path.parent() {
                    fs::create_dir_all(parent)?;
                }
                let blob_id = entry
                    .blob_id
                    .as_deref()
                    .ok_or(MutationError::InvalidPayload)?;
                let mut input = read_blob(payload_root, blob_id)
                    .map_err(|_| MutationError::InvalidPayload)?
                    .ok_or(MutationError::InvalidPayload)?;
                let mut output = fs::File::create(&path)?;
                std::io::copy(&mut input, &mut output)?;
                set_executable(&path, entry.executable)?;
            }
        }
    }
    Ok(())
}

fn verify_materialized(payload_root: &Path, destination: &Path) -> Result<(), MutationError> {
    let manifest = verify_payload(payload_root).map_err(|_| MutationError::InvalidPayload)?;
    verify_manifest_tree(&manifest, payload_root, destination)
}

fn verify_manifest_tree(
    manifest: &PayloadManifest,
    payload_root: &Path,
    destination: &Path,
) -> Result<(), MutationError> {
    let expected = manifest
        .entries
        .iter()
        .map(|entry| (PathBuf::from(&entry.relative_path), entry))
        .collect::<std::collections::BTreeMap<_, _>>();
    let mut actual = BTreeSet::new();
    let mut pending = vec![destination.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for child in fs::read_dir(directory)? {
            let path = child?.path();
            let relative = path
                .strip_prefix(destination)
                .map_err(|_| MutationError::VerificationFailed)?
                .to_path_buf();
            let metadata = fs::symlink_metadata(&path)?;
            if metadata.file_type().is_symlink() || !actual.insert(relative.clone()) {
                return Err(MutationError::VerificationFailed);
            }
            let entry = expected
                .get(&relative)
                .ok_or(MutationError::VerificationFailed)?;
            match entry.kind {
                PayloadEntryKind::Directory if metadata.is_dir() => pending.push(path),
                PayloadEntryKind::File if metadata.is_file() => {
                    let blob_id = entry
                        .blob_id
                        .as_deref()
                        .ok_or(MutationError::InvalidPayload)?;
                    let blob = payload_root.join("blobs").join(blob_id);
                    if metadata.len() != entry.size
                        || digest_file(&path)? != blob_id
                        || digest_file(&blob)? != blob_id
                        || is_executable(&path)? != entry.executable
                    {
                        return Err(MutationError::VerificationFailed);
                    }
                }
                _ => return Err(MutationError::VerificationFailed),
            }
        }
    }
    if actual.len() == expected.len() {
        Ok(())
    } else {
        Err(MutationError::VerificationFailed)
    }
}

pub fn content_hash_path(root: &Path) -> Result<String, MutationError> {
    let response = build_manifest(&ManifestRequest {
        root: root.to_path_buf(),
    })
    .map_err(|_| MutationError::StaleTarget)?;
    aggregate_manifest_hash(&response)
}

fn aggregate_manifest_hash(manifest: &ManifestResponse) -> Result<String, MutationError> {
    let mut records = manifest
        .records
        .iter()
        .map(|record| {
            let relative = record
                .relative_path
                .as_os_str()
                .to_str()
                .ok_or(MutationError::StaleTarget)?
                .nfc()
                .collect::<String>();
            let target = record
                .symlink_target
                .as_ref()
                .map(|target| {
                    target
                        .as_os_str()
                        .to_str()
                        .ok_or(MutationError::StaleTarget)
                        .map(|target| target.nfc().collect::<String>())
                })
                .transpose()?;
            Ok((relative, target, record))
        })
        .collect::<Result<Vec<_>, MutationError>>()?;
    records.sort_by(|left, right| left.0.cmp(&right.0));
    if records.windows(2).any(|pair| pair[0].0 == pair[1].0) {
        return Err(MutationError::StaleTarget);
    }
    let mut hasher = Sha256::new();
    hasher.update(b"skill-deck-content-manifest");
    hasher.update([1]);
    for (relative, target, record) in records {
        let tag = match record.kind {
            ManifestKind::Directory => b'd',
            ManifestKind::File => b'f',
            ManifestKind::Symlink => b'l',
        };
        hasher.update([tag, u8::from(record.executable)]);
        hash_field(&mut hasher, relative.as_bytes());
        hash_field(
            &mut hasher,
            record.digest.as_deref().unwrap_or("").as_bytes(),
        );
        hash_field(&mut hasher, target.as_deref().unwrap_or("").as_bytes());
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn hash_field(hasher: &mut Sha256, value: &[u8]) {
    hasher.update((value.len() as u64).to_be_bytes());
    hasher.update(value);
}

fn digest_file(path: &Path) -> Result<String, MutationError> {
    let mut file = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            return Ok(format!("{:x}", hasher.finalize()));
        }
        hasher.update(&buffer[..read]);
    }
}

#[cfg(target_os = "linux")]
fn cleanup_partial(entries: &mut Vec<StagedEntry>) {
    while let Some(entry) = entries.pop() {
        if let Some(stage) = entry.stage {
            let _ = remove_no_follow(&stage);
        }
    }
}

fn remove_no_follow(path: &Path) -> Result<(), MutationError> {
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

fn check_cancelled(is_cancelled: &impl Fn() -> bool) -> Result<(), MutationError> {
    if is_cancelled() {
        Err(MutationError::Cancelled)
    } else {
        Ok(())
    }
}

#[cfg(target_os = "linux")]
fn set_executable(path: &Path, executable: bool) -> Result<(), MutationError> {
    use std::os::unix::fs::PermissionsExt;
    let mut permissions = fs::metadata(path)?.permissions();
    let mut mode = permissions.mode();
    if executable {
        mode |= 0o111;
    } else {
        mode &= !0o111;
    }
    permissions.set_mode(mode);
    fs::set_permissions(path, permissions)?;
    Ok(())
}

#[cfg(unix)]
fn is_executable(path: &Path) -> Result<bool, MutationError> {
    use std::os::unix::fs::PermissionsExt;
    Ok(fs::metadata(path)?.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn is_executable(_path: &Path) -> Result<bool, MutationError> {
    Err(MutationError::UnsupportedPlatform)
}

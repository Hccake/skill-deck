use std::fmt;
use std::path::{Path, PathBuf};

use crate::linux_mutation::ParentIdentity;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogSnapshot {
    pub bytes: Option<Vec<u8>>,
    pub revision: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogWrite {
    pub expected_revision: Option<String>,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetExpectation {
    pub parent: ParentIdentity,
    pub fingerprint: String,
    pub content_hash: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContentAction {
    Upsert { payload_root: PathBuf },
    Delete,
    DeleteIfPresent,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibraryCommit {
    pub root: PathBuf,
    pub operation_id: String,
    pub destination: PathBuf,
    pub expected_target: TargetExpectation,
    pub content: ContentAction,
    pub catalog: CatalogWrite,
}

#[derive(Debug)]
pub enum LibraryError {
    UnsupportedPlatform,
    InvalidRequest,
    StaleTarget,
    InvalidPayload,
    RecoveryIncomplete,
    Io(std::io::Error),
}

impl fmt::Display for LibraryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for LibraryError {}

impl From<std::io::Error> for LibraryError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

pub fn read_catalog(root: &Path) -> Result<CatalogSnapshot, LibraryError> {
    read_catalog_platform(root)
}

pub fn commit(request: LibraryCommit) -> Result<(), LibraryError> {
    commit_platform(request)
}

pub fn write_catalog(
    root: &Path,
    library_ids: &[String],
    catalog: CatalogWrite,
) -> Result<String, LibraryError> {
    write_catalog_platform(root, library_ids, catalog)
}

#[cfg(not(target_os = "linux"))]
fn read_catalog_platform(_root: &Path) -> Result<CatalogSnapshot, LibraryError> {
    Err(LibraryError::UnsupportedPlatform)
}

#[cfg(not(target_os = "linux"))]
fn commit_platform(_request: LibraryCommit) -> Result<(), LibraryError> {
    Err(LibraryError::UnsupportedPlatform)
}

#[cfg(not(target_os = "linux"))]
fn write_catalog_platform(
    _root: &Path,
    _library_ids: &[String],
    _catalog: CatalogWrite,
) -> Result<String, LibraryError> {
    Err(LibraryError::UnsupportedPlatform)
}

#[cfg(target_os = "linux")]
fn read_catalog_platform(root: &Path) -> Result<CatalogSnapshot, LibraryError> {
    let catalog = read_optional(&root.join("catalog.json"))?;
    let current_revision = catalog.as_deref().map(revision);
    recover(root, current_revision.as_deref())?;
    let bytes = read_optional(&root.join("catalog.json"))?;
    Ok(CatalogSnapshot {
        revision: bytes.as_deref().map(revision),
        bytes,
    })
}

#[cfg(target_os = "linux")]
fn commit_platform(request: LibraryCommit) -> Result<(), LibraryError> {
    validate_request(&request)?;
    let current = read_catalog_platform(&request.root)?;
    if current.revision != request.catalog.expected_revision {
        return Err(LibraryError::StaleTarget);
    }
    validate_target(&request.destination, &request.expected_target)?;

    let transaction = request
        .root
        .join(".transactions")
        .join(&request.operation_id);
    if std::fs::symlink_metadata(&transaction).is_ok() {
        return Err(LibraryError::StaleTarget);
    }
    std::fs::create_dir_all(&transaction)?;
    let stage = transaction.join("stage");
    let backup = transaction.join("backup");
    write_transaction(
        &transaction,
        &request.destination,
        matches!(request.content, ContentAction::Upsert { .. }),
        "preparing",
        None,
    )?;

    let new_revision = revision(&request.catalog.bytes);
    let result = (|| {
        match &request.content {
            ContentAction::Upsert { payload_root } => {
                materialize_payload(payload_root, &stage)?;
                verify_materialized(payload_root, &stage)?;
                write_state(&transaction.join("phase"), "staged")?;
            }
            ContentAction::Delete => {
                if !request.destination.is_dir() {
                    return Err(LibraryError::StaleTarget);
                }
            }
            ContentAction::DeleteIfPresent => {
                if std::fs::symlink_metadata(&request.destination)
                    .is_ok_and(|metadata| !metadata.is_dir() || metadata.file_type().is_symlink())
                {
                    return Err(LibraryError::StaleTarget);
                }
            }
        }
        validate_target(&request.destination, &request.expected_target)?;
        if std::fs::symlink_metadata(&request.destination).is_ok() {
            write_state(&transaction.join("phase"), "backedUp")?;
            std::fs::rename(&request.destination, &backup)?;
        }
        write_state(&transaction.join("phase"), "activated")?;
        if matches!(request.content, ContentAction::Upsert { .. }) {
            std::fs::create_dir_all(
                request
                    .destination
                    .parent()
                    .ok_or(LibraryError::InvalidRequest)?,
            )?;
            std::fs::rename(&stage, &request.destination)?;
            if let ContentAction::Upsert { payload_root } = &request.content {
                verify_materialized(payload_root, &request.destination)?;
            }
        }
        write_state(&transaction.join("expected-catalog-hash"), &new_revision)?;
        write_state(&transaction.join("phase"), "catalogPrepared")?;
        write_catalog_document(
            &request.root.join("catalog.json"),
            request.catalog.expected_revision.as_deref(),
            &request.catalog.bytes,
        )?;
        write_state(&transaction.join("phase"), "catalogCommitted")?;
        remove_any(&backup)?;
        std::fs::remove_dir_all(&transaction)?;
        Ok(())
    })();
    if let Err(error) = result {
        let current = read_optional(&request.root.join("catalog.json"))?;
        let current_revision = current.as_deref().map(revision);
        recover(&request.root, current_revision.as_deref())?;
        if current_revision.as_deref() == Some(new_revision.as_str()) {
            return Ok(());
        }
        return Err(error);
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn validate_target(destination: &Path, expected: &TargetExpectation) -> Result<(), LibraryError> {
    let target = crate::projection::project_targets(&crate::projection::ProjectionRequest {
        destinations: vec![destination.to_path_buf()],
    })
    .map_err(|_| LibraryError::StaleTarget)?
    .targets
    .pop()
    .ok_or(LibraryError::StaleTarget)?;
    if target.anchor_device != expected.parent.device
        || target.anchor_inode != expected.parent.inode
        || crate::linux_mutation::fingerprint_path(&target.physical_destination).as_deref()
            != Ok(expected.fingerprint.as_str())
    {
        return Err(LibraryError::StaleTarget);
    }
    if let Some(expected_hash) = &expected.content_hash {
        if crate::linux_mutation::content_hash_path(&target.physical_destination).as_deref()
            != Ok(expected_hash.as_str())
        {
            return Err(LibraryError::StaleTarget);
        }
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn write_catalog_platform(
    root: &Path,
    library_ids: &[String],
    catalog: CatalogWrite,
) -> Result<String, LibraryError> {
    if !root.is_absolute()
        || catalog.bytes.is_empty()
        || library_ids.iter().any(|id| !valid_component(id))
    {
        return Err(LibraryError::InvalidRequest);
    }
    let current = read_catalog_platform(root)?;
    if current.revision != catalog.expected_revision {
        return Err(LibraryError::StaleTarget);
    }
    let mut created = Vec::new();
    for library_id in library_ids {
        let library = root.join("libraries").join(library_id);
        if std::fs::symlink_metadata(&library).is_err() {
            std::fs::create_dir_all(library.join("skills"))?;
            created.push(library);
        }
    }
    let result = write_catalog_document(
        &root.join("catalog.json"),
        catalog.expected_revision.as_deref(),
        &catalog.bytes,
    );
    if result.is_err() {
        for library in created {
            let _ = std::fs::remove_dir_all(library);
        }
    }
    result?;
    Ok(revision(&catalog.bytes))
}

#[cfg(target_os = "linux")]
fn validate_request(request: &LibraryCommit) -> Result<(), LibraryError> {
    let managed = request.root.join("libraries");
    if !request.root.is_absolute()
        || !request.destination.starts_with(&managed)
        || request.operation_id.is_empty()
        || request.operation_id.contains(['/', '\\', '\0'])
        || request.catalog.bytes.is_empty()
    {
        return Err(LibraryError::InvalidRequest);
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn valid_component(value: &str) -> bool {
    !value.is_empty() && !matches!(value, "." | "..") && !value.contains(['/', '\\', '\0'])
}

#[cfg(target_os = "linux")]
fn write_transaction(
    transaction: &Path,
    destination: &Path,
    desired_presence: bool,
    phase: &str,
    expected_catalog_hash: Option<&str>,
) -> Result<(), LibraryError> {
    write_state(
        &transaction.join("destination"),
        destination.to_str().ok_or(LibraryError::InvalidRequest)?,
    )?;
    write_state(
        &transaction.join("desired-presence"),
        if desired_presence { "1" } else { "0" },
    )?;
    if let Some(expected) = expected_catalog_hash {
        write_state(&transaction.join("expected-catalog-hash"), expected)?;
    }
    write_state(&transaction.join("phase"), phase)
}

#[cfg(target_os = "linux")]
fn write_state(path: &Path, value: &str) -> Result<(), LibraryError> {
    use std::io::Write;
    use std::os::unix::fs::PermissionsExt;

    let parent = path.parent().ok_or(LibraryError::InvalidRequest)?;
    std::fs::create_dir_all(parent)?;
    let temporary = path.with_extension(format!("tmp.{}", std::process::id()));
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)?;
    std::fs::set_permissions(&temporary, std::fs::Permissions::from_mode(0o600))?;
    let result = (|| {
        file.write_all(value.as_bytes())?;
        file.sync_all()?;
        std::fs::rename(&temporary, path)?;
        std::fs::File::open(parent)?.sync_all()?;
        Ok::<_, std::io::Error>(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result.map_err(LibraryError::from)
}

#[cfg(target_os = "linux")]
fn write_catalog_document(
    path: &Path,
    expected_revision: Option<&str>,
    bytes: &[u8],
) -> Result<(), LibraryError> {
    crate::document::write_document_atomic(path, expected_revision, bytes)
        .map(|_| ())
        .map_err(|error| match error {
            crate::document::DocumentWriteError::Conflict => LibraryError::StaleTarget,
            crate::document::DocumentWriteError::InvalidTarget => LibraryError::InvalidRequest,
            crate::document::DocumentWriteError::UnsupportedPlatform => {
                LibraryError::UnsupportedPlatform
            }
            crate::document::DocumentWriteError::Io => {
                LibraryError::Io(std::io::Error::other("failed to write Library catalog"))
            }
        })
}

#[cfg(target_os = "linux")]
fn materialize_payload(payload_root: &Path, destination: &Path) -> Result<(), LibraryError> {
    use std::io::Write;
    use std::os::unix::fs::PermissionsExt;

    let manifest =
        crate::payload::verify_payload(payload_root).map_err(|_| LibraryError::InvalidPayload)?;
    std::fs::create_dir(destination)?;
    for entry in manifest.entries {
        let target = destination.join(&entry.relative_path);
        match entry.kind {
            crate::payload::PayloadEntryKind::Directory => std::fs::create_dir_all(&target)?,
            crate::payload::PayloadEntryKind::File => {
                std::fs::create_dir_all(target.parent().ok_or(LibraryError::InvalidPayload)?)?;
                let blob_id = entry.blob_id.ok_or(LibraryError::InvalidPayload)?;
                let mut input = crate::payload::read_blob(payload_root, &blob_id)
                    .map_err(|_| LibraryError::InvalidPayload)?
                    .ok_or(LibraryError::InvalidPayload)?;
                let mut output = std::fs::File::create(&target)?;
                std::io::copy(&mut input, &mut output)?;
                output.flush()?;
                std::fs::set_permissions(
                    &target,
                    std::fs::Permissions::from_mode(if entry.executable { 0o755 } else { 0o644 }),
                )?;
            }
        }
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn verify_materialized(payload_root: &Path, destination: &Path) -> Result<(), LibraryError> {
    let manifest =
        crate::payload::verify_payload(payload_root).map_err(|_| LibraryError::InvalidPayload)?;
    for entry in manifest.entries {
        let target = destination.join(&entry.relative_path);
        let valid = match entry.kind {
            crate::payload::PayloadEntryKind::Directory => target.is_dir(),
            crate::payload::PayloadEntryKind::File => {
                target.is_file()
                    && entry.content_hash.as_deref().is_some_and(|expected| {
                        std::fs::read(&target)
                            .ok()
                            .is_some_and(|bytes| revision_raw(&bytes) == expected)
                    })
            }
        };
        if !valid {
            return Err(LibraryError::InvalidPayload);
        }
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn recover(root: &Path, catalog_revision: Option<&str>) -> Result<(), LibraryError> {
    let transactions = root.join(".transactions");
    let Ok(entries) = std::fs::read_dir(&transactions) else {
        return Ok(());
    };
    for entry in entries {
        let transaction = entry?.path();
        if !transaction.is_dir() {
            continue;
        }
        let destination = read_required_text(&transaction.join("destination"))?;
        let destination = PathBuf::from(destination);
        if !destination.starts_with(root.join("libraries")) {
            return Err(LibraryError::RecoveryIncomplete);
        }
        let phase = read_required_text(&transaction.join("phase"))?;
        let desired_presence =
            match read_required_text(&transaction.join("desired-presence"))?.as_str() {
                "1" => true,
                "0" => false,
                _ => return Err(LibraryError::RecoveryIncomplete),
            };
        let stage = transaction.join("stage");
        let backup = transaction.join("backup");
        match phase.as_str() {
            "preparing" | "staged" => {}
            "backedUp" if !destination.exists() && backup.is_dir() && stage.exists() => {
                std::fs::rename(&backup, &destination)?;
            }
            "backedUp" if destination.exists() && !backup.exists() => {}
            "backedUp" if destination.exists() && backup.is_dir() && !stage.exists() => {}
            "activated" => rollback(&destination, &backup)?,
            "catalogPrepared" if destination.exists() == desired_presence => {
                let expected = read_required_text(&transaction.join("expected-catalog-hash"))?;
                if !same_revision(catalog_revision, &expected) {
                    rollback(&destination, &backup)?;
                }
            }
            "catalogCommitted" if destination.exists() == desired_presence => {}
            _ => return Err(LibraryError::RecoveryIncomplete),
        }
        remove_any(&stage)?;
        remove_any(&backup)?;
        std::fs::remove_dir_all(&transaction)?;
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn rollback(destination: &Path, backup: &Path) -> Result<(), LibraryError> {
    remove_any(destination)?;
    if backup.exists() {
        std::fs::rename(backup, destination)?;
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn remove_any(path: &Path) -> Result<(), LibraryError> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {
            std::fs::remove_dir_all(path)?
        }
        Ok(_) => std::fs::remove_file(path)?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn read_required_text(path: &Path) -> Result<String, LibraryError> {
    std::fs::read_to_string(path).map_err(|_| LibraryError::RecoveryIncomplete)
}

#[cfg(target_os = "linux")]
fn read_optional(path: &Path) -> Result<Option<Vec<u8>>, LibraryError> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

#[cfg(target_os = "linux")]
fn revision(bytes: &[u8]) -> String {
    format!("sha256:{}", revision_raw(bytes))
}

#[cfg(target_os = "linux")]
fn revision_raw(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(bytes))
}

#[cfg(target_os = "linux")]
fn same_revision(actual: Option<&str>, expected: &str) -> bool {
    actual.map(|value| value.strip_prefix("sha256:").unwrap_or(value))
        == Some(expected.strip_prefix("sha256:").unwrap_or(expected))
}

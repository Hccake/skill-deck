//! Small synchronous document-publication mechanics.
//!
//! The caller owns path authorization, schema validation, parent creation and
//! read/modify/write serialization. This module never deletes a `.bak`, follows
//! a recovery policy, or treats an I/O error after publication as a rollback.
//! Snapshot comparison is optimistic: it is not a lock for cooperating or
//! uncooperative external processes. Directory transactions remain separate.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static NEXT_TEMPORARY: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PublicationState {
    NotPublished,
    PublishedUnconfirmed,
    OutcomeUnknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WritePhase {
    Preparing,
    Writing,
    BeforePublish,
    Publishing,
    Confirming,
}

#[derive(Debug)]
pub struct AtomicWriteError {
    pub publication: PublicationState,
    pub phase: WritePhase,
    source: io::Error,
    conflict: bool,
}

impl AtomicWriteError {
    pub fn is_conflict(&self) -> bool {
        self.conflict
    }

    fn io(phase: WritePhase, publication: PublicationState, source: io::Error) -> Self {
        Self {
            publication,
            phase,
            source,
            conflict: false,
        }
    }

    fn conflict() -> Self {
        Self {
            publication: PublicationState::NotPublished,
            phase: WritePhase::BeforePublish,
            source: io::Error::other("document changed since its snapshot was read"),
            conflict: true,
        }
    }
}

impl std::fmt::Display for AtomicWriteError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "document {:?} failed ({:?}): {}",
            self.phase, self.publication, self.source
        )
    }
}

impl std::error::Error for AtomicWriteError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.source)
    }
}

#[derive(Clone, Copy)]
enum Expectation<'a> {
    Any,
    Snapshot(Option<&'a [u8]>),
}

/// Publish complete bytes in an existing, authorized parent directory.
/// A successful return includes file sync and, on Unix, parent-directory sync.
/// Other platforms retain their existing directory-sync limitations.
pub fn replace(path: &Path, bytes: &[u8]) -> Result<(), AtomicWriteError> {
    replace_with_hook(path, Expectation::Any, bytes, |_| Ok(()))
}

/// The expected bytes are the bytes used to compute this update, not a new
/// snapshot acquired inside a save wrapper. `None` means expected missing.
pub fn replace_if_unchanged(
    path: &Path,
    expected: Option<&[u8]>,
    bytes: &[u8],
) -> Result<(), AtomicWriteError> {
    replace_with_hook(path, Expectation::Snapshot(expected), bytes, |_| Ok(()))
}

/// Remove a document only while it still matches the observed bytes. A
/// successful return includes parent-directory sync where the platform
/// supports it. `None` means the caller observed the document as missing.
pub fn remove_if_unchanged(path: &Path, expected: Option<&[u8]>) -> Result<(), AtomicWriteError> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .filter(|_| path.file_name().is_some())
        .ok_or_else(|| {
            AtomicWriteError::io(
                WritePhase::Preparing,
                PublicationState::NotPublished,
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "document needs an explicit parent and file name",
                ),
            )
        })?;
    check_expected(path, Expectation::Snapshot(expected))?;
    if expected.is_none() {
        return Ok(());
    }
    check_expected(path, Expectation::Snapshot(expected))?;
    fs::remove_file(path).map_err(|error| {
        AtomicWriteError::io(
            WritePhase::Publishing,
            PublicationState::OutcomeUnknown,
            error,
        )
    })?;
    sync_directory(parent).map_err(|error| {
        AtomicWriteError::io(
            WritePhase::Confirming,
            PublicationState::PublishedUnconfirmed,
            error,
        )
    })
}

fn replace_with_hook(
    path: &Path,
    expected: Expectation<'_>,
    bytes: &[u8],
    mut hook: impl FnMut(WritePhase) -> io::Result<()>,
) -> Result<(), AtomicWriteError> {
    let before = |phase, error| AtomicWriteError::io(phase, PublicationState::NotPublished, error);
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .filter(|_| path.file_name().is_some())
        .ok_or_else(|| {
            before(
                WritePhase::Preparing,
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "document needs an explicit parent and file name",
                ),
            )
        })?;
    check_expected(path, expected)?;
    let mut temporary =
        TemporaryDocument::create(parent).map_err(|error| before(WritePhase::Preparing, error))?;
    hook(WritePhase::Writing).map_err(|error| before(WritePhase::Writing, error))?;
    let file = temporary
        .file
        .as_mut()
        .expect("new temporary has its file handle");
    file.write_all(bytes)
        .map_err(|error| before(WritePhase::Writing, error))?;
    file.sync_all()
        .map_err(|error| before(WritePhase::Writing, error))?;
    // Closing before rename also works on platforms with restrictive sharing.
    drop(temporary.file.take());
    hook(WritePhase::BeforePublish).map_err(|error| before(WritePhase::BeforePublish, error))?;
    check_expected(path, expected)?;
    let temporary_path = temporary
        .path
        .as_ref()
        .expect("unpublished temporary has a path");
    fs::rename(temporary_path, path).map_err(|error| {
        AtomicWriteError::io(
            WritePhase::Publishing,
            // A failed namespace operation on some storage may have taken effect.
            // Do not promise rollback solely from its error return.
            PublicationState::OutcomeUnknown,
            error,
        )
    })?;
    temporary.path = None;
    hook(WritePhase::Confirming)
        .and_then(|()| sync_directory(parent))
        .map_err(|error| {
            AtomicWriteError::io(
                WritePhase::Confirming,
                PublicationState::PublishedUnconfirmed,
                error,
            )
        })
}

fn check_expected(path: &Path, expected: Expectation<'_>) -> Result<(), AtomicWriteError> {
    let Expectation::Snapshot(expected) = expected else {
        return Ok(());
    };
    let limit = expected.map_or(0, <[u8]>::len);
    match read_optional_bounded(path, limit) {
        Ok(current) if current.as_deref() == expected => Ok(()),
        Ok(_) => Err(AtomicWriteError::conflict()),
        // The observed file grew beyond the old snapshot, so it cannot match.
        Err(error) if error.kind() == io::ErrorKind::InvalidData => {
            Err(AtomicWriteError::conflict())
        }
        Err(error) => Err(AtomicWriteError::io(
            WritePhase::BeforePublish,
            PublicationState::NotPublished,
            error,
        )),
    }
}

/// No truncated bytes are returned as a valid document snapshot. The caller
/// chooses a resource-specific limit; zero permits only an empty file.
pub fn read_optional_bounded(path: &Path, limit: usize) -> io::Result<Option<Vec<u8>>> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    if !file.metadata()?.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "document is not a regular file",
        ));
    }
    let bound = u64::try_from(limit).unwrap_or(u64::MAX).saturating_add(1);
    let mut bytes = Vec::new();
    file.take(bound).read_to_end(&mut bytes)?;
    if bytes.len() > limit {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "document exceeds its read limit",
        ));
    }
    Ok(Some(bytes))
}

#[cfg(unix)]
pub fn sync_directory(parent: &Path) -> io::Result<()> {
    File::open(parent)?.sync_all()
}

#[cfg(not(unix))]
pub fn sync_directory(_parent: &Path) -> io::Result<()> {
    // Do not claim Unix directory durability on Windows or other platforms.
    Ok(())
}

struct TemporaryDocument {
    path: Option<PathBuf>,
    file: Option<File>,
}

impl TemporaryDocument {
    fn create(parent: &Path) -> io::Result<Self> {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(io::Error::other)?
            .as_nanos();
        for _ in 0..64 {
            let sequence = NEXT_TEMPORARY.fetch_add(1, Ordering::Relaxed);
            let path = parent.join(format!(
                ".skill-deck-document-{}-{nonce:x}-{sequence:x}",
                std::process::id(),
            ));
            let mut options = OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            match options.open(&path) {
                Ok(file) => {
                    return Ok(Self {
                        path: Some(path),
                        file: Some(file),
                    })
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error),
            }
        }
        Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "cannot allocate a unique document temporary",
        ))
    }
}

impl Drop for TemporaryDocument {
    fn drop(&mut self) {
        drop(self.file.take());
        if let Some(path) = &self.path {
            let _ = fs::remove_file(path);
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn complete_document_replacement_preserves_unrelated_backup() {
        let root = tempdir().unwrap();
        let target = root.path().join("document.json");
        let backup = root.path().join("document.json.bak");
        fs::write(&target, b"old").unwrap();
        fs::write(&backup, b"last-known-good").unwrap();
        replace(&target, b"new").unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"new");
        assert_eq!(fs::read(&backup).unwrap(), b"last-known-good");
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 2);
    }

    #[test]
    fn prepublication_failure_keeps_old_bytes_and_removes_only_own_temporary() {
        for phase in [WritePhase::Writing, WritePhase::BeforePublish] {
            let root = tempdir().unwrap();
            let target = root.path().join("document.json");
            fs::write(&target, b"old").unwrap();
            let error = replace_with_hook(&target, Expectation::Any, b"new", |at| {
                if at == phase {
                    Err(io::Error::other("injected"))
                } else {
                    Ok(())
                }
            })
            .unwrap_err();
            assert_eq!(error.publication, PublicationState::NotPublished);
            assert_eq!(fs::read(&target).unwrap(), b"old");
            assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
        }
    }

    #[test]
    fn failure_after_publish_is_not_reported_as_unchanged() {
        let root = tempdir().unwrap();
        let target = root.path().join("document.json");
        fs::write(&target, b"old").unwrap();
        let error = replace_with_hook(&target, Expectation::Any, b"new", |phase| {
            if phase == WritePhase::Confirming {
                Err(io::Error::other("injected directory sync failure"))
            } else {
                Ok(())
            }
        })
        .unwrap_err();
        assert_eq!(error.publication, PublicationState::PublishedUnconfirmed);
        assert_eq!(error.phase, WritePhase::Confirming);
        assert_eq!(fs::read(&target).unwrap(), b"new");
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
    }

    #[test]
    fn conditional_replace_rechecks_the_original_bytes_at_publish() {
        let root = tempdir().unwrap();
        let target = root.path().join("document.json");
        fs::write(&target, b"old").unwrap();
        let error = replace_with_hook(
            &target,
            Expectation::Snapshot(Some(b"old")),
            b"new",
            |phase| {
                if phase == WritePhase::BeforePublish {
                    fs::write(&target, b"external")?;
                }
                Ok(())
            },
        )
        .unwrap_err();
        assert!(error.is_conflict());
        assert_eq!(error.publication, PublicationState::NotPublished);
        assert_eq!(fs::read(&target).unwrap(), b"external");
    }

    #[test]
    fn missing_and_empty_are_different_snapshots() {
        let root = tempdir().unwrap();
        let target = root.path().join("document.json");
        assert_eq!(read_optional_bounded(&target, 0).unwrap(), None);
        replace_if_unchanged(&target, None, b"").unwrap();
        assert_eq!(read_optional_bounded(&target, 0).unwrap(), Some(Vec::new()));
        assert!(replace_if_unchanged(&target, None, b"unexpected")
            .unwrap_err()
            .is_conflict());
        replace_if_unchanged(&target, Some(b""), b"next").unwrap();
    }

    #[test]
    fn conditional_remove_preserves_a_document_changed_after_observation() {
        let root = tempdir().unwrap();
        let target = root.path().join("document.json");
        fs::write(&target, b"external").unwrap();

        let error = remove_if_unchanged(&target, Some(b"observed")).unwrap_err();

        assert!(error.is_conflict());
        assert_eq!(error.publication, PublicationState::NotPublished);
        assert_eq!(fs::read(&target).unwrap(), b"external");
    }

    #[test]
    fn conditional_remove_distinguishes_missing_from_present() {
        let root = tempdir().unwrap();
        let target = root.path().join("document.json");
        remove_if_unchanged(&target, None).unwrap();
        fs::write(&target, b"created-later").unwrap();

        assert!(remove_if_unchanged(&target, None)
            .unwrap_err()
            .is_conflict());
        remove_if_unchanged(&target, Some(b"created-later")).unwrap();
        assert!(!target.exists());
    }

    #[test]
    fn bounded_read_rejects_oversize_instead_of_returning_truncated_snapshot() {
        let root = tempdir().unwrap();
        let target = root.path().join("document.json");
        fs::write(&target, b"12345").unwrap();
        assert_eq!(
            read_optional_bounded(&target, 5).unwrap(),
            Some(b"12345".to_vec())
        );
        assert_eq!(
            read_optional_bounded(&target, 4).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        assert!(read_optional_bounded(root.path(), 100).is_err());
    }

    #[test]
    fn primitive_does_not_create_missing_parent_directories() {
        let root = tempdir().unwrap();
        let parent = root.path().join("not-created");
        let error = replace(&parent.join("document.json"), b"new").unwrap_err();
        assert_eq!(error.publication, PublicationState::NotPublished);
        assert!(!parent.exists());
    }

    #[cfg(unix)]
    #[test]
    fn private_temporary_does_not_publish_world_readable_data() {
        use std::os::unix::fs::PermissionsExt;
        let root = tempdir().unwrap();
        let target = root.path().join("document.json");
        replace(&target, b"private").unwrap();
        assert_eq!(
            fs::metadata(target).unwrap().permissions().mode() & 0o077,
            0
        );
    }
}

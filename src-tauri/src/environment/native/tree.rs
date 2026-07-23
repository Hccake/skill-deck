use std::fs;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::environment::runtime::{
    projected_physical_target_key, EntryFingerprint, ExecutionBackend, PhysicalParentIdentity,
    PhysicalTargetKey,
};
use crate::error::AppError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeEntryKind {
    Missing,
    File,
    Directory,
    Symlink,
    #[cfg_attr(
        not(target_os = "windows"),
        expect(dead_code, reason = "Windows reparse points are classified on Windows")
    )]
    ReparsePoint,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeEntryInspection {
    pub kind: NativeEntryKind,
    pub fingerprint: EntryFingerprint,
    pub link_target: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeTargetProjection {
    pub key: PhysicalTargetKey,
    pub physical_destination: PathBuf,
    pub fingerprint: EntryFingerprint,
}

pub fn project_target(
    logical_destination: &Path,
    backend: ExecutionBackend,
) -> Result<NativeTargetProjection, AppError> {
    if !logical_destination.is_absolute()
        || !matches!(
            backend,
            ExecutionBackend::NativeWindows | ExecutionBackend::NativeUnix
        )
    {
        return Err(unsafe_projection(logical_destination));
    }
    let mut current = logical_destination
        .parent()
        .ok_or_else(|| unsafe_projection(logical_destination))?
        .to_path_buf();
    let mut relative = vec![component_name(logical_destination, logical_destination)?];
    loop {
        match fs::symlink_metadata(&current) {
            Ok(_) => break,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                relative.push(component_name(&current, logical_destination)?);
                current = current
                    .parent()
                    .ok_or_else(|| unsafe_projection(logical_destination))?
                    .to_path_buf();
            }
            Err(error) => return Err(error.into()),
        }
    }
    let physical_ancestor = fs::canonicalize(&current)?;
    if !physical_ancestor.is_dir() {
        return Err(unsafe_projection(logical_destination));
    }
    relative.reverse();
    let physical_destination = relative
        .iter()
        .fold(physical_ancestor.clone(), |path, component| {
            path.join(component)
        });
    let key = projected_physical_target_key(
        backend.clone(),
        physical_parent_identity(&physical_ancestor)?,
        relative.iter().map(String::as_str),
        !matches!(backend, ExecutionBackend::NativeWindows),
    )?;
    let fingerprint = inspect_entry_no_follow(&physical_destination)?.fingerprint;
    Ok(NativeTargetProjection {
        key,
        physical_destination,
        fingerprint,
    })
}

fn component_name(path: &Path, destination: &Path) -> Result<String, AppError> {
    path.file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty() && !matches!(*name, "." | ".."))
        .map(str::to_string)
        .ok_or_else(|| unsafe_projection(destination))
}

fn unsafe_projection(path: &Path) -> AppError {
    AppError::UnsafePath {
        path: path.to_string_lossy().into_owned(),
        reason: "target has no safe existing ancestor".to_string(),
    }
}

pub fn inspect_entry_no_follow(path: &Path) -> Result<NativeEntryInspection, AppError> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(NativeEntryInspection {
                kind: NativeEntryKind::Missing,
                fingerprint: EntryFingerprint("entry-v1-missing".to_string()),
                link_target: None,
            });
        }
        Err(error) => return Err(error.into()),
    };
    let kind = classify_metadata(&metadata);
    let link_target = matches!(
        kind,
        NativeEntryKind::Symlink | NativeEntryKind::ReparsePoint
    )
    .then(|| fs::read_link(path).ok())
    .flatten();
    let fingerprint = metadata_fingerprint(path, &metadata, link_target.as_deref())?;
    Ok(NativeEntryInspection {
        kind,
        fingerprint,
        link_target,
    })
}

pub fn physical_parent_identity(path: &Path) -> Result<PhysicalParentIdentity, AppError> {
    let physical = fs::canonicalize(path)?;
    let metadata = fs::metadata(&physical)?;
    if !metadata.is_dir() {
        return Err(AppError::UnsafePath {
            path: path.to_string_lossy().into_owned(),
            reason: "physical parent is not a directory".to_string(),
        });
    }
    platform_parent_identity(&metadata, path)
}

pub fn remove_entry_no_follow(path: &Path) -> Result<(), AppError> {
    match inspect_entry_no_follow(path)?.kind {
        NativeEntryKind::Missing => Ok(()),
        NativeEntryKind::Directory => fs::remove_dir_all(path).map_err(Into::into),
        NativeEntryKind::ReparsePoint => fs::remove_dir(path)
            .or_else(|_| fs::remove_file(path))
            .map_err(Into::into),
        NativeEntryKind::File | NativeEntryKind::Symlink | NativeEntryKind::Other => {
            fs::remove_file(path).map_err(Into::into)
        }
    }
}

fn classify_metadata(metadata: &fs::Metadata) -> NativeEntryKind {
    if metadata.file_type().is_symlink() {
        return NativeEntryKind::Symlink;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
        if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return NativeEntryKind::ReparsePoint;
        }
    }
    if metadata.is_dir() {
        NativeEntryKind::Directory
    } else if metadata.is_file() {
        NativeEntryKind::File
    } else {
        NativeEntryKind::Other
    }
}

fn metadata_fingerprint(
    _path: &Path,
    metadata: &fs::Metadata,
    link_target: Option<&Path>,
) -> Result<EntryFingerprint, AppError> {
    let mut hasher = Sha256::new();
    hasher.update(b"skill-deck-native-entry-v1\0");
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        hasher.update(metadata.dev().to_le_bytes());
        hasher.update(metadata.ino().to_le_bytes());
        hasher.update(metadata.mode().to_le_bytes());
        hasher.update(metadata.size().to_le_bytes());
        hasher.update(metadata.mtime().to_le_bytes());
        hasher.update(metadata.mtime_nsec().to_le_bytes());
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        let (volume_serial, file_id) = windows_file_identity(_path, true)?;
        hasher.update(metadata.file_attributes().to_le_bytes());
        hasher.update(metadata.file_size().to_le_bytes());
        hasher.update(metadata.last_write_time().to_le_bytes());
        hasher.update(volume_serial.to_le_bytes());
        hasher.update(file_id.to_le_bytes());
    }
    #[cfg(not(any(unix, windows)))]
    {
        hasher.update(metadata.len().to_le_bytes());
    }
    if let Some(target) = link_target {
        hasher.update(target.to_string_lossy().as_bytes());
    }
    Ok(EntryFingerprint(format!(
        "entry-v1-{:x}",
        hasher.finalize()
    )))
}

#[cfg(unix)]
fn platform_parent_identity(
    metadata: &fs::Metadata,
    _path: &Path,
) -> Result<PhysicalParentIdentity, AppError> {
    use std::os::unix::fs::MetadataExt;
    Ok(PhysicalParentIdentity::Unix {
        device: metadata.dev(),
        inode: metadata.ino(),
    })
}

#[cfg(windows)]
fn platform_parent_identity(
    _metadata: &fs::Metadata,
    path: &Path,
) -> Result<PhysicalParentIdentity, AppError> {
    let (volume_serial, file_id) = windows_file_identity(path, false)?;
    Ok(PhysicalParentIdentity::Windows {
        volume_serial,
        file_id,
    })
}

#[cfg(windows)]
fn windows_file_identity(path: &Path, no_follow: bool) -> Result<(u64, u128), AppError> {
    use std::ffi::c_void;
    use std::mem::{size_of, MaybeUninit};
    use std::os::windows::fs::OpenOptionsExt;
    use std::os::windows::io::AsRawHandle;

    use windows_sys::Win32::Storage::FileSystem::{
        FileIdInfo, GetFileInformationByHandleEx, FILE_FLAG_BACKUP_SEMANTICS,
        FILE_FLAG_OPEN_REPARSE_POINT, FILE_ID_INFO, FILE_SHARE_DELETE, FILE_SHARE_READ,
        FILE_SHARE_WRITE,
    };

    let mut flags = FILE_FLAG_BACKUP_SEMANTICS;
    if no_follow {
        flags |= FILE_FLAG_OPEN_REPARSE_POINT;
    }
    let file = fs::OpenOptions::new()
        .access_mode(0)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
        .custom_flags(flags)
        .open(path)?;
    let mut info = MaybeUninit::<FILE_ID_INFO>::uninit();
    let succeeded = unsafe {
        GetFileInformationByHandleEx(
            file.as_raw_handle(),
            FileIdInfo,
            info.as_mut_ptr().cast::<c_void>(),
            size_of::<FILE_ID_INFO>() as u32,
        )
    };
    if succeeded == 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    let info = unsafe { info.assume_init() };
    Ok((
        info.VolumeSerialNumber,
        u128::from_le_bytes(info.FileId.Identifier),
    ))
}

#[cfg(not(any(unix, windows)))]
fn platform_parent_identity(
    _metadata: &fs::Metadata,
    path: &Path,
) -> Result<PhysicalParentIdentity, AppError> {
    Err(stable_identity_unavailable(path))
}

#[cfg(not(any(unix, windows)))]
fn stable_identity_unavailable(path: &Path) -> AppError {
    AppError::CapabilityUnavailable {
        capability: "stableIdentity".to_string(),
        path: Some(path.to_string_lossy().into_owned()),
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::*;

    #[cfg(unix)]
    #[test]
    fn inspection_and_removal_do_not_follow_a_final_symlink() {
        use std::os::unix::fs::symlink;

        let temp = tempdir().expect("temp");
        let target = temp.path().join("target");
        let link = temp.path().join("link");
        fs::create_dir(&target).expect("target");
        fs::write(target.join("keep.txt"), b"keep").expect("content");
        symlink(&target, &link).expect("symlink");

        let inspected = inspect_entry_no_follow(&link).expect("inspect");
        assert_eq!(inspected.kind, NativeEntryKind::Symlink);
        assert_eq!(inspected.link_target.as_deref(), Some(target.as_path()));

        remove_entry_no_follow(&link).expect("remove link");
        assert!(!link.exists());
        assert_eq!(
            fs::read(target.join("keep.txt")).expect("target remains"),
            b"keep"
        );
    }

    #[cfg(unix)]
    #[test]
    fn broken_symlink_is_visible_and_removable() {
        use std::os::unix::fs::symlink;

        let temp = tempdir().expect("temp");
        let link = temp.path().join("broken");
        symlink(temp.path().join("missing"), &link).expect("symlink");

        let inspected = inspect_entry_no_follow(&link).expect("inspect");
        assert_eq!(inspected.kind, NativeEntryKind::Symlink);
        assert!(!link.exists());
        assert!(link.symlink_metadata().is_ok());

        remove_entry_no_follow(&link).expect("remove broken link");
        assert!(link.symlink_metadata().is_err());
    }

    #[cfg(any(unix, windows))]
    #[test]
    fn parent_identity_is_shared_by_siblings_and_differs_across_parents() {
        let temp = tempdir().expect("temp");
        let first_parent = temp.path().join("first");
        let second_parent = temp.path().join("second");
        fs::create_dir(&first_parent).expect("first");
        fs::create_dir(&second_parent).expect("second");

        let first = physical_parent_identity(&first_parent).expect("identity");
        let first_again = physical_parent_identity(&first_parent).expect("same identity");
        let second = physical_parent_identity(&second_parent).expect("other identity");
        assert_eq!(first, first_again);
        assert_ne!(first, second);
    }

    #[test]
    fn missing_entry_has_a_stable_missing_fingerprint() {
        let temp = tempdir().expect("temp");
        let first = inspect_entry_no_follow(&temp.path().join("missing")).expect("inspect");
        let second = inspect_entry_no_follow(&temp.path().join("missing")).expect("inspect");
        assert_eq!(first.kind, NativeEntryKind::Missing);
        assert_eq!(first.fingerprint, second.fingerprint);
    }

    #[test]
    fn different_entries_have_different_fingerprints() {
        let temp = tempdir().expect("temp");
        let first = temp.path().join("first");
        let second = temp.path().join("second");
        fs::write(&first, b"same content").expect("first");
        fs::write(&second, b"same content").expect("second");

        let first_fingerprint = inspect_entry_no_follow(&first).expect("first fingerprint");
        let second_fingerprint = inspect_entry_no_follow(&second).expect("second fingerprint");

        assert_ne!(
            first_fingerprint.fingerprint,
            second_fingerprint.fingerprint
        );
    }

    #[test]
    fn target_projection_resolves_existing_ancestors_without_creating_the_root() {
        let temp = tempdir().expect("temp");
        let destination = temp.path().join(".custom/skills/demo");
        let physical_destination = fs::canonicalize(temp.path())
            .expect("physical temp root")
            .join(".custom/skills/demo");

        let projection = project_target(
            &destination,
            crate::environment::runtime::ExecutionBackend::NativeUnix,
        )
        .expect("projection");

        assert!(!destination.parent().unwrap().exists());
        assert_eq!(projection.physical_destination, physical_destination);
        assert_eq!(projection.fingerprint.0, "entry-v1-missing");
        assert_eq!(
            projection.key.normalized_final_child_name,
            ".custom/skills/demo"
        );
        assert_eq!(
            projection.key.physical_parent,
            physical_parent_identity(temp.path()).unwrap()
        );
    }
}

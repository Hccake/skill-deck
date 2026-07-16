use std::fs;
use std::io::Read;
use std::path::Path;

use sha2::{Digest, Sha256};
use walkdir::WalkDir;

use crate::environment::content_manifest::{
    ContentManifest, ContentManifestReader, ContentManifestRecord, ContentManifestTarget,
};
use crate::environment::native::tree::project_target;
use crate::environment::runtime::ExecutionBackend;
use crate::environment::types::EnvironmentRef;
use crate::error::AppError;

pub struct NativeContentManifestReader;

impl ContentManifestReader for NativeContentManifestReader {
    fn read<'a>(
        &'a self,
        target: &'a ContentManifestTarget,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<ContentManifest, AppError>> + Send + 'a>,
    > {
        let target = target.clone();
        Box::pin(async move {
            tokio::task::spawn_blocking(move || read_target(&target))
                .await
                .map_err(|error| AppError::ExecutionFailed {
                    message: format!("native content manifest task failed: {error}"),
                })?
        })
    }
}

fn read_target(target: &ContentManifestTarget) -> Result<ContentManifest, AppError> {
    if target.location.environment != EnvironmentRef::Host
        || !matches!(
            target.key.backend,
            ExecutionBackend::NativeWindows | ExecutionBackend::NativeUnix
        )
    {
        return Err(AppError::StorageUnsupported {
            path: target.location.native_path.clone(),
        });
    }
    let path = Path::new(&target.location.native_path);
    let projection = project_target(path, target.key.backend.clone())?;
    if projection.key != target.key || projection.physical_destination != path {
        return Err(AppError::StaleTarget);
    }
    read_directory(path)
}

pub(crate) fn read_directory(root: &Path) -> Result<ContentManifest, AppError> {
    let metadata = fs::symlink_metadata(root)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(AppError::UnsafePath {
            path: root.to_string_lossy().into_owned(),
            reason: "content manifest root is not a physical directory".to_string(),
        });
    }
    let mut records = Vec::new();
    for entry in WalkDir::new(root).follow_links(false).into_iter() {
        let entry = entry.map_err(|error| AppError::Io {
            message: error.to_string(),
        })?;
        if entry.path() == root {
            continue;
        }
        let relative = entry
            .path()
            .strip_prefix(root)
            .map_err(|error| AppError::UnsafePath {
                path: entry.path().to_string_lossy().into_owned(),
                reason: error.to_string(),
            })?;
        let relative = relative_path(relative)?;
        let metadata = fs::symlink_metadata(entry.path())?;
        let record = if metadata.file_type().is_symlink() {
            let target = fs::read_link(entry.path()).and_then(|target| {
                target.into_os_string().into_string().map_err(|_| {
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "symlink target is not UTF-8",
                    )
                })
            })?;
            ContentManifestRecord::symlink(relative, target)?
        } else if metadata.is_dir() {
            ContentManifestRecord::directory(relative)?
        } else if metadata.is_file() {
            ContentManifestRecord::file(
                relative,
                digest_file(entry.path())?,
                executable(&metadata),
            )?
        } else {
            return Err(AppError::UnsafePath {
                path: entry.path().to_string_lossy().into_owned(),
                reason: "content manifest contains an unsupported entry type".to_string(),
            });
        };
        records.push(record);
    }
    ContentManifest::from_records(records)
}

fn relative_path(path: &Path) -> Result<String, AppError> {
    path.components()
        .map(|component| {
            component
                .as_os_str()
                .to_str()
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .ok_or_else(|| AppError::UnsafePath {
                    path: path.to_string_lossy().into_owned(),
                    reason: "content manifest path is not valid UTF-8".to_string(),
                })
        })
        .collect::<Result<Vec<_>, _>>()
        .map(|components| components.join("/"))
}

fn digest_file(path: &Path) -> Result<String, AppError> {
    let mut file = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

#[cfg(unix)]
fn executable(metadata: &fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;
    metadata.permissions().mode() & 0o111 != 0
}

#[cfg(not(unix))]
fn executable(_metadata: &fs::Metadata) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use std::fs;

    use sha2::Digest;

    use crate::environment::content_manifest::{ContentManifestRecord, ContentManifestRecordKind};

    use super::read_directory;

    #[cfg(unix)]
    #[test]
    fn native_reader_captures_files_executable_empty_directories_and_symlinks() {
        use std::os::unix::fs::{symlink, PermissionsExt};

        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("skill");
        fs::create_dir_all(root.join("empty")).unwrap();
        fs::write(root.join("SKILL.md"), b"content").unwrap();
        fs::write(root.join("run.sh"), b"#!/bin/sh\n").unwrap();
        let mut permissions = fs::metadata(root.join("run.sh")).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(root.join("run.sh"), permissions).unwrap();
        symlink("run.sh", root.join("current")).unwrap();

        let manifest = read_directory(&root).unwrap();
        assert_eq!(manifest.records().len(), 4);
        assert!(manifest.records().iter().any(|record| {
            record.relative_path == "empty" && record.kind == ContentManifestRecordKind::Directory
        }));
        assert!(manifest.records().iter().any(|record| {
            record.relative_path == "run.sh"
                && record.kind == ContentManifestRecordKind::File
                && record.executable
        }));
        assert!(manifest.records().iter().any(|record| {
            record.relative_path == "current"
                && record.kind == ContentManifestRecordKind::Symlink
                && record.symlink_target.as_deref() == Some("run.sh")
        }));

        let skill_digest = format!("{:x}", sha2::Sha256::digest(b"content"));
        assert!(manifest
            .records()
            .contains(&ContentManifestRecord::file("SKILL.md", skill_digest, false).unwrap()));
    }

    #[test]
    fn native_reader_fails_closed_for_a_non_directory_root() {
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("not-a-directory");
        fs::write(&file, b"content").unwrap();

        assert!(read_directory(&file).is_err());
    }
}

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use tempfile::NamedTempFile;

use crate::environment::types::{EnvironmentRef, ResourceLocator};
use crate::error::AppError;
use crate::storage::atomic_document::{AtomicDocumentIo, IoFuture};

#[derive(Clone, Copy)]
pub struct NativeAtomicDocumentIo;

impl AtomicDocumentIo for NativeAtomicDocumentIo {
    fn read_optional<'a>(
        &'a self,
        target: &'a ResourceLocator,
    ) -> IoFuture<'a, Result<Option<Vec<u8>>, AppError>> {
        Box::pin(async move {
            let path = host_path(target)?;
            match fs::read(path) {
                Ok(bytes) => Ok(Some(bytes)),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
                Err(error) => Err(error.into()),
            }
        })
    }

    fn write_atomic<'a>(
        &'a self,
        target: &'a ResourceLocator,
        bytes: Vec<u8>,
    ) -> IoFuture<'a, Result<(), AppError>> {
        Box::pin(async move { write_native_atomic(host_path(target)?, &bytes) })
    }
}

fn write_native_atomic(path: &Path, bytes: &[u8]) -> Result<(), AppError> {
    let parent = path.parent().ok_or_else(|| AppError::UnsafePath {
        path: path.to_string_lossy().into_owned(),
        reason: "document path has no parent".to_string(),
    })?;
    fs::create_dir_all(parent)?;
    let legacy_backup = backup_path(path);
    if legacy_backup.exists() {
        fs::remove_file(&legacy_backup)?;
    }

    let mut temporary = NamedTempFile::new_in(parent)?;
    temporary.write_all(bytes)?;
    temporary.as_file_mut().sync_all()?;
    temporary.persist(path).map_err(|error| error.error)?;
    sync_parent(parent)?;
    Ok(())
}

pub(crate) fn backup_path(path: &Path) -> PathBuf {
    let mut name = path
        .file_name()
        .map(|name| name.to_os_string())
        .unwrap_or_default();
    name.push(".bak");
    path.with_file_name(name)
}

fn host_path(locator: &ResourceLocator) -> Result<&Path, AppError> {
    if locator.environment != EnvironmentRef::Host {
        return Err(AppError::StorageUnsupported {
            path: locator.native_path.clone(),
        });
    }
    Ok(Path::new(&locator.native_path))
}

#[cfg(unix)]
fn sync_parent(parent: &Path) -> Result<(), AppError> {
    fs::File::open(parent)?.sync_all()?;
    Ok(())
}

#[cfg(not(unix))]
fn sync_parent(_parent: &Path) -> Result<(), AppError> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::*;
    use crate::environment::types::EnvironmentRef;
    use crate::error::AppError;

    fn locator(path: &Path) -> ResourceLocator {
        ResourceLocator {
            environment: EnvironmentRef::Host,
            native_path: path.to_string_lossy().into_owned(),
        }
    }

    #[tokio::test]
    async fn atomic_write_round_trips_without_leaving_a_sidecar() {
        let temp = tempdir().expect("temp");
        let path = temp.path().join("state/document.json");
        let target = locator(&path);
        let io = NativeAtomicDocumentIo;

        assert_eq!(io.read_optional(&target).await.unwrap(), None);
        assert!(!backup_path(&path).exists());

        io.write_atomic(&target, b"first".to_vec())
            .await
            .expect("first write");
        assert_eq!(
            io.read_optional(&target).await.unwrap(),
            Some(b"first".to_vec())
        );
        assert!(!backup_path(&path).exists());

        fs::write(backup_path(&path), b"legacy backup").expect("legacy backup");
        io.write_atomic(&target, b"second".to_vec())
            .await
            .expect("second write");
        assert_eq!(fs::read(&path).unwrap(), b"second");
        assert!(!backup_path(&path).exists());

        io.write_atomic(&target, b"third".to_vec())
            .await
            .expect("third write");
        assert_eq!(fs::read(&path).unwrap(), b"third");
        assert!(!backup_path(&path).exists());
        assert_eq!(
            fs::read_dir(path.parent().unwrap())
                .unwrap()
                .filter_map(Result::ok)
                .count(),
            1
        );
    }

    #[tokio::test]
    async fn rejects_a_locator_owned_by_another_environment() {
        let io = NativeAtomicDocumentIo;
        let target = ResourceLocator {
            environment: EnvironmentRef::Wsl {
                distro_name: "Ubuntu".to_string(),
            },
            native_path: "/tmp/document.json".to_string(),
        };
        assert!(matches!(
            io.write_atomic(&target, b"data".to_vec()).await,
            Err(AppError::StorageUnsupported { .. })
        ));
    }
}

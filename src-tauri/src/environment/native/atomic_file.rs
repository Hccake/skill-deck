use std::fs;
use std::path::Path;
#[cfg(test)]
use std::path::PathBuf;

use crate::environment::types::{EnvironmentRef, ResourceLocator};
use crate::error::AppError;
use crate::storage::atomic_document::{
    AtomicDocumentIo, DocumentSnapshot, DocumentWriteFailure, IoFuture,
};

#[derive(Clone, Copy)]
pub struct NativeAtomicDocumentIo;

impl AtomicDocumentIo for NativeAtomicDocumentIo {
    fn observe<'a>(
        &'a self,
        target: &'a ResourceLocator,
        max_bytes: u64,
    ) -> IoFuture<'a, Result<DocumentSnapshot, AppError>> {
        let path = native_path(target).map(Path::to_path_buf);
        Box::pin(async move {
            let path = path?;
            let max_bytes = usize::try_from(max_bytes).map_err(|_| AppError::Validation {
                field: Some("documentRead".to_string()),
                message: "document read limit is not supported on this platform".to_string(),
            })?;
            tokio::task::spawn_blocking(move || {
                environment_engine::atomic_document::read_optional_bounded(&path, max_bytes)
                    .map(|bytes| DocumentSnapshot {
                        bytes,
                        generation: None,
                    })
                    .map_err(AppError::from)
            })
            .await
            .map_err(native_document_task_error)?
        })
    }

    fn replace<'a>(
        &'a self,
        target: &'a ResourceLocator,
        expected: DocumentSnapshot,
        bytes: Vec<u8>,
    ) -> IoFuture<'a, Result<DocumentSnapshot, DocumentWriteFailure>> {
        Box::pin(async move {
            let path = native_path(target).map_err(DocumentWriteFailure::not_published)?;
            if expected.generation.is_some() {
                return Err(DocumentWriteFailure::not_published(
                    AppError::StaleEnvironment,
                ));
            }
            prepare_parent(path).map_err(DocumentWriteFailure::not_published)?;
            environment_engine::atomic_document::replace_if_unchanged(
                path,
                expected.bytes.as_deref(),
                &bytes,
            )
            .map_err(DocumentWriteFailure::from_engine)?;
            Ok(DocumentSnapshot {
                bytes: Some(bytes),
                generation: None,
            })
        })
    }

    fn remove<'a>(
        &'a self,
        target: &'a ResourceLocator,
        expected: DocumentSnapshot,
    ) -> IoFuture<'a, Result<(), DocumentWriteFailure>> {
        Box::pin(async move {
            let path = native_path(target).map_err(DocumentWriteFailure::not_published)?;
            if expected.generation.is_some() {
                return Err(DocumentWriteFailure::not_published(
                    AppError::StaleEnvironment,
                ));
            }
            environment_engine::atomic_document::remove_if_unchanged(
                path,
                expected.bytes.as_deref(),
            )
            .map_err(DocumentWriteFailure::from_engine)
        })
    }
}

pub(crate) fn write_native_atomic(path: &Path, bytes: &[u8]) -> Result<(), AppError> {
    prepare_parent(path)?;
    environment_engine::atomic_document::replace(path, bytes)
        .map_err(DocumentWriteFailure::from_engine)
        .map_err(DocumentWriteFailure::into_error)
}

fn prepare_parent(path: &Path) -> Result<(), AppError> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .ok_or_else(|| AppError::UnsafePath {
            path: path.to_string_lossy().into_owned(),
            reason: "document path has no explicit parent".to_string(),
        })?;
    // Parent creation remains the Native adapter's explicit private-store
    // policy, not an implicit fallback inside the publication primitive.
    fs::create_dir_all(parent)?;
    Ok(())
}

fn native_document_task_error(error: tokio::task::JoinError) -> AppError {
    AppError::Io {
        message: format!("native document task did not complete: {error}"),
    }
}

#[cfg(test)]
pub(crate) fn backup_path(path: &Path) -> PathBuf {
    let mut name = path
        .file_name()
        .map(|name| name.to_os_string())
        .unwrap_or_default();
    name.push(".bak");
    path.with_file_name(name)
}

fn native_path(locator: &ResourceLocator) -> Result<&Path, AppError> {
    if locator.environment != EnvironmentRef::Native {
        return Err(AppError::StorageUnsupported {
            path: locator.native_path.clone(),
        });
    }
    Ok(Path::new(&locator.native_path))
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
            environment: EnvironmentRef::Native,
            native_path: path.to_string_lossy().into_owned(),
        }
    }

    #[tokio::test]
    async fn atomic_write_does_not_create_or_delete_an_unrelated_sidecar() {
        let temp = tempdir().expect("temp");
        let path = temp.path().join("state/document.json");
        let target = locator(&path);
        let io = NativeAtomicDocumentIo;

        assert_eq!(io.observe(&target, 32).await.unwrap().bytes, None);
        assert!(!backup_path(&path).exists());

        let snapshot = io.observe(&target, 32).await.unwrap();
        io.replace(&target, snapshot, b"first".to_vec())
            .await
            .expect("first write");
        assert_eq!(
            io.observe(&target, 32).await.unwrap().bytes,
            Some(b"first".to_vec())
        );
        assert!(!backup_path(&path).exists());

        fs::write(backup_path(&path), b"legacy backup").expect("legacy backup");
        let snapshot = io.observe(&target, 32).await.unwrap();
        io.replace(&target, snapshot, b"second".to_vec())
            .await
            .expect("second write");
        assert_eq!(fs::read(&path).unwrap(), b"second");
        assert_eq!(fs::read(backup_path(&path)).unwrap(), b"legacy backup");

        let snapshot = io.observe(&target, 32).await.unwrap();
        io.replace(&target, snapshot, b"third".to_vec())
            .await
            .expect("third write");
        assert_eq!(fs::read(&path).unwrap(), b"third");
        assert_eq!(fs::read(backup_path(&path)).unwrap(), b"legacy backup");
        assert_eq!(
            fs::read_dir(path.parent().unwrap())
                .unwrap()
                .filter_map(Result::ok)
                .count(),
            2
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
            io.observe(&target, 32).await,
            Err(AppError::StorageUnsupported { .. })
        ));
    }
    #[tokio::test]
    async fn conditional_save_uses_original_snapshot_not_a_fresh_revision() {
        let temp = tempdir().unwrap();
        let path = temp.path().join("document.json");
        let target = locator(&path);
        let io = NativeAtomicDocumentIo;
        let missing = io.observe(&target, 16).await.unwrap();
        io.replace(&target, missing, b"old".to_vec()).await.unwrap();
        let snapshot = io.observe(&target, 16).await.unwrap();
        fs::write(&path, b"external").unwrap();
        let failure = io
            .replace(&target, snapshot, b"new".to_vec())
            .await
            .unwrap_err();
        assert_eq!(
            failure.publication,
            crate::storage::atomic_document::PublicationState::NotPublished
        );
        assert!(matches!(failure.error, AppError::StaleTarget));
        assert_eq!(fs::read(path).unwrap(), b"external");
    }

    #[tokio::test]
    async fn successful_replace_returns_the_committed_snapshot() {
        let temp = tempdir().unwrap();
        let path = temp.path().join("document.json");
        let target = locator(&path);
        fs::write(&path, b"old").unwrap();
        let snapshot = NativeAtomicDocumentIo.observe(&target, 16).await.unwrap();

        let receipt = NativeAtomicDocumentIo
            .replace(&target, snapshot, b"new".to_vec())
            .await
            .unwrap();

        assert_eq!(receipt.bytes.as_deref(), Some(b"new".as_slice()));
        assert_eq!(receipt.generation, None);
    }

    #[tokio::test]
    async fn bounded_observe_rejects_an_oversized_document() {
        let temp = tempdir().unwrap();
        let path = temp.path().join("document.json");
        let target = locator(&path);
        fs::write(&path, b"12345").unwrap();

        let result = NativeAtomicDocumentIo.observe(&target, 4).await;

        assert!(result.is_err());
        assert_eq!(fs::read(path).unwrap(), b"12345");
    }
}

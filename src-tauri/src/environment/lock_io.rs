use crate::environment::native::atomic_file::NativeAtomicDocumentIo;
use crate::environment::types::ResourceLocator;
use crate::environment::wsl::{WslSession, WslWorkspace};
use crate::error::AppError;
use crate::storage::atomic_document::AtomicDocumentIo;
use sha2::{Digest, Sha256};

pub struct LockDocumentSnapshot {
    pub bytes: Option<Vec<u8>>,
    pub revision: Option<String>,
    pub generation: Option<u64>,
}

pub enum EnvironmentLockIo {
    Native,
    ActiveWsl {
        session: Box<WslSession>,
        workspace: WslWorkspace,
    },
}

impl EnvironmentLockIo {
    pub async fn read_optional(
        &self,
        locator: &ResourceLocator,
    ) -> Result<Option<Vec<u8>>, AppError> {
        Ok(self.read_optional_snapshot(locator).await?.bytes)
    }

    pub async fn read_optional_snapshot(
        &self,
        locator: &ResourceLocator,
    ) -> Result<LockDocumentSnapshot, AppError> {
        match self {
            Self::Native => {
                let bytes = NativeAtomicDocumentIo.read_optional(locator).await?;
                let revision = bytes.as_deref().map(document_revision);
                Ok(LockDocumentSnapshot {
                    bytes,
                    revision,
                    generation: None,
                })
            }
            Self::ActiveWsl { session, workspace } => {
                require_active_wsl_target(session, locator)?;
                let snapshot = workspace
                    .read_optional_document_snapshot_once(
                        locator.native_path.clone(),
                        environment_protocol::MAX_DOCUMENT_BYTES,
                    )
                    .await?;
                Ok(LockDocumentSnapshot {
                    bytes: snapshot.bytes,
                    revision: snapshot.revision,
                    generation: Some(snapshot.generation),
                })
            }
        }
    }

    #[cfg(test)]
    pub async fn read(&self, locator: &ResourceLocator) -> Result<Vec<u8>, AppError> {
        self.read_optional(locator)
            .await?
            .ok_or_else(|| AppError::PathNotFound {
                path: locator.native_path.clone(),
            })
    }

    #[cfg(test)]
    pub async fn write_atomic(
        &self,
        locator: &ResourceLocator,
        bytes: Vec<u8>,
    ) -> Result<(), AppError> {
        match self {
            Self::Native => NativeAtomicDocumentIo.write_atomic(locator, bytes).await,
            Self::ActiveWsl { session, workspace } => {
                let snapshot = self.read_optional_snapshot(locator).await?;
                require_active_wsl_target(session, locator)?;
                workspace
                    .write_document_atomic(
                        snapshot.generation.ok_or(AppError::StaleEnvironment)?,
                        locator.native_path.clone(),
                        snapshot.revision,
                        bytes,
                    )
                    .await
                    .map(|_| ())
            }
        }
    }

    pub async fn write_if_revision(
        &self,
        locator: &ResourceLocator,
        expected_generation: Option<u64>,
        expected_revision: Option<String>,
        bytes: Vec<u8>,
    ) -> Result<(), AppError> {
        match self {
            Self::Native => {
                let current = NativeAtomicDocumentIo.read_optional(locator).await?;
                if current.as_deref().map(document_revision) != expected_revision {
                    return Err(AppError::StaleTarget);
                }
                NativeAtomicDocumentIo.write_atomic(locator, bytes).await
            }
            Self::ActiveWsl { session, workspace } => {
                require_active_wsl_target(session, locator)?;
                workspace
                    .write_document_atomic(
                        expected_generation.ok_or(AppError::StaleEnvironment)?,
                        locator.native_path.clone(),
                        expected_revision,
                        bytes,
                    )
                    .await
                    .map(|_| ())
            }
        }
    }
}

fn document_revision(bytes: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(bytes))
}

fn require_active_wsl_target(
    session: &WslSession,
    locator: &ResourceLocator,
) -> Result<(), AppError> {
    match &locator.environment {
        crate::environment::types::EnvironmentRef::Wsl { distro_name }
            if distro_name.eq_ignore_ascii_case(&session.distro_name)
                && locator.native_path.starts_with('/') =>
        {
            Ok(())
        }
        _ => Err(AppError::StorageUnsupported {
            path: locator.native_path.clone(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::EnvironmentLockIo;
    use crate::environment::types::{EnvironmentRef, ResourceLocator};

    #[tokio::test]
    async fn native_lock_io_round_trips_bytes_atomically() {
        let temp = tempdir().expect("tempdir");
        let locator = ResourceLocator {
            environment: EnvironmentRef::Native,
            native_path: temp
                .path()
                .join("state/lock.json")
                .to_string_lossy()
                .to_string(),
        };
        let io = EnvironmentLockIo::Native;

        io.write_atomic(&locator, br#"{"skills":{}}\n"#.to_vec())
            .await
            .expect("write lock");

        assert_eq!(
            io.read(&locator).await.expect("read lock"),
            br#"{"skills":{}}\n"#
        );
    }

    #[tokio::test]
    async fn native_optional_read_distinguishes_missing_lock_from_empty_bytes() {
        let temp = tempdir().expect("tempdir");
        let locator = ResourceLocator {
            environment: EnvironmentRef::Native,
            native_path: temp
                .path()
                .join("state/lock.json")
                .to_string_lossy()
                .to_string(),
        };
        let io = EnvironmentLockIo::Native;

        assert_eq!(
            io.read_optional(&locator).await.expect("missing lock"),
            None
        );

        io.write_atomic(&locator, Vec::new())
            .await
            .expect("write empty lock");
        assert_eq!(
            io.read_optional(&locator).await.expect("empty lock"),
            Some(Vec::new())
        );
    }

    #[tokio::test]
    async fn native_lock_io_does_not_leave_a_previous_version_sidecar() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("state/lock.json");
        let locator = ResourceLocator {
            environment: EnvironmentRef::Native,
            native_path: path.to_string_lossy().into_owned(),
        };
        let io = EnvironmentLockIo::Native;

        io.write_atomic(&locator, b"first".to_vec()).await.unwrap();
        io.write_atomic(&locator, b"second".to_vec()).await.unwrap();

        assert_eq!(std::fs::read(&path).unwrap(), b"second");
        assert!(!path.with_file_name("lock.json.bak").exists());
        assert_eq!(
            std::fs::read_dir(path.parent().unwrap()).unwrap().count(),
            1
        );
    }
}

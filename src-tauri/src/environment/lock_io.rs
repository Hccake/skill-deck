use crate::environment::native::atomic_file::NativeAtomicDocumentIo;
use crate::environment::types::ResourceLocator;
use crate::environment::wsl::operations::atomic_file::WslAtomicDocumentIo;
use crate::environment::wsl::{WslSession, WslWorkspace};
use crate::error::AppError;
use crate::storage::atomic_document::{AtomicDocumentIo, DocumentSnapshot, DocumentWriteFailure};

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
        Ok(self.observe(locator).await?.bytes)
    }

    pub async fn observe(&self, locator: &ResourceLocator) -> Result<DocumentSnapshot, AppError> {
        match self {
            Self::Native => {
                NativeAtomicDocumentIo
                    .observe(locator, u64::from(environment_protocol::MAX_DOCUMENT_BYTES))
                    .await
            }
            Self::ActiveWsl { session, workspace } => {
                require_active_wsl_target(session, locator)?;
                WslAtomicDocumentIo::from_active_session((**session).clone(), workspace.clone())
                    .observe(locator, u64::from(environment_protocol::MAX_DOCUMENT_BYTES))
                    .await
            }
        }
    }

    pub async fn replace(
        &self,
        locator: &ResourceLocator,
        expected: DocumentSnapshot,
        bytes: Vec<u8>,
    ) -> Result<DocumentSnapshot, DocumentWriteFailure> {
        match self {
            Self::Native => {
                NativeAtomicDocumentIo
                    .replace(locator, expected, bytes)
                    .await
            }
            Self::ActiveWsl { session, workspace } => {
                require_active_wsl_target(session, locator)
                    .map_err(DocumentWriteFailure::not_published)?;
                WslAtomicDocumentIo::from_active_session((**session).clone(), workspace.clone())
                    .replace(locator, expected, bytes)
                    .await
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
        let snapshot = self.observe(locator).await?;
        self.replace(locator, snapshot, bytes)
            .await
            .map(|_| ())
            .map_err(DocumentWriteFailure::into_error)
    }
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

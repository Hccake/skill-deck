use crate::environment::native::atomic_file::NativeAtomicDocumentIo;
use crate::environment::types::ResourceLocator;
use crate::environment::wsl::operations::atomic_file::WslAtomicDocumentIo;
use crate::environment::wsl::WslSession;
use crate::error::AppError;
use crate::storage::atomic_document::AtomicDocumentIo;

pub enum EnvironmentLockIo {
    Native,
    ActiveWsl(WslSession),
}

impl EnvironmentLockIo {
    pub async fn read_optional(
        &self,
        locator: &ResourceLocator,
    ) -> Result<Option<Vec<u8>>, AppError> {
        match self {
            Self::Native => NativeAtomicDocumentIo.read_optional(locator).await,
            Self::ActiveWsl(session) => {
                WslAtomicDocumentIo::from_active_session(session.clone())
                    .read_optional(locator)
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

    pub async fn write_atomic(
        &self,
        locator: &ResourceLocator,
        bytes: Vec<u8>,
    ) -> Result<(), AppError> {
        match self {
            Self::Native => NativeAtomicDocumentIo.write_atomic(locator, bytes).await,
            Self::ActiveWsl(session) => {
                WslAtomicDocumentIo::from_active_session(session.clone())
                    .write_atomic(locator, bytes)
                    .await
            }
        }
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

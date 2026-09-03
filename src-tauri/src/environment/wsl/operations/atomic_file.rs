use crate::environment::types::{EnvironmentRef, ResourceLocator};
use crate::environment::wsl::{WslSession, WslWorkspace};
use crate::error::AppError;
use crate::storage::atomic_document::{AtomicDocumentIo, IoFuture};

pub struct WslAtomicDocumentIo {
    access: WslAtomicDocumentAccess,
}

enum WslAtomicDocumentAccess {
    Workspace(WslWorkspace),
    Active {
        session: WslSession,
        workspace: WslWorkspace,
    },
}

impl WslAtomicDocumentIo {
    pub fn new(workspace: WslWorkspace) -> Self {
        Self {
            access: WslAtomicDocumentAccess::Workspace(workspace),
        }
    }

    pub(crate) fn from_active_session(session: WslSession, workspace: WslWorkspace) -> Self {
        Self {
            access: WslAtomicDocumentAccess::Active { session, workspace },
        }
    }

    fn workspace(&self) -> &WslWorkspace {
        match &self.access {
            WslAtomicDocumentAccess::Workspace(workspace)
            | WslAtomicDocumentAccess::Active { workspace, .. } => workspace,
        }
    }

    fn path<'a>(&self, target: &'a ResourceLocator) -> Result<&'a str, AppError> {
        let expected_distro_name = match &self.access {
            WslAtomicDocumentAccess::Workspace(workspace) => workspace.distro_name(),
            WslAtomicDocumentAccess::Active { session, .. } => &session.distro_name,
        };
        match &target.environment {
            EnvironmentRef::Wsl { distro_name }
                if distro_name.eq_ignore_ascii_case(expected_distro_name)
                    && target.native_path.starts_with('/') =>
            {
                Ok(&target.native_path)
            }
            _ => Err(AppError::StorageUnsupported {
                path: target.native_path.clone(),
            }),
        }
    }
}

impl AtomicDocumentIo for WslAtomicDocumentIo {
    fn read_optional<'a>(
        &'a self,
        target: &'a ResourceLocator,
    ) -> IoFuture<'a, Result<Option<Vec<u8>>, AppError>> {
        Box::pin(async move {
            self.workspace()
                .read_optional_document(
                    self.path(target)?.to_string(),
                    environment_protocol::MAX_DOCUMENT_BYTES,
                )
                .await
        })
    }

    fn write_atomic<'a>(
        &'a self,
        target: &'a ResourceLocator,
        bytes: Vec<u8>,
    ) -> IoFuture<'a, Result<(), AppError>> {
        Box::pin(async move {
            let path = self.path(target)?.to_string();
            let snapshot = self
                .workspace()
                .read_optional_document_snapshot_once(
                    path.clone(),
                    environment_protocol::MAX_DOCUMENT_BYTES,
                )
                .await?;
            self.workspace()
                .write_document_atomic(snapshot.generation, path, snapshot.revision, bytes)
                .await
                .map(|_| ())
        })
    }
}

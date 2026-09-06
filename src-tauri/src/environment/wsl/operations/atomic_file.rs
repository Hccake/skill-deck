use crate::environment::types::{EnvironmentRef, ResourceLocator};
use crate::environment::wsl::{WslSession, WslWorkspace};
use crate::error::AppError;
use crate::storage::atomic_document::{
    AtomicDocumentIo, DocumentSnapshot, DocumentWriteFailure, IoFuture,
};
use sha2::{Digest, Sha256};

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
    fn observe<'a>(
        &'a self,
        target: &'a ResourceLocator,
        max_bytes: u64,
    ) -> IoFuture<'a, Result<DocumentSnapshot, AppError>> {
        Box::pin(async move {
            let max_bytes = u32::try_from(max_bytes).map_err(|_| AppError::Validation {
                field: Some("documentRead".to_string()),
                message: "document read limit exceeds the WSL protocol".to_string(),
            })?;
            let snapshot = self
                .workspace()
                .read_optional_document_snapshot_once(self.path(target)?.to_string(), max_bytes)
                .await?;
            Ok(DocumentSnapshot {
                bytes: snapshot.bytes,
                generation: Some(snapshot.generation),
            })
        })
    }

    fn replace<'a>(
        &'a self,
        target: &'a ResourceLocator,
        expected: DocumentSnapshot,
        bytes: Vec<u8>,
    ) -> IoFuture<'a, Result<DocumentSnapshot, DocumentWriteFailure>> {
        Box::pin(async move {
            let path = self
                .path(target)
                .map_err(DocumentWriteFailure::not_published)?
                .to_string();
            let (generation, revision) = snapshot_binding(&expected)?;
            // The request stays bound to the generation and original bytes.
            // A transport error is not proof that the Worker did not publish.
            self.workspace()
                .commit_document_atomic(generation, path, revision, bytes.clone())
                .await?;
            Ok(DocumentSnapshot {
                bytes: Some(bytes),
                generation: Some(generation),
            })
        })
    }

    fn remove<'a>(
        &'a self,
        target: &'a ResourceLocator,
        expected: DocumentSnapshot,
    ) -> IoFuture<'a, Result<(), DocumentWriteFailure>> {
        Box::pin(async move {
            let path = self
                .path(target)
                .map_err(DocumentWriteFailure::not_published)?
                .to_string();
            let (generation, revision) = snapshot_binding(&expected)?;
            self.workspace()
                .remove_document_if_revision(generation, path, revision)
                .await
        })
    }
}

fn snapshot_binding(
    snapshot: &DocumentSnapshot,
) -> Result<(u64, Option<String>), DocumentWriteFailure> {
    let generation = snapshot
        .generation
        .ok_or_else(|| DocumentWriteFailure::not_published(AppError::StaleEnvironment))?;
    let revision = snapshot
        .bytes
        .as_deref()
        .map(|bytes| format!("sha256:{:x}", Sha256::digest(bytes)));
    Ok((generation, revision))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::atomic_document::{DocumentSnapshot, PublicationState};

    #[test]
    fn snapshot_binding_rejects_missing_worker_generation() {
        let failure = snapshot_binding(&DocumentSnapshot {
            bytes: None,
            generation: None,
        })
        .unwrap_err();
        assert_eq!(failure.publication, PublicationState::NotPublished);
        assert!(matches!(failure.error, AppError::StaleEnvironment));
    }

    #[test]
    fn snapshot_binding_distinguishes_missing_and_empty_documents() {
        let absent = DocumentSnapshot {
            bytes: None,
            generation: Some(7),
        };
        let empty = DocumentSnapshot {
            bytes: Some(Vec::new()),
            generation: Some(7),
        };
        assert_eq!(snapshot_binding(&absent).unwrap(), (7, None));
        assert_eq!(
            snapshot_binding(&empty).unwrap(),
            (
                7,
                Some(
                    "sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
                        .to_string()
                )
            )
        );
    }
}

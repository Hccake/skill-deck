use std::fmt;
use std::path::PathBuf;

use environment_engine::library::{
    self as engine, CatalogWrite, ContentAction, LibraryCommit, TargetExpectation,
};
use environment_engine::linux_mutation::ParentIdentity;
use environment_protocol::{
    LibraryCatalogResponse, LibraryMemberAction, LibraryOperationAction, LibraryOperationRequest,
    MAX_REQUEST_DEADLINE_MILLIS,
};

use crate::payload::{PayloadError, PayloadManager};

pub struct LibraryManager {
    root: PathBuf,
}

#[derive(Debug)]
pub enum LibraryError {
    InvalidRequest,
    StaleTarget,
    StalePayload,
    RecoveryIncomplete,
    Io,
}

impl fmt::Display for LibraryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for LibraryError {}

impl LibraryManager {
    pub fn new(home: PathBuf) -> Self {
        Self {
            root: home.join(".skill-deck/skill-libraries"),
        }
    }

    pub fn read_catalog(&self) -> Result<LibraryCatalogResponse, LibraryError> {
        let snapshot = engine::read_catalog(&self.root).map_err(map_engine_error)?;
        Ok(LibraryCatalogResponse {
            present: snapshot.bytes.is_some(),
            bytes: snapshot.bytes.unwrap_or_default(),
            revision: snapshot.revision,
        })
    }

    pub fn execute(
        &self,
        request: LibraryOperationRequest,
        payloads: &PayloadManager,
    ) -> Result<String, LibraryError> {
        if request.deadline_millis == 0
            || request.deadline_millis > MAX_REQUEST_DEADLINE_MILLIS
            || !valid_component(&request.operation_id)
            || request.catalog_bytes.is_empty()
        {
            return Err(LibraryError::InvalidRequest);
        }
        let catalog = CatalogWrite {
            expected_revision: request.expected_catalog_revision,
            bytes: request.catalog_bytes,
        };
        match request.action {
            LibraryOperationAction::SaveCatalog { library_ids } => {
                engine::write_catalog(&self.root, &library_ids, catalog).map_err(map_engine_error)
            }
            LibraryOperationAction::CommitMember {
                library_id,
                skill_name,
                expected_anchor_device,
                expected_anchor_inode,
                expected_fingerprint,
                expected_content_hash,
                mutation,
            } => {
                if !valid_component(&library_id) || !valid_component(&skill_name) {
                    return Err(LibraryError::InvalidRequest);
                }
                let content = match mutation {
                    LibraryMemberAction::Upsert { payload_id } => ContentAction::Upsert {
                        payload_root: payloads
                            .payload_root(payload_id)
                            .map_err(map_payload_error)?,
                    },
                    LibraryMemberAction::Delete => ContentAction::Delete,
                };
                engine::commit(LibraryCommit {
                    root: self.root.clone(),
                    operation_id: request.operation_id,
                    destination: self
                        .root
                        .join("libraries")
                        .join(library_id)
                        .join("skills")
                        .join(skill_name),
                    expected_target: expectation(
                        expected_anchor_device,
                        expected_anchor_inode,
                        expected_fingerprint,
                        expected_content_hash,
                    ),
                    content,
                    catalog,
                })
                .map_err(map_engine_error)?;
                Ok(catalog_revision(&self.root)?)
            }
            LibraryOperationAction::DeleteLibrary {
                library_id,
                expected_anchor_device,
                expected_anchor_inode,
                expected_fingerprint,
                expected_content_hash,
            } => {
                if !valid_component(&library_id) {
                    return Err(LibraryError::InvalidRequest);
                }
                engine::commit(LibraryCommit {
                    root: self.root.clone(),
                    operation_id: request.operation_id,
                    destination: self.root.join("libraries").join(library_id),
                    expected_target: expectation(
                        expected_anchor_device,
                        expected_anchor_inode,
                        expected_fingerprint,
                        expected_content_hash,
                    ),
                    content: ContentAction::DeleteIfPresent,
                    catalog,
                })
                .map_err(map_engine_error)?;
                Ok(catalog_revision(&self.root)?)
            }
        }
    }
}

fn expectation(
    device: u64,
    inode: u64,
    fingerprint: String,
    content_hash: Option<String>,
) -> TargetExpectation {
    TargetExpectation {
        parent: ParentIdentity { device, inode },
        fingerprint,
        content_hash,
    }
}

fn catalog_revision(root: &std::path::Path) -> Result<String, LibraryError> {
    engine::read_catalog(root)
        .map_err(map_engine_error)?
        .revision
        .ok_or(LibraryError::Io)
}

fn valid_component(value: &str) -> bool {
    !value.is_empty() && !matches!(value, "." | "..") && !value.contains(['/', '\\', '\0'])
}

fn map_engine_error(error: engine::LibraryError) -> LibraryError {
    match error {
        engine::LibraryError::InvalidRequest | engine::LibraryError::UnsupportedPlatform => {
            LibraryError::InvalidRequest
        }
        engine::LibraryError::StaleTarget => LibraryError::StaleTarget,
        engine::LibraryError::InvalidPayload => LibraryError::StalePayload,
        engine::LibraryError::RecoveryIncomplete => LibraryError::RecoveryIncomplete,
        engine::LibraryError::Io(_) => LibraryError::Io,
    }
}

fn map_payload_error(_error: PayloadError) -> LibraryError {
    LibraryError::StalePayload
}

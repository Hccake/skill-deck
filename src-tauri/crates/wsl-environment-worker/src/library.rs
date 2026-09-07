use std::fmt;
use std::path::PathBuf;

use environment_engine::library::{
    self as engine, CatalogWrite, ContentAction, LibraryCommit, TargetExpectation,
};
use environment_engine::linux_mutation::ParentIdentity;
use environment_protocol::{
    LibraryApplicationIndex, LibraryCatalogResponse, LibraryMemberAction, LibraryOperationAction,
    LibraryOperationRequest, MAX_DIRECTORY_COUNT_LIMIT, MAX_REQUEST_DEADLINE_MILLIS,
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

    pub fn list_applications(&self) -> Result<LibraryApplicationIndex, LibraryError> {
        let applications = self.root.join("applications");
        let mut project_ids = Vec::new();
        let mut problem_keys = Vec::new();
        match std::fs::read_dir(&applications) {
            Ok(entries) => {
                for entry in entries {
                    let entry = match entry {
                        Ok(entry) => entry,
                        Err(_) => {
                            problem_keys.push("applications/<unreadable>".to_string());
                            continue;
                        }
                    };
                    let name = entry.file_name().to_string_lossy().into_owned();
                    if name != "global.json" && name != "projects" {
                        problem_keys.push(name);
                    }
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(LibraryApplicationIndex {
                    project_ids,
                    problem_keys,
                    complete: true,
                });
            }
            Err(_) => {
                problem_keys.push("applications".to_string());
                return Ok(LibraryApplicationIndex {
                    project_ids,
                    problem_keys,
                    complete: false,
                });
            }
        }
        let projects = applications.join("projects");
        match std::fs::read_dir(projects) {
            Ok(entries) => {
                for (index, entry) in entries.enumerate() {
                    if index == MAX_DIRECTORY_COUNT_LIMIT as usize {
                        problem_keys.push("projects/<limit>".to_string());
                        break;
                    }
                    let entry = match entry {
                        Ok(entry) => entry,
                        Err(_) => {
                            problem_keys.push("projects/<unreadable>".to_string());
                            continue;
                        }
                    };
                    let name = entry.file_name();
                    let display = name.to_string_lossy().into_owned();
                    let project_id = std::path::Path::new(&name)
                        .file_stem()
                        .and_then(|value| value.to_str())
                        .filter(|_| {
                            std::path::Path::new(&name)
                                .extension()
                                .is_some_and(|value| value == "json")
                        });
                    let valid = entry.file_type().is_ok_and(|kind| kind.is_file())
                        && project_id.is_some_and(valid_component);
                    if let Some(project_id) = project_id.filter(|_| valid) {
                        project_ids.push(project_id.to_string());
                    } else {
                        problem_keys.push(format!("projects/{display}"));
                    }
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => problem_keys.push("projects".to_string()),
        }
        project_ids.sort();
        project_ids.dedup();
        problem_keys.sort();
        problem_keys.dedup();
        Ok(LibraryApplicationIndex {
            complete: problem_keys.is_empty(),
            project_ids,
            problem_keys,
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
            || request.catalog_bytes.len() > environment_protocol::MAX_DOCUMENT_BYTES as usize
        {
            return Err(LibraryError::InvalidRequest);
        }
        let catalog = CatalogWrite {
            expected_revision: request.expected_catalog_revision,
            bytes: request.catalog_bytes,
            max_current_bytes: environment_protocol::MAX_DOCUMENT_BYTES as usize,
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

#[cfg(test)]
mod tests {
    use super::LibraryManager;

    #[test]
    fn inaccessible_project_namespace_returns_an_incomplete_inventory() {
        let home = tempfile::tempdir().unwrap();
        let applications = home.path().join(".skill-deck/skill-libraries/applications");
        std::fs::create_dir_all(&applications).unwrap();
        std::fs::write(applications.join("projects"), b"not a directory").unwrap();

        let inventory = LibraryManager::new(home.path().to_path_buf())
            .list_applications()
            .unwrap();

        assert!(inventory.project_ids.is_empty());
        assert_eq!(inventory.problem_keys, vec!["projects"]);
        assert!(!inventory.complete);
    }
}

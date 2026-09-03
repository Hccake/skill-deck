use crate::environment::wsl::WslWorkspace;
use crate::error::AppError;

const PATH_METADATA_DEADLINE_MILLIS: u64 = 20_000;
const EVE_PACKAGE_LIMIT: u32 = 1024 * 1024;
const PATH_METADATA_AGGREGATE_LIMIT: u32 = 8 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathMetadataQuery {
    pub path: String,
    pub inspect_content: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathMetadataKind {
    Missing,
    Directory,
    SymlinkDirectory,
    SymlinkOther,
    Other,
    BrokenLink,
    Inaccessible,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathMetadataContent {
    NotRequested,
    Empty,
    Unreadable,
    Bytes(Vec<u8>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathMetadataFact {
    pub path: String,
    pub kind: PathMetadataKind,
    pub content: PathMetadataContent,
}

impl WslWorkspace {
    pub(crate) async fn inspect_path_metadata(
        &self,
        queries: Vec<PathMetadataQuery>,
    ) -> Result<Vec<PathMetadataFact>, AppError> {
        if queries.is_empty() || queries.iter().any(|query| !query.path.starts_with('/')) {
            return Err(AppError::Validation {
                field: Some("pathMetadata.queries".to_string()),
                message: "WSL path metadata requires absolute paths".to_string(),
            });
        }
        let expected_paths = queries
            .iter()
            .map(|query| query.path.clone())
            .collect::<Vec<_>>();
        let response: environment_protocol::PathMetadataResponse = self
            .request_worker_payload(environment_protocol::Message::InspectPaths {
                request: environment_protocol::PathMetadataRequest {
                    queries: queries
                        .into_iter()
                        .map(|query| environment_protocol::PathMetadataQuery {
                            path: query.path,
                            content_limit: query.inspect_content.then_some(EVE_PACKAGE_LIMIT),
                        })
                        .collect(),
                    aggregate_content_limit: PATH_METADATA_AGGREGATE_LIMIT,
                    deadline_millis: PATH_METADATA_DEADLINE_MILLIS,
                },
            })
            .await?;
        if response.facts.len() != expected_paths.len()
            || response
                .facts
                .iter()
                .zip(&expected_paths)
                .any(|(fact, path)| fact.path != *path)
        {
            return Err(protocol_error());
        }
        Ok(response
            .facts
            .into_iter()
            .map(|fact| PathMetadataFact {
                path: fact.path,
                kind: match fact.kind {
                    environment_protocol::PathMetadataKind::Missing => PathMetadataKind::Missing,
                    environment_protocol::PathMetadataKind::Directory => {
                        PathMetadataKind::Directory
                    }
                    environment_protocol::PathMetadataKind::SymlinkDirectory => {
                        PathMetadataKind::SymlinkDirectory
                    }
                    environment_protocol::PathMetadataKind::SymlinkOther => {
                        PathMetadataKind::SymlinkOther
                    }
                    environment_protocol::PathMetadataKind::Other => PathMetadataKind::Other,
                    environment_protocol::PathMetadataKind::BrokenLink => {
                        PathMetadataKind::BrokenLink
                    }
                    environment_protocol::PathMetadataKind::Inaccessible => {
                        PathMetadataKind::Inaccessible
                    }
                },
                content: match fact.content {
                    environment_protocol::PathMetadataContent::NotRequested => {
                        PathMetadataContent::NotRequested
                    }
                    environment_protocol::PathMetadataContent::Empty => PathMetadataContent::Empty,
                    environment_protocol::PathMetadataContent::Unreadable => {
                        PathMetadataContent::Unreadable
                    }
                    environment_protocol::PathMetadataContent::Bytes(bytes) => {
                        PathMetadataContent::Bytes(bytes)
                    }
                },
            })
            .collect())
    }
}

fn protocol_error() -> AppError {
    AppError::ConfigurationCorrupted {
        message: "invalid WSL Worker path metadata response".to_string(),
    }
}

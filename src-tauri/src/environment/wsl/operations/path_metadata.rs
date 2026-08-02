use tokio::time::Duration;

use crate::core::mutation::CancellationSignal;
use crate::environment::wsl::protocol::{
    decode_nul_records, wsl_operation, WslOperationDescriptor, WslOperationExecutor,
    WslOperationRequest, DEFAULT_WSL_STDERR_LIMIT,
};
use crate::environment::wsl::{WslSession, WslWorkspace};
use crate::error::AppError;

pub(crate) const PATH_METADATA_SCRIPT: &str = include_str!("../scripts/path-metadata.sh");
const PATH_METADATA_OPERATION: WslOperationDescriptor =
    wsl_operation("path-metadata", "inspect", PATH_METADATA_SCRIPT);

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

pub async fn inspect(
    session: &WslSession,
    queries: &[PathMetadataQuery],
    cancellation: Option<CancellationSignal>,
) -> Result<Vec<PathMetadataFact>, AppError> {
    if queries.is_empty() || queries.iter().any(|query| !query.path.starts_with('/')) {
        return Err(AppError::Validation {
            field: Some("pathMetadata.queries".to_string()),
            message: "WSL path metadata requires absolute paths".to_string(),
        });
    }
    let mut args = Vec::with_capacity(queries.len() * 2);
    for query in queries {
        args.push(query.path.clone());
        args.push(if query.inspect_content { "1" } else { "0" }.to_string());
    }
    let output = WslOperationExecutor::execute(
        &PATH_METADATA_OPERATION,
        WslOperationRequest {
            session: session.clone(),
            args,
            stdin: Vec::new(),
            timeout: Duration::from_secs(20),
            stdout_limit: queries.len().saturating_mul(1024 * 1024 + 1024),
            stderr_limit: DEFAULT_WSL_STDERR_LIMIT,
            cancellation,
        },
    )
    .await?;
    let facts = parse_path_metadata(&output.stdout)?;
    if facts.len() != queries.len()
        || facts
            .iter()
            .zip(queries)
            .any(|(fact, query)| fact.path != query.path)
    {
        return Err(protocol_error());
    }
    Ok(facts)
}

impl WslWorkspace {
    pub(crate) async fn inspect_path_metadata(
        &self,
        queries: Vec<PathMetadataQuery>,
        cancellation: Option<CancellationSignal>,
    ) -> Result<Vec<PathMetadataFact>, AppError> {
        self.with_session_retry(move |session| {
            let queries = queries.clone();
            let cancellation = cancellation.clone();
            async move { inspect(&session, &queries, cancellation).await }
        })
        .await
    }
}

pub fn parse_path_metadata(bytes: &[u8]) -> Result<Vec<PathMetadataFact>, AppError> {
    let records = decode_nul_records(bytes);
    if records.first().map(String::as_str) != Some("1") {
        return Err(protocol_error());
    }
    let mut facts = Vec::new();
    let mut index = 1;
    while index < records.len() {
        if records.get(index).map(String::as_str) != Some("path") || index + 4 >= records.len() {
            return Err(protocol_error());
        }
        let kind = match records[index + 2].as_str() {
            "missing" => PathMetadataKind::Missing,
            "directory" => PathMetadataKind::Directory,
            "symlink-directory" => PathMetadataKind::SymlinkDirectory,
            "symlink-other" => PathMetadataKind::SymlinkOther,
            "other" => PathMetadataKind::Other,
            "broken-link" => PathMetadataKind::BrokenLink,
            "inaccessible" => PathMetadataKind::Inaccessible,
            _ => return Err(protocol_error()),
        };
        let content = match records[index + 3].as_str() {
            "none" => PathMetadataContent::NotRequested,
            "eve-unreadable" => PathMetadataContent::Unreadable,
            "eve-empty" => PathMetadataContent::Empty,
            "eve" => PathMetadataContent::Bytes(records[index + 4].as_bytes().to_vec()),
            _ => return Err(protocol_error()),
        };
        facts.push(PathMetadataFact {
            path: records[index + 1].clone(),
            kind,
            content,
        });
        index += 5;
    }
    Ok(facts)
}

fn protocol_error() -> AppError {
    AppError::ConfigurationCorrupted {
        message: "invalid WSL path metadata protocol response".to_string(),
    }
}

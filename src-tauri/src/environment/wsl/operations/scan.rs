use std::collections::BTreeSet;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::core::mutation::CancellationSignal;
use crate::environment::wsl::operations::source_acquisition::WslNativeSource;
use crate::environment::wsl::WslWorkspace;
use crate::error::AppError;

const SCAN_RESPONSE_METADATA_ALLOWANCE: usize = 4 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanRequest {
    pub roots: Vec<String>,
    #[serde(default)]
    pub stat_only_root_indexes: BTreeSet<u32>,
    #[serde(default)]
    pub recursive: bool,
    pub per_file_limit: u32,
    pub aggregate_limit: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ScannedEntryKind {
    Missing,
    File,
    Directory,
    Symlink,
    Other,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScannedEntry {
    pub root_index: u32,
    pub relative_path: String,
    pub kind: ScannedEntryKind,
    pub resolved_target: Option<String>,
    pub size: u64,
    pub mode: u32,
    pub modified_seconds: i64,
    pub content_bytes: Vec<u8>,
    pub truncated: bool,
    pub error_code: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanResponse {
    pub entries: Vec<ScannedEntry>,
    pub root_count: u32,
    pub total_content_bytes: u32,
}

pub async fn scan(
    workspace: &WslWorkspace,
    source: &WslNativeSource,
    request: ScanRequest,
    cancellation: Option<CancellationSignal>,
) -> Result<ScanResponse, AppError> {
    execute_scan(
        workspace,
        source,
        request,
        environment_protocol::SourceScanMode::Recursive,
        cancellation,
    )
    .await
}

pub async fn scan_priority_directories(
    workspace: &WslWorkspace,
    source: &WslNativeSource,
    request: ScanRequest,
    cancellation: Option<CancellationSignal>,
) -> Result<ScanResponse, AppError> {
    if request.recursive {
        return Err(AppError::Validation {
            field: Some("scanRequest.recursive".to_string()),
            message: "priority directory scan must not enable recursive mode".to_string(),
        });
    }
    execute_scan(
        workspace,
        source,
        request,
        environment_protocol::SourceScanMode::PriorityDirectories,
        cancellation,
    )
    .await
}

async fn execute_scan(
    workspace: &WslWorkspace,
    source: &WslNativeSource,
    request: ScanRequest,
    mode: environment_protocol::SourceScanMode,
    cancellation: Option<CancellationSignal>,
) -> Result<ScanResponse, AppError> {
    validate_request(&request)?;
    let handle = source.handle();
    let roots = request
        .roots
        .iter()
        .enumerate()
        .map(|(index, root)| {
            Ok(environment_protocol::SourceScanRoot {
                relative_path: relative_source_path(source.native_root(), root)?.into_bytes(),
                stat_only: request
                    .stat_only_root_indexes
                    .contains(&u32::try_from(index).unwrap_or(u32::MAX)),
            })
        })
        .collect::<Result<Vec<_>, AppError>>()?;
    let response: environment_protocol::SourceScanResponse = workspace
        .request_worker_payload_for_generation(
            handle.generation,
            environment_protocol::Message::ScanSource {
                request: environment_protocol::SourceScanRequest {
                    source_id: handle.id,
                    roots,
                    mode,
                    per_file_limit: request.per_file_limit,
                    aggregate_limit: request.aggregate_limit,
                    deadline_millis: 30_000,
                },
            },
            usize::try_from(request.aggregate_limit)
                .unwrap_or(usize::MAX)
                .saturating_add(SCAN_RESPONSE_METADATA_ALLOWANCE),
            cancellation,
            Duration::from_secs(35),
        )
        .await?;
    Ok(ScanResponse {
        entries: response
            .entries
            .into_iter()
            .map(map_entry)
            .collect::<Result<_, _>>()?,
        root_count: u32::try_from(request.roots.len()).unwrap_or(u32::MAX),
        total_content_bytes: response.total_content_bytes,
    })
}

fn relative_source_path(source_root: &str, requested: &str) -> Result<String, AppError> {
    if requested == source_root {
        return Ok(String::new());
    }
    requested
        .strip_prefix(source_root)
        .and_then(|relative| relative.strip_prefix('/'))
        .filter(|relative| {
            !relative
                .split('/')
                .any(|component| component.is_empty() || matches!(component, "." | ".."))
        })
        .map(str::to_string)
        .ok_or_else(|| AppError::UnsafePath {
            path: requested.to_string(),
            reason: "WSL scan root is outside its Source handle".to_string(),
        })
}

fn map_entry(entry: environment_protocol::SourceEntry) -> Result<ScannedEntry, AppError> {
    Ok(ScannedEntry {
        root_index: entry.root_index,
        relative_path: String::from_utf8(entry.relative_path).map_err(|_| protocol_error())?,
        kind: match entry.kind {
            environment_protocol::SourceEntryKind::Missing => ScannedEntryKind::Missing,
            environment_protocol::SourceEntryKind::File => ScannedEntryKind::File,
            environment_protocol::SourceEntryKind::Directory => ScannedEntryKind::Directory,
            environment_protocol::SourceEntryKind::Symlink => ScannedEntryKind::Symlink,
            environment_protocol::SourceEntryKind::Other => ScannedEntryKind::Other,
        },
        resolved_target: entry
            .link_target
            .map(String::from_utf8)
            .transpose()
            .map_err(|_| protocol_error())?,
        size: 0,
        mode: 0,
        modified_seconds: 0,
        content_bytes: entry.content_bytes,
        truncated: entry.truncated,
        error_code: entry.error_code.map(|error| {
            match error {
                environment_protocol::SourceEntryErrorCode::PathUnavailable => "pathUnavailable",
                environment_protocol::SourceEntryErrorCode::ReadFailed => "readFailed",
                environment_protocol::SourceEntryErrorCode::ReadLinkFailed => "readLinkFailed",
            }
            .to_string()
        }),
    })
}

fn validate_request(request: &ScanRequest) -> Result<(), AppError> {
    if request.roots.is_empty()
        || request.per_file_limit == 0
        || request.aggregate_limit == 0
        || request.per_file_limit > request.aggregate_limit
        || request
            .stat_only_root_indexes
            .iter()
            .any(|index| usize::try_from(*index).unwrap_or(usize::MAX) >= request.roots.len())
    {
        return Err(AppError::Validation {
            field: Some("scanRequest".to_string()),
            message: "invalid WSL scan request".to_string(),
        });
    }
    Ok(())
}

fn protocol_error() -> AppError {
    AppError::ConfigurationCorrupted {
        message: "WSL Worker scan response contains a non-UTF-8 path".to_string(),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn scan_roots_must_belong_to_the_source_handle() {
        assert_eq!(
            super::relative_source_path("/home/alice/repo", "/home/alice/repo/skills/demo")
                .unwrap(),
            "skills/demo"
        );
        assert!(super::relative_source_path("/home/alice/repo", "/home/alice/other").is_err());
        assert!(
            super::relative_source_path("/home/alice/repo", "/home/alice/repo/../other").is_err()
        );
    }
}

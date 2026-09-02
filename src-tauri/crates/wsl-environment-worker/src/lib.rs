use std::fs::File;
use std::io::{self, Read};
use std::path::Path;

#[cfg(target_os = "linux")]
use environment_engine::inspection::{
    self as engine_inspection, EntryKind as EngineEntryKind, ErrorCode as EngineErrorCode,
    InspectionError as EngineInspectionError,
};
#[cfg(target_os = "linux")]
use environment_engine::{
    directory as engine_directory, document as engine_document, entry as engine_entry,
    manifest as engine_manifest, path as engine_path, projection as engine_projection,
};
#[cfg(target_os = "linux")]
use environment_protocol::{
    DirectoryCountFact, DocumentReadFact, DocumentReadState, EntryFact, EntryFactKind,
    EntryMetadata, InspectionEntryKind, InspectionErrorCode, InspectionFact, ManifestRecord,
    ManifestRecordKind, PathMetadataContent, PathMetadataFact, PathMetadataKind, ProjectedTarget,
    MAX_DIRECTORY_COUNT_LIMIT, MAX_DOCUMENT_BYTES, MAX_INSPECTION_CONTENT_BYTES,
    MAX_INSPECTION_FACTS, MAX_INSPECTION_ROOTS, MAX_MANIFEST_RECORDS,
    MAX_PATH_CONTENT_BYTES_PER_FILE, MAX_REQUEST_DEADLINE_MILLIS,
};
use environment_protocol::{
    DirectoryCountRequest, DirectoryCountResponse, DirectoryListRequest, DirectoryListResponse,
    DocumentReadRequest, DocumentReadResponse, EntryFactsRequest, EntryFactsResponse,
    InspectionRequest, InspectionResponse, ManifestRequest, ManifestResponse,
    MapWindowsPathsRequest, MapWindowsPathsResponse, Message, PathKind, PathMetadataRequest,
    PathMetadataResponse, ProjectionRequest, ProjectionResponse,
};
use sha2::{Digest, Sha256};

pub mod inbound_transfer;
pub mod library;
pub mod mutation;
pub mod payload;
pub mod source;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkerIdentity {
    pub distro: String,
    pub user: String,
    pub uid: u32,
    pub home: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dispatch {
    pub response: Option<Message>,
    pub close: bool,
}

pub struct WorkerRuntime {
    build_id: String,
    identity: WorkerIdentity,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestError {
    pub code: &'static str,
    pub phase: &'static str,
}

impl WorkerRuntime {
    pub fn new(build_id: String, identity: WorkerIdentity) -> Self {
        Self { build_id, identity }
    }

    pub fn dispatch(&self, message: Message) -> Dispatch {
        match message {
            Message::Handshake { build_id } if build_id == self.build_id => Dispatch {
                response: Some(Message::HandshakeResult {
                    build_id: self.build_id.clone(),
                    distro: self.identity.distro.clone(),
                    user: self.identity.user.clone(),
                    uid: self.identity.uid,
                    home: self.identity.home.clone(),
                }),
                close: false,
            },
            Message::Handshake { .. } => Dispatch {
                response: Some(error("buildMismatch", "handshake")),
                close: true,
            },
            Message::ObservePath { path } => match execute_path_observation(&path) {
                Ok(kind) => Dispatch {
                    response: Some(Message::PathObserved { kind }),
                    close: false,
                },
                Err(error) => Dispatch {
                    response: Some(error_message(error)),
                    close: false,
                },
            },
            Message::Shutdown => Dispatch {
                response: None,
                close: true,
            },
            _ => Dispatch {
                response: Some(error("unexpectedMessage", "request")),
                close: true,
            },
        }
    }
}

pub fn file_sha256(path: &Path) -> io::Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("sha256:{:x}", hasher.finalize()))
}

pub fn execute_path_observation(path: &str) -> Result<PathKind, RequestError> {
    let path = Path::new(path);
    if !path.is_absolute() {
        return Err(RequestError {
            code: "invalidPath",
            phase: "request",
        });
    }
    Ok(observe_path(path))
}

pub fn error_message(request_error: RequestError) -> Message {
    error(request_error.code, request_error.phase)
}

#[cfg(target_os = "linux")]
pub fn execute_inspection<F>(
    request: InspectionRequest,
    is_cancelled: F,
) -> Result<InspectionResponse, RequestError>
where
    F: Fn() -> bool,
{
    use std::os::unix::ffi::OsStrExt;

    validate_inspection_request(&request)?;
    let snapshot = engine_inspection::inspect_with_cancel(
        &engine_inspection::InspectionRequest {
            roots: request
                .roots
                .into_iter()
                .map(|root| engine_inspection::InspectionRoot {
                    path: root.path.into(),
                    stat_only: root.stat_only,
                })
                .collect(),
            per_file_limit: request.per_file_limit,
            aggregate_limit: request.aggregate_limit,
        },
        is_cancelled,
    )
    .map_err(|error| RequestError {
        code: match error {
            EngineInspectionError::Cancelled => "cancelled",
            EngineInspectionError::InvalidRequest => "invalidRequest",
            EngineInspectionError::UnsupportedPlatform => "unsupportedPlatform",
        },
        phase: "inspection",
    })?;
    if snapshot.facts.len() > MAX_INSPECTION_FACTS {
        return Err(RequestError {
            code: "resultTooLarge",
            phase: "inspection",
        });
    }

    Ok(InspectionResponse {
        facts: snapshot
            .facts
            .into_iter()
            .map(|fact| InspectionFact {
                root_index: fact.root_index,
                relative_path: fact.relative_path.as_os_str().as_bytes().to_vec(),
                kind: match fact.kind {
                    EngineEntryKind::Missing => InspectionEntryKind::Missing,
                    EngineEntryKind::File => InspectionEntryKind::File,
                    EngineEntryKind::Directory => InspectionEntryKind::Directory,
                    EngineEntryKind::Symlink => InspectionEntryKind::Symlink,
                    EngineEntryKind::Other => InspectionEntryKind::Other,
                },
                resolved_target: fact
                    .resolved_target
                    .map(|target| target.as_os_str().as_bytes().to_vec()),
                content_bytes: fact.content_bytes,
                truncated: fact.truncated,
                error_code: fact.error_code.map(|code| match code {
                    EngineErrorCode::PathUnavailable => InspectionErrorCode::PathUnavailable,
                    EngineErrorCode::ReadFailed => InspectionErrorCode::ReadFailed,
                    EngineErrorCode::ReadLinkFailed => InspectionErrorCode::ReadLinkFailed,
                }),
            })
            .collect(),
        total_content_bytes: snapshot.total_content_bytes,
    })
}

#[cfg(target_os = "linux")]
pub fn execute_path_metadata<F>(
    request: PathMetadataRequest,
    is_cancelled: F,
) -> Result<PathMetadataResponse, RequestError>
where
    F: Fn() -> bool,
{
    if request.queries.is_empty()
        || request.queries.len() > MAX_INSPECTION_ROOTS
        || request
            .queries
            .iter()
            .any(|query| !Path::new(&query.path).is_absolute())
        || request.queries.iter().any(|query| {
            query
                .content_limit
                .is_some_and(|limit| limit == 0 || limit > MAX_PATH_CONTENT_BYTES_PER_FILE)
        })
        || request.aggregate_content_limit == 0
        || request.aggregate_content_limit > MAX_INSPECTION_CONTENT_BYTES
        || request.deadline_millis == 0
        || request.deadline_millis > MAX_REQUEST_DEADLINE_MILLIS
    {
        return Err(RequestError {
            code: "invalidRequest",
            phase: "pathMetadata",
        });
    }
    let response = engine_path::inspect_paths_with_cancel(
        &engine_path::PathRequest {
            queries: request
                .queries
                .into_iter()
                .map(|query| engine_path::PathQuery {
                    path: query.path.into(),
                    content_limit: query.content_limit,
                })
                .collect(),
            aggregate_content_limit: request.aggregate_content_limit,
        },
        is_cancelled,
    )
    .map_err(|error| RequestError {
        code: match error {
            engine_path::PathError::Cancelled => "cancelled",
            engine_path::PathError::InvalidRequest => "invalidRequest",
            engine_path::PathError::UnsupportedPlatform => "unsupportedPlatform",
        },
        phase: "pathMetadata",
    })?;
    Ok(PathMetadataResponse {
        facts: response
            .facts
            .into_iter()
            .map(|fact| PathMetadataFact {
                path: fact.path.to_string_lossy().into_owned(),
                kind: match fact.kind {
                    engine_path::PathKind::Missing => PathMetadataKind::Missing,
                    engine_path::PathKind::Directory => PathMetadataKind::Directory,
                    engine_path::PathKind::SymlinkDirectory => PathMetadataKind::SymlinkDirectory,
                    engine_path::PathKind::SymlinkOther => PathMetadataKind::SymlinkOther,
                    engine_path::PathKind::Other => PathMetadataKind::Other,
                    engine_path::PathKind::BrokenLink => PathMetadataKind::BrokenLink,
                    engine_path::PathKind::Inaccessible => PathMetadataKind::Inaccessible,
                },
                content: match fact.content {
                    engine_path::ContentState::NotRequested => PathMetadataContent::NotRequested,
                    engine_path::ContentState::Empty => PathMetadataContent::Empty,
                    engine_path::ContentState::Unreadable => PathMetadataContent::Unreadable,
                    engine_path::ContentState::Bytes(bytes) => PathMetadataContent::Bytes(bytes),
                },
                content_truncated: fact.content_truncated,
            })
            .collect(),
        total_content_bytes: response.total_content_bytes,
    })
}

#[cfg(not(target_os = "linux"))]
pub fn execute_path_metadata<F>(
    _request: PathMetadataRequest,
    _is_cancelled: F,
) -> Result<PathMetadataResponse, RequestError>
where
    F: Fn() -> bool,
{
    Err(RequestError {
        code: "unsupportedPlatform",
        phase: "pathMetadata",
    })
}

#[cfg(target_os = "linux")]
pub fn execute_directory_count<F>(
    request: DirectoryCountRequest,
    is_cancelled: F,
) -> Result<DirectoryCountResponse, RequestError>
where
    F: Fn() -> bool,
{
    if request.paths.is_empty()
        || request.paths.len() > MAX_INSPECTION_ROOTS
        || request
            .paths
            .iter()
            .any(|path| !Path::new(path).is_absolute())
        || request.limit == 0
        || request.limit > MAX_DIRECTORY_COUNT_LIMIT
        || request.deadline_millis == 0
        || request.deadline_millis > MAX_REQUEST_DEADLINE_MILLIS
    {
        return Err(RequestError {
            code: "invalidRequest",
            phase: "directoryCount",
        });
    }
    let response = engine_directory::count_entries_with_cancel(
        &engine_directory::DirectoryCountRequest {
            paths: request.paths.into_iter().map(Into::into).collect(),
            limit: request.limit,
        },
        is_cancelled,
    )
    .map_err(|error| RequestError {
        code: match error {
            engine_directory::DirectoryCountError::Cancelled => "cancelled",
            engine_directory::DirectoryCountError::InvalidRequest => "invalidRequest",
            engine_directory::DirectoryCountError::UnsupportedPlatform => "unsupportedPlatform",
        },
        phase: "directoryCount",
    })?;
    Ok(DirectoryCountResponse {
        facts: response
            .facts
            .into_iter()
            .map(|fact| DirectoryCountFact {
                path: fact.path.to_string_lossy().into_owned(),
                observed_count: fact.observed_count,
                truncated: fact.truncated,
            })
            .collect(),
    })
}

#[cfg(target_os = "linux")]
pub fn execute_document_read<F>(
    request: DocumentReadRequest,
    is_cancelled: F,
) -> Result<DocumentReadResponse, RequestError>
where
    F: Fn() -> bool,
{
    if request.queries.is_empty()
        || request.queries.len() > MAX_INSPECTION_ROOTS
        || request.queries.iter().any(|query| {
            !Path::new(&query.path).is_absolute()
                || query.limit == 0
                || query.limit > MAX_DOCUMENT_BYTES
        })
        || request.aggregate_limit == 0
        || request.aggregate_limit > MAX_DOCUMENT_BYTES
        || request.deadline_millis == 0
        || request.deadline_millis > MAX_REQUEST_DEADLINE_MILLIS
    {
        return Err(RequestError {
            code: "invalidRequest",
            phase: "documentRead",
        });
    }
    let response = engine_document::read_documents_with_cancel(
        &engine_document::DocumentRequest {
            queries: request
                .queries
                .into_iter()
                .map(|query| engine_document::DocumentQuery {
                    path: query.path.into(),
                    limit: query.limit,
                })
                .collect(),
            aggregate_limit: request.aggregate_limit,
        },
        is_cancelled,
    )
    .map_err(|error| RequestError {
        code: match error {
            engine_document::DocumentError::Cancelled => "cancelled",
            engine_document::DocumentError::InvalidRequest => "invalidRequest",
            engine_document::DocumentError::UnsupportedPlatform => "unsupportedPlatform",
        },
        phase: "documentRead",
    })?;
    Ok(DocumentReadResponse {
        facts: response
            .facts
            .into_iter()
            .map(|fact| DocumentReadFact {
                path: fact.path.to_string_lossy().into_owned(),
                state: match fact.state {
                    engine_document::DocumentState::Missing => DocumentReadState::Missing,
                    engine_document::DocumentState::NotFile => DocumentReadState::NotFile,
                    engine_document::DocumentState::Unreadable => DocumentReadState::Unreadable,
                    engine_document::DocumentState::Bytes(bytes) => DocumentReadState::Bytes(bytes),
                },
                truncated: fact.truncated,
            })
            .collect(),
        total_content_bytes: response.total_content_bytes,
    })
}

#[cfg(target_os = "linux")]
pub fn execute_directory_list<F>(
    request: DirectoryListRequest,
    is_cancelled: F,
) -> Result<DirectoryListResponse, RequestError>
where
    F: Fn() -> bool,
{
    use std::os::unix::ffi::OsStrExt;

    if !Path::new(&request.path).is_absolute()
        || request.limit == 0
        || request.limit > MAX_DIRECTORY_COUNT_LIMIT
        || request.deadline_millis == 0
        || request.deadline_millis > MAX_REQUEST_DEADLINE_MILLIS
    {
        return Err(RequestError {
            code: "invalidRequest",
            phase: "directoryList",
        });
    }
    let response = engine_directory::list_child_directories_with_cancel(
        &engine_directory::DirectoryListRequest {
            path: request.path.into(),
            limit: request.limit,
        },
        is_cancelled,
    )
    .map_err(|error| RequestError {
        code: match error {
            engine_directory::DirectoryCountError::Cancelled => "cancelled",
            engine_directory::DirectoryCountError::InvalidRequest => "invalidRequest",
            engine_directory::DirectoryCountError::UnsupportedPlatform => "unsupportedPlatform",
        },
        phase: "directoryList",
    })?;
    Ok(DirectoryListResponse {
        names: response
            .names
            .into_iter()
            .map(|name| name.as_os_str().as_bytes().to_vec())
            .collect(),
        truncated: response.truncated,
    })
}

#[cfg(target_os = "linux")]
pub fn execute_map_windows_paths<F>(
    request: MapWindowsPathsRequest,
    is_cancelled: F,
) -> Result<MapWindowsPathsResponse, RequestError>
where
    F: Fn() -> bool,
{
    if request.paths.is_empty()
        || request.paths.len() > MAX_INSPECTION_ROOTS
        || request
            .paths
            .iter()
            .any(|path| !Path::new(path).is_absolute())
        || request.deadline_millis == 0
        || request.deadline_millis > MAX_REQUEST_DEADLINE_MILLIS
    {
        return Err(RequestError {
            code: "invalidRequest",
            phase: "pathMapping",
        });
    }
    let mut mapped = Vec::with_capacity(request.paths.len());
    for path in request.paths {
        if is_cancelled() {
            return Err(RequestError {
                code: "cancelled",
                phase: "pathMapping",
            });
        }
        mapped.push(map_path_to_windows(Path::new(&path)));
    }
    Ok(MapWindowsPathsResponse { mapped })
}

#[cfg(target_os = "linux")]
pub fn execute_entry_facts<F>(
    request: EntryFactsRequest,
    is_cancelled: F,
) -> Result<EntryFactsResponse, RequestError>
where
    F: Fn() -> bool,
{
    use std::os::unix::ffi::OsStrExt;

    validate_paths(&request.paths, request.deadline_millis, "entryFacts")?;
    let response = engine_entry::inspect_entries_with_cancel(
        &engine_entry::EntryRequest {
            paths: request.paths.into_iter().map(Into::into).collect(),
        },
        is_cancelled,
    )
    .map_err(|error| planning_error(entry_error_code(error), "entryFacts"))?;
    Ok(EntryFactsResponse {
        facts: response
            .facts
            .into_iter()
            .map(|fact| EntryFact {
                kind: match fact.kind {
                    engine_entry::EntryKind::Missing => EntryFactKind::Missing,
                    engine_entry::EntryKind::File => EntryFactKind::File,
                    engine_entry::EntryKind::Directory => EntryFactKind::Directory,
                    engine_entry::EntryKind::Symlink => EntryFactKind::Symlink,
                    engine_entry::EntryKind::BrokenLink => EntryFactKind::BrokenLink,
                    engine_entry::EntryKind::Other => EntryFactKind::Other,
                },
                metadata: fact.metadata.map(|metadata| EntryMetadata {
                    device: metadata.device,
                    inode: metadata.inode,
                    mode: metadata.mode,
                    size: metadata.size,
                    mtime_seconds: metadata.mtime_seconds,
                    mtime_nanos: metadata.mtime_nanos,
                }),
                link_target: fact
                    .link_target
                    .map(|target| target.as_os_str().as_bytes().to_vec()),
            })
            .collect(),
    })
}

#[cfg(target_os = "linux")]
pub fn execute_projection<F>(
    request: ProjectionRequest,
    is_cancelled: F,
) -> Result<ProjectionResponse, RequestError>
where
    F: Fn() -> bool,
{
    use std::os::unix::ffi::OsStrExt;
    validate_paths(&request.destinations, request.deadline_millis, "projection")?;
    let response = engine_projection::project_targets_with_cancel(
        &engine_projection::ProjectionRequest {
            destinations: request.destinations.into_iter().map(Into::into).collect(),
        },
        &is_cancelled,
    )
    .map_err(|error| planning_error(projection_error_code(error), "projection"))?;
    let mut targets = Vec::with_capacity(response.targets.len());
    for target in response.targets {
        if is_cancelled() {
            return Err(planning_error("cancelled", "projection"));
        }
        let storage_projection = map_path_to_windows(&target.physical_anchor)
            .ok_or_else(|| planning_error("pathMappingFailed", "projection"))?;
        targets.push(ProjectedTarget {
            anchor_device: target.anchor_device,
            anchor_inode: target.anchor_inode,
            physical_destination: target.physical_destination.as_os_str().as_bytes().to_vec(),
            relative_components: target
                .relative_components
                .into_iter()
                .map(|component| component.as_os_str().as_bytes().to_vec())
                .collect(),
            storage_projection,
        });
    }
    Ok(ProjectionResponse { targets })
}

#[cfg(target_os = "linux")]
pub fn execute_manifest<F>(
    request: ManifestRequest,
    is_cancelled: F,
) -> Result<ManifestResponse, RequestError>
where
    F: Fn() -> bool,
{
    use std::os::unix::ffi::OsStrExt;

    validate_paths(
        std::slice::from_ref(&request.root),
        request.deadline_millis,
        "manifest",
    )?;
    let response = engine_manifest::build_manifest_with_cancel(
        &engine_manifest::ManifestRequest {
            root: request.root.into(),
        },
        is_cancelled,
    )
    .map_err(|error| planning_error(manifest_error_code(error), "manifest"))?;
    if response.records.len() > MAX_MANIFEST_RECORDS {
        return Err(planning_error("resultTooLarge", "manifest"));
    }
    Ok(ManifestResponse {
        records: response
            .records
            .into_iter()
            .map(|record| ManifestRecord {
                relative_path: record.relative_path.as_os_str().as_bytes().to_vec(),
                kind: match record.kind {
                    engine_manifest::ManifestKind::Directory => ManifestRecordKind::Directory,
                    engine_manifest::ManifestKind::File => ManifestRecordKind::File,
                    engine_manifest::ManifestKind::Symlink => ManifestRecordKind::Symlink,
                },
                digest: record.digest,
                executable: record.executable,
                symlink_target: record
                    .symlink_target
                    .map(|target| target.as_os_str().as_bytes().to_vec()),
            })
            .collect(),
    })
}

#[cfg(target_os = "linux")]
fn validate_paths(
    paths: &[String],
    deadline: u64,
    phase: &'static str,
) -> Result<(), RequestError> {
    if paths.is_empty()
        || paths.len() > MAX_INSPECTION_ROOTS
        || paths.iter().any(|path| !Path::new(path).is_absolute())
        || deadline == 0
        || deadline > MAX_REQUEST_DEADLINE_MILLIS
    {
        return Err(planning_error("invalidRequest", phase));
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn entry_error_code(error: engine_entry::EntryError) -> &'static str {
    match error {
        engine_entry::EntryError::UnsupportedPlatform => "unsupportedPlatform",
        engine_entry::EntryError::InvalidRequest => "invalidRequest",
        engine_entry::EntryError::Unavailable => "pathUnavailable",
        engine_entry::EntryError::Cancelled => "cancelled",
    }
}

#[cfg(target_os = "linux")]
fn projection_error_code(error: engine_projection::ProjectionError) -> &'static str {
    match error {
        engine_projection::ProjectionError::UnsupportedPlatform => "unsupportedPlatform",
        engine_projection::ProjectionError::InvalidRequest => "invalidRequest",
        engine_projection::ProjectionError::Unavailable => "pathUnavailable",
        engine_projection::ProjectionError::Cancelled => "cancelled",
    }
}

#[cfg(target_os = "linux")]
fn manifest_error_code(error: engine_manifest::ManifestError) -> &'static str {
    match error {
        engine_manifest::ManifestError::UnsupportedPlatform => "unsupportedPlatform",
        engine_manifest::ManifestError::InvalidRequest => "invalidRequest",
        engine_manifest::ManifestError::Unavailable => "pathUnavailable",
        engine_manifest::ManifestError::UnsupportedEntry => "unsupportedEntry",
        engine_manifest::ManifestError::Cancelled => "cancelled",
    }
}

fn planning_error(code: &'static str, phase: &'static str) -> RequestError {
    RequestError { code, phase }
}

#[cfg(target_os = "linux")]
#[allow(
    clippy::disallowed_methods,
    reason = "Environment Worker 是独立 crate，必须直接构造受控的 wslpath 子进程"
)]
fn map_path_to_windows(path: &Path) -> Option<String> {
    use std::process::{Command, Stdio};

    let output = Command::new("wslpath")
        .arg("-w")
        .arg("--")
        .arg(path)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() || output.stdout.len() > 16 * 1024 {
        return None;
    }
    String::from_utf8(output.stdout)
        .ok()
        .map(|value| value.trim_end_matches(['\r', '\n']).to_string())
        .filter(|value| !value.is_empty())
}

#[cfg(not(target_os = "linux"))]
pub fn execute_entry_facts<F>(
    _request: EntryFactsRequest,
    _is_cancelled: F,
) -> Result<EntryFactsResponse, RequestError>
where
    F: Fn() -> bool,
{
    Err(planning_error("unsupportedPlatform", "entryFacts"))
}

#[cfg(not(target_os = "linux"))]
pub fn execute_projection<F>(
    _request: ProjectionRequest,
    _is_cancelled: F,
) -> Result<ProjectionResponse, RequestError>
where
    F: Fn() -> bool,
{
    Err(planning_error("unsupportedPlatform", "projection"))
}

#[cfg(not(target_os = "linux"))]
pub fn execute_manifest<F>(
    _request: ManifestRequest,
    _is_cancelled: F,
) -> Result<ManifestResponse, RequestError>
where
    F: Fn() -> bool,
{
    Err(planning_error("unsupportedPlatform", "manifest"))
}

#[cfg(not(target_os = "linux"))]
pub fn execute_map_windows_paths<F>(
    _request: MapWindowsPathsRequest,
    _is_cancelled: F,
) -> Result<MapWindowsPathsResponse, RequestError>
where
    F: Fn() -> bool,
{
    Err(RequestError {
        code: "unsupportedPlatform",
        phase: "pathMapping",
    })
}

#[cfg(not(target_os = "linux"))]
pub fn execute_directory_list<F>(
    _request: DirectoryListRequest,
    _is_cancelled: F,
) -> Result<DirectoryListResponse, RequestError>
where
    F: Fn() -> bool,
{
    Err(RequestError {
        code: "unsupportedPlatform",
        phase: "directoryList",
    })
}

#[cfg(not(target_os = "linux"))]
pub fn execute_document_read<F>(
    _request: DocumentReadRequest,
    _is_cancelled: F,
) -> Result<DocumentReadResponse, RequestError>
where
    F: Fn() -> bool,
{
    Err(RequestError {
        code: "unsupportedPlatform",
        phase: "documentRead",
    })
}

#[cfg(not(target_os = "linux"))]
pub fn execute_directory_count<F>(
    _request: DirectoryCountRequest,
    _is_cancelled: F,
) -> Result<DirectoryCountResponse, RequestError>
where
    F: Fn() -> bool,
{
    Err(RequestError {
        code: "unsupportedPlatform",
        phase: "directoryCount",
    })
}

#[cfg(not(target_os = "linux"))]
pub fn execute_inspection<F>(
    _request: InspectionRequest,
    _is_cancelled: F,
) -> Result<InspectionResponse, RequestError>
where
    F: Fn() -> bool,
{
    Err(RequestError {
        code: "unsupportedPlatform",
        phase: "inspection",
    })
}

#[cfg(target_os = "linux")]
fn validate_inspection_request(request: &InspectionRequest) -> Result<(), RequestError> {
    if request.roots.is_empty()
        || request.roots.len() > MAX_INSPECTION_ROOTS
        || request
            .roots
            .iter()
            .any(|root| !Path::new(&root.path).is_absolute())
        || request.per_file_limit == 0
        || request.aggregate_limit == 0
        || request.per_file_limit > request.aggregate_limit
        || request.aggregate_limit > MAX_INSPECTION_CONTENT_BYTES
        || request.deadline_millis == 0
        || request.deadline_millis > MAX_REQUEST_DEADLINE_MILLIS
    {
        return Err(RequestError {
            code: "invalidRequest",
            phase: "inspection",
        });
    }
    Ok(())
}

fn observe_path(path: &Path) -> PathKind {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => match std::fs::metadata(path) {
            Ok(target) if target.is_dir() => PathKind::SymlinkDirectory,
            Ok(_) => PathKind::SymlinkOther,
            Err(error) if error.kind() == io::ErrorKind::NotFound => PathKind::BrokenLink,
            Err(_) => PathKind::Inaccessible,
        },
        Ok(metadata) if metadata.is_dir() => PathKind::Directory,
        Ok(metadata) if metadata.is_file() => PathKind::File,
        Ok(_) => PathKind::Other,
        Err(error) if error.kind() == io::ErrorKind::NotFound => PathKind::Missing,
        Err(_) => PathKind::Inaccessible,
    }
}

fn error(code: &str, phase: &str) -> Message {
    Message::Error {
        code: code.to_string(),
        phase: phase.to_string(),
        parameters: Vec::new(),
    }
}

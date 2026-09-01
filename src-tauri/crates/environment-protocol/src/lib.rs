use std::io;
use std::sync::Arc;

use bytes::Bytes;
use futures_util::SinkExt;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite};
use tokio::sync::{mpsc, oneshot, Mutex};
use tokio::task::JoinHandle;
use tokio_util::codec::{FramedWrite, LengthDelimitedCodec};

pub const MAX_FRAME_BYTES: usize = 1024 * 1024;
pub const MAX_PAYLOAD_CHUNK_BYTES: usize = 256 * 1024;
pub const MAX_RESPONSE_TRANSFER_BYTES: usize = 36 * 1024 * 1024;
pub const MAX_PAYLOAD_TRANSFER_BYTES: usize = 256 * 1024 * 1024;
pub const MAX_MUTATION_TRANSFER_BYTES: usize = 36 * 1024 * 1024;
pub const MAX_CONCURRENT_READ_REQUESTS: usize = 8;
pub const MAX_PENDING_READ_REQUESTS: usize = 64;
pub const MAX_INSPECTION_ROOTS: usize = 256;
pub const MAX_INSPECTION_FACTS: usize = 65_536;
pub const MAX_INSPECTION_CONTENT_BYTES: u32 = 8 * 1024 * 1024;
pub const MAX_PATH_CONTENT_BYTES_PER_FILE: u32 = 1024 * 1024;
pub const MAX_DIRECTORY_COUNT_LIMIT: u32 = 10_000;
pub const MAX_DOCUMENT_BYTES: u32 = 16 * 1024 * 1024;
pub const MAX_MANIFEST_RECORDS: usize = 262_144;
pub const MAX_REQUEST_DEADLINE_MILLIS: u64 = 120_000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum WireRecord {
    Control(Envelope),
    PayloadChunk {
        transfer_id: u64,
        #[serde(with = "serde_bytes")]
        bytes: Vec<u8>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Envelope {
    pub request_id: u64,
    pub message: Message,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PathKind {
    Missing,
    File,
    Directory,
    SymlinkDirectory,
    SymlinkOther,
    BrokenLink,
    Other,
    Inaccessible,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InspectionRequest {
    pub roots: Vec<InspectionRoot>,
    pub per_file_limit: u32,
    pub aggregate_limit: u32,
    pub deadline_millis: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InspectionRoot {
    pub path: String,
    pub stat_only: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum InspectionEntryKind {
    Missing,
    File,
    Directory,
    Symlink,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum InspectionErrorCode {
    PathUnavailable,
    ReadFailed,
    ReadLinkFailed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InspectionFact {
    pub root_index: u32,
    #[serde(with = "serde_bytes")]
    pub relative_path: Vec<u8>,
    pub kind: InspectionEntryKind,
    pub resolved_target: Option<Vec<u8>>,
    #[serde(with = "serde_bytes")]
    pub content_bytes: Vec<u8>,
    pub truncated: bool,
    pub error_code: Option<InspectionErrorCode>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InspectionResponse {
    pub facts: Vec<InspectionFact>,
    pub total_content_bytes: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PathMetadataRequest {
    pub queries: Vec<PathMetadataQuery>,
    pub aggregate_content_limit: u32,
    pub deadline_millis: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PathMetadataQuery {
    pub path: String,
    pub content_limit: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PathMetadataKind {
    Missing,
    Directory,
    SymlinkDirectory,
    SymlinkOther,
    Other,
    BrokenLink,
    Inaccessible,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PathMetadataContent {
    NotRequested,
    Empty,
    Unreadable,
    Bytes(#[serde(with = "serde_bytes")] Vec<u8>),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PathMetadataFact {
    pub path: String,
    pub kind: PathMetadataKind,
    pub content: PathMetadataContent,
    pub content_truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PathMetadataResponse {
    pub facts: Vec<PathMetadataFact>,
    pub total_content_bytes: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DirectoryCountRequest {
    pub paths: Vec<String>,
    pub limit: u32,
    pub deadline_millis: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DirectoryCountFact {
    pub path: String,
    pub observed_count: Option<u32>,
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DirectoryCountResponse {
    pub facts: Vec<DirectoryCountFact>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocumentReadRequest {
    pub queries: Vec<DocumentReadQuery>,
    pub aggregate_limit: u32,
    pub deadline_millis: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocumentReadQuery {
    pub path: String,
    pub limit: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DocumentReadState {
    Missing,
    NotFile,
    Unreadable,
    Bytes(#[serde(with = "serde_bytes")] Vec<u8>),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocumentReadFact {
    pub path: String,
    pub state: DocumentReadState,
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocumentReadResponse {
    pub facts: Vec<DocumentReadFact>,
    pub total_content_bytes: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocumentWritePreparation {
    pub path: String,
    pub expected_revision: Option<String>,
    pub total_bytes: u64,
    pub sha256: String,
    pub deadline_millis: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocumentRemoveRequest {
    pub path: String,
    pub expected_revision: Option<String>,
    pub deadline_millis: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LibraryCatalogResponse {
    #[serde(with = "serde_bytes")]
    pub bytes: Vec<u8>,
    pub present: bool,
    pub revision: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LibraryOperationPreparation {
    pub total_bytes: u64,
    pub sha256: String,
    pub deadline_millis: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LibraryOperationRequest {
    pub operation_id: String,
    pub expected_catalog_revision: Option<String>,
    #[serde(with = "serde_bytes")]
    pub catalog_bytes: Vec<u8>,
    pub action: LibraryOperationAction,
    pub deadline_millis: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum LibraryOperationAction {
    SaveCatalog {
        library_ids: Vec<String>,
    },
    CommitMember {
        library_id: String,
        skill_name: String,
        expected_anchor_device: u64,
        expected_anchor_inode: u64,
        expected_fingerprint: String,
        expected_content_hash: Option<String>,
        mutation: LibraryMemberAction,
    },
    DeleteLibrary {
        library_id: String,
        expected_anchor_device: u64,
        expected_anchor_inode: u64,
        expected_fingerprint: String,
        expected_content_hash: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum LibraryMemberAction {
    Upsert { payload_id: u64 },
    Delete,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DirectoryListRequest {
    pub path: String,
    pub limit: u32,
    pub deadline_millis: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DirectoryListResponse {
    pub names: Vec<Vec<u8>>,
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MapWindowsPathsRequest {
    pub paths: Vec<String>,
    pub deadline_millis: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MapWindowsPathsResponse {
    pub mapped: Vec<Option<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MapHostPathsRequest {
    pub paths: Vec<String>,
    pub deadline_millis: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MapHostPathsResponse {
    pub mapped: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntryFactsRequest {
    pub paths: Vec<String>,
    pub deadline_millis: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EntryFactKind {
    Missing,
    File,
    Directory,
    Symlink,
    BrokenLink,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntryMetadata {
    pub device: u64,
    pub inode: u64,
    pub mode: u32,
    pub size: u64,
    pub mtime_seconds: i64,
    pub mtime_nanos: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntryFact {
    pub kind: EntryFactKind,
    pub metadata: Option<EntryMetadata>,
    pub link_target: Option<Vec<u8>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntryFactsResponse {
    pub facts: Vec<EntryFact>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectionRequest {
    pub destinations: Vec<String>,
    pub deadline_millis: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectedTarget {
    pub anchor_device: u64,
    pub anchor_inode: u64,
    pub physical_destination: Vec<u8>,
    pub relative_components: Vec<Vec<u8>>,
    pub storage_projection: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectionResponse {
    pub targets: Vec<ProjectedTarget>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManifestRequest {
    pub root: String,
    pub deadline_millis: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ManifestRecordKind {
    Directory,
    File,
    Symlink,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManifestRecord {
    pub relative_path: Vec<u8>,
    pub kind: ManifestRecordKind,
    pub digest: Option<String>,
    pub executable: bool,
    pub symlink_target: Option<Vec<u8>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManifestResponse {
    pub records: Vec<ManifestRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitSourceRequest {
    pub url: String,
    pub git_ref: Option<String>,
    pub proxy: Option<String>,
    pub deadline_millis: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenLocalSourceRequest {
    pub path: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SourceScanMode {
    Recursive,
    PriorityDirectories,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceScanRoot {
    #[serde(with = "serde_bytes")]
    pub relative_path: Vec<u8>,
    pub stat_only: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceScanRequest {
    pub source_id: u64,
    pub roots: Vec<SourceScanRoot>,
    pub mode: SourceScanMode,
    pub per_file_limit: u32,
    pub aggregate_limit: u32,
    pub deadline_millis: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SourceEntryKind {
    Missing,
    File,
    Directory,
    Symlink,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SourceEntryErrorCode {
    PathUnavailable,
    ReadFailed,
    ReadLinkFailed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceEntry {
    pub root_index: u32,
    #[serde(with = "serde_bytes")]
    pub relative_path: Vec<u8>,
    pub kind: SourceEntryKind,
    pub link_target: Option<Vec<u8>>,
    #[serde(with = "serde_bytes")]
    pub content_bytes: Vec<u8>,
    pub truncated: bool,
    pub error_code: Option<SourceEntryErrorCode>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceScanResponse {
    pub entries: Vec<SourceEntry>,
    pub total_content_bytes: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PayloadEntryKind {
    File,
    Directory,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PayloadEntry {
    pub relative_path: String,
    pub kind: PayloadEntryKind,
    pub blob_id: Option<String>,
    pub content_hash: Option<String>,
    pub size: u64,
    pub executable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PayloadManifest {
    pub entries: Vec<PayloadEntry>,
    pub payload_root_hash: String,
    pub payload_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AcquirePayloadFromSourceRequest {
    pub source_id: u64,
    #[serde(with = "serde_bytes")]
    pub relative_path: Vec<u8>,
    pub session_id: String,
    pub payload_name: String,
    pub deadline_millis: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerifyPayloadRequest {
    pub session_id: String,
    pub payload_name: String,
    pub deadline_millis: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PayloadReadyResponse {
    pub payload_id: u64,
    pub manifest: PayloadManifest,
    pub total_bytes: u64,
    pub computed_hash: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PayloadCleanupResponse {
    pub removed_sessions: u32,
    pub protected_sessions: u32,
    pub retained_external_bytes: u64,
    pub cleanup_blocked: bool,
    pub warnings: Vec<PayloadCleanupWarning>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PayloadCleanupWarning {
    pub code: String,
    pub candidate_name: Option<String>,
    pub technical_details: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MutationUnitRequest {
    pub resource_id: String,
    pub operation_id: String,
    pub unit_id: String,
    #[serde(with = "serde_bytes")]
    pub initial_marker_json: Vec<u8>,
    pub entries: Vec<MutationEntry>,
    pub lock: Option<MutationLock>,
    pub deadline_millis: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MutationEntry {
    pub destination: String,
    pub expected_anchor_device: u64,
    pub expected_anchor_inode: u64,
    pub expected_fingerprint: String,
    pub expected_content_hash: Option<String>,
    pub action: MutationEntryAction,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum MutationEntryAction {
    Keep,
    Materialize { payload_id: u64 },
    Symlink { target: String },
    Remove,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MutationLockSchema {
    Global,
    Project,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum MutationLockEntry {
    Replace {
        key: String,
        #[serde(with = "serde_bytes")]
        replacement_json: Vec<u8>,
    },
    Remove {
        key: String,
    },
    MoveAndReplace {
        from: String,
        to: String,
        #[serde(with = "serde_bytes")]
        replacement_json: Vec<u8>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MutationLock {
    pub target: String,
    pub legacy_target: Option<String>,
    pub schema: MutationLockSchema,
    pub entry: MutationLockEntry,
    pub root_replacements_json: std::collections::BTreeMap<String, Vec<u8>>,
    pub expected_entries_json: std::collections::BTreeMap<String, Option<Vec<u8>>>,
    pub expected_roots_json: std::collections::BTreeMap<String, Option<Vec<u8>>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MutationLockReceipt {
    pub entries_json: std::collections::BTreeMap<String, Option<Vec<u8>>>,
    pub roots_json: std::collections::BTreeMap<String, Option<Vec<u8>>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum MutationUnitOutcome {
    Succeeded {
        lock: Option<MutationLockReceipt>,
        cleanup: Option<MutationCleanupToken>,
    },
    Failed {
        code: String,
        phase: String,
        parameters: Vec<(String, String)>,
        message: String,
    },
    Cancelled,
    RecoveryRequired {
        resource_id: String,
        message: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MutationCleanupToken {
    pub resource_id: String,
    pub marker_sha256: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MutationRecoveryState {
    Present,
    Unreadable,
    Unsafe,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MutationRecoveryRecord {
    pub resource_id: String,
    pub managed_root: String,
    pub state: MutationRecoveryState,
    #[serde(with = "serde_bytes")]
    pub marker_bytes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MutationRecoveryList {
    pub records: Vec<MutationRecoveryRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Message {
    Handshake {
        build_id: String,
    },
    HandshakeResult {
        build_id: String,
        distro: String,
        user: String,
        uid: u32,
        home: String,
    },
    Progress {
        current: u32,
        total: u32,
    },
    Cancel {
        target_request_id: u64,
    },
    BeginTransfer {
        transfer_id: u64,
        total_bytes: u64,
        sha256: String,
        owner_request_id: u64,
    },
    TransferCompleted {
        transfer_id: u64,
        total_bytes: u64,
        sha256: String,
    },
    Error {
        code: String,
        phase: String,
        parameters: Vec<(String, String)>,
    },
    Shutdown,
    ObservePath {
        path: String,
    },
    PathObserved {
        kind: PathKind,
    },
    InspectFilesystem {
        request: InspectionRequest,
    },
    InspectPaths {
        request: PathMetadataRequest,
    },
    CountDirectoryEntries {
        request: DirectoryCountRequest,
    },
    ReadDocuments {
        request: DocumentReadRequest,
    },
    PrepareDocumentWrite {
        request: DocumentWritePreparation,
    },
    DocumentWritten {
        revision: String,
    },
    RemoveDocument {
        request: DocumentRemoveRequest,
    },
    DocumentRemoved,
    ReadLibraryCatalog {
        deadline_millis: u64,
    },
    PrepareLibraryOperation {
        request: LibraryOperationPreparation,
    },
    LibraryOperationCompleted {
        catalog_revision: String,
    },
    ListChildDirectories {
        request: DirectoryListRequest,
    },
    MapPathsToWindows {
        request: MapWindowsPathsRequest,
    },
    InspectEntries {
        request: EntryFactsRequest,
    },
    ProjectTargets {
        request: ProjectionRequest,
    },
    BuildManifest {
        request: ManifestRequest,
    },
    AcquireGitSource {
        request: GitSourceRequest,
    },
    OpenLocalSource {
        request: OpenLocalSourceRequest,
    },
    SourceOpened {
        source_id: u64,
        root: String,
        revision: Option<String>,
    },
    ReleaseSource {
        source_id: u64,
    },
    SourceReleased {
        source_id: u64,
    },
    ScanSource {
        request: SourceScanRequest,
    },
    SourceFingerprint {
        source_id: u64,
        #[serde(with = "serde_bytes")]
        relative_path: Vec<u8>,
        deadline_millis: u64,
    },
    SourceFingerprintResult {
        fingerprint: String,
    },
    SourceRevision {
        source_id: u64,
        #[serde(with = "serde_bytes")]
        relative_path: Vec<u8>,
        deadline_millis: u64,
    },
    SourceRevisionResult {
        revision: String,
    },
    ProbeGit {
        request: GitSourceRequest,
    },
    GitProbed {
        revision: String,
    },
    AcquirePayloadFromSource {
        request: AcquirePayloadFromSourceRequest,
    },
    VerifyPayload {
        request: VerifyPayloadRequest,
    },
    ReadPayloadBlob {
        payload_id: u64,
        blob_id: String,
        deadline_millis: u64,
    },
    RemovePayload {
        session_id: String,
        payload_name: String,
    },
    PayloadRemoved {
        session_id: String,
        payload_name: String,
    },
    RemovePayloadSession {
        session_id: String,
    },
    PayloadSessionRemoved {
        session_id: String,
    },
    SweepPayloadOrphans {
        protected_session_ids: Vec<String>,
    },
    BeginPayloadUpload {
        session_id: String,
        payload_name: String,
    },
    PayloadUploadBegun {
        upload_id: u64,
    },
    UploadPayloadBlob {
        upload_id: u64,
        blob_id: String,
        total_bytes: u64,
        sha256: String,
    },
    FinalizePayloadUpload {
        upload_id: u64,
        total_bytes: u64,
        sha256: String,
    },
    TransferReady {
        transfer_id: u64,
    },
    PayloadBlobUploaded {
        upload_id: u64,
        blob_id: String,
    },
    PayloadUploadFinalized {
        payload_id: u64,
    },
    PrepareMutationUnit {
        resource_id: String,
        total_bytes: u64,
        sha256: String,
    },
    MutationAccepted {
        resource_id: String,
    },
    AcknowledgeMutationUnit {
        cleanup: MutationCleanupToken,
    },
    MutationAcknowledged {
        resource_id: String,
    },
    ListMutationRecovery,
    CleanupMutationRecovery {
        resource_id: String,
        #[serde(with = "serde_bytes")]
        expected_marker_json: Vec<u8>,
        backups: Vec<String>,
    },
    MutationRecoveryCleaned {
        resource_id: String,
    },
    MapHostPaths {
        request: MapHostPathsRequest,
    },
}

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum DecodeError {
    #[error("invalid postcard record: {0}")]
    Postcard(#[from] postcard::Error),
    #[error("wire record contains {count} trailing bytes")]
    TrailingBytes { count: usize },
}

pub fn encode(record: &WireRecord) -> Result<Vec<u8>, postcard::Error> {
    postcard::to_stdvec(record)
}

pub fn decode(bytes: &[u8]) -> Result<WireRecord, DecodeError> {
    let (record, remaining) = postcard::take_from_bytes(bytes)?;
    if !remaining.is_empty() {
        return Err(DecodeError::TrailingBytes {
            count: remaining.len(),
        });
    }
    Ok(record)
}

pub fn encode_inspection_response(
    response: &InspectionResponse,
) -> Result<Vec<u8>, postcard::Error> {
    postcard::to_stdvec(response)
}

pub fn encode_payload<T>(value: &T) -> Result<Vec<u8>, postcard::Error>
where
    T: Serialize,
{
    postcard::to_stdvec(value)
}

pub fn decode_payload<T>(bytes: &[u8]) -> Result<T, DecodeError>
where
    T: DeserializeOwned,
{
    let (value, remaining) = postcard::take_from_bytes(bytes)?;
    if !remaining.is_empty() {
        return Err(DecodeError::TrailingBytes {
            count: remaining.len(),
        });
    }
    Ok(value)
}

pub fn decode_inspection_response(bytes: &[u8]) -> Result<InspectionResponse, DecodeError> {
    decode_payload(bytes)
}

pub fn codec() -> LengthDelimitedCodec {
    LengthDelimitedCodec::builder()
        .max_frame_length(MAX_FRAME_BYTES)
        .new_codec()
}

#[derive(Debug, thiserror::Error)]
pub enum WriterError {
    #[error("protocol writer is closed")]
    Closed,
    #[error("failed to encode protocol record: {0}")]
    Encode(#[from] postcard::Error),
    #[error("failed to write protocol record: {0}")]
    Io(#[from] io::Error),
    #[error("binary transfer exceeds its boundary")]
    TransferTooLarge,
    #[error("binary transfer content does not match its declaration")]
    TransferMismatch,
}

#[derive(Clone)]
pub struct ProtocolWriter {
    control: mpsc::Sender<WireRecord>,
    binary: mpsc::Sender<BinaryRecord>,
    transfer_gate: Arc<Mutex<()>>,
}

enum BinaryRecord {
    Record(WireRecord),
    Barrier(WireRecord, oneshot::Sender<()>),
}

impl ProtocolWriter {
    pub async fn send_control(&self, record: WireRecord) -> Result<(), WriterError> {
        self.control
            .send(record)
            .await
            .map_err(|_| WriterError::Closed)
    }

    pub async fn send_binary(&self, record: WireRecord) -> Result<(), WriterError> {
        self.binary
            .send(BinaryRecord::Record(record))
            .await
            .map_err(|_| WriterError::Closed)
    }

    pub async fn send_binary_barrier(&self, record: WireRecord) -> Result<(), WriterError> {
        let (written_tx, written_rx) = oneshot::channel();
        self.binary
            .send(BinaryRecord::Barrier(record, written_tx))
            .await
            .map_err(|_| WriterError::Closed)?;
        written_rx.await.map_err(|_| WriterError::Closed)
    }

    pub async fn send_transfer(
        &self,
        owner_request_id: u64,
        transfer_id: u64,
        payload: &[u8],
    ) -> Result<(), WriterError> {
        self.send_transfer_with_limit(
            owner_request_id,
            transfer_id,
            payload,
            MAX_RESPONSE_TRANSFER_BYTES,
        )
        .await
    }

    pub async fn send_transfer_with_limit(
        &self,
        owner_request_id: u64,
        transfer_id: u64,
        payload: &[u8],
        transfer_limit: usize,
    ) -> Result<(), WriterError> {
        if payload.len() > transfer_limit || payload.len() > MAX_PAYLOAD_TRANSFER_BYTES {
            return Err(WriterError::TransferTooLarge);
        }
        let _guard = self.transfer_gate.lock().await;
        let total_bytes = payload.len() as u64;
        let sha256 = format!("sha256:{:x}", Sha256::digest(payload));
        self.send_binary(WireRecord::Control(Envelope {
            request_id: owner_request_id,
            message: Message::BeginTransfer {
                transfer_id,
                total_bytes,
                sha256: sha256.clone(),
                owner_request_id,
            },
        }))
        .await?;
        for chunk in payload.chunks(MAX_PAYLOAD_CHUNK_BYTES) {
            self.send_binary(WireRecord::PayloadChunk {
                transfer_id,
                bytes: chunk.to_vec(),
            })
            .await?;
        }
        self.send_binary_barrier(WireRecord::Control(Envelope {
            request_id: owner_request_id,
            message: Message::TransferCompleted {
                transfer_id,
                total_bytes,
                sha256,
            },
        }))
        .await
    }

    pub async fn send_reader_transfer_with_limit<R>(
        &self,
        owner_request_id: u64,
        transfer_id: u64,
        mut reader: R,
        total_bytes: u64,
        sha256: String,
        transfer_limit: usize,
    ) -> Result<(), WriterError>
    where
        R: AsyncRead + Unpin,
    {
        if total_bytes > transfer_limit as u64 || total_bytes > MAX_PAYLOAD_TRANSFER_BYTES as u64 {
            return Err(WriterError::TransferTooLarge);
        }
        let _guard = self.transfer_gate.lock().await;
        self.send_binary(WireRecord::Control(Envelope {
            request_id: owner_request_id,
            message: Message::BeginTransfer {
                transfer_id,
                total_bytes,
                sha256: sha256.clone(),
                owner_request_id,
            },
        }))
        .await?;
        let mut hasher = Sha256::new();
        let mut received = 0_u64;
        let mut buffer = vec![0_u8; MAX_PAYLOAD_CHUNK_BYTES];
        while received < total_bytes {
            let remaining = usize::try_from(total_bytes - received)
                .unwrap_or(usize::MAX)
                .min(buffer.len());
            let read = reader.read(&mut buffer[..remaining]).await?;
            if read == 0 {
                return Err(WriterError::TransferMismatch);
            }
            received += read as u64;
            hasher.update(&buffer[..read]);
            self.send_binary(WireRecord::PayloadChunk {
                transfer_id,
                bytes: buffer[..read].to_vec(),
            })
            .await?;
        }
        let mut extra = [0_u8; 1];
        if reader.read(&mut extra).await? != 0
            || format!("sha256:{:x}", hasher.finalize()) != sha256
        {
            return Err(WriterError::TransferMismatch);
        }
        self.send_binary_barrier(WireRecord::Control(Envelope {
            request_id: owner_request_id,
            message: Message::TransferCompleted {
                transfer_id,
                total_bytes,
                sha256,
            },
        }))
        .await
    }
}

pub fn spawn_writer<W>(output: W) -> (ProtocolWriter, JoinHandle<Result<(), WriterError>>)
where
    W: AsyncWrite + Unpin + Send + 'static,
{
    let (control_tx, mut control_rx) = mpsc::channel(64);
    let (binary_tx, mut binary_rx) = mpsc::channel(8);
    let task = tokio::spawn(async move {
        let mut sink = FramedWrite::new(output, codec());
        let mut control_open = true;
        let mut binary_open = true;
        let mut control_first = true;
        while control_open || binary_open {
            let selected = if control_first {
                tokio::select! {
                    biased;
                    value = control_rx.recv(), if control_open => match value {
                        Some(record) => Some((record, None)),
                        None => { control_open = false; None }
                    },
                    value = binary_rx.recv(), if binary_open => match value {
                        Some(BinaryRecord::Record(record)) => Some((record, None)),
                        Some(BinaryRecord::Barrier(record, written)) => Some((record, Some(written))),
                        None => { binary_open = false; None }
                    },
                }
            } else {
                tokio::select! {
                    biased;
                    value = binary_rx.recv(), if binary_open => match value {
                        Some(BinaryRecord::Record(record)) => Some((record, None)),
                        Some(BinaryRecord::Barrier(record, written)) => Some((record, Some(written))),
                        None => { binary_open = false; None }
                    },
                    value = control_rx.recv(), if control_open => match value {
                        Some(record) => Some((record, None)),
                        None => { control_open = false; None }
                    },
                }
            };
            let Some((record, written)) = selected else {
                continue;
            };
            control_first = !control_first;
            sink.send(Bytes::from(encode(&record)?)).await?;
            if let Some(written) = written {
                let _ = written.send(());
            }
        }
        sink.close().await?;
        Ok(())
    });
    (
        ProtocolWriter {
            control: control_tx,
            binary: binary_tx,
            transfer_gate: Arc::new(Mutex::new(())),
        },
        task,
    )
}

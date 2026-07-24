use std::collections::{BTreeMap, BTreeSet, HashSet};

use sha2::{Digest, Sha256};
use tokio::time::Duration;

use crate::application::payload_session::{
    BackendAcquiredPayload, PayloadCleanupReport, PayloadCleanupWarning, PayloadCleanupWarningCode,
    PayloadLocalSource, PayloadSessionMaintenance, PayloadSessionStorage, PayloadStorageFuture,
    PayloadStorageKey,
};
use crate::core::mutation::CancellationSignal;
use crate::core::skill_payload::{
    verify_skill_payload_integrity, verify_skill_payload_manifest, PayloadEntry, PayloadEntryKind,
    SkillPayload, SkillPayloadManifest,
};
use crate::environment::wsl::WslSession;
use crate::environment::wsl_protocol::{
    wsl_operation, wsl_operation_with_features, WslExecutionFeature, WslOperationDescriptor,
    WslOperationExecutor, WslOperationRequest, DEFAULT_WSL_STDERR_LIMIT,
};
use crate::error::AppError;

const PROTOCOL_VERSION: &str = "1";
#[cfg(all(test, target_os = "linux"))]
const OWNER_FILE: &str = ".skill-deck-owner";
#[cfg(all(test, target_os = "linux"))]
const MANIFEST_FILE: &str = "manifest.json";
#[cfg(all(test, target_os = "linux"))]
const BLOB_LIST_FILE: &str = "blob-list";
const MAX_MANIFEST_BYTES: usize = 8 * 1024 * 1024;
const MAX_BRIDGE_BLOB_BYTES: usize = 256 * 1024 * 1024;

const ACQUIRE_SCRIPT: &str = include_str!("../scripts/acquire.sh");

const STORE_BEGIN_SCRIPT: &str = include_str!("../scripts/acquire.sh");

const STORE_BLOB_SCRIPT: &str = include_str!("../scripts/acquire.sh");

const STORE_FINALIZE_SCRIPT: &str = include_str!("../scripts/acquire.sh");

const FINALIZE_SCRIPT: &str = include_str!("../scripts/acquire.sh");

const VERIFY_SCRIPT: &str = include_str!("../scripts/acquire.sh");

const READ_BLOB_SCRIPT: &str = include_str!("../scripts/acquire.sh");

const REMOVE_PAYLOAD_SCRIPT: &str = include_str!("../scripts/acquire.sh");

const REMOVE_SESSION_SCRIPT: &str = include_str!("../scripts/acquire.sh");

const SWEEP_ORPHANS_SCRIPT: &str = include_str!("../scripts/acquire.sh");
const SOURCE_FINGERPRINT_SCRIPT: &str = include_str!("../scripts/acquire.sh");
const SOURCE_REVISION_SCRIPT: &str = include_str!("../scripts/acquire.sh");
const ACQUIRE_OPERATION: WslOperationDescriptor = wsl_operation_with_features(
    "payload",
    "acquire",
    ACQUIRE_SCRIPT,
    &[
        WslExecutionFeature::NulSafeXargs,
        WslExecutionFeature::NulSafeSort,
        WslExecutionFeature::Sha256Sum,
        WslExecutionFeature::CanonicalReadlink,
        WslExecutionFeature::StableStat,
    ],
);
const STORE_BEGIN_OPERATION: WslOperationDescriptor =
    wsl_operation("payload", "store-begin", STORE_BEGIN_SCRIPT);
const STORE_BLOB_OPERATION: WslOperationDescriptor = wsl_operation_with_features(
    "payload",
    "store-blob",
    STORE_BLOB_SCRIPT,
    &[WslExecutionFeature::Sha256Sum],
);
const STORE_FINALIZE_OPERATION: WslOperationDescriptor = wsl_operation_with_features(
    "payload",
    "store-finalize",
    STORE_FINALIZE_SCRIPT,
    &[WslExecutionFeature::Sha256Sum],
);
const FINALIZE_OPERATION: WslOperationDescriptor = wsl_operation_with_features(
    "payload",
    "finalize",
    FINALIZE_SCRIPT,
    &[WslExecutionFeature::Sha256Sum],
);
const VERIFY_OPERATION: WslOperationDescriptor = wsl_operation_with_features(
    "payload",
    "verify",
    VERIFY_SCRIPT,
    &[WslExecutionFeature::Sha256Sum],
);
const READ_BLOB_OPERATION: WslOperationDescriptor = wsl_operation_with_features(
    "payload",
    "read-blob",
    READ_BLOB_SCRIPT,
    &[WslExecutionFeature::Sha256Sum],
);
const REMOVE_PAYLOAD_OPERATION: WslOperationDescriptor =
    wsl_operation("payload", "remove-payload", REMOVE_PAYLOAD_SCRIPT);
const REMOVE_SESSION_OPERATION: WslOperationDescriptor =
    wsl_operation("payload", "remove-session", REMOVE_SESSION_SCRIPT);
const SWEEP_ORPHANS_OPERATION: WslOperationDescriptor =
    wsl_operation("payload", "sweep-orphans", SWEEP_ORPHANS_SCRIPT);
const SOURCE_FINGERPRINT_OPERATION: WslOperationDescriptor = wsl_operation_with_features(
    "payload",
    "fingerprint",
    SOURCE_FINGERPRINT_SCRIPT,
    &[
        WslExecutionFeature::NulSafeXargs,
        WslExecutionFeature::NulSafeSort,
        WslExecutionFeature::Sha256Sum,
        WslExecutionFeature::CanonicalReadlink,
        WslExecutionFeature::StableStat,
    ],
);
const SOURCE_REVISION_OPERATION: WslOperationDescriptor =
    wsl_operation("payload", "source-revision", SOURCE_REVISION_SCRIPT);

pub struct WslPayloadSessionStorage {
    session: WslSession,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WslAcquiredPayload {
    pub manifest: SkillPayloadManifest,
    pub total_bytes: u64,
    pub computed_hash: String,
}

impl WslPayloadSessionStorage {
    pub fn new(session: WslSession) -> Self {
        Self { session }
    }

    fn managed_paths(&self, key: &PayloadStorageKey) -> Result<(String, String), AppError> {
        let session_root = managed_session_root(key.session_id())?;
        let payload_root = format!("{session_root}/payload-{}", digest(key.skill_path()));
        Ok((session_root, payload_root))
    }

    async fn run(
        &self,
        operation: &WslOperationDescriptor,
        args: Vec<String>,
        stdout_limit: usize,
    ) -> Result<Vec<u8>, AppError> {
        self.run_with(
            operation,
            args,
            Vec::new(),
            Duration::from_secs(30),
            stdout_limit,
            None,
        )
        .await
    }

    async fn run_with(
        &self,
        operation: &WslOperationDescriptor,
        args: Vec<String>,
        stdin: Vec<u8>,
        timeout: Duration,
        stdout_limit: usize,
        cancellation: Option<CancellationSignal>,
    ) -> Result<Vec<u8>, AppError> {
        let output = WslOperationExecutor::execute(
            operation,
            WslOperationRequest {
                session: self.session.clone(),
                args,
                stdin,
                timeout,
                stdout_limit,
                stderr_limit: DEFAULT_WSL_STDERR_LIMIT,
                cancellation,
            },
        )
        .await?;
        Ok(output.stdout)
    }

    pub async fn acquire_from_path(
        &self,
        key: &PayloadStorageKey,
        source_root: &str,
        cancellation: Option<CancellationSignal>,
    ) -> Result<WslAcquiredPayload, AppError> {
        if !source_root.starts_with('/') {
            return Err(AppError::UnsafePath {
                path: source_root.to_string(),
                reason: "WSL payload source must be an absolute POSIX path".to_string(),
            });
        }
        let (session_root, payload_root) = self.managed_paths(key)?;
        let base_args = vec![
            source_root.to_string(),
            session_root.clone(),
            payload_root.clone(),
            key.session_id().to_string(),
        ];
        let response = self
            .run_with(
                &ACQUIRE_OPERATION,
                base_args,
                Vec::new(),
                Duration::from_secs(60),
                MAX_MANIFEST_BYTES,
                cancellation.clone(),
            )
            .await?;
        let acquired = match parse_acquire_response(&response) {
            Ok(acquired) => acquired,
            Err(error) => {
                let _ = self
                    .run(
                        &REMOVE_PAYLOAD_OPERATION,
                        vec![session_root, payload_root, key.session_id().to_string()],
                        0,
                    )
                    .await;
                return Err(error);
            }
        };
        let finalize_args = vec![
            session_root.clone(),
            payload_root.clone(),
            key.session_id().to_string(),
        ];
        let finalize = self
            .run_with(
                &FINALIZE_OPERATION,
                finalize_args,
                finalize_request(&acquired.manifest)?,
                Duration::from_secs(30),
                32,
                cancellation,
            )
            .await
            .and_then(|response| parse_finalize_response(&response));
        if let Err(error) = finalize {
            let _ = self
                .run(
                    &REMOVE_PAYLOAD_OPERATION,
                    vec![session_root, payload_root, key.session_id().to_string()],
                    0,
                )
                .await;
            return Err(error);
        }
        Ok(acquired)
    }
}

impl PayloadSessionStorage for WslPayloadSessionStorage {
    fn local_source(&self, key: &PayloadStorageKey) -> Result<PayloadLocalSource, AppError> {
        let (_, payload_root) = self.managed_paths(key)?;
        Ok(PayloadLocalSource::WslManaged {
            distro_name: self.session.distro_name.clone(),
            payload_root,
        })
    }

    fn store<'a>(
        &'a self,
        key: &'a PayloadStorageKey,
        payload: SkillPayload,
    ) -> PayloadStorageFuture<'a, Result<u64, AppError>> {
        Box::pin(async move {
            verify_skill_payload_integrity(&payload)?;
            let manifest = payload.manifest();
            let total_bytes = payload.blobs.values().map(|blob| blob.len() as u64).sum();
            let (session_root, payload_root) = self.managed_paths(key)?;
            let session_id = key.session_id().to_string();
            let stage_root = format!("{payload_root}.upload");
            let store_result = async {
                let response = self
                    .run_with(
                        &STORE_BEGIN_OPERATION,
                        vec![
                            session_root.clone(),
                            payload_root.clone(),
                            session_id.clone(),
                        ],
                        Vec::new(),
                        Duration::from_secs(10),
                        32,
                        None,
                    )
                    .await?;
                parse_finalize_response(&response)?;
                for (blob_id, blob) in &payload.blobs {
                    let response = self
                        .run_with(
                            &STORE_BLOB_OPERATION,
                            vec![
                                session_root.clone(),
                                payload_root.clone(),
                                session_id.clone(),
                                blob_id.clone(),
                            ],
                            blob.clone(),
                            Duration::from_secs(60),
                            32,
                            None,
                        )
                        .await?;
                    parse_finalize_response(&response)?;
                }
                let request = finalize_request(&manifest)?;
                if request.len() > MAX_MANIFEST_BYTES {
                    return Err(AppError::CapabilityUnavailable {
                        capability: "wslPayloadManifestSize".to_string(),
                        path: None,
                    });
                }
                let response = self
                    .run_with(
                        &STORE_FINALIZE_OPERATION,
                        vec![
                            session_root.clone(),
                            payload_root.clone(),
                            session_id.clone(),
                        ],
                        request,
                        Duration::from_secs(30),
                        32,
                        None,
                    )
                    .await?;
                parse_finalize_response(&response)?;
                Ok(total_bytes)
            }
            .await;
            if store_result.is_err() {
                let _ = self
                    .run(
                        &REMOVE_PAYLOAD_OPERATION,
                        vec![session_root, stage_root, session_id],
                        0,
                    )
                    .await;
            }
            store_result
        })
    }

    fn source_metadata_fingerprint<'a>(
        &'a self,
        source_root: &'a str,
    ) -> PayloadStorageFuture<'a, Result<String, AppError>> {
        Box::pin(async move {
            if !source_root.starts_with('/') {
                return Err(AppError::UnsafePath {
                    path: source_root.to_string(),
                    reason: "WSL payload source must be an absolute POSIX path".to_string(),
                });
            }
            let response = self
                .run(
                    &SOURCE_FINGERPRINT_OPERATION,
                    vec![source_root.to_string()],
                    128,
                )
                .await?;
            parse_source_fingerprint(&response)
        })
    }

    fn source_upstream_revision<'a>(
        &'a self,
        repository_root: &'a str,
        skill_path: &'a str,
    ) -> PayloadStorageFuture<'a, Result<Option<String>, AppError>> {
        Box::pin(async move {
            if !self.session.git_available {
                return Err(AppError::CapabilityUnavailable {
                    capability: "wslGit".to_string(),
                    path: Some(repository_root.to_string()),
                });
            }
            if !repository_root.starts_with('/') {
                return Err(AppError::UnsafePath {
                    path: repository_root.to_string(),
                    reason: "WSL Git source root must be an absolute POSIX path".to_string(),
                });
            }
            let skill_directory = crate::core::skill_paths::normalize_skill_folder_path(skill_path);
            let response = self
                .run(
                    &SOURCE_REVISION_OPERATION,
                    vec![repository_root.to_string(), skill_directory],
                    128,
                )
                .await?;
            parse_source_revision(&response).map(Some)
        })
    }

    fn acquire_from_source_path<'a>(
        &'a self,
        key: &'a PayloadStorageKey,
        source_root: &'a str,
        cancellation: Option<CancellationSignal>,
    ) -> PayloadStorageFuture<'a, Result<BackendAcquiredPayload, AppError>> {
        Box::pin(async move {
            let acquired = self
                .acquire_from_path(key, source_root, cancellation)
                .await?;
            Ok(BackendAcquiredPayload {
                manifest: acquired.manifest,
                total_bytes: acquired.total_bytes,
                computed_hash: acquired.computed_hash,
            })
        })
    }

    fn verify<'a>(
        &'a self,
        key: &'a PayloadStorageKey,
    ) -> PayloadStorageFuture<'a, Result<Option<SkillPayloadManifest>, AppError>> {
        Box::pin(async move {
            let (session_root, payload_root) = self.managed_paths(key)?;
            let base_args = vec![
                session_root.clone(),
                payload_root.clone(),
                key.session_id().to_string(),
            ];
            let response = self
                .run(&VERIFY_OPERATION, base_args.clone(), MAX_MANIFEST_BYTES)
                .await;
            let manifest = match response {
                Ok(response) => parse_manifest_response(&response)?,
                Err(AppError::WslCommandFailed {
                    exit_code: Some(69..=72),
                    ..
                }) => return Ok(None),
                Err(error) => return Err(error),
            };
            let mut exact_args = base_args;
            exact_args.push("--expected".to_string());
            let exact_response = self
                .run_with(
                    &VERIFY_OPERATION,
                    exact_args,
                    expected_blob_list(&manifest),
                    Duration::from_secs(30),
                    MAX_MANIFEST_BYTES,
                    None,
                )
                .await?;
            let exact_manifest = parse_manifest_response(&exact_response)?;
            if exact_manifest != manifest {
                return Err(AppError::StalePayload);
            }
            Ok(Some(manifest))
        })
    }

    fn read_blob<'a>(
        &'a self,
        key: &'a PayloadStorageKey,
        blob_id: &'a str,
    ) -> PayloadStorageFuture<'a, Result<Option<Vec<u8>>, AppError>> {
        Box::pin(async move {
            if !valid_blob_id(blob_id) {
                return Err(AppError::StalePayload);
            }
            let (session_root, payload_root) = self.managed_paths(key)?;
            let response = self
                .run(
                    &READ_BLOB_OPERATION,
                    vec![
                        session_root,
                        payload_root,
                        key.session_id().to_string(),
                        blob_id.to_string(),
                    ],
                    MAX_BRIDGE_BLOB_BYTES,
                )
                .await;
            match response {
                Ok(response) => Ok(Some(parse_blob_response(&response)?)),
                Err(AppError::WslCommandFailed {
                    exit_code: Some(63 | 71 | 72),
                    ..
                }) => Ok(None),
                Err(error) => Err(error),
            }
        })
    }

    fn remove<'a>(
        &'a self,
        key: &'a PayloadStorageKey,
    ) -> PayloadStorageFuture<'a, Result<(), AppError>> {
        Box::pin(async move {
            let (session_root, payload_root) = self.managed_paths(key)?;
            self.run(
                &REMOVE_PAYLOAD_OPERATION,
                vec![session_root, payload_root, key.session_id().to_string()],
                0,
            )
            .await?;
            Ok(())
        })
    }

    fn remove_session<'a>(
        &'a self,
        session_id: &'a str,
    ) -> PayloadStorageFuture<'a, Result<(), AppError>> {
        Box::pin(async move {
            let session_root = managed_session_root(session_id)?;
            self.run(
                &REMOVE_SESSION_OPERATION,
                vec![session_root, session_id.to_string()],
                0,
            )
            .await?;
            Ok(())
        })
    }
}

impl PayloadSessionMaintenance for WslPayloadSessionStorage {
    fn sweep_orphans<'a>(
        &'a self,
        protected_session_ids: &'a HashSet<String>,
    ) -> PayloadStorageFuture<'a, Result<PayloadCleanupReport, AppError>> {
        Box::pin(async move {
            let mut protected = protected_session_ids.iter().cloned().collect::<Vec<_>>();
            protected.sort();
            let mut args = Vec::with_capacity(protected.len() + 1);
            args.push("/tmp".to_string());
            args.extend(protected);
            let response = self
                .run(&SWEEP_ORPHANS_OPERATION, args, 1024 * 1024)
                .await?;
            parse_cleanup_report(&response)
        })
    }
}

fn managed_session_root(session_id: &str) -> Result<String, AppError> {
    if session_id.is_empty()
        || session_id.len() > 128
        || !session_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(AppError::StalePayload);
    }
    Ok(format!("/tmp/skill-deck-source-{session_id}"))
}

fn digest(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}

fn valid_blob_id(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

fn parse_manifest_response(bytes: &[u8]) -> Result<SkillPayloadManifest, AppError> {
    let manifest = serde_json::from_slice(bytes)?;
    verify_skill_payload_manifest(&manifest)?;
    Ok(manifest)
}

fn parse_acquire_response(bytes: &[u8]) -> Result<WslAcquiredPayload, AppError> {
    let mut cursor = 0;
    if take_text_field(bytes, &mut cursor)? != PROTOCOL_VERSION {
        return Err(protocol_error("unsupported WSL acquire protocol version"));
    }
    if take_text_field(bytes, &mut cursor)? != "H" {
        return Err(protocol_error("missing WSL acquire CLI hash record"));
    }
    let computed_hash = take_text_field(bytes, &mut cursor)?.to_string();
    if !valid_blob_id(&computed_hash) {
        return Err(protocol_error("invalid WSL acquire CLI hash"));
    }
    let mut entries = Vec::new();
    let mut blob_sizes = BTreeMap::new();
    while cursor < bytes.len() {
        if take_text_field(bytes, &mut cursor)? != "E" {
            return Err(protocol_error("invalid WSL acquire record tag"));
        }
        let kind = take_text_field(bytes, &mut cursor)?;
        let relative_path = take_text_field(bytes, &mut cursor)?.to_string();
        let blob_id = take_text_field(bytes, &mut cursor)?;
        let size = parse_text_field::<u64>(bytes, &mut cursor, "entry size")?;
        let executable = match take_text_field(bytes, &mut cursor)? {
            "0" => false,
            "1" => true,
            _ => return Err(protocol_error("invalid WSL executable flag")),
        };
        let entry = match kind {
            "directory" if blob_id.is_empty() && size == 0 && !executable => PayloadEntry {
                relative_path,
                kind: PayloadEntryKind::Directory,
                blob_id: None,
                content_hash: None,
                size: 0,
                executable: false,
            },
            "file" if valid_blob_id(blob_id) => {
                match blob_sizes.insert(blob_id.to_string(), size) {
                    Some(previous) if previous != size => {
                        return Err(protocol_error("conflicting WSL blob sizes"));
                    }
                    _ => {}
                }
                PayloadEntry {
                    relative_path,
                    kind: PayloadEntryKind::File,
                    blob_id: Some(blob_id.to_string()),
                    content_hash: Some(blob_id.to_string()),
                    size,
                    executable,
                }
            }
            _ => return Err(protocol_error("invalid WSL acquire entry")),
        };
        entries.push(entry);
    }
    let manifest = SkillPayloadManifest::from_entries(entries)?;
    Ok(WslAcquiredPayload {
        manifest,
        total_bytes: blob_sizes.values().copied().sum(),
        computed_hash,
    })
}

fn parse_source_fingerprint(bytes: &[u8]) -> Result<String, AppError> {
    let mut cursor = 0;
    if take_text_field(bytes, &mut cursor)? != PROTOCOL_VERSION {
        return Err(protocol_error(
            "unsupported WSL source fingerprint protocol version",
        ));
    }
    let fingerprint = take_text_field(bytes, &mut cursor)?.to_string();
    if cursor != bytes.len() || !valid_blob_id(&fingerprint) {
        return Err(protocol_error("invalid WSL source metadata fingerprint"));
    }
    Ok(fingerprint)
}

fn parse_source_revision(bytes: &[u8]) -> Result<String, AppError> {
    let mut cursor = 0;
    if take_text_field(bytes, &mut cursor)? != PROTOCOL_VERSION {
        return Err(protocol_error(
            "unsupported WSL source revision protocol version",
        ));
    }
    let revision = take_text_field(bytes, &mut cursor)?.to_string();
    if cursor != bytes.len()
        || !matches!(revision.len(), 40 | 64)
        || !revision.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(protocol_error("invalid WSL source revision"));
    }
    Ok(revision.to_ascii_lowercase())
}

fn parse_finalize_response(bytes: &[u8]) -> Result<(), AppError> {
    (bytes == b"1\0")
        .then_some(())
        .ok_or_else(|| protocol_error("invalid WSL acquire finalize response"))
}

fn expected_blob_ids(manifest: &SkillPayloadManifest) -> BTreeSet<&str> {
    manifest
        .entries
        .iter()
        .filter_map(|entry| entry.blob_id.as_deref())
        .collect()
}

fn expected_blob_list(manifest: &SkillPayloadManifest) -> Vec<u8> {
    let mut list = expected_blob_ids(manifest)
        .into_iter()
        .collect::<Vec<_>>()
        .join("\n")
        .into_bytes();
    if !list.is_empty() {
        list.push(b'\n');
    }
    list
}

fn finalize_request(manifest: &SkillPayloadManifest) -> Result<Vec<u8>, AppError> {
    let ids = expected_blob_ids(manifest);
    let mut request = format!("{}\n", ids.len()).into_bytes();
    for id in ids {
        request.extend_from_slice(id.as_bytes());
        request.push(b'\n');
    }
    request.extend(serde_json::to_vec(manifest)?);
    Ok(request)
}

fn take_text_field<'a>(bytes: &'a [u8], cursor: &mut usize) -> Result<&'a str, AppError> {
    let remaining = bytes
        .get(*cursor..)
        .ok_or_else(|| protocol_error("WSL acquire cursor is out of range"))?;
    let length = remaining
        .iter()
        .position(|byte| *byte == 0)
        .ok_or_else(|| protocol_error("WSL acquire field terminator is missing"))?;
    let field = std::str::from_utf8(&remaining[..length])
        .map_err(|_| protocol_error("WSL acquire field is not UTF-8"))?;
    *cursor += length + 1;
    Ok(field)
}

fn parse_text_field<T>(bytes: &[u8], cursor: &mut usize, field: &str) -> Result<T, AppError>
where
    T: std::str::FromStr,
{
    take_text_field(bytes, cursor)?
        .parse()
        .map_err(|_| protocol_error(&format!("invalid WSL acquire {field}")))
}

fn protocol_error(message: &str) -> AppError {
    AppError::ConfigurationCorrupted {
        message: message.to_string(),
    }
}

fn parse_blob_response(bytes: &[u8]) -> Result<Vec<u8>, AppError> {
    Ok(bytes.to_vec())
}

fn parse_cleanup_report(bytes: &[u8]) -> Result<PayloadCleanupReport, AppError> {
    let records = bytes
        .split(|byte| *byte == 0)
        .filter(|record| !record.is_empty())
        .map(|record| String::from_utf8_lossy(record).into_owned())
        .collect::<Vec<_>>();
    if records.first().map(String::as_str) != Some(PROTOCOL_VERSION) {
        return Err(protocol_error("invalid payload cleanup protocol version"));
    }
    let mut cursor = 1;
    let mut report = PayloadCleanupReport::default();
    while cursor < records.len() {
        match records[cursor].as_str() {
            "W" if cursor + 3 < records.len() => {
                let code = match records[cursor + 1].as_str() {
                    "unknownEntry" => PayloadCleanupWarningCode::UnknownEntry,
                    "invalidMarker" => PayloadCleanupWarningCode::InvalidMarker,
                    "futureMarkerVersion" => PayloadCleanupWarningCode::FutureMarkerVersion,
                    "boundaryRejected" => PayloadCleanupWarningCode::BoundaryRejected,
                    "deleteFailed" => PayloadCleanupWarningCode::DeleteFailed,
                    "sizeUnavailable" => PayloadCleanupWarningCode::SizeUnavailable,
                    _ => return Err(protocol_error("unknown payload cleanup warning code")),
                };
                report.warnings.push(PayloadCleanupWarning {
                    code,
                    candidate_name: Some(records[cursor + 2].clone()),
                    technical_details: (records[cursor + 3] != "-")
                        .then(|| records[cursor + 3].clone()),
                });
                cursor += 4;
            }
            "S" if cursor + 4 < records.len() => {
                report.removed_sessions = records[cursor + 1]
                    .parse()
                    .map_err(|_| protocol_error("invalid removed session count"))?;
                report.protected_sessions = records[cursor + 2]
                    .parse()
                    .map_err(|_| protocol_error("invalid protected session count"))?;
                report.external_retained_bytes = records[cursor + 3]
                    .parse()
                    .map_err(|_| protocol_error("invalid external retained bytes"))?;
                report.capacity_blocked = match records[cursor + 4].as_str() {
                    "0" => false,
                    "1" => true,
                    _ => return Err(protocol_error("invalid payload capacity state")),
                };
                cursor += 5;
            }
            _ => return Err(protocol_error("malformed payload cleanup response")),
        }
    }
    Ok(report)
}

#[cfg(all(test, target_os = "linux"))]
#[allow(
    clippy::disallowed_methods,
    reason = "acquire 协议测试需要直接执行真实 Git 和 shell fixture"
)]
mod tests {
    use std::collections::BTreeMap;
    use std::collections::BTreeSet;
    use std::fs;
    use std::io::Write;
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};
    use std::process::{Command, Stdio};

    use tempfile::tempdir;

    use super::*;
    use crate::application::payload_session::PayloadLocalSource;
    use crate::core::skill_payload::{
        build_skill_payload, compute_cli_project_hash_from_payload, SkillPayloadManifest,
    };

    fn session() -> WslSession {
        WslSession {
            distro_name: "Ubuntu".to_string(),
            user: "alice".to_string(),
            uid: 1000,
            home: "/home/alice".to_string(),
            xdg_state_home: None,
            config_home: "/home/alice/.config".to_string(),
            environment: BTreeMap::new(),
            git_available: true,
            execution_profile: crate::environment::wsl_protocol::WslExecutionProfile::all_supported(
            ),
            runtime_generation: 0,
        }
    }

    #[test]
    fn local_source_is_an_opaque_backend_owned_wsl_path() {
        let storage = WslPayloadSessionStorage::new(session());
        let key = PayloadStorageKey::new("session-1", "skills/demo");
        assert_eq!(
            storage.local_source(&key).expect("local source"),
            PayloadLocalSource::WslManaged {
                distro_name: "Ubuntu".to_string(),
                payload_root: format!(
                    "/tmp/skill-deck-source-session-1/payload-{}",
                    digest("skills/demo")
                ),
            }
        );
    }

    #[test]
    fn source_revision_script_returns_selected_git_tree_object_id() {
        let repo = tempdir().expect("repo");
        let skill = repo.path().join("skills/demo");
        fs::create_dir_all(&skill).expect("skill");
        fs::write(skill.join("SKILL.md"), b"demo").expect("document");
        for args in [
            vec!["init"],
            vec!["config", "user.email", "test@example.com"],
            vec!["config", "user.name", "Skill Deck Test"],
            vec!["add", "."],
            vec!["commit", "-m", "fixture"],
        ] {
            let output = Command::new("git")
                .arg("-C")
                .arg(repo.path())
                .args(args)
                .output()
                .expect("git command");
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        let expected = Command::new("git")
            .arg("-C")
            .arg(repo.path())
            .args(["rev-parse", "HEAD:skills/demo"])
            .output()
            .expect("expected revision");
        let output = Command::new("/bin/sh")
            .arg("-c")
            .arg(ACQUIRE_SCRIPT)
            .arg("--")
            .arg("source-revision")
            .arg(repo.path())
            .arg("skills/demo")
            .output()
            .expect("source revision script");

        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            output.stdout,
            format!("1\0{}\0", String::from_utf8_lossy(&expected.stdout).trim()).into_bytes()
        );
    }

    #[test]
    fn source_revision_parser_rejects_non_git_hashes() {
        assert_eq!(
            parse_source_revision(format!("1\0{}\0", "A".repeat(40)).as_bytes()).unwrap(),
            "a".repeat(40)
        );
        assert!(parse_source_revision(format!("1\0{}\0", "a".repeat(39)).as_bytes()).is_err());
        assert!(parse_source_revision(format!("1\0{}z\0", "a".repeat(39)).as_bytes()).is_err());
    }

    fn fixture() -> (tempfile::TempDir, PathBuf, PathBuf, SkillPayloadManifest) {
        let temp = tempdir().expect("temp");
        let source = temp.path().join("source");
        fs::create_dir(&source).unwrap();
        fs::write(source.join("SKILL.md"), [0, 1, 2, 255]).unwrap();
        let payload = build_skill_payload(&source).unwrap();
        let manifest = payload.manifest();
        let session_root = temp.path().join("skill-deck-source-session-1");
        let payload_root = session_root.join("payload-demo");
        fs::create_dir_all(payload_root.join("blobs")).unwrap();
        fs::write(session_root.join(OWNER_FILE), b"1\nsession-1\n").unwrap();
        fs::write(
            payload_root.join(MANIFEST_FILE),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();
        let ids = payload.blobs.keys().cloned().collect::<BTreeSet<_>>();
        fs::write(
            payload_root.join(BLOB_LIST_FILE),
            ids.iter()
                .map(String::as_str)
                .collect::<Vec<_>>()
                .join("\n")
                + "\n",
        )
        .unwrap();
        for (id, blob) in payload.blobs {
            fs::write(payload_root.join("blobs").join(id), blob).unwrap();
        }
        (temp, session_root, payload_root, manifest)
    }

    #[test]
    fn verify_script_returns_only_manifest_after_local_blob_hash_validation() {
        let (_temp, session_root, payload_root, expected) = fixture();
        let output = Command::new("/bin/sh")
            .arg("-c")
            .arg(VERIFY_SCRIPT)
            .arg("--")
            .arg("verify")
            .arg(&session_root)
            .arg(&payload_root)
            .arg("session-1")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(parse_manifest_response(&output.stdout).unwrap(), expected);

        let blob = fs::read_dir(payload_root.join("blobs"))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        fs::write(blob, b"tampered").unwrap();
        let output = Command::new("/bin/sh")
            .arg("-c")
            .arg(VERIFY_SCRIPT)
            .arg("--")
            .arg("verify")
            .arg(&session_root)
            .arg(&payload_root)
            .arg("session-1")
            .output()
            .unwrap();
        assert!(!output.status.success());
    }

    #[test]
    fn acquire_script_builds_backend_local_full_tree_snapshot() {
        let temp = tempdir().expect("temp");
        let source = temp.path().join("source");
        fs::create_dir_all(source.join("scripts")).unwrap();
        fs::create_dir_all(source.join("assets")).unwrap();
        fs::create_dir_all(source.join(".git")).unwrap();
        fs::write(source.join("SKILL.md"), b"skill").unwrap();
        fs::write(source.join("scripts/run.sh"), b"#!/bin/sh\n").unwrap();
        fs::write(source.join("assets/data.bin"), [0, 1, 255]).unwrap();
        fs::write(source.join("metadata.json"), b"excluded").unwrap();
        fs::write(source.join(".git/config"), b"excluded").unwrap();
        fs::set_permissions(
            source.join("scripts/run.sh"),
            fs::Permissions::from_mode(0o755),
        )
        .unwrap();
        let expected_payload = build_skill_payload(&source).unwrap();
        let expected_hash = compute_cli_project_hash_from_payload(&expected_payload).unwrap();
        let expected = expected_payload.manifest();
        let session_root = temp.path().join("skill-deck-source-session-2");
        let payload_root = session_root.join("payload-demo");

        let output = Command::new("/bin/sh")
            .arg("-c")
            .arg(ACQUIRE_SCRIPT)
            .arg("--")
            .arg("acquire")
            .arg(&source)
            .arg(&session_root)
            .arg(&payload_root)
            .arg("session-2")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let acquired = parse_acquire_response(&output.stdout).unwrap();
        assert_eq!(acquired.manifest, expected);
        assert_eq!(acquired.total_bytes, 5 + 10 + 3);
        assert_eq!(acquired.computed_hash, expected_hash);
        assert!(!payload_root.join(MANIFEST_FILE).exists());

        let mut child = Command::new("/bin/sh")
            .arg("-c")
            .arg(FINALIZE_SCRIPT)
            .arg("--")
            .arg("finalize")
            .arg(&session_root)
            .arg(&payload_root)
            .arg("session-2")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(&finalize_request(&acquired.manifest).unwrap())
            .unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(output.stdout, b"1\0");

        let mut child = Command::new("/bin/sh")
            .arg("-c")
            .arg(VERIFY_SCRIPT)
            .arg("--")
            .arg("verify")
            .arg(&session_root)
            .arg(&payload_root)
            .arg("session-2")
            .arg("--expected")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(&expected_blob_list(&expected))
            .unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(output.status.success());
        assert_eq!(parse_manifest_response(&output.stdout).unwrap(), expected);
    }

    #[cfg(unix)]
    #[test]
    fn source_fingerprint_tracks_mode_content_and_safe_link_targets() {
        let temp = tempdir().expect("temp");
        let source = temp.path().join("source");
        fs::create_dir_all(source.join("scripts")).expect("scripts");
        fs::write(source.join("SKILL.md"), b"skill").expect("Skill");
        fs::write(source.join("scripts/run.sh"), b"#!/bin/sh\n").expect("script");
        std::os::unix::fs::symlink("scripts/run.sh", source.join("run")).expect("internal link");

        let fingerprint = || {
            let output = Command::new("/bin/sh")
                .arg("-c")
                .arg(ACQUIRE_SCRIPT)
                .arg("--")
                .arg("fingerprint")
                .arg(&source)
                .output()
                .expect("fingerprint script");
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            parse_source_fingerprint(&output.stdout).expect("fingerprint response")
        };

        let initial = fingerprint();
        fs::set_permissions(
            source.join("scripts/run.sh"),
            fs::Permissions::from_mode(0o755),
        )
        .expect("mode");
        let mode_changed = fingerprint();
        fs::write(source.join("scripts/run.sh"), b"#!/bin/sh\necho changed\n").expect("content");
        let content_changed = fingerprint();

        assert_ne!(initial, mode_changed);
        assert_ne!(mode_changed, content_changed);
    }

    #[cfg(unix)]
    #[test]
    fn source_fingerprint_rejects_external_links() {
        let temp = tempdir().expect("temp");
        let source = temp.path().join("source");
        fs::create_dir(&source).expect("source");
        fs::write(temp.path().join("outside"), b"outside").expect("outside");
        std::os::unix::fs::symlink(temp.path().join("outside"), source.join("external"))
            .expect("external link");

        let output = Command::new("/bin/sh")
            .arg("-c")
            .arg(ACQUIRE_SCRIPT)
            .arg("--")
            .arg("fingerprint")
            .arg(&source)
            .output()
            .expect("fingerprint script");

        assert!(!output.status.success());
    }

    #[test]
    fn store_scripts_write_an_in_memory_payload_into_backend_local_storage() {
        let temp = tempdir().expect("temp");
        let source = temp.path().join("source");
        fs::create_dir_all(source.join("assets")).unwrap();
        fs::write(source.join("SKILL.md"), b"skill").unwrap();
        fs::write(source.join("assets/data.bin"), [0, 1, 2, 255]).unwrap();
        let payload = build_skill_payload(&source).unwrap();
        let session_root = temp.path().join("skill-deck-source-session-3");
        let payload_root = session_root.join("payload-demo");

        let begin = Command::new("/bin/sh")
            .arg("-c")
            .arg(STORE_BEGIN_SCRIPT)
            .arg("--")
            .arg("store-begin")
            .arg(&session_root)
            .arg(&payload_root)
            .arg("session-3")
            .output()
            .unwrap();
        assert!(
            begin.status.success(),
            "{}",
            String::from_utf8_lossy(&begin.stderr)
        );

        for (blob_id, blob) in &payload.blobs {
            let mut child = Command::new("/bin/sh")
                .arg("-c")
                .arg(STORE_BLOB_SCRIPT)
                .arg("--")
                .arg("store-blob")
                .arg(&session_root)
                .arg(&payload_root)
                .arg("session-3")
                .arg(blob_id)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap();
            child.stdin.take().unwrap().write_all(blob).unwrap();
            let output = child.wait_with_output().unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        let mut child = Command::new("/bin/sh")
            .arg("-c")
            .arg(STORE_FINALIZE_SCRIPT)
            .arg("--")
            .arg("store-finalize")
            .arg(&session_root)
            .arg(&payload_root)
            .arg("session-3")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(&finalize_request(&payload.manifest()).unwrap())
            .unwrap();
        let finalized = child.wait_with_output().unwrap();
        assert!(
            finalized.status.success(),
            "{}",
            String::from_utf8_lossy(&finalized.stderr)
        );

        let verified = Command::new("/bin/sh")
            .arg("-c")
            .arg(VERIFY_SCRIPT)
            .arg("--")
            .arg("verify")
            .arg(&session_root)
            .arg(&payload_root)
            .arg("session-3")
            .output()
            .unwrap();
        assert!(verified.status.success());
        assert_eq!(
            parse_manifest_response(&verified.stdout).unwrap(),
            payload.manifest()
        );
        for (blob_id, blob) in &payload.blobs {
            assert_eq!(
                fs::read(payload_root.join("blobs").join(blob_id)).unwrap(),
                *blob
            );
        }
    }

    #[test]
    fn blob_protocol_preserves_binary_content() {
        let (_temp, session_root, payload_root, manifest) = fixture();
        let blob_id = manifest.entries[0].blob_id.as_deref().unwrap();
        let output = Command::new("/bin/sh")
            .arg("-c")
            .arg(READ_BLOB_SCRIPT)
            .arg("--")
            .arg("read-blob")
            .arg(&session_root)
            .arg(&payload_root)
            .arg("session-1")
            .arg(blob_id)
            .output()
            .unwrap();
        assert!(output.status.success());
        assert_eq!(parse_blob_response(&output.stdout).unwrap(), [0, 1, 2, 255]);
    }

    #[test]
    fn removal_script_requires_matching_owner_and_managed_child() {
        let (temp, session_root, payload_root, _manifest) = fixture();
        let forged = temp.path().join("outside");
        fs::create_dir(&forged).unwrap();
        assert!(!run_remove(&session_root, &forged, "session-1"));
        assert!(forged.is_dir());
        assert!(run_remove(&session_root, &payload_root, "session-1"));
        assert!(!payload_root.exists());
    }

    #[test]
    fn reconnect_sweep_preserves_protected_sessions_and_reports_invalid_roots() {
        let temp = tempdir().expect("temp");
        let protected = temp.path().join("skill-deck-source-protected");
        let orphan = temp.path().join("skill-deck-source-orphan");
        let invalid = temp.path().join("skill-deck-source-invalid");
        for (root, id) in [(&protected, "protected"), (&orphan, "orphan")] {
            fs::create_dir(root).expect("session root");
            fs::write(root.join(OWNER_FILE), format!("1\n{id}\n")).expect("owner");
            fs::write(root.join("payload.bin"), b"payload").expect("payload");
        }
        fs::create_dir(&invalid).expect("invalid root");
        fs::write(invalid.join("payload.bin"), b"retained").expect("invalid payload");

        let output = Command::new("/bin/sh")
            .arg("-c")
            .arg(SWEEP_ORPHANS_SCRIPT)
            .arg("--")
            .arg("sweep-orphans")
            .arg(temp.path())
            .arg("protected")
            .output()
            .expect("sweep script");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let report = parse_cleanup_report(&output.stdout).expect("cleanup report");

        assert_eq!(report.removed_sessions, 1);
        assert_eq!(report.protected_sessions, 1);
        assert!(report.capacity_blocked);
        assert!(report.external_retained_bytes >= b"retained".len() as u64);
        assert!(protected.is_dir());
        assert!(!orphan.exists());
        assert!(invalid.is_dir());
    }

    fn run_remove(session_root: &Path, payload_root: &Path, session_id: &str) -> bool {
        Command::new("/bin/sh")
            .arg("-c")
            .arg(REMOVE_PAYLOAD_SCRIPT)
            .arg("--")
            .arg("remove-payload")
            .arg(session_root)
            .arg(payload_root)
            .arg(session_id)
            .status()
            .unwrap()
            .success()
    }
}

#[cfg(all(test, not(target_os = "linux")))]
mod portable_tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::application::payload_session::PayloadLocalSource;

    fn session() -> WslSession {
        WslSession {
            distro_name: "Ubuntu".to_string(),
            user: "alice".to_string(),
            uid: 1000,
            home: "/home/alice".to_string(),
            xdg_state_home: None,
            config_home: "/home/alice/.config".to_string(),
            environment: BTreeMap::new(),
            git_available: true,
            execution_profile: crate::environment::wsl_protocol::WslExecutionProfile::all_supported(
            ),
            runtime_generation: 0,
        }
    }

    #[test]
    fn local_source_is_an_opaque_backend_owned_wsl_path() {
        let storage = WslPayloadSessionStorage::new(session());
        let key = PayloadStorageKey::new("session-1", "skills/demo");
        assert_eq!(
            storage.local_source(&key).expect("local source"),
            PayloadLocalSource::WslManaged {
                distro_name: "Ubuntu".to_string(),
                payload_root: format!(
                    "/tmp/skill-deck-source-session-1/payload-{}",
                    digest("skills/demo")
                ),
            }
        );
    }

    #[test]
    fn source_revision_parser_rejects_non_git_hashes() {
        assert_eq!(
            parse_source_revision(format!("1\0{}\0", "A".repeat(40)).as_bytes()).unwrap(),
            "a".repeat(40)
        );
        assert!(parse_source_revision(format!("1\0{}\0", "a".repeat(39)).as_bytes()).is_err());
        assert!(parse_source_revision(format!("1\0{}z\0", "a".repeat(39)).as_bytes()).is_err());
    }
}

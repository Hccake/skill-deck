use std::collections::{HashMap, HashSet};
use std::sync::Mutex;
use std::time::Duration;

use sha2::{Digest, Sha256};

use crate::application::payload_session::{
    BackendAcquiredPayload, PayloadCleanupReport, PayloadCleanupWarning, PayloadCleanupWarningCode,
    PayloadLocalSource, PayloadSessionMaintenance, PayloadSessionStorage, PayloadStorageFuture,
    PayloadStorageKey,
};
use crate::core::mutation::CancellationSignal;
use crate::core::skill_payload::{
    verify_skill_payload_integrity, PayloadEntry, PayloadEntryKind, SkillPayload,
    SkillPayloadManifest,
};
use crate::environment::wsl::operations::source_acquisition::{
    WorkerSourceHandle, WslNativeSource,
};
use crate::environment::wsl::WslWorkspace;
use crate::error::AppError;

const MAX_MANIFEST_BYTES: usize = 8 * 1024 * 1024;
const MAX_BLOB_BYTES: usize = 256 * 1024 * 1024;

#[derive(Debug, Clone)]
struct SourceBinding {
    handle: WorkerSourceHandle,
    root: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct WorkerPayloadHandle {
    generation: u64,
    id: u64,
}

pub struct WslPayloadSessionStorage {
    workspace: WslWorkspace,
    source: Option<SourceBinding>,
    handles: Mutex<HashMap<PayloadStorageKey, WorkerPayloadHandle>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WslAcquiredPayload {
    pub manifest: SkillPayloadManifest,
    pub total_bytes: u64,
    pub computed_hash: String,
}

impl WslPayloadSessionStorage {
    pub fn new(workspace: WslWorkspace) -> Self {
        Self {
            workspace,
            source: None,
            handles: Mutex::new(HashMap::new()),
        }
    }

    pub(crate) fn for_source(workspace: WslWorkspace, source: &WslNativeSource) -> Self {
        Self {
            workspace,
            source: Some(SourceBinding {
                handle: source.handle(),
                root: source.native_root().to_string(),
            }),
            handles: Mutex::new(HashMap::new()),
        }
    }

    fn payload_name(&self, key: &PayloadStorageKey) -> String {
        format!("payload-{}", digest(key.skill_path()))
    }

    fn remember_handle(&self, key: &PayloadStorageKey, generation: u64, id: u64) {
        self.handles
            .lock()
            .expect("WSL payload handle map lock poisoned")
            .insert(key.clone(), WorkerPayloadHandle { generation, id });
    }

    fn forget_key(&self, key: &PayloadStorageKey) {
        self.handles
            .lock()
            .expect("WSL payload handle map lock poisoned")
            .remove(key);
    }

    fn handle(&self, key: &PayloadStorageKey) -> Result<WorkerPayloadHandle, AppError> {
        self.handles
            .lock()
            .expect("WSL payload handle map lock poisoned")
            .get(key)
            .copied()
            .ok_or(AppError::StalePayload)
    }

    async fn remove_key_best_effort(&self, key: &PayloadStorageKey) {
        let _ = self
            .workspace
            .request_worker_control_once(
                environment_protocol::Message::RemovePayload {
                    session_id: key.session_id().to_string(),
                    payload_name: self.payload_name(key),
                },
                None,
                Duration::from_secs(10),
            )
            .await;
        self.forget_key(key);
    }

    async fn store_in_worker(
        &self,
        key: &PayloadStorageKey,
        payload: SkillPayload,
    ) -> Result<u64, AppError> {
        verify_skill_payload_integrity(&payload)?;
        let manifest = payload.manifest();
        let payload_name = self.payload_name(key);
        let (generation, response) = self
            .workspace
            .request_worker_control_once(
                environment_protocol::Message::BeginPayloadUpload {
                    session_id: key.session_id().to_string(),
                    payload_name: payload_name.clone(),
                },
                None,
                Duration::from_secs(10),
            )
            .await?;
        let upload_id = match response {
            environment_protocol::Message::PayloadUploadBegun { upload_id } => upload_id,
            message => return Err(worker_response_error(message, "PayloadUploadBegun")),
        };
        let result = async {
            for (blob_id, blob) in &payload.blobs {
                if blob.len() > MAX_BLOB_BYTES {
                    return Err(AppError::CapabilityUnavailable {
                        capability: "wslPayloadBlobSize".to_string(),
                        path: None,
                    });
                }
                let digest = format!("sha256:{blob_id}");
                let response = self
                    .workspace
                    .request_worker_control_for_generation(
                        generation,
                        environment_protocol::Message::UploadPayloadBlob {
                            upload_id,
                            blob_id: blob_id.clone(),
                            total_bytes: blob.len() as u64,
                            sha256: digest,
                        },
                        None,
                        Duration::from_secs(10),
                    )
                    .await?;
                let transfer_id = match response {
                    environment_protocol::Message::TransferReady { transfer_id } => transfer_id,
                    message => return Err(worker_response_error(message, "TransferReady")),
                };
                let response = self
                    .workspace
                    .send_worker_transfer_for_generation(
                        generation,
                        transfer_id,
                        blob,
                        MAX_BLOB_BYTES,
                        Duration::from_secs(60),
                    )
                    .await?;
                match response {
                    environment_protocol::Message::PayloadBlobUploaded {
                        upload_id: actual_upload,
                        blob_id: actual_blob,
                    } if actual_upload == upload_id && actual_blob == *blob_id => {}
                    message => {
                        return Err(worker_response_error(message, "PayloadBlobUploaded"));
                    }
                }
            }
            let manifest_bytes = serde_json::to_vec(&manifest)?;
            if manifest_bytes.is_empty() || manifest_bytes.len() > MAX_MANIFEST_BYTES {
                return Err(AppError::CapabilityUnavailable {
                    capability: "wslPayloadManifestSize".to_string(),
                    path: None,
                });
            }
            let manifest_sha = format!("sha256:{:x}", Sha256::digest(&manifest_bytes));
            let response = self
                .workspace
                .request_worker_control_for_generation(
                    generation,
                    environment_protocol::Message::FinalizePayloadUpload {
                        upload_id,
                        total_bytes: manifest_bytes.len() as u64,
                        sha256: manifest_sha,
                    },
                    None,
                    Duration::from_secs(10),
                )
                .await?;
            let transfer_id = match response {
                environment_protocol::Message::TransferReady { transfer_id } => transfer_id,
                message => return Err(worker_response_error(message, "TransferReady")),
            };
            let response = self
                .workspace
                .send_worker_transfer_for_generation(
                    generation,
                    transfer_id,
                    &manifest_bytes,
                    MAX_MANIFEST_BYTES,
                    Duration::from_secs(30),
                )
                .await?;
            let payload_id = match response {
                environment_protocol::Message::PayloadUploadFinalized { payload_id, .. } => {
                    payload_id
                }
                message => return Err(worker_response_error(message, "PayloadUploadFinalized")),
            };
            self.remember_handle(key, generation, payload_id);
            Ok(payload.blobs.values().map(|blob| blob.len() as u64).sum())
        }
        .await;
        if result.is_err() {
            self.remove_key_best_effort(key).await;
        }
        result
    }

    fn source_binding(&self) -> Result<&SourceBinding, AppError> {
        self.source
            .as_ref()
            .ok_or_else(|| AppError::CapabilityUnavailable {
                capability: "wslSourceHandle".to_string(),
                path: None,
            })
    }

    pub async fn acquire_from_path(
        &self,
        key: &PayloadStorageKey,
        source_root: &str,
        cancellation: Option<CancellationSignal>,
    ) -> Result<WslAcquiredPayload, AppError> {
        if let Some(source) = &self.source {
            return self
                .acquire_from_bound_source(key, source, source_root, cancellation)
                .await;
        }
        if !source_root.starts_with('/') {
            return Err(AppError::UnsafePath {
                path: source_root.to_string(),
                reason: "WSL payload source must be an absolute POSIX path".to_string(),
            });
        }
        let (generation, response) = self
            .workspace
            .request_worker_control_once(
                environment_protocol::Message::OpenLocalSource {
                    request: environment_protocol::OpenLocalSourceRequest {
                        path: source_root.to_string(),
                    },
                },
                cancellation.clone(),
                Duration::from_secs(10),
            )
            .await?;
        let (source_id, root) = match response {
            environment_protocol::Message::SourceOpened {
                source_id, root, ..
            } => (source_id, root),
            message => return Err(worker_response_error(message, "SourceOpened")),
        };
        let source = SourceBinding {
            handle: WorkerSourceHandle {
                generation,
                id: source_id,
            },
            root,
        };
        let result = self
            .acquire_from_bound_source(key, &source, source_root, cancellation)
            .await;
        let _ = self
            .workspace
            .request_worker_control_for_generation(
                generation,
                environment_protocol::Message::ReleaseSource { source_id },
                None,
                Duration::from_secs(10),
            )
            .await;
        result
    }

    async fn acquire_from_bound_source(
        &self,
        key: &PayloadStorageKey,
        source: &SourceBinding,
        source_root: &str,
        cancellation: Option<CancellationSignal>,
    ) -> Result<WslAcquiredPayload, AppError> {
        let relative_path = relative_source_path(&source.root, source_root)?;
        let response: environment_protocol::PayloadReadyResponse = self
            .workspace
            .request_worker_payload_for_generation(
                source.handle.generation,
                environment_protocol::Message::AcquirePayloadFromSource {
                    request: environment_protocol::AcquirePayloadFromSourceRequest {
                        source_id: source.handle.id,
                        relative_path: relative_path.into_bytes(),
                        session_id: key.session_id().to_string(),
                        payload_name: self.payload_name(key),
                        deadline_millis: 60_000,
                    },
                },
                MAX_MANIFEST_BYTES,
                cancellation,
                Duration::from_secs(65),
            )
            .await?;
        self.remember_handle(key, source.handle.generation, response.payload_id);
        Ok(WslAcquiredPayload {
            manifest: map_manifest(response.manifest)?,
            total_bytes: response.total_bytes,
            computed_hash: response.computed_hash.ok_or_else(|| {
                AppError::ConfigurationCorrupted {
                    message: "WSL Worker source payload omitted its CLI hash".to_string(),
                }
            })?,
        })
    }
}

impl PayloadSessionStorage for WslPayloadSessionStorage {
    fn local_source(&self, key: &PayloadStorageKey) -> Result<PayloadLocalSource, AppError> {
        let handle = self.handle(key)?;
        Ok(PayloadLocalSource::WslManaged {
            distro_name: self.workspace.distro_name().to_string(),
            worker_generation: handle.generation,
            worker_payload_id: handle.id,
        })
    }

    fn store<'a>(
        &'a self,
        key: &'a PayloadStorageKey,
        payload: SkillPayload,
    ) -> PayloadStorageFuture<'a, Result<u64, AppError>> {
        Box::pin(async move { self.store_in_worker(key, payload).await })
    }

    fn acquire_from_source_path<'a>(
        &'a self,
        key: &'a PayloadStorageKey,
        source_root: &'a str,
        cancellation: Option<CancellationSignal>,
    ) -> PayloadStorageFuture<'a, Result<BackendAcquiredPayload, AppError>> {
        Box::pin(async move {
            let source = self.source_binding()?;
            let response = self
                .acquire_from_bound_source(key, source, source_root, cancellation)
                .await?;
            Ok(BackendAcquiredPayload {
                manifest: response.manifest,
                total_bytes: response.total_bytes,
                computed_hash: response.computed_hash,
            })
        })
    }

    fn acquire_from_path<'a>(
        &'a self,
        key: &'a PayloadStorageKey,
        source_root: &'a str,
        cancellation: Option<CancellationSignal>,
    ) -> PayloadStorageFuture<'a, Result<BackendAcquiredPayload, AppError>> {
        Box::pin(async move {
            let response =
                WslPayloadSessionStorage::acquire_from_path(self, key, source_root, cancellation)
                    .await?;
            Ok(BackendAcquiredPayload {
                manifest: response.manifest,
                total_bytes: response.total_bytes,
                computed_hash: response.computed_hash,
            })
        })
    }

    fn source_metadata_fingerprint<'a>(
        &'a self,
        source_root: &'a str,
    ) -> PayloadStorageFuture<'a, Result<String, AppError>> {
        Box::pin(async move {
            let source = self.source_binding()?;
            let relative_path = relative_source_path(&source.root, source_root)?;
            let response = self
                .workspace
                .request_worker_control_for_generation(
                    source.handle.generation,
                    environment_protocol::Message::SourceFingerprint {
                        source_id: source.handle.id,
                        relative_path: relative_path.into_bytes(),
                        deadline_millis: 30_000,
                    },
                    None,
                    Duration::from_secs(35),
                )
                .await?;
            match response {
                environment_protocol::Message::SourceFingerprintResult { fingerprint } => {
                    Ok(fingerprint)
                }
                message => Err(worker_response_error(message, "SourceFingerprintResult")),
            }
        })
    }

    fn source_upstream_revision<'a>(
        &'a self,
        repository_root: &'a str,
        skill_path: &'a str,
    ) -> PayloadStorageFuture<'a, Result<Option<String>, AppError>> {
        Box::pin(async move {
            let source = self.source_binding()?;
            if repository_root != source.root {
                return Err(AppError::StalePayload);
            }
            let response = self
                .workspace
                .request_worker_control_for_generation(
                    source.handle.generation,
                    environment_protocol::Message::SourceRevision {
                        source_id: source.handle.id,
                        relative_path: normalize_skill_revision_path(skill_path)?.into_bytes(),
                        deadline_millis: 30_000,
                    },
                    None,
                    Duration::from_secs(35),
                )
                .await?;
            match response {
                environment_protocol::Message::SourceRevisionResult { revision } => {
                    Ok(Some(revision))
                }
                message => Err(worker_response_error(message, "SourceRevisionResult")),
            }
        })
    }

    fn verify<'a>(
        &'a self,
        key: &'a PayloadStorageKey,
    ) -> PayloadStorageFuture<'a, Result<Option<SkillPayloadManifest>, AppError>> {
        Box::pin(async move {
            let (generation, response): (u64, Option<environment_protocol::PayloadReadyResponse>) =
                self.workspace
                    .request_worker_payload_once(
                        environment_protocol::Message::VerifyPayload {
                            request: environment_protocol::VerifyPayloadRequest {
                                session_id: key.session_id().to_string(),
                                payload_name: self.payload_name(key),
                                deadline_millis: 30_000,
                            },
                        },
                        MAX_MANIFEST_BYTES,
                        None,
                        Duration::from_secs(35),
                    )
                    .await?;
            let Some(response) = response else {
                self.forget_key(key);
                return Ok(None);
            };
            self.remember_handle(key, generation, response.payload_id);
            map_manifest(response.manifest).map(Some)
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
            let handle = self.handle(key)?;
            let result = self
                .workspace
                .request_worker_bytes_for_generation(
                    handle.generation,
                    environment_protocol::Message::ReadPayloadBlob {
                        payload_id: handle.id,
                        blob_id: blob_id.to_string(),
                        deadline_millis: 60_000,
                    },
                    MAX_BLOB_BYTES,
                    Duration::from_secs(65),
                )
                .await;
            match result {
                Ok(blob) => Ok(Some(blob)),
                Err(AppError::ExecutionFailed { message })
                    if message.contains("missingPayload") =>
                {
                    Ok(None)
                }
                Err(error) => Err(error),
            }
        })
    }

    fn remove<'a>(
        &'a self,
        key: &'a PayloadStorageKey,
    ) -> PayloadStorageFuture<'a, Result<(), AppError>> {
        Box::pin(async move {
            let (_, response) = self
                .workspace
                .request_worker_control_once(
                    environment_protocol::Message::RemovePayload {
                        session_id: key.session_id().to_string(),
                        payload_name: self.payload_name(key),
                    },
                    None,
                    Duration::from_secs(10),
                )
                .await?;
            match response {
                environment_protocol::Message::PayloadRemoved { .. } => {
                    self.forget_key(key);
                    Ok(())
                }
                message => Err(worker_response_error(message, "PayloadRemoved")),
            }
        })
    }

    fn remove_session<'a>(
        &'a self,
        session_id: &'a str,
    ) -> PayloadStorageFuture<'a, Result<(), AppError>> {
        Box::pin(async move {
            let (_, response) = self
                .workspace
                .request_worker_control_once(
                    environment_protocol::Message::RemovePayloadSession {
                        session_id: session_id.to_string(),
                    },
                    None,
                    Duration::from_secs(10),
                )
                .await?;
            match response {
                environment_protocol::Message::PayloadSessionRemoved { .. } => {
                    self.handles
                        .lock()
                        .expect("WSL payload handle map lock poisoned")
                        .retain(|key, _| key.session_id() != session_id);
                    Ok(())
                }
                message => Err(worker_response_error(message, "PayloadSessionRemoved")),
            }
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
            let (_, response): (u64, environment_protocol::PayloadCleanupResponse) = self
                .workspace
                .request_worker_payload_once(
                    environment_protocol::Message::SweepPayloadOrphans {
                        protected_session_ids: protected,
                    },
                    1024 * 1024,
                    None,
                    Duration::from_secs(35),
                )
                .await?;
            Ok(PayloadCleanupReport {
                removed_sessions: response.removed_sessions as usize,
                protected_sessions: response.protected_sessions as usize,
                external_retained_bytes: response.retained_external_bytes,
                capacity_blocked: response.cleanup_blocked,
                warnings: response
                    .warnings
                    .into_iter()
                    .map(|warning| {
                        Ok::<_, AppError>(PayloadCleanupWarning {
                            code: cleanup_warning_code(&warning.code)?,
                            candidate_name: warning.candidate_name,
                            technical_details: warning.technical_details,
                        })
                    })
                    .collect::<Result<_, _>>()?,
            })
        })
    }
}

fn map_manifest(
    manifest: environment_protocol::PayloadManifest,
) -> Result<SkillPayloadManifest, AppError> {
    let expected_root_hash = manifest.payload_root_hash.clone();
    let expected_payload_id = manifest.payload_id.clone();
    let entries = manifest
        .entries
        .into_iter()
        .map(|entry| PayloadEntry {
            relative_path: entry.relative_path,
            kind: match entry.kind {
                environment_protocol::PayloadEntryKind::File => PayloadEntryKind::File,
                environment_protocol::PayloadEntryKind::Directory => PayloadEntryKind::Directory,
            },
            blob_id: entry.blob_id,
            content_hash: entry.content_hash,
            size: entry.size,
            executable: entry.executable,
        })
        .collect();
    let mapped = SkillPayloadManifest::from_entries(entries)?;
    if mapped.payload_id().as_str() != expected_payload_id
        || mapped.payload_root_hash != expected_root_hash
    {
        return Err(AppError::StalePayload);
    }
    Ok(mapped)
}

fn relative_source_path(source_root: &str, requested: &str) -> Result<String, AppError> {
    if requested == source_root {
        return Ok(String::new());
    }
    requested
        .strip_prefix(source_root)
        .and_then(|relative| relative.strip_prefix('/'))
        .map(normalize_relative_path)
        .transpose()?
        .ok_or(AppError::StalePayload)
}

fn normalize_skill_revision_path(path: &str) -> Result<String, AppError> {
    let path = crate::core::skill_paths::normalize_skill_folder_path(path);
    normalize_relative_path(&path)
}

fn normalize_relative_path(path: &str) -> Result<String, AppError> {
    if path.is_empty() {
        return Ok(String::new());
    }
    if path.starts_with('/')
        || path.contains('\\')
        || path
            .split('/')
            .any(|component| component.is_empty() || matches!(component, "." | ".."))
    {
        return Err(AppError::StalePayload);
    }
    Ok(path.to_string())
}

fn worker_response_error(message: environment_protocol::Message, expected: &str) -> AppError {
    match message {
        environment_protocol::Message::Error { code, phase, .. } => AppError::ExecutionFailed {
            message: format!("WSL Worker request failed during {phase}: {code}"),
        },
        _ => AppError::ConfigurationCorrupted {
            message: format!("WSL Worker returned an invalid {expected} response"),
        },
    }
}

fn cleanup_warning_code(code: &str) -> Result<PayloadCleanupWarningCode, AppError> {
    match code {
        "unknownEntry" => Ok(PayloadCleanupWarningCode::UnknownEntry),
        "invalidMarker" => Ok(PayloadCleanupWarningCode::InvalidMarker),
        "futureMarkerVersion" => Ok(PayloadCleanupWarningCode::FutureMarkerVersion),
        "boundaryRejected" => Ok(PayloadCleanupWarningCode::BoundaryRejected),
        "deleteFailed" => Ok(PayloadCleanupWarningCode::DeleteFailed),
        "sizeUnavailable" => Ok(PayloadCleanupWarningCode::SizeUnavailable),
        _ => Err(AppError::ConfigurationCorrupted {
            message: format!("unknown WSL payload cleanup warning code: {code}"),
        }),
    }
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

#[cfg(test)]
mod tests {
    #[test]
    fn source_paths_are_relative_to_the_bound_worker_source() {
        assert_eq!(
            super::relative_source_path("/home/alice/repo", "/home/alice/repo/skills/demo")
                .unwrap(),
            "skills/demo"
        );
        assert!(super::relative_source_path("/home/alice/repo", "/home/alice/other").is_err());
    }

    #[test]
    fn source_revision_uses_the_skill_directory_instead_of_the_skill_md_blob() {
        assert_eq!(
            super::normalize_skill_revision_path("quick-brainstorm/SKILL.md").unwrap(),
            "quick-brainstorm"
        );
        assert_eq!(
            super::normalize_skill_revision_path("skills/demo/skill.md").unwrap(),
            "skills/demo"
        );
        assert_eq!(
            super::normalize_skill_revision_path("SKILL.md").unwrap(),
            ""
        );
    }
}

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::application::payload_session::{
    PayloadCleanupReport, PayloadCleanupWarning, PayloadCleanupWarningCode, PayloadLocalSource,
    PayloadSessionMaintenance, PayloadSessionStorage, PayloadStorageFuture, PayloadStorageKey,
};
use crate::core::skill_payload::{PayloadEntry, SkillPayload};
use crate::error::AppError;

const MARKER_NAME: &str = ".skill-deck-payload-owner.json";
const MANIFEST_NAME: &str = "payload.json";
const STORAGE_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SessionMarker {
    schema_version: u32,
    session_id: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredPayloadManifest {
    schema_version: u32,
    session_id: String,
    skill_path: String,
    payload_id: String,
    payload_root_hash: String,
    entries: Vec<PayloadEntry>,
}

#[derive(Clone)]
pub struct NativePayloadSessionStorage {
    root: PathBuf,
}

impl NativePayloadSessionStorage {
    pub fn new(root: impl AsRef<Path>) -> Result<Self, AppError> {
        fs::create_dir_all(root.as_ref())?;
        Ok(Self {
            root: fs::canonicalize(root.as_ref())?,
        })
    }

    fn session_dir(&self, session_id: &str) -> PathBuf {
        self.root.join(format!("session-{}", digest(session_id)))
    }

    fn payload_dir(&self, key: &PayloadStorageKey) -> PathBuf {
        self.session_dir(key.session_id())
            .join(format!("payload-{}", digest(key.skill_path())))
    }

    fn ensure_session(&self, session_id: &str) -> Result<PathBuf, AppError> {
        let session_dir = self.session_dir(session_id);
        fs::create_dir_all(&session_dir)?;
        let marker_path = session_dir.join(MARKER_NAME);
        if marker_path.exists() {
            self.verify_marker(&session_dir, session_id)?;
        } else {
            write_new_file(
                &marker_path,
                &serde_json::to_vec(&SessionMarker {
                    schema_version: STORAGE_SCHEMA_VERSION,
                    session_id: session_id.to_string(),
                })?,
            )?;
        }
        Ok(session_dir)
    }

    fn verify_marker(&self, session_dir: &Path, session_id: &str) -> Result<(), AppError> {
        let marker: SessionMarker =
            serde_json::from_slice(&fs::read(session_dir.join(MARKER_NAME))?)?;
        if marker.schema_version != STORAGE_SCHEMA_VERSION || marker.session_id != session_id {
            return Err(AppError::ConfigurationCorrupted {
                message: "payload session ownership marker does not match".to_string(),
            });
        }
        Ok(())
    }
}

impl PayloadSessionStorage for NativePayloadSessionStorage {
    fn local_source(&self, key: &PayloadStorageKey) -> Result<PayloadLocalSource, AppError> {
        Ok(PayloadLocalSource::NativeManaged {
            payload_root: self.payload_dir(key),
        })
    }

    fn store<'a>(
        &'a self,
        key: &'a PayloadStorageKey,
        payload: SkillPayload,
    ) -> PayloadStorageFuture<'a, Result<u64, AppError>> {
        let storage = self.clone();
        let key = key.clone();
        Box::pin(async move {
            spawn_native_io(move || {
                let session_dir = storage.ensure_session(key.session_id())?;
                let destination = storage.payload_dir(&key);
                if destination.exists() {
                    return Err(AppError::StalePayload);
                }
                let stage = session_dir.join(format!(".stage-{}", Uuid::new_v4().simple()));
                fs::create_dir(&stage)?;
                let result = (|| {
                    let blobs_dir = stage.join("blobs");
                    fs::create_dir(&blobs_dir)?;
                    for (blob_id, content) in &payload.blobs {
                        if !valid_blob_id(blob_id) {
                            return Err(AppError::StalePayload);
                        }
                        write_new_file(&blobs_dir.join(blob_id), content)?;
                    }
                    write_new_file(
                        &stage.join(MANIFEST_NAME),
                        &serde_json::to_vec(&StoredPayloadManifest {
                            schema_version: STORAGE_SCHEMA_VERSION,
                            session_id: key.session_id().to_string(),
                            skill_path: key.skill_path().to_string(),
                            payload_id: payload.payload_id.as_str().to_string(),
                            payload_root_hash: payload.payload_root_hash.clone(),
                            entries: payload.entries.clone(),
                        })?,
                    )?;
                    fs::rename(&stage, &destination)?;
                    Ok(payload.blobs.values().map(|blob| blob.len() as u64).sum())
                })();
                if result.is_err() {
                    let _ = fs::remove_dir_all(&stage);
                }
                result
            })
            .await
        })
    }

    fn verify<'a>(
        &'a self,
        key: &'a PayloadStorageKey,
    ) -> PayloadStorageFuture<
        'a,
        Result<Option<crate::core::skill_payload::SkillPayloadManifest>, AppError>,
    > {
        let storage = self.clone();
        let key = key.clone();
        Box::pin(async move {
            spawn_native_io(move || {
                let session_dir = storage.session_dir(key.session_id());
                let payload_dir = storage.payload_dir(&key);
                if !payload_dir.exists() {
                    return Ok(None);
                }
                storage.verify_marker(&session_dir, key.session_id())?;
                let manifest: StoredPayloadManifest =
                    serde_json::from_slice(&fs::read(payload_dir.join(MANIFEST_NAME))?)?;
                if manifest.schema_version != STORAGE_SCHEMA_VERSION
                    || manifest.session_id != key.session_id()
                    || manifest.skill_path != key.skill_path()
                {
                    return Err(AppError::StalePayload);
                }
                let mut blobs = BTreeMap::new();
                for entry in &manifest.entries {
                    let Some(blob_id) = entry.blob_id.as_deref() else {
                        continue;
                    };
                    if !valid_blob_id(blob_id) {
                        return Err(AppError::StalePayload);
                    }
                    if !blobs.contains_key(blob_id) {
                        blobs.insert(
                            blob_id.to_string(),
                            fs::read(payload_dir.join("blobs").join(blob_id))?,
                        );
                    }
                }
                Ok(Some(
                    SkillPayload::restore_verified(
                        manifest.entries,
                        blobs,
                        manifest.payload_root_hash,
                        manifest.payload_id,
                    )?
                    .manifest(),
                ))
            })
            .await
        })
    }

    fn read_blob<'a>(
        &'a self,
        key: &'a PayloadStorageKey,
        blob_id: &'a str,
    ) -> PayloadStorageFuture<'a, Result<Option<Vec<u8>>, AppError>> {
        let storage = self.clone();
        let key = key.clone();
        let blob_id = blob_id.to_string();
        Box::pin(async move {
            spawn_native_io(move || {
                if !valid_blob_id(&blob_id) {
                    return Err(AppError::StalePayload);
                }
                let session_dir = storage.session_dir(key.session_id());
                let payload_dir = storage.payload_dir(&key);
                if !payload_dir.exists() {
                    return Ok(None);
                }
                storage.verify_marker(&session_dir, key.session_id())?;
                match fs::read(payload_dir.join("blobs").join(&blob_id)) {
                    Ok(blob) => Ok(Some(blob)),
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
                    Err(error) => Err(error.into()),
                }
            })
            .await
        })
    }

    fn remove<'a>(
        &'a self,
        key: &'a PayloadStorageKey,
    ) -> PayloadStorageFuture<'a, Result<(), AppError>> {
        let storage = self.clone();
        let key = key.clone();
        Box::pin(async move {
            spawn_native_io(move || {
                let session_dir = storage.session_dir(key.session_id());
                let payload_dir = storage.payload_dir(&key);
                if !payload_dir.exists() {
                    return Ok(());
                }
                storage.verify_marker(&session_dir, key.session_id())?;
                fs::remove_dir_all(payload_dir)?;
                Ok(())
            })
            .await
        })
    }

    fn remove_session<'a>(
        &'a self,
        session_id: &'a str,
    ) -> PayloadStorageFuture<'a, Result<(), AppError>> {
        let storage = self.clone();
        let session_id = session_id.to_string();
        Box::pin(async move {
            spawn_native_io(move || {
                let session_dir = storage.session_dir(&session_id);
                if !session_dir.exists() {
                    return Ok(());
                }
                storage.verify_marker(&session_dir, &session_id)?;
                fs::remove_dir_all(session_dir)?;
                Ok(())
            })
            .await
        })
    }
}

impl PayloadSessionMaintenance for NativePayloadSessionStorage {
    fn sweep_orphans<'a>(
        &'a self,
        protected_session_ids: &'a std::collections::HashSet<String>,
    ) -> PayloadStorageFuture<'a, Result<PayloadCleanupReport, AppError>> {
        let storage = self.clone();
        let protected_session_ids = protected_session_ids.clone();
        Box::pin(async move {
            spawn_native_io(move || {
                let mut report = PayloadCleanupReport::default();
                let mut entries = fs::read_dir(&storage.root)?.collect::<Result<Vec<_>, _>>()?;
                entries.sort_by_key(|entry| entry.file_name());
                for entry in entries {
                    let path = entry.path();
                    let candidate_name = entry.file_name().to_string_lossy().into_owned();
                    let inspected = inspect_session_candidate(&storage, &path);
                    match inspected {
                        Ok(session_id) if protected_session_ids.contains(&session_id) => {
                            report.protected_sessions = report.protected_sessions.saturating_add(1);
                        }
                        Ok(_) => match fs::remove_dir_all(&path) {
                            Ok(()) => {
                                report.removed_sessions = report.removed_sessions.saturating_add(1)
                            }
                            Err(error) => retain_candidate(
                                &mut report,
                                &path,
                                candidate_name,
                                PayloadCleanupWarningCode::DeleteFailed,
                                Some(error.to_string()),
                            ),
                        },
                        Err((code, details)) => {
                            retain_candidate(&mut report, &path, candidate_name, code, details)
                        }
                    }
                }
                Ok(report)
            })
            .await
        })
    }
}

async fn spawn_native_io<T>(
    operation: impl FnOnce() -> Result<T, AppError> + Send + 'static,
) -> Result<T, AppError>
where
    T: Send + 'static,
{
    tokio::task::spawn_blocking(operation)
        .await
        .map_err(|error| AppError::ExecutionFailed {
            message: format!("native payload task failed: {error}"),
        })?
}

fn inspect_session_candidate(
    storage: &NativePayloadSessionStorage,
    path: &Path,
) -> Result<String, (PayloadCleanupWarningCode, Option<String>)> {
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    if !name.starts_with("session-") {
        return Err((PayloadCleanupWarningCode::UnknownEntry, None));
    }
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        (
            PayloadCleanupWarningCode::InvalidMarker,
            Some(error.to_string()),
        )
    })?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err((PayloadCleanupWarningCode::BoundaryRejected, None));
    }
    let marker: SessionMarker =
        serde_json::from_slice(&fs::read(path.join(MARKER_NAME)).map_err(|error| {
            (
                PayloadCleanupWarningCode::InvalidMarker,
                Some(error.to_string()),
            )
        })?)
        .map_err(|error| {
            (
                PayloadCleanupWarningCode::InvalidMarker,
                Some(error.to_string()),
            )
        })?;
    if marker.schema_version != STORAGE_SCHEMA_VERSION {
        return Err((PayloadCleanupWarningCode::FutureMarkerVersion, None));
    }
    if storage.session_dir(&marker.session_id) != path {
        return Err((PayloadCleanupWarningCode::InvalidMarker, None));
    }
    Ok(marker.session_id)
}

fn retain_candidate(
    report: &mut PayloadCleanupReport,
    path: &Path,
    candidate_name: String,
    code: PayloadCleanupWarningCode,
    technical_details: Option<String>,
) {
    match directory_size_no_follow(path) {
        Ok(bytes) => {
            report.external_retained_bytes = report.external_retained_bytes.saturating_add(bytes)
        }
        Err(error) => {
            report.capacity_blocked = true;
            report.warnings.push(PayloadCleanupWarning {
                code: PayloadCleanupWarningCode::SizeUnavailable,
                candidate_name: Some(candidate_name.clone()),
                technical_details: Some(error.to_string()),
            });
        }
    }
    if matches!(
        code,
        PayloadCleanupWarningCode::InvalidMarker
            | PayloadCleanupWarningCode::FutureMarkerVersion
            | PayloadCleanupWarningCode::BoundaryRejected
            | PayloadCleanupWarningCode::UnknownEntry
    ) {
        report.capacity_blocked = true;
    }
    report.warnings.push(PayloadCleanupWarning {
        code,
        candidate_name: Some(candidate_name),
        technical_details,
    });
}

fn directory_size_no_follow(path: &Path) -> Result<u64, AppError> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() {
        return Err(AppError::UnsafePath {
            path: path.to_string_lossy().into_owned(),
            reason: "payload maintenance does not follow links".to_string(),
        });
    }
    if metadata.is_file() {
        return Ok(metadata.len());
    }
    if !metadata.is_dir() {
        return Err(AppError::UnsafePath {
            path: path.to_string_lossy().into_owned(),
            reason: "payload maintenance entry type is unsupported".to_string(),
        });
    }
    let mut total = 0_u64;
    for entry in fs::read_dir(path)? {
        total = total.saturating_add(directory_size_no_follow(&entry?.path())?);
    }
    Ok(total)
}

fn digest(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}

fn valid_blob_id(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn write_new_file(path: &Path, content: &[u8]) -> Result<(), AppError> {
    use std::io::Write;

    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    file.write_all(content)?;
    file.sync_all()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Arc;

    use tempfile::tempdir;

    use super::*;
    use crate::application::payload_session::{
        load_payload_from_storage, PayloadLocalSource, PayloadSessionLimits,
        PayloadSessionMaintenance, PayloadSessionManager, PayloadSessionStorage, PayloadStorageKey,
    };
    use crate::core::skill_payload::build_skill_payload;
    use crate::environment::types::EnvironmentRef;

    fn payload() -> crate::core::skill_payload::SkillPayload {
        let source = tempdir().expect("source");
        let root = source.path().join("demo");
        fs::create_dir_all(root.join("scripts")).expect("scripts");
        fs::write(root.join("SKILL.md"), [0, 159, 146, 150]).expect("binary");
        fs::write(root.join("scripts/run.sh"), b"#!/bin/sh\n").expect("script");
        build_skill_payload(&root).expect("payload")
    }

    fn key(session_id: &str, skill_path: &str) -> PayloadStorageKey {
        PayloadStorageKey::new(session_id, skill_path)
    }

    #[tokio::test]
    async fn stores_and_reopens_a_verified_full_payload() {
        let cache = tempdir().expect("cache");
        let storage = NativePayloadSessionStorage::new(cache.path()).expect("storage");
        let original = payload();
        let key = key("session-1", "skills/demo");

        let bytes = storage.store(&key, original.clone()).await.expect("store");
        let manifest = storage
            .verify(&key)
            .await
            .expect("verify")
            .expect("manifest");
        let loaded = load_payload_from_storage(&storage, &key, &manifest)
            .await
            .expect("payload");

        assert_eq!(bytes, 14);
        assert_eq!(loaded, original);
        assert_eq!(
            storage.local_source(&key).expect("local source"),
            PayloadLocalSource::NativeManaged {
                payload_root: storage.payload_dir(&key),
            }
        );
    }

    #[tokio::test]
    async fn opaque_storage_key_cannot_escape_the_managed_cache() {
        let parent = tempdir().expect("parent");
        let cache = parent.path().join("cache");
        let storage = NativePayloadSessionStorage::new(&cache).expect("storage");
        storage
            .store(&key("session-2", "../../outside"), payload())
            .await
            .expect("store");

        assert!(!parent.path().join("outside").exists());
        assert_eq!(fs::read_dir(&cache).expect("cache entries").count(), 1);
    }

    #[tokio::test]
    async fn cleanup_requires_a_matching_ownership_marker() {
        let cache = tempdir().expect("cache");
        let storage = NativePayloadSessionStorage::new(cache.path()).expect("storage");
        let key = key("session-3", "skills/demo");
        storage.store(&key, payload()).await.expect("store");
        let unrelated = storage.session_dir("unrelated");
        fs::create_dir(&unrelated).expect("unrelated");

        storage
            .remove_session("session-3")
            .await
            .expect("remove owned");

        assert!(storage.verify(&key).await.expect("load removed").is_none());
        assert!(unrelated.is_dir());
        assert!(storage.remove_session("unrelated").await.is_err());
        assert!(unrelated.is_dir());
    }

    #[tokio::test]
    async fn process_manager_keeps_native_payload_alive_after_acquire_returns() {
        let cache = tempdir().expect("cache");
        let storage = Arc::new(NativePayloadSessionStorage::new(cache.path()).expect("storage"));
        let now = Arc::new(AtomicU64::new(1_000));
        let manager = PayloadSessionManager::new(
            storage,
            PayloadSessionLimits {
                ttl_ms: 100,
                max_sessions: 4,
                max_bytes: 1024,
            },
            {
                let now = now.clone();
                move || now.load(Ordering::SeqCst)
            },
        );
        let discovery = manager
            .discover(EnvironmentRef::Host, "source-v1")
            .await
            .expect("discovery");
        let handle = manager
            .acquire_payload(&discovery, "skills/demo", payload())
            .await
            .expect("handle");

        let lease = manager.pin_verified(&handle).await.expect("lease");
        assert_eq!(lease.manifest().payload_root_hash, handle.manifest_hash);
    }

    #[tokio::test]
    async fn startup_sweep_removes_only_owned_unprotected_payload_sessions() {
        let cache = tempdir().expect("cache");
        let storage = NativePayloadSessionStorage::new(cache.path()).expect("storage");
        let protected = key("protected-session", "skills/protected");
        let orphan = key("orphan-session", "skills/orphan");
        storage
            .store(&protected, payload())
            .await
            .expect("protected");
        storage.store(&orphan, payload()).await.expect("orphan");
        let invalid = cache.path().join("session-invalid");
        fs::create_dir(&invalid).expect("invalid root");
        fs::write(invalid.join("unknown.bin"), b"retained").expect("invalid bytes");

        let report = storage
            .sweep_orphans(&HashSet::from(["protected-session".to_string()]))
            .await
            .expect("sweep");

        assert_eq!(report.removed_sessions, 1);
        assert_eq!(report.protected_sessions, 1);
        assert!(report.capacity_blocked);
        assert!(report.external_retained_bytes >= b"retained".len() as u64);
        assert!(storage.verify(&protected).await.unwrap().is_some());
        assert!(storage.verify(&orphan).await.unwrap().is_none());
        assert!(invalid.is_dir());
    }
}

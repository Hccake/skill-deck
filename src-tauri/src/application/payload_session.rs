use std::collections::{BTreeMap, HashMap, HashSet};
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::{Arc, Mutex, Weak};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use specta::Type;
use uuid::Uuid;

use crate::application::agent_intent::AgentWriteIntent;
use crate::application::mutation::plan::RuntimeRevisions;
use crate::core::mutation::CancellationSignal;
use crate::core::skill_payload::{SkillPayload, SkillPayloadManifest};
use crate::environment::planning::{ResolvedTargetFact, TargetEntryKind};
use crate::environment::runtime::ExecutionBackend;
use crate::environment::types::{
    same_environment_identity, ContextRef, ContextScope, EnvironmentKey, EnvironmentRef,
};
use crate::error::AppError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiscoverySourceLocation {
    Native {
        root: PathBuf,
        ref_revision: Option<String>,
    },
    WslNative {
        distro_name: String,
        linux_root: String,
        ref_revision: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoverySourceDescriptor {
    pub source: String,
    pub source_type: String,
    pub source_url: Option<String>,
    pub ref_name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoverySkillSnapshot {
    pub skill_name: String,
    pub install_dir_name: String,
    pub relative_path: String,
    pub plugin_name: Option<String>,
    pub source_metadata_fingerprint: String,
}

#[derive(Clone)]
pub struct RetainedDiscoverySource {
    location: DiscoverySourceLocation,
    descriptor: DiscoverySourceDescriptor,
    skills: BTreeMap<String, DiscoverySkillSnapshot>,
    _owner: Arc<dyn Send + Sync>,
}

impl RetainedDiscoverySource {
    pub fn new(
        location: DiscoverySourceLocation,
        descriptor: DiscoverySourceDescriptor,
        skills: BTreeMap<String, DiscoverySkillSnapshot>,
        owner: impl Send + Sync + 'static,
    ) -> Self {
        Self {
            location,
            descriptor,
            skills,
            _owner: Arc::new(owner),
        }
    }

    pub fn location(&self) -> &DiscoverySourceLocation {
        &self.location
    }

    pub fn descriptor(&self) -> &DiscoverySourceDescriptor {
        &self.descriptor
    }

    pub fn skill(&self, relative_path: &str) -> Option<&DiscoverySkillSnapshot> {
        self.skills.get(relative_path)
    }

    pub fn skills(&self) -> impl Iterator<Item = &DiscoverySkillSnapshot> {
        self.skills.values()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DiscoverySessionHandle {
    pub session_id: String,
    pub environment: EnvironmentRef,
    pub source_fingerprint: String,
    pub expires_at_epoch_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AcquiredPayloadHandle {
    pub session_id: String,
    pub skill_path: String,
    pub environment: EnvironmentRef,
    pub payload_id: String,
    pub manifest_hash: String,
    pub source_fingerprint: String,
    pub expires_at_epoch_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CopySourceSnapshot {
    pub source_context: ContextRef,
    pub skill_name: String,
    pub revisions: RuntimeRevisions,
    pub lock_entry: Option<Value>,
    pub project_identity: ResolvedTargetFact,
    pub canonical_identity: ResolvedTargetFact,
    pub agent_intents: Vec<AgentWriteIntent>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PayloadPlanningMetadata {
    pub skill_name: String,
    pub install_dir_name: String,
    pub source: String,
    pub source_type: String,
    pub source_url: Option<String>,
    pub ref_name: Option<String>,
    pub skill_path: String,
    pub plugin_name: Option<String>,
    pub computed_hash: String,
    /// Provider 可比较的上游版本，例如 Git tree object ID。
    pub upstream_revision: Option<String>,
}

impl PayloadPlanningMetadata {
    #[cfg(test)]
    fn legacy_incomplete(skill_path: &str) -> Self {
        let name = skill_path
            .trim_end_matches('/')
            .rsplit('/')
            .next()
            .unwrap_or(skill_path)
            .to_string();
        Self {
            skill_name: name.clone(),
            install_dir_name: name,
            source: String::new(),
            source_type: String::new(),
            source_url: None,
            ref_name: None,
            skill_path: skill_path.to_string(),
            plugin_name: None,
            computed_hash: String::new(),
            upstream_revision: None,
        }
    }

    pub fn global_skill_folder_hash(&self) -> String {
        match self.source_type.as_str() {
            "well-known" | "github" => self.upstream_revision.clone().unwrap_or_default(),
            _ => self
                .upstream_revision
                .clone()
                .unwrap_or_else(|| self.computed_hash.clone()),
        }
    }

    pub fn validate(&self) -> Result<(), AppError> {
        if self.skill_name.trim().is_empty()
            || self.install_dir_name.trim().is_empty()
            || self.source.trim().is_empty()
            || self.source_type.trim().is_empty()
            || self.skill_path.trim().is_empty()
            || self.computed_hash.trim().is_empty()
        {
            return Err(AppError::Validation {
                field: Some("payloadPlanningMetadata".to_string()),
                message: "payload planning metadata is incomplete".to_string(),
            });
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy)]
pub struct PayloadSessionLimits {
    pub ttl_ms: u64,
    pub max_sessions: usize,
    pub max_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PayloadStorageKey {
    session_id: String,
    skill_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PayloadLocalSource {
    InProcess,
    NativeManaged {
        payload_root: PathBuf,
    },
    WslManaged {
        distro_name: String,
        payload_root: String,
    },
}

impl PayloadStorageKey {
    pub fn new(session_id: impl Into<String>, skill_path: impl Into<String>) -> Self {
        Self {
            session_id: session_id.into(),
            skill_path: skill_path.into(),
        }
    }

    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    pub fn skill_path(&self) -> &str {
        &self.skill_path
    }
}

pub type PayloadStorageFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PayloadCleanupWarningCode {
    UnknownEntry,
    InvalidMarker,
    FutureMarkerVersion,
    BoundaryRejected,
    DeleteFailed,
    SizeUnavailable,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PayloadCleanupWarning {
    pub code: PayloadCleanupWarningCode,
    pub candidate_name: Option<String>,
    pub technical_details: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PayloadCleanupReport {
    pub removed_sessions: usize,
    pub protected_sessions: usize,
    pub external_retained_bytes: u64,
    pub capacity_blocked: bool,
    pub warnings: Vec<PayloadCleanupWarning>,
}

pub trait PayloadSessionMaintenance: Send + Sync {
    fn sweep_orphans<'a>(
        &'a self,
        protected_session_ids: &'a HashSet<String>,
    ) -> PayloadStorageFuture<'a, Result<PayloadCleanupReport, AppError>>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackendAcquiredPayload {
    pub manifest: SkillPayloadManifest,
    pub total_bytes: u64,
    pub computed_hash: String,
}

pub trait PayloadSessionStorage: Send + Sync {
    fn local_source(&self, _key: &PayloadStorageKey) -> Result<PayloadLocalSource, AppError> {
        Ok(PayloadLocalSource::InProcess)
    }

    fn store<'a>(
        &'a self,
        key: &'a PayloadStorageKey,
        payload: SkillPayload,
    ) -> PayloadStorageFuture<'a, Result<u64, AppError>>;

    fn acquire_from_source_path<'a>(
        &'a self,
        _key: &'a PayloadStorageKey,
        _source_root: &'a str,
        _cancellation: Option<CancellationSignal>,
    ) -> PayloadStorageFuture<'a, Result<BackendAcquiredPayload, AppError>> {
        Box::pin(async {
            Err(AppError::CapabilityUnavailable {
                capability: "backendPayloadAcquisition".to_string(),
                path: None,
            })
        })
    }

    fn source_metadata_fingerprint<'a>(
        &'a self,
        _source_root: &'a str,
    ) -> PayloadStorageFuture<'a, Result<String, AppError>> {
        Box::pin(async {
            Err(AppError::CapabilityUnavailable {
                capability: "backendSourceMetadataFingerprint".to_string(),
                path: None,
            })
        })
    }

    fn source_upstream_revision<'a>(
        &'a self,
        _repository_root: &'a str,
        _skill_path: &'a str,
    ) -> PayloadStorageFuture<'a, Result<Option<String>, AppError>> {
        Box::pin(async { Ok(None) })
    }

    fn verify<'a>(
        &'a self,
        key: &'a PayloadStorageKey,
    ) -> PayloadStorageFuture<'a, Result<Option<SkillPayloadManifest>, AppError>>;

    fn read_blob<'a>(
        &'a self,
        key: &'a PayloadStorageKey,
        blob_id: &'a str,
    ) -> PayloadStorageFuture<'a, Result<Option<Vec<u8>>, AppError>>;

    fn remove<'a>(
        &'a self,
        key: &'a PayloadStorageKey,
    ) -> PayloadStorageFuture<'a, Result<(), AppError>>;

    fn remove_session<'a>(
        &'a self,
        session_id: &'a str,
    ) -> PayloadStorageFuture<'a, Result<(), AppError>>;
}

#[cfg(test)]
#[derive(Default)]
pub struct InMemoryPayloadSessionStorage {
    payloads: Mutex<HashMap<PayloadStorageKey, Arc<SkillPayload>>>,
}

#[cfg(test)]
impl PayloadSessionStorage for InMemoryPayloadSessionStorage {
    fn store<'a>(
        &'a self,
        key: &'a PayloadStorageKey,
        payload: SkillPayload,
    ) -> PayloadStorageFuture<'a, Result<u64, AppError>> {
        Box::pin(async move {
            let bytes = payload.blobs.values().map(|blob| blob.len() as u64).sum();
            lock(&self.payloads)?.insert(key.clone(), Arc::new(payload));
            Ok(bytes)
        })
    }

    fn verify<'a>(
        &'a self,
        key: &'a PayloadStorageKey,
    ) -> PayloadStorageFuture<'a, Result<Option<SkillPayloadManifest>, AppError>> {
        Box::pin(async move {
            let payload = lock(&self.payloads)?.get(key).cloned();
            match payload {
                Some(payload) => {
                    crate::core::skill_payload::verify_skill_payload_integrity(&payload)?;
                    Ok(Some(payload.manifest()))
                }
                None => Ok(None),
            }
        })
    }

    fn read_blob<'a>(
        &'a self,
        key: &'a PayloadStorageKey,
        blob_id: &'a str,
    ) -> PayloadStorageFuture<'a, Result<Option<Vec<u8>>, AppError>> {
        Box::pin(async move {
            Ok(lock(&self.payloads)?
                .get(key)
                .and_then(|payload| payload.blobs.get(blob_id))
                .cloned())
        })
    }

    fn remove<'a>(
        &'a self,
        key: &'a PayloadStorageKey,
    ) -> PayloadStorageFuture<'a, Result<(), AppError>> {
        Box::pin(async move {
            lock(&self.payloads)?.remove(key);
            Ok(())
        })
    }

    fn remove_session<'a>(
        &'a self,
        session_id: &'a str,
    ) -> PayloadStorageFuture<'a, Result<(), AppError>> {
        Box::pin(async move {
            lock(&self.payloads)?.retain(|key, _| key.session_id != session_id);
            Ok(())
        })
    }
}

#[derive(Clone)]
pub struct PayloadSessionManager {
    inner: Arc<PayloadSessionManagerInner>,
}

struct PayloadSessionManagerInner {
    limits: PayloadSessionLimits,
    default_storage: Arc<dyn PayloadSessionStorage>,
    now: Arc<dyn Fn() -> u64 + Send + Sync>,
    sessions: Mutex<HashMap<String, SessionRecord>>,
    maintenance: Mutex<HashMap<EnvironmentKey, PayloadMaintenanceState>>,
}

#[derive(Debug, Clone, Copy, Default)]
struct PayloadMaintenanceState {
    external_retained_bytes: u64,
    capacity_blocked: bool,
    gate: PayloadMaintenanceGate,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum PayloadMaintenanceGate {
    #[default]
    Ready,
    Pending,
    Failed,
}

#[derive(Clone)]
struct SessionRecord {
    environment: EnvironmentRef,
    environment_key: EnvironmentKey,
    invalidated: bool,
    source_fingerprint: String,
    expires_at_epoch_ms: u64,
    created_at_epoch_ms: u64,
    total_bytes: u64,
    pin_count: usize,
    busy_count: usize,
    pending_payloads: HashSet<String>,
    payloads: HashMap<String, PayloadRecord>,
    copy_source_snapshots: HashMap<String, CopySourceSnapshot>,
    storage: Arc<dyn PayloadSessionStorage>,
    retained_source: Option<RetainedDiscoverySource>,
}

#[derive(Clone)]
struct PayloadRecord {
    payload_id: String,
    manifest_hash: String,
    planning_metadata: PayloadPlanningMetadata,
}

pub struct PinnedPayloadLease {
    manager: Weak<PayloadSessionManagerInner>,
    session_id: String,
    storage: Arc<dyn PayloadSessionStorage>,
    key: PayloadStorageKey,
    manifest: SkillPayloadManifest,
    planning_metadata: PayloadPlanningMetadata,
}

impl PinnedPayloadLease {
    pub fn manifest(&self) -> &SkillPayloadManifest {
        &self.manifest
    }

    pub fn planning_metadata(&self) -> &PayloadPlanningMetadata {
        &self.planning_metadata
    }

    pub async fn load_payload(&self) -> Result<SkillPayload, AppError> {
        load_payload_from_storage(self.storage.as_ref(), &self.key, &self.manifest).await
    }

    pub fn local_source(&self) -> Result<PayloadLocalSource, AppError> {
        self.storage.local_source(&self.key)
    }
}

impl Drop for PinnedPayloadLease {
    fn drop(&mut self) {
        let Some(manager) = self.manager.upgrade() else {
            return;
        };
        if let Ok(mut sessions) = manager.sessions.lock() {
            if let Some(session) = sessions.get_mut(&self.session_id) {
                session.pin_count = session.pin_count.saturating_sub(1);
            }
        };
    }
}

impl PayloadSessionManager {
    pub fn new(
        storage: Arc<dyn PayloadSessionStorage>,
        limits: PayloadSessionLimits,
        now: impl Fn() -> u64 + Send + Sync + 'static,
    ) -> Self {
        Self {
            inner: Arc::new(PayloadSessionManagerInner {
                limits,
                default_storage: storage,
                now: Arc::new(now),
                sessions: Mutex::new(HashMap::new()),
                maintenance: Mutex::new(HashMap::new()),
            }),
        }
    }

    #[cfg(test)]
    pub fn in_memory(
        limits: PayloadSessionLimits,
        now: impl Fn() -> u64 + Send + Sync + 'static,
    ) -> Self {
        Self::new(
            Arc::new(InMemoryPayloadSessionStorage::default()),
            limits,
            now,
        )
    }

    pub async fn discover(
        &self,
        environment: EnvironmentRef,
        source_fingerprint: impl Into<String>,
    ) -> Result<DiscoverySessionHandle, AppError> {
        self.discover_internal(
            environment,
            source_fingerprint.into(),
            self.inner.default_storage.clone(),
            None,
        )
        .await
    }

    pub async fn discover_with_source(
        &self,
        environment: EnvironmentRef,
        source_fingerprint: impl Into<String>,
        storage: Arc<dyn PayloadSessionStorage>,
        retained_source: RetainedDiscoverySource,
    ) -> Result<DiscoverySessionHandle, AppError> {
        self.discover_internal(
            environment,
            source_fingerprint.into(),
            storage,
            Some(retained_source),
        )
        .await
    }

    pub async fn discover_with_retained_source(
        &self,
        environment: EnvironmentRef,
        source_fingerprint: impl Into<String>,
        retained_source: RetainedDiscoverySource,
    ) -> Result<DiscoverySessionHandle, AppError> {
        self.discover_internal(
            environment,
            source_fingerprint.into(),
            self.inner.default_storage.clone(),
            Some(retained_source),
        )
        .await
    }

    async fn discover_internal(
        &self,
        environment: EnvironmentRef,
        source_fingerprint: String,
        storage: Arc<dyn PayloadSessionStorage>,
        retained_source: Option<RetainedDiscoverySource>,
    ) -> Result<DiscoverySessionHandle, AppError> {
        let now = (self.inner.now)();
        let session_id = Uuid::new_v4().simple().to_string();
        let expires_at_epoch_ms = now.saturating_add(self.inner.limits.ttl_ms);
        let record = SessionRecord {
            environment: environment.clone(),
            environment_key: EnvironmentKey::from_ref(&environment),
            invalidated: false,
            source_fingerprint: source_fingerprint.clone(),
            expires_at_epoch_ms,
            created_at_epoch_ms: now,
            total_bytes: 0,
            pin_count: 0,
            busy_count: 0,
            pending_payloads: HashSet::new(),
            payloads: HashMap::new(),
            copy_source_snapshots: HashMap::new(),
            storage: storage.clone(),
            retained_source,
        };
        lock(&self.inner.sessions)?.insert(session_id.clone(), record);
        if let Err(error) = self.enforce_capacity(Some(&session_id)).await {
            lock(&self.inner.sessions)?.remove(&session_id);
            storage.remove_session(&session_id).await?;
            return Err(error);
        }

        Ok(DiscoverySessionHandle {
            session_id,
            environment,
            source_fingerprint,
            expires_at_epoch_ms,
        })
    }

    pub fn source_snapshot(
        &self,
        discovery: &DiscoverySessionHandle,
    ) -> Result<RetainedDiscoverySource, AppError> {
        let now = (self.inner.now)();
        let mut sessions = lock(&self.inner.sessions)?;
        valid_discovery_session(&mut sessions, discovery, now)?
            .retained_source
            .clone()
            .ok_or(AppError::StalePayload)
    }

    pub fn storage_for_discovery(
        &self,
        discovery: &DiscoverySessionHandle,
    ) -> Result<Arc<dyn PayloadSessionStorage>, AppError> {
        let now = (self.inner.now)();
        let mut sessions = lock(&self.inner.sessions)?;
        Ok(valid_discovery_session(&mut sessions, discovery, now)?
            .storage
            .clone())
    }

    pub fn existing_payload_handle(
        &self,
        discovery: &DiscoverySessionHandle,
        skill_path: &str,
    ) -> Result<Option<AcquiredPayloadHandle>, AppError> {
        let now = (self.inner.now)();
        let mut sessions = lock(&self.inner.sessions)?;
        let session = valid_discovery_session(&mut sessions, discovery, now)?;
        Ok(session
            .payloads
            .get(skill_path)
            .map(|payload| AcquiredPayloadHandle {
                session_id: discovery.session_id.clone(),
                skill_path: skill_path.to_string(),
                environment: discovery.environment.clone(),
                payload_id: payload.payload_id.clone(),
                manifest_hash: payload.manifest_hash.clone(),
                source_fingerprint: discovery.source_fingerprint.clone(),
                expires_at_epoch_ms: discovery.expires_at_epoch_ms,
            }))
    }

    pub fn bind_copy_source_snapshot(
        &self,
        handle: &AcquiredPayloadHandle,
        snapshot: CopySourceSnapshot,
    ) -> Result<(), AppError> {
        let now = (self.inner.now)();
        let mut sessions = lock(&self.inner.sessions)?;
        validate_payload_handle(&sessions, handle, now)?;
        let session = sessions
            .get_mut(&handle.session_id)
            .expect("validated session");
        let payload = session
            .payloads
            .get(&handle.skill_path)
            .expect("validated payload");
        validate_copy_source_snapshot_binding(handle, payload, &snapshot)?;
        match session.copy_source_snapshots.get(&handle.skill_path) {
            Some(existing) if existing == &snapshot => Ok(()),
            Some(_) => Err(AppError::StalePayload),
            None => {
                session
                    .copy_source_snapshots
                    .insert(handle.skill_path.clone(), snapshot);
                Ok(())
            }
        }
    }

    pub fn copy_source_snapshot(
        &self,
        handle: &AcquiredPayloadHandle,
    ) -> Result<CopySourceSnapshot, AppError> {
        let now = (self.inner.now)();
        let sessions = lock(&self.inner.sessions)?;
        validate_payload_handle(&sessions, handle, now)?;
        let session = sessions.get(&handle.session_id).expect("validated session");
        let payload = session
            .payloads
            .get(&handle.skill_path)
            .expect("validated payload");
        let snapshot = session
            .copy_source_snapshots
            .get(&handle.skill_path)
            .ok_or(AppError::StalePayload)?;
        validate_copy_source_snapshot_binding(handle, payload, snapshot)?;
        Ok(snapshot.clone())
    }

    pub fn protected_session_ids(
        &self,
        environment: &EnvironmentRef,
    ) -> Result<HashSet<String>, AppError> {
        Ok(lock(&self.inner.sessions)?
            .iter()
            .filter(|(_, session)| session.environment_key == EnvironmentKey::from_ref(environment))
            .map(|(session_id, _)| session_id.clone())
            .collect())
    }

    pub fn retire_wsl_sessions(&self) -> usize {
        let Ok(mut sessions) = self.inner.sessions.lock() else {
            log::error!("payload session lock poisoned while retiring WSL sessions");
            return 0;
        };
        let retired_ids = sessions
            .iter()
            .filter(|(_, session)| matches!(session.environment, EnvironmentRef::Wsl { .. }))
            .map(|(session_id, _)| session_id.clone())
            .collect::<Vec<_>>();
        let retired = retired_ids
            .iter()
            .filter_map(|session_id| sessions.remove(session_id))
            .collect::<Vec<_>>();
        let retired_count = retired.len();
        drop(sessions);
        drop(retired);
        retired_count
    }

    pub fn begin_maintenance(&self, environment: &EnvironmentRef) -> Result<(), AppError> {
        self.update_maintenance_state(environment, |state| {
            state.gate = PayloadMaintenanceGate::Pending;
        })
    }

    #[cfg(test)]
    pub fn apply_maintenance_report(
        &self,
        environment: &EnvironmentRef,
        report: &PayloadCleanupReport,
    ) -> Result<(), AppError> {
        self.record_maintenance_report(environment, report)?;
        self.complete_maintenance(environment)
    }

    pub fn record_maintenance_report(
        &self,
        environment: &EnvironmentRef,
        report: &PayloadCleanupReport,
    ) -> Result<(), AppError> {
        self.update_maintenance_state(environment, |state| {
            state.external_retained_bytes = report.external_retained_bytes;
            state.capacity_blocked = report.capacity_blocked;
        })
    }

    pub fn complete_maintenance(&self, environment: &EnvironmentRef) -> Result<(), AppError> {
        self.update_maintenance_state(environment, |state| {
            state.gate = PayloadMaintenanceGate::Ready;
        })
    }

    pub fn fail_maintenance(&self, environment: &EnvironmentRef) -> Result<(), AppError> {
        self.update_maintenance_state(environment, |state| {
            state.gate = PayloadMaintenanceGate::Failed;
        })
    }

    fn update_maintenance_state(
        &self,
        environment: &EnvironmentRef,
        update: impl FnOnce(&mut PayloadMaintenanceState),
    ) -> Result<(), AppError> {
        let mut maintenance = lock(&self.inner.maintenance)?;
        let state = maintenance
            .entry(EnvironmentKey::from_ref(environment))
            .or_default();
        update(state);
        Ok(())
    }

    #[cfg(test)]
    pub async fn acquire_payload(
        &self,
        discovery: &DiscoverySessionHandle,
        skill_path: impl Into<String>,
        payload: SkillPayload,
    ) -> Result<AcquiredPayloadHandle, AppError> {
        let skill_path = skill_path.into();
        let metadata = PayloadPlanningMetadata::legacy_incomplete(&skill_path);
        self.acquire_payload_record(discovery, skill_path, payload, metadata)
            .await
    }

    pub async fn acquire_payload_with_metadata(
        &self,
        discovery: &DiscoverySessionHandle,
        skill_path: impl Into<String>,
        payload: SkillPayload,
        planning_metadata: PayloadPlanningMetadata,
    ) -> Result<AcquiredPayloadHandle, AppError> {
        let skill_path = skill_path.into();
        planning_metadata.validate()?;
        if planning_metadata.skill_path != skill_path {
            return Err(AppError::StalePayload);
        }
        self.acquire_payload_record(discovery, skill_path, payload, planning_metadata)
            .await
    }

    async fn acquire_payload_record(
        &self,
        discovery: &DiscoverySessionHandle,
        skill_path: String,
        payload: SkillPayload,
        planning_metadata: PayloadPlanningMetadata,
    ) -> Result<AcquiredPayloadHandle, AppError> {
        let now = (self.inner.now)();
        let payload_id = payload.payload_id.as_str().to_string();
        let manifest_hash = payload.payload_root_hash.clone();
        let key = PayloadStorageKey::new(&discovery.session_id, &skill_path);

        let (existing, storage) = {
            let mut sessions = lock(&self.inner.sessions)?;
            let session = valid_discovery_session(&mut sessions, discovery, now)?;
            let storage = session.storage.clone();
            if let Some(existing) = session.payloads.get(&skill_path) {
                if existing.payload_id != payload_id
                    || existing.manifest_hash != manifest_hash
                    || existing.planning_metadata != planning_metadata
                {
                    return Err(AppError::StalePayload);
                }
                (true, storage)
            } else {
                if !session.pending_payloads.insert(skill_path.clone()) {
                    return Err(AppError::MutationBusy);
                }
                session.busy_count = session.busy_count.saturating_add(1);
                (false, storage)
            }
        };
        if !existing {
            let stored = storage.store(&key, payload).await;
            let bytes = match stored {
                Ok(bytes) => bytes,
                Err(error) => {
                    finish_payload_io(&self.inner.sessions, discovery, &skill_path)?;
                    return Err(error);
                }
            };
            {
                let mut sessions = lock(&self.inner.sessions)?;
                let session = valid_discovery_session(&mut sessions, discovery, now)?;
                session.pending_payloads.remove(&skill_path);
                session.busy_count = session.busy_count.saturating_sub(1);
                session.total_bytes = session.total_bytes.saturating_add(bytes);
                session.payloads.insert(
                    skill_path.clone(),
                    PayloadRecord {
                        payload_id: payload_id.clone(),
                        manifest_hash: manifest_hash.clone(),
                        planning_metadata: planning_metadata.clone(),
                    },
                );
            }
            if let Err(error) = self.enforce_capacity(Some(&discovery.session_id)).await {
                storage.remove(&key).await?;
                if let Some(session) = lock(&self.inner.sessions)?.get_mut(&discovery.session_id) {
                    session.payloads.remove(&skill_path);
                    session.total_bytes = session.total_bytes.saturating_sub(bytes);
                }
                return Err(error);
            }
        }

        Ok(AcquiredPayloadHandle {
            session_id: discovery.session_id.clone(),
            skill_path,
            environment: discovery.environment.clone(),
            payload_id,
            manifest_hash,
            source_fingerprint: discovery.source_fingerprint.clone(),
            expires_at_epoch_ms: discovery.expires_at_epoch_ms,
        })
    }

    #[cfg(test)]
    pub async fn register_existing_payload(
        &self,
        discovery: &DiscoverySessionHandle,
        skill_path: impl Into<String>,
        manifest: SkillPayloadManifest,
        total_bytes: u64,
    ) -> Result<AcquiredPayloadHandle, AppError> {
        let skill_path = skill_path.into();
        let metadata = PayloadPlanningMetadata::legacy_incomplete(&skill_path);
        self.register_existing_payload_record(
            discovery,
            skill_path,
            manifest,
            total_bytes,
            metadata,
        )
        .await
    }

    pub async fn register_existing_payload_with_metadata(
        &self,
        discovery: &DiscoverySessionHandle,
        skill_path: impl Into<String>,
        manifest: SkillPayloadManifest,
        total_bytes: u64,
        planning_metadata: PayloadPlanningMetadata,
    ) -> Result<AcquiredPayloadHandle, AppError> {
        let skill_path = skill_path.into();
        planning_metadata.validate()?;
        if planning_metadata.skill_path != skill_path {
            return Err(AppError::StalePayload);
        }
        self.register_existing_payload_record(
            discovery,
            skill_path,
            manifest,
            total_bytes,
            planning_metadata,
        )
        .await
    }

    async fn register_existing_payload_record(
        &self,
        discovery: &DiscoverySessionHandle,
        skill_path: String,
        manifest: SkillPayloadManifest,
        total_bytes: u64,
        planning_metadata: PayloadPlanningMetadata,
    ) -> Result<AcquiredPayloadHandle, AppError> {
        crate::core::skill_payload::verify_skill_payload_manifest(&manifest)?;
        let now = (self.inner.now)();
        let payload_id = manifest.payload_id().as_str().to_string();
        let manifest_hash = manifest.payload_root_hash.clone();
        let key = PayloadStorageKey::new(&discovery.session_id, &skill_path);
        let (inserted, storage) = {
            let mut sessions = lock(&self.inner.sessions)?;
            let session = valid_discovery_session(&mut sessions, discovery, now)?;
            let storage = session.storage.clone();
            if session.pending_payloads.contains(&skill_path) {
                return Err(AppError::MutationBusy);
            }
            if let Some(existing) = session.payloads.get(&skill_path) {
                if existing.payload_id != payload_id
                    || existing.manifest_hash != manifest_hash
                    || existing.planning_metadata != planning_metadata
                {
                    return Err(AppError::StalePayload);
                }
                (false, storage)
            } else {
                session.total_bytes = session.total_bytes.saturating_add(total_bytes);
                session.payloads.insert(
                    skill_path.clone(),
                    PayloadRecord {
                        payload_id: payload_id.clone(),
                        manifest_hash: manifest_hash.clone(),
                        planning_metadata: planning_metadata.clone(),
                    },
                );
                (true, storage)
            }
        };
        if inserted {
            if let Err(error) = self.enforce_capacity(Some(&discovery.session_id)).await {
                storage.remove(&key).await?;
                if let Some(session) = lock(&self.inner.sessions)?.get_mut(&discovery.session_id) {
                    session.payloads.remove(&skill_path);
                    session.total_bytes = session.total_bytes.saturating_sub(total_bytes);
                }
                return Err(error);
            }
        }
        Ok(AcquiredPayloadHandle {
            session_id: discovery.session_id.clone(),
            skill_path,
            environment: discovery.environment.clone(),
            payload_id,
            manifest_hash,
            source_fingerprint: discovery.source_fingerprint.clone(),
            expires_at_epoch_ms: discovery.expires_at_epoch_ms,
        })
    }

    pub async fn pin_verified(
        &self,
        handle: &AcquiredPayloadHandle,
    ) -> Result<PinnedPayloadLease, AppError> {
        self.ensure_maintenance_ready(&handle.environment)?;
        let now = (self.inner.now)();
        let key = PayloadStorageKey::new(&handle.session_id, &handle.skill_path);
        let storage = {
            let mut sessions = lock(&self.inner.sessions)?;
            validate_payload_handle(&sessions, handle, now)?;
            let session = sessions
                .get_mut(&handle.session_id)
                .expect("validated session");
            session.busy_count += 1;
            session.storage.clone()
        };
        let verified = storage.verify(&key).await;
        let manifest = match verified {
            Ok(Some(manifest)) => manifest,
            Ok(None) => {
                finish_handle_io(&self.inner.sessions, handle)?;
                return Err(AppError::StalePayload);
            }
            Err(error) => {
                finish_handle_io(&self.inner.sessions, handle)?;
                return Err(error);
            }
        };
        if manifest.payload_id().as_str() != handle.payload_id
            || manifest.payload_root_hash != handle.manifest_hash
        {
            finish_handle_io(&self.inner.sessions, handle)?;
            return Err(AppError::StalePayload);
        }
        let planning_metadata = {
            let mut sessions = lock(&self.inner.sessions)?;
            validate_payload_handle(&sessions, handle, now)?;
            let session = sessions
                .get_mut(&handle.session_id)
                .expect("validated session");
            session.busy_count = session.busy_count.saturating_sub(1);
            session.pin_count = session.pin_count.saturating_add(1);
            session
                .payloads
                .get(&handle.skill_path)
                .expect("validated payload")
                .planning_metadata
                .clone()
        };

        Ok(PinnedPayloadLease {
            manager: Arc::downgrade(&self.inner),
            session_id: handle.session_id.clone(),
            storage,
            key,
            manifest,
            planning_metadata,
        })
    }

    pub async fn pin_derived_payload(
        &self,
        canonical: &PinnedPayloadLease,
        derivation_id: &str,
        payload: SkillPayload,
    ) -> Result<PinnedPayloadLease, AppError> {
        if derivation_id.is_empty()
            || derivation_id.len() > 64
            || !derivation_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        {
            return Err(AppError::StalePayload);
        }
        crate::core::skill_payload::verify_skill_payload_integrity(&payload)?;
        let owner = canonical.manager.upgrade().ok_or(AppError::StalePayload)?;
        if !Arc::ptr_eq(&owner, &self.inner) {
            return Err(AppError::StalePayload);
        }
        let now = (self.inner.now)();
        let discovery = {
            let sessions = lock(&self.inner.sessions)?;
            let session = sessions
                .get(&canonical.session_id)
                .filter(|session| session.expires_at_epoch_ms >= now && !session.invalidated)
                .ok_or_else(|| expired(&canonical.session_id))?;
            let record = session
                .payloads
                .get(canonical.key.skill_path())
                .ok_or(AppError::StalePayload)?;
            if record.payload_id != canonical.manifest.payload_id().as_str()
                || record.manifest_hash != canonical.manifest.payload_root_hash
                || record.planning_metadata != canonical.planning_metadata
            {
                return Err(AppError::StalePayload);
            }
            DiscoverySessionHandle {
                session_id: canonical.session_id.clone(),
                environment: session.environment.clone(),
                source_fingerprint: session.source_fingerprint.clone(),
                expires_at_epoch_ms: session.expires_at_epoch_ms,
            }
        };
        let mut key_hasher = Sha256::new();
        key_hasher.update(canonical.key.skill_path().as_bytes());
        key_hasher.update([0]);
        key_hasher.update(derivation_id.as_bytes());
        let storage_key = format!(
            "__skill_deck_derived__/{derivation_id}/{:x}",
            key_hasher.finalize()
        );
        let handle = self
            .acquire_payload_record(
                &discovery,
                storage_key,
                payload,
                canonical.planning_metadata.clone(),
            )
            .await?;
        self.pin_verified(&handle).await
    }

    #[cfg(test)]
    pub async fn cleanup(&self) -> Result<usize, AppError> {
        self.cleanup_internal(None).await
    }

    #[cfg(test)]
    pub async fn invalidate_environment(
        &self,
        environment: &EnvironmentRef,
    ) -> Result<usize, AppError> {
        {
            let mut sessions = lock(&self.inner.sessions)?;
            let environment_key = EnvironmentKey::from_ref(environment);
            for session in sessions.values_mut() {
                if session.environment_key == environment_key {
                    session.invalidated = true;
                }
            }
        }
        self.cleanup_internal(None).await
    }

    async fn enforce_capacity(&self, protected_session_id: Option<&str>) -> Result<(), AppError> {
        let protected_environment = protected_session_id.and_then(|session_id| {
            self.inner
                .sessions
                .lock()
                .ok()?
                .get(session_id)
                .map(|session| (session.environment_key.clone(), session.environment.clone()))
        });
        if let Some((environment_key, environment)) = protected_environment.as_ref() {
            self.ensure_maintenance_ready(environment)?;
            let maintenance = lock(&self.inner.maintenance)?;
            let state = maintenance.get(environment_key).copied();
            if state.is_some_and(|state| state.capacity_blocked) {
                return Err(AppError::PayloadStorageRequiresCleanup {
                    environment: environment.clone(),
                });
            }
        }
        self.cleanup_internal(protected_session_id).await?;
        let sessions = lock(&self.inner.sessions)?;
        let session_bytes = sessions
            .values()
            .map(|session| session.total_bytes)
            .sum::<u64>();
        let external_bytes = lock(&self.inner.maintenance)?
            .values()
            .map(|state| state.external_retained_bytes)
            .sum::<u64>();
        let total_bytes = session_bytes.saturating_add(external_bytes);
        if sessions.len() > self.inner.limits.max_sessions
            || total_bytes > self.inner.limits.max_bytes
        {
            return Err(AppError::CapabilityUnavailable {
                capability: "payloadSessionCapacity".to_string(),
                path: None,
            });
        }
        Ok(())
    }

    fn ensure_maintenance_ready(&self, environment: &EnvironmentRef) -> Result<(), AppError> {
        let state = lock(&self.inner.maintenance)?
            .get(&EnvironmentKey::from_ref(environment))
            .copied();
        match state.map(|state| state.gate) {
            Some(PayloadMaintenanceGate::Pending) => Err(AppError::CapabilityUnavailable {
                capability: "runtimeMaintenancePending".to_string(),
                path: None,
            }),
            Some(PayloadMaintenanceGate::Failed) => Err(AppError::CapabilityUnavailable {
                capability: "runtimeMaintenanceFailed".to_string(),
                path: None,
            }),
            _ => Ok(()),
        }
    }

    async fn cleanup_internal(
        &self,
        protected_session_id: Option<&str>,
    ) -> Result<usize, AppError> {
        let now = (self.inner.now)();
        let external_bytes = lock(&self.inner.maintenance)?
            .values()
            .map(|state| state.external_retained_bytes)
            .sum::<u64>();
        let mut removed = 0;
        loop {
            let candidate = {
                let mut sessions = lock(&self.inner.sessions)?;
                let over_count = sessions.len() > self.inner.limits.max_sessions;
                let over_bytes = sessions
                    .values()
                    .map(|session| session.total_bytes)
                    .sum::<u64>()
                    .saturating_add(external_bytes)
                    > self.inner.limits.max_bytes;
                let mut removable = sessions
                    .iter()
                    .filter(|(session_id, session)| {
                        session.pin_count == 0
                            && session.busy_count == 0
                            && protected_session_id != Some(session_id.as_str())
                            && (session.invalidated
                                || session.expires_at_epoch_ms < now
                                || over_count
                                || over_bytes)
                    })
                    .map(|(session_id, session)| {
                        (
                            session_id.clone(),
                            session.expires_at_epoch_ms < now,
                            session.created_at_epoch_ms,
                        )
                    })
                    .collect::<Vec<_>>();
                removable.sort_by_key(|(_, expired, created)| (!*expired, *created));
                removable.first().and_then(|(session_id, _, _)| {
                    sessions
                        .remove(session_id)
                        .map(|record| (session_id.clone(), record))
                })
            };
            let Some((session_id, record)) = candidate else {
                break;
            };
            if let Err(error) = record.storage.remove_session(&session_id).await {
                lock(&self.inner.sessions)?.insert(session_id, record);
                return Err(error);
            }
            removed += 1;
        }
        Ok(removed)
    }
}

fn valid_discovery_session<'a>(
    sessions: &'a mut HashMap<String, SessionRecord>,
    discovery: &DiscoverySessionHandle,
    now: u64,
) -> Result<&'a mut SessionRecord, AppError> {
    let session = sessions
        .get_mut(&discovery.session_id)
        .filter(|session| session.expires_at_epoch_ms >= now)
        .ok_or_else(|| expired(&discovery.session_id))?;
    if session.invalidated {
        return Err(AppError::StaleEnvironment);
    }
    if session.environment_key != EnvironmentKey::from_ref(&discovery.environment)
        || session.source_fingerprint != discovery.source_fingerprint
        || session.expires_at_epoch_ms != discovery.expires_at_epoch_ms
    {
        return Err(AppError::StalePayload);
    }
    Ok(session)
}

fn validate_payload_handle(
    sessions: &HashMap<String, SessionRecord>,
    handle: &AcquiredPayloadHandle,
    now: u64,
) -> Result<(), AppError> {
    let session = sessions
        .get(&handle.session_id)
        .filter(|session| session.expires_at_epoch_ms >= now)
        .ok_or_else(|| expired(&handle.session_id))?;
    if session.invalidated {
        return Err(AppError::StaleEnvironment);
    }
    let payload = session
        .payloads
        .get(&handle.skill_path)
        .ok_or(AppError::StalePayload)?;
    if session.environment_key != EnvironmentKey::from_ref(&handle.environment)
        || session.source_fingerprint != handle.source_fingerprint
        || session.expires_at_epoch_ms != handle.expires_at_epoch_ms
        || payload.payload_id != handle.payload_id
        || payload.manifest_hash != handle.manifest_hash
    {
        return Err(AppError::StalePayload);
    }
    Ok(())
}

fn validate_copy_source_snapshot_binding(
    handle: &AcquiredPayloadHandle,
    payload: &PayloadRecord,
    snapshot: &CopySourceSnapshot,
) -> Result<(), AppError> {
    let valid_project = matches!(
        &snapshot.source_context.scope,
        ContextScope::Project { project_id } if !project_id.trim().is_empty()
    );
    let host_identity = matches!(
        snapshot.project_identity.key.backend,
        ExecutionBackend::NativeWindows | ExecutionBackend::NativeUnix
    ) && matches!(
        snapshot.project_identity.destination.environment,
        EnvironmentRef::Host
    ) && snapshot.project_identity.entry_kind == TargetEntryKind::Directory;
    if !valid_project
        || !same_environment_identity(&snapshot.source_context.environment, &handle.environment)
        || snapshot.skill_name != payload.planning_metadata.skill_name
        || !host_identity
    {
        return Err(AppError::StalePayload);
    }
    Ok(())
}

fn finish_payload_io(
    sessions: &Mutex<HashMap<String, SessionRecord>>,
    discovery: &DiscoverySessionHandle,
    skill_path: &str,
) -> Result<(), AppError> {
    if let Some(session) = lock(sessions)?.get_mut(&discovery.session_id) {
        session.pending_payloads.remove(skill_path);
        session.busy_count = session.busy_count.saturating_sub(1);
    }
    Ok(())
}

fn finish_handle_io(
    sessions: &Mutex<HashMap<String, SessionRecord>>,
    handle: &AcquiredPayloadHandle,
) -> Result<(), AppError> {
    if let Some(session) = lock(sessions)?.get_mut(&handle.session_id) {
        session.busy_count = session.busy_count.saturating_sub(1);
    }
    Ok(())
}

pub(crate) async fn load_payload_from_storage(
    storage: &dyn PayloadSessionStorage,
    key: &PayloadStorageKey,
    manifest: &SkillPayloadManifest,
) -> Result<SkillPayload, AppError> {
    let mut blobs = std::collections::BTreeMap::new();
    for entry in &manifest.entries {
        let Some(blob_id) = entry.blob_id.as_deref() else {
            continue;
        };
        if !blobs.contains_key(blob_id) {
            let blob = storage
                .read_blob(key, blob_id)
                .await?
                .ok_or(AppError::StalePayload)?;
            blobs.insert(blob_id.to_string(), blob);
        }
    }
    SkillPayload::restore_verified(
        manifest.entries.clone(),
        blobs,
        manifest.payload_root_hash.clone(),
        manifest.payload_id().as_str().to_string(),
    )
}

fn expired(session_id: &str) -> AppError {
    AppError::PayloadSessionExpired {
        session_id: session_id.to_string(),
    }
}

fn lock<T>(mutex: &Mutex<T>) -> Result<std::sync::MutexGuard<'_, T>, AppError> {
    mutex.lock().map_err(|_| AppError::Io {
        message: "payload session state is unavailable".to_string(),
    })
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::{Arc, Mutex};

    use tempfile::tempdir;

    use super::*;
    use crate::core::skill_payload::build_skill_payload;
    use crate::environment::runtime::{
        ContextSnapshotRevision, EntryFingerprint, PhysicalParentIdentity, PhysicalTargetKey,
    };
    use crate::environment::types::{EnvironmentRef, ResourceLocator};
    use crate::error::AppError;

    fn payload() -> crate::core::skill_payload::SkillPayload {
        let temp = tempdir().expect("tempdir");
        let root = temp.path().join("demo");
        fs::create_dir_all(&root).expect("root");
        fs::write(root.join("SKILL.md"), b"skill").expect("skill");
        build_skill_payload(&root).expect("payload")
    }

    fn manager(now: Arc<AtomicU64>) -> PayloadSessionManager {
        PayloadSessionManager::in_memory(
            PayloadSessionLimits {
                ttl_ms: 100,
                max_sessions: 4,
                max_bytes: 1024 * 1024,
            },
            move || now.load(Ordering::SeqCst),
        )
    }

    fn copy_source_snapshot(environment: EnvironmentRef) -> CopySourceSnapshot {
        CopySourceSnapshot {
            source_context: ContextRef {
                environment,
                scope: ContextScope::Project {
                    project_id: "source-project".to_string(),
                },
            },
            skill_name: "demo".to_string(),
            revisions: RuntimeRevisions {
                registry: "registry-1".to_string(),
                environment: "environment-1".to_string(),
                context: ContextSnapshotRevision::parse("context-source-1").unwrap(),
            },
            lock_entry: Some(serde_json::json!({
                "source": "owner/repo",
                "sourceType": "github"
            })),
            project_identity: ResolvedTargetFact {
                key: PhysicalTargetKey {
                    backend: ExecutionBackend::NativeUnix,
                    physical_parent: PhysicalParentIdentity::Unix {
                        device: 1,
                        inode: 2,
                    },
                    normalized_final_child_name: "source".to_string(),
                },
                destination: ResourceLocator {
                    environment: EnvironmentRef::Host,
                    native_path: "/work/source".to_string(),
                },
                fingerprint: EntryFingerprint("entry-v1-source".to_string()),
                entry_kind: TargetEntryKind::Directory,
                link_target: None,
            },
            canonical_identity: ResolvedTargetFact {
                key: PhysicalTargetKey {
                    backend: ExecutionBackend::NativeUnix,
                    physical_parent: PhysicalParentIdentity::Unix {
                        device: 1,
                        inode: 3,
                    },
                    normalized_final_child_name: "demo".to_string(),
                },
                destination: ResourceLocator {
                    environment: EnvironmentRef::Host,
                    native_path: "/source/.agents/skills/demo".to_string(),
                },
                fingerprint: EntryFingerprint("entry-v1-demo".to_string()),
                entry_kind: TargetEntryKind::Directory,
                link_target: None,
            },
            agent_intents: Vec::new(),
        }
    }

    #[derive(Default)]
    struct MutableStorage {
        payloads: Mutex<HashMap<PayloadStorageKey, SkillPayload>>,
        blob_reads: std::sync::atomic::AtomicUsize,
        stores: std::sync::atomic::AtomicUsize,
    }

    impl MutableStorage {
        fn tamper_blob(&self) {
            let mut payloads = self.payloads.lock().expect("storage");
            let payload = payloads.values_mut().next().expect("payload");
            payload.blobs.values_mut().next().expect("blob")[0] ^= 0xff;
        }

        fn tamper_manifest(&self) {
            let mut payloads = self.payloads.lock().expect("storage");
            let payload = payloads.values_mut().next().expect("payload");
            payload.entries[0].executable = !payload.entries[0].executable;
        }

        fn len(&self) -> usize {
            self.payloads.lock().expect("storage").len()
        }

        fn blob_reads(&self) -> usize {
            self.blob_reads.load(Ordering::SeqCst)
        }

        fn seed(&self, key: PayloadStorageKey, payload: SkillPayload) {
            self.payloads.lock().expect("storage").insert(key, payload);
        }

        fn stores(&self) -> usize {
            self.stores.load(Ordering::SeqCst)
        }
    }

    impl PayloadSessionStorage for MutableStorage {
        fn store<'a>(
            &'a self,
            key: &'a PayloadStorageKey,
            payload: SkillPayload,
        ) -> PayloadStorageFuture<'a, Result<u64, AppError>> {
            Box::pin(async move {
                self.stores.fetch_add(1, Ordering::SeqCst);
                let bytes = payload.blobs.values().map(|blob| blob.len() as u64).sum();
                self.payloads
                    .lock()
                    .expect("storage")
                    .insert(key.clone(), payload);
                Ok(bytes)
            })
        }

        fn verify<'a>(
            &'a self,
            key: &'a PayloadStorageKey,
        ) -> PayloadStorageFuture<'a, Result<Option<SkillPayloadManifest>, AppError>> {
            Box::pin(async move {
                let payload = self.payloads.lock().expect("storage").get(key).cloned();
                match payload {
                    Some(payload) => {
                        crate::core::skill_payload::verify_skill_payload_integrity(&payload)?;
                        Ok(Some(payload.manifest()))
                    }
                    None => Ok(None),
                }
            })
        }

        fn read_blob<'a>(
            &'a self,
            key: &'a PayloadStorageKey,
            blob_id: &'a str,
        ) -> PayloadStorageFuture<'a, Result<Option<Vec<u8>>, AppError>> {
            Box::pin(async move {
                self.blob_reads.fetch_add(1, Ordering::SeqCst);
                Ok(self
                    .payloads
                    .lock()
                    .expect("storage")
                    .get(key)
                    .and_then(|payload| payload.blobs.get(blob_id))
                    .cloned())
            })
        }

        fn remove<'a>(
            &'a self,
            key: &'a PayloadStorageKey,
        ) -> PayloadStorageFuture<'a, Result<(), AppError>> {
            Box::pin(async move {
                self.payloads.lock().expect("storage").remove(key);
                Ok(())
            })
        }

        fn remove_session<'a>(
            &'a self,
            session_id: &'a str,
        ) -> PayloadStorageFuture<'a, Result<(), AppError>> {
            Box::pin(async move {
                self.payloads
                    .lock()
                    .expect("storage")
                    .retain(|key, _| key.session_id != session_id);
                Ok(())
            })
        }
    }

    fn manager_with_storage(
        storage: Arc<dyn PayloadSessionStorage>,
        limits: PayloadSessionLimits,
        now: Arc<AtomicU64>,
    ) -> PayloadSessionManager {
        PayloadSessionManager::new(storage, limits, move || now.load(Ordering::SeqCst))
    }

    struct DropCounter(Arc<AtomicU64>);

    impl Drop for DropCounter {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    #[tokio::test]
    async fn retained_source_owner_lives_until_the_discovery_session_is_cleaned() {
        let now = Arc::new(AtomicU64::new(1_000));
        let drops = Arc::new(AtomicU64::new(0));
        let manager = manager(now.clone());
        let discovery = manager
            .discover_with_source(
                EnvironmentRef::Host,
                "source-v1",
                Arc::new(InMemoryPayloadSessionStorage::default()),
                RetainedDiscoverySource::new(
                    DiscoverySourceLocation::Native {
                        root: PathBuf::from("/managed/source"),
                        ref_revision: None,
                    },
                    DiscoverySourceDescriptor {
                        source: "source".to_string(),
                        source_type: "local".to_string(),
                        source_url: None,
                        ref_name: None,
                    },
                    BTreeMap::new(),
                    DropCounter(drops.clone()),
                ),
            )
            .await
            .expect("discovery");

        assert_eq!(drops.load(Ordering::SeqCst), 0);
        assert_eq!(
            manager
                .source_snapshot(&discovery)
                .expect("retained source")
                .location(),
            &DiscoverySourceLocation::Native {
                root: PathBuf::from("/managed/source"),
                ref_revision: None,
            }
        );

        now.store(1_101, Ordering::SeqCst);
        assert_eq!(manager.cleanup().await.expect("cleanup"), 1);
        assert_eq!(drops.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn each_discovery_session_uses_its_own_backend_storage() {
        let now = Arc::new(AtomicU64::new(2_000));
        let default_storage = Arc::new(MutableStorage::default());
        let wsl_storage = Arc::new(MutableStorage::default());
        let manager = manager_with_storage(
            default_storage.clone(),
            PayloadSessionLimits {
                ttl_ms: 100,
                max_sessions: 4,
                max_bytes: 1024 * 1024,
            },
            now,
        );
        let discovery = manager
            .discover_with_source(
                EnvironmentRef::Wsl {
                    distro_name: "Ubuntu-24.04".to_string(),
                },
                "source-v1",
                wsl_storage.clone(),
                RetainedDiscoverySource::new(
                    DiscoverySourceLocation::WslNative {
                        distro_name: "Ubuntu-24.04".to_string(),
                        linux_root: "/mnt/c/tmp/skill-deck-source-1/repo".to_string(),
                        ref_revision: None,
                    },
                    DiscoverySourceDescriptor {
                        source: "source".to_string(),
                        source_type: "git".to_string(),
                        source_url: Some("https://example.com/repo.git".to_string()),
                        ref_name: None,
                    },
                    BTreeMap::new(),
                    (),
                ),
            )
            .await
            .expect("discovery");

        let handle = manager
            .acquire_payload(&discovery, "skills/demo", payload())
            .await
            .expect("payload handle");
        let lease = manager.pin_verified(&handle).await.expect("pin");

        assert_eq!(default_storage.stores(), 0);
        assert_eq!(wsl_storage.stores(), 1);
        assert_eq!(lease.manifest().payload_root_hash, handle.manifest_hash);
    }

    #[tokio::test]
    async fn preview_handle_holds_no_lease_and_expires_normally() {
        let now = Arc::new(AtomicU64::new(1_000));
        let manager = manager(now.clone());
        let discovery = manager
            .discover(EnvironmentRef::Host, "source-v1")
            .await
            .expect("discovery");
        let handle = manager
            .acquire_payload(&discovery, "skills/demo", payload())
            .await
            .expect("payload handle");

        now.store(1_101, Ordering::SeqCst);
        assert_eq!(manager.cleanup().await.expect("cleanup"), 1);
        assert!(matches!(
            manager.pin_verified(&handle).await,
            Err(AppError::PayloadSessionExpired { .. })
        ));
    }

    #[tokio::test]
    async fn copy_source_snapshot_uses_the_payload_handle_lifetime() {
        let now = Arc::new(AtomicU64::new(2_500));
        let manager = manager(now.clone());
        let discovery = manager
            .discover(EnvironmentRef::Host, "source-v1")
            .await
            .expect("discovery");
        let handle = manager
            .acquire_payload(&discovery, "skills/demo", payload())
            .await
            .expect("payload handle");
        let snapshot = copy_source_snapshot(EnvironmentRef::Host);

        manager
            .bind_copy_source_snapshot(&handle, snapshot.clone())
            .expect("bind snapshot");
        assert_eq!(
            manager
                .copy_source_snapshot(&handle)
                .expect("read snapshot"),
            snapshot
        );

        now.store(2_601, Ordering::SeqCst);
        assert!(matches!(
            manager.copy_source_snapshot(&handle),
            Err(AppError::PayloadSessionExpired { .. })
        ));
    }

    #[tokio::test]
    async fn copy_source_snapshot_rejects_conflicting_or_forged_bindings() {
        let now = Arc::new(AtomicU64::new(3_000));
        let manager = manager(now);
        let discovery = manager
            .discover(EnvironmentRef::Host, "source-v1")
            .await
            .expect("discovery");
        let handle = manager
            .acquire_payload(&discovery, "skills/demo", payload())
            .await
            .expect("payload handle");
        let snapshot = copy_source_snapshot(EnvironmentRef::Host);

        manager
            .bind_copy_source_snapshot(&handle, snapshot.clone())
            .expect("bind snapshot");
        manager
            .bind_copy_source_snapshot(&handle, snapshot.clone())
            .expect("idempotent binding");

        let mut conflicting = snapshot;
        conflicting.lock_entry = None;
        assert!(matches!(
            manager.bind_copy_source_snapshot(&handle, conflicting),
            Err(AppError::StalePayload)
        ));

        let mut forged = handle;
        forged.environment = EnvironmentRef::Wsl {
            distro_name: "Ubuntu".to_string(),
        };
        assert!(matches!(
            manager.copy_source_snapshot(&forged),
            Err(AppError::StalePayload)
        ));
    }

    #[tokio::test]
    async fn execute_lease_protects_payload_until_drop() {
        let now = Arc::new(AtomicU64::new(5_000));
        let manager = manager(now.clone());
        let discovery = manager
            .discover(EnvironmentRef::Host, "source-v1")
            .await
            .expect("discovery");
        let handle = manager
            .acquire_payload(&discovery, "skills/demo", payload())
            .await
            .expect("payload handle");
        let lease = manager.pin_verified(&handle).await.expect("pin payload");

        now.store(5_101, Ordering::SeqCst);
        assert_eq!(manager.cleanup().await.expect("protected cleanup"), 0);
        assert_eq!(lease.manifest().payload_root_hash, handle.manifest_hash);

        drop(lease);
        assert_eq!(manager.cleanup().await.expect("cleanup after drop"), 1);
    }

    #[tokio::test]
    async fn pin_verifies_only_manifest_until_bridge_requests_payload_blobs() {
        let now = Arc::new(AtomicU64::new(8_000));
        let storage = Arc::new(MutableStorage::default());
        let manager = manager_with_storage(
            storage.clone(),
            PayloadSessionLimits {
                ttl_ms: 100,
                max_sessions: 4,
                max_bytes: 1024 * 1024,
            },
            now,
        );
        let discovery = manager
            .discover(EnvironmentRef::Host, "source-v1")
            .await
            .expect("discovery");
        let handle = manager
            .acquire_payload(&discovery, "skills/demo", payload())
            .await
            .expect("payload handle");

        let lease = manager.pin_verified(&handle).await.expect("pin");
        assert_eq!(lease.manifest().payload_root_hash, handle.manifest_hash);
        assert_eq!(storage.blob_reads(), 0);
        assert_eq!(
            lease.local_source().expect("backend-local source"),
            PayloadLocalSource::InProcess
        );
        assert_eq!(storage.blob_reads(), 0);

        let bridged = lease.load_payload().await.expect("bridge payload");
        assert_eq!(bridged.payload_root_hash, handle.manifest_hash);
        assert_eq!(storage.blob_reads(), 1);
    }

    #[tokio::test]
    async fn registers_payload_already_acquired_in_the_owning_backend() {
        let now = Arc::new(AtomicU64::new(9_000));
        let storage = Arc::new(MutableStorage::default());
        let manager = manager_with_storage(
            storage.clone(),
            PayloadSessionLimits {
                ttl_ms: 100,
                max_sessions: 4,
                max_bytes: 1024 * 1024,
            },
            now,
        );
        let discovery = manager
            .discover(EnvironmentRef::Host, "source-v1")
            .await
            .expect("discovery");
        let payload = payload();
        let manifest = payload.manifest();
        storage.seed(
            PayloadStorageKey::new(&discovery.session_id, "skills/demo"),
            payload,
        );

        let handle = manager
            .register_existing_payload(&discovery, "skills/demo", manifest, 5)
            .await
            .expect("register existing");
        let lease = manager.pin_verified(&handle).await.expect("pin");
        assert_eq!(lease.manifest().payload_root_hash, handle.manifest_hash);
        assert_eq!(storage.stores(), 0);
    }

    #[tokio::test]
    async fn pin_rejects_tampered_blob_content() {
        let now = Arc::new(AtomicU64::new(10_000));
        let storage = Arc::new(MutableStorage::default());
        let manager = manager_with_storage(
            storage.clone(),
            PayloadSessionLimits {
                ttl_ms: 100,
                max_sessions: 4,
                max_bytes: 1024 * 1024,
            },
            now,
        );
        let discovery = manager
            .discover(EnvironmentRef::Host, "source-v1")
            .await
            .expect("discovery");
        let handle = manager
            .acquire_payload(&discovery, "skills/demo", payload())
            .await
            .expect("payload handle");

        storage.tamper_blob();

        assert!(matches!(
            manager.pin_verified(&handle).await,
            Err(AppError::StalePayload)
        ));
    }

    #[tokio::test]
    async fn pin_rejects_tampered_manifest() {
        let now = Arc::new(AtomicU64::new(20_000));
        let storage = Arc::new(MutableStorage::default());
        let manager = manager_with_storage(
            storage.clone(),
            PayloadSessionLimits {
                ttl_ms: 100,
                max_sessions: 4,
                max_bytes: 1024 * 1024,
            },
            now,
        );
        let discovery = manager
            .discover(EnvironmentRef::Host, "source-v1")
            .await
            .expect("discovery");
        let handle = manager
            .acquire_payload(&discovery, "skills/demo", payload())
            .await
            .expect("payload handle");

        storage.tamper_manifest();

        assert!(matches!(
            manager.pin_verified(&handle).await,
            Err(AppError::StalePayload)
        ));
    }

    #[tokio::test]
    async fn cleanup_waits_for_every_concurrent_lease() {
        let now = Arc::new(AtomicU64::new(30_000));
        let manager = manager(now.clone());
        let discovery = manager
            .discover(EnvironmentRef::Host, "source-v1")
            .await
            .expect("discovery");
        let handle = manager
            .acquire_payload(&discovery, "skills/demo", payload())
            .await
            .expect("payload handle");
        let first = manager.pin_verified(&handle).await.expect("first lease");
        let second = manager.pin_verified(&handle).await.expect("second lease");

        now.store(30_101, Ordering::SeqCst);
        drop(first);
        assert_eq!(manager.cleanup().await.expect("one lease remains"), 0);
        assert_eq!(second.manifest().payload_root_hash, handle.manifest_hash);

        drop(second);
        assert_eq!(manager.cleanup().await.expect("all leases released"), 1);
    }

    #[tokio::test]
    async fn pinned_lease_keeps_immutable_lock_planning_metadata_inside_the_session() {
        let now = Arc::new(AtomicU64::new(35_000));
        let manager = manager(now);
        let discovery = manager
            .discover(EnvironmentRef::Host, "source-v1")
            .await
            .expect("discovery");
        let metadata = PayloadPlanningMetadata {
            skill_name: "demo".to_string(),
            install_dir_name: "demo".to_string(),
            source: "owner/repo".to_string(),
            source_type: "github".to_string(),
            source_url: Some("https://github.com/owner/repo.git".to_string()),
            ref_name: Some("main".to_string()),
            skill_path: "skills/demo".to_string(),
            plugin_name: None,
            computed_hash: "computed-v1".to_string(),
            upstream_revision: Some("tree-v1".to_string()),
        };
        let handle = manager
            .acquire_payload_with_metadata(&discovery, "skills/demo", payload(), metadata.clone())
            .await
            .expect("payload handle");

        let lease = manager.pin_verified(&handle).await.expect("lease");

        assert_eq!(lease.planning_metadata(), &metadata);
        assert!(!serde_json::to_value(&handle)
            .unwrap()
            .as_object()
            .unwrap()
            .contains_key("planningMetadata"));
    }

    #[tokio::test]
    async fn derived_payload_reuses_the_canonical_backend_session_and_metadata() {
        let now = Arc::new(AtomicU64::new(38_000));
        let storage = Arc::new(MutableStorage::default());
        let manager = manager_with_storage(
            storage.clone(),
            PayloadSessionLimits {
                ttl_ms: 100,
                max_sessions: 4,
                max_bytes: 1024 * 1024,
            },
            now,
        );
        let discovery = manager
            .discover(
                EnvironmentRef::Wsl {
                    distro_name: "Ubuntu".to_string(),
                },
                "source-v1",
            )
            .await
            .unwrap();
        let temp = tempdir().unwrap();
        fs::write(
            temp.path().join("SKILL.md"),
            "---\nname: demo\ndescription: Demo\n---\n# Demo\n",
        )
        .unwrap();
        let canonical_payload = build_skill_payload(temp.path()).unwrap();
        let metadata = PayloadPlanningMetadata {
            skill_name: "demo".to_string(),
            install_dir_name: "demo".to_string(),
            source: "owner/repo".to_string(),
            source_type: "github".to_string(),
            source_url: Some("https://github.com/owner/repo.git".to_string()),
            ref_name: Some("main".to_string()),
            skill_path: "skills/demo".to_string(),
            plugin_name: None,
            computed_hash: "canonical-computed-hash".to_string(),
            upstream_revision: Some("canonical-tree-hash".to_string()),
        };
        let handle = manager
            .acquire_payload_with_metadata(
                &discovery,
                "skills/demo",
                canonical_payload,
                metadata.clone(),
            )
            .await
            .unwrap();
        let canonical = manager.pin_verified(&handle).await.unwrap();
        let derived_payload =
            crate::core::eve::derive_eve_skill_payload(&canonical.load_payload().await.unwrap())
                .unwrap();

        let first = manager
            .pin_derived_payload(&canonical, "eve", derived_payload.clone())
            .await
            .unwrap();
        let second = manager
            .pin_derived_payload(&canonical, "eve", derived_payload)
            .await
            .unwrap();

        assert_ne!(
            first.manifest().payload_id(),
            canonical.manifest().payload_id()
        );
        assert_eq!(first.manifest(), second.manifest());
        assert_eq!(first.planning_metadata(), &metadata);
        assert_eq!(second.planning_metadata(), &metadata);
        assert_eq!(
            first.local_source().unwrap(),
            canonical.local_source().unwrap()
        );
        assert_eq!(storage.stores(), 2);
    }

    #[tokio::test]
    async fn session_count_limit_never_evicts_a_pinned_session() {
        let now = Arc::new(AtomicU64::new(40_000));
        let storage = Arc::new(MutableStorage::default());
        let manager = manager_with_storage(
            storage,
            PayloadSessionLimits {
                ttl_ms: 100,
                max_sessions: 1,
                max_bytes: 1024 * 1024,
            },
            now,
        );
        let first = manager
            .discover(EnvironmentRef::Host, "source-v1")
            .await
            .expect("first discovery");
        let handle = manager
            .acquire_payload(&first, "skills/demo", payload())
            .await
            .expect("payload handle");
        let lease = manager.pin_verified(&handle).await.expect("lease");

        assert!(matches!(
            manager.discover(EnvironmentRef::Host, "source-v2").await,
            Err(AppError::CapabilityUnavailable { .. })
        ));
        assert_eq!(lease.manifest().payload_root_hash, handle.manifest_hash);

        drop(lease);
        assert_eq!(
            manager
                .cleanup()
                .await
                .expect("failed session was rolled back"),
            0
        );
        manager
            .discover(EnvironmentRef::Host, "source-v3")
            .await
            .expect("old unpinned session can be evicted");
        assert!(matches!(
            manager.pin_verified(&handle).await,
            Err(AppError::PayloadSessionExpired { .. })
        ));
    }

    #[tokio::test]
    async fn oversized_payload_is_rejected_without_leaking_storage() {
        let now = Arc::new(AtomicU64::new(50_000));
        let storage = Arc::new(MutableStorage::default());
        let manager = manager_with_storage(
            storage.clone(),
            PayloadSessionLimits {
                ttl_ms: 100,
                max_sessions: 4,
                max_bytes: 4,
            },
            now,
        );
        let discovery = manager
            .discover(EnvironmentRef::Host, "source-v1")
            .await
            .expect("discovery");

        assert!(matches!(
            manager
                .acquire_payload(&discovery, "skills/demo", payload())
                .await,
            Err(AppError::CapabilityUnavailable { .. })
        ));
        assert_eq!(storage.len(), 0);
    }

    #[tokio::test]
    async fn serialized_handles_expose_identity_but_no_backend_path_or_credentials() {
        let now = Arc::new(AtomicU64::new(60_000));
        let manager = manager(now);
        let discovery = manager
            .discover(EnvironmentRef::Host, "sha256-source")
            .await
            .expect("discovery");
        let handle = manager
            .acquire_payload(&discovery, "skills/demo", payload())
            .await
            .expect("payload handle");

        let json = serde_json::to_value(handle).expect("serialize handle");
        assert_eq!(
            json.as_object()
                .expect("object")
                .keys()
                .map(String::as_str)
                .collect::<std::collections::BTreeSet<_>>(),
            [
                "environment",
                "expiresAtEpochMs",
                "manifestHash",
                "payloadId",
                "sessionId",
                "skillPath",
                "sourceFingerprint",
            ]
            .into_iter()
            .collect()
        );
        let encoded = json.to_string();
        assert!(!encoded.contains("nativePath"));
        assert!(!encoded.contains("credential"));
        assert!(!encoded.contains("prepared"));
    }

    #[tokio::test]
    async fn manager_restart_invalidates_old_handles_even_when_storage_survives() {
        let now = Arc::new(AtomicU64::new(70_000));
        let storage = Arc::new(MutableStorage::default());
        let limits = PayloadSessionLimits {
            ttl_ms: 100,
            max_sessions: 4,
            max_bytes: 1024 * 1024,
        };
        let first_manager = manager_with_storage(storage.clone(), limits, now.clone());
        let discovery = first_manager
            .discover(EnvironmentRef::Host, "source-v1")
            .await
            .expect("discovery");
        let handle = first_manager
            .acquire_payload(&discovery, "skills/demo", payload())
            .await
            .expect("payload handle");
        drop(first_manager);

        let restarted = manager_with_storage(storage, limits, now);
        assert!(matches!(
            restarted.pin_verified(&handle).await,
            Err(AppError::PayloadSessionExpired { .. })
        ));
    }

    #[tokio::test]
    async fn environment_loss_invalidates_handles_but_defers_cleanup_until_lease_drop() {
        let now = Arc::new(AtomicU64::new(80_000));
        let manager = manager(now);
        let environment = EnvironmentRef::Wsl {
            distro_name: "Ubuntu".to_string(),
        };
        let discovery = manager
            .discover(environment.clone(), "source-v1")
            .await
            .unwrap();
        let handle = manager
            .acquire_payload(&discovery, "skills/demo", payload())
            .await
            .unwrap();
        let lease = manager.pin_verified(&handle).await.unwrap();

        assert_eq!(
            manager.invalidate_environment(&environment).await.unwrap(),
            0
        );
        assert!(matches!(
            manager.pin_verified(&handle).await,
            Err(AppError::StaleEnvironment)
        ));
        assert_eq!(manager.cleanup().await.unwrap(), 0);

        drop(lease);
        assert_eq!(manager.cleanup().await.unwrap(), 1);
    }

    #[tokio::test]
    async fn discovery_session_accepts_case_insensitive_wsl_environment_alias() {
        let now = Arc::new(AtomicU64::new(85_000));
        let manager = manager(now);
        let discovery = manager
            .discover(
                EnvironmentRef::Wsl {
                    distro_name: "Ubuntu".to_string(),
                },
                "source-v1",
            )
            .await
            .expect("discovery");
        let mut alias = discovery.clone();
        alias.environment = EnvironmentRef::Wsl {
            distro_name: "UBUNTU".to_string(),
        };

        manager
            .storage_for_discovery(&alias)
            .expect("same environment identity reuses the session");
    }

    #[tokio::test]
    async fn payload_maintenance_accounts_external_bytes_and_blocks_unmeasurable_roots() {
        let now = Arc::new(AtomicU64::new(90_000));
        let storage = Arc::new(MutableStorage::default());
        let manager = manager_with_storage(
            storage.clone(),
            PayloadSessionLimits {
                ttl_ms: 100,
                max_sessions: 4,
                max_bytes: 8,
            },
            now,
        );
        manager
            .apply_maintenance_report(
                &EnvironmentRef::Host,
                &PayloadCleanupReport {
                    removed_sessions: 0,
                    protected_sessions: 0,
                    external_retained_bytes: 4,
                    capacity_blocked: false,
                    warnings: Vec::new(),
                },
            )
            .expect("maintenance report");
        let discovery = manager
            .discover(EnvironmentRef::Host, "source-v1")
            .await
            .expect("discovery within remaining capacity");

        assert!(matches!(
            manager
                .acquire_payload(&discovery, "skills/demo", payload())
                .await,
            Err(AppError::CapabilityUnavailable { .. })
        ));
        assert_eq!(storage.len(), 0);

        manager
            .apply_maintenance_report(
                &EnvironmentRef::Host,
                &PayloadCleanupReport {
                    removed_sessions: 0,
                    protected_sessions: 0,
                    external_retained_bytes: 0,
                    capacity_blocked: true,
                    warnings: vec![PayloadCleanupWarning {
                        code: PayloadCleanupWarningCode::SizeUnavailable,
                        candidate_name: Some("session-future".to_string()),
                        technical_details: None,
                    }],
                },
            )
            .expect("blocked report");

        assert!(matches!(
            manager.discover(EnvironmentRef::Host, "source-v2").await,
            Err(AppError::PayloadStorageRequiresCleanup {
                environment: EnvironmentRef::Host
            })
        ));
    }

    #[tokio::test]
    async fn payload_maintenance_pending_blocks_discovery_until_a_report_is_applied() {
        let now = Arc::new(AtomicU64::new(95_000));
        let manager = manager(now);
        let environment = EnvironmentRef::Wsl {
            distro_name: "Ubuntu".to_string(),
        };

        manager
            .begin_maintenance(&environment)
            .expect("begin maintenance");

        assert!(matches!(
            manager.discover(environment.clone(), "source-pending").await,
            Err(AppError::CapabilityUnavailable { capability, path: None })
                if capability == "runtimeMaintenancePending"
        ));

        manager
            .apply_maintenance_report(&environment, &PayloadCleanupReport::default())
            .expect("maintenance report");

        manager
            .discover(environment, "source-ready")
            .await
            .expect("discovery after maintenance");
    }

    #[tokio::test]
    async fn payload_maintenance_pending_blocks_pinning_existing_handles() {
        let now = Arc::new(AtomicU64::new(97_000));
        let manager = manager(now);
        let discovery = manager
            .discover(EnvironmentRef::Host, "source-existing")
            .await
            .expect("discovery");
        let handle = manager
            .acquire_payload(&discovery, "skills/demo", payload())
            .await
            .expect("payload");
        manager
            .begin_maintenance(&EnvironmentRef::Host)
            .expect("begin maintenance");

        let error = match manager.pin_verified(&handle).await {
            Ok(_) => panic!("maintenance gate did not reject pin"),
            Err(error) => error,
        };
        assert!(
            matches!(
                error,
                AppError::CapabilityUnavailable { ref capability, path: None }
                    if capability == "runtimeMaintenancePending"
            ),
            "unexpected error: {error:?}"
        );
    }

    #[tokio::test]
    async fn failed_runtime_maintenance_returns_a_stable_payload_gate_error() {
        let now = Arc::new(AtomicU64::new(96_000));
        let manager = manager(now);
        let environment = EnvironmentRef::Wsl {
            distro_name: "Ubuntu".to_string(),
        };
        manager
            .begin_maintenance(&environment)
            .expect("begin maintenance");
        manager
            .fail_maintenance(&environment)
            .expect("fail maintenance");

        assert!(matches!(
            manager.discover(
                EnvironmentRef::Wsl {
                    distro_name: "ubuntu".to_string(),
                },
                "source-failed"
            ).await,
            Err(AppError::CapabilityUnavailable { capability, path: None })
                if capability == "runtimeMaintenanceFailed"
        ));
    }

    #[tokio::test]
    async fn protected_session_ids_are_scoped_by_environment() {
        let now = Arc::new(AtomicU64::new(100_000));
        let manager = manager(now);
        let host = manager
            .discover(EnvironmentRef::Host, "host-source")
            .await
            .expect("host");
        let wsl = manager
            .discover(
                EnvironmentRef::Wsl {
                    distro_name: "Ubuntu".to_string(),
                },
                "wsl-source",
            )
            .await
            .expect("wsl");

        assert_eq!(
            manager
                .protected_session_ids(&EnvironmentRef::Host)
                .expect("host protected sessions"),
            HashSet::from([host.session_id])
        );
        assert_eq!(
            manager
                .protected_session_ids(&EnvironmentRef::Wsl {
                    distro_name: "ubuntu".to_string(),
                })
                .expect("WSL protected sessions"),
            HashSet::from([wsl.session_id])
        );
    }

    #[tokio::test]
    async fn retiring_wsl_sessions_keeps_host_sessions_and_invalidates_old_handles() {
        let now = Arc::new(AtomicU64::new(100_000));
        let manager = manager(now);
        let host = manager
            .discover(EnvironmentRef::Host, "host-source")
            .await
            .expect("host");
        let wsl = manager
            .discover(
                EnvironmentRef::Wsl {
                    distro_name: "Ubuntu".to_string(),
                },
                "wsl-source",
            )
            .await
            .expect("wsl");

        assert_eq!(manager.retire_wsl_sessions(), 1);

        assert!(manager.storage_for_discovery(&host).is_ok());
        assert!(matches!(
            manager.storage_for_discovery(&wsl),
            Err(AppError::PayloadSessionExpired { session_id })
                if session_id == wsl.session_id
        ));
    }

    #[test]
    fn global_lock_hash_projection_keeps_cli_semantics_without_inventing_revisions() {
        let metadata =
            |source_type: &str, upstream_revision: Option<&str>| PayloadPlanningMetadata {
                skill_name: "demo".to_string(),
                install_dir_name: "demo".to_string(),
                source: "source".to_string(),
                source_type: source_type.to_string(),
                source_url: None,
                ref_name: None,
                skill_path: "skills/demo".to_string(),
                plugin_name: None,
                computed_hash: "computed-sha256".to_string(),
                upstream_revision: upstream_revision.map(str::to_string),
            };

        assert_eq!(
            metadata("local", None).global_skill_folder_hash(),
            "computed-sha256"
        );
        assert_eq!(metadata("well-known", None).global_skill_folder_hash(), "");
        assert_eq!(metadata("github", None).global_skill_folder_hash(), "");
        assert_eq!(
            metadata("github", Some("tree-object-id")).global_skill_folder_hash(),
            "tree-object-id"
        );
    }
}

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::{Arc, Mutex, MutexGuard};

use serde::{Deserialize, Serialize};
use specta::Type;
use tokio::sync::{watch, Semaphore};

use crate::application::payload_session::DiscoverySessionHandle;
use crate::application::source_evidence_state::SourceEvidenceStateFile;
use crate::application::source_snapshot_reuse::{PayloadAcquisitionKey, SourceSnapshotReuseIndex};
use crate::core::mutation::CancellationSignal;
use crate::core::source_identity::{
    AcquisitionDescriptor, AcquisitionTransportIdentity, NormalizedRef, RemoteSourceIdentity,
    SourceIdentity, SourceProvider,
};
use crate::environment::types::{EnvironmentKey, EnvironmentRef};
use crate::error::AppError;

pub const EVIDENCE_TTL_MS: u64 = 60 * 60 * 1_000;
pub const DETECTOR_CONCURRENCY_LIMIT: usize = 4;
const TRANSIENT_BACKOFF_DELAYS_MS: [u64; 6] = [
    30_000,
    60_000,
    2 * 60_000,
    5 * 60_000,
    10 * 60_000,
    30 * 60_000,
];
const PROVIDER_COOLDOWN_FALLBACK_MS: u64 = 5 * 60_000;
const DIAGNOSTIC_RETENTION_MS: u64 = 7 * 24 * 60 * 60 * 1_000;
const SOURCE_RETENTION_MS: u64 = 30 * 24 * 60 * 60 * 1_000;
const PERSISTED_STATE_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub enum SourceSuppressionWarningCode {
    SuppressionCleanupFailed,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RemoteEvidenceKey {
    pub remote: RemoteSourceIdentity,
    pub normalized_ref: NormalizedRef,
}

impl RemoteEvidenceKey {
    #[cfg(test)]
    pub fn new(remote: RemoteSourceIdentity, normalized_ref: NormalizedRef) -> Self {
        Self {
            remote,
            normalized_ref,
        }
    }

    pub fn from_identity(identity: &SourceIdentity) -> Self {
        Self {
            remote: identity.remote().clone(),
            normalized_ref: identity.normalized_ref().clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProviderThrottleKey {
    pub provider: SourceProvider,
    pub remote_authority: String,
}

impl ProviderThrottleKey {
    #[cfg(test)]
    pub fn new(provider: SourceProvider, remote_authority: impl Into<String>) -> Self {
        Self {
            provider,
            remote_authority: remote_authority.into(),
        }
    }

    pub fn from_identity(identity: &SourceIdentity) -> Self {
        Self {
            provider: identity.remote().provider().clone(),
            remote_authority: identity.remote().authority().to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SkillRevision {
    GitTreeOid(String),
    CliContentHash(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteSnapshotId {
    pub requested_ref: NormalizedRef,
    pub resolved_ref: String,
    pub commit_revision: String,
}

impl RemoteSnapshotId {
    pub fn new(
        requested_ref: NormalizedRef,
        resolved_ref: impl Into<String>,
        commit_revision: impl Into<String>,
    ) -> Self {
        Self {
            requested_ref,
            resolved_ref: resolved_ref.into(),
            commit_revision: commit_revision.into(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct SourceSnapshotFacts {
    pub discovery_session: DiscoverySessionHandle,
    pub snapshot_id: RemoteSnapshotId,
    pub complete_skill_path_catalog: BTreeSet<String>,
}

impl PartialEq for SourceSnapshotFacts {
    fn eq(&self, other: &Self) -> bool {
        self.discovery_session.session_id == other.discovery_session.session_id
            && self.discovery_session.environment == other.discovery_session.environment
            && self.discovery_session.source_fingerprint
                == other.discovery_session.source_fingerprint
            && self.discovery_session.expires_at_epoch_ms
                == other.discovery_session.expires_at_epoch_ms
            && self.snapshot_id == other.snapshot_id
            && self.complete_skill_path_catalog == other.complete_skill_path_catalog
    }
}

impl Eq for SourceSnapshotFacts {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteEvidenceObservation {
    pub snapshot_id: RemoteSnapshotId,
    pub provider_validation: Option<String>,
    pub complete_skill_path_catalog: BTreeSet<String>,
    pub skill_revisions: BTreeMap<String, SkillRevision>,
    pub snapshot_facts: Option<SourceSnapshotFacts>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteEvidenceEntry {
    pub checked_at_epoch_ms: u64,
    pub expires_at_epoch_ms: u64,
    pub snapshot_id: RemoteSnapshotId,
    pub provider_validation: Option<String>,
    pub complete_skill_path_catalog: BTreeSet<String>,
    pub skill_revisions: BTreeMap<String, SkillRevision>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub enum EvidenceFailureReason {
    RateLimited,
    AuthenticationRequired,
    RefNotFound,
    RepositoryNotFound,
    NotFoundOrUnauthorized,
    Network,
    IncompleteEvidence,
    SourceUnavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct EvidenceDetectionFailure {
    pub reason: EvidenceFailureReason,
    pub message: String,
    pub retry_at_epoch_ms: Option<u64>,
    pub provider_cooldown: bool,
}

impl EvidenceDetectionFailure {
    #[cfg(test)]
    pub fn network(message: impl Into<String>) -> Self {
        Self {
            reason: EvidenceFailureReason::Network,
            message: message.into(),
            retry_at_epoch_ms: None,
            provider_cooldown: false,
        }
    }

    fn incomplete(message: impl Into<String>) -> Self {
        Self {
            reason: EvidenceFailureReason::IncompleteEvidence,
            message: message.into(),
            retry_at_epoch_ms: None,
            provider_cooldown: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[expect(
    clippy::large_enum_variant,
    reason = "successful evidence carries the complete typed snapshot used by the coordinator"
)]
pub enum EvidenceDetectionOutcome {
    Modified(RemoteEvidenceObservation),
    NotModified,
    Failed(EvidenceDetectionFailure),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvidenceCheckMode {
    Automatic,
    Force,
}

#[derive(Clone)]
pub struct EvidenceCheckRequest {
    pub environment: EnvironmentRef,
    pub key: RemoteEvidenceKey,
    pub throttle_key: ProviderThrottleKey,
    pub mode: EvidenceCheckMode,
    pub requested_skill_paths: BTreeSet<String>,
    pub acquisition: Arc<AcquisitionDescriptor>,
    pub acquisition_transport_identity: AcquisitionTransportIdentity,
}

#[derive(Clone)]
pub struct EvidenceDetectionRequest {
    pub environment: EnvironmentRef,
    pub key: RemoteEvidenceKey,
    pub requested_skill_paths: BTreeSet<String>,
    pub acquisition: Arc<AcquisitionDescriptor>,
    pub acquisition_transport_identity: AcquisitionTransportIdentity,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct EvidenceAttempt {
    pub checked_at_epoch_ms: u64,
    pub failure: Option<EvidenceDetectionFailure>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub enum EvidenceFreshness {
    Fresh,
    Cached,
    Stale,
    CoolingDown,
    BackingOff,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvidenceCheckResult {
    pub evidence: Option<RemoteEvidenceEntry>,
    pub evidence_is_fresh: bool,
    pub freshness: EvidenceFreshness,
    pub last_attempt: Option<EvidenceAttempt>,
}

pub type EvidenceFuture<'a> =
    Pin<Box<dyn Future<Output = Result<EvidenceDetectionOutcome, AppError>> + Send + 'a>>;

pub trait SourceEvidenceDetector: Send + Sync {
    fn detect<'a>(
        &'a self,
        request: EvidenceDetectionRequest,
        previous: Option<RemoteEvidenceEntry>,
        cancellation: CancellationSignal,
    ) -> EvidenceFuture<'a>;
}

#[derive(Clone)]
pub struct SourceEvidenceCoordinator {
    inner: Arc<SourceEvidenceCoordinatorInner>,
}

struct SourceEvidenceCoordinatorInner {
    detector: Arc<dyn SourceEvidenceDetector>,
    detector_permits: Arc<Semaphore>,
    snapshots: Option<Arc<SourceSnapshotReuseIndex>>,
    now: Arc<dyn Fn() -> u64 + Send + Sync>,
    state: Mutex<CoordinatorState>,
    state_file: Option<SourceEvidenceStateFile>,
    persistence_lock: Mutex<()>,
}

type DetectionCompletion = Result<(), AppError>;
type SealedDetectionBatch = (BTreeSet<String>, Option<RemoteEvidenceEntry>);

#[derive(Clone, Default)]
struct CoordinatorState {
    evidence: HashMap<RemoteEvidenceKey, RemoteEvidenceEntry>,
    last_referenced: HashMap<RemoteEvidenceKey, u64>,
    attempts: HashMap<EnvironmentEvidenceKey, EvidenceAttempt>,
    in_flight: HashMap<EnvironmentEvidenceKey, InFlightDetection>,
    network_backoff: HashMap<EnvironmentEvidenceKey, u64>,
    network_failure_counts: HashMap<EnvironmentEvidenceKey, u32>,
    provider_cooldowns: HashMap<EnvironmentThrottleKey, u64>,
    next_batch_id: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct EnvironmentEvidenceKey {
    environment: EnvironmentKey,
    evidence: RemoteEvidenceKey,
}

impl EnvironmentEvidenceKey {
    fn new(environment: &EnvironmentRef, evidence: &RemoteEvidenceKey) -> Self {
        Self {
            environment: EnvironmentKey::from_ref(environment),
            evidence: evidence.clone(),
        }
    }

    fn from_environment_key(environment: EnvironmentKey, evidence: RemoteEvidenceKey) -> Self {
        Self {
            environment,
            evidence,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct EnvironmentThrottleKey {
    environment: EnvironmentKey,
    throttle: ProviderThrottleKey,
}

impl EnvironmentThrottleKey {
    fn new(environment: &EnvironmentRef, throttle: &ProviderThrottleKey) -> Self {
        Self {
            environment: EnvironmentKey::from_ref(environment),
            throttle: throttle.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PersistedMapEntry<K, V> {
    key: K,
    value: V,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
enum PersistedSourceProvider {
    Github,
    Gitlab,
    Git,
}

impl From<&SourceProvider> for PersistedSourceProvider {
    fn from(value: &SourceProvider) -> Self {
        match value {
            SourceProvider::Github => Self::Github,
            SourceProvider::Gitlab => Self::Gitlab,
            SourceProvider::Git => Self::Git,
        }
    }
}

impl From<PersistedSourceProvider> for SourceProvider {
    fn from(value: PersistedSourceProvider) -> Self {
        match value {
            PersistedSourceProvider::Github => Self::Github,
            PersistedSourceProvider::Gitlab => Self::Gitlab,
            PersistedSourceProvider::Git => Self::Git,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "camelCase")]
enum PersistedNormalizedRef {
    Default,
    Named(String),
}

impl From<&NormalizedRef> for PersistedNormalizedRef {
    fn from(value: &NormalizedRef) -> Self {
        match value {
            NormalizedRef::Default => Self::Default,
            NormalizedRef::Named(value) => Self::Named(value.clone()),
        }
    }
}

impl From<PersistedNormalizedRef> for NormalizedRef {
    fn from(value: PersistedNormalizedRef) -> Self {
        match value {
            PersistedNormalizedRef::Default => Self::Default,
            PersistedNormalizedRef::Named(value) => Self::Named(value),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "camelCase")]
enum PersistedEnvironmentKey {
    #[serde(alias = "host")]
    Native,
    Wsl(String),
}

impl From<&EnvironmentKey> for PersistedEnvironmentKey {
    fn from(value: &EnvironmentKey) -> Self {
        match value {
            EnvironmentKey::Native => Self::Native,
            EnvironmentKey::Wsl(value) => Self::Wsl(value.clone()),
        }
    }
}

impl From<PersistedEnvironmentKey> for EnvironmentKey {
    fn from(value: PersistedEnvironmentKey) -> Self {
        match value {
            PersistedEnvironmentKey::Native => Self::Native,
            PersistedEnvironmentKey::Wsl(value) => Self::wsl(&value),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PersistedRemoteEvidenceKey {
    provider: PersistedSourceProvider,
    authority: String,
    repository: String,
    normalized_ref: PersistedNormalizedRef,
}

impl From<&RemoteEvidenceKey> for PersistedRemoteEvidenceKey {
    fn from(value: &RemoteEvidenceKey) -> Self {
        Self {
            provider: value.remote.provider().into(),
            authority: value.remote.authority().to_string(),
            repository: value.remote.repository().to_string(),
            normalized_ref: (&value.normalized_ref).into(),
        }
    }
}

impl PersistedRemoteEvidenceKey {
    fn into_runtime(self) -> Result<RemoteEvidenceKey, AppError> {
        Ok(RemoteEvidenceKey {
            remote: RemoteSourceIdentity::from_parts(
                self.provider.into(),
                self.authority,
                self.repository,
            )?,
            normalized_ref: self.normalized_ref.into(),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PersistedProviderThrottleKey {
    provider: PersistedSourceProvider,
    remote_authority: String,
}

impl From<&ProviderThrottleKey> for PersistedProviderThrottleKey {
    fn from(value: &ProviderThrottleKey) -> Self {
        Self {
            provider: (&value.provider).into(),
            remote_authority: value.remote_authority.clone(),
        }
    }
}

impl PersistedProviderThrottleKey {
    fn into_runtime(self) -> Result<ProviderThrottleKey, AppError> {
        let remote_authority = self.remote_authority.trim().to_ascii_lowercase();
        if remote_authority.is_empty() {
            return Err(invalid_persisted_state("provider authority is empty"));
        }
        Ok(ProviderThrottleKey {
            provider: self.provider.into(),
            remote_authority,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PersistedEnvironmentEvidenceKey {
    environment: PersistedEnvironmentKey,
    evidence: PersistedRemoteEvidenceKey,
}

impl From<&EnvironmentEvidenceKey> for PersistedEnvironmentEvidenceKey {
    fn from(value: &EnvironmentEvidenceKey) -> Self {
        Self {
            environment: (&value.environment).into(),
            evidence: (&value.evidence).into(),
        }
    }
}

impl PersistedEnvironmentEvidenceKey {
    fn into_runtime(self) -> Result<EnvironmentEvidenceKey, AppError> {
        Ok(EnvironmentEvidenceKey {
            environment: self.environment.into(),
            evidence: self.evidence.into_runtime()?,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PersistedEnvironmentThrottleKey {
    environment: PersistedEnvironmentKey,
    throttle: PersistedProviderThrottleKey,
}

impl From<&EnvironmentThrottleKey> for PersistedEnvironmentThrottleKey {
    fn from(value: &EnvironmentThrottleKey) -> Self {
        Self {
            environment: (&value.environment).into(),
            throttle: (&value.throttle).into(),
        }
    }
}

impl PersistedEnvironmentThrottleKey {
    fn into_runtime(self) -> Result<EnvironmentThrottleKey, AppError> {
        Ok(EnvironmentThrottleKey {
            environment: self.environment.into(),
            throttle: self.throttle.into_runtime()?,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "camelCase")]
enum PersistedSkillRevision {
    GitTreeOid(String),
    CliContentHash(String),
}

impl From<&SkillRevision> for PersistedSkillRevision {
    fn from(value: &SkillRevision) -> Self {
        match value {
            SkillRevision::GitTreeOid(value) => Self::GitTreeOid(value.clone()),
            SkillRevision::CliContentHash(value) => Self::CliContentHash(value.clone()),
        }
    }
}

impl From<PersistedSkillRevision> for SkillRevision {
    fn from(value: PersistedSkillRevision) -> Self {
        match value {
            PersistedSkillRevision::GitTreeOid(value) => Self::GitTreeOid(value),
            PersistedSkillRevision::CliContentHash(value) => Self::CliContentHash(value),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PersistedRemoteSnapshotId {
    requested_ref: PersistedNormalizedRef,
    resolved_ref: String,
    commit_revision: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PersistedRemoteEvidenceEntry {
    checked_at_epoch_ms: u64,
    expires_at_epoch_ms: u64,
    snapshot_id: PersistedRemoteSnapshotId,
    provider_validation: Option<String>,
    complete_skill_path_catalog: BTreeSet<String>,
    skill_revisions: BTreeMap<String, PersistedSkillRevision>,
}

impl From<&RemoteEvidenceEntry> for PersistedRemoteEvidenceEntry {
    fn from(value: &RemoteEvidenceEntry) -> Self {
        Self {
            checked_at_epoch_ms: value.checked_at_epoch_ms,
            expires_at_epoch_ms: value.expires_at_epoch_ms,
            snapshot_id: PersistedRemoteSnapshotId {
                requested_ref: (&value.snapshot_id.requested_ref).into(),
                resolved_ref: value.snapshot_id.resolved_ref.clone(),
                commit_revision: value.snapshot_id.commit_revision.clone(),
            },
            provider_validation: value.provider_validation.clone(),
            complete_skill_path_catalog: value.complete_skill_path_catalog.clone(),
            skill_revisions: value
                .skill_revisions
                .iter()
                .map(|(path, revision)| (path.clone(), revision.into()))
                .collect(),
        }
    }
}

impl From<PersistedRemoteEvidenceEntry> for RemoteEvidenceEntry {
    fn from(value: PersistedRemoteEvidenceEntry) -> Self {
        Self {
            checked_at_epoch_ms: value.checked_at_epoch_ms,
            expires_at_epoch_ms: value.expires_at_epoch_ms,
            snapshot_id: RemoteSnapshotId {
                requested_ref: value.snapshot_id.requested_ref.into(),
                resolved_ref: value.snapshot_id.resolved_ref,
                commit_revision: value.snapshot_id.commit_revision,
            },
            provider_validation: value.provider_validation,
            complete_skill_path_catalog: value.complete_skill_path_catalog,
            skill_revisions: value
                .skill_revisions
                .into_iter()
                .map(|(path, revision)| (path, revision.into()))
                .collect(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PersistedEvidenceAttempt {
    checked_at_epoch_ms: u64,
    failure: Option<PersistedEvidenceFailure>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PersistedEvidenceFailure {
    reason: EvidenceFailureReason,
    retry_at_epoch_ms: Option<u64>,
    provider_cooldown: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PersistedCoordinatorState {
    schema_version: u32,
    updated_at_epoch_ms: u64,
    evidence: Vec<PersistedMapEntry<PersistedRemoteEvidenceKey, PersistedRemoteEvidenceEntry>>,
    last_referenced: Vec<PersistedMapEntry<PersistedRemoteEvidenceKey, u64>>,
    attempts: Vec<PersistedMapEntry<PersistedEnvironmentEvidenceKey, PersistedEvidenceAttempt>>,
    network_backoff: Vec<PersistedMapEntry<PersistedEnvironmentEvidenceKey, u64>>,
    network_failure_counts: Vec<PersistedMapEntry<PersistedEnvironmentEvidenceKey, u32>>,
    provider_cooldowns: Vec<PersistedMapEntry<PersistedEnvironmentThrottleKey, u64>>,
}

impl PersistedCoordinatorState {
    fn from_coordinator(state: &CoordinatorState, updated_at_epoch_ms: u64) -> Self {
        Self {
            schema_version: PERSISTED_STATE_SCHEMA_VERSION,
            updated_at_epoch_ms,
            evidence: sorted_persisted_entries(state.evidence.iter().map(|(key, value)| {
                PersistedMapEntry {
                    key: key.into(),
                    value: value.into(),
                }
            })),
            last_referenced: sorted_persisted_entries(state.last_referenced.iter().map(
                |(key, value)| PersistedMapEntry {
                    key: key.into(),
                    value: *value,
                },
            )),
            attempts: sorted_attempts(&state.attempts),
            network_backoff: sorted_persisted_entries(state.network_backoff.iter().map(
                |(key, value)| PersistedMapEntry {
                    key: key.into(),
                    value: *value,
                },
            )),
            network_failure_counts: sorted_persisted_entries(
                state
                    .network_failure_counts
                    .iter()
                    .map(|(key, value)| PersistedMapEntry {
                        key: key.into(),
                        value: *value,
                    }),
            ),
            provider_cooldowns: sorted_persisted_entries(state.provider_cooldowns.iter().map(
                |(key, value)| PersistedMapEntry {
                    key: key.into(),
                    value: *value,
                },
            )),
        }
    }

    fn into_coordinator(self) -> Result<CoordinatorState, AppError> {
        if self.schema_version != PERSISTED_STATE_SCHEMA_VERSION {
            return Err(invalid_persisted_state("unsupported schema version"));
        }
        let mut state = CoordinatorState::default();
        for entry in self.evidence {
            insert_unique(
                &mut state.evidence,
                entry.key.into_runtime()?,
                entry.value.into(),
                "evidence",
            )?;
        }
        for entry in self.last_referenced {
            insert_unique(
                &mut state.last_referenced,
                entry.key.into_runtime()?,
                entry.value,
                "lastReferenced",
            )?;
        }
        for entry in self.attempts {
            let failure = entry.value.failure.map(|failure| EvidenceDetectionFailure {
                reason: failure.reason,
                message: persisted_failure_message(failure.reason).to_string(),
                retry_at_epoch_ms: failure.retry_at_epoch_ms,
                provider_cooldown: failure.provider_cooldown,
            });
            insert_unique(
                &mut state.attempts,
                entry.key.into_runtime()?,
                EvidenceAttempt {
                    checked_at_epoch_ms: entry.value.checked_at_epoch_ms,
                    failure,
                },
                "attempts",
            )?;
        }
        for entry in self.network_backoff {
            insert_unique(
                &mut state.network_backoff,
                entry.key.into_runtime()?,
                entry.value,
                "networkBackoff",
            )?;
        }
        for entry in self.network_failure_counts {
            insert_unique(
                &mut state.network_failure_counts,
                entry.key.into_runtime()?,
                entry.value,
                "networkFailureCounts",
            )?;
        }
        for entry in self.provider_cooldowns {
            insert_unique(
                &mut state.provider_cooldowns,
                entry.key.into_runtime()?,
                entry.value,
                "providerCooldowns",
            )?;
        }
        Ok(state)
    }
}

fn sorted_persisted_entries<K, V>(
    entries: impl IntoIterator<Item = PersistedMapEntry<K, V>>,
) -> Vec<PersistedMapEntry<K, V>>
where
    K: Ord,
{
    let mut entries = entries.into_iter().collect::<Vec<_>>();
    entries.sort_by(|left, right| left.key.cmp(&right.key));
    entries
}

fn sorted_attempts(
    values: &HashMap<EnvironmentEvidenceKey, EvidenceAttempt>,
) -> Vec<PersistedMapEntry<PersistedEnvironmentEvidenceKey, PersistedEvidenceAttempt>> {
    sorted_persisted_entries(values.iter().map(|(key, value)| {
        PersistedMapEntry {
            key: key.into(),
            value: PersistedEvidenceAttempt {
                checked_at_epoch_ms: value.checked_at_epoch_ms,
                failure: value
                    .failure
                    .as_ref()
                    .map(|failure| PersistedEvidenceFailure {
                        reason: failure.reason,
                        retry_at_epoch_ms: failure.retry_at_epoch_ms,
                        provider_cooldown: failure.provider_cooldown,
                    }),
            },
        }
    }))
}

fn insert_unique<K, V>(
    values: &mut HashMap<K, V>,
    key: K,
    value: V,
    field: &str,
) -> Result<(), AppError>
where
    K: Eq + std::hash::Hash,
{
    if values.insert(key, value).is_some() {
        return Err(invalid_persisted_state(&format!(
            "duplicate key in {field}"
        )));
    }
    Ok(())
}

fn persisted_failure_message(reason: EvidenceFailureReason) -> &'static str {
    match reason {
        EvidenceFailureReason::RateLimited => "The provider rate limit is still active.",
        EvidenceFailureReason::AuthenticationRequired => {
            "Authentication is required to check this source."
        }
        EvidenceFailureReason::RefNotFound => "The configured ref was not found.",
        EvidenceFailureReason::RepositoryNotFound => "The repository was not found.",
        EvidenceFailureReason::NotFoundOrUnauthorized => {
            "The repository was not found or is not accessible."
        }
        EvidenceFailureReason::Network => "The previous network check did not complete.",
        EvidenceFailureReason::IncompleteEvidence => {
            "The provider returned incomplete source evidence."
        }
        EvidenceFailureReason::SourceUnavailable => {
            "The installation source was unavailable during the previous check."
        }
    }
}

fn invalid_persisted_state(message: &str) -> AppError {
    AppError::Json {
        message: format!("invalid update-check state: {message}"),
    }
}

#[derive(Clone)]
struct InFlightDetection {
    batch_id: u64,
    receiver: watch::Receiver<Option<DetectionCompletion>>,
    collecting_skill_paths: BTreeSet<String>,
    running_skill_paths: Option<BTreeSet<String>>,
    pending_skill_paths: BTreeSet<String>,
    acquisition: Arc<AcquisitionDescriptor>,
    acquisition_transport_identity: AcquisitionTransportIdentity,
}

impl SourceEvidenceCoordinator {
    #[cfg(test)]
    pub fn with_snapshot_reuse(
        detector: Arc<dyn SourceEvidenceDetector>,
        snapshots: Arc<SourceSnapshotReuseIndex>,
    ) -> Self {
        Self::build(detector, Some(snapshots), || {
            chrono::Utc::now().timestamp_millis().max(0) as u64
        })
    }

    pub fn with_snapshot_reuse_and_state_path(
        detector: Arc<dyn SourceEvidenceDetector>,
        snapshots: Arc<SourceSnapshotReuseIndex>,
        state_path: PathBuf,
    ) -> Result<Self, AppError> {
        Self::build_with_state_path(detector, Some(snapshots), state_path, || {
            chrono::Utc::now().timestamp_millis().max(0) as u64
        })
    }

    #[cfg(test)]
    pub fn with_clock(
        detector: Arc<dyn SourceEvidenceDetector>,
        now: impl Fn() -> u64 + Send + Sync + 'static,
    ) -> Self {
        Self::build(detector, None, now)
    }

    #[cfg(test)]
    fn with_clock_and_snapshots(
        detector: Arc<dyn SourceEvidenceDetector>,
        snapshots: Arc<SourceSnapshotReuseIndex>,
        now: impl Fn() -> u64 + Send + Sync + 'static,
    ) -> Self {
        Self::build(detector, Some(snapshots), now)
    }

    #[cfg(test)]
    fn with_clock_and_state_path(
        detector: Arc<dyn SourceEvidenceDetector>,
        state_path: PathBuf,
        now: impl Fn() -> u64 + Send + Sync + 'static,
    ) -> Result<Self, AppError> {
        Self::build_with_state_path(detector, None, state_path, now)
    }

    #[cfg(test)]
    fn build(
        detector: Arc<dyn SourceEvidenceDetector>,
        snapshots: Option<Arc<SourceSnapshotReuseIndex>>,
        now: impl Fn() -> u64 + Send + Sync + 'static,
    ) -> Self {
        Self {
            inner: Arc::new(SourceEvidenceCoordinatorInner {
                detector,
                detector_permits: Arc::new(Semaphore::new(DETECTOR_CONCURRENCY_LIMIT)),
                snapshots,
                now: Arc::new(now),
                state: Mutex::new(CoordinatorState::default()),
                state_file: None,
                persistence_lock: Mutex::new(()),
            }),
        }
    }

    fn build_with_state_path(
        detector: Arc<dyn SourceEvidenceDetector>,
        snapshots: Option<Arc<SourceSnapshotReuseIndex>>,
        state_path: PathBuf,
        now: impl Fn() -> u64 + Send + Sync + 'static,
    ) -> Result<Self, AppError> {
        let now: Arc<dyn Fn() -> u64 + Send + Sync> = Arc::new(now);
        let state_file = SourceEvidenceStateFile::new(state_path);
        let (state, loaded) = load_persisted_state(&state_file, now())?;
        let coordinator = Self {
            inner: Arc::new(SourceEvidenceCoordinatorInner {
                detector,
                detector_permits: Arc::new(Semaphore::new(DETECTOR_CONCURRENCY_LIMIT)),
                snapshots,
                now,
                state: Mutex::new(state),
                state_file: Some(state_file),
                persistence_lock: Mutex::new(()),
            }),
        };
        if loaded {
            coordinator.persist_current_state()?;
        }
        Ok(coordinator)
    }

    #[cfg(test)]
    pub fn record_provider_cooldown(
        &self,
        environment: &EnvironmentRef,
        key: ProviderThrottleKey,
        deadline_epoch_ms: u64,
    ) {
        if let Ok(mut state) = self.inner.state.lock() {
            state
                .provider_cooldowns
                .entry(EnvironmentThrottleKey::new(environment, &key))
                .and_modify(|deadline| *deadline = (*deadline).max(deadline_epoch_ms))
                .or_insert(deadline_epoch_ms);
        }
    }

    /// 凭据成功变更后，只解除 Native GitHub 的认证类失败和服务商限流冷却期。
    /// 成功的远端证据、其他 Environment 以及非认证失败保持不变。
    pub fn clear_native_github_auth_suppression(&self) -> Result<(), AppError> {
        let native = EnvironmentKey::from_ref(&EnvironmentRef::Native);
        update_persisted_state(&self.inner, |state| {
            state.attempts.retain(|key, attempt| {
                if key.environment != native
                    || key.evidence.remote.provider() != &SourceProvider::Github
                {
                    return true;
                }
                !attempt.failure.as_ref().is_some_and(|failure| {
                    is_user_action_required(failure.reason)
                        && matches!(
                            failure.reason,
                            EvidenceFailureReason::AuthenticationRequired
                                | EvidenceFailureReason::NotFoundOrUnauthorized
                        )
                })
            });
            state.provider_cooldowns.retain(|key, _| {
                key.environment != native || key.throttle.provider != SourceProvider::Github
            });
        })
    }

    /// 来源修复成功后只解除目标来源的失败抑制，不影响其他来源或远端证据。
    pub fn clear_source_suppression(
        &self,
        environment: &EnvironmentRef,
        key: &RemoteEvidenceKey,
    ) -> Result<(), AppError> {
        let operation_key = EnvironmentEvidenceKey::new(environment, key);
        update_persisted_state(&self.inner, |state| {
            state.attempts.remove(&operation_key);
            state.network_backoff.remove(&operation_key);
            state.network_failure_counts.remove(&operation_key);
        })
    }

    pub fn record_acquisition(
        &self,
        key: RemoteEvidenceKey,
        acquisition_key: PayloadAcquisitionKey,
        facts: SourceSnapshotFacts,
        skill_revisions: BTreeMap<String, SkillRevision>,
    ) -> Result<(), AppError> {
        if key.normalized_ref != facts.snapshot_id.requested_ref
            || acquisition_key.normalized_ref != facts.snapshot_id.requested_ref
            || skill_revisions
                .keys()
                .any(|path| !facts.complete_skill_path_catalog.contains(path))
        {
            return Err(AppError::StalePayload);
        }
        let checked_at = (self.inner.now)();
        let mut state = state(&self.inner)?;
        let operation_key = EnvironmentEvidenceKey::from_environment_key(
            acquisition_key.environment.clone(),
            key.clone(),
        );
        let skill_revisions = match state.evidence.get(&key) {
            Some(previous) if previous.snapshot_id == facts.snapshot_id => {
                let mut merged = previous.skill_revisions.clone();
                merged.retain(|path, _| facts.complete_skill_path_catalog.contains(path));
                merged.extend(skill_revisions);
                merged
            }
            _ => skill_revisions,
        };
        state.evidence.insert(
            key.clone(),
            RemoteEvidenceEntry {
                checked_at_epoch_ms: checked_at,
                expires_at_epoch_ms: checked_at.saturating_add(EVIDENCE_TTL_MS),
                snapshot_id: facts.snapshot_id.clone(),
                provider_validation: None,
                complete_skill_path_catalog: facts.complete_skill_path_catalog.clone(),
                skill_revisions,
            },
        );
        state.last_referenced.insert(key.clone(), checked_at);
        state.attempts.insert(
            operation_key.clone(),
            EvidenceAttempt {
                checked_at_epoch_ms: checked_at,
                failure: None,
            },
        );
        state.network_backoff.remove(&operation_key);
        state.network_failure_counts.remove(&operation_key);
        drop(state);
        self.persist_current_state()?;
        if let Some(snapshots) = &self.inner.snapshots {
            snapshots.remember(
                acquisition_key,
                facts.snapshot_id.commit_revision,
                facts.discovery_session,
            );
        }
        Ok(())
    }

    fn persist_current_state(&self) -> Result<(), AppError> {
        persist_current_state(self.inner.as_ref())
    }

    pub async fn check(
        &self,
        request: EvidenceCheckRequest,
        cancellation: CancellationSignal,
    ) -> Result<EvidenceCheckResult, AppError> {
        let evidence_key = request.key.clone();
        let operation_key = EnvironmentEvidenceKey::new(&request.environment, &request.key);
        let throttle_key = EnvironmentThrottleKey::new(&request.environment, &request.throttle_key);
        let requested_skill_paths = request.requested_skill_paths.clone();
        loop {
            let now = (self.inner.now)();
            let mut spawned = None;
            let mut receiver = {
                let mut state = state(&self.inner)?;
                state
                    .provider_cooldowns
                    .retain(|_, deadline| *deadline > now);
                state.network_backoff.retain(|_, deadline| *deadline > now);

                if let Some(deadline) = state.provider_cooldowns.get(&throttle_key).copied() {
                    return Ok(result_from_provider_cooldown(
                        &state,
                        &request.key,
                        &operation_key,
                        deadline,
                        now,
                    ));
                }

                if request.mode == EvidenceCheckMode::Automatic {
                    if state
                        .attempts
                        .get(&operation_key)
                        .and_then(|attempt| attempt.failure.as_ref())
                        .is_some_and(|failure| is_user_action_required(failure.reason))
                    {
                        return Ok(result_from_state(
                            &state,
                            &request.key,
                            &operation_key,
                            EvidenceFreshness::Unavailable,
                            now,
                        ));
                    }
                    if let Some(evidence) = state.evidence.get(&request.key) {
                        if evidence.expires_at_epoch_ms >= now
                            && evidence_covers_requested_paths(evidence, &requested_skill_paths)
                        {
                            return Ok(result_from_state(
                                &state,
                                &request.key,
                                &operation_key,
                                EvidenceFreshness::Cached,
                                now,
                            ));
                        }
                    }
                }
                if request.mode == EvidenceCheckMode::Automatic
                    && state.network_backoff.contains_key(&operation_key)
                {
                    return Ok(result_from_state(
                        &state,
                        &request.key,
                        &operation_key,
                        EvidenceFreshness::BackingOff,
                        now,
                    ));
                }

                let completed = state
                    .in_flight
                    .get(&operation_key)
                    .is_some_and(|batch| batch.receiver.borrow().is_some());
                if completed {
                    let previous = state
                        .in_flight
                        .remove(&operation_key)
                        .expect("completed batch must still be registered");
                    let mut paths = previous.pending_skill_paths;
                    if let Some(evidence) = state.evidence.get(&request.key) {
                        paths.retain(|path| !evidence_covers_path(evidence, path));
                    }
                    paths.extend(requested_skill_paths.clone());
                    let (batch_id, sender, receiver) =
                        register_detection_batch(&mut state, &operation_key, &request, paths);
                    spawned = Some((batch_id, sender));
                    receiver
                } else if let Some(in_flight) = state.in_flight.get_mut(&operation_key) {
                    let current_paths = in_flight
                        .running_skill_paths
                        .as_ref()
                        .unwrap_or(&in_flight.collecting_skill_paths);
                    if current_paths.is_superset(&requested_skill_paths) {
                        // The current immutable batch already covers this waiter.
                    } else if in_flight.running_skill_paths.is_none()
                        && in_flight
                            .acquisition
                            .acquisition_equivalent(request.acquisition.as_ref())
                        && in_flight.acquisition_transport_identity
                            == request.acquisition_transport_identity
                    {
                        in_flight
                            .collecting_skill_paths
                            .extend(requested_skill_paths.clone());
                    } else {
                        in_flight
                            .pending_skill_paths
                            .extend(requested_skill_paths.clone());
                    }
                    in_flight.receiver.clone()
                } else {
                    let (batch_id, sender, receiver) = register_detection_batch(
                        &mut state,
                        &operation_key,
                        &request,
                        requested_skill_paths.clone(),
                    );
                    spawned = Some((batch_id, sender));
                    receiver
                }
            };

            if let Some((batch_id, sender)) = spawned {
                let inner = self.inner.clone();
                let detection_request = request.clone();
                let detection_operation_key = operation_key.clone();
                tokio::spawn(async move {
                    run_detection(
                        inner,
                        detection_request,
                        detection_operation_key,
                        batch_id,
                        sender,
                    )
                    .await;
                });
            }

            loop {
                if let Some(completion) = receiver.borrow().clone() {
                    completion?;
                    let now = (self.inner.now)();
                    let state = state(&self.inner)?;
                    let freshness =
                        freshness_after_attempt(&state, &evidence_key, &operation_key, now);
                    let result =
                        result_from_state(&state, &evidence_key, &operation_key, freshness, now);
                    if result.evidence.as_ref().is_some_and(|evidence| {
                        evidence_covers_requested_paths(evidence, &requested_skill_paths)
                    }) || result
                        .last_attempt
                        .as_ref()
                        .is_some_and(|attempt| attempt.failure.is_some())
                    {
                        return Ok(result);
                    }
                    break;
                }
                tokio::select! {
                    changed = receiver.changed() => {
                        if changed.is_err() && receiver.borrow().is_none() {
                            return Err(AppError::ExecutionFailed {
                                message: "source evidence detector ended without a result".to_string(),
                            });
                        }
                    }
                    () = cancellation.cancelled() => return Err(AppError::MutationCancelled),
                }
            }
        }
    }
}

fn register_detection_batch(
    state: &mut CoordinatorState,
    operation_key: &EnvironmentEvidenceKey,
    request: &EvidenceCheckRequest,
    requested_skill_paths: BTreeSet<String>,
) -> (
    u64,
    watch::Sender<Option<DetectionCompletion>>,
    watch::Receiver<Option<DetectionCompletion>>,
) {
    let batch_id = state.next_batch_id;
    state.next_batch_id = state.next_batch_id.wrapping_add(1);
    let (sender, receiver) = watch::channel(None);
    state.in_flight.insert(
        operation_key.clone(),
        InFlightDetection {
            batch_id,
            receiver: receiver.clone(),
            collecting_skill_paths: requested_skill_paths,
            running_skill_paths: None,
            pending_skill_paths: BTreeSet::new(),
            acquisition: request.acquisition.clone(),
            acquisition_transport_identity: request.acquisition_transport_identity.clone(),
        },
    );
    (batch_id, sender, receiver)
}

fn evidence_covers_requested_paths(
    evidence: &RemoteEvidenceEntry,
    requested_skill_paths: &BTreeSet<String>,
) -> bool {
    requested_skill_paths
        .iter()
        .all(|path| evidence_covers_path(evidence, path))
}

fn evidence_covers_path(evidence: &RemoteEvidenceEntry, path: &str) -> bool {
    !evidence.complete_skill_path_catalog.contains(path)
        || evidence.skill_revisions.contains_key(path)
}

async fn run_detection(
    inner: Arc<SourceEvidenceCoordinatorInner>,
    request: EvidenceCheckRequest,
    operation_key: EnvironmentEvidenceKey,
    batch_id: u64,
    sender: watch::Sender<Option<DetectionCompletion>>,
) {
    let key = request.key.clone();
    let throttle_key = EnvironmentThrottleKey::new(&request.environment, &request.throttle_key);
    let acquisition_transport_identity = request.acquisition_transport_identity.clone();
    let completion = match inner.detector_permits.clone().acquire_owned().await {
        Ok(_permit) => match seal_detection_batch(&inner, &key, &operation_key, batch_id) {
            Ok(Some((requested_skill_paths, previous))) => {
                let outcome =
                    if let Some(deadline) = provider_cooldown_deadline(&inner, &throttle_key) {
                        Ok(EvidenceDetectionOutcome::Failed(EvidenceDetectionFailure {
                            reason: EvidenceFailureReason::RateLimited,
                            message: "The provider rate limit is still active.".to_string(),
                            retry_at_epoch_ms: Some(deadline),
                            provider_cooldown: true,
                        }))
                    } else {
                        inner
                            .detector
                            .detect(
                                EvidenceDetectionRequest {
                                    environment: request.environment,
                                    key: key.clone(),
                                    requested_skill_paths,
                                    acquisition: request.acquisition,
                                    acquisition_transport_identity: request
                                        .acquisition_transport_identity,
                                },
                                previous,
                                CancellationSignal::default(),
                            )
                            .await
                    };
                finish_detection(
                    &inner,
                    &throttle_key,
                    &key,
                    &operation_key,
                    &acquisition_transport_identity,
                    outcome,
                )
            }
            Ok(None) => return,
            Err(error) => Err(error),
        },
        Err(_) => Err(AppError::ExecutionFailed {
            message: "source evidence detector gate closed".to_string(),
        }),
    };
    let _ = sender.send(Some(completion));
}

fn provider_cooldown_deadline(
    inner: &SourceEvidenceCoordinatorInner,
    throttle_key: &EnvironmentThrottleKey,
) -> Option<u64> {
    let now = (inner.now)();
    state(inner).ok().and_then(|mut state| {
        state
            .provider_cooldowns
            .retain(|_, deadline| *deadline > now);
        state.provider_cooldowns.get(throttle_key).copied()
    })
}

fn seal_detection_batch(
    inner: &SourceEvidenceCoordinatorInner,
    evidence_key: &RemoteEvidenceKey,
    operation_key: &EnvironmentEvidenceKey,
    batch_id: u64,
) -> Result<Option<SealedDetectionBatch>, AppError> {
    let mut state = state(inner)?;
    let Some(batch) = state.in_flight.get_mut(operation_key) else {
        return Ok(None);
    };
    if batch.batch_id != batch_id || batch.running_skill_paths.is_some() {
        return Ok(None);
    }
    let paths = std::mem::take(&mut batch.collecting_skill_paths);
    batch.running_skill_paths = Some(paths.clone());
    let previous = state.evidence.get(evidence_key).cloned();
    Ok(Some((paths, previous)))
}

fn finish_detection(
    inner: &SourceEvidenceCoordinatorInner,
    throttle_key: &EnvironmentThrottleKey,
    key: &RemoteEvidenceKey,
    operation_key: &EnvironmentEvidenceKey,
    acquisition_transport_identity: &AcquisitionTransportIdentity,
    outcome: Result<EvidenceDetectionOutcome, AppError>,
) -> DetectionCompletion {
    let checked_at = (inner.now)();
    let result = (|| {
        let mut state = state(inner)?;
        state.last_referenced.insert(key.clone(), checked_at);
        match outcome {
            Ok(EvidenceDetectionOutcome::Modified(observation)) => {
                if observation.snapshot_id.requested_ref != key.normalized_ref {
                    record_failure(
                        &mut state,
                        throttle_key,
                        operation_key,
                        checked_at,
                        EvidenceDetectionFailure::incomplete(
                            "provider evidence does not match the requested ref",
                        ),
                    );
                    discard_pending_paths(&mut state, operation_key);
                    return Ok(());
                }
                if let Some(snapshot) = &observation.snapshot_facts {
                    let matches_observation = snapshot.snapshot_id == observation.snapshot_id
                        && snapshot.complete_skill_path_catalog
                            == observation.complete_skill_path_catalog;
                    if !matches_observation {
                        record_failure(
                            &mut state,
                            throttle_key,
                            operation_key,
                            checked_at,
                            EvidenceDetectionFailure::incomplete(
                                "snapshot facts do not match provider evidence",
                            ),
                        );
                        discard_pending_paths(&mut state, operation_key);
                        return Ok(());
                    }
                }
                let skill_revisions = match state.evidence.get(key) {
                    Some(previous) if previous.snapshot_id == observation.snapshot_id => {
                        let mut merged = previous.skill_revisions.clone();
                        merged.retain(|path, _| {
                            observation.complete_skill_path_catalog.contains(path)
                        });
                        merged.extend(observation.skill_revisions);
                        merged
                    }
                    _ => observation.skill_revisions,
                };
                state.evidence.insert(
                    key.clone(),
                    RemoteEvidenceEntry {
                        checked_at_epoch_ms: checked_at,
                        expires_at_epoch_ms: checked_at.saturating_add(EVIDENCE_TTL_MS),
                        snapshot_id: observation.snapshot_id,
                        provider_validation: observation.provider_validation,
                        complete_skill_path_catalog: observation.complete_skill_path_catalog,
                        skill_revisions,
                    },
                );
                state.network_backoff.remove(operation_key);
                state.network_failure_counts.remove(operation_key);
                state.attempts.insert(
                    operation_key.clone(),
                    EvidenceAttempt {
                        checked_at_epoch_ms: checked_at,
                        failure: None,
                    },
                );
                if let (Some(snapshots), Some(snapshot)) =
                    (&inner.snapshots, observation.snapshot_facts)
                {
                    snapshots.remember(
                        PayloadAcquisitionKey::new(
                            acquisition_transport_identity.clone(),
                            snapshot.snapshot_id.requested_ref,
                            &snapshot.discovery_session.environment,
                        ),
                        snapshot.snapshot_id.commit_revision,
                        snapshot.discovery_session,
                    );
                }
            }
            Ok(EvidenceDetectionOutcome::NotModified) => {
                if let Some(evidence) = state.evidence.get_mut(key) {
                    evidence.checked_at_epoch_ms = checked_at;
                    evidence.expires_at_epoch_ms = checked_at.saturating_add(EVIDENCE_TTL_MS);
                    state.network_backoff.remove(operation_key);
                    state.network_failure_counts.remove(operation_key);
                    state.attempts.insert(
                        operation_key.clone(),
                        EvidenceAttempt {
                            checked_at_epoch_ms: checked_at,
                            failure: None,
                        },
                    );
                } else {
                    record_failure(
                        &mut state,
                        throttle_key,
                        operation_key,
                        checked_at,
                        EvidenceDetectionFailure::incomplete(
                            "provider returned not-modified without cached evidence",
                        ),
                    );
                    discard_pending_paths(&mut state, operation_key);
                }
            }
            Ok(EvidenceDetectionOutcome::Failed(failure)) => {
                record_failure(&mut state, throttle_key, operation_key, checked_at, failure);
                discard_pending_paths(&mut state, operation_key);
            }
            Err(error) => {
                record_failure(
                    &mut state,
                    throttle_key,
                    operation_key,
                    checked_at,
                    EvidenceDetectionFailure {
                        reason: EvidenceFailureReason::SourceUnavailable,
                        message: error.to_string(),
                        retry_at_epoch_ms: None,
                        provider_cooldown: false,
                    },
                );
                discard_pending_paths(&mut state, operation_key);
            }
        }
        Ok(())
    })();
    if result.is_ok() {
        persist_current_state(inner)?;
    }
    result
}

fn discard_pending_paths(state: &mut CoordinatorState, key: &EnvironmentEvidenceKey) {
    if let Some(batch) = state.in_flight.get_mut(key) {
        batch.pending_skill_paths.clear();
    }
}

fn record_failure(
    state: &mut CoordinatorState,
    throttle_key: &EnvironmentThrottleKey,
    key: &EnvironmentEvidenceKey,
    checked_at: u64,
    mut failure: EvidenceDetectionFailure,
) {
    if is_transient_failure(failure.reason) {
        let failures = state
            .network_failure_counts
            .entry(key.clone())
            .and_modify(|failures| *failures = failures.saturating_add(1))
            .or_insert(1);
        let index = (failures.saturating_sub(1) as usize)
            .min(TRANSIENT_BACKOFF_DELAYS_MS.len().saturating_sub(1));
        let delay = TRANSIENT_BACKOFF_DELAYS_MS[index];
        let deadline = checked_at.saturating_add(delay);
        failure.retry_at_epoch_ms = Some(deadline);
        state.network_backoff.insert(key.clone(), deadline);
    } else if failure.provider_cooldown {
        let deadline = failure
            .retry_at_epoch_ms
            .unwrap_or_else(|| checked_at.saturating_add(PROVIDER_COOLDOWN_FALLBACK_MS));
        failure.retry_at_epoch_ms = Some(deadline);
        state
            .provider_cooldowns
            .entry(throttle_key.clone())
            .and_modify(|existing| *existing = (*existing).max(deadline))
            .or_insert(deadline);
    }
    state.attempts.insert(
        key.clone(),
        EvidenceAttempt {
            checked_at_epoch_ms: checked_at,
            failure: Some(failure),
        },
    );
}

fn result_from_state(
    state: &CoordinatorState,
    evidence_key: &RemoteEvidenceKey,
    operation_key: &EnvironmentEvidenceKey,
    requested_freshness: EvidenceFreshness,
    now: u64,
) -> EvidenceCheckResult {
    let evidence = state.evidence.get(evidence_key).cloned();
    let freshness = match requested_freshness {
        EvidenceFreshness::Fresh | EvidenceFreshness::Unavailable => match &evidence {
            Some(entry) if entry.expires_at_epoch_ms >= now => requested_freshness,
            Some(_) => EvidenceFreshness::Stale,
            None => EvidenceFreshness::Unavailable,
        },
        other => other,
    };
    EvidenceCheckResult {
        evidence_is_fresh: evidence
            .as_ref()
            .is_some_and(|entry| entry.expires_at_epoch_ms >= now),
        evidence,
        freshness,
        last_attempt: state.attempts.get(operation_key).cloned(),
    }
}

fn result_from_provider_cooldown(
    state: &CoordinatorState,
    evidence_key: &RemoteEvidenceKey,
    operation_key: &EnvironmentEvidenceKey,
    deadline: u64,
    now: u64,
) -> EvidenceCheckResult {
    let mut result = result_from_state(
        state,
        evidence_key,
        operation_key,
        EvidenceFreshness::CoolingDown,
        now,
    );
    let needs_cooldown_attempt = result.last_attempt.as_ref().is_none_or(|attempt| {
        attempt.failure.as_ref().is_none_or(|failure| {
            !failure.provider_cooldown
                || failure
                    .retry_at_epoch_ms
                    .is_none_or(|retry_at| retry_at < deadline)
        })
    });
    if needs_cooldown_attempt {
        result.last_attempt = Some(EvidenceAttempt {
            checked_at_epoch_ms: now,
            failure: Some(EvidenceDetectionFailure {
                reason: EvidenceFailureReason::RateLimited,
                message: "The provider rate limit is still active.".to_string(),
                retry_at_epoch_ms: Some(deadline),
                provider_cooldown: true,
            }),
        });
    }
    result
}

fn freshness_after_attempt(
    state: &CoordinatorState,
    evidence_key: &RemoteEvidenceKey,
    operation_key: &EnvironmentEvidenceKey,
    now: u64,
) -> EvidenceFreshness {
    match state.attempts.get(operation_key) {
        Some(attempt) if attempt.failure.is_none() => EvidenceFreshness::Fresh,
        _ => match state.evidence.get(evidence_key) {
            Some(evidence) if evidence.expires_at_epoch_ms >= now => EvidenceFreshness::Cached,
            Some(_) => EvidenceFreshness::Stale,
            None => EvidenceFreshness::Unavailable,
        },
    }
}

fn is_user_action_required(reason: EvidenceFailureReason) -> bool {
    matches!(
        reason,
        EvidenceFailureReason::AuthenticationRequired
            | EvidenceFailureReason::RefNotFound
            | EvidenceFailureReason::RepositoryNotFound
            | EvidenceFailureReason::NotFoundOrUnauthorized
    )
}

fn is_transient_failure(reason: EvidenceFailureReason) -> bool {
    matches!(
        reason,
        EvidenceFailureReason::Network
            | EvidenceFailureReason::IncompleteEvidence
            | EvidenceFailureReason::SourceUnavailable
    )
}

fn load_persisted_state(
    state_file: &SourceEvidenceStateFile,
    now: u64,
) -> Result<(CoordinatorState, bool), AppError> {
    let Some(bytes) = state_file.read_optional()? else {
        return Ok((CoordinatorState::default(), false));
    };
    let loaded = serde_json::from_slice::<PersistedCoordinatorState>(&bytes)
        .map_err(|error| invalid_persisted_state(&error.to_string()))
        .and_then(PersistedCoordinatorState::into_coordinator);
    match loaded {
        Ok(mut state) => {
            prune_persisted_state(&mut state, now);
            Ok((state, true))
        }
        Err(error) => {
            state_file.quarantine(now)?;
            log::warn!("update-check state was quarantined: {error}");
            Ok((CoordinatorState::default(), false))
        }
    }
}

fn persist_current_state(inner: &SourceEvidenceCoordinatorInner) -> Result<(), AppError> {
    let Some(state_file) = &inner.state_file else {
        return Ok(());
    };
    let _persistence_guard = inner
        .persistence_lock
        .lock()
        .map_err(|_| coordinator_unavailable())?;
    let now = (inner.now)();
    let persisted = {
        let mut state = state(inner)?;
        prune_persisted_state(&mut state, now);
        PersistedCoordinatorState::from_coordinator(&state, now)
    };
    state_file.write_atomic(&serde_json::to_vec_pretty(&persisted)?)
}

fn update_persisted_state(
    inner: &SourceEvidenceCoordinatorInner,
    mutate: impl FnOnce(&mut CoordinatorState),
) -> Result<(), AppError> {
    let Some(state_file) = &inner.state_file else {
        let mut state = state(inner)?;
        mutate(&mut state);
        return Ok(());
    };
    let _persistence_guard = inner
        .persistence_lock
        .lock()
        .map_err(|_| coordinator_unavailable())?;
    let now = (inner.now)();
    let mut state = state(inner)?;
    let mut next = state.clone();
    mutate(&mut next);
    prune_persisted_state(&mut next, now);
    let persisted = PersistedCoordinatorState::from_coordinator(&next, now);
    state_file.write_atomic(&serde_json::to_vec_pretty(&persisted)?)?;
    *state = next;
    Ok(())
}

fn prune_persisted_state(state: &mut CoordinatorState, now: u64) {
    state.attempts.retain(|_, attempt| {
        attempt
            .failure
            .as_ref()
            .is_some_and(|failure| is_user_action_required(failure.reason))
            || attempt
                .checked_at_epoch_ms
                .saturating_add(DIAGNOSTIC_RETENTION_MS)
                >= now
    });
    state.network_backoff.retain(|_, deadline| *deadline > now);
    state
        .provider_cooldowns
        .retain(|_, deadline| *deadline > now);
    state
        .network_failure_counts
        .retain(|key, _| state.attempts.contains_key(key));

    state.evidence.retain(|key, evidence| {
        state
            .last_referenced
            .get(key)
            .copied()
            .unwrap_or(evidence.checked_at_epoch_ms)
            .saturating_add(SOURCE_RETENTION_MS)
            >= now
    });
    state
        .last_referenced
        .retain(|key, _| state.evidence.contains_key(key));
}

fn state(
    inner: &SourceEvidenceCoordinatorInner,
) -> Result<MutexGuard<'_, CoordinatorState>, AppError> {
    inner.state.lock().map_err(|_| coordinator_unavailable())
}

fn coordinator_unavailable() -> AppError {
    AppError::ExecutionFailed {
        message: "source evidence coordinator state is unavailable".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet, VecDeque};
    use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use super::*;
    use crate::core::mutation::CancellationSignal;
    use crate::core::parse_source;
    use crate::core::source_identity::{
        NormalizedRef, RemoteSourceIdentity, SourceIdentity, SourceProvider,
    };
    use crate::environment::types::EnvironmentRef;
    use tempfile::tempdir;

    struct ScriptedDetector {
        calls: AtomicUsize,
        active: AtomicUsize,
        peak: AtomicUsize,
        delay: Duration,
        outcomes: Mutex<VecDeque<Result<EvidenceDetectionOutcome, crate::error::AppError>>>,
    }

    impl ScriptedDetector {
        fn new(outcomes: impl IntoIterator<Item = EvidenceDetectionOutcome>) -> Self {
            Self {
                calls: AtomicUsize::new(0),
                active: AtomicUsize::new(0),
                peak: AtomicUsize::new(0),
                delay: Duration::ZERO,
                outcomes: Mutex::new(outcomes.into_iter().map(Ok).collect()),
            }
        }

        fn delayed(outcome: EvidenceDetectionOutcome, delay: Duration) -> Self {
            Self {
                delay,
                ..Self::new([outcome])
            }
        }

        fn calls(&self) -> usize {
            self.calls.load(Ordering::SeqCst)
        }

        fn peak(&self) -> usize {
            self.peak.load(Ordering::SeqCst)
        }
    }

    impl SourceEvidenceDetector for ScriptedDetector {
        fn detect<'a>(
            &'a self,
            _request: EvidenceDetectionRequest,
            _previous: Option<RemoteEvidenceEntry>,
            _cancellation: CancellationSignal,
        ) -> EvidenceFuture<'a> {
            Box::pin(async move {
                self.calls.fetch_add(1, Ordering::SeqCst);
                let active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
                self.peak.fetch_max(active, Ordering::SeqCst);
                if !self.delay.is_zero() {
                    tokio::time::sleep(self.delay).await;
                }
                self.active.fetch_sub(1, Ordering::SeqCst);
                self.outcomes
                    .lock()
                    .unwrap()
                    .pop_front()
                    .unwrap_or_else(|| {
                        Ok(modified(
                            "revision-1",
                            [(
                                "skills/alpha",
                                SkillRevision::GitTreeOid("tree-alpha".into()),
                            )],
                        ))
                    })
            })
        }
    }

    fn key(repository: &str) -> RemoteEvidenceKey {
        RemoteEvidenceKey::new(
            RemoteSourceIdentity::new(SourceProvider::Github, "github.com", repository),
            NormalizedRef::Named("main".to_string()),
        )
    }

    fn throttle_key() -> ProviderThrottleKey {
        ProviderThrottleKey::new(SourceProvider::Github, "github.com")
    }

    fn request(repository: &str, mode: EvidenceCheckMode) -> EvidenceCheckRequest {
        request_for_path(repository, mode, "skills/alpha")
    }

    fn request_for_path(
        repository: &str,
        mode: EvidenceCheckMode,
        skill_path: &str,
    ) -> EvidenceCheckRequest {
        request_for_path_in_environment(repository, mode, skill_path, EnvironmentRef::Native)
    }

    fn request_in_environment(
        repository: &str,
        mode: EvidenceCheckMode,
        environment: EnvironmentRef,
    ) -> EvidenceCheckRequest {
        request_for_path_in_environment(repository, mode, "skills/alpha", environment)
    }

    fn request_for_path_in_environment(
        repository: &str,
        mode: EvidenceCheckMode,
        skill_path: &str,
        environment: EnvironmentRef,
    ) -> EvidenceCheckRequest {
        EvidenceCheckRequest {
            environment,
            key: key(repository),
            throttle_key: throttle_key(),
            mode,
            requested_skill_paths: BTreeSet::from([skill_path.to_string()]),
            acquisition: Arc::new(
                SourceIdentity::from_parsed(
                    &parse_source(&format!("https://github.com/{repository}#main")).unwrap(),
                )
                .unwrap()
                .acquisition()
                .clone(),
            ),
            acquisition_transport_identity: SourceIdentity::from_parsed(
                &parse_source(&format!("https://github.com/{repository}#main")).unwrap(),
            )
            .unwrap()
            .acquisition_transport()
            .clone(),
        }
    }

    fn modified<const N: usize>(
        ref_revision: &str,
        revisions: [(&str, SkillRevision); N],
    ) -> EvidenceDetectionOutcome {
        EvidenceDetectionOutcome::Modified(RemoteEvidenceObservation {
            snapshot_id: RemoteSnapshotId::new(
                NormalizedRef::Named("main".to_string()),
                "refs/heads/main",
                ref_revision,
            ),
            provider_validation: None,
            complete_skill_path_catalog: BTreeSet::from([
                "skills/alpha".to_string(),
                "skills/beta".to_string(),
            ]),
            skill_revisions: revisions
                .into_iter()
                .map(|(path, revision)| (path.to_string(), revision))
                .collect::<BTreeMap<_, _>>(),
            snapshot_facts: None,
        })
    }

    fn coordinator(
        detector: Arc<dyn SourceEvidenceDetector>,
        now: Arc<AtomicU64>,
    ) -> SourceEvidenceCoordinator {
        SourceEvidenceCoordinator::with_clock(detector, move || now.load(Ordering::SeqCst))
    }

    fn persistent_coordinator(
        detector: Arc<dyn SourceEvidenceDetector>,
        now: Arc<AtomicU64>,
        path: &std::path::Path,
    ) -> SourceEvidenceCoordinator {
        SourceEvidenceCoordinator::with_clock_and_state_path(
            detector,
            path.to_path_buf(),
            move || now.load(Ordering::SeqCst),
        )
        .expect("persistent coordinator")
    }

    #[tokio::test]
    async fn automatic_reuses_successful_evidence_for_one_hour() {
        let detector = Arc::new(ScriptedDetector::new([
            modified(
                "revision-1",
                [("skills/alpha", SkillRevision::GitTreeOid("tree-a".into()))],
            ),
            modified(
                "revision-2",
                [("skills/alpha", SkillRevision::GitTreeOid("tree-b".into()))],
            ),
        ]));
        let now = Arc::new(AtomicU64::new(1_000));
        let coordinator = coordinator(detector.clone(), now.clone());

        let first = coordinator
            .check(
                request("acme/tools", EvidenceCheckMode::Automatic),
                CancellationSignal::default(),
            )
            .await
            .unwrap();
        now.store(1_000 + 59 * 60 * 1_000, Ordering::SeqCst);
        let cached = coordinator
            .check(
                request("acme/tools", EvidenceCheckMode::Automatic),
                CancellationSignal::default(),
            )
            .await
            .unwrap();
        now.store(1_000 + 60 * 60 * 1_000 + 1, Ordering::SeqCst);
        let refreshed = coordinator
            .check(
                request("acme/tools", EvidenceCheckMode::Automatic),
                CancellationSignal::default(),
            )
            .await
            .unwrap();

        assert_eq!(first.freshness, EvidenceFreshness::Fresh);
        assert_eq!(cached.freshness, EvidenceFreshness::Cached);
        assert_eq!(refreshed.freshness, EvidenceFreshness::Fresh);
        assert_eq!(
            refreshed.evidence.unwrap().snapshot_id.commit_revision,
            "revision-2"
        );
        assert_eq!(detector.calls(), 2);
    }

    #[tokio::test]
    async fn force_requests_join_the_same_inflight_detection() {
        let detector = Arc::new(ScriptedDetector::delayed(
            modified(
                "revision-1",
                [("skills/alpha", SkillRevision::GitTreeOid("tree-a".into()))],
            ),
            Duration::from_millis(60),
        ));
        let coordinator = Arc::new(coordinator(
            detector.clone(),
            Arc::new(AtomicU64::new(1_000)),
        ));

        let left = {
            let coordinator = coordinator.clone();
            tokio::spawn(async move {
                coordinator
                    .check(
                        request("acme/tools", EvidenceCheckMode::Force),
                        CancellationSignal::default(),
                    )
                    .await
            })
        };
        tokio::time::sleep(Duration::from_millis(10)).await;
        let right = coordinator
            .check(
                request("acme/tools", EvidenceCheckMode::Force),
                CancellationSignal::default(),
            )
            .await;

        assert!(left.await.unwrap().is_ok());
        assert!(right.is_ok());
        assert_eq!(detector.calls(), 1);
    }

    #[tokio::test]
    async fn force_respects_provider_cooldown() {
        let detector = Arc::new(ScriptedDetector::new([modified("revision-1", [])]));
        let coordinator = coordinator(detector.clone(), Arc::new(AtomicU64::new(1_000)));
        coordinator.record_provider_cooldown(&EnvironmentRef::Native, throttle_key(), 2_000);

        let result = coordinator
            .check(
                request("acme/tools", EvidenceCheckMode::Force),
                CancellationSignal::default(),
            )
            .await
            .unwrap();

        assert_eq!(result.freshness, EvidenceFreshness::CoolingDown);
        assert_eq!(detector.calls(), 0);
    }

    #[tokio::test]
    async fn provider_cooldown_is_the_latest_failure_exposed_to_the_caller() {
        let detector = Arc::new(ScriptedDetector::new([EvidenceDetectionOutcome::Failed(
            EvidenceDetectionFailure::network("offline"),
        )]));
        let coordinator = coordinator(detector.clone(), Arc::new(AtomicU64::new(1_000)));
        coordinator
            .check(
                request("acme/tools", EvidenceCheckMode::Force),
                CancellationSignal::default(),
            )
            .await
            .unwrap();
        coordinator.record_provider_cooldown(&EnvironmentRef::Native, throttle_key(), 5_000);

        let result = coordinator
            .check(
                request("acme/tools", EvidenceCheckMode::Force),
                CancellationSignal::default(),
            )
            .await
            .unwrap();
        let failure = result.last_attempt.unwrap().failure.unwrap();

        assert_eq!(result.freshness, EvidenceFreshness::CoolingDown);
        assert_eq!(failure.reason, EvidenceFailureReason::RateLimited);
        assert_eq!(failure.retry_at_epoch_ms, Some(5_000));
        assert!(failure.provider_cooldown);
        assert_eq!(detector.calls(), 1);
    }

    #[tokio::test]
    async fn provider_cooldown_is_isolated_by_environment() {
        let detector = Arc::new(ScriptedDetector::new([modified(
            "revision-1",
            [("skills/alpha", SkillRevision::GitTreeOid("tree-a".into()))],
        )]));
        let coordinator = coordinator(detector.clone(), Arc::new(AtomicU64::new(1_000)));
        coordinator.record_provider_cooldown(&EnvironmentRef::Native, throttle_key(), 2_000);

        let wsl = coordinator
            .check(
                request_in_environment(
                    "acme/tools",
                    EvidenceCheckMode::Force,
                    EnvironmentRef::Wsl {
                        distro_name: "Ubuntu".to_string(),
                    },
                ),
                CancellationSignal::default(),
            )
            .await
            .unwrap();

        assert_eq!(wsl.freshness, EvidenceFreshness::Fresh);
        assert_eq!(detector.calls(), 1);
    }

    #[tokio::test]
    async fn provider_cooldown_normalizes_wsl_distro_identity() {
        let detector = Arc::new(ScriptedDetector::new([modified("revision-1", [])]));
        let coordinator = coordinator(detector.clone(), Arc::new(AtomicU64::new(1_000)));
        coordinator.record_provider_cooldown(
            &EnvironmentRef::Wsl {
                distro_name: "Ubuntu".to_string(),
            },
            throttle_key(),
            2_000,
        );

        let result = coordinator
            .check(
                request_in_environment(
                    "acme/tools",
                    EvidenceCheckMode::Force,
                    EnvironmentRef::Wsl {
                        distro_name: "ubuntu".to_string(),
                    },
                ),
                CancellationSignal::default(),
            )
            .await
            .unwrap();

        assert_eq!(result.freshness, EvidenceFreshness::CoolingDown);
        assert_eq!(detector.calls(), 0);
    }

    #[tokio::test]
    async fn automatic_respects_network_backoff() {
        let detector = Arc::new(ScriptedDetector::new([EvidenceDetectionOutcome::Failed(
            EvidenceDetectionFailure {
                reason: EvidenceFailureReason::Network,
                message: "offline".to_string(),
                retry_at_epoch_ms: Some(2_000),
                provider_cooldown: false,
            },
        )]));
        let coordinator = coordinator(detector.clone(), Arc::new(AtomicU64::new(1_000)));
        coordinator
            .check(
                request("acme/tools", EvidenceCheckMode::Automatic),
                CancellationSignal::default(),
            )
            .await
            .unwrap();

        let backed_off = coordinator
            .check(
                request("acme/tools", EvidenceCheckMode::Automatic),
                CancellationSignal::default(),
            )
            .await
            .unwrap();

        assert_eq!(backed_off.freshness, EvidenceFreshness::BackingOff);
        assert_eq!(detector.calls(), 1);
    }

    #[tokio::test]
    async fn automatic_does_not_repeat_user_action_failures_but_force_can_retry() {
        let detector = Arc::new(ScriptedDetector::new([
            EvidenceDetectionOutcome::Failed(EvidenceDetectionFailure {
                reason: EvidenceFailureReason::AuthenticationRequired,
                message: "token required".to_string(),
                retry_at_epoch_ms: None,
                provider_cooldown: false,
            }),
            modified(
                "revision-1",
                [("skills/alpha", SkillRevision::GitTreeOid("tree-a".into()))],
            ),
        ]));
        let coordinator = coordinator(detector.clone(), Arc::new(AtomicU64::new(1_000)));

        coordinator
            .check(
                request("acme/tools", EvidenceCheckMode::Force),
                CancellationSignal::default(),
            )
            .await
            .unwrap();
        let automatic = coordinator
            .check(
                request("acme/tools", EvidenceCheckMode::Automatic),
                CancellationSignal::default(),
            )
            .await
            .unwrap();
        assert_eq!(
            automatic.last_attempt.unwrap().failure.unwrap().reason,
            EvidenceFailureReason::AuthenticationRequired
        );
        assert_eq!(detector.calls(), 1);

        let forced = coordinator
            .check(
                request("acme/tools", EvidenceCheckMode::Force),
                CancellationSignal::default(),
            )
            .await
            .unwrap();
        assert_eq!(forced.freshness, EvidenceFreshness::Fresh);
        assert_eq!(detector.calls(), 2);
    }

    #[tokio::test]
    async fn every_user_action_failure_suppresses_automatic_and_allows_force_retry() {
        for reason in [
            EvidenceFailureReason::AuthenticationRequired,
            EvidenceFailureReason::RefNotFound,
            EvidenceFailureReason::RepositoryNotFound,
            EvidenceFailureReason::NotFoundOrUnauthorized,
        ] {
            let detector = Arc::new(ScriptedDetector::new([
                EvidenceDetectionOutcome::Failed(EvidenceDetectionFailure {
                    reason,
                    message: "user action required".to_string(),
                    retry_at_epoch_ms: None,
                    provider_cooldown: false,
                }),
                modified(
                    "revision-1",
                    [("skills/alpha", SkillRevision::GitTreeOid("tree-a".into()))],
                ),
            ]));
            let coordinator = coordinator(detector.clone(), Arc::new(AtomicU64::new(1_000)));

            coordinator
                .check(
                    request("acme/tools", EvidenceCheckMode::Automatic),
                    CancellationSignal::default(),
                )
                .await
                .unwrap();
            let suppressed = coordinator
                .check(
                    request("acme/tools", EvidenceCheckMode::Automatic),
                    CancellationSignal::default(),
                )
                .await
                .unwrap();

            assert_eq!(
                suppressed.last_attempt.unwrap().failure.unwrap().reason,
                reason
            );
            assert_eq!(detector.calls(), 1);

            let forced = coordinator
                .check(
                    request("acme/tools", EvidenceCheckMode::Force),
                    CancellationSignal::default(),
                )
                .await
                .unwrap();
            assert_eq!(forced.freshness, EvidenceFreshness::Fresh);
            assert_eq!(detector.calls(), 2);
        }
    }

    #[tokio::test]
    async fn credential_change_clears_only_native_github_auth_suppression_and_cooldown() {
        let detector = Arc::new(ScriptedDetector::new([
            EvidenceDetectionOutcome::Failed(EvidenceDetectionFailure {
                reason: EvidenceFailureReason::AuthenticationRequired,
                message: "token required".to_string(),
                retry_at_epoch_ms: None,
                provider_cooldown: false,
            }),
            EvidenceDetectionOutcome::Failed(EvidenceDetectionFailure::network("offline")),
        ]));
        let coordinator = coordinator(detector, Arc::new(AtomicU64::new(1_000)));
        coordinator
            .check(
                request("acme/private", EvidenceCheckMode::Force),
                CancellationSignal::default(),
            )
            .await
            .unwrap();
        coordinator
            .check(
                request("acme/network", EvidenceCheckMode::Force),
                CancellationSignal::default(),
            )
            .await
            .unwrap();
        coordinator.record_provider_cooldown(&EnvironmentRef::Native, throttle_key(), 60_000);

        coordinator.clear_native_github_auth_suppression().unwrap();

        let auth_key = EnvironmentEvidenceKey::new(&EnvironmentRef::Native, &key("acme/private"));
        let network_key =
            EnvironmentEvidenceKey::new(&EnvironmentRef::Native, &key("acme/network"));
        let throttle = EnvironmentThrottleKey::new(&EnvironmentRef::Native, &throttle_key());
        let state = state(&coordinator.inner).unwrap();
        assert!(!state.attempts.contains_key(&auth_key));
        assert!(state.attempts.contains_key(&network_key));
        assert!(state.network_backoff.contains_key(&network_key));
        assert!(state.network_failure_counts.contains_key(&network_key));
        assert!(!state.provider_cooldowns.contains_key(&throttle));
    }

    #[tokio::test]
    async fn credential_cleanup_write_failure_keeps_memory_and_disk_suppression() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("state/update-check.json");
        let now = Arc::new(AtomicU64::new(1_000));
        let detector = Arc::new(ScriptedDetector::new([
            EvidenceDetectionOutcome::Failed(EvidenceDetectionFailure {
                reason: EvidenceFailureReason::AuthenticationRequired,
                message: "token required".to_string(),
                retry_at_epoch_ms: None,
                provider_cooldown: false,
            }),
            EvidenceDetectionOutcome::Failed(EvidenceDetectionFailure {
                reason: EvidenceFailureReason::RateLimited,
                message: "rate limited".to_string(),
                retry_at_epoch_ms: Some(60_000),
                provider_cooldown: true,
            }),
            modified(
                "unexpected-revision",
                [(
                    "skills/alpha",
                    SkillRevision::GitTreeOid("unexpected-tree".into()),
                )],
            ),
        ]));
        let coordinator = persistent_coordinator(detector.clone(), now.clone(), &path);
        coordinator
            .check(
                request("acme/private", EvidenceCheckMode::Force),
                CancellationSignal::default(),
            )
            .await
            .unwrap();
        coordinator
            .check(
                request("acme/limited", EvidenceCheckMode::Force),
                CancellationSignal::default(),
            )
            .await
            .unwrap();
        let persisted_before = std::fs::read(&path).expect("persisted state before cleanup");
        coordinator
            .inner
            .state_file
            .as_ref()
            .expect("state file")
            .set_write_failure(true);

        assert!(coordinator.clear_native_github_auth_suppression().is_err());
        coordinator
            .inner
            .state_file
            .as_ref()
            .expect("state file")
            .set_write_failure(false);
        assert_eq!(
            std::fs::read(&path).expect("persisted state after cleanup"),
            persisted_before
        );

        let cooling_down = coordinator
            .check(
                request("acme/private", EvidenceCheckMode::Force),
                CancellationSignal::default(),
            )
            .await
            .unwrap();
        assert_eq!(cooling_down.freshness, EvidenceFreshness::CoolingDown);
        assert_eq!(detector.calls(), 2);

        let restarted_now = Arc::new(AtomicU64::new(1_000));
        let restarted_detector = Arc::new(ScriptedDetector::new([modified(
            "unexpected-restart-revision",
            [(
                "skills/alpha",
                SkillRevision::GitTreeOid("unexpected-restart-tree".into()),
            )],
        )]));
        let restarted =
            persistent_coordinator(restarted_detector.clone(), restarted_now.clone(), &path);
        let restarted_cooldown = restarted
            .check(
                request("acme/private", EvidenceCheckMode::Force),
                CancellationSignal::default(),
            )
            .await
            .unwrap();
        assert_eq!(restarted_cooldown.freshness, EvidenceFreshness::CoolingDown);
        assert_eq!(restarted_detector.calls(), 0);
        restarted_now.store(60_001, Ordering::SeqCst);
        let restarted_suppression = restarted
            .check(
                request("acme/private", EvidenceCheckMode::Automatic),
                CancellationSignal::default(),
            )
            .await
            .unwrap();
        assert_eq!(
            restarted_suppression.freshness,
            EvidenceFreshness::Unavailable
        );
        assert_eq!(restarted_detector.calls(), 0);

        now.store(60_001, Ordering::SeqCst);
        let still_suppressed = coordinator
            .check(
                request("acme/private", EvidenceCheckMode::Automatic),
                CancellationSignal::default(),
            )
            .await
            .unwrap();
        assert_eq!(still_suppressed.freshness, EvidenceFreshness::Unavailable);
        assert_eq!(detector.calls(), 2);
    }

    #[tokio::test]
    async fn source_repair_clears_only_the_exact_environment_and_source() {
        let detector = Arc::new(ScriptedDetector::new([
            EvidenceDetectionOutcome::Failed(EvidenceDetectionFailure::network("first")),
            EvidenceDetectionOutcome::Failed(EvidenceDetectionFailure::network("second")),
            EvidenceDetectionOutcome::Failed(EvidenceDetectionFailure::network("wsl-first")),
        ]));
        let coordinator = coordinator(detector, Arc::new(AtomicU64::new(1_000)));
        coordinator
            .check(
                request("acme/first", EvidenceCheckMode::Force),
                CancellationSignal::default(),
            )
            .await
            .unwrap();
        coordinator
            .check(
                request("acme/second", EvidenceCheckMode::Force),
                CancellationSignal::default(),
            )
            .await
            .unwrap();
        coordinator
            .check(
                request_in_environment(
                    "acme/first",
                    EvidenceCheckMode::Force,
                    EnvironmentRef::Wsl {
                        distro_name: "Ubuntu".to_string(),
                    },
                ),
                CancellationSignal::default(),
            )
            .await
            .unwrap();
        coordinator.record_provider_cooldown(&EnvironmentRef::Native, throttle_key(), 60_000);

        coordinator
            .clear_source_suppression(&EnvironmentRef::Native, &key("acme/first"))
            .unwrap();

        let first = EnvironmentEvidenceKey::new(&EnvironmentRef::Native, &key("acme/first"));
        let second = EnvironmentEvidenceKey::new(&EnvironmentRef::Native, &key("acme/second"));
        let wsl_first = EnvironmentEvidenceKey::new(
            &EnvironmentRef::Wsl {
                distro_name: "Ubuntu".to_string(),
            },
            &key("acme/first"),
        );
        let throttle = EnvironmentThrottleKey::new(&EnvironmentRef::Native, &throttle_key());
        let state = state(&coordinator.inner).unwrap();
        assert!(!state.attempts.contains_key(&first));
        assert!(!state.network_backoff.contains_key(&first));
        assert!(!state.network_failure_counts.contains_key(&first));
        assert!(state.attempts.contains_key(&second));
        assert!(state.network_backoff.contains_key(&second));
        assert!(state.network_failure_counts.contains_key(&second));
        assert!(state.attempts.contains_key(&wsl_first));
        assert!(state.network_backoff.contains_key(&wsl_first));
        assert!(state.network_failure_counts.contains_key(&wsl_first));
        assert!(state.provider_cooldowns.contains_key(&throttle));
    }

    #[tokio::test]
    async fn source_cleanup_write_failure_keeps_memory_and_disk_backoff() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("state/update-check.json");
        let now = Arc::new(AtomicU64::new(1_000));
        let detector = Arc::new(ScriptedDetector::new([
            EvidenceDetectionOutcome::Failed(EvidenceDetectionFailure::network("offline")),
            modified(
                "unexpected-revision",
                [(
                    "skills/alpha",
                    SkillRevision::GitTreeOid("unexpected-tree".into()),
                )],
            ),
        ]));
        let coordinator = persistent_coordinator(detector.clone(), now.clone(), &path);
        coordinator
            .check(
                request("acme/tools", EvidenceCheckMode::Force),
                CancellationSignal::default(),
            )
            .await
            .unwrap();
        let persisted_before = std::fs::read(&path).expect("persisted state before cleanup");
        coordinator
            .inner
            .state_file
            .as_ref()
            .expect("state file")
            .set_write_failure(true);

        assert!(coordinator
            .clear_source_suppression(&EnvironmentRef::Native, &key("acme/tools"))
            .is_err());
        coordinator
            .inner
            .state_file
            .as_ref()
            .expect("state file")
            .set_write_failure(false);
        assert_eq!(
            std::fs::read(&path).expect("persisted state after cleanup"),
            persisted_before
        );

        let backed_off = coordinator
            .check(
                request("acme/tools", EvidenceCheckMode::Automatic),
                CancellationSignal::default(),
            )
            .await
            .unwrap();
        assert_eq!(backed_off.freshness, EvidenceFreshness::BackingOff);
        assert_eq!(detector.calls(), 1);

        let restarted_detector = Arc::new(ScriptedDetector::new([modified(
            "unexpected-restart-revision",
            [(
                "skills/alpha",
                SkillRevision::GitTreeOid("unexpected-restart-tree".into()),
            )],
        )]));
        let restarted = persistent_coordinator(restarted_detector.clone(), now, &path);
        let restarted_backoff = restarted
            .check(
                request("acme/tools", EvidenceCheckMode::Automatic),
                CancellationSignal::default(),
            )
            .await
            .unwrap();
        assert_eq!(restarted_backoff.freshness, EvidenceFreshness::BackingOff);
        assert_eq!(restarted_detector.calls(), 0);
    }

    #[tokio::test]
    async fn force_bypasses_transient_backoff_and_all_transient_failures_share_the_schedule() {
        let detector = Arc::new(ScriptedDetector::new([
            EvidenceDetectionOutcome::Failed(EvidenceDetectionFailure::network("offline")),
            EvidenceDetectionOutcome::Failed(EvidenceDetectionFailure::incomplete("truncated")),
            EvidenceDetectionOutcome::Failed(EvidenceDetectionFailure {
                reason: EvidenceFailureReason::SourceUnavailable,
                message: "unavailable".to_string(),
                retry_at_epoch_ms: None,
                provider_cooldown: false,
            }),
        ]));
        let now = Arc::new(AtomicU64::new(1_000));
        let coordinator = coordinator(detector.clone(), now.clone());

        coordinator
            .check(
                request("acme/tools", EvidenceCheckMode::Force),
                CancellationSignal::default(),
            )
            .await
            .unwrap();
        now.store(31_000, Ordering::SeqCst);
        let automatic = coordinator
            .check(
                request("acme/tools", EvidenceCheckMode::Automatic),
                CancellationSignal::default(),
            )
            .await
            .unwrap();
        assert_eq!(automatic.freshness, EvidenceFreshness::Unavailable);
        assert_eq!(
            automatic.last_attempt.unwrap().failure.unwrap().reason,
            EvidenceFailureReason::IncompleteEvidence
        );

        let blocked = coordinator
            .check(
                request("acme/tools", EvidenceCheckMode::Automatic),
                CancellationSignal::default(),
            )
            .await
            .unwrap();
        assert_eq!(blocked.freshness, EvidenceFreshness::BackingOff);
        assert_eq!(detector.calls(), 2);

        let forced = coordinator
            .check(
                request("acme/tools", EvidenceCheckMode::Force),
                CancellationSignal::default(),
            )
            .await
            .unwrap();
        assert_eq!(
            forced.last_attempt.unwrap().failure.unwrap().reason,
            EvidenceFailureReason::SourceUnavailable
        );
        assert_eq!(detector.calls(), 3);
    }

    #[tokio::test]
    async fn transient_backoff_follows_the_full_schedule_and_survives_restart() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("state/update-check.json");
        let now = Arc::new(AtomicU64::new(1_000));
        let cases = [
            (EvidenceFailureReason::Network, 30_000),
            (EvidenceFailureReason::IncompleteEvidence, 60_000),
            (EvidenceFailureReason::SourceUnavailable, 2 * 60_000),
            (EvidenceFailureReason::Network, 5 * 60_000),
            (EvidenceFailureReason::IncompleteEvidence, 10 * 60_000),
            (EvidenceFailureReason::SourceUnavailable, 30 * 60_000),
            (EvidenceFailureReason::Network, 30 * 60_000),
        ];
        let mut previous_deadline = None;

        for (reason, expected_delay) in cases {
            if let Some(deadline) = previous_deadline {
                now.store(deadline - 1, Ordering::SeqCst);
            }
            let failure = match reason {
                EvidenceFailureReason::Network => EvidenceDetectionFailure::network("offline"),
                EvidenceFailureReason::IncompleteEvidence => {
                    EvidenceDetectionFailure::incomplete("incomplete")
                }
                EvidenceFailureReason::SourceUnavailable => EvidenceDetectionFailure {
                    reason,
                    message: "unavailable".to_string(),
                    retry_at_epoch_ms: None,
                    provider_cooldown: false,
                },
                _ => unreachable!("only transient failures belong in this schedule"),
            };
            let detector = Arc::new(ScriptedDetector::new([EvidenceDetectionOutcome::Failed(
                failure,
            )]));
            let coordinator = persistent_coordinator(detector.clone(), now.clone(), &path);

            if let Some(deadline) = previous_deadline {
                let blocked = coordinator
                    .check(
                        request("acme/tools", EvidenceCheckMode::Automatic),
                        CancellationSignal::default(),
                    )
                    .await
                    .unwrap();
                assert_eq!(blocked.freshness, EvidenceFreshness::BackingOff);
                assert_eq!(detector.calls(), 0);
                now.store(deadline, Ordering::SeqCst);
            }

            let checked_at = now.load(Ordering::SeqCst);
            let result = coordinator
                .check(
                    request("acme/tools", EvidenceCheckMode::Automatic),
                    CancellationSignal::default(),
                )
                .await
                .unwrap();
            let failure = result.last_attempt.unwrap().failure.unwrap();
            let deadline = checked_at + expected_delay;

            assert_eq!(failure.reason, reason);
            assert_eq!(failure.retry_at_epoch_ms, Some(deadline));
            assert_eq!(detector.calls(), 1);
            previous_deadline = Some(deadline);
            drop(coordinator);
        }
    }

    #[tokio::test]
    async fn network_backoff_is_isolated_by_environment() {
        let detector = Arc::new(ScriptedDetector::new([
            EvidenceDetectionOutcome::Failed(EvidenceDetectionFailure::network("offline")),
            modified(
                "revision-1",
                [("skills/alpha", SkillRevision::GitTreeOid("tree-a".into()))],
            ),
        ]));
        let coordinator = coordinator(detector.clone(), Arc::new(AtomicU64::new(1_000)));

        let native = coordinator
            .check(
                request_in_environment(
                    "acme/tools",
                    EvidenceCheckMode::Force,
                    EnvironmentRef::Native,
                ),
                CancellationSignal::default(),
            )
            .await
            .unwrap();
        let wsl = coordinator
            .check(
                request_in_environment(
                    "acme/tools",
                    EvidenceCheckMode::Force,
                    EnvironmentRef::Wsl {
                        distro_name: "Ubuntu".to_string(),
                    },
                ),
                CancellationSignal::default(),
            )
            .await
            .unwrap();

        assert_eq!(native.freshness, EvidenceFreshness::Unavailable);
        assert_eq!(wsl.freshness, EvidenceFreshness::Fresh);
        assert_eq!(detector.calls(), 2);
    }

    #[tokio::test]
    async fn in_flight_detection_is_isolated_by_environment() {
        let detector = Arc::new(ScriptedDetector::delayed(
            modified(
                "revision-1",
                [("skills/alpha", SkillRevision::GitTreeOid("tree-a".into()))],
            ),
            Duration::from_millis(60),
        ));
        let coordinator = Arc::new(coordinator(
            detector.clone(),
            Arc::new(AtomicU64::new(1_000)),
        ));

        let native = {
            let coordinator = coordinator.clone();
            tokio::spawn(async move {
                coordinator
                    .check(
                        request_in_environment(
                            "acme/tools",
                            EvidenceCheckMode::Force,
                            EnvironmentRef::Native,
                        ),
                        CancellationSignal::default(),
                    )
                    .await
            })
        };
        let wsl = coordinator.check(
            request_in_environment(
                "acme/tools",
                EvidenceCheckMode::Force,
                EnvironmentRef::Wsl {
                    distro_name: "Ubuntu".to_string(),
                },
            ),
            CancellationSignal::default(),
        );

        assert!(native.await.unwrap().is_ok());
        assert!(wsl.await.is_ok());
        assert_eq!(detector.calls(), 2);
    }

    #[tokio::test]
    async fn successful_evidence_is_shared_across_environments() {
        let detector = Arc::new(ScriptedDetector::new([modified(
            "revision-1",
            [("skills/alpha", SkillRevision::GitTreeOid("tree-a".into()))],
        )]));
        let coordinator = coordinator(detector.clone(), Arc::new(AtomicU64::new(1_000)));

        coordinator
            .check(
                request_in_environment(
                    "acme/tools",
                    EvidenceCheckMode::Automatic,
                    EnvironmentRef::Native,
                ),
                CancellationSignal::default(),
            )
            .await
            .unwrap();
        let wsl = coordinator
            .check(
                request_in_environment(
                    "acme/tools",
                    EvidenceCheckMode::Automatic,
                    EnvironmentRef::Wsl {
                        distro_name: "Ubuntu".to_string(),
                    },
                ),
                CancellationSignal::default(),
            )
            .await
            .unwrap();

        assert_eq!(wsl.freshness, EvidenceFreshness::Cached);
        assert_eq!(wsl.last_attempt, None);
        assert_eq!(detector.calls(), 1);
    }

    #[tokio::test]
    async fn persisted_evidence_is_reused_after_restart() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("state/update-check.json");
        let now = Arc::new(AtomicU64::new(1_000));
        let first_detector = Arc::new(ScriptedDetector::new([modified(
            "revision-1",
            [("skills/alpha", SkillRevision::GitTreeOid("tree-a".into()))],
        )]));
        let first = persistent_coordinator(first_detector.clone(), now.clone(), &path);

        first
            .check(
                request("acme/tools", EvidenceCheckMode::Automatic),
                CancellationSignal::default(),
            )
            .await
            .unwrap();
        assert_eq!(first_detector.calls(), 1);
        drop(first);

        now.store(2_000, Ordering::SeqCst);
        let second_detector = Arc::new(ScriptedDetector::new([]));
        let second = persistent_coordinator(second_detector.clone(), now, &path);
        let cached = second
            .check(
                request("acme/tools", EvidenceCheckMode::Automatic),
                CancellationSignal::default(),
            )
            .await
            .unwrap();

        assert_eq!(cached.freshness, EvidenceFreshness::Cached);
        assert_eq!(second_detector.calls(), 0);
    }

    #[tokio::test]
    async fn user_action_failure_remains_suppressed_after_restart_but_force_can_retry() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("state/update-check.json");
        let now = Arc::new(AtomicU64::new(1_000));
        let first_detector = Arc::new(ScriptedDetector::new([EvidenceDetectionOutcome::Failed(
            EvidenceDetectionFailure {
                reason: EvidenceFailureReason::AuthenticationRequired,
                message: "token required".to_string(),
                retry_at_epoch_ms: None,
                provider_cooldown: false,
            },
        )]));
        let first = persistent_coordinator(first_detector.clone(), now.clone(), &path);
        first
            .check(
                request("acme/private", EvidenceCheckMode::Automatic),
                CancellationSignal::default(),
            )
            .await
            .unwrap();
        assert_eq!(first_detector.calls(), 1);
        drop(first);

        now.store(2_000, Ordering::SeqCst);
        let second_detector = Arc::new(ScriptedDetector::new([modified(
            "revision-1",
            [("skills/alpha", SkillRevision::GitTreeOid("tree-a".into()))],
        )]));
        let second = persistent_coordinator(second_detector.clone(), now, &path);
        let automatic = second
            .check(
                request("acme/private", EvidenceCheckMode::Automatic),
                CancellationSignal::default(),
            )
            .await
            .unwrap();

        assert_eq!(automatic.freshness, EvidenceFreshness::Unavailable);
        assert_eq!(
            automatic.last_attempt.unwrap().failure.unwrap().reason,
            EvidenceFailureReason::AuthenticationRequired,
        );
        assert_eq!(second_detector.calls(), 0);

        let forced = second
            .check(
                request("acme/private", EvidenceCheckMode::Force),
                CancellationSignal::default(),
            )
            .await
            .unwrap();
        assert_eq!(forced.freshness, EvidenceFreshness::Fresh);
        assert_eq!(second_detector.calls(), 1);
    }

    #[tokio::test]
    async fn provider_cooldown_remains_effective_after_restart_until_its_deadline() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("state/update-check.json");
        let now = Arc::new(AtomicU64::new(1_000));
        let first_detector = Arc::new(ScriptedDetector::new([EvidenceDetectionOutcome::Failed(
            EvidenceDetectionFailure {
                reason: EvidenceFailureReason::RateLimited,
                message: "rate limited".to_string(),
                retry_at_epoch_ms: Some(5_000),
                provider_cooldown: true,
            },
        )]));
        let first = persistent_coordinator(first_detector.clone(), now.clone(), &path);
        first
            .check(
                request("acme/tools", EvidenceCheckMode::Force),
                CancellationSignal::default(),
            )
            .await
            .unwrap();
        assert_eq!(first_detector.calls(), 1);
        drop(first);

        now.store(2_000, Ordering::SeqCst);
        let second_detector = Arc::new(ScriptedDetector::new([modified(
            "revision-1",
            [("skills/alpha", SkillRevision::GitTreeOid("tree-a".into()))],
        )]));
        let second = persistent_coordinator(second_detector.clone(), now.clone(), &path);
        let blocked = second
            .check(
                request("acme/tools", EvidenceCheckMode::Force),
                CancellationSignal::default(),
            )
            .await
            .unwrap();
        let failure = blocked.last_attempt.unwrap().failure.unwrap();

        assert_eq!(blocked.freshness, EvidenceFreshness::CoolingDown);
        assert_eq!(failure.retry_at_epoch_ms, Some(5_000));
        assert_eq!(second_detector.calls(), 0);

        now.store(5_000, Ordering::SeqCst);
        let refreshed = second
            .check(
                request("acme/tools", EvidenceCheckMode::Force),
                CancellationSignal::default(),
            )
            .await
            .unwrap();
        assert_eq!(refreshed.freshness, EvidenceFreshness::Fresh);
        assert_eq!(second_detector.calls(), 1);
    }

    #[tokio::test]
    async fn corrupt_persisted_state_is_quarantined_before_rebuilding() {
        let temp = tempdir().expect("tempdir");
        let state_dir = temp.path().join("state");
        std::fs::create_dir_all(&state_dir).expect("state dir");
        let path = state_dir.join("update-check.json");
        std::fs::write(&path, b"not-json").expect("corrupt state");
        let detector = Arc::new(ScriptedDetector::new([modified(
            "revision-1",
            [("skills/alpha", SkillRevision::GitTreeOid("tree-a".into()))],
        )]));
        let coordinator =
            persistent_coordinator(detector.clone(), Arc::new(AtomicU64::new(1_000)), &path);

        assert!(!path.exists());
        assert!(std::fs::read_dir(&state_dir)
            .unwrap()
            .filter_map(Result::ok)
            .any(|entry| entry
                .file_name()
                .to_string_lossy()
                .starts_with("update-check.json.corrupt-")));

        coordinator
            .check(
                request("acme/tools", EvidenceCheckMode::Automatic),
                CancellationSignal::default(),
            )
            .await
            .unwrap();
        assert_eq!(detector.calls(), 1);
    }

    #[tokio::test]
    async fn persisted_failure_omits_raw_provider_message() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("state/update-check.json");
        let detector = Arc::new(ScriptedDetector::new([EvidenceDetectionOutcome::Failed(
            EvidenceDetectionFailure::network("secret-provider-error"),
        )]));
        let coordinator = persistent_coordinator(detector, Arc::new(AtomicU64::new(1_000)), &path);

        coordinator
            .check(
                request("acme/tools", EvidenceCheckMode::Automatic),
                CancellationSignal::default(),
            )
            .await
            .unwrap();

        let persisted = std::fs::read_to_string(path).expect("persisted state");
        assert!(!persisted.contains("secret-provider-error"));
        assert!(persisted.contains("network"));
    }

    #[tokio::test]
    async fn persisted_state_prunes_diagnostics_after_seven_days_and_sources_after_thirty() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("state/update-check.json");
        let now = Arc::new(AtomicU64::new(1_000));
        let detector = Arc::new(ScriptedDetector::new([
            modified(
                "revision-1",
                [("skills/alpha", SkillRevision::GitTreeOid("tree-a".into()))],
            ),
            EvidenceDetectionOutcome::Failed(EvidenceDetectionFailure::network("offline")),
        ]));
        let coordinator = persistent_coordinator(detector, now.clone(), &path);
        coordinator
            .check(
                request("acme/tools", EvidenceCheckMode::Automatic),
                CancellationSignal::default(),
            )
            .await
            .unwrap();
        now.store(2_000, Ordering::SeqCst);
        coordinator
            .check(
                request("acme/tools", EvidenceCheckMode::Automatic),
                CancellationSignal::default(),
            )
            .await
            .unwrap();
        drop(coordinator);

        now.store(2_000 + 8 * 24 * 60 * 60 * 1_000, Ordering::SeqCst);
        drop(persistent_coordinator(
            Arc::new(ScriptedDetector::new([])),
            now.clone(),
            &path,
        ));
        let after_diagnostic_cleanup: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(
            after_diagnostic_cleanup["attempts"]
                .as_array()
                .unwrap()
                .len(),
            0
        );
        assert_eq!(
            after_diagnostic_cleanup["evidence"]
                .as_array()
                .unwrap()
                .len(),
            1
        );

        now.store(2_000 + 31 * 24 * 60 * 60 * 1_000, Ordering::SeqCst);
        drop(persistent_coordinator(
            Arc::new(ScriptedDetector::new([])),
            now,
            &path,
        ));
        let after_source_cleanup: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(
            after_source_cleanup["evidence"].as_array().unwrap().len(),
            0
        );
    }

    #[tokio::test]
    async fn network_backoff_doubles_and_resets_after_success() {
        let now = Arc::new(AtomicU64::new(1_000));
        let detector = Arc::new(ScriptedDetector::new([
            EvidenceDetectionOutcome::Failed(EvidenceDetectionFailure::network("first")),
            EvidenceDetectionOutcome::Failed(EvidenceDetectionFailure::network("second")),
            modified(
                "revision-1",
                [("skills/alpha", SkillRevision::GitTreeOid("tree-a".into()))],
            ),
            EvidenceDetectionOutcome::Failed(EvidenceDetectionFailure::network("after success")),
        ]));
        let coordinator = SourceEvidenceCoordinator::with_clock(detector.clone(), {
            let now = now.clone();
            move || now.load(Ordering::SeqCst)
        });

        let first = coordinator
            .check(
                request("acme/tools", EvidenceCheckMode::Automatic),
                CancellationSignal::default(),
            )
            .await
            .unwrap();
        assert_eq!(first.freshness, EvidenceFreshness::Unavailable);
        assert_eq!(detector.calls(), 1);

        now.store(30_999, Ordering::SeqCst);
        let backing_off = coordinator
            .check(
                request("acme/tools", EvidenceCheckMode::Automatic),
                CancellationSignal::default(),
            )
            .await
            .unwrap();
        assert_eq!(backing_off.freshness, EvidenceFreshness::BackingOff);
        assert_eq!(detector.calls(), 1);

        now.store(31_000, Ordering::SeqCst);
        coordinator
            .check(
                request("acme/tools", EvidenceCheckMode::Automatic),
                CancellationSignal::default(),
            )
            .await
            .unwrap();
        assert_eq!(detector.calls(), 2);

        now.store(90_999, Ordering::SeqCst);
        let backing_off = coordinator
            .check(
                request("acme/tools", EvidenceCheckMode::Automatic),
                CancellationSignal::default(),
            )
            .await
            .unwrap();
        assert_eq!(backing_off.freshness, EvidenceFreshness::BackingOff);
        assert_eq!(detector.calls(), 2);

        now.store(91_000, Ordering::SeqCst);
        coordinator
            .check(
                request("acme/tools", EvidenceCheckMode::Automatic),
                CancellationSignal::default(),
            )
            .await
            .unwrap();
        assert_eq!(detector.calls(), 3);

        now.store(91_001, Ordering::SeqCst);
        coordinator
            .check(
                request("acme/tools", EvidenceCheckMode::Force),
                CancellationSignal::default(),
            )
            .await
            .unwrap();
        assert_eq!(detector.calls(), 4);
        let operation_key =
            EnvironmentEvidenceKey::new(&EnvironmentRef::Native, &key("acme/tools"));
        assert_eq!(
            state(&coordinator.inner)
                .unwrap()
                .network_backoff
                .get(&operation_key),
            Some(&121_001),
        );

        now.store(120_999, Ordering::SeqCst);
        let cached = coordinator
            .check(
                request("acme/tools", EvidenceCheckMode::Automatic),
                CancellationSignal::default(),
            )
            .await
            .unwrap();
        assert_eq!(cached.freshness, EvidenceFreshness::Cached);
        assert_eq!(detector.calls(), 4);
    }

    #[tokio::test]
    async fn failed_refresh_preserves_last_successful_evidence_and_ttl() {
        let detector = Arc::new(ScriptedDetector::new([
            modified(
                "revision-1",
                [("skills/alpha", SkillRevision::GitTreeOid("tree-a".into()))],
            ),
            EvidenceDetectionOutcome::Failed(EvidenceDetectionFailure::network("offline")),
        ]));
        let now = Arc::new(AtomicU64::new(1_000));
        let coordinator = coordinator(detector, now.clone());
        let first = coordinator
            .check(
                request("acme/tools", EvidenceCheckMode::Automatic),
                CancellationSignal::default(),
            )
            .await
            .unwrap();
        let first_evidence = first.evidence.unwrap();

        now.store(first_evidence.expires_at_epoch_ms + 1, Ordering::SeqCst);
        let failed = coordinator
            .check(
                request("acme/tools", EvidenceCheckMode::Automatic),
                CancellationSignal::default(),
            )
            .await
            .unwrap();

        assert_eq!(failed.freshness, EvidenceFreshness::Stale);
        assert_eq!(failed.evidence.unwrap(), first_evidence);
        assert_eq!(
            failed.last_attempt.unwrap().failure.unwrap().reason,
            EvidenceFailureReason::Network
        );
    }

    #[tokio::test]
    async fn not_modified_refreshes_ttl_without_replacing_cached_facts() {
        let detector = Arc::new(ScriptedDetector::new([
            modified(
                "revision-1",
                [("skills/alpha", SkillRevision::GitTreeOid("tree-a".into()))],
            ),
            EvidenceDetectionOutcome::NotModified,
        ]));
        let now = Arc::new(AtomicU64::new(1_000));
        let coordinator = coordinator(detector.clone(), now.clone());

        let first = coordinator
            .check(
                request("acme/tools", EvidenceCheckMode::Force),
                CancellationSignal::default(),
            )
            .await
            .unwrap()
            .evidence
            .unwrap();
        now.store(first.expires_at_epoch_ms + 1, Ordering::SeqCst);
        let refreshed = coordinator
            .check(
                request("acme/tools", EvidenceCheckMode::Force),
                CancellationSignal::default(),
            )
            .await
            .unwrap();
        let evidence = refreshed.evidence.unwrap();

        assert_eq!(refreshed.freshness, EvidenceFreshness::Fresh);
        assert_eq!(evidence.snapshot_id.commit_revision, "revision-1");
        assert_eq!(evidence.skill_revisions, first.skill_revisions);
        assert_eq!(
            evidence.expires_at_epoch_ms,
            now.load(Ordering::SeqCst) + EVIDENCE_TTL_MS
        );
        assert_eq!(detector.calls(), 2);
    }

    #[tokio::test]
    async fn sparse_revisions_merge_only_under_the_same_ref_revision() {
        let detector = Arc::new(ScriptedDetector::new([
            modified(
                "revision-1",
                [("skills/alpha", SkillRevision::GitTreeOid("tree-a".into()))],
            ),
            modified(
                "revision-1",
                [("skills/beta", SkillRevision::GitTreeOid("tree-b".into()))],
            ),
            modified(
                "revision-2",
                [("skills/alpha", SkillRevision::GitTreeOid("tree-a2".into()))],
            ),
        ]));
        let coordinator = coordinator(detector, Arc::new(AtomicU64::new(1_000)));

        coordinator
            .check(
                request("acme/tools", EvidenceCheckMode::Force),
                CancellationSignal::default(),
            )
            .await
            .unwrap();
        let merged = coordinator
            .check(
                request("acme/tools", EvidenceCheckMode::Force),
                CancellationSignal::default(),
            )
            .await
            .unwrap()
            .evidence
            .unwrap();
        assert_eq!(merged.skill_revisions.len(), 2);

        let replaced = coordinator
            .check(
                request("acme/tools", EvidenceCheckMode::Force),
                CancellationSignal::default(),
            )
            .await
            .unwrap()
            .evidence
            .unwrap();
        assert_eq!(replaced.snapshot_id.commit_revision, "revision-2");
        assert_eq!(replaced.skill_revisions.len(), 1);
    }

    #[tokio::test]
    async fn detector_concurrency_is_limited_to_four() {
        let detector = Arc::new(ScriptedDetector::delayed(
            modified(
                "revision-1",
                [(
                    "skills/alpha",
                    SkillRevision::GitTreeOid("tree-alpha".into()),
                )],
            ),
            Duration::from_millis(40),
        ));
        let coordinator = Arc::new(coordinator(
            detector.clone(),
            Arc::new(AtomicU64::new(1_000)),
        ));
        let checks = (0..8)
            .map(|index| {
                let coordinator = coordinator.clone();
                tokio::spawn(async move {
                    coordinator
                        .check(
                            request(&format!("acme/tools-{index}"), EvidenceCheckMode::Force),
                            CancellationSignal::default(),
                        )
                        .await
                })
            })
            .collect::<Vec<_>>();
        for check in checks {
            check.await.unwrap().unwrap();
        }

        assert_eq!(detector.peak(), DETECTOR_CONCURRENCY_LIMIT);
    }

    #[tokio::test]
    async fn cancelling_one_waiter_does_not_cancel_shared_detection() {
        let detector = Arc::new(ScriptedDetector::delayed(
            modified(
                "revision-1",
                [("skills/alpha", SkillRevision::GitTreeOid("tree-a".into()))],
            ),
            Duration::from_millis(60),
        ));
        let coordinator = Arc::new(coordinator(
            detector.clone(),
            Arc::new(AtomicU64::new(1_000)),
        ));
        let cancellation = CancellationSignal::default();
        let cancelled_waiter = {
            let coordinator = coordinator.clone();
            let cancellation = cancellation.clone();
            tokio::spawn(async move {
                coordinator
                    .check(
                        request("acme/tools", EvidenceCheckMode::Force),
                        cancellation,
                    )
                    .await
            })
        };
        tokio::time::sleep(Duration::from_millis(10)).await;
        let surviving_waiter = {
            let coordinator = coordinator.clone();
            tokio::spawn(async move {
                coordinator
                    .check(
                        request("acme/tools", EvidenceCheckMode::Force),
                        CancellationSignal::default(),
                    )
                    .await
            })
        };
        cancellation.cancel();

        assert!(matches!(
            cancelled_waiter.await.unwrap(),
            Err(crate::error::AppError::MutationCancelled)
        ));
        assert!(surviving_waiter.await.unwrap().is_ok());
        assert_eq!(detector.calls(), 1);
    }

    struct ControlledBatchDetector {
        calls: AtomicUsize,
        started: tokio::sync::mpsc::UnboundedSender<(usize, BTreeSet<String>)>,
        release: Arc<tokio::sync::Semaphore>,
        failed_calls: BTreeSet<usize>,
        catalog: BTreeSet<String>,
    }

    impl ControlledBatchDetector {
        fn new(
            failed_calls: BTreeSet<usize>,
        ) -> (
            Arc<Self>,
            tokio::sync::mpsc::UnboundedReceiver<(usize, BTreeSet<String>)>,
        ) {
            let (started, receiver) = tokio::sync::mpsc::unbounded_channel();
            (
                Arc::new(Self {
                    calls: AtomicUsize::new(0),
                    started,
                    release: Arc::new(tokio::sync::Semaphore::new(0)),
                    failed_calls,
                    catalog: BTreeSet::from([
                        "skills/alpha".to_string(),
                        "skills/beta".to_string(),
                        "skills/gamma".to_string(),
                    ]),
                }),
                receiver,
            )
        }

        fn release_one(&self) {
            self.release.add_permits(1);
        }
    }

    impl SourceEvidenceDetector for ControlledBatchDetector {
        fn detect<'a>(
            &'a self,
            request: EvidenceDetectionRequest,
            _previous: Option<RemoteEvidenceEntry>,
            _cancellation: CancellationSignal,
        ) -> EvidenceFuture<'a> {
            Box::pin(async move {
                let call = self.calls.fetch_add(1, Ordering::SeqCst);
                let paths = request.requested_skill_paths;
                self.started
                    .send((call, paths.clone()))
                    .expect("controlled detector receiver");
                let permit = self
                    .release
                    .acquire()
                    .await
                    .expect("controlled detector release");
                permit.forget();
                if self.failed_calls.contains(&call) {
                    return Ok(EvidenceDetectionOutcome::Failed(EvidenceDetectionFailure {
                        reason: EvidenceFailureReason::SourceUnavailable,
                        message: "controlled failure".to_string(),
                        retry_at_epoch_ms: None,
                        provider_cooldown: false,
                    }));
                }
                Ok(EvidenceDetectionOutcome::Modified(
                    RemoteEvidenceObservation {
                        snapshot_id: RemoteSnapshotId::new(
                            NormalizedRef::Named("main".to_string()),
                            "refs/heads/main",
                            "revision-1",
                        ),
                        provider_validation: None,
                        complete_skill_path_catalog: self.catalog.clone(),
                        skill_revisions: paths
                            .into_iter()
                            .map(|path| {
                                (
                                    path.clone(),
                                    SkillRevision::GitTreeOid(format!("tree-{path}")),
                                )
                            })
                            .collect(),
                        snapshot_facts: None,
                    },
                ))
            })
        }
    }

    async fn wait_for_batch_state(
        coordinator: &SourceEvidenceCoordinator,
        predicate: impl Fn(&InFlightDetection) -> bool,
    ) {
        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                let matches = coordinator
                    .inner
                    .state
                    .lock()
                    .unwrap()
                    .in_flight
                    .values()
                    .next()
                    .is_some_and(&predicate);
                if matches {
                    return;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("batch state transition");
    }

    #[tokio::test]
    async fn collecting_batch_merges_paths_before_a_detector_permit_is_available() {
        let (detector, mut started) = ControlledBatchDetector::new(BTreeSet::new());
        let coordinator = Arc::new(coordinator(
            detector.clone(),
            Arc::new(AtomicU64::new(1_000)),
        ));
        let held_permits = coordinator
            .inner
            .detector_permits
            .clone()
            .acquire_many_owned(DETECTOR_CONCURRENCY_LIMIT as u32)
            .await
            .unwrap();
        let alpha = {
            let coordinator = coordinator.clone();
            tokio::spawn(async move {
                coordinator
                    .check(
                        request_for_path("acme/tools", EvidenceCheckMode::Force, "skills/alpha"),
                        CancellationSignal::default(),
                    )
                    .await
            })
        };
        wait_for_batch_state(&coordinator, |batch| {
            batch.collecting_skill_paths.contains("skills/alpha")
        })
        .await;
        let beta = {
            let coordinator = coordinator.clone();
            tokio::spawn(async move {
                coordinator
                    .check(
                        request_for_path("acme/tools", EvidenceCheckMode::Force, "skills/beta"),
                        CancellationSignal::default(),
                    )
                    .await
            })
        };
        wait_for_batch_state(&coordinator, |batch| {
            batch.collecting_skill_paths
                == BTreeSet::from(["skills/alpha".to_string(), "skills/beta".to_string()])
        })
        .await;
        drop(held_permits);
        let (_, paths) = started.recv().await.expect("sealed batch");
        assert_eq!(
            paths,
            BTreeSet::from(["skills/alpha".to_string(), "skills/beta".to_string(),])
        );
        detector.release_one();

        alpha.await.unwrap().unwrap();
        beta.await.unwrap().unwrap();
        assert_eq!(detector.calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn consecutive_running_tail_windows_are_promoted_without_losing_coverage() {
        let (detector, mut started) = ControlledBatchDetector::new(BTreeSet::new());
        let coordinator = Arc::new(coordinator(
            detector.clone(),
            Arc::new(AtomicU64::new(1_000)),
        ));
        let alpha = {
            let coordinator = coordinator.clone();
            tokio::spawn(async move {
                coordinator
                    .check(
                        request_for_path("acme/tools", EvidenceCheckMode::Force, "skills/alpha"),
                        CancellationSignal::default(),
                    )
                    .await
            })
        };
        let (call, paths) = started.recv().await.expect("first batch");
        assert_eq!(call, 0);
        assert_eq!(paths, BTreeSet::from(["skills/alpha".to_string()]));
        let beta = {
            let coordinator = coordinator.clone();
            tokio::spawn(async move {
                coordinator
                    .check(
                        request_for_path("acme/tools", EvidenceCheckMode::Force, "skills/beta"),
                        CancellationSignal::default(),
                    )
                    .await
            })
        };
        wait_for_batch_state(&coordinator, |batch| {
            batch.pending_skill_paths.contains("skills/beta")
        })
        .await;
        detector.release_one();
        let (call, paths) = started.recv().await.expect("second batch");
        assert_eq!(call, 1);
        assert_eq!(paths, BTreeSet::from(["skills/beta".to_string()]));
        let gamma = {
            let coordinator = coordinator.clone();
            tokio::spawn(async move {
                coordinator
                    .check(
                        request_for_path("acme/tools", EvidenceCheckMode::Force, "skills/gamma"),
                        CancellationSignal::default(),
                    )
                    .await
            })
        };
        wait_for_batch_state(&coordinator, |batch| {
            batch.pending_skill_paths.contains("skills/gamma")
        })
        .await;
        detector.release_one();
        let (call, paths) = started.recv().await.expect("third batch");
        assert_eq!(call, 2);
        assert_eq!(paths, BTreeSet::from(["skills/gamma".to_string()]));
        detector.release_one();

        let alpha = alpha.await.unwrap().unwrap().evidence.unwrap();
        let beta = beta.await.unwrap().unwrap().evidence.unwrap();
        let gamma = gamma.await.unwrap().unwrap().evidence.unwrap();
        assert!(alpha.skill_revisions.contains_key("skills/alpha"));
        assert!(beta.skill_revisions.contains_key("skills/beta"));
        assert!(gamma.skill_revisions.contains_key("skills/gamma"));
        assert_eq!(detector.calls.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn catalog_presence_without_revision_keeps_the_waiter_pending() {
        let detector = Arc::new(ScriptedDetector::new([
            modified("revision-1", []),
            modified(
                "revision-1",
                [("skills/beta", SkillRevision::GitTreeOid("tree-beta".into()))],
            ),
        ]));
        let coordinator = coordinator(detector.clone(), Arc::new(AtomicU64::new(1_000)));

        let result = coordinator
            .check(
                request_for_path("acme/tools", EvidenceCheckMode::Force, "skills/beta"),
                CancellationSignal::default(),
            )
            .await
            .unwrap();

        assert!(result
            .evidence
            .unwrap()
            .skill_revisions
            .contains_key("skills/beta"));
        assert_eq!(detector.calls(), 2);
    }

    #[tokio::test]
    async fn path_absent_from_a_complete_catalog_completes_without_revision() {
        let detector = Arc::new(ScriptedDetector::new([EvidenceDetectionOutcome::Modified(
            RemoteEvidenceObservation {
                snapshot_id: RemoteSnapshotId::new(
                    NormalizedRef::Named("main".to_string()),
                    "refs/heads/main",
                    "revision-1",
                ),
                provider_validation: None,
                complete_skill_path_catalog: BTreeSet::from(["skills/alpha".to_string()]),
                skill_revisions: BTreeMap::new(),
                snapshot_facts: None,
            },
        )]));
        let coordinator = coordinator(detector.clone(), Arc::new(AtomicU64::new(1_000)));

        let result = coordinator
            .check(
                request_for_path("acme/tools", EvidenceCheckMode::Force, "skills/missing"),
                CancellationSignal::default(),
            )
            .await
            .unwrap();

        assert!(!result
            .evidence
            .unwrap()
            .complete_skill_path_catalog
            .contains("skills/missing"));
        assert_eq!(detector.calls(), 1);
    }

    #[tokio::test]
    async fn failed_batch_discards_pending_paths_that_already_received_the_failure() {
        let (detector, mut started) = ControlledBatchDetector::new(BTreeSet::from([0]));
        let coordinator = Arc::new(coordinator(
            detector.clone(),
            Arc::new(AtomicU64::new(1_000)),
        ));
        let alpha = {
            let coordinator = coordinator.clone();
            tokio::spawn(async move {
                coordinator
                    .check(
                        request_for_path("acme/tools", EvidenceCheckMode::Force, "skills/alpha"),
                        CancellationSignal::default(),
                    )
                    .await
            })
        };
        let (_, paths) = started.recv().await.expect("failed batch");
        assert_eq!(paths, BTreeSet::from(["skills/alpha".to_string()]));
        let beta = {
            let coordinator = coordinator.clone();
            tokio::spawn(async move {
                coordinator
                    .check(
                        request_for_path("acme/tools", EvidenceCheckMode::Force, "skills/beta"),
                        CancellationSignal::default(),
                    )
                    .await
            })
        };
        wait_for_batch_state(&coordinator, |batch| {
            batch.pending_skill_paths.contains("skills/beta")
        })
        .await;
        detector.release_one();
        assert!(alpha
            .await
            .unwrap()
            .unwrap()
            .last_attempt
            .unwrap()
            .failure
            .is_some());
        assert!(beta
            .await
            .unwrap()
            .unwrap()
            .last_attempt
            .unwrap()
            .failure
            .is_some());

        let gamma = {
            let coordinator = coordinator.clone();
            tokio::spawn(async move {
                coordinator
                    .check(
                        request_for_path("acme/tools", EvidenceCheckMode::Force, "skills/gamma"),
                        CancellationSignal::default(),
                    )
                    .await
            })
        };
        let (call, paths) = started.recv().await.expect("next batch");
        assert_eq!(call, 1);
        assert_eq!(paths, BTreeSet::from(["skills/gamma".to_string()]));
        detector.release_one();
        gamma.await.unwrap().unwrap();
        assert_eq!(detector.calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn coordinator_registers_detector_snapshot_facts_for_reuse() {
        let snapshots = Arc::new(SourceSnapshotReuseIndex::with_clock(|| 1_000));
        let detector = Arc::new(ScriptedDetector::new([EvidenceDetectionOutcome::Modified(
            RemoteEvidenceObservation {
                snapshot_id: RemoteSnapshotId::new(
                    NormalizedRef::Named("main".to_string()),
                    "refs/heads/main",
                    "revision-1",
                ),
                provider_validation: None,
                complete_skill_path_catalog: BTreeSet::from(["skills/alpha".to_string()]),
                skill_revisions: BTreeMap::from([(
                    "skills/alpha".to_string(),
                    SkillRevision::CliContentHash("hash-a".to_string()),
                )]),
                snapshot_facts: Some(SourceSnapshotFacts {
                    discovery_session: DiscoverySessionHandle {
                        session_id: "session-1".to_string(),
                        environment: EnvironmentRef::Native,
                        source_fingerprint: "source-1".to_string(),
                        expires_at_epoch_ms: 60_000,
                    },
                    snapshot_id: RemoteSnapshotId::new(
                        NormalizedRef::Named("main".to_string()),
                        "refs/heads/main",
                        "revision-1",
                    ),
                    complete_skill_path_catalog: BTreeSet::from(["skills/alpha".to_string()]),
                }),
            },
        )]));
        let coordinator = SourceEvidenceCoordinator::with_clock_and_snapshots(
            detector,
            snapshots.clone(),
            || 1_000,
        );

        coordinator
            .check(
                request("acme/tools", EvidenceCheckMode::Force),
                CancellationSignal::default(),
            )
            .await
            .unwrap();

        assert_eq!(snapshots.len(), 1);
    }

    #[tokio::test]
    async fn acquisition_refreshes_evidence_without_running_detector() {
        let detector = Arc::new(ScriptedDetector::new([]));
        let coordinator = coordinator(detector.clone(), Arc::new(AtomicU64::new(1_000)));
        let identity = SourceIdentity::from_parsed(
            &parse_source("https://github.com/acme/tools#main").unwrap(),
        )
        .unwrap();
        coordinator
            .record_acquisition(
                RemoteEvidenceKey::from_identity(&identity),
                PayloadAcquisitionKey::from_identity(&identity, &EnvironmentRef::Native),
                SourceSnapshotFacts {
                    discovery_session: DiscoverySessionHandle {
                        session_id: "session-1".to_string(),
                        environment: EnvironmentRef::Native,
                        source_fingerprint: "source-1".to_string(),
                        expires_at_epoch_ms: 60_000,
                    },
                    snapshot_id: RemoteSnapshotId::new(
                        NormalizedRef::Named("main".to_string()),
                        "main",
                        "revision-1",
                    ),
                    complete_skill_path_catalog: BTreeSet::from(["skills/alpha".to_string()]),
                },
                BTreeMap::from([(
                    "skills/alpha".to_string(),
                    SkillRevision::CliContentHash("hash-a".to_string()),
                )]),
            )
            .unwrap();

        let result = coordinator
            .check(
                request("acme/tools", EvidenceCheckMode::Automatic),
                CancellationSignal::default(),
            )
            .await
            .unwrap();

        assert_eq!(result.freshness, EvidenceFreshness::Cached);
        assert_eq!(detector.calls(), 0);
    }

    #[tokio::test]
    async fn acquisition_merges_sparse_revisions_only_for_the_same_snapshot() {
        let detector = Arc::new(ScriptedDetector::new([]));
        let coordinator = coordinator(detector.clone(), Arc::new(AtomicU64::new(1_000)));
        let identity = SourceIdentity::from_parsed(
            &parse_source("https://github.com/acme/tools#main").unwrap(),
        )
        .unwrap();
        let evidence_key = RemoteEvidenceKey::from_identity(&identity);
        let acquisition_key =
            PayloadAcquisitionKey::from_identity(&identity, &EnvironmentRef::Native);
        let facts = |session_id: &str,
                     commit_revision: &str,
                     catalog: BTreeSet<String>|
         -> SourceSnapshotFacts {
            SourceSnapshotFacts {
                discovery_session: DiscoverySessionHandle {
                    session_id: session_id.to_string(),
                    environment: EnvironmentRef::Native,
                    source_fingerprint: format!("source-{session_id}"),
                    expires_at_epoch_ms: 60_000,
                },
                snapshot_id: RemoteSnapshotId::new(
                    NormalizedRef::Named("main".to_string()),
                    "main",
                    commit_revision,
                ),
                complete_skill_path_catalog: catalog,
            }
        };
        let full_catalog = BTreeSet::from(["skills/alpha".to_string(), "skills/beta".to_string()]);

        coordinator
            .record_acquisition(
                evidence_key.clone(),
                acquisition_key.clone(),
                facts("1", "revision-1", full_catalog.clone()),
                BTreeMap::from([(
                    "skills/alpha".to_string(),
                    SkillRevision::CliContentHash("hash-a".to_string()),
                )]),
            )
            .unwrap();
        coordinator
            .record_acquisition(
                evidence_key.clone(),
                acquisition_key.clone(),
                facts("2", "revision-1", full_catalog),
                BTreeMap::from([(
                    "skills/beta".to_string(),
                    SkillRevision::CliContentHash("hash-b".to_string()),
                )]),
            )
            .unwrap();
        let merged = coordinator
            .check(
                request("acme/tools", EvidenceCheckMode::Automatic),
                CancellationSignal::default(),
            )
            .await
            .unwrap()
            .evidence
            .unwrap();
        assert_eq!(merged.skill_revisions.len(), 2);

        coordinator
            .record_acquisition(
                evidence_key,
                acquisition_key,
                facts(
                    "3",
                    "revision-2",
                    BTreeSet::from(["skills/beta".to_string()]),
                ),
                BTreeMap::from([(
                    "skills/beta".to_string(),
                    SkillRevision::CliContentHash("hash-b2".to_string()),
                )]),
            )
            .unwrap();
        let pruned = coordinator
            .check(
                request("acme/tools", EvidenceCheckMode::Automatic),
                CancellationSignal::default(),
            )
            .await
            .unwrap()
            .evidence
            .unwrap();
        assert_eq!(
            pruned.skill_revisions,
            BTreeMap::from([(
                "skills/beta".to_string(),
                SkillRevision::CliContentHash("hash-b2".to_string()),
            )])
        );
        assert_eq!(detector.calls(), 0);
    }

    #[test]
    fn provider_cooldown_never_shrinks_across_repositories() {
        let mut state = CoordinatorState::default();
        let environment = EnvironmentRef::Native;
        let throttle = EnvironmentThrottleKey::new(&environment, &throttle_key());
        let alpha = EnvironmentEvidenceKey::new(&environment, &key("acme/alpha"));
        let beta = EnvironmentEvidenceKey::new(&environment, &key("acme/beta"));
        record_failure(
            &mut state,
            &throttle,
            &alpha,
            1_000,
            EvidenceDetectionFailure {
                reason: EvidenceFailureReason::RateLimited,
                message: "long cooldown".to_string(),
                retry_at_epoch_ms: Some(5_000),
                provider_cooldown: true,
            },
        );
        record_failure(
            &mut state,
            &throttle,
            &beta,
            1_100,
            EvidenceDetectionFailure {
                reason: EvidenceFailureReason::RateLimited,
                message: "short cooldown".to_string(),
                retry_at_epoch_ms: Some(3_000),
                provider_cooldown: true,
            },
        );

        assert_eq!(state.provider_cooldowns.get(&throttle), Some(&5_000));
    }
}

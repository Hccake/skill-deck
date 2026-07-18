use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex, MutexGuard};

use serde::Serialize;
use specta::Type;
use tokio::sync::{watch, Semaphore};

use crate::application::payload_session::DiscoverySessionHandle;
use crate::application::source_snapshot_reuse::{PayloadAcquisitionKey, SourceSnapshotReuseIndex};
use crate::core::mutation::CancellationSignal;
use crate::core::source_identity::{
    AcquisitionDescriptor, AcquisitionTransportIdentity, NormalizedRef, RemoteSourceIdentity,
    SourceIdentity, SourceProvider,
};
use crate::error::AppError;

pub const EVIDENCE_TTL_MS: u64 = 15 * 60 * 1_000;
pub const DETECTOR_CONCURRENCY_LIMIT: usize = 4;
const NETWORK_BACKOFF_BASE_MS: u64 = 30_000;
const NETWORK_BACKOFF_MAX_MS: u64 = 5 * 60 * 1_000;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
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

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
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
    pub key: RemoteEvidenceKey,
    pub throttle_key: ProviderThrottleKey,
    pub mode: EvidenceCheckMode,
    pub requested_skill_paths: BTreeSet<String>,
    pub acquisition: Arc<AcquisitionDescriptor>,
    pub acquisition_transport_identity: AcquisitionTransportIdentity,
}

#[derive(Clone)]
pub struct EvidenceDetectionRequest {
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
}

type DetectionCompletion = Result<(), AppError>;
type SealedDetectionBatch = (BTreeSet<String>, Option<RemoteEvidenceEntry>);

#[derive(Default)]
struct CoordinatorState {
    evidence: HashMap<RemoteEvidenceKey, RemoteEvidenceEntry>,
    attempts: HashMap<RemoteEvidenceKey, EvidenceAttempt>,
    in_flight: HashMap<RemoteEvidenceKey, InFlightDetection>,
    network_backoff: HashMap<RemoteEvidenceKey, u64>,
    network_failure_counts: HashMap<RemoteEvidenceKey, u32>,
    provider_cooldowns: HashMap<ProviderThrottleKey, u64>,
    next_batch_id: u64,
}

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
    #[cfg(any(test, feature = "wsl-integration-tests"))]
    #[allow(dead_code, reason = "used by the feature-gated WSL acceptance harness")]
    pub fn new(detector: Arc<dyn SourceEvidenceDetector>) -> Self {
        Self::build(detector, None, || {
            chrono::Utc::now().timestamp_millis().max(0) as u64
        })
    }

    pub fn with_snapshot_reuse(
        detector: Arc<dyn SourceEvidenceDetector>,
        snapshots: Arc<SourceSnapshotReuseIndex>,
    ) -> Self {
        Self::build(detector, Some(snapshots), || {
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
            }),
        }
    }

    #[cfg(test)]
    pub fn record_provider_cooldown(&self, key: ProviderThrottleKey, deadline_epoch_ms: u64) {
        if let Ok(mut state) = self.inner.state.lock() {
            state
                .provider_cooldowns
                .entry(key)
                .and_modify(|deadline| *deadline = (*deadline).max(deadline_epoch_ms))
                .or_insert(deadline_epoch_ms);
        }
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
        state.attempts.insert(
            key.clone(),
            EvidenceAttempt {
                checked_at_epoch_ms: checked_at,
                failure: None,
            },
        );
        state.network_backoff.remove(&key);
        state.network_failure_counts.remove(&key);
        drop(state);
        if let Some(snapshots) = &self.inner.snapshots {
            snapshots.remember(
                acquisition_key,
                facts.snapshot_id.commit_revision,
                facts.discovery_session,
            );
        }
        Ok(())
    }

    pub async fn check(
        &self,
        request: EvidenceCheckRequest,
        cancellation: CancellationSignal,
    ) -> Result<EvidenceCheckResult, AppError> {
        let key = request.key.clone();
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

                if state.provider_cooldowns.contains_key(&request.throttle_key) {
                    return Ok(result_from_state(
                        &state,
                        &request.key,
                        EvidenceFreshness::CoolingDown,
                        now,
                    ));
                }

                if request.mode == EvidenceCheckMode::Automatic {
                    if let Some(evidence) = state.evidence.get(&request.key) {
                        if evidence.expires_at_epoch_ms >= now
                            && evidence_covers_requested_paths(evidence, &requested_skill_paths)
                        {
                            return Ok(result_from_state(
                                &state,
                                &request.key,
                                EvidenceFreshness::Cached,
                                now,
                            ));
                        }
                    }
                }
                if state.network_backoff.contains_key(&request.key) {
                    return Ok(result_from_state(
                        &state,
                        &request.key,
                        EvidenceFreshness::BackingOff,
                        now,
                    ));
                }

                let completed = state
                    .in_flight
                    .get(&request.key)
                    .is_some_and(|batch| batch.receiver.borrow().is_some());
                if completed {
                    let previous = state
                        .in_flight
                        .remove(&request.key)
                        .expect("completed batch must still be registered");
                    let mut paths = previous.pending_skill_paths;
                    if let Some(evidence) = state.evidence.get(&request.key) {
                        paths.retain(|path| !evidence_covers_path(evidence, path));
                    }
                    paths.extend(requested_skill_paths.clone());
                    let (batch_id, sender, receiver) =
                        register_detection_batch(&mut state, &request, paths);
                    spawned = Some((batch_id, sender));
                    receiver
                } else if let Some(in_flight) = state.in_flight.get_mut(&request.key) {
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
                tokio::spawn(async move {
                    run_detection(inner, detection_request, batch_id, sender).await;
                });
            }

            loop {
                if let Some(completion) = receiver.borrow().clone() {
                    completion?;
                    let now = (self.inner.now)();
                    let state = state(&self.inner)?;
                    let freshness = freshness_after_attempt(&state, &key, now);
                    let result = result_from_state(&state, &key, freshness, now);
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
        request.key.clone(),
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
    batch_id: u64,
    sender: watch::Sender<Option<DetectionCompletion>>,
) {
    let key = request.key.clone();
    let acquisition_transport_identity = request.acquisition_transport_identity.clone();
    let completion = match inner.detector_permits.clone().acquire_owned().await {
        Ok(_permit) => match seal_detection_batch(&inner, &key, batch_id) {
            Ok(Some((requested_skill_paths, previous))) => {
                let outcome = inner
                    .detector
                    .detect(
                        EvidenceDetectionRequest {
                            key: key.clone(),
                            requested_skill_paths,
                            acquisition: request.acquisition,
                            acquisition_transport_identity: request.acquisition_transport_identity,
                        },
                        previous,
                        CancellationSignal::default(),
                    )
                    .await;
                finish_detection(
                    &inner,
                    &request.throttle_key,
                    &key,
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

fn seal_detection_batch(
    inner: &SourceEvidenceCoordinatorInner,
    key: &RemoteEvidenceKey,
    batch_id: u64,
) -> Result<Option<SealedDetectionBatch>, AppError> {
    let mut state = state(inner)?;
    let Some(batch) = state.in_flight.get_mut(key) else {
        return Ok(None);
    };
    if batch.batch_id != batch_id || batch.running_skill_paths.is_some() {
        return Ok(None);
    }
    let paths = std::mem::take(&mut batch.collecting_skill_paths);
    batch.running_skill_paths = Some(paths.clone());
    let previous = state.evidence.get(key).cloned();
    Ok(Some((paths, previous)))
}

fn finish_detection(
    inner: &SourceEvidenceCoordinatorInner,
    throttle_key: &ProviderThrottleKey,
    key: &RemoteEvidenceKey,
    acquisition_transport_identity: &AcquisitionTransportIdentity,
    outcome: Result<EvidenceDetectionOutcome, AppError>,
) -> DetectionCompletion {
    let checked_at = (inner.now)();
    let mut state = state(inner)?;
    match outcome {
        Ok(EvidenceDetectionOutcome::Modified(observation)) => {
            if observation.snapshot_id.requested_ref != key.normalized_ref {
                record_failure(
                    &mut state,
                    throttle_key,
                    key,
                    checked_at,
                    EvidenceDetectionFailure::incomplete(
                        "provider evidence does not match the requested ref",
                    ),
                );
                discard_pending_paths(&mut state, key);
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
                        key,
                        checked_at,
                        EvidenceDetectionFailure::incomplete(
                            "snapshot facts do not match provider evidence",
                        ),
                    );
                    discard_pending_paths(&mut state, key);
                    return Ok(());
                }
            }
            let skill_revisions = match state.evidence.get(key) {
                Some(previous) if previous.snapshot_id == observation.snapshot_id => {
                    let mut merged = previous.skill_revisions.clone();
                    merged.retain(|path, _| observation.complete_skill_path_catalog.contains(path));
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
            state.network_backoff.remove(key);
            state.network_failure_counts.remove(key);
            state.attempts.insert(
                key.clone(),
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
                state.network_backoff.remove(key);
                state.network_failure_counts.remove(key);
                state.attempts.insert(
                    key.clone(),
                    EvidenceAttempt {
                        checked_at_epoch_ms: checked_at,
                        failure: None,
                    },
                );
            } else {
                record_failure(
                    &mut state,
                    throttle_key,
                    key,
                    checked_at,
                    EvidenceDetectionFailure::incomplete(
                        "provider returned not-modified without cached evidence",
                    ),
                );
                discard_pending_paths(&mut state, key);
            }
        }
        Ok(EvidenceDetectionOutcome::Failed(failure)) => {
            record_failure(&mut state, throttle_key, key, checked_at, failure);
            discard_pending_paths(&mut state, key);
        }
        Err(error) => {
            record_failure(
                &mut state,
                throttle_key,
                key,
                checked_at,
                EvidenceDetectionFailure {
                    reason: EvidenceFailureReason::SourceUnavailable,
                    message: error.to_string(),
                    retry_at_epoch_ms: None,
                    provider_cooldown: false,
                },
            );
            discard_pending_paths(&mut state, key);
        }
    }
    Ok(())
}

fn discard_pending_paths(state: &mut CoordinatorState, key: &RemoteEvidenceKey) {
    if let Some(batch) = state.in_flight.get_mut(key) {
        batch.pending_skill_paths.clear();
    }
}

fn record_failure(
    state: &mut CoordinatorState,
    throttle_key: &ProviderThrottleKey,
    key: &RemoteEvidenceKey,
    checked_at: u64,
    mut failure: EvidenceDetectionFailure,
) {
    if failure.reason == EvidenceFailureReason::Network {
        let failures = state
            .network_failure_counts
            .entry(key.clone())
            .and_modify(|failures| *failures = failures.saturating_add(1))
            .or_insert(1);
        let multiplier = 1_u64
            .checked_shl(failures.saturating_sub(1).min(31))
            .unwrap_or(u64::MAX);
        let delay = NETWORK_BACKOFF_BASE_MS
            .saturating_mul(multiplier)
            .min(NETWORK_BACKOFF_MAX_MS);
        let deadline = checked_at.saturating_add(delay);
        failure.retry_at_epoch_ms = Some(deadline);
        state.network_backoff.insert(key.clone(), deadline);
    } else if let Some(deadline) = failure.retry_at_epoch_ms {
        if failure.provider_cooldown {
            state
                .provider_cooldowns
                .entry(throttle_key.clone())
                .and_modify(|existing| *existing = (*existing).max(deadline))
                .or_insert(deadline);
        } else {
            state.network_backoff.insert(key.clone(), deadline);
        }
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
    key: &RemoteEvidenceKey,
    requested_freshness: EvidenceFreshness,
    now: u64,
) -> EvidenceCheckResult {
    let evidence = state.evidence.get(key).cloned();
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
        last_attempt: state.attempts.get(key).cloned(),
    }
}

fn freshness_after_attempt(
    state: &CoordinatorState,
    key: &RemoteEvidenceKey,
    now: u64,
) -> EvidenceFreshness {
    match state.attempts.get(key) {
        Some(attempt) if attempt.failure.is_none() => EvidenceFreshness::Fresh,
        _ => match state.evidence.get(key) {
            Some(evidence) if evidence.expires_at_epoch_ms >= now => EvidenceFreshness::Cached,
            Some(_) => EvidenceFreshness::Stale,
            None => EvidenceFreshness::Unavailable,
        },
    }
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
        EvidenceCheckRequest {
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

    #[tokio::test]
    async fn automatic_reuses_successful_evidence_for_fifteen_minutes() {
        let detector = Arc::new(ScriptedDetector::new([modified(
            "revision-1",
            [("skills/alpha", SkillRevision::GitTreeOid("tree-a".into()))],
        )]));
        let now = Arc::new(AtomicU64::new(1_000));
        let coordinator = coordinator(detector.clone(), now.clone());

        let first = coordinator
            .check(
                request("acme/tools", EvidenceCheckMode::Automatic),
                CancellationSignal::default(),
            )
            .await
            .unwrap();
        now.store(1_000 + EVIDENCE_TTL_MS - 1, Ordering::SeqCst);
        let cached = coordinator
            .check(
                request("acme/tools", EvidenceCheckMode::Automatic),
                CancellationSignal::default(),
            )
            .await
            .unwrap();

        assert_eq!(first.freshness, EvidenceFreshness::Fresh);
        assert_eq!(cached.freshness, EvidenceFreshness::Cached);
        assert_eq!(detector.calls(), 1);
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
        coordinator.record_provider_cooldown(throttle_key(), 2_000);

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
    async fn force_respects_network_backoff() {
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
                request("acme/tools", EvidenceCheckMode::Force),
                CancellationSignal::default(),
            )
            .await
            .unwrap();

        let backed_off = coordinator
            .check(
                request("acme/tools", EvidenceCheckMode::Force),
                CancellationSignal::default(),
            )
            .await
            .unwrap();

        assert_eq!(backed_off.freshness, EvidenceFreshness::BackingOff);
        assert_eq!(detector.calls(), 1);
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
                request("acme/tools", EvidenceCheckMode::Force),
                CancellationSignal::default(),
            )
            .await
            .unwrap();
        assert_eq!(first.freshness, EvidenceFreshness::Unavailable);
        assert_eq!(detector.calls(), 1);

        now.store(30_999, Ordering::SeqCst);
        let backing_off = coordinator
            .check(
                request("acme/tools", EvidenceCheckMode::Force),
                CancellationSignal::default(),
            )
            .await
            .unwrap();
        assert_eq!(backing_off.freshness, EvidenceFreshness::BackingOff);
        assert_eq!(detector.calls(), 1);

        now.store(31_000, Ordering::SeqCst);
        coordinator
            .check(
                request("acme/tools", EvidenceCheckMode::Force),
                CancellationSignal::default(),
            )
            .await
            .unwrap();
        assert_eq!(detector.calls(), 2);

        now.store(90_999, Ordering::SeqCst);
        let backing_off = coordinator
            .check(
                request("acme/tools", EvidenceCheckMode::Force),
                CancellationSignal::default(),
            )
            .await
            .unwrap();
        assert_eq!(backing_off.freshness, EvidenceFreshness::BackingOff);
        assert_eq!(detector.calls(), 2);

        now.store(91_000, Ordering::SeqCst);
        coordinator
            .check(
                request("acme/tools", EvidenceCheckMode::Force),
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

        now.store(121_000, Ordering::SeqCst);
        let backing_off = coordinator
            .check(
                request("acme/tools", EvidenceCheckMode::Force),
                CancellationSignal::default(),
            )
            .await
            .unwrap();
        assert_eq!(backing_off.freshness, EvidenceFreshness::BackingOff);
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
                        environment: EnvironmentRef::Host,
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
                PayloadAcquisitionKey::from_identity(&identity, &EnvironmentRef::Host),
                SourceSnapshotFacts {
                    discovery_session: DiscoverySessionHandle {
                        session_id: "session-1".to_string(),
                        environment: EnvironmentRef::Host,
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
            PayloadAcquisitionKey::from_identity(&identity, &EnvironmentRef::Host);
        let facts = |session_id: &str,
                     commit_revision: &str,
                     catalog: BTreeSet<String>|
         -> SourceSnapshotFacts {
            SourceSnapshotFacts {
                discovery_session: DiscoverySessionHandle {
                    session_id: session_id.to_string(),
                    environment: EnvironmentRef::Host,
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
        let throttle = throttle_key();
        record_failure(
            &mut state,
            &throttle,
            &key("acme/alpha"),
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
            &key("acme/beta"),
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

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use crate::application::git_transport::GitSourceTransport;
use crate::application::github_access::GithubTreeAccess;
use crate::application::payload_session::{DiscoverySourceLocation, PayloadSessionManager};
use crate::application::source_acquisition::{
    AcquireSelectedPayloadsRequest, GitSourceDiscovery, SelectedPayloadAcquisitionService,
};
use crate::application::source_evidence::{
    EvidenceDetectionFailure, EvidenceDetectionOutcome, EvidenceDetectionRequest,
    EvidenceFailureReason, EvidenceFuture, RemoteEvidenceEntry, RemoteEvidenceObservation,
    RemoteSnapshotId, SkillRevision, SourceEvidenceDetector, SourceSnapshotFacts,
};
use crate::application::source_snapshot_reuse::{PayloadAcquisitionKey, SourceSnapshotReuseIndex};
#[cfg(test)]
use crate::application::wsl_source_access::UnavailableWslSourceAccess;
use crate::application::wsl_source_access::WslSourceAccess;
use crate::core::mutation::CancellationSignal;
use crate::core::skill_paths::normalize_skill_folder_path;
use crate::core::source_identity::{NormalizedRef, SourceProvider};
use crate::core::{GithubTreeFailure, GithubTreeFetchOutcome};
use crate::environment::types::{EnvironmentRef, SkillLocation, SkillLocationRef};
use crate::error::AppError;
#[cfg(test)]
use crate::models::{ParsedSource, SourceType};

pub struct RuntimeSourceEvidenceDetector {
    payloads: Arc<PayloadSessionManager>,
    snapshots: Arc<SourceSnapshotReuseIndex>,
    github: Arc<dyn GithubTreeAccess>,
    git_transport: Arc<dyn GitSourceTransport>,
    wsl_source: Arc<dyn WslSourceAccess>,
}

impl RuntimeSourceEvidenceDetector {
    pub fn new(
        payloads: Arc<PayloadSessionManager>,
        snapshots: Arc<SourceSnapshotReuseIndex>,
        github: Arc<dyn GithubTreeAccess>,
        git_transport: Arc<dyn GitSourceTransport>,
        wsl_source: Arc<dyn WslSourceAccess>,
    ) -> Self {
        Self {
            payloads,
            snapshots,
            github,
            git_transport,
            wsl_source,
        }
    }

    #[cfg(test)]
    pub(crate) fn with_git_transport(
        payloads: Arc<PayloadSessionManager>,
        snapshots: Arc<SourceSnapshotReuseIndex>,
        git_transport: Arc<dyn GitSourceTransport>,
    ) -> Self {
        Self {
            payloads,
            snapshots,
            github: Arc::new(UnavailableGithubAccess),
            git_transport,
            wsl_source: Arc::new(UnavailableWslSourceAccess),
        }
    }

    #[cfg(test)]
    fn with_github_access(
        payloads: Arc<PayloadSessionManager>,
        snapshots: Arc<SourceSnapshotReuseIndex>,
        github: Arc<dyn GithubTreeAccess>,
    ) -> Self {
        Self {
            payloads,
            snapshots,
            github,
            git_transport: Arc::new(
                crate::application::git_transport::UnavailableGitSourceTransport,
            ),
            wsl_source: Arc::new(UnavailableWslSourceAccess),
        }
    }

    async fn detect_github(
        &self,
        request: EvidenceDetectionRequest,
        previous: Option<RemoteEvidenceEntry>,
    ) -> Result<EvidenceDetectionOutcome, AppError> {
        let requested_ref = resolved_ref(&request.key.normalized_ref);
        let validation = previous
            .as_ref()
            .filter(|entry| evidence_covers_requested_paths(entry, &request.requested_skill_paths))
            .and_then(|entry| entry.provider_validation.as_deref());
        let mut outcome = self
            .github
            .fetch_tree(request.key.remote.repository(), &requested_ref, validation)
            .await;
        if matches!(outcome, GithubTreeFetchOutcome::NotModified) && previous.is_none() {
            outcome = self
                .github
                .fetch_tree(request.key.remote.repository(), &requested_ref, None)
                .await;
        }
        Ok(match outcome {
            GithubTreeFetchOutcome::Modified(snapshot) => {
                let catalog = snapshot
                    .entries
                    .iter()
                    .filter(|entry| {
                        entry.entry_type == "blob"
                            && entry
                                .path
                                .rsplit('/')
                                .next()
                                .is_some_and(|name| name.eq_ignore_ascii_case("SKILL.md"))
                    })
                    .map(|entry| normalize_skill_folder_path(&entry.path))
                    .collect::<BTreeSet<_>>();
                let requested = request.requested_skill_paths;
                let tree_revisions = snapshot
                    .entries
                    .iter()
                    .filter(|entry| entry.entry_type == "tree")
                    .map(|entry| {
                        (
                            normalize_skill_folder_path(&entry.path),
                            entry.revision.clone(),
                        )
                    })
                    .collect::<BTreeMap<_, _>>();
                let skill_revisions = requested
                    .into_iter()
                    .filter_map(|path| {
                        (if path.is_empty() {
                            Some(snapshot.root_tree_revision.clone())
                        } else {
                            tree_revisions.get(&path).cloned()
                        })
                        .map(|revision| (path, SkillRevision::GitTreeOid(revision)))
                    })
                    .collect();
                EvidenceDetectionOutcome::Modified(RemoteEvidenceObservation {
                    snapshot_id: RemoteSnapshotId::new(
                        request.key.normalized_ref,
                        requested_ref,
                        snapshot.ref_revision,
                    ),
                    provider_validation: snapshot.validation,
                    complete_skill_path_catalog: catalog,
                    skill_revisions,
                    snapshot_facts: None,
                })
            }
            GithubTreeFetchOutcome::NotModified => match previous {
                Some(_) => EvidenceDetectionOutcome::NotModified,
                None => failure(
                    EvidenceFailureReason::IncompleteEvidence,
                    "incomplete GitHub tree",
                    None,
                    false,
                ),
            },
            GithubTreeFetchOutcome::Incomplete => failure(
                EvidenceFailureReason::IncompleteEvidence,
                "incomplete GitHub tree",
                None,
                false,
            ),
            GithubTreeFetchOutcome::RateLimited { retry_at_epoch_ms } => failure(
                EvidenceFailureReason::RateLimited,
                "GitHub rate limit reached",
                retry_at_epoch_ms,
                true,
            ),
            GithubTreeFetchOutcome::Failed(reason) => {
                let reason = match reason {
                    GithubTreeFailure::AuthenticationRequired => {
                        EvidenceFailureReason::AuthenticationRequired
                    }
                    GithubTreeFailure::NotFoundOrUnauthorized => {
                        EvidenceFailureReason::NotFoundOrUnauthorized
                    }
                    GithubTreeFailure::Network => EvidenceFailureReason::Network,
                    GithubTreeFailure::SourceUnavailable => {
                        EvidenceFailureReason::SourceUnavailable
                    }
                };
                failure(reason, "GitHub tree request failed", None, false)
            }
        })
    }

    async fn detect_by_clone(
        &self,
        request: EvidenceDetectionRequest,
        previous: Option<RemoteEvidenceEntry>,
        cancellation: CancellationSignal,
    ) -> Result<EvidenceDetectionOutcome, AppError> {
        let source = request.acquisition.source().to_string();
        let provider = request.key.remote.provider().clone();
        let parsed = request.acquisition.parsed_source(&provider);
        let environment = request.environment.clone();
        let acquisition_key = PayloadAcquisitionKey::new(
            request.acquisition_transport_identity.clone(),
            request.key.normalized_ref.clone(),
            &environment,
        );
        let reusable = if previous.is_some() && matches!(&environment, EnvironmentRef::Native) {
            let probe_source = source.clone();
            let probe_ref = request.acquisition.git_ref().map(ToString::to_string);
            let probe_cancellation = cancellation.clone();
            let git_transport = Arc::clone(&self.git_transport);
            let probed = tokio::task::spawn_blocking(move || {
                git_transport.probe_ref_revision(
                    &probe_source,
                    probe_ref.as_deref(),
                    probe_cancellation,
                )
            })
            .await
            .map_err(|error| AppError::ExecutionFailed {
                message: format!("Git ref probe task failed: {error}"),
            })?;
            match probed {
                Ok(ref_revision) => self
                    .snapshots
                    .find(&acquisition_key, &ref_revision, self.payloads.as_ref())
                    .map(|discovery| (discovery, ref_revision)),
                Err(error) => {
                    return Ok(git_failure(error));
                }
            }
        } else {
            None
        };

        let (discovery_session, ref_revision) = match reusable {
            Some(reusable) => reusable,
            None => {
                let discovery = GitSourceDiscovery::new(
                    self.payloads.clone(),
                    Arc::clone(&self.git_transport),
                    Arc::clone(&self.wsl_source),
                )
                .discover(
                    SkillLocationRef {
                        environment: environment.clone(),
                        scope: SkillLocation::Global,
                    },
                    parsed,
                    source,
                    |_| {},
                    cancellation.clone(),
                )
                .await;
                let discovery = match discovery {
                    Ok(discovery) => discovery,
                    Err(error) => return Ok(git_failure(error)),
                };
                let snapshot = self
                    .payloads
                    .source_snapshot(&discovery.discovery_session)?;
                let ref_revision =
                    discovery_ref_revision(snapshot.location()).ok_or_else(|| {
                        AppError::GitCloneFailed {
                            message: "cloned source has no resolvable HEAD revision".to_string(),
                        }
                    })?;
                (discovery.discovery_session, ref_revision)
            }
        };

        let snapshot = self.payloads.source_snapshot(&discovery_session)?;
        let catalog = snapshot
            .skills()
            .map(|skill| normalize_skill_folder_path(&skill.relative_path))
            .collect::<BTreeSet<_>>();
        let selected_paths = request
            .requested_skill_paths
            .iter()
            .filter_map(|requested_path| {
                snapshot
                    .skills()
                    .find(|skill| {
                        normalize_skill_folder_path(&skill.relative_path) == *requested_path
                    })
                    .map(|skill| (requested_path.clone(), skill.relative_path.clone()))
            })
            .collect::<Vec<_>>();
        let handles = SelectedPayloadAcquisitionService::new(self.payloads.clone())
            .acquire(AcquireSelectedPayloadsRequest {
                discovery_session: discovery_session.clone(),
                skill_paths: selected_paths
                    .iter()
                    .map(|(_, relative_path)| relative_path.clone())
                    .collect(),
            })
            .await?;
        let mut skill_revisions = BTreeMap::new();
        for ((skill_path, _), handle) in selected_paths.into_iter().zip(handles) {
            let lease = self.payloads.pin_verified(&handle).await?;
            let metadata = lease.planning_metadata();
            if let Some(revision) = clone_skill_revision(
                provider.clone(),
                metadata.computed_hash.clone(),
                metadata.upstream_revision.clone(),
            ) {
                skill_revisions.insert(skill_path, revision);
            }
        }

        let requested_ref = request.key.normalized_ref;
        let resolved_ref = resolved_ref(&requested_ref);
        let snapshot_id = RemoteSnapshotId::new(requested_ref, resolved_ref, ref_revision);
        Ok(EvidenceDetectionOutcome::Modified(
            RemoteEvidenceObservation {
                snapshot_id: snapshot_id.clone(),
                provider_validation: None,
                complete_skill_path_catalog: catalog.clone(),
                skill_revisions,
                snapshot_facts: Some(SourceSnapshotFacts {
                    discovery_session,
                    snapshot_id,
                    complete_skill_path_catalog: catalog,
                }),
            },
        ))
    }
}

#[cfg(test)]
struct UnavailableGithubAccess;

#[cfg(test)]
impl GithubTreeAccess for UnavailableGithubAccess {
    fn fetch_tree<'a>(
        &'a self,
        _repository: &'a str,
        _git_ref: &'a str,
        _validation: Option<&'a str>,
    ) -> crate::application::github_access::GithubTreeFuture<'a> {
        Box::pin(async { GithubTreeFetchOutcome::Failed(crate::core::GithubTreeFailure::Network) })
    }
}

fn discovery_ref_revision(location: &DiscoverySourceLocation) -> Option<String> {
    match location {
        DiscoverySourceLocation::Native { ref_revision, .. }
        | DiscoverySourceLocation::WslNative { ref_revision, .. } => ref_revision.clone(),
    }
}

fn clone_skill_revision(
    provider: SourceProvider,
    computed_hash: String,
    upstream_revision: Option<String>,
) -> Option<SkillRevision> {
    match provider {
        SourceProvider::Github => upstream_revision.map(SkillRevision::GitTreeOid),
        SourceProvider::Gitlab | SourceProvider::Git => {
            Some(SkillRevision::CliContentHash(computed_hash))
        }
    }
}

impl SourceEvidenceDetector for RuntimeSourceEvidenceDetector {
    fn detect<'a>(
        &'a self,
        request: EvidenceDetectionRequest,
        _previous: Option<RemoteEvidenceEntry>,
        cancellation: CancellationSignal,
    ) -> EvidenceFuture<'a> {
        Box::pin(async move {
            match (&request.environment, request.key.remote.provider()) {
                (EnvironmentRef::Native, SourceProvider::Github) => {
                    self.detect_github(request, _previous).await
                }
                (_, SourceProvider::Github | SourceProvider::Gitlab | SourceProvider::Git) => {
                    self.detect_by_clone(request, _previous, cancellation).await
                }
            }
        })
    }
}

fn git_failure(error: AppError) -> EvidenceDetectionOutcome {
    let (reason, retry_at_epoch_ms) = match error {
        AppError::GitAuthFailed { .. } => (EvidenceFailureReason::AuthenticationRequired, None),
        AppError::GitRepoNotFound { .. } => (EvidenceFailureReason::RepositoryNotFound, None),
        AppError::GitRefNotFound { .. } => (EvidenceFailureReason::RefNotFound, None),
        AppError::GitTimeout { .. } | AppError::GitNetworkError { .. } => (
            EvidenceFailureReason::Network,
            Some((chrono::Utc::now().timestamp_millis().max(0) as u64).saturating_add(30_000)),
        ),
        _ => (EvidenceFailureReason::SourceUnavailable, None),
    };
    failure(
        reason,
        "Git source evidence request failed",
        retry_at_epoch_ms,
        false,
    )
}

fn failure(
    reason: EvidenceFailureReason,
    message: &str,
    retry_at_epoch_ms: Option<u64>,
    provider_cooldown: bool,
) -> EvidenceDetectionOutcome {
    EvidenceDetectionOutcome::Failed(EvidenceDetectionFailure {
        reason,
        message: message.to_string(),
        retry_at_epoch_ms,
        provider_cooldown,
    })
}

fn resolved_ref(normalized_ref: &NormalizedRef) -> String {
    match normalized_ref {
        NormalizedRef::Default => "HEAD".to_string(),
        NormalizedRef::Named(value) => value.clone(),
    }
}

fn evidence_covers_requested_paths(
    evidence: &RemoteEvidenceEntry,
    requested_skill_paths: &BTreeSet<String>,
) -> bool {
    requested_skill_paths.iter().all(|path| {
        !evidence.complete_skill_path_catalog.contains(path)
            || evidence.skill_revisions.contains_key(path)
    })
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::net::SocketAddr;
    use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use std::thread;

    use super::*;
    use crate::application::payload_session::{PayloadSessionLimits, PayloadSessionManager};
    use crate::application::source_evidence::{
        EvidenceCheckMode, EvidenceCheckRequest, EvidenceDetectionOutcome,
        EvidenceDetectionRequest, EvidenceFreshness, ProviderThrottleKey, RemoteEvidenceKey,
        SkillRevision, SourceEvidenceCoordinator, SourceEvidenceDetector,
    };
    use crate::application::source_snapshot_reuse::SourceSnapshotReuseIndex;
    use crate::core::mutation::CancellationSignal;
    use crate::core::parse_source;
    use crate::core::source_identity::SourceIdentity;
    use crate::environment::types::EnvironmentRef;
    use crate::git_fixture::{DeterministicGitTransport, SkillTreeFixture};
    use crate::runtime::github_client::GithubApiClient;

    fn payloads() -> Arc<PayloadSessionManager> {
        Arc::new(PayloadSessionManager::in_memory(
            PayloadSessionLimits {
                ttl_ms: 30 * 60 * 1_000,
                max_sessions: 8,
                max_bytes: 64 * 1024 * 1024,
            },
            || 1_000,
        ))
    }

    #[test]
    fn git_failure_only_schedules_network_retry_for_network_and_timeout_errors() {
        let cases = [
            (
                AppError::GitNetworkError {
                    message: "offline".to_string(),
                },
                EvidenceFailureReason::Network,
                true,
            ),
            (
                AppError::GitTimeout { timeout_secs: 120 },
                EvidenceFailureReason::Network,
                true,
            ),
            (
                AppError::GitCloneFailed {
                    message: "git exited with status 128".to_string(),
                },
                EvidenceFailureReason::SourceUnavailable,
                false,
            ),
        ];

        for (error, expected_reason, expects_retry) in cases {
            let EvidenceDetectionOutcome::Failed(failure) = git_failure(error) else {
                panic!("Git failure must produce failed evidence");
            };
            assert_eq!(failure.reason, expected_reason);
            assert_eq!(failure.retry_at_epoch_ms.is_some(), expects_retry);
        }
    }

    struct HttpResponse {
        status: &'static str,
        headers: Vec<(&'static str, &'static str)>,
        body: &'static str,
    }

    struct HttpFixture {
        addr: SocketAddr,
        requests: Arc<Mutex<Vec<String>>>,
        stopped: Arc<AtomicBool>,
        worker: Option<thread::JoinHandle<()>>,
    }

    impl HttpFixture {
        fn new(responses: Vec<HttpResponse>) -> Self {
            let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
            let addr = server.server_addr().to_ip().expect("HTTP fixture address");
            let requests = Arc::new(Mutex::new(Vec::new()));
            let stopped = Arc::new(AtomicBool::new(false));
            let worker = {
                let requests = requests.clone();
                let stopped = stopped.clone();
                thread::spawn(move || {
                    let mut responses = VecDeque::from(responses);
                    while !stopped.load(Ordering::SeqCst) {
                        let Some(request) = server
                            .recv_timeout(std::time::Duration::from_millis(10))
                            .expect("receive fixture request")
                        else {
                            continue;
                        };
                        let request_text = format!(
                            "{} {} HTTP/1.1\r\n{}\r\n",
                            request.method(),
                            request.url(),
                            request
                                .headers()
                                .iter()
                                .map(|header| format!("{header}\r\n"))
                                .collect::<String>()
                        );
                        requests.lock().unwrap().push(request_text);
                        let response = responses.pop_front().expect("fixture response");
                        let status = response
                            .status
                            .split_whitespace()
                            .next()
                            .and_then(|value| value.parse::<u16>().ok())
                            .expect("fixture status code");
                        let mut reply = tiny_http::Response::from_string(response.body)
                            .with_status_code(status);
                        for (name, value) in response.headers {
                            reply.add_header(
                                tiny_http::Header::from_bytes(name, value)
                                    .expect("fixture response header"),
                            );
                        }
                        request.respond(reply).expect("send fixture response");
                    }
                })
            };
            Self {
                addr,
                requests,
                stopped,
                worker: Some(worker),
            }
        }

        fn base_url(&self) -> String {
            format!("http://{}", self.addr)
        }
    }

    impl Drop for HttpFixture {
        fn drop(&mut self) {
            self.stopped.store(true, Ordering::SeqCst);
            if let Some(worker) = self.worker.take() {
                let _ = worker.join();
            }
        }
    }

    fn github_request(skill_path: &str) -> EvidenceDetectionRequest {
        let identity = SourceIdentity::from_parsed(
            &parse_source("https://github.com/acme/tools#main").unwrap(),
        )
        .unwrap();
        EvidenceDetectionRequest {
            environment: EnvironmentRef::Native,
            key: RemoteEvidenceKey::from_identity(&identity),
            requested_skill_paths: BTreeSet::from([skill_path.to_string()]),
            acquisition: Arc::new(identity.acquisition().clone()),
            acquisition_transport_identity: identity.acquisition_transport().clone(),
        }
    }

    fn github_detector(fixture: &HttpFixture) -> RuntimeSourceEvidenceDetector {
        RuntimeSourceEvidenceDetector::with_github_access(
            payloads(),
            Arc::new(SourceSnapshotReuseIndex::default()),
            Arc::new(GithubApiClient::with_base_url(fixture.base_url())),
        )
    }

    struct CountingDetector {
        inner: RuntimeSourceEvidenceDetector,
        calls: AtomicUsize,
    }

    impl CountingDetector {
        fn new(inner: RuntimeSourceEvidenceDetector) -> Self {
            Self {
                inner,
                calls: AtomicUsize::new(0),
            }
        }

        fn calls(&self) -> usize {
            self.calls.load(Ordering::SeqCst)
        }
    }

    impl SourceEvidenceDetector for CountingDetector {
        fn detect<'a>(
            &'a self,
            request: EvidenceDetectionRequest,
            previous: Option<RemoteEvidenceEntry>,
            cancellation: CancellationSignal,
        ) -> EvidenceFuture<'a> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.inner.detect(request, previous, cancellation)
        }
    }

    fn github_check_request(mode: EvidenceCheckMode) -> EvidenceCheckRequest {
        let identity = SourceIdentity::from_parsed(
            &parse_source("https://github.com/acme/tools#main").unwrap(),
        )
        .unwrap();
        EvidenceCheckRequest {
            environment: EnvironmentRef::Native,
            key: RemoteEvidenceKey::from_identity(&identity),
            throttle_key: ProviderThrottleKey::from_identity(&identity),
            mode,
            requested_skill_paths: BTreeSet::from(["skills/alpha".to_string()]),
            acquisition: Arc::new(identity.acquisition().clone()),
            acquisition_transport_identity: identity.acquisition_transport().clone(),
        }
    }

    fn successful_github_response() -> HttpResponse {
        HttpResponse {
            status: "200 OK",
            headers: vec![("Content-Type", "application/json"), ("ETag", "tree-v1")],
            body: r#"{"sha":"root-tree-v1","truncated":false,"tree":[{"path":"skills/alpha","type":"tree","sha":"tree-alpha"},{"path":"skills/alpha/SKILL.md","type":"blob","sha":"blob-alpha"}]}"#,
        }
    }

    #[tokio::test]
    async fn github_tree_uses_one_request_and_tree_sha_as_source_revision() {
        let fixture = HttpFixture::new(vec![HttpResponse {
            status: "200 OK",
            headers: vec![("Content-Type", "application/json"), ("ETag", "tree-v1")],
            body: r#"{"sha":"root-tree-v1","truncated":false,"tree":[{"path":"skills/alpha","type":"tree","sha":"tree-alpha"},{"path":"skills/alpha/SKILL.md","type":"blob","sha":"blob-alpha"},{"path":"skills/beta","type":"tree","sha":"tree-beta"},{"path":"skills/beta/SKILL.md","type":"blob","sha":"blob-beta"}]}"#,
        }]);

        let outcome = github_detector(&fixture)
            .detect(
                github_request("skills/alpha"),
                None,
                CancellationSignal::default(),
            )
            .await
            .unwrap();
        let EvidenceDetectionOutcome::Modified(facts) = outcome else {
            panic!("expected modified facts, got {outcome:?}");
        };

        assert_eq!(
            facts.snapshot_id.requested_ref,
            NormalizedRef::Named("main".into())
        );
        assert_eq!(facts.snapshot_id.commit_revision, "root-tree-v1");
        assert_eq!(facts.complete_skill_path_catalog.len(), 2);
        assert_eq!(
            facts.skill_revisions.get("skills/alpha"),
            Some(&SkillRevision::GitTreeOid("tree-alpha".into()))
        );
        assert!(!facts.skill_revisions.contains_key("skills/beta"));
        let requests = fixture.requests.lock().unwrap();
        assert_eq!(requests.len(), 1);
        assert!(requests[0].contains("/repos/acme/tools/git/trees/main?recursive=1"));
        assert!(!requests[0].to_ascii_lowercase().contains("if-none-match"));
    }

    #[tokio::test]
    async fn github_repo_root_skill_uses_root_tree_revision() {
        let fixture = HttpFixture::new(vec![HttpResponse {
            status: "200 OK",
            headers: vec![("Content-Type", "application/json")],
            body: r#"{"sha":"root-tree-v1","truncated":false,"tree":[{"path":"SKILL.md","type":"blob","sha":"blob-root"}]}"#,
        }]);

        let outcome = github_detector(&fixture)
            .detect(github_request(""), None, CancellationSignal::default())
            .await
            .unwrap();
        let EvidenceDetectionOutcome::Modified(facts) = outcome else {
            panic!("expected modified facts, got {outcome:?}");
        };

        assert_eq!(
            facts.complete_skill_path_catalog,
            BTreeSet::from(["".into()])
        );
        assert_eq!(
            facts.skill_revisions.get(""),
            Some(&SkillRevision::GitTreeOid("root-tree-v1".into()))
        );
    }

    #[tokio::test]
    async fn github_wsl_refresh_does_not_use_the_native_api_client() {
        let fixture = HttpFixture::new(vec![HttpResponse {
            status: "200 OK",
            headers: vec![("Content-Type", "application/json")],
            body: r#"{"sha":"root-tree-v1","truncated":false,"tree":[]}"#,
        }]);
        let mut request = github_request("skills/alpha");
        request.environment = EnvironmentRef::Wsl {
            distro_name: "Ubuntu".to_string(),
        };

        let _ = github_detector(&fixture)
            .detect(request, None, CancellationSignal::default())
            .await
            .unwrap();

        assert!(fixture.requests.lock().unwrap().is_empty());
    }

    #[test]
    fn clone_revision_keeps_github_tree_oid_and_generic_git_content_hash_distinct() {
        assert_eq!(
            clone_skill_revision(
                SourceProvider::Github,
                "content-hash".to_string(),
                Some("tree-oid".to_string()),
            ),
            Some(SkillRevision::GitTreeOid("tree-oid".to_string()))
        );
        assert_eq!(
            clone_skill_revision(
                SourceProvider::Git,
                "content-hash".to_string(),
                Some("tree-oid".to_string()),
            ),
            Some(SkillRevision::CliContentHash("content-hash".to_string()))
        );
    }

    fn previous_github_evidence() -> RemoteEvidenceEntry {
        RemoteEvidenceEntry {
            checked_at_epoch_ms: 1,
            expires_at_epoch_ms: 2,
            snapshot_id: RemoteSnapshotId::new(
                NormalizedRef::Named("main".into()),
                "main",
                "commit-v1",
            ),
            provider_validation: Some("tree-v1".into()),
            complete_skill_path_catalog: BTreeSet::from(["skills/alpha".into()]),
            skill_revisions: BTreeMap::from([(
                "skills/alpha".into(),
                SkillRevision::GitTreeOid("tree-alpha".into()),
            )]),
        }
    }

    #[tokio::test]
    async fn github_304_with_unchanged_commit_reuses_previous_facts() {
        let fixture = HttpFixture::new(vec![HttpResponse {
            status: "304 Not Modified",
            headers: vec![],
            body: "",
        }]);

        let outcome = github_detector(&fixture)
            .detect(
                github_request("skills/alpha"),
                Some(previous_github_evidence()),
                CancellationSignal::default(),
            )
            .await
            .unwrap();

        assert_eq!(outcome, EvidenceDetectionOutcome::NotModified);
        let requests = fixture.requests.lock().unwrap();
        assert_eq!(requests.len(), 1);
        assert!(requests[0]
            .to_ascii_lowercase()
            .contains("if-none-match: tree-v1"));
    }

    #[tokio::test]
    async fn github_skips_conditional_validation_when_cached_evidence_lacks_path_coverage() {
        let fixture = HttpFixture::new(vec![HttpResponse {
            status: "200 OK",
            headers: vec![("Content-Type", "application/json"), ("ETag", "tree-v1")],
            body: r#"{"sha":"root-tree-v1","truncated":false,"tree":[{"path":"skills/alpha","type":"tree","sha":"tree-alpha"},{"path":"skills/alpha/SKILL.md","type":"blob","sha":"blob-alpha"},{"path":"skills/beta","type":"tree","sha":"tree-beta"},{"path":"skills/beta/SKILL.md","type":"blob","sha":"blob-beta"}]}"#,
        }]);
        let mut previous = previous_github_evidence();
        previous
            .complete_skill_path_catalog
            .insert("skills/beta".to_string());

        let outcome = github_detector(&fixture)
            .detect(
                github_request("skills/beta"),
                Some(previous),
                CancellationSignal::default(),
            )
            .await
            .unwrap();
        let EvidenceDetectionOutcome::Modified(facts) = outcome else {
            panic!("expected enriched evidence, got {outcome:?}");
        };

        assert_eq!(
            facts.skill_revisions.get("skills/beta"),
            Some(&SkillRevision::GitTreeOid("tree-beta".into()))
        );
        let requests = fixture.requests.lock().unwrap();
        assert_eq!(requests.len(), 1);
        assert!(!requests[0].to_ascii_lowercase().contains("if-none-match"));
    }

    #[tokio::test]
    async fn github_304_reuses_the_previous_tree_revision() {
        let fixture = HttpFixture::new(vec![HttpResponse {
            status: "304 Not Modified",
            headers: vec![],
            body: "",
        }]);

        let outcome = github_detector(&fixture)
            .detect(
                github_request("skills/alpha"),
                Some(previous_github_evidence()),
                CancellationSignal::default(),
            )
            .await
            .unwrap();
        assert_eq!(outcome, EvidenceDetectionOutcome::NotModified);
    }

    #[tokio::test]
    async fn github_truncated_rate_limit_and_ambiguous_404_are_typed_failures() {
        let cases = [
            (
                vec![HttpResponse {
                    status: "200 OK",
                    headers: vec![("Content-Type", "application/json")],
                    body: r#"{"sha":"root-tree-v1","truncated":true,"tree":[]}"#,
                }],
                EvidenceFailureReason::IncompleteEvidence,
            ),
            (
                vec![HttpResponse {
                    status: "403 Forbidden",
                    headers: vec![("X-RateLimit-Remaining", "0"), ("Retry-After", "30")],
                    body: "",
                }],
                EvidenceFailureReason::RateLimited,
            ),
            (
                vec![HttpResponse {
                    status: "404 Not Found",
                    headers: vec![],
                    body: "",
                }],
                EvidenceFailureReason::NotFoundOrUnauthorized,
            ),
        ];

        for (responses, expected_reason) in cases {
            let fixture = HttpFixture::new(responses);
            let outcome = github_detector(&fixture)
                .detect(
                    github_request("skills/alpha"),
                    None,
                    CancellationSignal::default(),
                )
                .await
                .unwrap();
            let EvidenceDetectionOutcome::Failed(failure) = outcome else {
                panic!("expected failure, got {outcome:?}");
            };
            assert_eq!(failure.reason, expected_reason);
        }
    }

    #[tokio::test]
    async fn coordinator_reuses_fresh_github_evidence_without_detector_or_http_requests() {
        let fixture = HttpFixture::new(vec![
            successful_github_response(),
            successful_github_response(),
        ]);
        let detector = Arc::new(CountingDetector::new(github_detector(&fixture)));
        let now = Arc::new(AtomicU64::new(1_000));
        let coordinator = SourceEvidenceCoordinator::with_clock(detector.clone(), {
            let now = now.clone();
            move || now.load(Ordering::SeqCst)
        });
        let request = github_check_request(EvidenceCheckMode::Automatic);

        let first = coordinator
            .check(request.clone(), CancellationSignal::default())
            .await
            .unwrap();
        let cached = coordinator
            .check(request, CancellationSignal::default())
            .await
            .unwrap();

        assert_eq!(first.freshness, EvidenceFreshness::Fresh);
        assert_eq!(cached.freshness, EvidenceFreshness::Cached);
        assert_eq!(detector.calls(), 1);
        assert_eq!(fixture.requests.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn coordinator_backoff_blocks_http_until_one_concurrent_batch_after_deadline() {
        let fixture = HttpFixture::new(vec![
            HttpResponse {
                status: "500 Internal Server Error",
                headers: vec![],
                body: "",
            },
            successful_github_response(),
            successful_github_response(),
        ]);
        let detector = Arc::new(CountingDetector::new(github_detector(&fixture)));
        let now = Arc::new(AtomicU64::new(1_000));
        let coordinator = SourceEvidenceCoordinator::with_clock(detector.clone(), {
            let now = now.clone();
            move || now.load(Ordering::SeqCst)
        });
        let request = github_check_request(EvidenceCheckMode::Automatic);

        let failed = coordinator
            .check(request.clone(), CancellationSignal::default())
            .await
            .unwrap();
        let failure = failed.last_attempt.unwrap().failure.unwrap();
        assert_eq!(failure.reason, EvidenceFailureReason::SourceUnavailable);
        let retry_at = failure.retry_at_epoch_ms.expect("transient retry deadline");

        now.store(retry_at - 1, Ordering::SeqCst);
        let backed_off = coordinator
            .check(request.clone(), CancellationSignal::default())
            .await
            .unwrap();
        assert_eq!(backed_off.freshness, EvidenceFreshness::BackingOff);
        assert_eq!(detector.calls(), 1);
        assert_eq!(fixture.requests.lock().unwrap().len(), 1);

        now.store(retry_at, Ordering::SeqCst);
        let (left, right) = tokio::join!(
            coordinator.check(request.clone(), CancellationSignal::default()),
            coordinator.check(request, CancellationSignal::default()),
        );

        assert!(left.unwrap().evidence.is_some());
        assert!(right.unwrap().evidence.is_some());
        assert_eq!(detector.calls(), 2);
        assert_eq!(fixture.requests.lock().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn coordinator_cooldown_blocks_automatic_and_force_until_one_batch_after_deadline() {
        let fixture = HttpFixture::new(vec![
            HttpResponse {
                status: "429 Too Many Requests",
                headers: vec![("Retry-After", "30")],
                body: "",
            },
            successful_github_response(),
            successful_github_response(),
        ]);
        let detector = Arc::new(CountingDetector::new(github_detector(&fixture)));
        let now = Arc::new(AtomicU64::new(1_000));
        let coordinator = SourceEvidenceCoordinator::with_clock(detector.clone(), {
            let now = now.clone();
            move || now.load(Ordering::SeqCst)
        });
        let automatic = github_check_request(EvidenceCheckMode::Automatic);

        let limited = coordinator
            .check(automatic.clone(), CancellationSignal::default())
            .await
            .unwrap();
        let failure = limited.last_attempt.unwrap().failure.unwrap();
        assert_eq!(failure.reason, EvidenceFailureReason::RateLimited);
        assert!(failure.provider_cooldown);
        let retry_at = failure.retry_at_epoch_ms.expect("provider retry deadline");

        let before_automatic = coordinator
            .check(automatic.clone(), CancellationSignal::default())
            .await
            .unwrap();
        let before_force = coordinator
            .check(
                github_check_request(EvidenceCheckMode::Force),
                CancellationSignal::default(),
            )
            .await
            .unwrap();
        assert_eq!(before_automatic.freshness, EvidenceFreshness::CoolingDown);
        assert_eq!(before_force.freshness, EvidenceFreshness::CoolingDown);
        assert_eq!(detector.calls(), 1);
        assert_eq!(fixture.requests.lock().unwrap().len(), 1);

        now.store(retry_at, Ordering::SeqCst);
        let (left, right) = tokio::join!(
            coordinator.check(automatic.clone(), CancellationSignal::default()),
            coordinator.check(automatic, CancellationSignal::default()),
        );

        assert!(left.unwrap().evidence.is_some());
        assert!(right.unwrap().evidence.is_some());
        assert_eq!(detector.calls(), 2);
        assert_eq!(fixture.requests.lock().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn clone_detector_discovers_all_paths_but_hashes_only_requested_skills() {
        let remote = SkillTreeFixture::new(&["skills/alpha", "skills/beta"]);
        let parsed = ParsedSource {
            source_type: SourceType::Git,
            url: remote.source(),
            subpath: None,
            local_path: None,
            git_ref: Some("main".to_string()),
            skill_filter: None,
        };
        let identity = SourceIdentity::from_parsed(&parsed).unwrap();
        let payloads = payloads();
        let git_transport = Arc::new(DeterministicGitTransport::for_fixture(&remote));
        let detector = RuntimeSourceEvidenceDetector::with_git_transport(
            payloads.clone(),
            Arc::new(SourceSnapshotReuseIndex::default()),
            git_transport.clone(),
        );

        let outcome = detector
            .detect(
                EvidenceDetectionRequest {
                    environment: EnvironmentRef::Native,
                    key: RemoteEvidenceKey::from_identity(&identity),
                    requested_skill_paths: BTreeSet::from(["skills/alpha".to_string()]),
                    acquisition: Arc::new(identity.acquisition().clone()),
                    acquisition_transport_identity: identity.acquisition_transport().clone(),
                },
                None,
                CancellationSignal::default(),
            )
            .await
            .unwrap();

        let EvidenceDetectionOutcome::Modified(evidence) = outcome else {
            panic!("expected modified evidence, got {outcome:?}");
        };
        assert_eq!(
            evidence.snapshot_id.requested_ref,
            NormalizedRef::Named("main".into())
        );
        assert_eq!(evidence.snapshot_id.resolved_ref, "main");
        assert_eq!(evidence.snapshot_id.commit_revision, "fixture-revision-1");
        assert_eq!(evidence.complete_skill_path_catalog.len(), 2);
        assert!(matches!(
            evidence.skill_revisions.get("skills/alpha"),
            Some(SkillRevision::CliContentHash(_))
        ));
        assert!(!evidence.skill_revisions.contains_key("skills/beta"));
        assert_eq!(
            payloads
                .protected_session_ids(&EnvironmentRef::Native)
                .unwrap()
                .len(),
            1
        );
        assert_eq!(git_transport.clone_count(), 1);
    }

    #[tokio::test]
    async fn clone_detector_routes_wsl_refresh_away_from_native_git_transport() {
        let remote = SkillTreeFixture::new(&["skills/alpha"]);
        let parsed = ParsedSource {
            source_type: SourceType::Git,
            url: remote.source(),
            subpath: None,
            local_path: None,
            git_ref: Some("main".to_string()),
            skill_filter: None,
        };
        let identity = SourceIdentity::from_parsed(&parsed).unwrap();
        let git_transport = Arc::new(DeterministicGitTransport::for_fixture(&remote));
        let detector = RuntimeSourceEvidenceDetector::with_git_transport(
            payloads(),
            Arc::new(SourceSnapshotReuseIndex::default()),
            git_transport.clone(),
        );

        let _ = detector
            .detect(
                EvidenceDetectionRequest {
                    environment: EnvironmentRef::Wsl {
                        distro_name: "Ubuntu".to_string(),
                    },
                    key: RemoteEvidenceKey::from_identity(&identity),
                    requested_skill_paths: BTreeSet::from(["skills/alpha".to_string()]),
                    acquisition: Arc::new(identity.acquisition().clone()),
                    acquisition_transport_identity: identity.acquisition_transport().clone(),
                },
                None,
                CancellationSignal::default(),
            )
            .await
            .unwrap();

        assert_eq!(git_transport.clone_count(), 0);
    }

    #[test]
    fn wsl_native_snapshot_exposes_its_git_revision() {
        let location = DiscoverySourceLocation::WslNative {
            distro_name: "Ubuntu".to_string(),
            linux_root: "/tmp/skill-deck-source".to_string(),
            ref_revision: Some("wsl-revision-1".to_string()),
        };

        assert_eq!(
            discovery_ref_revision(&location).as_deref(),
            Some("wsl-revision-1")
        );
    }

    #[tokio::test]
    async fn clone_detector_enriches_a_retained_snapshot_without_recloning() {
        let remote = SkillTreeFixture::new(&["skills/alpha", "skills/beta"]);
        let parsed = ParsedSource {
            source_type: SourceType::Git,
            url: remote.source(),
            subpath: None,
            local_path: None,
            git_ref: Some("main".to_string()),
            skill_filter: None,
        };
        let identity = SourceIdentity::from_parsed(&parsed).unwrap();
        let payloads = payloads();
        let snapshots = Arc::new(SourceSnapshotReuseIndex::default());
        let git_transport = Arc::new(DeterministicGitTransport::for_fixture(&remote));
        let detector = Arc::new(RuntimeSourceEvidenceDetector::with_git_transport(
            payloads,
            snapshots.clone(),
            git_transport.clone(),
        ));
        let coordinator = SourceEvidenceCoordinator::with_snapshot_reuse(detector, snapshots);
        let request = |path: &str| EvidenceCheckRequest {
            environment: EnvironmentRef::Native,
            key: RemoteEvidenceKey::from_identity(&identity),
            throttle_key: ProviderThrottleKey::from_identity(&identity),
            mode: EvidenceCheckMode::Force,
            requested_skill_paths: BTreeSet::from([path.to_string()]),
            acquisition: Arc::new(identity.acquisition().clone()),
            acquisition_transport_identity: identity.acquisition_transport().clone(),
        };

        let first = coordinator
            .check(request("skills/alpha"), CancellationSignal::default())
            .await
            .unwrap();
        assert!(first.evidence.is_some());

        let second = coordinator
            .check(request("skills/beta"), CancellationSignal::default())
            .await
            .unwrap();
        let second = second.evidence.expect("enriched evidence");

        assert!(matches!(
            second.skill_revisions.get("skills/beta"),
            Some(SkillRevision::CliContentHash(_))
        ));
        assert_eq!(git_transport.clone_count(), 1);
    }

    #[tokio::test]
    async fn clone_detector_reacquires_when_the_remote_ref_changes() {
        let remote = SkillTreeFixture::new(&["skills/alpha"]);
        let parsed = ParsedSource {
            source_type: SourceType::Git,
            url: remote.source(),
            subpath: None,
            local_path: None,
            git_ref: Some("main".to_string()),
            skill_filter: None,
        };
        let identity = SourceIdentity::from_parsed(&parsed).unwrap();
        let git_transport = Arc::new(DeterministicGitTransport::for_fixture(&remote));
        let detector = RuntimeSourceEvidenceDetector::with_git_transport(
            payloads(),
            Arc::new(SourceSnapshotReuseIndex::default()),
            git_transport.clone(),
        );
        let request = || EvidenceDetectionRequest {
            environment: EnvironmentRef::Native,
            key: RemoteEvidenceKey::from_identity(&identity),
            requested_skill_paths: BTreeSet::from(["skills/alpha".to_string()]),
            acquisition: Arc::new(identity.acquisition().clone()),
            acquisition_transport_identity: identity.acquisition_transport().clone(),
        };
        let first_outcome = detector
            .detect(request(), None, CancellationSignal::default())
            .await
            .unwrap();
        let EvidenceDetectionOutcome::Modified(first) = first_outcome else {
            panic!("expected first snapshot, got {first_outcome:?}");
        };
        let first_revision = first.snapshot_id.commit_revision.clone();
        let previous = entry(first);
        remote.commit_change("skills/alpha");

        let outcome = detector
            .detect(request(), Some(previous), CancellationSignal::default())
            .await
            .unwrap();
        let EvidenceDetectionOutcome::Modified(second) = outcome else {
            panic!("expected changed snapshot, got {outcome:?}");
        };

        assert_ne!(second.snapshot_id.commit_revision, first_revision);
        assert_eq!(git_transport.clone_count(), 2);
    }

    #[tokio::test]
    async fn clone_detector_does_not_reuse_after_remote_probe_failure() {
        let remote = SkillTreeFixture::new(&["skills/alpha"]);
        let parsed = ParsedSource {
            source_type: SourceType::Git,
            url: remote.source(),
            subpath: None,
            local_path: None,
            git_ref: Some("main".to_string()),
            skill_filter: None,
        };
        let identity = SourceIdentity::from_parsed(&parsed).unwrap();
        let git_transport = Arc::new(DeterministicGitTransport::for_fixture(&remote));
        let detector = RuntimeSourceEvidenceDetector::with_git_transport(
            payloads(),
            Arc::new(SourceSnapshotReuseIndex::default()),
            git_transport.clone(),
        );
        let request = || EvidenceDetectionRequest {
            environment: EnvironmentRef::Native,
            key: RemoteEvidenceKey::from_identity(&identity),
            requested_skill_paths: BTreeSet::from(["skills/alpha".to_string()]),
            acquisition: Arc::new(identity.acquisition().clone()),
            acquisition_transport_identity: identity.acquisition_transport().clone(),
        };
        let first_outcome = detector
            .detect(request(), None, CancellationSignal::default())
            .await
            .unwrap();
        let EvidenceDetectionOutcome::Modified(first) = first_outcome else {
            panic!("expected first snapshot, got {first_outcome:?}");
        };
        git_transport.reject_probes();

        let outcome = detector
            .detect(request(), Some(entry(first)), CancellationSignal::default())
            .await
            .unwrap();
        let EvidenceDetectionOutcome::Failed(failure) = outcome else {
            panic!("expected probe failure, got {outcome:?}");
        };

        assert_eq!(failure.reason, EvidenceFailureReason::SourceUnavailable);
        assert_eq!(failure.message, "Git source evidence request failed");
        assert_eq!(git_transport.clone_count(), 1);
    }

    fn entry(facts: RemoteEvidenceObservation) -> RemoteEvidenceEntry {
        RemoteEvidenceEntry {
            checked_at_epoch_ms: 1,
            expires_at_epoch_ms: 2,
            snapshot_id: facts.snapshot_id,
            provider_validation: facts.provider_validation,
            complete_skill_path_catalog: facts.complete_skill_path_catalog,
            skill_revisions: facts.skill_revisions,
        }
    }
}

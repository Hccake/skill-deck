use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use crate::application::payload_session::{DiscoverySourceLocation, PayloadSessionManager};
use crate::application::source_acquisition::{
    AcquireSelectedPayloadsRequest, SelectedPayloadAcquisitionService, SourceDiscoveryService,
};
use crate::application::source_evidence::{
    EvidenceDetectionFailure, EvidenceDetectionOutcome, EvidenceDetectionRequest,
    EvidenceFailureReason, EvidenceFuture, RemoteEvidenceEntry, RemoteEvidenceObservation,
    RemoteSnapshotId, SkillRevision, SourceEvidenceDetector, SourceSnapshotFacts,
};
use crate::application::source_snapshot_reuse::{PayloadAcquisitionKey, SourceSnapshotReuseIndex};
use crate::core::mutation::CancellationSignal;
use crate::core::skill_paths::normalize_skill_folder_path;
use crate::core::source_identity::{NormalizedRef, SourceProvider};
use crate::core::{compute_local_ref_revision, probe_remote_ref_revision};
use crate::core::{GithubApiClient, GithubTreeFailure, GithubTreeFetchOutcome};
use crate::environment::types::{ContextRef, ContextScope, EnvironmentRef};
use crate::environment::wsl::EnvironmentRegistry;
use crate::error::AppError;
#[cfg(test)]
use crate::models::{ParsedSource, SourceType};

pub struct RuntimeSourceEvidenceDetector {
    payloads: Arc<PayloadSessionManager>,
    environments: Arc<EnvironmentRegistry>,
    snapshots: Arc<SourceSnapshotReuseIndex>,
    github: GithubApiClient,
}

impl RuntimeSourceEvidenceDetector {
    pub fn new(
        payloads: Arc<PayloadSessionManager>,
        environments: Arc<EnvironmentRegistry>,
        snapshots: Arc<SourceSnapshotReuseIndex>,
    ) -> Self {
        Self {
            payloads,
            environments,
            snapshots,
            github: GithubApiClient::new(),
        }
    }

    #[cfg(test)]
    fn with_github_client(
        payloads: Arc<PayloadSessionManager>,
        environments: Arc<EnvironmentRegistry>,
        snapshots: Arc<SourceSnapshotReuseIndex>,
        github: GithubApiClient,
    ) -> Self {
        Self {
            payloads,
            environments,
            snapshots,
            github,
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
        if matches!(outcome, GithubTreeFetchOutcome::NotModified { .. }) && previous.is_none() {
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
            GithubTreeFetchOutcome::NotModified { ref_revision } => match previous {
                Some(previous) if previous.snapshot_id.commit_revision == ref_revision => {
                    EvidenceDetectionOutcome::NotModified
                }
                Some(previous) => EvidenceDetectionOutcome::Modified(RemoteEvidenceObservation {
                    snapshot_id: RemoteSnapshotId::new(
                        request.key.normalized_ref,
                        requested_ref,
                        ref_revision,
                    ),
                    provider_validation: previous.provider_validation,
                    complete_skill_path_catalog: previous.complete_skill_path_catalog,
                    skill_revisions: previous.skill_revisions,
                    snapshot_facts: None,
                }),
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
        let parsed = request
            .acquisition
            .parsed_source(request.key.remote.provider());
        let acquisition_key = PayloadAcquisitionKey::new(
            request.acquisition_transport_identity.clone(),
            request.key.normalized_ref.clone(),
            &EnvironmentRef::Host,
        );
        let reusable = if previous.is_some() {
            let probe_source = source.clone();
            let probe_ref = request.acquisition.git_ref().map(ToString::to_string);
            let probe_cancellation = cancellation.clone();
            let probed = tokio::task::spawn_blocking(move || {
                probe_remote_ref_revision(&probe_source, probe_ref.as_deref(), probe_cancellation)
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
                let discovery =
                    SourceDiscoveryService::new(self.payloads.clone(), self.environments.as_ref())
                        .discover_parsed_with_cancellation(
                            ContextRef {
                                environment: EnvironmentRef::Host,
                                scope: ContextScope::Global,
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
                let ref_revision = match snapshot.location() {
                    DiscoverySourceLocation::Native { root } => compute_local_ref_revision(root),
                    DiscoverySourceLocation::WslNative { .. } => None,
                }
                .ok_or_else(|| AppError::GitCloneFailed {
                    message: "cloned source has no resolvable HEAD revision".to_string(),
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
            skill_revisions.insert(
                skill_path,
                SkillRevision::CliContentHash(lease.planning_metadata().computed_hash.clone()),
            );
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

impl SourceEvidenceDetector for RuntimeSourceEvidenceDetector {
    fn detect<'a>(
        &'a self,
        request: EvidenceDetectionRequest,
        _previous: Option<RemoteEvidenceEntry>,
        cancellation: CancellationSignal,
    ) -> EvidenceFuture<'a> {
        Box::pin(async move {
            match request.key.remote.provider() {
                SourceProvider::Gitlab | SourceProvider::Git => {
                    self.detect_by_clone(request, _previous, cancellation).await
                }
                SourceProvider::Github => self.detect_github(request, _previous).await,
            }
        })
    }
}

fn git_failure(error: AppError) -> EvidenceDetectionOutcome {
    let (reason, retry_at_epoch_ms) = match error {
        AppError::GitAuthFailed { .. } => (EvidenceFailureReason::AuthenticationRequired, None),
        AppError::GitRepoNotFound { .. } => (EvidenceFailureReason::RepositoryNotFound, None),
        AppError::GitRefNotFound { .. } => (EvidenceFailureReason::RefNotFound, None),
        AppError::GitTimeout { .. } | AppError::GitCloneFailed { .. } => (
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
    use std::fs;
    use std::io::{self, Read, Write};
    use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream};
    use std::path::{Path, PathBuf};
    use std::process::{Child, Command, Stdio};
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use std::thread;

    use tempfile::TempDir;

    use super::*;
    use crate::application::payload_session::{PayloadSessionLimits, PayloadSessionManager};
    use crate::application::source_evidence::{
        EvidenceCheckMode, EvidenceCheckRequest, EvidenceDetectionOutcome,
        EvidenceDetectionRequest, ProviderThrottleKey, RemoteEvidenceKey, SkillRevision,
        SourceEvidenceCoordinator, SourceEvidenceDetector,
    };
    use crate::application::source_snapshot_reuse::SourceSnapshotReuseIndex;
    use crate::core::mutation::CancellationSignal;
    use crate::core::parse_source;
    use crate::core::source_identity::SourceIdentity;
    use crate::core::GithubApiClient;
    use crate::environment::types::EnvironmentRef;
    use crate::environment::wsl::EnvironmentRegistry;

    struct BareSkillRepo {
        _root: TempDir,
        work: PathBuf,
        remote: PathBuf,
        server: GitDaemon,
    }

    impl BareSkillRepo {
        fn new(skill_paths: &[&str]) -> Self {
            let root = tempfile::tempdir().unwrap();
            let work = root.path().join("work");
            let remote = root.path().join("remote.git");
            run(
                root.path(),
                &["init", "-q", "-b", "main", work.to_str().unwrap()],
            );
            run(&work, &["config", "user.email", "test@example.com"]);
            run(&work, &["config", "user.name", "Test"]);
            run(&work, &["config", "commit.gpgsign", "false"]);
            for skill_path in skill_paths {
                let skill_root = work.join(skill_path);
                fs::create_dir_all(&skill_root).unwrap();
                let name = skill_root.file_name().unwrap().to_string_lossy();
                fs::write(
                    skill_root.join("SKILL.md"),
                    format!("---\nname: {name}\ndescription: test skill\n---\n"),
                )
                .unwrap();
            }
            run(&work, &["add", "-A"]);
            run(&work, &["commit", "-q", "-m", "initial"]);
            run(
                root.path(),
                &[
                    "clone",
                    "-q",
                    "--bare",
                    work.to_str().unwrap(),
                    remote.to_str().unwrap(),
                ],
            );
            let server = GitDaemon::new(root.path().to_path_buf());
            Self {
                _root: root,
                work,
                remote,
                server,
            }
        }

        fn source(&self) -> String {
            format!("git://{}/remote.git", self.server.addr)
        }

        fn clone_count(&self) -> usize {
            self.server.clone_count.load(Ordering::SeqCst)
        }

        fn commit_change(&self, skill_path: &str) {
            fs::write(
                self.work.join(skill_path).join("SKILL.md"),
                "---\nname: changed\ndescription: changed skill\n---\n",
            )
            .unwrap();
            run(&self.work, &["add", "-A"]);
            run(&self.work, &["commit", "-q", "-m", "change"]);
            run(
                &self.work,
                &["push", "-q", self.remote.to_str().unwrap(), "main"],
            );
        }

        fn stop_upstream(&mut self) {
            self.server.stop_upstream();
        }
    }

    struct GitDaemon {
        addr: SocketAddr,
        clone_count: Arc<AtomicUsize>,
        reject_connections: Arc<AtomicBool>,
        stopped: Arc<AtomicBool>,
        worker: Option<thread::JoinHandle<()>>,
        child: Child,
    }

    impl GitDaemon {
        fn new(root: PathBuf) -> Self {
            let inner_listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let inner_addr = inner_listener.local_addr().unwrap();
            drop(inner_listener);
            let child = Command::new("git")
                .args([
                    "daemon",
                    "--export-all",
                    "--reuseaddr",
                    "--listen=127.0.0.1",
                    &format!("--port={}", inner_addr.port()),
                    &format!("--base-path={}", root.display()),
                    root.to_str().unwrap(),
                ])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap();
            for _ in 0..100 {
                if TcpStream::connect(inner_addr).is_ok() {
                    break;
                }
                thread::sleep(std::time::Duration::from_millis(5));
            }
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            listener.set_nonblocking(true).unwrap();
            let addr = listener.local_addr().unwrap();
            let clone_count = Arc::new(AtomicUsize::new(0));
            let reject_connections = Arc::new(AtomicBool::new(false));
            let stopped = Arc::new(AtomicBool::new(false));
            let worker = {
                let clone_count = clone_count.clone();
                let reject_connections = reject_connections.clone();
                let stopped = stopped.clone();
                thread::spawn(move || {
                    while !stopped.load(Ordering::SeqCst) {
                        match listener.accept() {
                            Ok((stream, _)) => {
                                if reject_connections.load(Ordering::SeqCst) {
                                    drop(stream);
                                    continue;
                                }
                                let clone_count = clone_count.clone();
                                thread::spawn(move || {
                                    proxy_git_connection(stream, inner_addr, clone_count)
                                });
                            }
                            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                                thread::sleep(std::time::Duration::from_millis(2));
                            }
                            Err(_) => break,
                        }
                    }
                })
            };
            Self {
                addr,
                clone_count,
                reject_connections,
                stopped,
                worker: Some(worker),
                child,
            }
        }

        fn stop_upstream(&mut self) {
            self.reject_connections.store(true, Ordering::SeqCst);
        }
    }

    impl Drop for GitDaemon {
        fn drop(&mut self) {
            self.stopped.store(true, Ordering::SeqCst);
            let _ = TcpStream::connect(self.addr);
            if let Some(worker) = self.worker.take() {
                let _ = worker.join();
            }
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }

    fn proxy_git_connection(
        mut client: TcpStream,
        upstream_addr: SocketAddr,
        clone_count: Arc<AtomicUsize>,
    ) {
        let Ok(mut upstream) = TcpStream::connect(upstream_addr) else {
            return;
        };
        let mut client_reader = client.try_clone().unwrap();
        let mut upstream_writer = upstream.try_clone().unwrap();
        let inbound = thread::spawn(move || {
            let mut buffer = [0_u8; 8 * 1024];
            let mut recent = Vec::new();
            let mut counted = false;
            let result = loop {
                match client_reader.read(&mut buffer) {
                    Ok(0) => break Ok(()),
                    Ok(length) => {
                        recent.extend_from_slice(&buffer[..length]);
                        if !counted
                            && (recent.windows(5).any(|window| window == b"want ")
                                || recent.windows(13).any(|window| window == b"command=fetch"))
                        {
                            clone_count.fetch_add(1, Ordering::SeqCst);
                            counted = true;
                        }
                        if recent.len() > 32 {
                            recent.drain(..recent.len() - 32);
                        }
                        if let Err(error) = upstream_writer.write_all(&buffer[..length]) {
                            break Err(error);
                        }
                    }
                    Err(error) => break Err(error),
                }
            };
            let _ = upstream_writer.shutdown(Shutdown::Write);
            result
        });
        let _ = io::copy(&mut upstream, &mut client);
        let _ = client.shutdown(Shutdown::Write);
        let _ = inbound.join();
    }

    fn run(cwd: &Path, args: &[&str]) {
        let status = Command::new("git")
            .current_dir(cwd)
            .args(args)
            .status()
            .unwrap();
        assert!(status.success(), "git command failed: {args:?}");
    }

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
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            listener.set_nonblocking(true).unwrap();
            let addr = listener.local_addr().unwrap();
            let requests = Arc::new(Mutex::new(Vec::new()));
            let stopped = Arc::new(AtomicBool::new(false));
            let worker = {
                let requests = requests.clone();
                let stopped = stopped.clone();
                thread::spawn(move || {
                    let mut responses = VecDeque::from(responses);
                    while !stopped.load(Ordering::SeqCst) {
                        match listener.accept() {
                            Ok((mut stream, _)) => {
                                let mut bytes = [0_u8; 8 * 1024];
                                let length = stream.read(&mut bytes).unwrap_or_default();
                                requests
                                    .lock()
                                    .unwrap()
                                    .push(String::from_utf8_lossy(&bytes[..length]).into_owned());
                                let response = responses.pop_front().expect("fixture response");
                                let mut headers = response
                                    .headers
                                    .into_iter()
                                    .map(|(name, value)| format!("{name}: {value}\r\n"))
                                    .collect::<String>();
                                headers.push_str(&format!(
                                    "Content-Length: {}\r\nConnection: close\r\n",
                                    response.body.len()
                                ));
                                let response = format!(
                                    "HTTP/1.1 {}\r\n{}\r\n{}",
                                    response.status, headers, response.body
                                );
                                stream.write_all(response.as_bytes()).unwrap();
                            }
                            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                                thread::sleep(std::time::Duration::from_millis(2));
                            }
                            Err(_) => break,
                        }
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
            let _ = TcpStream::connect(self.addr);
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
            key: RemoteEvidenceKey::from_identity(&identity),
            requested_skill_paths: BTreeSet::from([skill_path.to_string()]),
            acquisition: Arc::new(identity.acquisition().clone()),
            acquisition_transport_identity: identity.acquisition_transport().clone(),
        }
    }

    fn github_detector(fixture: &HttpFixture) -> RuntimeSourceEvidenceDetector {
        RuntimeSourceEvidenceDetector::with_github_client(
            payloads(),
            Arc::new(EnvironmentRegistry::default()),
            Arc::new(SourceSnapshotReuseIndex::default()),
            GithubApiClient::with_base_url(fixture.base_url()),
        )
    }

    #[tokio::test]
    async fn github_tree_uses_the_resolved_commit_for_the_following_tree_request() {
        let fixture = HttpFixture::new(vec![
            HttpResponse {
                status: "200 OK",
                headers: vec![("Content-Type", "application/json")],
                body: r#"{"sha":"commit-v1"}"#,
            },
            HttpResponse {
                status: "200 OK",
                headers: vec![("Content-Type", "application/json"), ("ETag", "tree-v1")],
                body: r#"{"sha":"root-tree-v1","truncated":false,"tree":[{"path":"skills/alpha","type":"tree","sha":"tree-alpha"},{"path":"skills/alpha/SKILL.md","type":"blob","sha":"blob-alpha"},{"path":"skills/beta","type":"tree","sha":"tree-beta"},{"path":"skills/beta/SKILL.md","type":"blob","sha":"blob-beta"}]}"#,
            },
        ]);

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
        assert_eq!(facts.snapshot_id.commit_revision, "commit-v1");
        assert_eq!(facts.complete_skill_path_catalog.len(), 2);
        assert_eq!(
            facts.skill_revisions.get("skills/alpha"),
            Some(&SkillRevision::GitTreeOid("tree-alpha".into()))
        );
        assert!(!facts.skill_revisions.contains_key("skills/beta"));
        let requests = fixture.requests.lock().unwrap();
        assert!(requests[0].contains("/repos/acme/tools/commits/main"));
        assert!(requests[1].contains("/repos/acme/tools/git/trees/commit-v1?recursive=1"));
        assert!(!requests[0].to_ascii_lowercase().contains("if-none-match"));
    }

    #[tokio::test]
    async fn github_repo_root_skill_uses_root_tree_revision() {
        let fixture = HttpFixture::new(vec![
            HttpResponse {
                status: "200 OK",
                headers: vec![("Content-Type", "application/json")],
                body: r#"{"sha":"commit-v1"}"#,
            },
            HttpResponse {
                status: "200 OK",
                headers: vec![("Content-Type", "application/json")],
                body: r#"{"sha":"root-tree-v1","truncated":false,"tree":[{"path":"SKILL.md","type":"blob","sha":"blob-root"}]}"#,
            },
        ]);

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
        let fixture = HttpFixture::new(vec![
            HttpResponse {
                status: "200 OK",
                headers: vec![("Content-Type", "application/json")],
                body: r#"{"sha":"commit-v1"}"#,
            },
            HttpResponse {
                status: "304 Not Modified",
                headers: vec![],
                body: "",
            },
        ]);

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
        assert!(!requests[0].to_ascii_lowercase().contains("if-none-match"));
        assert!(requests[1]
            .to_ascii_lowercase()
            .contains("if-none-match: tree-v1"));
    }

    #[tokio::test]
    async fn github_skips_conditional_validation_when_cached_evidence_lacks_path_coverage() {
        let fixture = HttpFixture::new(vec![
            HttpResponse {
                status: "200 OK",
                headers: vec![("Content-Type", "application/json")],
                body: r#"{"sha":"commit-v1"}"#,
            },
            HttpResponse {
                status: "200 OK",
                headers: vec![("Content-Type", "application/json"), ("ETag", "tree-v1")],
                body: r#"{"sha":"root-tree-v1","truncated":false,"tree":[{"path":"skills/alpha","type":"tree","sha":"tree-alpha"},{"path":"skills/alpha/SKILL.md","type":"blob","sha":"blob-alpha"},{"path":"skills/beta","type":"tree","sha":"tree-beta"},{"path":"skills/beta/SKILL.md","type":"blob","sha":"blob-beta"}]}"#,
            },
        ]);
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
        assert!(!requests[1].to_ascii_lowercase().contains("if-none-match"));
    }

    #[tokio::test]
    async fn github_304_with_advanced_commit_publishes_new_ref_revision() {
        let fixture = HttpFixture::new(vec![
            HttpResponse {
                status: "200 OK",
                headers: vec![("Content-Type", "application/json")],
                body: r#"{"sha":"commit-v2"}"#,
            },
            HttpResponse {
                status: "304 Not Modified",
                headers: vec![],
                body: "",
            },
        ]);

        let outcome = github_detector(&fixture)
            .detect(
                github_request("skills/alpha"),
                Some(previous_github_evidence()),
                CancellationSignal::default(),
            )
            .await
            .unwrap();
        let EvidenceDetectionOutcome::Modified(facts) = outcome else {
            panic!("expected modified identity, got {outcome:?}");
        };

        assert_eq!(facts.snapshot_id.commit_revision, "commit-v2");
        assert_eq!(
            facts.skill_revisions.get("skills/alpha"),
            Some(&SkillRevision::GitTreeOid("tree-alpha".into()))
        );
    }

    #[tokio::test]
    async fn github_truncated_rate_limit_and_ambiguous_404_are_typed_failures() {
        let cases = [
            (
                vec![
                    HttpResponse {
                        status: "200 OK",
                        headers: vec![("Content-Type", "application/json")],
                        body: r#"{"sha":"commit-v1"}"#,
                    },
                    HttpResponse {
                        status: "200 OK",
                        headers: vec![("Content-Type", "application/json")],
                        body: r#"{"sha":"root-tree-v1","truncated":true,"tree":[]}"#,
                    },
                ],
                EvidenceFailureReason::IncompleteEvidence,
            ),
            (
                vec![
                    HttpResponse {
                        status: "200 OK",
                        headers: vec![("Content-Type", "application/json")],
                        body: r#"{"sha":"commit-v1"}"#,
                    },
                    HttpResponse {
                        status: "403 Forbidden",
                        headers: vec![("X-RateLimit-Remaining", "0"), ("Retry-After", "30")],
                        body: "",
                    },
                ],
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
    async fn clone_detector_discovers_all_paths_but_hashes_only_requested_skills() {
        let remote = BareSkillRepo::new(&["skills/alpha", "skills/beta"]);
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
        let detector = RuntimeSourceEvidenceDetector::new(
            payloads.clone(),
            Arc::new(EnvironmentRegistry::default()),
            Arc::new(SourceSnapshotReuseIndex::default()),
        );

        let outcome = detector
            .detect(
                EvidenceDetectionRequest {
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
        assert_eq!(evidence.snapshot_id.commit_revision.len(), 40);
        assert_eq!(evidence.complete_skill_path_catalog.len(), 2);
        assert!(matches!(
            evidence.skill_revisions.get("skills/alpha"),
            Some(SkillRevision::CliContentHash(_))
        ));
        assert!(!evidence.skill_revisions.contains_key("skills/beta"));
        assert_eq!(
            payloads
                .protected_session_ids(&EnvironmentRef::Host)
                .unwrap()
                .len(),
            1
        );
        assert_eq!(remote.clone_count(), 1);
    }

    #[tokio::test]
    async fn clone_detector_enriches_a_retained_snapshot_without_recloning() {
        let remote = BareSkillRepo::new(&["skills/alpha", "skills/beta"]);
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
        let detector = Arc::new(RuntimeSourceEvidenceDetector::new(
            payloads,
            Arc::new(EnvironmentRegistry::default()),
            snapshots.clone(),
        ));
        let coordinator = SourceEvidenceCoordinator::with_snapshot_reuse(detector, snapshots);
        let request = |path: &str| EvidenceCheckRequest {
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
        assert_eq!(remote.clone_count(), 1);
    }

    #[tokio::test]
    async fn clone_detector_reacquires_when_the_remote_ref_changes() {
        let remote = BareSkillRepo::new(&["skills/alpha"]);
        let parsed = ParsedSource {
            source_type: SourceType::Git,
            url: remote.source(),
            subpath: None,
            local_path: None,
            git_ref: Some("main".to_string()),
            skill_filter: None,
        };
        let identity = SourceIdentity::from_parsed(&parsed).unwrap();
        let detector = RuntimeSourceEvidenceDetector::new(
            payloads(),
            Arc::new(EnvironmentRegistry::default()),
            Arc::new(SourceSnapshotReuseIndex::default()),
        );
        let request = || EvidenceDetectionRequest {
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
        assert_eq!(remote.clone_count(), 2);
    }

    #[tokio::test]
    async fn clone_detector_does_not_reuse_after_remote_probe_failure() {
        let mut remote = BareSkillRepo::new(&["skills/alpha"]);
        let parsed = ParsedSource {
            source_type: SourceType::Git,
            url: remote.source(),
            subpath: None,
            local_path: None,
            git_ref: Some("main".to_string()),
            skill_filter: None,
        };
        let identity = SourceIdentity::from_parsed(&parsed).unwrap();
        let detector = RuntimeSourceEvidenceDetector::new(
            payloads(),
            Arc::new(EnvironmentRegistry::default()),
            Arc::new(SourceSnapshotReuseIndex::default()),
        );
        let request = || EvidenceDetectionRequest {
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
        remote.stop_upstream();

        let outcome = detector
            .detect(request(), Some(entry(first)), CancellationSignal::default())
            .await
            .unwrap();
        let EvidenceDetectionOutcome::Failed(failure) = outcome else {
            panic!("expected probe failure, got {outcome:?}");
        };

        assert_eq!(failure.reason, EvidenceFailureReason::Network);
        assert_eq!(remote.clone_count(), 1);
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

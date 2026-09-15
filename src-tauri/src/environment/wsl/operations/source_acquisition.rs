use tokio::time::Duration;

use crate::core::classify_git_failure;
use crate::core::mutation::CancellationSignal;
use crate::environment::wsl::{WslSession, WslWorkspace};
use crate::error::AppError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WslAcquisitionSource {
    Git {
        url: String,
        git_ref: Option<String>,
    },
    Local {
        native_path: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct WorkerSourceHandle {
    pub generation: u64,
    pub id: u64,
}

#[derive(Debug)]
pub struct WslNativeSource {
    workspace: WslWorkspace,
    handle: Option<WorkerSourceHandle>,
    native_root: String,
    managed_owner_registered: bool,
    ref_revision: Option<String>,
    managed_bytes: u64,
}

impl WslNativeSource {
    pub fn native_root(&self) -> &str {
        &self.native_root
    }

    pub fn ref_revision(&self) -> Option<&str> {
        self.ref_revision.as_deref()
    }

    pub fn managed_bytes(&self) -> u64 {
        self.managed_bytes
    }

    pub(crate) fn handle(&self) -> WorkerSourceHandle {
        self.handle
            .expect("active WSL source must own a Worker handle")
    }
}

impl Drop for WslNativeSource {
    fn drop(&mut self) {
        if let Some(handle) = self.handle.take() {
            self.workspace.defer_worker_source_release(handle);
        }
        if self.managed_owner_registered {
            self.workspace.release_source_owner();
        }
    }
}

pub async fn acquire_wsl_source_native(
    workspace: WslWorkspace,
    _session: &WslSession,
    source: WslAcquisitionSource,
    git_timeout: Duration,
    proxy: Option<String>,
    cancellation: CancellationSignal,
) -> Result<WslNativeSource, AppError> {
    let (message, source_url) = match source {
        WslAcquisitionSource::Git { url, git_ref } => (
            environment_protocol::Message::AcquireGitSource {
                request: environment_protocol::GitSourceRequest {
                    url: url.clone(),
                    git_ref,
                    proxy,
                    deadline_millis: duration_millis(git_timeout),
                },
            },
            Some(url),
        ),
        WslAcquisitionSource::Local { native_path } => {
            if !native_path.starts_with('/') {
                return Err(AppError::UnsafePath {
                    path: native_path,
                    reason: "WSL local Source must use an absolute POSIX path".to_string(),
                });
            }
            (
                environment_protocol::Message::OpenLocalSource {
                    request: environment_protocol::OpenLocalSourceRequest { path: native_path },
                },
                None,
            )
        }
    };
    let (generation, response) = workspace
        .request_worker_control_once(
            message,
            Some(cancellation),
            git_timeout.saturating_add(Duration::from_secs(5)),
        )
        .await
        .map_err(|error| map_transport_timeout(error, git_timeout))?;
    let (id, native_root, ref_revision, managed_bytes) = match response {
        environment_protocol::Message::SourceOpened {
            source_id,
            root,
            revision,
            managed_bytes,
        } => (source_id, root, revision, managed_bytes),
        environment_protocol::Message::Error {
            code,
            phase,
            parameters,
        } if source_url.is_some() => {
            return Err(map_git_error(
                &code,
                &phase,
                &parameters,
                source_url.as_deref().unwrap_or_default(),
                "clone",
                git_timeout,
            ));
        }
        environment_protocol::Message::Error { code, phase, .. } => {
            return Err(AppError::ExecutionFailed {
                message: format!("WSL Worker source request failed during {phase}: {code}"),
            });
        }
        _ => return Err(protocol_error("invalid WSL Worker SourceOpened response")),
    };
    workspace.register_source_owner()?;
    Ok(WslNativeSource {
        workspace,
        handle: Some(WorkerSourceHandle { generation, id }),
        native_root,
        managed_owner_registered: true,
        ref_revision,
        managed_bytes,
    })
}

pub(crate) async fn probe_wsl_git_connection(
    workspace: &WslWorkspace,
    url: &str,
    proxy: Option<String>,
    timeout: Duration,
    cancellation: CancellationSignal,
) -> Result<(), AppError> {
    probe_wsl_git_ref(workspace, url, None, proxy, timeout, cancellation)
        .await
        .map(|_| ())
}

pub(crate) async fn probe_wsl_git_ref(
    workspace: &WslWorkspace,
    url: &str,
    git_ref: Option<&str>,
    proxy: Option<String>,
    timeout: Duration,
    cancellation: CancellationSignal,
) -> Result<String, AppError> {
    let (_, response) = workspace
        .request_worker_control_once(
            environment_protocol::Message::ProbeGit {
                request: environment_protocol::GitSourceRequest {
                    url: url.to_string(),
                    git_ref: git_ref.map(str::to_string),
                    proxy,
                    deadline_millis: duration_millis(timeout),
                },
            },
            Some(cancellation),
            timeout.saturating_add(Duration::from_secs(5)),
        )
        .await
        .map_err(|error| map_transport_timeout(error, timeout))?;
    match response {
        environment_protocol::Message::GitProbed { revision } => Ok(revision),
        environment_protocol::Message::Error {
            code,
            phase,
            parameters,
        } => Err(map_git_error(
            &code,
            &phase,
            &parameters,
            url,
            "ls-remote",
            timeout,
        )),
        _ => Err(protocol_error("invalid WSL Worker GitProbed response")),
    }
}

fn map_git_error(
    code: &str,
    phase: &str,
    parameters: &[(String, String)],
    url: &str,
    operation: &str,
    timeout: Duration,
) -> AppError {
    if code == "deadlineExceeded" {
        return AppError::GitTimeout {
            timeout_secs: u32::try_from(timeout.as_secs()).unwrap_or(u32::MAX),
        };
    }
    let parameter = |name: &str| {
        parameters
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    };
    if code == "gitFailed" {
        let exit_code = parameter("exitCode").and_then(|value| value.parse().ok());
        return classify_git_failure(
            parameter("stderr").unwrap_or_default(),
            url,
            operation,
            exit_code,
        );
    }
    if code == "gitUnavailable" {
        return AppError::GitCloneFailed {
            message: parameter("message")
                .unwrap_or("Git is not available in the selected WSL distribution")
                .to_string(),
        };
    }
    AppError::ExecutionFailed {
        message: format!("WSL Worker Git request failed during {phase}: {code}"),
    }
}

fn duration_millis(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis())
        .unwrap_or(u64::MAX)
        .max(1)
}

fn map_transport_timeout(error: AppError, timeout: Duration) -> AppError {
    if error == AppError::WslCommandTimedOut {
        AppError::GitTimeout {
            timeout_secs: u32::try_from(timeout.as_secs()).unwrap_or(u32::MAX),
        }
    } else {
        error
    }
}

fn protocol_error(message: &str) -> AppError {
    AppError::ConfigurationCorrupted {
        message: message.to_string(),
    }
}

#[cfg(all(test, target_os = "windows"))]
#[allow(
    clippy::disallowed_methods,
    reason = "真实 WSL 2 门禁的 Drop guard 需要同步启动 wsl.exe 清理测试 fixture"
)]
mod windows_wsl2_tests {
    use std::process::Stdio;

    use crate::application::payload_session::{PayloadSessionStorage, PayloadStorageKey};
    use crate::core::mutation::CancellationSignal;
    use crate::environment::wsl::operations::acquire::WslPayloadSessionStorage;
    use crate::environment::wsl::operations::scan::{scan, ScanRequest};
    use crate::environment::wsl::WslRuntime;

    #[tokio::test]
    #[ignore = "requires Windows with an Ubuntu WSL 2 distribution"]
    async fn real_wsl2_worker_completes_git_scan_payload_and_release() {
        let distro =
            std::env::var("SKILL_DECK_TEST_WSL_DISTRO").unwrap_or_else(|_| "Ubuntu".to_string());
        let fixture = format!(
            "/tmp/skill-deck-worker-gate-{}",
            uuid::Uuid::new_v4().simple()
        );
        run_fixture_command(
            &distro,
            r#"set -eu
root=$1
mkdir -p "$root"
git init -b main "$root"
git -C "$root" config user.email test@example.com
git -C "$root" config user.name 'Skill Deck Test'
printf '%s\n' '---' 'name: worker-gate' 'description: Worker gate' '---' > "$root/SKILL.md"
mkdir -p "$root/one/two/three/four/five/six/deep"
printf '%s\n' '---' 'name: deep-gate' 'description: Deep gate' '---' > "$root/one/two/three/four/five/six/deep/skill.md"
ln -s one/two/three/four/five/six "$root/alias"
git -C "$root" add .
git -C "$root" commit -m fixture
git -C "$root" tag -a release -m release
git -C "$root" commit --allow-empty -m branch
git -C "$root" branch release
"#,
            &fixture,
        )
        .await;
        let _fixture_cleanup = FixtureCleanup {
            distro: distro.clone(),
            fixture: fixture.clone(),
        };

        let runtime = WslRuntime::for_wsl_test();
        let workspace = runtime.workspace(&distro).unwrap();
        let session = runtime.connect(&distro).await.unwrap();
        let source = super::acquire_wsl_source_native(
            workspace.clone(),
            &session,
            super::WslAcquisitionSource::Git {
                url: fixture.clone(),
                git_ref: None,
            },
            std::time::Duration::from_secs(30),
            None,
            CancellationSignal::default(),
        )
        .await
        .unwrap();
        let branch = super::probe_wsl_git_ref(
            &workspace,
            &fixture,
            Some("release"),
            None,
            std::time::Duration::from_secs(30),
            CancellationSignal::default(),
        )
        .await
        .unwrap();
        let tag = super::probe_wsl_git_ref(
            &workspace,
            &fixture,
            Some("refs/tags/release"),
            None,
            std::time::Duration::from_secs(30),
            CancellationSignal::default(),
        )
        .await
        .unwrap();
        assert_eq!(source.ref_revision(), Some(branch.as_str()));
        assert_ne!(branch, tag);
        assert!(source.managed_bytes() > 0);
        let inventory = scan(
            &workspace,
            &source,
            ScanRequest {
                roots: vec![source.native_root().to_string()],
                stat_only_root_indexes: Default::default(),
                recursive: true,
                per_file_limit: 256 * 1024,
                aggregate_limit: 1024 * 1024,
            },
            None,
        )
        .await
        .unwrap();
        assert!(inventory
            .entries
            .iter()
            .any(|entry| entry.relative_path == "SKILL.md"));

        let storage = WslPayloadSessionStorage::for_source(workspace, &source);
        let deep = storage
            .read_source_skill_md(
                &format!("{}/one/two/three/four/five/six/deep", source.native_root()),
                CancellationSignal::default(),
            )
            .await
            .unwrap();
        assert!(String::from_utf8(deep).unwrap().contains("name: deep-gate"));
        assert!(matches!(storage.read_source_skill_md(
            &format!("{}/alias/deep", source.native_root()), CancellationSignal::default()
        ).await, Err(crate::error::AppError::CapabilityUnavailable { capability, .. }) if capability == "sourceDirectoryLinks"));
        let key = PayloadStorageKey::new("worker-gate", "SKILL.md");
        let acquired = storage
            .acquire_from_source_path(&key, source.native_root(), None)
            .await
            .unwrap();
        assert_eq!(
            storage.verify(&key).await.unwrap().unwrap(),
            acquired.manifest
        );
        let blob_id = acquired
            .manifest
            .entries
            .iter()
            .find_map(|entry| entry.blob_id.as_deref())
            .unwrap();
        assert!(!storage
            .read_blob(&key, blob_id)
            .await
            .unwrap()
            .unwrap()
            .is_empty());
        storage.remove(&key).await.unwrap();
        storage.remove_session(key.session_id()).await.unwrap();
        crate::application::payload_session::RetainedSourceCleanup::remove(&storage)
            .await
            .unwrap();
        drop(source);
    }

    #[tokio::test]
    #[ignore = "requires Windows with an Ubuntu WSL 2 distribution"]
    async fn real_windows_http_payload_executes_in_wsl_after_source_release() {
        use crate::application::mutation::coordinator::PreparedUnitExecutor;
        use crate::application::mutation::plan::{
            ExecutionUnit, ExpectedTargetEntry, PreparedEntryAction, PreparedEntryMutation,
            RuntimeRevisions,
        };
        use crate::application::payload_session::{
            DiscoverySourceLocation, PayloadSessionLimits, PayloadSessionManager,
        };
        use crate::application::source_acquisition::{
            retain_discovered_source, AcquireSelectedPayloadsRequest, InternalSkillVisibility,
            ManagedDownloadedDirectory, RetainedSourceOptions, SelectedPayloadAcquisitionService,
        };
        use crate::environment::content_manifest::{ContentManifestReader, ContentManifestTarget};
        use crate::environment::planning::{RuntimeTargetFactResolver, TargetFactResolver};
        use crate::environment::runtime::ContextSnapshotRevision;
        use crate::environment::types::{
            EnvironmentRef, ResourceLocator, SkillLocation, SkillLocationRef,
        };
        use std::collections::BTreeMap;
        use std::sync::Arc;

        let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
        let url = format!(
            "http://{}/.well-known/agent-skills/index.json?scope=test",
            server.server_addr()
        );
        let server_task = std::thread::spawn(move || {
            for _ in 0..2 {
                let request = server
                    .recv_timeout(std::time::Duration::from_secs(10))
                    .unwrap()
                    .unwrap();
                let body = if request.url().contains("index.json?scope=test") {
                    r#"{"skills":[{"name":"http-gate","description":"HTTP gate","files":["SKILL.md"]}]}"#
                } else {
                    assert_eq!(
                        request.url(),
                        "/.well-known/agent-skills/http-gate/SKILL.md"
                    );
                    "---\nname: http-gate\ndescription: HTTP gate\n---\nprepared content"
                };
                request
                    .respond(tiny_http::Response::from_string(body))
                    .unwrap();
            }
        });
        let settings = Arc::new(crate::runtime::proxy_settings::ProxySettingsStore::new(
            crate::models::NetworkProxySettings::default(),
        ));
        let http = crate::runtime::http_transport::HttpTransport::new(settings);
        let fetched =
            crate::runtime::wellknown_protocol::fetch_selected_wellknown_skills_with_client(
                &http,
                &url,
                Some(&["http-gate".to_string()]),
                &CancellationSignal::default(),
            )
            .await
            .unwrap();
        server_task.join().unwrap();
        let native_root = fetched.repo_path.clone();
        let distro =
            std::env::var("SKILL_DECK_TEST_WSL_DISTRO").unwrap_or_else(|_| "Ubuntu".to_string());
        let fixture = format!(
            "/tmp/skill-deck-http-update-gate-{}",
            uuid::Uuid::new_v4().simple()
        );
        run_fixture_command(
            &distro,
            "mkdir -p \"$1/http-gate\"\nprintf old > \"$1/http-gate/SKILL.md\"",
            &fixture,
        )
        .await;
        let _cleanup = FixtureCleanup {
            distro: distro.clone(),
            fixture: fixture.clone(),
        };
        let runtime = Arc::new(WslRuntime::for_wsl_test());
        let workspace = runtime.workspace(&distro).unwrap();
        let session = runtime.connect(&distro).await.unwrap();
        let environment = EnvironmentRef::Wsl {
            distro_name: distro,
        };
        let storage = Arc::new(WslPayloadSessionStorage::new(workspace.clone()));
        let manager = Arc::new(PayloadSessionManager::new(
            storage.clone(),
            PayloadSessionLimits {
                ttl_ms: 60_000,
                max_sessions: 4,
                max_bytes: 2_000_000,
            },
            || 1_000,
        ));
        let parsed = crate::models::ParsedSource {
            source_type: crate::models::SourceType::WellKnown,
            ..crate::core::parse_source(&url).unwrap()
        };
        let discovery = retain_discovered_source(
            manager.clone(),
            environment.clone(),
            parsed,
            url,
            DiscoverySourceLocation::Native {
                root: native_root.clone(),
                ref_revision: None,
            },
            native_root.clone(),
            ManagedDownloadedDirectory::new(native_root.clone()),
            RetainedSourceOptions {
                storage: Some(storage.clone()),
                trust_metadata: Some(fetched.trust_metadata),
                full_depth: true,
                internal_skill_visibility: InternalSkillVisibility::All,
                ..Default::default()
            },
        )
        .await
        .unwrap()
        .discovery_session;
        let handle = SelectedPayloadAcquisitionService::new(manager.clone())
            .acquire(AcquireSelectedPayloadsRequest {
                discovery_session: discovery.clone(),
                skill_paths: vec!["http-gate/SKILL.md".into()],
            })
            .await
            .unwrap()
            .remove(0);
        let lease = manager.pin_verified(&handle).await.unwrap();
        let expected = lease.manifest().payload_root_hash.clone();
        let payload_id = lease.manifest().payload_id().clone();
        manager.release_source_snapshot(&discovery).await.unwrap();
        assert!(!native_root.exists());
        let target = ResourceLocator {
            environment: environment.clone(),
            native_path: format!("{fixture}/http-gate"),
        };
        let targets = RuntimeTargetFactResolver::new(runtime);
        let fact = targets
            .resolve_environment(&environment, std::slice::from_ref(&target), None)
            .await
            .unwrap()
            .remove(0);
        let manifest = targets
            .read(&ContentManifestTarget {
                key: fact.key.clone(),
                location: fact.destination.clone(),
            })
            .await
            .unwrap();
        let unit = ExecutionUnit {
            id: "http-update-gate".into(),
            skill_name: "http-gate".into(),
            source: None,
            target: SkillLocationRef {
                environment,
                scope: SkillLocation::Global,
            },
            expected_revisions: RuntimeRevisions {
                registry: "test".into(),
                environment: "test".into(),
                context: ContextSnapshotRevision::parse("context-v1-http-gate").unwrap(),
            },
            primary_entry: Some(PreparedEntryMutation {
                key: fact.key.clone(),
                destination: fact.destination,
                action: PreparedEntryAction::Replace {
                    payload_id: payload_id.clone(),
                    requested_mode: crate::models::InstallMode::Copy,
                },
                reader_agent_ids: Vec::new(),
            }),
            additional_entries: Vec::new(),
            lock_mutation: None,
            expected_targets: vec![ExpectedTargetEntry {
                key: fact.key,
                fingerprint: fact.fingerprint,
                expected_content_manifest_hash: Some(manifest.hash().clone()),
            }],
        };
        let payloads = BTreeMap::from([(payload_id, lease)]);
        let executor = crate::environment::wsl::operations::materialize::WslPreparedUnitExecutor::for_operation(session, workspace.clone(), "http-update-gate", crate::core::mutation::MutationKind::Update);
        let prepared = executor
            .prepare(&unit, &payloads, CancellationSignal::default())
            .await
            .unwrap();
        executor
            .execute(prepared, None, CancellationSignal::default())
            .await
            .unwrap();
        let verification_key = PayloadStorageKey::new(
            format!("verify-{}", uuid::Uuid::new_v4().simple()),
            "installed",
        );
        let verification = storage
            .acquire_from_path(&verification_key, &target.native_path, None)
            .await
            .unwrap();
        assert_eq!(verification.manifest.payload_root_hash, expected);
        storage
            .remove_session(verification_key.session_id())
            .await
            .unwrap();
        drop(payloads);
        manager.retire_wsl_sessions();
        manager.cleanup().await.unwrap();
    }

    async fn run_fixture_command(distro: &str, script: &str, fixture: &str) {
        let status = crate::environment::wsl::wsl_command()
            .args([
                "--distribution",
                distro,
                "--exec",
                "/bin/sh",
                "-c",
                script,
                "--",
                fixture,
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .status()
            .await
            .unwrap();
        assert!(status.success());
    }

    struct FixtureCleanup {
        distro: String,
        fixture: String,
    }

    impl Drop for FixtureCleanup {
        fn drop(&mut self) {
            let _ = std::process::Command::new("wsl.exe")
                .args([
                    "--distribution",
                    &self.distro,
                    "--exec",
                    "/bin/rm",
                    "-rf",
                    "--",
                    &self.fixture,
                ])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }
    }
}

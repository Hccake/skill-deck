use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use serde_json::{json, Value};

use crate::application::copy::{CopyExecutionRequest, CopyRequest, CopyService};
use crate::application::copy_runtime::RuntimeCopyProjectComparator;
use crate::application::install::{InstallRequest, InstallService};
use crate::application::install_planner::ConcreteInstallPlanner;
use crate::application::manage_agents::{
    ManageAgentsPreviewRequest, ManageAgentsRequest, ManageAgentsService,
};
use crate::application::mutation::coordinator::RuntimeRevisionSource;
use crate::application::payload_session::{
    AcquiredPayloadHandle, PayloadPlanningMetadata, PayloadSessionLimits,
    PayloadSessionMaintenance, PayloadSessionManager, PayloadSessionStorage,
};
use crate::application::plan_runner::{RuntimeExecutionDependencies, RuntimePlanExecutor};
use crate::application::recovery::RecoveryResourceState;
use crate::application::recovery_runtime::RuntimeRecoveryGraph;
use crate::application::remove::{RemoveIntent, RemoveRequest, RemoveService};
use crate::application::runtime_facts::{HostRuntimeSnapshot, RuntimePlanningFactSource};
use crate::application::skill_entries::{InstalledSkillPayloadAcquirer, SkillEntryObserver};
use crate::application::source_evidence::{
    EvidenceDetectionOutcome, EvidenceDetectionRequest, EvidenceFuture, RemoteEvidenceObservation,
    RemoteSnapshotId, SkillRevision, SourceEvidenceCoordinator, SourceEvidenceDetector,
};
use crate::application::source_snapshot_reuse::{PayloadAcquisitionKey, SourceSnapshotReuseIndex};
use crate::application::update::{
    UpdateCheckMode, UpdateCheckRequest, UpdateCheckSelection, UpdateExecutionRequest,
    UpdateOutcome, UpdateRequest, UpdateService, UpdateSourceStatus,
};
use crate::application::update_check::UpdateCheckService;
use crate::application::update_planner::ConcreteUpdatePlanner;
use crate::core::mutation::CancellationSignal;
use crate::core::{parse_source, SourceIdentity};
use crate::environment::acquisition::{acquire_wsl_source_native, WslAcquisitionSource};
use crate::environment::content_manifest::{
    ContentManifest, ContentManifestReader, ContentManifestTarget,
};
use crate::environment::lock_io::EnvironmentLockIo;
use crate::environment::native::content_manifest::NativeContentManifestReader;
use crate::environment::native::tree::project_target;
use crate::environment::planning::RuntimeTargetFactResolver;
use crate::environment::recovery::{
    RecoveryEntryPhase, RecoveryExpectedEntryState, RecoveryMarker, RecoveryMarkerEntry,
    RecoveryMarkerKind, RecoveryMarkerStore, RECOVERY_MARKER_SCHEMA_VERSION,
};
use crate::environment::runtime::{ExecutionBackend, PhysicalParentIdentity, PhysicalTargetKey};
use crate::environment::types::{ContextRef, ContextScope, EnvironmentRef, ResourceLocator};
use crate::environment::wsl::operations::acquire::WslPayloadSessionStorage;
use crate::environment::wsl::operations::content_manifest::WslContentManifestReader;
use crate::environment::wsl::operations::projection::project_targets;
use crate::environment::wsl::{connect_wsl_environment, EnvironmentRegistry, WslSession};
use crate::environment::wsl_protocol::run_wsl_script;
use crate::error::{AppError, RecoveryResourceId};
use crate::models::InstallMode;
use crate::native_workflow_integration_support::{
    assert_succeeded, both_agent_intents, create_payload, fixed_time, test_registry,
    FixedUpdateAcquirer, StaticRegistry,
};
use uuid::Uuid;

const TIMEOUT: tokio::time::Duration = tokio::time::Duration::from_secs(30);

struct CountingEvidenceDetector(AtomicUsize);

impl SourceEvidenceDetector for CountingEvidenceDetector {
    fn detect<'a>(
        &'a self,
        request: EvidenceDetectionRequest,
        _previous: Option<crate::application::source_evidence::RemoteEvidenceEntry>,
        _cancellation: CancellationSignal,
    ) -> EvidenceFuture<'a> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Box::pin(async move {
            let requested_skill_paths = request.requested_skill_paths;
            Ok(EvidenceDetectionOutcome::Modified(
                RemoteEvidenceObservation {
                    snapshot_id: RemoteSnapshotId::new(
                        request.key.normalized_ref,
                        "main",
                        "fixture-revision",
                    ),
                    provider_validation: None,
                    complete_skill_path_catalog: requested_skill_paths.clone(),
                    skill_revisions: requested_skill_paths
                        .into_iter()
                        .map(|path| {
                            (
                                path.clone(),
                                SkillRevision::GitTreeOid(format!("fixture-{path}")),
                            )
                        })
                        .collect(),
                    snapshot_facts: None,
                },
            ))
        })
    }
}

struct WslWorkflowHarness {
    _host: tempfile::TempDir,
    session: WslSession,
    root: String,
    source_project: String,
    target_project: String,
    environments: Arc<EnvironmentRegistry>,
    facts: RuntimePlanningFactSource,
    targets: RuntimeTargetFactResolver,
    payloads: Arc<PayloadSessionManager>,
    execution: RuntimeExecutionDependencies,
}

impl WslWorkflowHarness {
    async fn new(mut session: WslSession, root: String) -> Result<Self, AppError> {
        let host = tempfile::tempdir()?;
        let source_project = format!("{root}/source-project");
        let target_project = format!("{root}/target-project");
        session.home = format!("{root}/home");
        session.config_home = format!("{root}/home/.config");
        session.xdg_state_home = None;
        run_wsl_script(
            &session,
            r#"mkdir -p -- "$1/.skill-deck" "$2/.builtin" "$2/.custom" "$3/.builtin" "$3/.custom""#,
            &[
                session.home.clone(),
                source_project.clone(),
                target_project.clone(),
            ],
            Vec::new(),
            TIMEOUT,
        )
        .await?;
        let io = EnvironmentLockIo::Wsl(session.clone());
        io.write_atomic(
            &locator(
                &session,
                format!("{}/.skill-deck/projects.json", session.home),
            ),
            serde_json::to_vec_pretty(&json!({
                "schemaVersion": 1,
                "projects": [
                    {
                        "id": "source",
                        "nativePath": source_project.clone(),
                        "displayName": "Source",
                        "order": 0,
                        "suppressCrossStorageWarning": false
                    },
                    {
                        "id": "target",
                        "nativePath": target_project.clone(),
                        "displayName": "Target",
                        "order": 1,
                        "suppressCrossStorageWarning": false
                    }
                ]
            }))?,
        )
        .await?;
        io.write_atomic(
            &locator(&session, format!("{source_project}/skills-lock.json")),
            serde_json::to_vec_pretty(&json!({
                "version": 1,
                "futureRoot": { "keep": true },
                "skills": {}
            }))?,
        )
        .await?;
        io.write_atomic(
            &locator(&session, format!("{target_project}/skills-lock.json")),
            serde_json::to_vec_pretty(&json!({
                "version": 1,
                "targetFutureRoot": "keep",
                "skills": {}
            }))?,
        )
        .await?;

        let environments = Arc::new(EnvironmentRegistry::default());
        environments.insert(session.clone());
        let registry = Arc::new(StaticRegistry(Arc::new(test_registry())));
        let facts = RuntimePlanningFactSource::with_host_snapshot(
            registry,
            environments.clone(),
            HostRuntimeSnapshot {
                home: host.path().join("host-home"),
                config_home: host.path().join("host-config"),
                projects_path: host.path().join("host-projects.json"),
                global_lock_path: host.path().join("host-lock.json"),
                environment_variables: BTreeMap::new(),
            },
        );
        let targets = RuntimeTargetFactResolver::new(environments.clone());
        let payloads = Arc::new(PayloadSessionManager::new(
            Arc::new(WslPayloadSessionStorage::new(session.clone())),
            PayloadSessionLimits {
                ttl_ms: 60_000,
                max_sessions: 16,
                max_bytes: 16 * 1024 * 1024,
            },
            || 1_000,
        ));
        let execution = RuntimeExecutionDependencies::new(
            environments.clone(),
            host.path().join("native-recovery"),
        )?;
        Ok(Self {
            _host: host,
            session,
            root,
            source_project,
            target_project,
            environments,
            facts,
            targets,
            payloads,
            execution,
        })
    }

    fn context(&self, project_id: &str) -> ContextRef {
        ContextRef {
            environment: EnvironmentRef::Wsl {
                distro_name: self.session.distro_name.clone(),
            },
            scope: ContextScope::Project {
                project_id: project_id.to_string(),
            },
        }
    }

    async fn payload(
        &self,
        version: &str,
        computed_hash: &str,
        remote_hash: &str,
    ) -> Result<AcquiredPayloadHandle, AppError> {
        let source = self._host.path().join(format!("payload-{version}/demo"));
        let payload = create_payload(&source, version)?;
        let discovery = self
            .payloads
            .discover(
                self.context("source").environment,
                format!("source-{version}"),
            )
            .await?;
        self.payloads
            .acquire_payload_with_metadata(
                &discovery,
                "skills/demo",
                payload,
                PayloadPlanningMetadata {
                    skill_name: "demo".to_string(),
                    install_dir_name: "demo".to_string(),
                    source: "owner/repo".to_string(),
                    source_type: "github".to_string(),
                    source_url: Some("https://github.com/owner/repo.git".to_string()),
                    ref_name: Some("main".to_string()),
                    skill_path: "skills/demo".to_string(),
                    plugin_name: Some("wsl-integration".to_string()),
                    computed_hash: computed_hash.to_string(),
                    upstream_revision: Some(remote_hash.to_string()),
                },
            )
            .await
    }

    fn executor(&self) -> RuntimePlanExecutor {
        let revisions: Arc<dyn RuntimeRevisionSource> = Arc::new(self.facts.clone());
        self.execution
            .executor(self.environments.clone(), revisions)
    }

    async fn install_preview(
        &self,
        handle: AcquiredPayloadHandle,
    ) -> Result<
        (
            InstallService<
                ConcreteInstallPlanner<RuntimePlanningFactSource, RuntimeTargetFactResolver>,
                RuntimePlanExecutor,
            >,
            InstallRequest,
            crate::application::mutation::plan::PreviewToken,
        ),
        AppError,
    > {
        let service = InstallService::new(
            self.payloads.clone(),
            ConcreteInstallPlanner::new(
                self.facts.clone(),
                self.targets.clone(),
                self.payloads.clone(),
                fixed_time,
            ),
            self.executor(),
        );
        let request = InstallRequest {
            context: self.context("source"),
            source: "owner/repo".to_string(),
            discovery_session: crate::application::payload_session::DiscoverySessionHandle {
                session_id: handle.session_id.clone(),
                environment: handle.environment.clone(),
                source_fingerprint: handle.source_fingerprint.clone(),
                expires_at_epoch_ms: handle.expires_at_epoch_ms,
            },
            payloads: vec![handle],
            skills: vec!["demo".to_string()],
            agent_intents: both_agent_intents(),
            requested_mode: InstallMode::Copy,
            acknowledge_risk: true,
        };
        let preview = service.preview(&request).await?;
        Ok((service, request, preview.token))
    }

    async fn json(&self, path: String) -> Result<Value, AppError> {
        let bytes = EnvironmentLockIo::Wsl(self.session.clone())
            .read_optional(&locator(&self.session, path))
            .await?
            .ok_or_else(|| AppError::PathNotFound {
                path: "WSL integration lock".to_string(),
            })?;
        Ok(serde_json::from_slice(&bytes)?)
    }

    async fn exists(&self, path: String) -> Result<bool, AppError> {
        let output = run_wsl_script(
            &self.session,
            r#"if [ -e "$1" ] || [ -L "$1" ]; then printf '1'; else printf '0'; fi"#,
            &[path],
            Vec::new(),
            TIMEOUT,
        )
        .await?;
        Ok(output == b"1")
    }

    async fn read(&self, path: String) -> Result<Vec<u8>, AppError> {
        run_wsl_script(
            &self.session,
            r#"cat -- "$1""#,
            &[path],
            Vec::new(),
            TIMEOUT,
        )
        .await
    }
}

async fn wsl_manifest(
    harness: &WslWorkflowHarness,
    path: String,
) -> Result<ContentManifest, AppError> {
    let projected = project_targets(&harness.session, std::slice::from_ref(&path), None)
        .await?
        .into_iter()
        .next()
        .ok_or_else(|| AppError::StaleTarget)?;
    WslContentManifestReader::new(harness.session.clone())
        .read(&ContentManifestTarget {
            key: PhysicalTargetKey {
                backend: ExecutionBackend::WslPosix {
                    distro_name: crate::environment::types::normalized_wsl_distro_name(
                        &harness.session.distro_name,
                    ),
                },
                physical_parent: PhysicalParentIdentity::Wsl {
                    distro_name: crate::environment::types::normalized_wsl_distro_name(
                        &harness.session.distro_name,
                    ),
                    device: projected.anchor_device,
                    inode: projected.anchor_inode,
                },
                normalized_final_child_name: projected.relative_components.join("/"),
            },
            location: locator(&harness.session, projected.physical_destination),
        })
        .await
}

async fn assert_wsl_update_contracts(harness: &WslWorkflowHarness) -> Result<(), AppError> {
    let identity = SourceIdentity::from_parsed(&parse_source("owner/repo#main")?)?;
    let detector = Arc::new(CountingEvidenceDetector(AtomicUsize::new(0)));
    let coordinator = SourceEvidenceCoordinator::new(detector.clone());
    let lock = serde_json::to_vec_pretty(&json!({
        "version": 1,
        "skills": {
            "demo": {
                "source": "owner/repo",
                "sourceType": "github",
                "sourceUrl": "https://github.com/owner/repo.git",
                "ref": "main",
                "skillPath": "skills/demo",
                "remoteHash": "previous-revision"
            }
        }
    }))?;
    let host_home = harness._host.path().join("host-home");
    let host_config = harness._host.path().join("host-config");
    std::fs::create_dir_all(&host_home)?;
    std::fs::create_dir_all(&host_config)?;
    std::fs::write(harness._host.path().join("host-lock.json"), &lock)?;
    let wsl_lock = locator(
        &harness.session,
        format!("{}/skills-lock.json", harness.source_project),
    );
    EnvironmentLockIo::Wsl(harness.session.clone())
        .write_atomic(&wsl_lock, lock)
        .await?;

    let service = UpdateCheckService::new(harness.facts.clone(), coordinator);
    let check_result = async {
        let host_context = ContextRef {
            environment: EnvironmentRef::Host,
            scope: ContextScope::Global,
        };
        let host = service
            .check(&UpdateCheckRequest {
                context: host_context,
                mode: UpdateCheckMode::Force,
                selection: UpdateCheckSelection::All,
            })
            .await?;
        let wsl = service
            .check(&UpdateCheckRequest {
                context: harness.context("source"),
                mode: UpdateCheckMode::Automatic,
                selection: UpdateCheckSelection::All,
            })
            .await?;
        if host.sources.len() != 1
            || host.skills.len() != 1
            || wsl.sources.len() != 1
            || wsl.skills.len() != 1
        {
            return Err(AppError::Custom {
                message: "Host/WSL update checks did not resolve both runtime Contexts".to_string(),
            });
        }
        Ok::<_, AppError>(())
    }
    .await;
    EnvironmentLockIo::Wsl(harness.session.clone())
        .write_atomic(
            &wsl_lock,
            serde_json::to_vec_pretty(&json!({
                "version": 1,
                "futureRoot": { "keep": true },
                "skills": {}
            }))?,
        )
        .await?;
    check_result?;
    if detector.0.load(Ordering::SeqCst) != 1 {
        return Err(AppError::Custom {
            message: "Host/WSL evidence did not share one remote observation".to_string(),
        });
    }

    let snapshots = SourceSnapshotReuseIndex::default();
    let host_payloads = PayloadSessionManager::in_memory(
        PayloadSessionLimits {
            ttl_ms: 60_000,
            max_sessions: 2,
            max_bytes: 1024,
        },
        || 1_000,
    );
    let host_discovery = host_payloads
        .discover(EnvironmentRef::Host, "host-source")
        .await?;
    let host_key = PayloadAcquisitionKey::from_identity(&identity, &EnvironmentRef::Host);
    snapshots.remember(host_key, "fixture-revision".to_string(), host_discovery);
    let wsl_key =
        PayloadAcquisitionKey::from_identity(&identity, &harness.context("source").environment);
    if snapshots
        .candidate(&wsl_key, harness.payloads.as_ref())
        .is_some()
    {
        return Err(AppError::Custom {
            message: "Host snapshot leaked into WSL payload storage".to_string(),
        });
    }

    assert_active_wsl_clone_cancellation(harness).await?;

    let native_root = harness._host.path().join("manifest-parity");
    std::fs::create_dir_all(native_root.join("nested"))?;
    std::fs::write(native_root.join("SKILL.md"), b"manifest fixture\n")?;
    std::fs::write(native_root.join("nested/guide.md"), b"guide fixture\n")?;
    let wsl_root = format!("{}/manifest-parity", harness.root);
    run_wsl_script(
        &harness.session,
        r#"mkdir -p -- "$1/nested"; printf 'manifest fixture\n' > "$1/SKILL.md"; printf 'guide fixture\n' > "$1/nested/guide.md""#,
        std::slice::from_ref(&wsl_root),
        Vec::new(),
        TIMEOUT,
    )
    .await?;
    let native_target = project_target(
        &native_root,
        if cfg!(windows) {
            ExecutionBackend::NativeWindows
        } else {
            ExecutionBackend::NativeUnix
        },
    )?;
    let native_manifest = NativeContentManifestReader
        .read(&ContentManifestTarget {
            key: native_target.key,
            location: ResourceLocator {
                environment: EnvironmentRef::Host,
                native_path: native_root.to_string_lossy().into_owned(),
            },
        })
        .await?;
    if native_manifest.hash() != wsl_manifest(harness, wsl_root).await?.hash() {
        return Err(AppError::Custom {
            message: "Native and WSL content manifests diverged".to_string(),
        });
    }
    Ok(())
}

async fn assert_active_wsl_clone_cancellation(
    harness: &WslWorkflowHarness,
) -> Result<(), AppError> {
    let previous_ext_policy = run_wsl_script(
        &harness.session,
        r#"git config --global --get protocol.ext.allow 2>/dev/null || :"#,
        &[],
        Vec::new(),
        TIMEOUT,
    )
    .await?;
    run_wsl_script(
        &harness.session,
        r#"git config --global protocol.ext.allow always"#,
        &[],
        Vec::new(),
        TIMEOUT,
    )
    .await?;

    let assertion = run_active_wsl_clone_cancellation(harness).await;
    let restore = run_wsl_script(
        &harness.session,
        r#"git config --global --unset-all protocol.ext.allow >/dev/null 2>&1 || :
if [ -n "$1" ]; then git config --global protocol.ext.allow "$1"; fi"#,
        &[String::from_utf8_lossy(&previous_ext_policy)
            .trim()
            .to_string()],
        Vec::new(),
        TIMEOUT,
    )
    .await;
    assertion?;
    restore.map(|_| ())
}

async fn run_active_wsl_clone_cancellation(harness: &WslWorkflowHarness) -> Result<(), AppError> {
    let fixture_root = format!("{}/clone-cancellation", harness.root);
    let wrapper = format!("{fixture_root}/gate-upload-pack.sh");
    let bare_repo = format!("{fixture_root}/remote.git");
    let started = format!("{fixture_root}/clone-started");
    let child_pid = format!("{fixture_root}/clone-child.pid");
    run_wsl_script(
        &harness.session,
        r#"set -eu
root=$1
wrapper=$2
rm -rf -- "$root"
mkdir -p -- "$root/work/skills/demo"
git init -q -b main "$root/work"
git -C "$root/work" config user.email test@example.com
git -C "$root/work" config user.name 'Skill Deck Test'
git -C "$root/work" config commit.gpgsign false
printf '%s\n' '# Demo' > "$root/work/skills/demo/SKILL.md"
git -C "$root/work" add -A
git -C "$root/work" commit -q -m initial
git clone -q --bare "$root/work" "$root/remote.git"
cat > "$wrapper" <<'EOF'
#!/bin/sh
root=${0%/*}
printf '%s\n' "$$" > "$root/clone-child.pid"
printf 'started\n' > "$root/clone-started"
trap 'exit 143' HUP INT TERM
while :; do sleep 1; done
EOF
chmod 700 "$wrapper""#,
        &[fixture_root.clone(), wrapper.clone()],
        Vec::new(),
        TIMEOUT,
    )
    .await?;
    let roots_before = managed_acquisition_roots(&harness.session).await?;
    let cancellation = CancellationSignal::default();
    let cancellation_for_task = cancellation.clone();
    let acquisition_session = harness.session.clone();
    let source_url = format!("ext::{wrapper} %S {bare_repo}");
    let acquisition = tokio::spawn(async move {
        acquire_wsl_source_native(
            &acquisition_session,
            WslAcquisitionSource::Git {
                url: source_url,
                git_ref: Some("main".to_string()),
            },
            cancellation_for_task,
        )
        .await
    });
    let started_result = run_wsl_script(
        &harness.session,
        r#"count=0
while [ ! -s "$1" ]; do
  count=$((count + 1))
  [ "$count" -lt 200 ] || exit 1
  sleep 0.05
done"#,
        std::slice::from_ref(&started),
        Vec::new(),
        tokio::time::Duration::from_secs(15),
    )
    .await;
    if let Err(error) = started_result {
        cancellation.cancel();
        let _ = tokio::time::timeout(TIMEOUT, acquisition).await;
        return Err(error);
    }
    let roots_during = match managed_acquisition_roots(&harness.session).await {
        Ok(roots) => roots,
        Err(error) => {
            cancellation.cancel();
            let _ = tokio::time::timeout(TIMEOUT, acquisition).await;
            return Err(error);
        }
    };
    let active_roots = roots_during
        .difference(&roots_before)
        .cloned()
        .collect::<Vec<_>>();
    if active_roots.len() != 1 {
        cancellation.cancel();
        let _ = tokio::time::timeout(TIMEOUT, acquisition).await;
        return Err(AppError::Custom {
            message: format!(
                "expected one active managed WSL acquisition directory, found {active_roots:?}"
            ),
        });
    }
    let active_root = active_roots[0].clone();

    cancellation.cancel();
    let acquisition_result = tokio::time::timeout(TIMEOUT, acquisition)
        .await
        .map_err(|_| AppError::ExecutionFailed {
            message: "cancelled WSL clone did not stop before the timeout".to_string(),
        })?
        .map_err(|error| AppError::ExecutionFailed {
            message: format!("cancelled WSL clone task failed: {error}"),
        })?;
    let cancellation_error = match acquisition_result {
        Err(error) => error,
        Ok(_) => {
            return Err(AppError::Custom {
                message: "active WSL clone completed after cancellation".to_string(),
            });
        }
    };
    if !matches!(cancellation_error, AppError::MutationCancelled) {
        return Err(cancellation_error);
    }

    let termination = run_wsl_script(
        &harness.session,
        r#"pid=$(cat -- "$1")
count=0
while kill -0 "$pid" 2>/dev/null; do
  count=$((count + 1))
  [ "$count" -lt 200 ] || exit 1
  sleep 0.05
done
[ ! -e "$2" ] && [ ! -L "$2" ]"#,
        &[child_pid.clone(), active_root.clone()],
        Vec::new(),
        tokio::time::Duration::from_secs(15),
    )
    .await;
    if let Err(error) = termination {
        let _ = run_wsl_script(
            &harness.session,
            r#"pid=$(cat -- "$1" 2>/dev/null || :)
[ -z "$pid" ] || kill "$pid" 2>/dev/null || :
rm -rf -- "$2""#,
            &[child_pid, active_root],
            Vec::new(),
            TIMEOUT,
        )
        .await;
        return Err(error);
    }
    Ok(())
}

async fn managed_acquisition_roots(session: &WslSession) -> Result<BTreeSet<String>, AppError> {
    let output = run_wsl_script(
        session,
        r#"find /tmp -maxdepth 1 -type d -name 'skill-deck-discovery-*' -print"#,
        &[],
        Vec::new(),
        TIMEOUT,
    )
    .await?;
    Ok(String::from_utf8_lossy(&output)
        .lines()
        .map(ToString::to_string)
        .collect())
}

pub async fn run_full_wsl_mutation_workflow(
    session: WslSession,
    root: String,
) -> Result<(), AppError> {
    let harness = WslWorkflowHarness::new(session, root).await?;
    assert_wsl_update_contracts(&harness).await?;
    let handle_v1 = harness.payload("v1", "computed-v1", "remote-v1").await?;
    let (install, request, token) = harness.install_preview(handle_v1).await?;
    let installed = install
        .execute(&request, token, CancellationSignal::default())
        .await?;
    assert_succeeded(&installed.units);
    assert!(
        harness
            .exists(format!(
                "{}/.agents/skills/demo/scripts/run.sh",
                harness.source_project
            ))
            .await?
    );
    assert!(
        harness
            .exists(format!(
                "{}/.builtin/skills/demo/references/guide.md",
                harness.source_project
            ))
            .await?
    );
    assert!(
        harness
            .exists(format!(
                "{}/.custom/skills/demo/assets/logo.bin",
                harness.source_project
            ))
            .await?
    );
    run_wsl_script(
        &harness.session,
        r#"rm -rf -- "$1"; ln -s -- "$2" "$1""#,
        &[
            format!("{}/.builtin/skills/demo", harness.source_project),
            "../../.agents/skills/demo".to_string(),
        ],
        Vec::new(),
        TIMEOUT,
    )
    .await?;

    let handle_v2 = harness.payload("v2", "computed-v2", "remote-v2").await?;
    let update = UpdateService::new(
        harness.payloads.clone(),
        ConcreteUpdatePlanner::new(
            harness.facts.clone(),
            harness.targets.clone(),
            harness.payloads.clone(),
            fixed_time,
        ),
        FixedUpdateAcquirer { handle: handle_v2 },
        harness.executor(),
    );
    let update_request = UpdateRequest {
        context: harness.context("source"),
        skill_names: vec!["demo".to_string()],
    };
    let preview = update.preview(&update_request).await?;
    assert_eq!(preview.skills[0].clean_copy_count, 1);
    assert!(preview.skills[0].overwrite_private_entries.is_empty());
    let execution = UpdateExecutionRequest {
        request: update_request,
        overwrite_private_entries: preview.skills[0]
            .overwrite_private_entries
            .iter()
            .map(|entry| entry.entry_id.clone())
            .collect(),
    };
    let updated = update
        .execute(&execution, preview.token, CancellationSignal::default())
        .await?;
    assert_eq!(updated.outcome, UpdateOutcome::Succeeded);
    assert_eq!(updated.sources.len(), 1);
    assert_eq!(updated.sources[0].status, UpdateSourceStatus::Acquired);
    assert_succeeded(
        &updated
            .skills
            .iter()
            .filter_map(|skill| skill.mutation.clone())
            .collect::<Vec<_>>(),
    );
    assert_eq!(
        harness
            .read(format!(
                "{}/.agents/skills/demo/scripts/run.sh",
                harness.source_project
            ))
            .await?,
        b"#!/bin/sh\necho v2\n"
    );
    assert_eq!(
        harness
            .read(format!(
                "{}/.builtin/skills/demo/scripts/run.sh",
                harness.source_project
            ))
            .await?,
        b"#!/bin/sh\necho v2\n"
    );
    let canonical_manifest = wsl_manifest(
        &harness,
        format!("{}/.agents/skills/demo", harness.source_project),
    )
    .await?;
    let copied_manifest = wsl_manifest(
        &harness,
        format!("{}/.custom/skills/demo", harness.source_project),
    )
    .await?;
    if canonical_manifest.hash() != copied_manifest.hash() {
        return Err(AppError::Custom {
            message: "WSL copy materialization diverged after update".to_string(),
        });
    }

    let observer = SkillEntryObserver::new(harness.facts.clone(), harness.targets.clone());
    let observed = observer.observe(&harness.context("source"), "demo").await?;
    let custom_entry = observed
        .entries
        .iter()
        .find(|entry| {
            entry
                .public
                .owners
                .iter()
                .any(|owner| owner.agent_id.as_str() == "custom-test")
        })
        .ok_or_else(|| AppError::Custom {
            message: "missing Custom Agent entry".to_string(),
        })?
        .public
        .entry_id
        .clone();
    let manage = ManageAgentsService::new(
        SkillEntryObserver::new(harness.facts.clone(), harness.targets.clone()),
        harness.targets.clone(),
        harness.payloads.clone(),
        InstalledSkillPayloadAcquirer::new(harness.payloads.clone(), harness.environments.clone()),
        harness.executor(),
    );
    let manage_request = ManageAgentsPreviewRequest {
        context: harness.context("source"),
        skill_name: "demo".to_string(),
        add: Vec::new(),
        remove_entry_ids: vec![custom_entry.clone()],
        requested_mode: InstallMode::Copy,
    };
    let manage_preview = manage.preview(&manage_request).await?;
    let managed = manage
        .execute(
            &ManageAgentsRequest {
                token: manage_preview.token,
                context: harness.context("source"),
                skill_name: "demo".to_string(),
                add: Vec::new(),
                remove_entry_ids: vec![custom_entry],
                requested_mode: InstallMode::Copy,
                confirm_entity_directories: true,
                canonical_payload: None,
            },
            CancellationSignal::default(),
        )
        .await?;
    assert_succeeded(&managed.units);

    let copy = CopyService::new(
        harness.facts.clone(),
        harness.targets.clone(),
        harness.payloads.clone(),
        InstalledSkillPayloadAcquirer::new(harness.payloads.clone(), harness.environments.clone()),
        harness.executor(),
        RuntimeCopyProjectComparator::new(harness.environments.clone()),
    );
    let copy_request = CopyRequest {
        skill_name: "demo".to_string(),
        source: harness.context("source"),
        target_environment: harness.context("target").environment,
        target_project_ids: vec!["target".to_string()],
        requested_mode: InstallMode::Copy,
        agent_intents: both_agent_intents(),
    };
    let copy_preview = copy.preview(&copy_request).await?;
    let copied = copy
        .execute(
            &CopyExecutionRequest {
                request: copy_request,
                token: copy_preview.token,
                payload: copy_preview.payload,
            },
            CancellationSignal::default(),
        )
        .await?;
    assert_succeeded(&copied.units);
    assert!(
        harness
            .exists(format!(
                "{}/.agents/skills/demo/scripts/run.sh",
                harness.target_project
            ))
            .await?
    );
    assert_eq!(
        harness
            .read(format!(
                "{}/.agents/skills/demo/references/guide.md",
                harness.target_project
            ))
            .await?,
        b"guide-v2"
    );
    let target_lock = harness
        .json(format!("{}/skills-lock.json", harness.target_project))
        .await?;
    assert_eq!(target_lock["targetFutureRoot"], "keep");
    assert_eq!(target_lock["skills"]["demo"]["remoteHash"], "remote-v2");

    let remove = RemoveService::new(
        SkillEntryObserver::new(harness.facts.clone(), harness.targets.clone()),
        harness.executor(),
    );
    let remove_preview = remove.preview(&harness.context("source"), "demo").await?;
    let removed = remove
        .execute(
            &RemoveRequest {
                token: remove_preview.token,
                context: harness.context("source"),
                skill_name: "demo".to_string(),
                intent: RemoveIntent::FullSkill,
            },
            CancellationSignal::default(),
        )
        .await?;
    assert_succeeded(&removed.units);
    assert!(
        !harness
            .exists(format!("{}/.agents/skills/demo", harness.source_project))
            .await?
    );
    let leaks = run_wsl_script(
        &harness.session,
        r#"find "$1" \( -name '.skill-deck-stage-*' -o -name '.skill-deck-backup-*' \) -print"#,
        std::slice::from_ref(&harness.root),
        Vec::new(),
        TIMEOUT,
    )
    .await?;
    if !leaks.is_empty() {
        return Err(AppError::Custom {
            message: format!("WSL staging leak: {}", String::from_utf8_lossy(&leaks)),
        });
    }
    Ok(())
}

pub async fn session_loss_invalidates_preview(
    session: WslSession,
    root: String,
) -> Result<(), AppError> {
    let harness = WslWorkflowHarness::new(session, root).await?;
    let handle = harness
        .payload("stale", "computed-stale", "remote-stale")
        .await?;
    let (install, request, token) = harness.install_preview(handle).await?;
    #[cfg(target_os = "windows")]
    {
        let status = std::process::Command::new("wsl.exe")
            .args(["--terminate", harness.session.distro_name.as_str()])
            .status()?;
        if !status.success() {
            return Err(AppError::EnvironmentUnavailable {
                environment: request.context.environment.clone(),
                message: "failed to terminate WSL distro for session-loss acceptance".to_string(),
            });
        }
    }
    let mut reconnected = connect_wsl_environment(&harness.session.distro_name).await?;
    reconnected.home = harness.session.home.clone();
    reconnected.config_home = harness.session.config_home.clone();
    harness.environments.insert(reconnected);
    let error = install
        .execute(&request, token, CancellationSignal::default())
        .await
        .expect_err("old preview must be stale after reconnect");
    if !matches!(
        error,
        AppError::StaleEnvironment
            | AppError::StaleContext
            | AppError::StaleTarget
            | AppError::StaleRegistry
    ) {
        return Err(AppError::Custom {
            message: format!("unexpected session-loss result: {error}"),
        });
    }
    if harness
        .exists(format!("{}/.agents/skills/demo", harness.source_project))
        .await?
    {
        return Err(AppError::Custom {
            message: "stale preview mutated the WSL project".to_string(),
        });
    }
    Ok(())
}

pub async fn cli_lock_conflict_preserves_external_change(
    session: WslSession,
    root: String,
) -> Result<(), AppError> {
    let harness = WslWorkflowHarness::new(session, root).await?;
    let handle = harness
        .payload("conflict", "computed-conflict", "remote-conflict")
        .await?;
    let (install, request, token) = harness.install_preview(handle).await?;
    let external = json!({
        "version": 1,
        "futureRoot": { "keep": true },
        "skills": {
            "demo": {
                "source": "cli/source",
                "computedHash": "cli-computed",
                "cliField": "preserve"
            }
        }
    });
    EnvironmentLockIo::Wsl(harness.session.clone())
        .write_atomic(
            &locator(
                &harness.session,
                format!("{}/skills-lock.json", harness.source_project),
            ),
            serde_json::to_vec_pretty(&external)?,
        )
        .await?;
    let error = install
        .execute(&request, token, CancellationSignal::default())
        .await
        .expect_err("CLI lock change must invalidate preview");
    if !matches!(
        error,
        AppError::StaleContext | AppError::StaleTarget | AppError::LockConflict { .. }
    ) {
        return Err(AppError::Custom {
            message: format!("unexpected CLI conflict result: {error}"),
        });
    }
    let lock = harness
        .json(format!("{}/skills-lock.json", harness.source_project))
        .await?;
    if lock["skills"]["demo"]["cliField"] != "preserve" {
        return Err(AppError::Custom {
            message: "CLI lock change was overwritten".to_string(),
        });
    }
    Ok(())
}

pub async fn reconnect_reindexes_recovery_and_sweeps_payloads(
    session: WslSession,
    root: String,
) -> Result<(), AppError> {
    let harness = WslWorkflowHarness::new(session, root).await?;
    let active_payload = harness
        .payload("reconnect", "computed-reconnect", "remote-reconnect")
        .await?;
    let active_session_id = active_payload.session_id.clone();
    let orphan_session_id = format!("orphan-{}", Uuid::new_v4().simple());
    let recovery_id = RecoveryResourceId::parse(format!("recovery-{}", Uuid::new_v4().simple()))
        .map_err(|error| AppError::Validation {
            field: Some("recoveryResourceId".to_string()),
            message: error.to_string(),
        })?;
    let environment = harness.context("source").environment;
    let destination = format!("{}/recovery-target", harness.source_project);
    let recovery_root = format!("/tmp/skill-deck-operation-{}", recovery_id.as_str());
    let orphan_root = format!("/tmp/skill-deck-source-{orphan_session_id}");
    run_wsl_script(
        &harness.session,
        r#"
mkdir -p -- "$1" "$2" "$3"
printf 'recovery target' > "$1/value"
printf '1\n%s\n' "$4" > "$2/.skill-deck-owner"
printf '1\n%s\n' "$5" > "$3/.skill-deck-owner"
printf 'orphan payload' > "$3/payload.bin"
"#,
        &[
            destination.clone(),
            recovery_root.clone(),
            orphan_root.clone(),
            recovery_id.as_str().to_string(),
            orphan_session_id.clone(),
        ],
        Vec::new(),
        TIMEOUT,
    )
    .await?;

    let marker = RecoveryMarker {
        schema_version: RECOVERY_MARKER_SCHEMA_VERSION,
        resource_id: recovery_id.clone(),
        kind: RecoveryMarkerKind::InProgress,
        environment: environment.clone(),
        operation_id: "wsl-reconnect-acceptance".to_string(),
        unit_id: "recovery-unit".to_string(),
        created_at_epoch_ms: 1_000,
        entries: vec![RecoveryMarkerEntry {
            physical_target_digest: "wsl-recovery-target".to_string(),
            destination: locator(&harness.session, destination),
            backup: None,
            expected_state: RecoveryExpectedEntryState::Present,
            original_fingerprint: "entry-v1-before-reconnect".to_string(),
            phase: RecoveryEntryPhase::LockCommitted,
        }],
    };
    let initial_graph = RuntimeRecoveryGraph::new(
        harness.environments.clone(),
        harness._host.path().join("initial-recovery-index"),
    )?;
    initial_graph
        .wsl_store(harness.session.clone())?
        .create(&marker)
        .await?;
    drop(initial_graph);

    let terminate_status = std::process::Command::new("wsl.exe")
        .args(["--terminate", harness.session.distro_name.as_str()])
        .status()?;
    if !terminate_status.success() {
        return Err(AppError::EnvironmentUnavailable {
            environment,
            message: "failed to terminate WSL distro for reconnect acceptance".to_string(),
        });
    }
    let reconnected = connect_wsl_environment(&harness.session.distro_name).await?;
    let reopened_environments = Arc::new(EnvironmentRegistry::default());
    reopened_environments.insert(reconnected.clone());

    let reopened_graph = RuntimeRecoveryGraph::new(
        reopened_environments,
        harness._host.path().join("reopened-recovery-index"),
    )?;
    reopened_graph.reindex_wsl(reconnected.clone()).await?;
    let recovery_service = reopened_graph.service();
    let status = recovery_service
        .list()
        .await?
        .into_iter()
        .find(|status| status.resource_id == recovery_id)
        .ok_or_else(|| AppError::Custom {
            message: "reconnected WSL recovery resource was not enumerated".to_string(),
        })?;
    if status.state != RecoveryResourceState::ConsistentCanCleanup {
        return Err(AppError::Custom {
            message: format!("unexpected reconnect recovery state: {:?}", status.state),
        });
    }

    let protected = harness.payloads.protected_session_ids(&environment)?;
    if !protected.contains(&active_session_id) {
        return Err(AppError::Custom {
            message: "active WSL payload session was not protected".to_string(),
        });
    }
    let maintenance = WslPayloadSessionStorage::new(reconnected.clone());
    let report = maintenance.sweep_orphans(&protected).await?;
    if report.protected_sessions == 0 || report.removed_sessions == 0 {
        return Err(AppError::Custom {
            message: "WSL reconnect sweep did not report protected and orphan sessions".to_string(),
        });
    }
    for (path, expected) in [
        (format!("/tmp/skill-deck-source-{active_session_id}"), true),
        (orphan_root, false),
    ] {
        let exists = run_wsl_script(
            &reconnected,
            r#"if [ -e "$1" ] || [ -L "$1" ]; then printf '1'; else printf '0'; fi"#,
            &[path],
            Vec::new(),
            TIMEOUT,
        )
        .await?
            == b"1";
        if exists != expected {
            return Err(AppError::Custom {
                message: "WSL reconnect sweep violated payload ownership".to_string(),
            });
        }
    }

    recovery_service
        .confirm_resolved(&recovery_id, &status.revision)
        .await?;
    if recovery_service.status(&recovery_id).await?.state != RecoveryResourceState::Missing {
        return Err(AppError::Custom {
            message: "resolved WSL recovery resource remained indexed".to_string(),
        });
    }
    maintenance.remove_session(&active_session_id).await?;
    Ok(())
}

pub async fn marker_before_batch_stage_failure_converges_after_reconnect(
    session: WslSession,
    root: String,
) -> Result<(), AppError> {
    let harness = WslWorkflowHarness::new(session, root).await?;
    let recovery_id =
        RecoveryResourceId::parse(format!("stage-failure-{}", Uuid::new_v4().simple())).map_err(
            |error| AppError::Validation {
                field: Some("recoveryResourceId".to_string()),
                message: error.to_string(),
            },
        )?;
    let environment = harness.context("source").environment;
    let destination = format!("{}/stage-failure-target", harness.source_project);
    let backup = format!(
        "{}/.skill-deck-backup-{}-000000",
        harness.source_project,
        recovery_id.as_str()
    );
    let marker = RecoveryMarker {
        schema_version: RECOVERY_MARKER_SCHEMA_VERSION,
        resource_id: recovery_id.clone(),
        kind: RecoveryMarkerKind::InProgress,
        environment: environment.clone(),
        operation_id: "wsl-stage-failure-acceptance".to_string(),
        unit_id: "stage-failure-unit".to_string(),
        created_at_epoch_ms: 1_000,
        entries: vec![RecoveryMarkerEntry {
            physical_target_digest: "wsl-stage-failure-target".to_string(),
            destination: locator(&harness.session, destination),
            backup: Some(locator(&harness.session, backup)),
            expected_state: RecoveryExpectedEntryState::Present,
            original_fingerprint: "entry-v1-missing".to_string(),
            phase: RecoveryEntryPhase::Staged,
        }],
    };
    let initial_graph = RuntimeRecoveryGraph::new(
        harness.environments.clone(),
        harness._host.path().join("stage-failure-initial-index"),
    )?;
    initial_graph
        .wsl_store(harness.session.clone())?
        .create(&marker)
        .await?;
    let operation_root = format!("/tmp/skill-deck-operation-{}", recovery_id.as_str());
    let stage_error = run_wsl_script(
        &harness.session,
        include_str!("../environment/wsl/scripts/materialize.sh"),
        &[
            "stage".to_string(),
            operation_root,
            recovery_id.as_str().to_string(),
        ],
        Vec::new(),
        TIMEOUT,
    )
    .await
    .expect_err("empty batch request must fail after the recovery marker exists");
    if !matches!(stage_error, AppError::WslCommandFailed { .. }) {
        return Err(AppError::Custom {
            message: format!("unexpected WSL batch stage failure: {stage_error}"),
        });
    }
    drop(initial_graph);

    let terminate_status = std::process::Command::new("wsl.exe")
        .args(["--terminate", harness.session.distro_name.as_str()])
        .status()?;
    if !terminate_status.success() {
        return Err(AppError::EnvironmentUnavailable {
            environment,
            message: "failed to terminate WSL distro after batch stage failure".to_string(),
        });
    }
    let reconnected = connect_wsl_environment(&harness.session.distro_name).await?;
    let reopened_environments = Arc::new(EnvironmentRegistry::default());
    reopened_environments.insert(reconnected.clone());
    let reopened_graph = RuntimeRecoveryGraph::new(
        reopened_environments,
        harness._host.path().join("stage-failure-reopened-index"),
    )?;
    reopened_graph.reindex_wsl(reconnected).await?;
    let recovery_service = reopened_graph.service();
    let status = recovery_service.status(&recovery_id).await?;
    if status.state != RecoveryResourceState::ConsistentCanCleanup {
        return Err(AppError::Custom {
            message: format!(
                "marker-before-stage failure did not converge after reconnect: {:?}",
                status.state
            ),
        });
    }
    recovery_service
        .confirm_resolved(&recovery_id, &status.revision)
        .await?;
    Ok(())
}

fn locator(session: &WslSession, native_path: String) -> ResourceLocator {
    ResourceLocator {
        environment: EnvironmentRef::Wsl {
            distro_name: session.distro_name.clone(),
        },
        native_path,
    }
}

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use std::sync::Arc;

use serde_json::{json, Value};

use crate::application::agent_intent::{AgentWriteIntent, PrivateEntryIntent};
use crate::application::copy::{CopyExecutionRequest, CopyRequest, CopyService};
use crate::application::copy_runtime::RuntimeCopyProjectComparator;
use crate::application::install::{InstallRequest, InstallService};
use crate::application::install_planner::ConcreteInstallPlanner;
use crate::application::manage_agents::{
    ManageAgentsPreviewRequest, ManageAgentsRequest, ManageAgentsService,
};
use crate::application::mutation::coordinator::RuntimeRevisionSource;
use crate::application::mutation::result::{MutationUnitResult, MutationUnitStatus};
use crate::application::payload_session::{
    AcquiredPayloadHandle, DiscoverySessionHandle, PayloadPlanningMetadata, PayloadSessionLimits,
    PayloadSessionManager,
};
use crate::application::plan_runner::{RuntimeExecutionDependencies, RuntimePlanExecutor};
use crate::application::remove::{RemoveRequest, RemoveSelection, RemoveService};
use crate::application::runtime_facts::{
    AgentRegistrySnapshotSource, HostRuntimeSnapshot, RuntimePlanningFactSource,
};
use crate::application::skill_entries::{InstalledSkillPayloadAcquirer, SkillEntryObserver};
use crate::application::source_evidence::{RemoteSnapshotId, SourceSnapshotFacts};
use crate::application::update::{
    AcquiredUpdateSource, UpdateAcquisitionGroup, UpdateExecutionRequest, UpdateFuture,
    UpdatePayloadAcquirer, UpdateRequest, UpdateService, UpdateSourceAcquisition,
};
use crate::application::update_planner::ConcreteUpdatePlanner;
use crate::core::agent_definition::{
    AgentAdapter, AgentDefinition, AgentId, AgentSource, CustomAgentDefinition, CustomPathBase,
    CustomPathSpec, CustomScopeDefinition, DetectionSpec, PathSpec, ScopeDefinition, ScopeLocation,
};
use crate::core::agent_registry::{AgentRegistry, AgentRegistrySnapshot};
use crate::core::agent_settings::CustomAgentRecord;
use crate::core::mutation::CancellationSignal;
use crate::core::skill_payload::{
    build_skill_payload, compute_cli_project_hash_from_payload, SkillPayload,
};
use crate::environment::planning::RuntimeTargetFactResolver;
use crate::environment::types::{ContextRef, ContextScope, EnvironmentRef};
use crate::environment::wsl::EnvironmentRegistry;
use crate::error::AppError;
use crate::models::InstallMode;

pub(crate) struct StaticRegistry(pub(crate) Arc<AgentRegistrySnapshot>);

impl AgentRegistrySnapshotSource for StaticRegistry {
    fn snapshot(&self) -> Arc<AgentRegistrySnapshot> {
        Arc::clone(&self.0)
    }
}

#[derive(Clone)]
pub(crate) struct FixedUpdateAcquirer {
    pub(crate) handle: AcquiredPayloadHandle,
}

impl UpdatePayloadAcquirer for FixedUpdateAcquirer {
    fn acquire<'a>(
        &'a self,
        groups: &'a [UpdateAcquisitionGroup],
        _cancellation: CancellationSignal,
    ) -> UpdateFuture<'a, Result<Vec<UpdateSourceAcquisition>, AppError>> {
        let handle = self.handle.clone();
        let valid =
            groups.len() == 1 && groups[0].skills.len() == 1 && groups[0].skills[0].name == "demo";
        Box::pin(async move {
            if !valid {
                return Err(AppError::StalePayload);
            }
            let group = &groups[0];
            Ok(vec![UpdateSourceAcquisition {
                source_result_id: group.source_result_id.clone(),
                source: group.source.clone(),
                skill_names: vec!["demo".to_string()],
                result: Ok(AcquiredUpdateSource {
                    facts: SourceSnapshotFacts {
                        discovery_session: DiscoverySessionHandle {
                            session_id: handle.session_id.clone(),
                            environment: handle.environment.clone(),
                            source_fingerprint: handle.source_fingerprint.clone(),
                            expires_at_epoch_ms: handle.expires_at_epoch_ms,
                        },
                        snapshot_id: RemoteSnapshotId::new(
                            group.key.normalized_ref.clone(),
                            "main",
                            "revision-v2",
                        ),
                        complete_skill_path_catalog: BTreeSet::from(["skills/demo".to_string()]),
                    },
                    payloads: vec![("demo".to_string(), handle)],
                }),
            }])
        })
    }
}

async fn run_native_workflow_integration() -> Result<(), AppError> {
    let temp = tempfile::tempdir()?;
    let root = temp.path();
    let source_project = root.join("source-project");
    let target_project = root.join("target-project");
    let projects_path = root.join("state/projects.json");
    let global_lock_path = root.join("state/global-lock.json");
    let recovery_root = root.join("recovery");
    let home = root.join("home");
    let config_home = root.join("config");

    for project in [&source_project, &target_project] {
        fs::create_dir_all(project.join(".builtin"))?;
        fs::create_dir_all(project.join(".custom"))?;
    }
    fs::create_dir_all(projects_path.parent().expect("state parent"))?;
    fs::create_dir_all(&home)?;
    fs::create_dir_all(&config_home)?;
    write_json(
        &projects_path,
        &json!({
            "schemaVersion": 1,
            "projects": [
                project("source", &source_project, "Source"),
                project("target", &target_project, "Target")
            ]
        }),
    )?;
    write_json(
        &source_project.join("skills-lock.json"),
        &json!({
            "version": 1,
            "futureRoot": { "keep": true },
            "skills": {
                "demo": {
                    "source": "legacy/source",
                    "futureEntry": 42,
                    "subagents": ["legacy-adapter"]
                }
            }
        }),
    )?;
    write_json(
        &target_project.join("skills-lock.json"),
        &json!({
            "version": 1,
            "targetFutureRoot": "keep",
            "skills": {
                "demo": {
                    "targetFutureEntry": "keep",
                    "computedHash": "stale-target"
                }
            }
        }),
    )?;

    let registry = Arc::new(StaticRegistry(Arc::new(test_registry())));
    let environments = Arc::new(EnvironmentRegistry::default());
    let facts = RuntimePlanningFactSource::with_host_snapshot(
        registry,
        environments.clone(),
        HostRuntimeSnapshot {
            home,
            config_home,
            projects_path,
            global_lock_path,
            environment_variables: BTreeMap::new(),
        },
    );
    let targets = RuntimeTargetFactResolver::new(environments.clone());
    let payloads = Arc::new(PayloadSessionManager::in_memory(
        PayloadSessionLimits {
            ttl_ms: 60_000,
            max_sessions: 16,
            max_bytes: 16 * 1024 * 1024,
        },
        || 1_000,
    ));
    let execution = RuntimeExecutionDependencies::new(environments.clone(), recovery_root.clone())?;
    assert!(Arc::ptr_eq(
        &execution.recovery_graph(),
        &execution.recovery_graph()
    ));

    let source_v1 = root.join("payload-v1/demo");
    let payload_v1 = create_payload(&source_v1, "v1")?;
    let discovery_v1 = payloads.discover(EnvironmentRef::Host, "source-v1").await?;
    let handle_v1 = payloads
        .acquire_payload_with_metadata(
            &discovery_v1,
            "skills/demo",
            payload_v1,
            metadata("computed-v1", "remote-v1"),
        )
        .await?;

    let source_v2 = root.join("payload-v2/demo");
    let payload_v2 = create_payload(&source_v2, "v2")?;
    let expected_copy_hash = compute_cli_project_hash_from_payload(&payload_v2)?;
    let discovery_v2 = payloads.discover(EnvironmentRef::Host, "source-v2").await?;
    let handle_v2 = payloads
        .acquire_payload_with_metadata(
            &discovery_v2,
            "skills/demo",
            payload_v2,
            metadata("computed-v2", "remote-v2"),
        )
        .await?;

    let source_context = project_context("source");
    let install = InstallService::new(
        payloads.clone(),
        ConcreteInstallPlanner::new(facts.clone(), targets.clone(), payloads.clone(), fixed_time),
        executor(&execution, &environments, &facts),
    );
    let install_request = InstallRequest {
        context: source_context.clone(),
        source: "owner/repo".to_string(),
        discovery_session: discovery_v1,
        payloads: vec![handle_v1],
        skills: vec!["demo".to_string()],
        agent_intents: both_agent_intents(),
        requested_mode: InstallMode::Copy,
        acknowledge_risk: true,
    };
    let install_preview = install.preview(&install_request).await?;
    let installed = install
        .execute(
            &install_request,
            install_preview.token,
            CancellationSignal::default(),
        )
        .await?;
    assert_succeeded(&installed.units);
    assert_payload_tree(&source_project.join(".agents/skills/demo"), "v1")?;
    assert_payload_tree(&source_project.join(".builtin/skills/demo"), "v1")?;
    assert_payload_tree(&source_project.join(".custom/skills/demo"), "v1")?;
    assert_lock_fields(
        &source_project.join("skills-lock.json"),
        "computed-v1",
        "remote-v1",
    )?;

    let observer = SkillEntryObserver::new(facts.clone(), targets.clone());
    let observed = observer.observe(&source_context, "demo").await?;
    let owners = observed
        .entries
        .iter()
        .flat_map(|entry| entry.public.owners.iter())
        .map(|owner| owner.agent_id.as_str())
        .collect::<BTreeSet<_>>();
    assert_eq!(owners, BTreeSet::from(["builtin-test", "custom-test"]));

    let update = UpdateService::new(
        payloads.clone(),
        ConcreteUpdatePlanner::new(facts.clone(), targets.clone(), payloads.clone(), fixed_time),
        FixedUpdateAcquirer { handle: handle_v2 },
        executor(&execution, &environments, &facts),
    );
    let update_request = UpdateRequest {
        context: source_context.clone(),
        skill_names: vec!["demo".to_string()],
    };
    let update_preview = update.preview(&update_request).await?;
    let overwrite_private_entries = update_preview.skills[0]
        .overwrite_private_entries
        .iter()
        .map(|entry| entry.entry_id.clone())
        .collect();
    let update_execution = UpdateExecutionRequest {
        request: update_request,
        overwrite_private_entries,
    };
    let updated = update
        .execute(
            &update_execution,
            update_preview.token,
            CancellationSignal::default(),
        )
        .await?;
    assert_succeeded(
        &updated
            .skills
            .iter()
            .filter_map(|skill| skill.mutation.clone())
            .collect::<Vec<_>>(),
    );
    assert_payload_tree(&source_project.join(".agents/skills/demo"), "v2")?;
    assert_payload_tree(&source_project.join(".builtin/skills/demo"), "v2")?;
    assert_payload_tree(&source_project.join(".custom/skills/demo"), "v2")?;
    assert_lock_fields(
        &source_project.join("skills-lock.json"),
        "computed-v2",
        "remote-v2",
    )?;

    let manage = ManageAgentsService::new(
        SkillEntryObserver::new(facts.clone(), targets.clone()),
        targets.clone(),
        payloads.clone(),
        InstalledSkillPayloadAcquirer::new(payloads.clone(), environments.clone()),
        executor(&execution, &environments, &facts),
    );
    let manage_observed = observer.observe(&source_context, "demo").await?;
    let custom_entry = manage_observed
        .entries
        .iter()
        .find(|entry| {
            entry
                .public
                .owners
                .iter()
                .any(|owner| owner.agent_id.as_str() == "custom-test")
        })
        .expect("custom Agent physical entry")
        .public
        .entry_id
        .clone();
    let manage_preview_request = ManageAgentsPreviewRequest {
        context: source_context.clone(),
        skill_name: "demo".to_string(),
        add: Vec::new(),
        remove_entry_ids: vec![custom_entry.clone()],
        requested_mode: InstallMode::Copy,
    };
    let manage_preview = manage.preview(&manage_preview_request).await?;
    let managed = manage
        .execute(
            &ManageAgentsRequest {
                token: manage_preview.token,
                context: source_context.clone(),
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
    assert!(!source_project.join(".custom/skills/demo").exists());

    let source_lock_path = source_project.join("skills-lock.json");
    let mut source_lock = read_json(&source_lock_path)?;
    let source_entry = source_lock["skills"]["demo"]
        .as_object_mut()
        .expect("source lock entry");
    source_entry.insert("subagents".to_string(), json!(["legacy-adapter"]));
    source_entry.insert("adapterState".to_string(), json!({ "legacy": true }));
    write_json(&source_lock_path, &source_lock)?;

    let copy = CopyService::new(
        facts.clone(),
        targets.clone(),
        payloads.clone(),
        InstalledSkillPayloadAcquirer::new(payloads.clone(), environments.clone()),
        executor(&execution, &environments, &facts),
        RuntimeCopyProjectComparator::new(environments.clone()),
    );
    let copy_request = CopyRequest {
        skill_name: "demo".to_string(),
        source: source_context.clone(),
        target_environment: EnvironmentRef::Host,
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
    assert_payload_tree(&target_project.join(".agents/skills/demo"), "v2")?;
    assert_payload_tree(&target_project.join(".builtin/skills/demo"), "v2")?;
    assert_payload_tree(&target_project.join(".custom/skills/demo"), "v2")?;
    let target_lock = read_json(&target_project.join("skills-lock.json"))?;
    assert_eq!(target_lock["targetFutureRoot"], "keep");
    assert_eq!(target_lock["skills"]["demo"]["targetFutureEntry"], "keep");
    assert_eq!(
        target_lock["skills"]["demo"]["computedHash"],
        expected_copy_hash
    );
    assert_eq!(target_lock["skills"]["demo"]["remoteHash"], "remote-v2");
    assert!(target_lock["skills"]["demo"].get("futureEntry").is_none());
    assert!(target_lock["skills"]["demo"].get("subagents").is_none());
    assert!(target_lock["skills"]["demo"].get("adapterState").is_none());

    let remove = RemoveService::new(
        SkillEntryObserver::new(facts.clone(), targets),
        executor(&execution, &environments, &facts),
    );
    let remove_preview = remove.preview(&source_context, "demo").await?;
    let removed = remove
        .execute(
            &RemoveRequest {
                token: remove_preview.token,
                context: source_context,
                skill_name: "demo".to_string(),
                selection: RemoveSelection {
                    remove_canonical: true,
                    entry_ids: remove_preview
                        .physical_entries
                        .iter()
                        .map(|entry| entry.entry_id.clone())
                        .collect(),
                    confirm_entity_directories: true,
                },
            },
            CancellationSignal::default(),
        )
        .await?;
    assert_succeeded(&removed.units);
    assert!(!source_project.join(".agents/skills/demo").exists());
    assert!(!source_project.join(".builtin/skills/demo").exists());
    assert_no_staging_leaks(root)?;
    assert_recovery_graph_is_empty(&recovery_root)?;
    Ok(())
}

fn executor(
    execution: &RuntimeExecutionDependencies,
    environments: &Arc<EnvironmentRegistry>,
    facts: &RuntimePlanningFactSource,
) -> RuntimePlanExecutor {
    let revisions: Arc<dyn RuntimeRevisionSource> = Arc::new(facts.clone());
    execution.executor(environments.clone(), revisions)
}

fn project(id: &str, path: &Path, display_name: &str) -> Value {
    json!({
        "id": id,
        "nativePath": path.to_string_lossy(),
        "displayName": display_name,
        "order": null,
        "suppressCrossStorageWarning": false
    })
}

fn project_context(id: &str) -> ContextRef {
    ContextRef {
        environment: EnvironmentRef::Host,
        scope: ContextScope::Project {
            project_id: id.to_string(),
        },
    }
}

pub(crate) fn test_registry() -> AgentRegistrySnapshot {
    let builtin = AgentDefinition {
        id: AgentId::parse("builtin-test").expect("built-in id"),
        display_name: "Built-in Test".to_string(),
        source: AgentSource::Builtin,
        aliases: Vec::new(),
        global: disabled_scope(),
        project: ScopeDefinition {
            enabled: true,
            reads_shared: true,
            private_path: Some(PathSpec::project(".builtin/skills")),
        },
        detection: DetectionSpec::AnyPathExists {
            paths: vec![PathSpec::project(".builtin")],
        },
        legacy_paths: Vec::new(),
        adapter: AgentAdapter::Standard,
    };
    let custom = CustomAgentDefinition {
        id: AgentId::parse("custom-test").expect("custom id"),
        display_name: "Custom Test".to_string(),
        global: CustomScopeDefinition {
            enabled: false,
            location: ScopeLocation::Shared,
            private_path: None,
        },
        project: CustomScopeDefinition {
            enabled: true,
            location: ScopeLocation::Both,
            private_path: Some(CustomPathSpec::based(
                CustomPathBase::Project,
                ".custom/skills",
            )),
        },
        detection_paths: vec![CustomPathSpec::based(CustomPathBase::Project, ".custom")],
    };
    AgentRegistry::build(vec![builtin], vec![CustomAgentRecord::valid(custom)])
        .snapshot()
        .clone()
}

fn disabled_scope() -> ScopeDefinition {
    ScopeDefinition {
        enabled: false,
        reads_shared: false,
        private_path: None,
    }
}

pub(crate) fn both_agent_intents() -> Vec<AgentWriteIntent> {
    ["builtin-test", "custom-test"]
        .into_iter()
        .map(|id| AgentWriteIntent {
            agent_id: AgentId::parse(id).expect("Agent id"),
            private_entry: PrivateEntryIntent::OptionalSelected,
            adapter_targets: Vec::new(),
        })
        .collect()
}

fn metadata(computed_hash: &str, remote_hash: &str) -> PayloadPlanningMetadata {
    PayloadPlanningMetadata {
        skill_name: "demo".to_string(),
        install_dir_name: "demo".to_string(),
        source: "owner/repo".to_string(),
        source_type: "github".to_string(),
        source_url: Some("https://github.com/owner/repo.git".to_string()),
        ref_name: Some("main".to_string()),
        skill_path: "skills/demo".to_string(),
        plugin_name: Some("integration".to_string()),
        computed_hash: computed_hash.to_string(),
        upstream_revision: Some(remote_hash.to_string()),
    }
}

pub(crate) fn create_payload(root: &Path, version: &str) -> Result<SkillPayload, AppError> {
    fs::create_dir_all(root.join("scripts"))?;
    fs::create_dir_all(root.join("references"))?;
    fs::create_dir_all(root.join("assets"))?;
    fs::write(
        root.join("SKILL.md"),
        format!("---\nname: demo\ndescription: {version}\n---\n# Demo {version}\n"),
    )?;
    fs::write(
        root.join("scripts/run.sh"),
        format!("#!/bin/sh\necho {version}\n"),
    )?;
    fs::write(root.join("references/guide.md"), format!("guide-{version}"))?;
    fs::write(root.join("assets/logo.bin"), [0, 1, 2, version.len() as u8])?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(
            root.join("scripts/run.sh"),
            fs::Permissions::from_mode(0o755),
        )?;
    }
    build_skill_payload(root)
}

fn assert_payload_tree(root: &Path, version: &str) -> Result<(), AppError> {
    assert!(root.join("SKILL.md").is_file());
    assert_eq!(
        fs::read_to_string(root.join("scripts/run.sh"))?,
        format!("#!/bin/sh\necho {version}\n")
    );
    assert_eq!(
        fs::read_to_string(root.join("references/guide.md"))?,
        format!("guide-{version}")
    );
    assert_eq!(
        fs::read(root.join("assets/logo.bin"))?,
        [0, 1, 2, version.len() as u8]
    );
    Ok(())
}

fn assert_lock_fields(path: &Path, computed_hash: &str, remote_hash: &str) -> Result<(), AppError> {
    let lock = read_json(path)?;
    assert_eq!(lock["futureRoot"]["keep"], true);
    assert_eq!(lock["skills"]["demo"]["computedHash"], computed_hash);
    assert_eq!(lock["skills"]["demo"]["remoteHash"], remote_hash);
    assert_eq!(lock["skills"]["demo"]["futureEntry"], 42);
    Ok(())
}

pub(crate) fn assert_succeeded(units: &[MutationUnitResult]) {
    assert!(!units.is_empty(), "workflow returned no mutation units");
    assert!(
        units
            .iter()
            .all(|unit| unit.status == MutationUnitStatus::Succeeded),
        "workflow returned non-success units: {units:?}"
    );
}

fn assert_no_staging_leaks(root: &Path) -> Result<(), AppError> {
    for entry in walkdir::WalkDir::new(root).follow_links(false) {
        let entry = entry.map_err(|error| AppError::ExecutionFailed {
            message: error.to_string(),
        })?;
        let name = entry.file_name().to_string_lossy();
        assert!(
            !name.starts_with(".skill-deck-stage-") && !name.starts_with(".skill-deck-backup-"),
            "staging artifact leaked: {}",
            entry.path().display()
        );
    }
    Ok(())
}

fn assert_recovery_graph_is_empty(root: &Path) -> Result<(), AppError> {
    if root.exists() {
        assert!(
            fs::read_dir(root)?.next().is_none(),
            "successful workflows retained recovery resources"
        );
    }
    Ok(())
}

fn write_json(path: &Path, value: &Value) -> Result<(), AppError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, serde_json::to_vec_pretty(value)?)?;
    Ok(())
}

fn read_json(path: &Path) -> Result<Value, AppError> {
    Ok(serde_json::from_slice(&fs::read(path)?)?)
}

pub(crate) fn fixed_time() -> String {
    "2026-07-18T00:00:00.000Z".to_string()
}

#[cfg(test)]
#[tokio::test]
async fn native_workflows_share_one_runtime_and_preserve_skill_deck_metadata() {
    run_native_workflow_integration()
        .await
        .expect("native workflow integration");
}

#[cfg(test)]
mod update_lifecycle {
    use std::io::{self, Read, Write};
    use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream};
    use std::path::{Path, PathBuf};
    use std::process::{Child, Command, Stdio};
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use std::thread;

    use super::*;
    use crate::application::install::{InstallFuture, InstallPlanExecutor};
    use crate::application::mutation::coordinator::{
        BoxFuture, MutationCoordinator, PreparedEntryExecutor,
    };
    use crate::application::mutation::plan::{ExecutionUnit, MutationPlan};
    use crate::application::mutation::result::{MutationUnitStatus, MutationWarning};
    use crate::application::payload_session::{PayloadSessionManager, PinnedPayloadLease};
    use crate::application::plan_runner::{RuntimeLockCommitter, RuntimePlanExecutor};
    use crate::application::resources::SkillIdentity;
    use crate::application::source_acquisition::{
        AcquireSelectedPayloadsRequest, SelectedPayloadAcquisitionService, SourceDiscoveryService,
    };
    use crate::application::source_evidence::{
        EvidenceDetectionRequest, EvidenceFuture, SourceEvidenceCoordinator, SourceEvidenceDetector,
    };
    use crate::application::source_evidence_provider::RuntimeSourceEvidenceDetector;
    use crate::application::source_snapshot_reuse::SourceSnapshotReuseIndex;
    use crate::application::update::{
        UpdateCheckMode, UpdateCheckRequest, UpdateCheckSelection, UpdateExecutionRequest,
        UpdateOutcome, UpdatePreview, UpdateRequest, UpdateResponse, UpdateSourceStatus,
        UpdateWarningCode,
    };
    use crate::application::update_check::UpdateCheckService;
    use crate::application::update_runtime::{RuntimeUpdatePayloadAcquirer, RuntimeUpdateService};
    use crate::core::mutation::CancellationSignal;
    use crate::core::skill_payload::{compute_cli_project_hash_from_payload, PayloadId};
    use crate::environment::native::materialize::{
        NativePreparedEntryExecutor, NativePreparedEntrySet,
    };
    use crate::environment::native::recovery::NativeRecoveryMarkerStore;
    use crate::environment::recovery::RecoveryMarkerStore;
    use crate::environment::runtime::ExecutionBackend;
    use crate::error::AppError;
    use crate::models::{ParsedSource, SourceType};

    struct CountingDetector {
        inner: RuntimeSourceEvidenceDetector,
        calls: Arc<AtomicUsize>,
    }

    impl SourceEvidenceDetector for CountingDetector {
        fn detect<'a>(
            &'a self,
            request: EvidenceDetectionRequest,
            previous: Option<crate::application::source_evidence::RemoteEvidenceEntry>,
            cancellation: CancellationSignal,
        ) -> EvidenceFuture<'a> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.inner.detect(request, previous, cancellation)
        }
    }

    struct BareSkillRepo {
        _root: tempfile::TempDir,
        work: PathBuf,
        remote: PathBuf,
        server: GitDaemon,
        revision: AtomicUsize,
    }

    impl BareSkillRepo {
        fn new(skill_names: &[&str]) -> Self {
            let root = tempfile::tempdir().expect("bare repository tempdir");
            let work = root.path().join("work");
            let remote = root.path().join("remote.git");
            run_git(
                root.path(),
                &[
                    "init",
                    "-q",
                    "-b",
                    "main",
                    work.to_str().expect("work path"),
                ],
            );
            run_git(&work, &["config", "user.email", "test@example.com"]);
            run_git(&work, &["config", "user.name", "Skill Deck Test"]);
            run_git(&work, &["config", "commit.gpgsign", "false"]);
            for skill_name in skill_names {
                write_remote_skill(&work, skill_name, "v1").expect("write initial Skill");
            }
            run_git(&work, &["add", "-A"]);
            run_git(&work, &["commit", "-q", "-m", "initial"]);
            run_git(
                root.path(),
                &[
                    "clone",
                    "-q",
                    "--bare",
                    work.to_str().expect("work path"),
                    remote.to_str().expect("remote path"),
                ],
            );
            let server = GitDaemon::new(root.path().to_path_buf());
            Self {
                _root: root,
                work,
                remote,
                server,
                revision: AtomicUsize::new(1),
            }
        }

        fn source(&self) -> String {
            format!("git://{}/remote.git", self.server.addr)
        }

        fn clone_count(&self) -> usize {
            self.server.clone_count.load(Ordering::SeqCst)
        }

        fn publish_change(&self, skill_name: &str) {
            let revision = self.revision.fetch_add(1, Ordering::SeqCst) + 1;
            write_remote_skill(&self.work, skill_name, &format!("v{revision}"))
                .expect("write changed Skill");
            run_git(&self.work, &["add", "-A"]);
            run_git(
                &self.work,
                &["commit", "-q", "-m", &format!("change {skill_name}")],
            );
            run_git(
                &self.work,
                &[
                    "push",
                    "-q",
                    self.remote.to_str().expect("remote path"),
                    "main",
                ],
            );
        }

        fn computed_hash(&self, skill_name: &str) -> String {
            let payload = build_skill_payload(&self.work.join("skills").join(skill_name))
                .expect("build expected payload");
            compute_cli_project_hash_from_payload(&payload).expect("compute expected CLI hash")
        }
    }

    struct GitDaemon {
        addr: SocketAddr,
        clone_count: Arc<AtomicUsize>,
        stopped: Arc<AtomicBool>,
        worker: Option<thread::JoinHandle<()>>,
        child: Child,
    }

    impl GitDaemon {
        fn new(root: PathBuf) -> Self {
            let inner_listener = TcpListener::bind("127.0.0.1:0").expect("reserve Git port");
            let inner_addr = inner_listener.local_addr().expect("Git daemon address");
            drop(inner_listener);
            let child = Command::new("git")
                .args([
                    "daemon",
                    "--export-all",
                    "--reuseaddr",
                    "--listen=127.0.0.1",
                    &format!("--port={}", inner_addr.port()),
                    &format!("--base-path={}", root.display()),
                    root.to_str().expect("repository root"),
                ])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .expect("start Git daemon");
            for _ in 0..100 {
                if TcpStream::connect(inner_addr).is_ok() {
                    break;
                }
                thread::sleep(std::time::Duration::from_millis(5));
            }

            let listener = TcpListener::bind("127.0.0.1:0").expect("bind Git proxy");
            listener
                .set_nonblocking(true)
                .expect("nonblocking Git proxy");
            let addr = listener.local_addr().expect("Git proxy address");
            let clone_count = Arc::new(AtomicUsize::new(0));
            let stopped = Arc::new(AtomicBool::new(false));
            let worker = {
                let clone_count = clone_count.clone();
                let stopped = stopped.clone();
                thread::spawn(move || {
                    while !stopped.load(Ordering::SeqCst) {
                        match listener.accept() {
                            Ok((stream, _)) => {
                                let clone_count = clone_count.clone();
                                thread::spawn(move || {
                                    proxy_git_connection(stream, inner_addr, clone_count)
                                });
                            }
                            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
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
                stopped,
                worker: Some(worker),
                child,
            }
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
        let mut client_reader = client.try_clone().expect("clone proxy client");
        let mut upstream_writer = upstream.try_clone().expect("clone proxy upstream");
        let inbound = thread::spawn(move || {
            let mut buffer = [0_u8; 8 * 1024];
            let mut recent = Vec::new();
            let mut counted = false;
            loop {
                match client_reader.read(&mut buffer) {
                    Ok(0) => break,
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
                        if upstream_writer.write_all(&buffer[..length]).is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
            let _ = upstream_writer.shutdown(Shutdown::Write);
        });
        let _ = io::copy(&mut upstream, &mut client);
        let _ = client.shutdown(Shutdown::Write);
        let _ = inbound.join();
    }

    fn run_git(cwd: &Path, args: &[&str]) {
        let status = Command::new("git")
            .current_dir(cwd)
            .args(args)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .expect("run Git fixture command");
        assert!(status.success(), "Git fixture command failed: {args:?}");
    }

    fn write_remote_skill(root: &Path, skill_name: &str, version: &str) -> Result<(), AppError> {
        let skill_root = root.join("skills").join(skill_name);
        fs::create_dir_all(skill_root.join("scripts"))?;
        fs::create_dir_all(skill_root.join("references"))?;
        fs::create_dir_all(skill_root.join("assets"))?;
        fs::write(
            skill_root.join("SKILL.md"),
            format!("---\nname: {skill_name}\ndescription: {version} lifecycle skill\n---\n"),
        )?;
        fs::write(
            skill_root.join("scripts/run.sh"),
            format!("#!/bin/sh\necho {skill_name}-{version}\n"),
        )?;
        fs::write(
            skill_root.join("references/guide.md"),
            format!("{skill_name}-{version}-guide"),
        )?;
        fs::write(
            skill_root.join("assets/payload.bin"),
            format!("{skill_name}-{version}-asset").as_bytes(),
        )?;
        Ok(())
    }

    struct StageFailureEntryExecutor {
        inner: NativePreparedEntryExecutor,
        failing_skill: String,
        private_root: PathBuf,
    }

    impl PreparedEntryExecutor for StageFailureEntryExecutor {
        type Staged = NativePreparedEntrySet;

        fn stage<'a>(
            &'a self,
            unit: &'a ExecutionUnit,
            payloads: &'a BTreeMap<PayloadId, PinnedPayloadLease>,
            cancellation: CancellationSignal,
        ) -> BoxFuture<'a, Result<Self::Staged, AppError>> {
            Box::pin(async move {
                if unit.skill_name != self.failing_skill {
                    return self.inner.stage(unit, payloads, cancellation).await;
                }
                let backup = self.private_root.with_extension("lifecycle-backup");
                fs::rename(&self.private_root, &backup)?;
                fs::write(
                    &self.private_root,
                    b"force a real not-a-directory staging error",
                )?;
                let staged = self.inner.stage(unit, payloads, cancellation).await;
                let remove_result = fs::remove_file(&self.private_root);
                let restore_result = fs::rename(&backup, &self.private_root);
                if let Err(error) = remove_result.and(restore_result) {
                    return Err(AppError::ExecutionFailed {
                        message: format!("failed to restore staging fixture: {error}"),
                    });
                }
                staged
            })
        }

        fn recheck_entries<'a>(
            &'a self,
            staged: &'a Self::Staged,
        ) -> BoxFuture<'a, Result<(), AppError>> {
            self.inner.recheck_entries(staged)
        }

        fn swap<'a>(&'a self, staged: &'a mut Self::Staged) -> BoxFuture<'a, Result<(), AppError>> {
            self.inner.swap(staged)
        }

        fn verify<'a>(&'a self, staged: &'a Self::Staged) -> BoxFuture<'a, Result<(), AppError>> {
            self.inner.verify(staged)
        }

        fn restore<'a>(
            &'a self,
            staged: &'a mut Self::Staged,
        ) -> BoxFuture<'a, Result<(), AppError>> {
            self.inner.restore(staged)
        }

        fn cleanup<'a>(
            &'a self,
            staged: Self::Staged,
        ) -> BoxFuture<'a, Result<Vec<MutationWarning>, AppError>> {
            self.inner.cleanup(staged)
        }
    }

    struct StageFailurePlanExecutor {
        environments: Arc<EnvironmentRegistry>,
        facts: RuntimePlanningFactSource,
        recovery_root: PathBuf,
        private_root: PathBuf,
    }

    impl InstallPlanExecutor for StageFailurePlanExecutor {
        fn execute<'a>(
            &'a self,
            plan: MutationPlan,
            cancellation: CancellationSignal,
        ) -> InstallFuture<'a, Vec<MutationUnitResult>> {
            Box::pin(async move {
                let recovery: Arc<dyn RecoveryMarkerStore> = Arc::new(
                    NativeRecoveryMarkerStore::new(&self.recovery_root)
                        .expect("native lifecycle recovery store"),
                );
                let entries = StageFailureEntryExecutor {
                    inner: NativePreparedEntryExecutor::new(
                        if cfg!(windows) {
                            ExecutionBackend::NativeWindows
                        } else {
                            ExecutionBackend::NativeUnix
                        },
                        plan.operation_id.clone(),
                        recovery,
                    ),
                    failing_skill: "beta".to_string(),
                    private_root: self.private_root.clone(),
                };
                MutationCoordinator::new(
                    entries,
                    RuntimeLockCommitter::new(self.environments.clone()),
                    self.facts.clone(),
                )
                .execute(plan, cancellation)
                .await
            })
        }
    }

    struct UpdateLifecycleFixture {
        _root: tempfile::TempDir,
        remote: BareSkillRepo,
        project_path: PathBuf,
        recovery_root: PathBuf,
        environments: Arc<EnvironmentRegistry>,
        facts: RuntimePlanningFactSource,
        targets: RuntimeTargetFactResolver,
        payloads: Arc<PayloadSessionManager>,
        execution: RuntimeExecutionDependencies,
        snapshots: Arc<SourceSnapshotReuseIndex>,
        evidence: SourceEvidenceCoordinator,
        detector_calls: Arc<AtomicUsize>,
        automatic_checks: AtomicUsize,
        preview_clone_count: AtomicUsize,
        final_hashes: Mutex<Option<(String, String)>>,
    }

    impl UpdateLifecycleFixture {
        async fn new(skill_names: [&str; 2]) -> Self {
            let root = tempfile::tempdir().expect("lifecycle fixture tempdir");
            let project_path = root.path().join("project");
            let projects_path = root.path().join("state/projects.json");
            let global_lock_path = root.path().join("state/global-lock.json");
            let recovery_root = root.path().join("recovery");
            let home = root.path().join("home");
            let config_home = root.path().join("config");
            fs::create_dir_all(project_path.join(".builtin"))
                .expect("create built-in fixture root");
            fs::create_dir_all(project_path.join(".custom")).expect("create custom fixture root");
            fs::create_dir_all(projects_path.parent().expect("state parent"))
                .expect("create state root");
            fs::create_dir_all(&home).expect("create fixture home");
            fs::create_dir_all(&config_home).expect("create fixture config home");
            write_json(
                &projects_path,
                &json!({
                    "schemaVersion": 1,
                    "projects": [project("source", &project_path, "Source")]
                }),
            )
            .expect("write projects fixture");
            write_json(
                &project_path.join("skills-lock.json"),
                &json!({ "version": 1, "skills": {} }),
            )
            .expect("write lifecycle lock");

            let environments = Arc::new(EnvironmentRegistry::default());
            let registry = Arc::new(StaticRegistry(Arc::new(test_registry())));
            let facts = RuntimePlanningFactSource::with_host_snapshot(
                registry,
                environments.clone(),
                HostRuntimeSnapshot {
                    home,
                    config_home,
                    projects_path,
                    global_lock_path,
                    environment_variables: BTreeMap::new(),
                },
            );
            let targets = RuntimeTargetFactResolver::new(environments.clone());
            let payloads = Arc::new(PayloadSessionManager::in_memory(
                PayloadSessionLimits {
                    ttl_ms: 30 * 60 * 1_000,
                    max_sessions: 16,
                    max_bytes: 64 * 1024 * 1024,
                },
                || 1_000,
            ));
            let snapshots = Arc::new(SourceSnapshotReuseIndex::default());
            let detector_calls = Arc::new(AtomicUsize::new(0));
            let detector = Arc::new(CountingDetector {
                inner: RuntimeSourceEvidenceDetector::new(
                    payloads.clone(),
                    environments.clone(),
                    snapshots.clone(),
                ),
                calls: detector_calls.clone(),
            });
            let evidence =
                SourceEvidenceCoordinator::with_snapshot_reuse(detector, snapshots.clone());
            let execution =
                RuntimeExecutionDependencies::new(environments.clone(), recovery_root.clone())
                    .expect("lifecycle execution dependencies");
            let remote = BareSkillRepo::new(&skill_names);
            Self {
                _root: root,
                remote,
                project_path,
                recovery_root,
                environments,
                facts,
                targets,
                payloads,
                execution,
                snapshots,
                evidence,
                detector_calls,
                automatic_checks: AtomicUsize::new(0),
                preview_clone_count: AtomicUsize::new(0),
                final_hashes: Mutex::new(None),
            }
        }

        fn context(&self) -> ContextRef {
            project_context("source")
        }

        fn executor(&self) -> RuntimePlanExecutor {
            executor(&self.execution, &self.environments, &self.facts)
        }

        fn update_service(&self) -> RuntimeUpdateService {
            UpdateService::new(
                self.payloads.clone(),
                ConcreteUpdatePlanner::new(
                    self.facts.clone(),
                    self.targets.clone(),
                    self.payloads.clone(),
                    fixed_time,
                ),
                RuntimeUpdatePayloadAcquirer::new(
                    self.payloads.clone(),
                    self.environments.clone(),
                    self.snapshots.clone(),
                    self.evidence.clone(),
                ),
                self.executor(),
            )
        }

        fn check_service(&self) -> UpdateCheckService<RuntimePlanningFactSource> {
            UpdateCheckService::new(self.facts.clone(), self.evidence.clone())
        }

        async fn install(&self) {
            let discovery =
                SourceDiscoveryService::new(self.payloads.clone(), self.environments.as_ref())
                    .discover_parsed_with_cancellation(
                        self.context(),
                        ParsedSource {
                            source_type: SourceType::Git,
                            url: self.remote.source(),
                            subpath: None,
                            local_path: None,
                            git_ref: Some("main".to_string()),
                            skill_filter: None,
                        },
                        self.remote.source(),
                        |_| {},
                        CancellationSignal::default(),
                    )
                    .await
                    .expect("discover lifecycle source");
            let handles = SelectedPayloadAcquisitionService::new(self.payloads.clone())
                .acquire(AcquireSelectedPayloadsRequest {
                    discovery_session: discovery.discovery_session.clone(),
                    skill_paths: vec![
                        "skills/alpha/SKILL.md".to_string(),
                        "skills/beta/SKILL.md".to_string(),
                    ],
                })
                .await
                .expect("acquire lifecycle payloads");
            let install = InstallService::new(
                self.payloads.clone(),
                ConcreteInstallPlanner::new(
                    self.facts.clone(),
                    self.targets.clone(),
                    self.payloads.clone(),
                    fixed_time,
                ),
                self.executor(),
            );
            for (skill_name, handle) in ["alpha", "beta"].into_iter().zip(handles) {
                let request = InstallRequest {
                    context: self.context(),
                    source: self.remote.source(),
                    discovery_session: discovery.discovery_session.clone(),
                    payloads: vec![handle],
                    skills: vec![skill_name.to_string()],
                    agent_intents: both_agent_intents(),
                    requested_mode: InstallMode::Copy,
                    acknowledge_risk: true,
                };
                let preview = install
                    .preview(&request)
                    .await
                    .expect("preview lifecycle install");
                let installed = install
                    .execute(&request, preview.token, CancellationSignal::default())
                    .await
                    .expect("execute lifecycle install");
                assert_succeeded(&installed.units);
            }
            assert_eq!(self.clone_count(), 1);
            fs::write(
                self.project_path.join(".builtin/skills/alpha/SKILL.md"),
                b"local alpha conflict\n",
            )
            .expect("write preserved conflict");
        }

        async fn check_automatic(&self) {
            let before = self.clone_count();
            let response = self
                .check_service()
                .check(&UpdateCheckRequest {
                    context: self.context(),
                    mode: UpdateCheckMode::Automatic,
                    selection: UpdateCheckSelection::All,
                })
                .await
                .expect("automatic update check");
            assert_eq!(response.skills.len(), 2);
            assert!(response.skills.iter().all(|skill| !skill.has_update));
            let ordinal = self.automatic_checks.fetch_add(1, Ordering::SeqCst);
            assert_eq!(self.clone_count(), before + usize::from(ordinal == 0));
        }

        fn detector_calls(&self) -> usize {
            self.detector_calls.load(Ordering::SeqCst)
        }

        fn clone_count(&self) -> usize {
            self.remote.clone_count()
        }

        async fn publish_change(&self, skill_name: &str) {
            self.remote.publish_change(skill_name);
        }

        async fn check_force_for(&self, skill_name: &str) {
            let before = self.clone_count();
            let response = self
                .check_service()
                .check(&UpdateCheckRequest {
                    context: self.context(),
                    mode: UpdateCheckMode::Force,
                    selection: UpdateCheckSelection::Skills(vec![SkillIdentity {
                        context: self.context(),
                        skill_name: skill_name.to_string(),
                    }]),
                })
                .await
                .expect("forced update check");
            assert_eq!(response.skills.len(), 1);
            assert!(response.skills[0].has_update, "{response:#?}");
            assert_eq!(self.clone_count(), before + 1);
            assert_eq!(self.detector_calls(), 2);
        }

        async fn preview<const N: usize>(&self, skill_names: [&str; N]) -> UpdatePreview {
            let before = self.clone_count();
            let preview = self
                .update_service()
                .preview(&UpdateRequest {
                    context: self.context(),
                    skill_names: skill_names.into_iter().map(ToString::to_string).collect(),
                })
                .await
                .expect("preview lifecycle update");
            assert_eq!(self.clone_count(), before);
            self.preview_clone_count.store(before, Ordering::SeqCst);
            preview
        }

        async fn cancel(&self, preview: UpdatePreview) {
            drop(preview);
            assert_eq!(
                self.clone_count(),
                self.preview_clone_count.load(Ordering::SeqCst)
            );
        }

        async fn confirm_preserving_conflicts<const N: usize>(
            &self,
            skill_names: [&str; N],
        ) -> UpdateResponse {
            let request = UpdateRequest {
                context: self.context(),
                skill_names: skill_names.into_iter().map(ToString::to_string).collect(),
            };
            let service = self.update_service();
            let preview = service
                .preview(&request)
                .await
                .expect("confirm update preview");
            assert_eq!(preview.skills[0].clean_copy_count, 1);
            assert_eq!(preview.skills[0].overwrite_private_entries.len(), 1);
            let response = service
                .execute(
                    &UpdateExecutionRequest {
                        request,
                        overwrite_private_entries: Vec::new(),
                    },
                    preview.token,
                    CancellationSignal::default(),
                )
                .await
                .expect("confirm lifecycle update");
            assert_eq!(
                fs::read_to_string(self.project_path.join(".builtin/skills/alpha/SKILL.md"))
                    .expect("read preserved conflict"),
                "local alpha conflict\n"
            );
            assert_eq!(
                fs::read_to_string(self.project_path.join(".custom/skills/alpha/SKILL.md"))
                    .expect("read clean copy"),
                fs::read_to_string(self.remote.work.join("skills/alpha/SKILL.md"))
                    .expect("read upstream alpha")
            );
            response
        }

        async fn assert_source_partial(&self) {
            let lock_path = self.project_path.join("skills-lock.json");
            let before = read_json(&lock_path).expect("read source partial lock");
            let saved_beta = before["skills"]["beta"].clone();
            let mut changed = before;
            let beta = changed["skills"]["beta"]
                .as_object_mut()
                .expect("beta lock object");
            beta.insert("source".to_string(), json!("unavailable/source"));
            beta.insert("sourceType".to_string(), json!("git"));
            beta.insert(
                "sourceUrl".to_string(),
                json!("git://127.0.0.1:1/unavailable.git"),
            );
            beta.insert("ref".to_string(), json!("main"));
            write_json(&lock_path, &changed).expect("write source partial lock");

            let request = UpdateRequest {
                context: self.context(),
                skill_names: vec!["alpha".to_string(), "beta".to_string()],
            };
            let service = self.update_service();
            let preview = service
                .preview(&request)
                .await
                .expect("source partial preview");
            let response = service
                .execute(
                    &UpdateExecutionRequest {
                        request,
                        overwrite_private_entries: Vec::new(),
                    },
                    preview.token,
                    CancellationSignal::default(),
                )
                .await
                .expect("source partial response");
            assert_eq!(response.outcome, UpdateOutcome::Partial);
            assert_eq!(
                response
                    .sources
                    .iter()
                    .filter(|source| source.status == UpdateSourceStatus::Acquired)
                    .count(),
                1
            );
            assert_eq!(
                response
                    .sources
                    .iter()
                    .filter(|source| source.status == UpdateSourceStatus::Failed)
                    .count(),
                1
            );

            let mut current = read_json(&lock_path).expect("read current partial lock");
            current["skills"]["beta"] = saved_beta;
            write_json(&lock_path, &current).expect("restore beta source metadata");
        }

        async fn assert_per_skill_staging_partial(&self) {
            fs::write(
                self.project_path.join(".custom/skills/alpha/SKILL.md"),
                b"second local alpha conflict\n",
            )
            .expect("write second preserved conflict");
            let lock_path = self.project_path.join("skills-lock.json");
            let beta_before = read_json(&lock_path).expect("read pre-partial lock")["skills"]
                ["beta"]["computedHash"]
                .as_str()
                .expect("beta computed hash")
                .to_string();
            self.remote.publish_change("beta");
            let before_clones = self.clone_count();
            let request = UpdateRequest {
                context: self.context(),
                skill_names: vec!["alpha".to_string(), "beta".to_string()],
            };
            let service = UpdateService::new(
                self.payloads.clone(),
                ConcreteUpdatePlanner::new(
                    self.facts.clone(),
                    self.targets.clone(),
                    self.payloads.clone(),
                    fixed_time,
                ),
                RuntimeUpdatePayloadAcquirer::new(
                    self.payloads.clone(),
                    self.environments.clone(),
                    self.snapshots.clone(),
                    self.evidence.clone(),
                ),
                StageFailurePlanExecutor {
                    environments: self.environments.clone(),
                    facts: self.facts.clone(),
                    recovery_root: self.recovery_root.clone(),
                    private_root: self.project_path.join(".custom/skills"),
                },
            );
            let preview = service
                .preview(&request)
                .await
                .expect("staging partial preview");
            let alpha = preview
                .skills
                .iter()
                .find(|skill| skill.skill_name == "alpha")
                .expect("alpha update preview");
            let beta = preview
                .skills
                .iter()
                .find(|skill| skill.skill_name == "beta")
                .expect("beta update preview");
            assert_eq!(alpha.overwrite_private_entries.len(), 2);
            assert_eq!(beta.clean_copy_count, 2);
            let response = service
                .execute(
                    &UpdateExecutionRequest {
                        request,
                        overwrite_private_entries: Vec::new(),
                    },
                    preview.token,
                    CancellationSignal::default(),
                )
                .await
                .expect("staging partial response");
            assert_eq!(response.outcome, UpdateOutcome::Partial, "{response:#?}");
            assert_eq!(response.sources.len(), 1);
            assert_eq!(response.sources[0].status, UpdateSourceStatus::Acquired);
            let alpha = response
                .skills
                .iter()
                .find(|skill| skill.skill_identity.skill_name == "alpha")
                .expect("alpha update result");
            let beta = response
                .skills
                .iter()
                .find(|skill| skill.skill_identity.skill_name == "beta")
                .expect("beta update result");
            assert_eq!(
                alpha.mutation.as_ref().map(|mutation| mutation.status),
                Some(MutationUnitStatus::Succeeded)
            );
            assert_eq!(
                beta.mutation.as_ref().map(|mutation| mutation.status),
                Some(MutationUnitStatus::Failed)
            );
            assert_eq!(self.clone_count(), before_clones + 1);
            *self.final_hashes.lock().expect("final hashes lock") =
                Some((self.remote.computed_hash("alpha"), beta_before));
        }

        async fn assert_final_lock_and_no_second_check(&self) {
            let (expected_alpha, expected_beta) = self
                .final_hashes
                .lock()
                .expect("final hashes lock")
                .clone()
                .expect("final hash expectations");
            let lock = read_json(&self.project_path.join("skills-lock.json"))
                .expect("read final lifecycle lock");
            assert_eq!(lock["skills"]["alpha"]["computedHash"], expected_alpha);
            assert_eq!(lock["skills"]["beta"]["computedHash"], expected_beta);
            assert_eq!(lock["skills"]["alpha"]["ref"], "main");
            assert_eq!(lock["skills"]["beta"]["ref"], "main");
            assert_eq!(self.detector_calls(), 2);
            assert_no_staging_leaks(self._root.path()).expect("no lifecycle staging leaks");
            assert_recovery_graph_is_empty(&self.recovery_root)
                .expect("empty lifecycle recovery graph");
        }
    }

    #[tokio::test]
    async fn native_update_lifecycle_reuses_source_work_and_preserves_conflicts() {
        let fixture = UpdateLifecycleFixture::new(["alpha", "beta"]).await;
        fixture.install().await;

        fixture.check_automatic().await;
        fixture.check_automatic().await;
        assert_eq!(fixture.detector_calls(), 1);

        fixture.publish_change("alpha").await;
        fixture.check_force_for("alpha").await;
        let preview = fixture.preview(["alpha"]).await;
        fixture.cancel(preview).await;
        let before = fixture.clone_count();
        let response = fixture.confirm_preserving_conflicts(["alpha"]).await;

        assert_eq!(fixture.clone_count(), before);
        assert_eq!(response.outcome, UpdateOutcome::Succeeded);
        assert!(response.skills[0]
            .warnings
            .contains(&UpdateWarningCode::PreservedConflictingCopy));

        fixture.assert_source_partial().await;
        fixture.assert_per_skill_staging_partial().await;
        fixture.assert_final_lock_and_no_second_check().await;
    }
}

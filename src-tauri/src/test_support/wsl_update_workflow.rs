use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::application::agent_selection::build_agent_selection_catalog;
use crate::application::library_candidates::ResolvedLibraryCandidateIndex;
use crate::application::mutation::coordinator::{BoxFuture, RuntimeRevisionSource};
use crate::application::mutation::plan::RuntimeRevisions;
use crate::application::payload_session::{
    PayloadPlanningMetadata, PayloadSessionLimits, PayloadSessionManager,
};
use crate::application::planning_facts::{
    ScopePlanningFuture, ScopePlanningSnapshot, ScopePlanningSnapshotSource,
};
use crate::application::resources::AuthorizedResourceReader;
use crate::application::scope_skill_placements::{
    observe_scope_skill_placements, readable_placements,
};
use crate::application::skill_libraries::*;
use crate::application::skill_source::{
    AcquiredSavedSkillSource, SavedSkillSourceAcquisition, SavedSkillSourceGroup,
    SkillSourceFuture, SkillSourceModule,
};
use crate::application::update::{UpdateOutcome, UpdateRequest, UpdateService};
use crate::application::update_planner::ConcreteUpdatePlanner;
use crate::application::update_records::{
    InstalledUpdateRecordProvider, InstalledUpdateRecordSnapshots, UpdateRecordState,
};
use crate::core::lossless_lock::LockSchema;
use crate::core::mutation::CancellationSignal;
use crate::core::skill_payload::{build_skill_payload, compute_cli_project_hash_from_payload};
use crate::environment::agent_environment::{AgentEnvironmentResolver, EnvironmentContext};
use crate::environment::context_resolver::ResolvedContext;
use crate::environment::inspection::FilesystemInspector;
use crate::environment::native::recovery::NativeRecoveryMarkerStore;
use crate::environment::planning::RuntimeTargetFactResolver;
use crate::environment::runtime::ContextSnapshotRevision;
use crate::environment::types::{
    EnvironmentRef, EnvironmentStatus, RegisteredProject, ResourceLocator, SkillLocation,
    SkillLocationRef,
};
use crate::environment::wsl::operations::atomic_file::WslAtomicDocumentIo;
use crate::environment::wsl::operations::inspection::WslInspector;
use crate::environment::wsl::{WslRuntime, WslWorkspace};
use crate::error::AppError;
use crate::git_fixture::BareSkillRepo;
use crate::runtime::plan_runner::{RuntimeLockCommitter, RuntimePlanExecutor};
use crate::runtime::resource_service::RuntimeResourceReader;
use crate::storage::lock_plan::load_lock_document;

// 只替换与本用例无关的库数据；目标观察、内容存储和事务仍使用真实 Worker。
struct EmptyLibraries;

impl SkillLibraryRepository for EmptyLibraries {
    fn load<'a>(
        &'a self,
        _: &'a EnvironmentRef,
    ) -> LibraryFuture<'a, Result<LibraryCatalog, AppError>> {
        Box::pin(async { Ok(LibraryCatalog::default()) })
    }

    fn resolve_collection<'a>(
        &'a self,
        _: &'a EnvironmentRef,
        _: &'a LibraryId,
    ) -> LibraryFuture<'a, Result<crate::application::skill_paths::ResolvedSkillRoot, AppError>>
    {
        Box::pin(async { Err(AppError::StaleTarget) })
    }

    fn save<'a>(
        &'a self,
        _: &'a EnvironmentRef,
        _: &'a LibraryCatalog,
    ) -> LibraryFuture<'a, Result<(), AppError>> {
        Box::pin(async { Err(AppError::StaleTarget) })
    }

    fn commit_member<'a>(
        &'a self,
        _: CommitLibraryMemberRequest,
    ) -> LibraryFuture<'a, Result<(), AppError>> {
        Box::pin(async { Err(AppError::StaleTarget) })
    }

    fn purge_retired<'a>(
        &'a self,
        _: PurgeRetiredLibraryMemberRequest,
    ) -> LibraryFuture<'a, Result<(), AppError>> {
        Box::pin(async { Err(AppError::StaleTarget) })
    }

    fn delete_library<'a>(
        &'a self,
        _: &'a EnvironmentRef,
        _: &'a LibraryId,
    ) -> LibraryFuture<'a, Result<LibraryCatalog, AppError>> {
        Box::pin(async { Err(AppError::StaleTarget) })
    }

    fn read_skill_content<'a>(
        &'a self,
        _: &'a EnvironmentRef,
        _: &'a LibraryId,
        _: &'a str,
    ) -> LibraryFuture<'a, Result<String, AppError>> {
        Box::pin(async { Err(AppError::StaleTarget) })
    }
}

#[derive(Clone)]
struct FixtureFacts {
    initial: ScopePlanningSnapshot,
    workspace: WslWorkspace,
}

#[derive(Clone)]
struct FixtureLocations {
    source: FixtureFacts,
    target: FixtureFacts,
}

struct FixtureSkillResource(
    crate::application::resources::SkillIdentity,
    Vec<ResourceLocator>,
);

impl crate::application::resources::ResourceContextSource for FixtureSkillResource {
    fn resolve<'a>(
        &'a self,
        _: &'a SkillLocationRef,
    ) -> crate::application::resources::ResourceFuture<
        'a,
        Result<crate::application::resources::ResolvedResourceContext, AppError>,
    > {
        Box::pin(async { Err(AppError::StaleContext) })
    }

    fn resolve_skill_candidates<'a>(
        &'a self,
        identity: &'a crate::application::resources::SkillIdentity,
    ) -> crate::application::resources::ResourceFuture<'a, Result<Vec<ResourceLocator>, AppError>>
    {
        Box::pin(async move {
            if identity != &self.0 {
                return Err(AppError::StaleContext);
            }
            Ok(self.1.clone())
        })
    }
}

impl ScopePlanningSnapshotSource for FixtureLocations {
    fn snapshot<'a>(
        &'a self,
        context: &'a SkillLocationRef,
    ) -> ScopePlanningFuture<'a, Result<ScopePlanningSnapshot, AppError>> {
        if context == &self.source.initial.resolved_context.context {
            ScopePlanningSnapshotSource::snapshot(&self.source, context)
        } else {
            ScopePlanningSnapshotSource::snapshot(&self.target, context)
        }
    }
}

impl RuntimeRevisionSource for FixtureLocations {
    fn current<'a>(
        &'a self,
        context: &'a SkillLocationRef,
    ) -> BoxFuture<'a, Result<RuntimeRevisions, AppError>> {
        if context == &self.source.initial.resolved_context.context {
            self.source.current(context)
        } else {
            self.target.current(context)
        }
    }
}

impl ScopePlanningSnapshotSource for FixtureFacts {
    fn snapshot<'a>(
        &'a self,
        context: &'a SkillLocationRef,
    ) -> ScopePlanningFuture<'a, Result<ScopePlanningSnapshot, AppError>> {
        Box::pin(async move {
            if context != &self.initial.resolved_context.context {
                return Err(AppError::StaleContext);
            }
            let mut snapshot = self.initial.clone();
            snapshot.lock_document = load_lock_document(
                &WslAtomicDocumentIo::new(self.workspace.clone()),
                &snapshot.resolved_context.lock,
                None,
                snapshot.lock_schema,
            )
            .await?;
            if let Some(project) = &snapshot.resolved_context.project {
                snapshot.eve_targets = crate::environment::agent_environment::inspect_eve_project(
                    &self.workspace,
                    &project.native_path,
                )
                .await?
                .install_targets(&project.native_path);
            }
            Ok(snapshot)
        })
    }
}

impl RuntimeRevisionSource for FixtureFacts {
    fn current<'a>(
        &'a self,
        _: &'a SkillLocationRef,
    ) -> BoxFuture<'a, Result<RuntimeRevisions, AppError>> {
        Box::pin(async { Ok(self.initial.revisions.clone()) })
    }
}

// 来源由受控 Git 夹具提供，完整内容通过正式存储接口传给 WSL。
struct FixtureSource {
    relocation_source: Option<(
        crate::environment::wsl::WslWorkspace,
        crate::environment::wsl::WslSession,
        String,
    )>,
    root: PathBuf,
    payloads: Arc<PayloadSessionManager>,
    calls: Arc<AtomicUsize>,
}

impl SkillSourceModule for FixtureSource {
    fn acquire_saved_groups<'a>(
        &'a self,
        groups: &'a [SavedSkillSourceGroup],
        cancellation: CancellationSignal,
    ) -> SkillSourceFuture<'a, Result<Vec<SavedSkillSourceAcquisition>, AppError>> {
        Box::pin(async move {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let mut results = Vec::new();
            for group in groups {
                let discovery = if let Some((workspace, session, remote)) = &self.relocation_source
                {
                    use crate::application::payload_session::{
                        DiscoverySourceDescriptor, DiscoverySourceLocation, RetainedDiscoverySource,
                    };
                    use crate::environment::wsl::operations::source_acquisition::{
                        acquire_wsl_source_native, WslAcquisitionSource,
                    };
                    let native = acquire_wsl_source_native(
                        workspace.clone(),
                        session,
                        WslAcquisitionSource::Git {
                            url: remote.clone(),
                            git_ref: group.skills[0].metadata.ref_name.clone(),
                        },
                        std::time::Duration::from_secs(30),
                        None,
                        cancellation.clone(),
                    )
                    .await?;
                    let storage = Arc::new(crate::environment::wsl::operations::acquire::WslPayloadSessionStorage::for_source(workspace.clone(), &native));
                    let retained = RetainedDiscoverySource::new(
                        DiscoverySourceLocation::WslNative {
                            distro_name: session.distro_name.clone(),
                            linux_root: native.native_root().into(),
                            ref_revision: native.ref_revision().map(str::to_string),
                        },
                        DiscoverySourceDescriptor {
                            source: group.skills[0].metadata.source.clone(),
                            source_type: "git".into(),
                            source_url: group.skills[0].metadata.source_url.clone(),
                            ref_name: group.skills[0].metadata.ref_name.clone(),
                            ref_kind: None,
                            redirected_download_host: None,
                        },
                        BTreeMap::new(),
                        native,
                    );
                    self.payloads
                        .discover_with_source(
                            group.environment.clone(),
                            format!("wsl-relocation-{}", uuid::Uuid::new_v4()),
                            storage,
                            retained,
                        )
                        .await?
                } else {
                    self.payloads
                        .discover(
                            group.environment.clone(),
                            format!("wsl-cli-fixture-{}", uuid::Uuid::new_v4()),
                        )
                        .await?
                };
                let mut handles = Vec::new();
                let mut leases = Vec::new();
                for skill in &group.skills {
                    if cancellation.is_cancelled() {
                        return Err(AppError::MutationCancelled);
                    }
                    if self.relocation_source.is_some() {
                        let selected = crate::application::source_acquisition::SelectedPayloadAcquisitionService::new(self.payloads.clone());
                        let path = selected
                            .resolve_saved_skill_path(
                                &discovery,
                                &skill.name,
                                skill.skill_path(),
                                cancellation.clone(),
                            )
                            .await?;
                        let handle = selected.acquire(crate::application::source_acquisition::AcquireSelectedPayloadsRequest { discovery_session: discovery.clone(), skill_paths: vec![path] }).await?.remove(0);
                        leases.push(Arc::new(self.payloads.pin_verified(&handle).await?));
                        handles.push((skill.name.clone(), handle));
                        continue;
                    }
                    let path =
                        crate::core::skill_paths::normalize_skill_folder_path(skill.skill_path());
                    let payload = build_skill_payload(&self.root.join(&path))?;
                    let computed_hash = compute_cli_project_hash_from_payload(&payload)?;
                    let handle = self.payloads.acquire_payload_with_metadata(
                        &discovery, path.clone(), payload,
                        PayloadPlanningMetadata {
                            skill_name: skill.name.clone(),
                            install_dir_name: crate::application::installed_skill_resolver::InstalledSkillResolver::install_dir_name(&skill.name)?,
                            source: skill.metadata.source.clone(),
                            source_type: skill.metadata.source_type.clone(),
                            source_url: skill.metadata.source_url.clone(),
                            ref_name: skill.metadata.ref_name.clone(),
                            ref_kind: skill.metadata.ref_kind,
                            skill_path: path,
                            plugin_name: None,
                            computed_hash,
                            upstream_revision: None,
                            well_known: None,
                        },
                    ).await?;
                    leases.push(Arc::new(self.payloads.pin_verified(&handle).await?));
                    handles.push((skill.name.clone(), handle));
                }
                results.push(SavedSkillSourceAcquisition {
                    source_result_id: group.source_result_id.clone(),
                    source: group.source.clone(),
                    skill_names: group
                        .skills
                        .iter()
                        .map(|skill| skill.name.clone())
                        .collect(),
                    result: Ok(AcquiredSavedSkillSource {
                        discovery_session: discovery,
                        payloads: handles,
                        skill_errors: Vec::new(),
                        redirected_download_hosts: Vec::new(),
                        _leases: leases,
                    }),
                });
            }
            Ok(results)
        })
    }
}

async fn run_fixture(
    distro: &str,
    script: &str,
    arguments: &[String],
) -> Result<Vec<u8>, AppError> {
    let mut command: tokio::process::Command =
        crate::background_process::std_command("wsl.exe").into();
    command
        .args([
            "--distribution",
            distro,
            "--exec",
            "/bin/sh",
            "-c",
            script,
            "--",
        ])
        .args(arguments)
        .kill_on_drop(true);
    let output = tokio::time::timeout(Duration::from_secs(30), command.output())
        .await
        .map_err(|_| AppError::ExecutionFailed {
            message: "WSL CLI fixture timed out".into(),
        })??;
    if !output.status.success() {
        return Err(AppError::ExecutionFailed {
            message: format!(
                "WSL fixture failed: {}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            ),
        });
    }
    Ok(output.stdout)
}

struct FixtureCleanup {
    distro: String,
    root: String,
}

impl Drop for FixtureCleanup {
    fn drop(&mut self) {
        if !self.root.starts_with("/tmp/skill-deck-cli-update-") {
            return;
        }
        let Ok(mut child) = crate::background_process::std_command("wsl.exe")
            .args([
                "--distribution",
                &self.distro,
                "--exec",
                "rm",
                "-rf",
                "--",
                &self.root,
            ])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
        else {
            return;
        };
        let deadline = Instant::now() + Duration::from_secs(10);
        while matches!(child.try_wait(), Ok(None)) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        if matches!(child.try_wait(), Ok(None)) {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

#[tokio::test]
#[ignore = "requires Windows, a matching Worker, and SKILL_DECK_TEST_WSL_NODE / SKILL_DECK_TEST_WSL_CLI"]
async fn real_wsl2_private_only_update_preserves_layout() {
    run_wsl_update(WslWorkflow::PrivateUpdate).await;
}

#[tokio::test]
#[ignore = "requires Windows, a matching Worker, and SKILL_DECK_TEST_WSL_NODE / SKILL_DECK_TEST_WSL_CLI"]
async fn real_wsl2_project_update_preserves_external_content() {
    run_wsl_update(WslWorkflow::ScopeBoundary).await;
}

#[tokio::test]
#[ignore = "requires Windows, a matching Worker, and SKILL_DECK_TEST_WSL_NODE / SKILL_DECK_TEST_WSL_CLI"]
async fn real_wsl2_eve_update_restores_recorded_missing_installation() {
    run_wsl_update(WslWorkflow::EveUpdate).await;
}

#[tokio::test]
#[ignore = "requires Windows, a matching Worker, and SKILL_DECK_TEST_WSL_NODE / SKILL_DECK_TEST_WSL_CLI"]
async fn real_wsl2_private_only_copy_manage_and_remove() {
    run_wsl_update(WslWorkflow::Adjacent).await;
}

#[tokio::test]
#[ignore = "requires Windows, a matching Worker, and SKILL_DECK_TEST_WSL_NODE / SKILL_DECK_TEST_WSL_CLI"]
async fn real_wsl2_dependency_copy_manage_remove_preserves_package() {
    run_wsl_update(WslWorkflow::Dependency).await;
}

#[tokio::test]
#[ignore = "requires Windows, a matching Worker, and SKILL_DECK_TEST_WSL_NODE / SKILL_DECK_TEST_WSL_CLI"]
async fn real_wsl2_agent_compatibility_preserves_current_and_legacy_installations() {
    run_wsl_update(WslWorkflow::AgentCompatibility).await;
}

#[tokio::test]
#[ignore = "requires Windows, WSL 2, and the skills CLI fixture"]
async fn real_wsl2_update_relocation_uses_worker_candidates_and_commits_new_path() {
    run_wsl_update(WslWorkflow::Relocation).await;
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum WslWorkflow {
    PrivateUpdate,
    ScopeBoundary,
    EveUpdate,
    Adjacent,
    Dependency,
    AgentCompatibility,
    Relocation,
}

async fn run_wsl_update(workflow: WslWorkflow) {
    let eve = workflow == WslWorkflow::EveUpdate;
    let distro = std::env::var("SKILL_DECK_TEST_WSL_DISTRO").unwrap_or_else(|_| "Ubuntu".into());
    let node = std::env::var("SKILL_DECK_TEST_WSL_NODE")
        .expect("set the target distribution Node executable");
    let cli = std::env::var("SKILL_DECK_TEST_WSL_CLI")
        .expect("set the target distribution skills@1.5.23 cli.mjs");
    let runtime = Arc::new(WslRuntime::for_wsl_test());
    let session = runtime
        .connect(&distro)
        .await
        .expect("connect the real Worker");
    let workspace = runtime.workspace(&distro).unwrap();
    let root = format!(
        "/tmp/skill-deck-cli-update-{}",
        uuid::Uuid::new_v4().simple()
    );
    let _cleanup = FixtureCleanup {
        distro: distro.clone(),
        root: root.clone(),
    };
    let remote = BareSkillRepo::new(&["alpha"]);
    let native_remote = url::Url::parse(&remote.local_source())
        .unwrap()
        .to_file_path()
        .unwrap();
    let mapped = workspace
        .map_host_path(native_remote.to_string_lossy().into_owned(), None)
        .await
        .unwrap();
    run_fixture(&distro, r#"set -eu
root=$1
node=$2
cli=$3
remote=$4
public=$5
mkdir -p "$root/project/.codebuddy" "$root/home"
git config --file "$root/home/gitconfig" "url.file://$remote.insteadOf" "$public"
git config --file "$root/home/gitconfig" protocol.file.allow always
cd "$root/project"
if [ "$6" = eve ]; then
  mkdir -p agent/subagents/research agent/subagents/writer
  printf '%s\n' '{"dependencies":{"eve":"*"}}' > package.json
  HOME="$root/home" USERPROFILE="$root/home" GIT_CONFIG_GLOBAL="$root/home/gitconfig" GIT_CONFIG_NOSYSTEM=1 DISABLE_TELEMETRY=1 DO_NOT_TRACK=1 "$node" "$cli" add "$public" --skill alpha --agent eve --subagent research writer --yes
else
  HOME="$root/home" USERPROFILE="$root/home" GIT_CONFIG_GLOBAL="$root/home/gitconfig" GIT_CONFIG_NOSYSTEM=1 DISABLE_TELEMETRY=1 DO_NOT_TRACK=1 "$node" "$cli" add "$public" --skill alpha --agent codebuddy --yes
fi
"#, &[root.clone(), node, cli, mapped.clone(), remote.source(), if eve { "eve".into() } else { "private".into() }]).await.unwrap();

    let environment = EnvironmentRef::Wsl {
        distro_name: distro.clone(),
    };
    let locator = |path: String| ResourceLocator {
        environment: environment.clone(),
        native_path: path,
    };
    let context = SkillLocationRef {
        environment: environment.clone(),
        scope: SkillLocation::Project {
            project_id: "fixture".into(),
        },
    };
    let project = format!("{root}/project");
    let home = format!("{root}/home");
    let environment_revision = format!("fixture-worker-{}", session.runtime_generation);
    let registry = crate::native_workflow_integration_support::test_registry();
    let agent_runtime = AgentEnvironmentResolver::from_environment(EnvironmentContext {
        environment: environment.clone(),
        home: home.clone(),
        config_home: format!("{home}/.config"),
        environment_variables: BTreeMap::new(),
        availability: EnvironmentStatus::Available,
        revision: environment_revision.clone(),
        wsl_workspace: Some(workspace.clone()),
    })
    .resolve_registry(&registry, Some(&project))
    .await
    .unwrap();
    let facts = FixtureFacts {
        initial: ScopePlanningSnapshot {
            resolved_context: ResolvedContext {
                context: context.clone(),
                project: Some(RegisteredProject {
                    id: "fixture".into(),
                    native_path: project.clone(),
                    display_name: Some("WSL fixture".into()),
                    order: None,
                    suppress_cross_storage_warning: false,
                }),
                home: locator(home),
                skill_root: locator(format!("{project}/.agents/skills")),
                lock: locator(format!("{project}/skills-lock.json")),
            },
            agent_runtime,
            revisions: RuntimeRevisions {
                registry: registry.revision.clone(),
                environment: environment_revision,
                context: ContextSnapshotRevision::parse("context-v1-wsl-cli-fixture").unwrap(),
            },
            lock_schema: LockSchema::Project,
            lock_document: crate::core::lossless_lock::LosslessLockDocument::empty(
                LockSchema::Project,
            ),
            eve_targets: Vec::new(),
        },
        workspace: workspace.clone(),
    };
    if workflow == WslWorkflow::AgentCompatibility {
        verify_wsl_agent_directories(&facts, runtime).await;
        return;
    }
    let libraries: Arc<dyn SkillLibraryRepository> = Arc::new(EmptyLibraries);
    let targets = RuntimeTargetFactResolver::new(runtime.clone());
    if workflow == WslWorkflow::PrivateUpdate {
        let global =
            crate::environment::context_resolver::ContextResolver::resolve_wsl_from_projects(
                SkillLocationRef {
                    environment: environment.clone(),
                    scope: SkillLocation::Global,
                },
                &session,
                Vec::new(),
            )
            .unwrap();
        let base = global.path_base(&targets).await;
        assert_eq!(base.logical_root.native_path, session.home);
        assert_eq!(base.logical_root.environment, environment);
        assert_eq!(
            base.path_style,
            crate::environment::context_resolver::DisplayPathStyle::Posix
        );
        assert!(base.physical_root.is_some());

        let alias = format!("{root}/project-alias");
        run_fixture(
            &distro,
            "ln -s -- \"$1\" \"$2\"",
            &[project.clone(), alias.clone()],
        )
        .await
        .unwrap();
        let mut linked = facts.initial.resolved_context.clone();
        linked.project.as_mut().unwrap().native_path = alias.clone();
        let base = linked.path_base(&targets).await;
        assert_eq!(base.logical_root.native_path, alias);
        assert_eq!(base.physical_root.unwrap().native_path, project);
    }
    let records =
        InstalledUpdateRecordProvider::new(facts.clone(), targets.clone(), libraries.clone())
            .snapshot_installed_records(&context, ["alpha".to_string()].into())
            .await
            .unwrap();
    assert!(matches!(records[0].state, UpdateRecordState::Source(_)));
    let observed_facts = ScopePlanningSnapshotSource::snapshot(&facts, &context)
        .await
        .unwrap();
    let catalog = build_agent_selection_catalog(
        &context,
        &observed_facts.agent_runtime,
        &observed_facts.eve_targets,
        &observed_facts.resolved_context.skill_root,
        &targets,
    )
    .await
    .unwrap();
    let known = ResolvedLibraryCandidateIndex::load_known(
        libraries.as_ref(),
        &targets,
        &environment,
        &["alpha".try_into().unwrap()].into(),
    )
    .await
    .unwrap();
    let observed =
        observe_scope_skill_placements(&targets, &context, "alpha", &observed_facts, &catalog)
            .await
            .unwrap();
    let placements = observed.describe(&catalog, &known).unwrap();
    let readable = readable_placements(&placements);
    let selected = readable.first().unwrap();
    assert_eq!(
        selected.entry.fact.destination.native_path,
        if eve {
            format!("{project}/agent/subagents/research/skills/alpha")
        } else {
            format!("{project}/.codebuddy/skills/alpha")
        }
    );
    RuntimeResourceReader::new(runtime.clone())
        .read_skill(selected.entry.fact.destination.clone())
        .await
        .unwrap();

    let eve_targets = observed_facts
        .eve_targets
        .iter()
        .cloned()
        .map(|target| crate::models::SkillInstallTargetInfo {
            target_id: target.target_id,
            agent: target.agent,
            display_name: target.display_name,
            subagent: target.subagent,
            path: target.path,
        })
        .collect::<Vec<_>>();
    let mut read_plan = crate::application::skill_read::build_skill_read_plan(
        &observed_facts.resolved_context,
        &observed_facts.agent_runtime,
        &eve_targets,
    )
    .unwrap();
    read_plan.set_project_lock(Some(
        &observed_facts.lock_document.to_pretty_bytes().unwrap(),
    ));
    let inspector = Arc::new(WslInspector::new(workspace.clone()));
    let snapshot = inspector.inspect(&read_plan.read_plan).await.unwrap();
    let listed = crate::application::skill_read::project_direct_skill_snapshot(
        &read_plan,
        snapshot,
        &observed_facts.agent_runtime,
        libraries.as_ref(),
        &targets,
        inspector.as_ref(),
    )
    .await
    .unwrap();
    assert_eq!(listed.skills.len(), 1);
    assert_eq!(listed.skills[0].name, "alpha");

    let writer = format!("{project}/agent/subagents/writer/skills/alpha");
    if eve {
        run_fixture(&distro, "rm -rf -- \"$1\"", std::slice::from_ref(&writer))
            .await
            .unwrap();
    }

    remote.publish_change("alpha");
    if workflow == WslWorkflow::Relocation {
        remote.relocate_skill("skills/alpha", "tools/alpha");
    }
    let clock = Arc::new(AtomicU64::new(1_000));
    let read_clock = clock.clone();
    let manager = Arc::new(PayloadSessionManager::new(
        workspace.payload_storage(),
        PayloadSessionLimits {
            ttl_ms: 60_000,
            max_sessions: 4,
            max_bytes: 16 * 1024 * 1024,
        },
        move || read_clock.load(Ordering::SeqCst),
    ));
    let source_calls = Arc::new(AtomicUsize::new(0));
    let recovery = tempfile::tempdir().unwrap();
    let executor = RuntimePlanExecutor::new(
        runtime.clone(),
        Arc::new(NativeRecoveryMarkerStore::new(recovery.path()).unwrap()),
        Arc::new(RuntimeLockCommitter::new()),
        Arc::new(facts.clone()),
    );
    let service = UpdateService::new(
        manager.clone(),
        ConcreteUpdatePlanner::new(
            facts.clone(),
            targets.clone(),
            manager.clone(),
            libraries.clone(),
            || "2026-09-14T00:00:00Z".into(),
        ),
        FixtureSource {
            relocation_source: (workflow == WslWorkflow::Relocation)
                .then(|| (workspace.clone(), session.clone(), mapped.clone())),
            root: remote.work.clone(),
            payloads: manager.clone(),
            calls: source_calls.clone(),
        },
        executor,
    );
    let request = UpdateRequest {
        context: context.clone(),
        skill_names: vec!["alpha".into()],
    };
    if workflow == WslWorkflow::Relocation {
        let prepared = service
            .prepare(&request, CancellationSignal::default())
            .await
            .unwrap();
        assert!(
            prepared.preview.blocked.is_empty(),
            "{:?}",
            prepared.preview
        );
        assert_eq!(
            prepared.preview.skills[0]
                .path_change
                .as_ref()
                .unwrap()
                .next_path,
            "tools/alpha"
        );
        let selected = prepared
            .preview
            .skills
            .iter()
            .flat_map(|skill| &skill.targets)
            .filter_map(|target| target.selectable_entry_id.clone())
            .collect::<Vec<_>>();
        let response = service
            .execute_prepared(prepared, &selected, CancellationSignal::default(), |_| {})
            .await
            .unwrap();
        assert_eq!(response.outcome, UpdateOutcome::Succeeded, "{response:?}");
        let latest = ScopePlanningSnapshotSource::snapshot(&facts, &context)
            .await
            .unwrap();
        assert_eq!(
            latest
                .lock_document
                .entry_snapshot("alpha")
                .value()
                .unwrap()["skillPath"],
            "tools/alpha/SKILL.md"
        );
        let expected = workspace
            .map_host_path(
                remote
                    .work
                    .join("tools/alpha/SKILL.md")
                    .to_string_lossy()
                    .into_owned(),
                None,
            )
            .await
            .unwrap();
        run_fixture(
            &distro,
            "cmp \"$1\" \"$2\"",
            &[
                expected,
                format!("{project}/.codebuddy/skills/alpha/SKILL.md"),
            ],
        )
        .await
        .unwrap();
        drop(service);
        clock.store(120_000, Ordering::SeqCst);
        manager.cleanup().await.unwrap();
        return;
    }
    if workflow == WslWorkflow::ScopeBoundary {
        use crate::application::mutation::result::OperationErrorCode;
        use crate::application::update::UpdateCoverage;

        run_fixture(
            &distro,
            r#"set -eu
mv "$1/project/.codebuddy" "$1/external"
ln -s "$1/external" "$1/project/.codebuddy"
mkdir -p "$1/project/.agents/skills"
cp -R "$1/external/skills/alpha" "$1/project/.agents/skills/alpha"
cp -R "$1/external" "$1/external-before"
"#,
            std::slice::from_ref(&root),
        )
        .await
        .unwrap();
        let prepared = service
            .prepare(&request, CancellationSignal::default())
            .await
            .unwrap();
        assert!(
            prepared.preview.blocked.is_empty(),
            "{:#?}",
            prepared.preview
        );
        let preview = &prepared.preview.skills[0];
        assert_eq!(preview.targets.len(), 1);
        assert_eq!(
            preview.targets[0].display_path.native_path,
            format!("{project}/.agents/skills/alpha")
        );
        assert!(preview.overwrite_private_entries.is_empty());
        assert_eq!(preview.preserved_targets.as_ref().unwrap().len(), 1);
        let calls = source_calls.load(Ordering::SeqCst);
        let response = service
            .execute_prepared(prepared, &[], CancellationSignal::default(), |_| {})
            .await
            .unwrap();
        assert_eq!(response.outcome, UpdateOutcome::Succeeded, "{response:#?}");
        assert_eq!(source_calls.load(Ordering::SeqCst), calls);
        let guide = run_fixture(
            &distro,
            r#"set -eu
test -L "$1/project/.codebuddy"
diff -r "$1/external-before" "$1/external"
cat "$1/project/.agents/skills/alpha/references/guide.md"
"#,
            std::slice::from_ref(&root),
        )
        .await
        .unwrap();
        assert_eq!(
            guide,
            std::fs::read(remote.work.join("skills/alpha/references/guide.md")).unwrap()
        );
        run_fixture(
            &distro,
            "rm -rf -- \"$1/.agents/skills/alpha\"",
            std::slice::from_ref(&project),
        )
        .await
        .unwrap();
        let lock_before = run_fixture(
            &distro,
            "cat \"$1/skills-lock.json\"",
            std::slice::from_ref(&project),
        )
        .await
        .unwrap();
        let prepared = service
            .prepare(&request, CancellationSignal::default())
            .await
            .unwrap();
        assert!(
            prepared.preview.blocked.is_empty(),
            "{:#?}",
            prepared.preview
        );
        let preview = &prepared.preview.skills[0];
        assert_eq!(
            preview.blocking_reasons,
            vec![OperationErrorCode::NoUpdateTargets]
        );
        assert!(preview.targets.is_empty() && preview.overwrite_private_entries.is_empty());
        assert_eq!(source_calls.load(Ordering::SeqCst), calls);
        let response = service
            .execute_prepared(prepared, &[], CancellationSignal::default(), |_| {})
            .await
            .unwrap();
        assert!(
            matches!(&response.skills[0].coverage, UpdateCoverage::NotUpdated { error } if error.code == OperationErrorCode::NoUpdateTargets)
        );
        assert!(response.skills[0].mutation.is_none());
        let lock_after = run_fixture(
            &distro,
            r#"set -eu
diff -r "$1/external-before" "$1/external"
test ! -e "$1/project/.agents/skills/alpha"
cat "$1/project/skills-lock.json"
"#,
            std::slice::from_ref(&root),
        )
        .await
        .unwrap();
        assert_eq!(lock_after, lock_before);
        clock.store(120_000, Ordering::SeqCst);
        manager.cleanup().await.unwrap();
        return;
    }
    let mut prepared = service
        .prepare(&request, CancellationSignal::default())
        .await
        .unwrap();
    assert!(
        prepared.preview.blocked.is_empty(),
        "{:#?}",
        prepared.preview
    );
    let base = prepared
        .preview
        .path_base
        .as_ref()
        .expect("preview path base");
    assert_eq!(base.logical_root.native_path, project);
    assert_eq!(base.physical_root.as_ref().unwrap().native_path, project);
    assert_eq!(base.logical_root.environment, environment);
    assert_eq!(
        prepared.preview.skills[0].targets.len(),
        if eve { 2 } else { 1 }
    );
    if eve {
        assert_eq!(
            prepared.preview.skills[0]
                .targets
                .iter()
                .filter(|target| target.restoring)
                .count(),
            1
        );
        drop(prepared);
        run_fixture(&distro, "test ! -e \"$1\"", std::slice::from_ref(&writer))
            .await
            .unwrap();
        prepared = service
            .prepare(&request, CancellationSignal::default())
            .await
            .unwrap();
        run_fixture(
            &distro,
            "mkdir -p -- \"$1\"\nprintf '%s' occupied > \"$1/SKILL.md\"",
            std::slice::from_ref(&writer),
        )
        .await
        .unwrap();
        let stale = service
            .execute_prepared(prepared, &[], CancellationSignal::default(), |_| {})
            .await
            .unwrap();
        assert_ne!(stale.outcome, UpdateOutcome::Succeeded);
        let kept = run_fixture(
            &distro,
            "cat \"$1/SKILL.md\"",
            std::slice::from_ref(&writer),
        )
        .await
        .unwrap();
        assert_eq!(kept, b"occupied");
        run_fixture(&distro, "rm -rf -- \"$1\"", std::slice::from_ref(&writer))
            .await
            .unwrap();
        prepared = service
            .prepare(&request, CancellationSignal::default())
            .await
            .unwrap();
    }
    let calls_before_execution = source_calls.load(Ordering::SeqCst);
    let selected_copies = prepared
        .preview
        .skills
        .iter()
        .flat_map(|skill| &skill.targets)
        .filter_map(|target| target.selectable_entry_id.clone())
        .collect::<Vec<_>>();
    let response = service
        .execute_prepared(
            prepared,
            &selected_copies,
            CancellationSignal::default(),
            |_| {},
        )
        .await
        .unwrap();
    assert_eq!(response.outcome, UpdateOutcome::Succeeded, "{response:#?}");
    assert_eq!(source_calls.load(Ordering::SeqCst), calls_before_execution);
    let guide = run_fixture(
        &distro,
        r#"set -eu
test ! -e "$1/.agents/skills/alpha"
test ! -e "$1/.minimax/skills/alpha"
test ! -e "$1/.custom/skills/alpha"
if [ "$2" = eve ]; then
  grep -q '^name: alpha$' "$1/agent/subagents/research/skills/alpha/SKILL.md"
  grep -q '^name: alpha$' "$1/agent/subagents/writer/skills/alpha/SKILL.md"
  cmp "$1/agent/subagents/research/skills/alpha/references/guide.md" "$1/agent/subagents/writer/skills/alpha/references/guide.md"
  cat "$1/agent/subagents/research/skills/alpha/references/guide.md"
else
  cat "$1/.codebuddy/skills/alpha/references/guide.md"
fi
"#,
        &[project.clone(), if eve { "eve".into() } else { "private".into() }],
    )
    .await
    .unwrap();
    assert_eq!(
        guide,
        std::fs::read(remote.work.join("skills/alpha/references/guide.md")).unwrap()
    );
    let final_facts = ScopePlanningSnapshotSource::snapshot(&facts, &context)
        .await
        .unwrap();
    assert_eq!(
        final_facts
            .lock_document
            .entry_snapshot("alpha")
            .value()
            .unwrap()["computedHash"],
        remote.computed_hash("alpha")
    );
    if eve {
        let lock_before = run_fixture(
            &distro,
            "cat \"$1/skills-lock.json\"",
            std::slice::from_ref(&project),
        )
        .await
        .unwrap();
        let acquired =
            crate::application::installed_skill_payload::InstalledSkillPayloadAcquirer::new(
                manager.clone(),
                runtime.clone(),
            )
            .acquire(&context, "alpha", &selected.entry.fact)
            .await
            .expect("updated Eve directory has ordinary skill metadata");
        assert_eq!(
            manager
                .pin_verified(&acquired)
                .await
                .unwrap()
                .planning_metadata()
                .skill_name,
            "alpha"
        );
        run_fixture(
            &distro,
            "rm -rf -- \"$1\"\nprintf '%s' preserve-file > \"$1.md\"",
            std::slice::from_ref(&writer),
        )
        .await
        .unwrap();
        let blocked = service
            .prepare(&request, CancellationSignal::default())
            .await
            .unwrap();
        assert_eq!(blocked.preview.blocked.len(), 1, "{:#?}", blocked.preview);
        assert!(blocked.preview.skills.is_empty());
        assert_eq!(
            run_fixture(&distro, "cat \"$1.md\"", std::slice::from_ref(&writer))
                .await
                .unwrap(),
            b"preserve-file"
        );
        assert_eq!(
            run_fixture(
                &distro,
                "cat \"$1/skills-lock.json\"",
                std::slice::from_ref(&project)
            )
            .await
            .unwrap(),
            lock_before
        );
        run_fixture(
            &distro,
            "rm -rf -- \"$1/agent/subagents/writer\"",
            std::slice::from_ref(&project),
        )
        .await
        .unwrap();
        let blocked = service
            .prepare(&request, CancellationSignal::default())
            .await
            .unwrap();
        assert_eq!(blocked.preview.blocked.len(), 1);
        run_fixture(
            &distro,
            "test ! -e \"$1/agent/subagents/writer\"",
            std::slice::from_ref(&project),
        )
        .await
        .unwrap();
    }
    if matches!(workflow, WslWorkflow::Adjacent | WslWorkflow::Dependency) {
        verify_wsl_adjacent_workflows(
            &facts,
            runtime,
            manager.clone(),
            workflow == WslWorkflow::Dependency,
        )
        .await;
    }
    clock.store(120_000, Ordering::SeqCst);
    manager.cleanup().await.unwrap();
}

async fn verify_wsl_agent_directories(initial: &FixtureFacts, runtime: Arc<WslRuntime>) {
    use crate::application::library_candidates::EmptyLibraryCandidateSource;
    use crate::application::remove::{RemoveIntent, RemoveRequest, RemoveService};
    let environment = initial.initial.resolved_context.context.environment.clone();
    let EnvironmentRef::Wsl { distro_name } = &environment else {
        panic!("WSL fixture")
    };
    let home = initial.initial.resolved_context.home.native_path.clone();
    let project = initial
        .initial
        .resolved_context
        .project
        .as_ref()
        .unwrap()
        .native_path
        .clone();
    run_fixture(
        distro_name,
        r##"set -eu
mkdir -p "$1/.factory" "$1/.pi/agent" "$1/.kilo" "$1/.kilocode"
for root in "$1" "$2"; do
  for pair in '.agents shared' '.kilo current' '.kilocode legacy'; do
    set -- $pair
    mkdir -p "$root/$1/skills/$2"
    printf '%s\n' '---' "name: $2" "description: $2" '---' "# $2" > "$root/$1/skills/$2/SKILL.md"
  done
done
"##,
        &[home.clone(), project.clone()],
    )
    .await
    .unwrap();
    let definitions = crate::core::builtin_agent_catalog::builtin_agent_definitions()
        .into_iter()
        .filter(|agent| matches!(agent.id.as_str(), "codex" | "droid" | "kilo" | "pi"))
        .collect();
    let registry = crate::core::agent_registry::AgentRegistry::build(definitions, vec![])
        .snapshot()
        .clone();
    let targets = RuntimeTargetFactResolver::new(runtime.clone());
    for global in [false, true] {
        let mut facts = initial.clone();
        let root = if global { &home } else { &project };
        if global {
            facts.initial.resolved_context.context.scope = SkillLocation::Global;
            facts.initial.resolved_context.project = None;
            facts.initial.resolved_context.skill_root.native_path =
                format!("{home}/.agents/skills");
            facts.initial.resolved_context.lock.native_path =
                format!("{home}/.agents/.skill-lock.json");
            facts.initial.lock_schema = LockSchema::Global;
        }
        facts.initial.agent_runtime =
            AgentEnvironmentResolver::from_environment(EnvironmentContext {
                environment: environment.clone(),
                home: home.clone(),
                config_home: format!("{home}/.config"),
                environment_variables: BTreeMap::new(),
                availability: EnvironmentStatus::Available,
                revision: initial.initial.revisions.environment.clone(),
                wsl_workspace: Some(initial.workspace.clone()),
            })
            .resolve_registry(&registry, (!global).then_some(project.as_str()))
            .await
            .unwrap();
        let context = facts.initial.resolved_context.context.clone();
        let read_plan = crate::application::skill_read::build_skill_read_plan(
            &facts.initial.resolved_context,
            &facts.initial.agent_runtime,
            &[],
        )
        .unwrap();
        let inspector = WslInspector::new(initial.workspace.clone());
        let inventory = inspector.inspect(&read_plan.read_plan).await.unwrap();
        let listed = crate::application::skill_read::project_direct_skill_snapshot(
            &read_plan,
            inventory,
            &facts.initial.agent_runtime,
            &EmptyLibraries,
            &targets,
            &inspector,
        )
        .await
        .unwrap();
        for name in ["shared", "current", "legacy"] {
            assert!(
                listed.skills.iter().any(|skill| skill.name == name),
                "missing {name}, global={global}"
            );
        }
        let shared = listed
            .skills
            .iter()
            .find(|skill| skill.name == "shared")
            .unwrap();
        for agent in ["droid", "kilo", "pi"] {
            assert!(shared.agents.iter().any(|id| id.as_str() == agent));
        }
        let catalog = build_agent_selection_catalog(
            &context,
            &facts.initial.agent_runtime,
            &[],
            &facts.initial.resolved_context.skill_root,
            &targets,
        )
        .await
        .unwrap();
        assert_eq!(
            catalog
                .options()
                .filter(|option| option
                    .public
                    .agent_ids
                    .iter()
                    .any(|id| id.as_str() == "kilo"))
                .count(),
            2
        );
        let recovery = tempfile::tempdir().unwrap();
        let executor = RuntimePlanExecutor::new(
            runtime.clone(),
            Arc::new(NativeRecoveryMarkerStore::new(recovery.path()).unwrap()),
            Arc::new(RuntimeLockCommitter::new()),
            Arc::new(facts.clone()),
        );
        let remove = RemoveService::new(
            facts.clone(),
            targets.clone(),
            executor,
            Arc::new(EmptyLibraryCandidateSource),
        );
        let preview = remove.preview(&context, "legacy").await.unwrap();
        let removed = remove
            .execute(
                &RemoveRequest {
                    token: preview.token,
                    context: context.clone(),
                    skill_name: "legacy".into(),
                    intent: RemoveIntent::FullSkill,
                },
                CancellationSignal::default(),
            )
            .await
            .unwrap();
        assert!(
            removed.units.iter().all(|unit| unit.status
                == crate::application::mutation::result::MutationUnitStatus::Succeeded),
            "{removed:?}"
        );
        run_fixture(distro_name, "test ! -e \"$1/.kilocode/skills/legacy\"\ntest -f \"$1/.kilo/skills/current/SKILL.md\"\ntest -f \"$1/.agents/skills/shared/SKILL.md\"", std::slice::from_ref(root)).await.unwrap();
        run_fixture(
            distro_name,
            "rmdir \"$1/.kilocode/skills\"\nln -s \"$1/.kilo/skills\" \"$1/.kilocode/skills\"",
            std::slice::from_ref(root),
        )
        .await
        .unwrap();
        let catalog = build_agent_selection_catalog(
            &context,
            &facts.initial.agent_runtime,
            &[],
            &facts.initial.resolved_context.skill_root,
            &targets,
        )
        .await
        .unwrap();
        let options = catalog
            .options()
            .filter(|option| {
                option
                    .public
                    .agent_ids
                    .iter()
                    .any(|id| id.as_str() == "kilo")
            })
            .collect::<Vec<_>>();
        assert_eq!(options.len(), 1);
        assert_eq!(options[0].public.agent_ids.len(), 1);
    }
}

async fn verify_wsl_adjacent_workflows(
    source: &FixtureFacts,
    runtime: Arc<WslRuntime>,
    payloads: Arc<PayloadSessionManager>,
    dependency: bool,
) {
    use crate::application::agent_selection::AgentSelectionSubmission;
    use crate::application::copy::{
        CopyExecutionRequest, CopyPreviewOutcome, CopyRequest, CopyService,
    };
    use crate::application::installed_skill_payload::InstalledSkillPayloadAcquirer;
    use crate::application::library_candidates::EmptyLibraryCandidateSource;
    use crate::application::manage_agents::{
        ManageAgentsPreviewOutcome, ManageAgentsPreviewRequest, ManageAgentsRequest,
        ManageAgentsService,
    };
    use crate::application::remove::{RemoveIntent, RemoveRequest, RemoveService};
    use crate::application::scope_skill_placements::ScopeSkillPlacementResolver;
    use crate::models::InstallMode;
    use crate::runtime::copy_service::RuntimeCopyProjectComparator;

    verify_wsl_copy_project_identity(source, runtime.clone()).await;

    let context = source.initial.resolved_context.context.clone();
    let EnvironmentRef::Wsl { distro_name } = &context.environment else {
        panic!("WSL fixture");
    };
    let project = source
        .initial
        .resolved_context
        .project
        .as_ref()
        .unwrap()
        .native_path
        .clone();
    let destination = format!("{project}-copy");
    if dependency {
        let node = std::env::var("SKILL_DECK_TEST_WSL_NODE").unwrap();
        run_fixture(distro_name, r#"set -eu
project=$1
node=$2
package="$project/node_modules/@example/tools/skills/alpha"
mkdir -p "$(dirname "$package")" "$project/.agents/skills"
mv "$project/.codebuddy/skills/alpha" "$package"
cp "$package/SKILL.md" "$project/package-before.md"
ln -s "$package" "$project/.agents/skills/alpha"
ln -s "$project/.agents/skills/alpha" "$project/.codebuddy/skills/alpha"
"$node" -e 'const fs=require("fs"); const p=process.argv[1]; const d=JSON.parse(fs.readFileSync(p)); d.skills.alpha={source:"@example/tools",sourceType:"node_modules",computedHash:d.skills.alpha.computedHash,skillPath:"skills/alpha/SKILL.md",version:"2.0.0",via:"."}; fs.writeFileSync(p,JSON.stringify(d))' "$project/skills-lock.json"
"#, &[project.clone(), node]).await.unwrap();
    }
    run_fixture(
        distro_name,
        "mkdir -p -- \"$1/.codebuddy\"",
        std::slice::from_ref(&destination),
    )
    .await
    .unwrap();
    if dependency {
        let storage = source.workspace.payload_storage();
        for (label, path) in [
            (
                "physical package",
                format!("{project}/node_modules/@example/tools/skills/alpha"),
            ),
            (
                "linked installation",
                format!("{project}/.agents/skills/alpha"),
            ),
        ] {
            let session_id = format!("dependency-source-{}", uuid::Uuid::new_v4().simple());
            let key =
                crate::application::payload_session::PayloadStorageKey::new(&session_id, "alpha");
            let acquired = storage.acquire_from_path(&key, &path, None).await;
            storage.remove_session(&session_id).await.unwrap();
            assert!(acquired.is_ok(), "{label}: {acquired:?}");
        }
        let identity = crate::application::resources::SkillIdentity {
            context: context.clone(),
            skill_name: "alpha".into(),
        };
        let reader = RuntimeResourceReader::new(runtime.clone());
        let physical = reader
            .read_skill(ResourceLocator {
                environment: context.environment.clone(),
                native_path: format!("{project}/node_modules/@example/tools/skills/alpha"),
            })
            .await
            .unwrap();
        run_fixture(
            distro_name,
            r#"set -eu
printf '%s' 'ordinary file' > "$1/not-a-directory"
ln -s "$1/not-a-directory" "$1/unreadable-skill"
"#,
            std::slice::from_ref(&project),
        )
        .await
        .unwrap();
        let service = crate::application::resources::ResourceService::new(
            FixtureSkillResource(
                identity.clone(),
                vec![
                    ResourceLocator {
                        environment: context.environment.clone(),
                        native_path: format!("{project}/unreadable-skill"),
                    },
                    ResourceLocator {
                        environment: context.environment.clone(),
                        native_path: format!("{project}/.agents/skills/alpha"),
                    },
                ],
            ),
            RuntimeTargetFactResolver::new(runtime.clone()),
            crate::environment::opener::SystemResourceOpener,
            reader,
        );
        assert_eq!(service.read_skill(&identity).await.unwrap(), physical);
    }
    let mut target = source.clone();
    target.initial.resolved_context.context.scope = SkillLocation::Project {
        project_id: "copy-target".into(),
    };
    target.initial.resolved_context.project.as_mut().unwrap().id = "copy-target".into();
    target
        .initial
        .resolved_context
        .project
        .as_mut()
        .unwrap()
        .native_path = destination.clone();
    target.initial.resolved_context.skill_root.native_path =
        format!("{destination}/.agents/skills");
    target.initial.resolved_context.lock.native_path = format!("{destination}/skills-lock.json");
    let home = source.initial.resolved_context.home.native_path.clone();
    target.initial.agent_runtime = AgentEnvironmentResolver::from_environment(EnvironmentContext {
        environment: context.environment.clone(),
        home: home.clone(),
        config_home: format!("{home}/.config"),
        environment_variables: BTreeMap::new(),
        availability: EnvironmentStatus::Available,
        revision: source.initial.revisions.environment.clone(),
        wsl_workspace: Some(source.workspace.clone()),
    })
    .resolve_registry(
        &crate::native_workflow_integration_support::test_registry(),
        Some(&destination),
    )
    .await
    .unwrap();
    let locations = FixtureLocations {
        source: source.clone(),
        target,
    };
    let targets = RuntimeTargetFactResolver::new(runtime.clone());
    let recovery = tempfile::tempdir().unwrap();
    let executor = || {
        RuntimePlanExecutor::new(
            runtime.clone(),
            Arc::new(NativeRecoveryMarkerStore::new(recovery.path()).unwrap()),
            Arc::new(RuntimeLockCommitter::new()),
            Arc::new(locations.clone()),
        )
    };
    let acquirer = || InstalledSkillPayloadAcquirer::new(payloads.clone(), runtime.clone());
    let copy = CopyService::new(
        locations.clone(),
        targets.clone(),
        payloads.clone(),
        acquirer(),
        executor(),
        RuntimeCopyProjectComparator::new(runtime.clone()),
        Arc::new(EmptyLibraryCandidateSource),
    );
    let selection = copy.selection(&context, "alpha").await.unwrap().selection;
    let request = CopyRequest {
        skill_name: "alpha".into(),
        source: context.clone(),
        target_environment: context.environment.clone(),
        target_project_ids: vec!["copy-target".into()],
        agent_selection: AgentSelectionSubmission {
            revision: selection.revision,
            selected_option_ids: selection.baseline_selected_option_ids,
            requested_mode: InstallMode::Copy,
        },
    };
    let CopyPreviewOutcome::Ready { preview } = copy.preview(&request).await.unwrap() else {
        panic!("copy preview");
    };
    let copied = copy
        .execute(
            &CopyExecutionRequest {
                request,
                token: preview.token,
                payload: preview.payload,
            },
            CancellationSignal::default(),
        )
        .await
        .unwrap();
    assert!(
        copied.units.iter().all(|unit| unit.status
            == crate::application::mutation::result::MutationUnitStatus::Succeeded),
        "{copied:#?}"
    );
    run_fixture(
        distro_name,
        if dependency {
            "test -f \"$1/.agents/skills/alpha/SKILL.md\"\ntest -L \"$2/.agents/skills/alpha\""
        } else {
            "test -f \"$1/.agents/skills/alpha/SKILL.md\"\ntest ! -e \"$2/.agents/skills/alpha\""
        },
        &[destination.clone(), project.clone()],
    )
    .await
    .unwrap();

    if dependency {
        let lock = load_lock_document(
            &WslAtomicDocumentIo::new(source.workspace.clone()),
            &locations.target.initial.resolved_context.lock,
            None,
            LockSchema::Project,
        )
        .await
        .unwrap();
        assert!(lock.entry_snapshot("alpha").value().is_none());
    }

    let manage = ManageAgentsService::new(
        locations.clone(),
        ScopeSkillPlacementResolver::new(targets.clone()),
        targets.clone(),
        payloads.clone(),
        acquirer(),
        executor(),
        Arc::new(EmptyLibraryCandidateSource),
    );
    let snapshot = manage.selection(&context, "alpha").await.unwrap();
    let selection = AgentSelectionSubmission {
        revision: snapshot.selection.revision,
        selected_option_ids: snapshot
            .selection
            .install_options
            .iter()
            .filter(|option| {
                option
                    .agent_ids
                    .iter()
                    .any(|id| matches!(id.as_str(), "codebuddy" | "minimax-code"))
            })
            .map(|option| option.id.clone())
            .collect(),
        requested_mode: InstallMode::Copy,
    };
    let ManageAgentsPreviewOutcome::Ready { preview } = manage
        .preview(&ManageAgentsPreviewRequest {
            context: context.clone(),
            skill_name: "alpha".into(),
            agent_selection: selection.clone(),
        })
        .await
        .unwrap()
    else {
        panic!("manage preview");
    };
    let added = manage
        .execute(
            &ManageAgentsRequest {
                context: context.clone(),
                skill_name: "alpha".into(),
                agent_selection: selection,
                token: preview.token,
                original_payload: preview.original_payload,
                confirm_entity_directories: false,
            },
            CancellationSignal::default(),
        )
        .await
        .unwrap();
    assert!(
        added.units.iter().all(|unit| unit.status
            == crate::application::mutation::result::MutationUnitStatus::Succeeded),
        "{added:#?}"
    );
    run_fixture(
        distro_name,
        if dependency {
            "test -f \"$1/.minimax/skills/alpha/SKILL.md\"\ntest -L \"$1/.agents/skills/alpha\"\ncmp \"$1/package-before.md\" \"$1/node_modules/@example/tools/skills/alpha/SKILL.md\""
        } else {
            "test -f \"$1/.minimax/skills/alpha/SKILL.md\"\ntest ! -e \"$1/.agents/skills/alpha\""
        },
        std::slice::from_ref(&project),
    )
    .await
    .unwrap();

    if dependency {
        let lock = load_lock_document(
            &WslAtomicDocumentIo::new(source.workspace.clone()),
            &source.initial.resolved_context.lock,
            None,
            LockSchema::Project,
        )
        .await
        .unwrap();
        assert_eq!(lock.entry_snapshot("alpha").value().unwrap()["via"], ".");
    }

    let remove = RemoveService::new(
        locations.clone(),
        targets,
        executor(),
        Arc::new(EmptyLibraryCandidateSource),
    );
    let preview = remove.preview(&context, "alpha").await.unwrap();
    let removed = remove
        .execute(
            &RemoveRequest {
                context: context.clone(),
                skill_name: "alpha".into(),
                token: preview.token,
                intent: RemoveIntent::FullSkill,
            },
            CancellationSignal::default(),
        )
        .await
        .unwrap();
    assert!(
        removed.units.iter().all(|unit| unit.status
            == crate::application::mutation::result::MutationUnitStatus::Succeeded),
        "{removed:#?}"
    );
    run_fixture(distro_name, "test ! -e \"$1/.codebuddy/skills/alpha\"\ntest ! -e \"$1/.minimax/skills/alpha\"\ntest ! -e \"$1/.agents/skills/alpha\"", std::slice::from_ref(&project)).await.unwrap();
    if dependency {
        run_fixture(
            distro_name,
            "cmp \"$1/package-before.md\" \"$1/node_modules/@example/tools/skills/alpha/SKILL.md\"",
            std::slice::from_ref(&project),
        )
        .await
        .unwrap();
        run_fixture(
            distro_name,
            r#"set -eu
test ! -e "$1/.agents/skills" || rmdir "$1/.agents/skills"
mkdir -p "$1/.agents"
ln -s "$1/node_modules/@example/tools/skills" "$1/.agents/skills"
"#,
            std::slice::from_ref(&project),
        )
        .await
        .unwrap();
        let preview = remove.preview(&context, "alpha").await.unwrap();
        assert!(matches!(remove.execute(&RemoveRequest {
            token:preview.token, context:context.clone(), skill_name:"alpha".into(), intent:RemoveIntent::FullSkill,
        }, CancellationSignal::default()).await,
            Err(AppError::CapabilityUnavailable { capability, .. }) if capability == "dependencyContentTarget"));
        run_fixture(
            distro_name,
            "cmp \"$1/package-before.md\" \"$1/node_modules/@example/tools/skills/alpha/SKILL.md\"",
            std::slice::from_ref(&project),
        )
        .await
        .unwrap();
        run_fixture(distro_name, r#"set -eu
project=$1
mkdir -p "$project/../packages"
mv "$project/node_modules/@example/tools" "$project/../packages/tools"
ln -s "$project/../packages/tools" "$project/node_modules/@example/tools"
printf '%s\n' '{"version":1,"skills":{"alpha":{"source":"@example/tools","sourceType":"node_modules","computedHash":"content"}}}' > "$project/skills-lock.json"
"#, std::slice::from_ref(&project)).await.unwrap();
        let preview = remove.preview(&context, "alpha").await.unwrap();
        assert!(matches!(remove.execute(&RemoveRequest {
            token:preview.token, context:context.clone(), skill_name:"alpha".into(), intent:RemoveIntent::FullSkill,
        }, CancellationSignal::default()).await,
            Err(AppError::CapabilityUnavailable { capability, .. }) if capability == "dependencyContentTarget"));
        run_fixture(
            distro_name,
            "cmp \"$1/package-before.md\" \"$1/../packages/tools/skills/alpha/SKILL.md\"",
            std::slice::from_ref(&project),
        )
        .await
        .unwrap();
    }
}

async fn verify_wsl_copy_project_identity(source: &FixtureFacts, runtime: Arc<WslRuntime>) {
    use crate::application::copy::CopyProjectComparator;
    use crate::environment::runtime::{ExecutionBackend, PhysicalIdentityComparison};
    use crate::environment::types::StorageAccess;
    use crate::runtime::copy_service::RuntimeCopyProjectComparator;

    let environment = &source.initial.resolved_context.context.environment;
    let EnvironmentRef::Wsl { distro_name } = environment else {
        panic!("WSL fixture")
    };
    let project = &source
        .initial
        .resolved_context
        .project
        .as_ref()
        .unwrap()
        .native_path;
    let comparator = RuntimeCopyProjectComparator::new(runtime);
    let identity = comparator.capture_source(&source.initial).await.unwrap();
    assert!(matches!(
        identity.key.backend,
        ExecutionBackend::WslPosix { .. }
    ));
    assert_eq!(identity.storage_access, StorageAccess::Native);
    let alias = format!("{project}-alias");
    let sibling = format!("{project}-sibling");
    run_fixture(
        distro_name,
        r#"set -eu
mkdir -p -- "$1/nested" "$3"
ln -s -- "$1" "$2"
"#,
        &[project.clone(), alias.clone(), sibling.clone()],
    )
    .await
    .unwrap();
    let at = |environment: EnvironmentRef, path: String| {
        let mut facts = source.initial.clone();
        facts.resolved_context.context.environment = environment;
        facts.resolved_context.project.as_mut().unwrap().native_path = path;
        facts
    };
    for (path, expected) in [
        (project.clone(), PhysicalIdentityComparison::Same),
        (alias.clone(), PhysicalIdentityComparison::Same),
        (format!("{alias}/nested"), PhysicalIdentityComparison::Same),
        (
            project.rsplit_once('/').unwrap().0.into(),
            PhysicalIdentityComparison::Same,
        ),
        (sibling, PhysicalIdentityComparison::Different),
    ] {
        let target = at(environment.clone(), path.clone());
        let compared = comparator.compare(&identity, &target).await.unwrap();
        assert_eq!(compared.physical_identity, expected, "{path}");
        assert_eq!(compared.target_storage_access, StorageAccess::Native);
    }
    let missing = at(environment.clone(), format!("{project}/missing-project"));
    assert!(comparator.capture_source(&missing).await.is_err());
    assert!(comparator.compare(&identity, &missing).await.is_err());

    let unc = source
        .workspace
        .map_path_to_windows(project.clone())
        .await
        .unwrap()
        .unwrap();
    let native_alias = at(EnvironmentRef::Native, unc);
    let compared = comparator.compare(&identity, &native_alias).await.unwrap();
    assert_eq!(compared.physical_identity, PhysicalIdentityComparison::Same);
    assert_eq!(compared.target_storage_access, StorageAccess::CrossStorage);

    let native = tempfile::tempdir().unwrap();
    let native_project = native.path().join("project");
    std::fs::create_dir_all(native_project.join("nested")).unwrap();
    let mapped = source
        .workspace
        .map_host_path(native_project.to_str().unwrap().into(), None)
        .await
        .unwrap();
    let mounted = at(environment.clone(), mapped.clone());
    let mounted_identity = comparator.capture_source(&mounted).await.unwrap();
    assert!(matches!(
        mounted_identity.key.backend,
        ExecutionBackend::NativeWindows
    ));
    assert_eq!(mounted_identity.storage_access, StorageAccess::CrossStorage);
    for path in [
        native_project.clone(),
        native_project.join("nested"),
        std::fs::canonicalize(&native_project).unwrap(),
    ] {
        let target = at(EnvironmentRef::Native, path.to_str().unwrap().into());
        assert_eq!(
            comparator
                .compare(&mounted_identity, &target)
                .await
                .unwrap()
                .physical_identity,
            PhysicalIdentityComparison::Same
        );
    }
    assert_eq!(
        comparator
            .compare(&mounted_identity, &source.initial)
            .await
            .unwrap()
            .physical_identity,
        PhysicalIdentityComparison::Different
    );
    let mounted_alias = format!("{project}-mounted-alias");
    run_fixture(
        distro_name,
        "ln -s -- \"$1\" \"$2\"",
        &[mapped, mounted_alias.clone()],
    )
    .await
    .unwrap();
    assert_eq!(
        comparator
            .compare(&mounted_identity, &at(environment.clone(), mounted_alias))
            .await
            .unwrap()
            .physical_identity,
        PhysicalIdentityComparison::Same
    );
    run_fixture(
        distro_name,
        "test ! -e \"$1/.skill-deck-project-identity\"",
        std::slice::from_ref(project),
    )
    .await
    .unwrap();
}

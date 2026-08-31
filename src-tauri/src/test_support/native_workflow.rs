use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde_json::{json, Value};

use crate::application::agent_registry_source::AgentRegistrySnapshotSource;
use crate::application::agent_selection::{
    build_agent_selection_catalog, test_submission_for_agents_and_own_directories,
};
use crate::application::copy::{
    CopyExecutionRequest, CopyPreviewOutcome, CopyRequest, CopyService,
};
use crate::application::install::{
    InstallFuture, InstallOperation, InstallPreviewOutcome, InstallRequest, InstallService,
};
use crate::application::install_planner::ConcreteInstallPlanner;
use crate::application::installed_skill_payload::InstalledSkillPayloadAcquirer;
use crate::application::installed_skill_resolver::SkillDirectoryName;
use crate::application::library_application::{
    ApplyLibraryApplicationRequest, LibraryApplicationDraft, LibraryApplicationFuture,
    LibraryApplicationModule, LibraryApplicationRecord, LibraryApplicationRepository,
};
use crate::application::library_candidates::LibraryCandidateSet;
use crate::application::library_candidates::{
    EmptyLibraryCandidateSource, LibraryCandidateSource, RepositoryLibraryCandidateSource,
};
use crate::application::manage_agents::{
    ManageAgentSelectionSnapshot, ManageAgentsPreview, ManageAgentsPreviewOutcome,
    ManageAgentsPreviewRequest, ManageAgentsRequest, ManageAgentsService,
};
use crate::application::mutation::coordinator::{
    BoxFuture, MutationCoordinator, PreparedEntryExecutor, PreparedLockCommitter,
    RuntimeRevisionSource,
};
use crate::application::mutation::executor::MutationPlanExecutor;
use crate::application::mutation::plan::{ExecutionUnit, MutationPlan};
use crate::application::mutation::result::{
    MutationUnitResult, MutationUnitStatus, MutationWarning,
};
use crate::application::payload_session::{
    AcquiredPayloadHandle, DiscoverySessionHandle, PayloadPlanningMetadata, PayloadSessionLimits,
    PayloadSessionManager, PinnedPayloadLease,
};
use crate::application::planning_facts::ScopePlanningSnapshotSource;
use crate::application::remove::{RemoveIntent, RemoveRequest, RemoveService};
use crate::application::scope_skill_placements::ScopeSkillPlacementResolver;
use crate::application::scope_skill_planning::{
    DirectSkillChangeRequest, LibraryElectionState, ScopeSkillPlanner,
};
use crate::application::skill_libraries::{
    LibraryCatalog, LibraryId, LibrarySkillRecord, SkillLibraryRecord, LIBRARY_SCHEMA_VERSION,
};
use crate::application::skill_source::SkillSourceModule;
use crate::application::source_evidence::{RemoteSnapshotId, SourceSnapshotFacts};
use crate::application::update::{
    AcquiredUpdateSource, UpdateAcquisitionGroup, UpdateExecutionRequest, UpdateFuture,
    UpdateRequest, UpdateService, UpdateSourceAcquisition,
};
use crate::application::update_planner::ConcreteUpdatePlanner;
use crate::core::agent_definition::{
    AgentDefinition, AgentId, CustomAgentDefinition, CustomPathBase, CustomPathSpec,
    CustomScopeDefinition, ScopeLocation,
};
use crate::core::agent_registry::{AgentRegistry, AgentRegistrySnapshot};
use crate::core::agent_settings::CustomAgentRecord;
use crate::core::builtin_agent_catalog::builtin_agent_definitions;
use crate::core::mutation::CancellationSignal;
use crate::core::skill_payload::{
    build_skill_payload, compute_cli_project_hash_from_payload, PayloadId, SkillPayload,
};
use crate::environment::native::materialize::{
    NativePreparedEntryExecutor, NativePreparedEntrySet,
};
use crate::environment::native::recovery::NativeRecoveryMarkerStore;
use crate::environment::planning::RuntimeTargetFactResolver;
use crate::environment::recovery::RecoveryMarkerStore;
use crate::environment::runtime::ExecutionBackend;
use crate::environment::types::{EnvironmentRef, SkillLocation, SkillLocationRef};
use crate::environment::wsl::WslRuntime;
use crate::error::AppError;
use crate::git_fixture::{BareSkillRepo as FileBareSkillRepo, CountingGitTransport};
use crate::models::InstallMode;
use crate::runtime::copy_service::RuntimeCopyProjectComparator;
use crate::runtime::plan_runner::{RuntimeExecutionDependencies, RuntimePlanExecutor};
use crate::runtime::planning_facts::{NativeRuntimeSnapshot, RuntimePlanningFactSource};
use crate::storage::lock_plan::{LockCommitReceipt, PreparedLockMutation};

pub(crate) struct StaticRegistry(pub(crate) Arc<AgentRegistrySnapshot>);

impl AgentRegistrySnapshotSource for StaticRegistry {
    fn snapshot(&self) -> Arc<AgentRegistrySnapshot> {
        Arc::clone(&self.0)
    }
}

async fn observe_skill(
    observer: &ScopeSkillPlacementResolver<RuntimeTargetFactResolver>,
    facts: &RuntimePlanningFactSource,
    targets: &RuntimeTargetFactResolver,
    context: &SkillLocationRef,
    skill_name: &str,
) -> Result<Vec<crate::application::skill_entry_projection::ObservedPlannedEntry>, AppError> {
    let planning = ScopePlanningSnapshotSource::snapshot(facts, context).await?;
    let catalog = build_agent_selection_catalog(
        context,
        &planning.agent_runtime,
        &planning.eve_targets,
        &planning.resolved_context.skill_root,
        targets,
    )
    .await?;
    let observed = observer
        .observe(context, skill_name, &planning, &catalog)
        .await?;
    let candidates = LibraryCandidateSet::empty();
    ScopeSkillPlanner::plan_direct_change(DirectSkillChangeRequest {
        skill: SkillDirectoryName::try_from(skill_name)?,
        catalog: &catalog,
        placements: observed.placements,
        libraries: LibraryElectionState {
            candidates: &candidates,
            selected_agent_ids: &[],
        },
        direct_changes: BTreeMap::new(),
    })
    .map_err(|error| error.into_app_error())?
    .project_observed_entries()
    .map_err(|error| error.into_app_error())
}

struct MemoryLibraryApplicationRepository {
    record: Mutex<LibraryApplicationRecord>,
    catalog: LibraryCatalog,
    members_root: PathBuf,
}

impl LibraryApplicationRepository for MemoryLibraryApplicationRepository {
    fn load_application<'a>(
        &'a self,
        _context: &'a SkillLocationRef,
    ) -> LibraryApplicationFuture<'a, Result<LibraryApplicationRecord, AppError>> {
        Box::pin(async move { Ok(self.record.lock().expect("library record lock").clone()) })
    }

    fn save_application<'a>(
        &'a self,
        record: &'a LibraryApplicationRecord,
    ) -> LibraryApplicationFuture<'a, Result<(), AppError>> {
        Box::pin(async move {
            *self.record.lock().expect("library record lock") = record.clone();
            Ok(())
        })
    }

    fn library_skill_locator<'a>(
        &'a self,
        context: &'a SkillLocationRef,
        library_id: &'a LibraryId,
        skill_name: &'a str,
    ) -> LibraryApplicationFuture<'a, Result<crate::environment::types::ResourceLocator, AppError>>
    {
        Box::pin(async move {
            let install_dir_name =
                crate::application::installed_skill_resolver::InstalledSkillResolver::install_dir_name(
                    skill_name,
                )?;
            Ok(crate::environment::types::ResourceLocator {
                environment: context.environment.clone(),
                native_path: self
                    .members_root
                    .join(library_id.as_str())
                    .join("skills")
                    .join(install_dir_name)
                    .to_string_lossy()
                    .into_owned(),
            })
        })
    }

    fn load_catalog<'a>(
        &'a self,
        _context: &'a SkillLocationRef,
    ) -> LibraryApplicationFuture<'a, Result<LibraryCatalog, AppError>> {
        Box::pin(async move { Ok(self.catalog.clone()) })
    }

    fn remove_application<'a>(
        &'a self,
        context: &'a SkillLocationRef,
    ) -> LibraryApplicationFuture<'a, Result<(), AppError>> {
        Box::pin(async move {
            *self.record.lock().expect("library record lock") =
                LibraryApplicationRecord::empty(context.clone());
            Ok(())
        })
    }
}

#[derive(Clone)]
pub(crate) struct FixedUpdateAcquirer {
    pub(crate) handle: AcquiredPayloadHandle,
}

struct VerifyFailureEntryExecutor {
    inner: NativePreparedEntryExecutor,
}

struct SelectiveVerifyFailureEntryExecutor {
    inner: NativePreparedEntryExecutor,
    failing_skill: String,
}

struct SelectiveVerifyStaged {
    inner: NativePreparedEntrySet,
    fail_verify: bool,
}

impl PreparedEntryExecutor for SelectiveVerifyFailureEntryExecutor {
    type Staged = SelectiveVerifyStaged;

    fn stage<'a>(
        &'a self,
        unit: &'a ExecutionUnit,
        payloads: &'a BTreeMap<PayloadId, PinnedPayloadLease>,
        cancellation: CancellationSignal,
    ) -> BoxFuture<'a, Result<Self::Staged, AppError>> {
        Box::pin(async move {
            Ok(SelectiveVerifyStaged {
                inner: self.inner.stage(unit, payloads, cancellation).await?,
                fail_verify: unit.skill_name == self.failing_skill,
            })
        })
    }

    fn recheck_entries<'a>(
        &'a self,
        staged: &'a Self::Staged,
    ) -> BoxFuture<'a, Result<(), AppError>> {
        self.inner.recheck_entries(&staged.inner)
    }

    fn swap<'a>(&'a self, staged: &'a mut Self::Staged) -> BoxFuture<'a, Result<(), AppError>> {
        self.inner.swap(&mut staged.inner)
    }

    fn verify<'a>(&'a self, staged: &'a Self::Staged) -> BoxFuture<'a, Result<(), AppError>> {
        Box::pin(async move {
            self.inner.verify(&staged.inner).await?;
            if staged.fail_verify {
                return Err(AppError::ExecutionFailed {
                    message: "injected direct-download verification failure".to_string(),
                });
            }
            Ok(())
        })
    }

    fn restore<'a>(&'a self, staged: &'a mut Self::Staged) -> BoxFuture<'a, Result<(), AppError>> {
        self.inner.restore(&mut staged.inner)
    }

    fn cleanup<'a>(
        &'a self,
        staged: Self::Staged,
    ) -> BoxFuture<'a, Result<Vec<MutationWarning>, AppError>> {
        self.inner.cleanup(staged.inner)
    }
}

impl PreparedEntryExecutor for VerifyFailureEntryExecutor {
    type Staged = NativePreparedEntrySet;

    fn stage<'a>(
        &'a self,
        unit: &'a ExecutionUnit,
        payloads: &'a BTreeMap<PayloadId, PinnedPayloadLease>,
        cancellation: CancellationSignal,
    ) -> BoxFuture<'a, Result<Self::Staged, AppError>> {
        self.inner.stage(unit, payloads, cancellation)
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
        Box::pin(async move {
            self.inner.verify(staged).await?;
            Err(AppError::ExecutionFailed {
                message: "injected Manage Agents verify failure".to_string(),
            })
        })
    }

    fn restore<'a>(&'a self, staged: &'a mut Self::Staged) -> BoxFuture<'a, Result<(), AppError>> {
        self.inner.restore(staged)
    }

    fn cleanup<'a>(
        &'a self,
        staged: Self::Staged,
    ) -> BoxFuture<'a, Result<Vec<MutationWarning>, AppError>> {
        self.inner.cleanup(staged)
    }
}

struct VerifyFailurePlanExecutor {
    environments: Arc<WslRuntime>,
    facts: RuntimePlanningFactSource,
    recovery_root: PathBuf,
}

struct SelectiveVerifyFailurePlanExecutor {
    environments: Arc<WslRuntime>,
    facts: RuntimePlanningFactSource,
    recovery_root: PathBuf,
    failing_skill: String,
}

struct LockFailurePlanExecutor {
    facts: RuntimePlanningFactSource,
    recovery_root: PathBuf,
    attempted: Arc<std::sync::atomic::AtomicBool>,
}

struct RejectingLockCommitter {
    attempted: Arc<std::sync::atomic::AtomicBool>,
}

impl PreparedLockCommitter for RejectingLockCommitter {
    fn commit<'a>(
        &'a self,
        _mutation: &'a PreparedLockMutation,
    ) -> BoxFuture<'a, Result<LockCommitReceipt, AppError>> {
        self.attempted
            .store(true, std::sync::atomic::Ordering::SeqCst);
        Box::pin(async {
            Err(AppError::ExecutionFailed {
                message: "injected Manage Agents lock failure".to_string(),
            })
        })
    }
}

impl MutationPlanExecutor for LockFailurePlanExecutor {
    fn execute<'a>(
        &'a self,
        plan: MutationPlan,
        cancellation: CancellationSignal,
    ) -> InstallFuture<'a, Vec<MutationUnitResult>> {
        Box::pin(async move {
            let recovery: Arc<dyn RecoveryMarkerStore> = Arc::new(
                NativeRecoveryMarkerStore::new(&self.recovery_root)
                    .expect("native Manage Agents recovery store"),
            );
            let entries = NativePreparedEntryExecutor::new(
                if cfg!(windows) {
                    ExecutionBackend::NativeWindows
                } else {
                    ExecutionBackend::NativeUnix
                },
                plan.operation_id.clone(),
                recovery,
            );
            MutationCoordinator::new(
                entries,
                RejectingLockCommitter {
                    attempted: Arc::clone(&self.attempted),
                },
                self.facts.clone(),
            )
            .execute(plan, cancellation)
            .await
        })
    }
}

impl MutationPlanExecutor for VerifyFailurePlanExecutor {
    fn execute<'a>(
        &'a self,
        plan: MutationPlan,
        cancellation: CancellationSignal,
    ) -> InstallFuture<'a, Vec<MutationUnitResult>> {
        Box::pin(async move {
            let recovery: Arc<dyn RecoveryMarkerStore> = Arc::new(
                NativeRecoveryMarkerStore::new(&self.recovery_root)
                    .expect("native Manage Agents recovery store"),
            );
            let entries = VerifyFailureEntryExecutor {
                inner: NativePreparedEntryExecutor::new(
                    if cfg!(windows) {
                        ExecutionBackend::NativeWindows
                    } else {
                        ExecutionBackend::NativeUnix
                    },
                    plan.operation_id.clone(),
                    recovery,
                ),
            };
            MutationCoordinator::new(
                entries,
                crate::runtime::plan_runner::RuntimeLockCommitter::new(self.environments.clone()),
                self.facts.clone(),
            )
            .execute(plan, cancellation)
            .await
        })
    }
}

impl MutationPlanExecutor for SelectiveVerifyFailurePlanExecutor {
    fn execute<'a>(
        &'a self,
        plan: MutationPlan,
        cancellation: CancellationSignal,
    ) -> InstallFuture<'a, Vec<MutationUnitResult>> {
        Box::pin(async move {
            let recovery: Arc<dyn RecoveryMarkerStore> = Arc::new(
                NativeRecoveryMarkerStore::new(&self.recovery_root)
                    .expect("native direct-download recovery store"),
            );
            let entries = SelectiveVerifyFailureEntryExecutor {
                inner: NativePreparedEntryExecutor::new(
                    if cfg!(windows) {
                        ExecutionBackend::NativeWindows
                    } else {
                        ExecutionBackend::NativeUnix
                    },
                    plan.operation_id.clone(),
                    recovery,
                ),
                failing_skill: self.failing_skill.clone(),
            };
            MutationCoordinator::new(
                entries,
                crate::runtime::plan_runner::RuntimeLockCommitter::new(self.environments.clone()),
                self.facts.clone(),
            )
            .execute(plan, cancellation)
            .await
        })
    }
}

impl SkillSourceModule for FixedUpdateAcquirer {
    fn acquire_saved_groups<'a>(
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
                    skill_errors: Vec::new(),
                    redirected_download_host: None,
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
        fs::create_dir_all(project.join(".agents/skills"))?;
        fs::create_dir_all(project.join(".codebuddy/skills"))?;
        fs::create_dir_all(project.join(".minimax/skills"))?;
        fs::create_dir_all(project.join(".custom/skills"))?;
    }
    fs::create_dir_all(source_project.join("agent/subagents/research/skills"))?;
    fs::write(
        source_project.join("package.json"),
        r#"{"dependencies":{"eve":"^0.11.5"}}"#,
    )?;
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
    let environments = Arc::new(WslRuntime::default());
    let facts = RuntimePlanningFactSource::with_native_snapshot(
        registry,
        environments.clone(),
        NativeRuntimeSnapshot {
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
    let discovery_v1 = payloads
        .discover(EnvironmentRef::Native, "source-v1")
        .await?;
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
    let discovery_v2 = payloads
        .discover(EnvironmentRef::Native, "source-v2")
        .await?;
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
        ConcreteInstallPlanner::new(
            facts.clone(),
            targets.clone(),
            payloads.clone(),
            fixed_time,
            Arc::new(EmptyLibraryCandidateSource),
        ),
        executor(&execution, &environments, &facts),
    );
    let selection_facts = ScopePlanningSnapshotSource::snapshot(&facts, &source_context).await?;
    let agent_selection = test_submission_for_agents_and_own_directories(
        &source_context,
        &selection_facts.agent_runtime,
        &selection_facts.eve_targets,
        &selection_facts.resolved_context.skill_root,
        &targets,
        &["codebuddy", "minimax-code", "custom-test", "eve"],
        InstallMode::Copy,
    )
    .await;
    let install_request = InstallRequest {
        context: source_context.clone(),
        source: "reader/repo".to_string(),
        discovery_session: discovery_v1,
        payloads: vec![handle_v1],
        skills: vec!["demo".to_string()],
        agent_selection,
        acknowledge_redirect: true,
    };
    let InstallPreviewOutcome::Ready {
        preview: install_preview,
    } = install
        .preview(InstallOperation::Install, &install_request)
        .await?
    else {
        panic!("expected ready install preview");
    };
    let installed = install
        .execute(
            InstallOperation::Install,
            &install_request,
            install_preview.token,
            CancellationSignal::default(),
        )
        .await?;
    assert_succeeded(&installed.units);
    assert_payload_tree(&source_project.join(".agents/skills/demo"), "v1")?;
    assert_payload_tree(&source_project.join(".codebuddy/skills/demo"), "v1")?;
    assert_payload_tree(&source_project.join(".minimax/skills/demo"), "v1")?;
    assert_payload_tree(&source_project.join(".custom/skills/demo"), "v1")?;
    assert!(source_project
        .join("agent/subagents/research/skills/demo")
        .is_dir());
    assert!(source_project.join("agent/skills/demo").is_dir());
    assert_lock_fields(
        &source_project.join("skills-lock.json"),
        "computed-v1",
        "remote-v1",
    )?;
    assert_eq!(
        read_json(&source_project.join("skills-lock.json"))?["skills"]["demo"]["subagents"],
        json!(["", "research"])
    );

    let observer = ScopeSkillPlacementResolver::new(targets.clone());
    let observed = observe_skill(&observer, &facts, &targets, &source_context, "demo").await?;
    let readers = observed
        .iter()
        .flat_map(|entry| entry.public.readers.iter())
        .map(|reader| reader.agent_id.as_str())
        .collect::<BTreeSet<_>>();
    assert_eq!(
        readers,
        BTreeSet::from(["codebuddy", "custom-test", "eve", "minimax-code"])
    );

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
    assert_payload_tree(&source_project.join(".codebuddy/skills/demo"), "v2")?;
    assert_payload_tree(&source_project.join(".minimax/skills/demo"), "v2")?;
    assert_payload_tree(&source_project.join(".custom/skills/demo"), "v2")?;
    assert!(source_project
        .join("agent/subagents/research/skills/demo")
        .is_dir());
    assert!(source_project.join("agent/skills/demo").is_dir());
    assert_lock_fields(
        &source_project.join("skills-lock.json"),
        "computed-v2",
        "remote-v2",
    )?;

    let manage = ManageAgentsService::new(
        facts.clone(),
        ScopeSkillPlacementResolver::new(targets.clone()),
        targets.clone(),
        payloads.clone(),
        InstalledSkillPayloadAcquirer::new(payloads.clone(), environments.clone()),
        executor(&execution, &environments, &facts),
        Arc::new(EmptyLibraryCandidateSource),
    );
    let manage_observed =
        observe_skill(&observer, &facts, &targets, &source_context, "demo").await?;
    let removed_entries = manage_observed
        .iter()
        .filter(|entry| {
            entry.public.readers.iter().any(|reader| {
                matches!(
                    reader.agent_id.as_str(),
                    "codebuddy" | "minimax-code" | "custom-test"
                ) || reader.logical_target_id.starts_with("eve:")
            })
        })
        .map(|entry| entry.public.entry_id.clone())
        .collect::<Vec<_>>();
    assert_eq!(removed_entries.len(), 5, "five physical Agent entries");
    let manage_selection = manage.selection(&source_context, "demo").await?;
    let remove_all_selection = manage_submission(&manage_selection, |_| false, InstallMode::Copy);
    let manage_preview_request = ManageAgentsPreviewRequest {
        context: source_context.clone(),
        skill_name: "demo".to_string(),
        agent_selection: remove_all_selection.clone(),
    };
    let manage_preview = ready_manage_preview(manage.preview(&manage_preview_request).await?);
    fs::write(
        source_project.join(".custom/skills/demo/external-change.txt"),
        b"changed after preview",
    )?;
    let stale_error = manage
        .execute(
            &ManageAgentsRequest {
                token: manage_preview.token,
                context: source_context.clone(),
                skill_name: "demo".to_string(),
                agent_selection: remove_all_selection,
                confirm_entity_directories: true,
                original_payload: None,
            },
            CancellationSignal::default(),
        )
        .await
        .expect_err("stale Manage Agents preview must be rejected");
    assert!(matches!(
        stale_error,
        AppError::StaleContext | AppError::StaleTarget
    ));
    assert!(source_project.join(".codebuddy/skills/demo").exists());
    assert!(source_project.join(".minimax/skills/demo").exists());
    assert!(source_project.join(".custom/skills/demo").exists());

    let refreshed_entries = observe_skill(&observer, &facts, &targets, &source_context, "demo")
        .await?
        .into_iter()
        .filter(|entry| {
            entry.public.readers.iter().any(|reader| {
                matches!(
                    reader.agent_id.as_str(),
                    "codebuddy" | "minimax-code" | "custom-test"
                ) || reader.logical_target_id.starts_with("eve:")
            })
        })
        .map(|entry| entry.public.entry_id)
        .collect::<Vec<_>>();
    assert_eq!(refreshed_entries.len(), 5);
    let refreshed_selection = manage.selection(&source_context, "demo").await?;
    let refreshed_submission =
        manage_submission(&refreshed_selection, |_| false, InstallMode::Copy);
    let refreshed_preview_request = ManageAgentsPreviewRequest {
        context: source_context.clone(),
        skill_name: "demo".to_string(),
        agent_selection: refreshed_submission.clone(),
    };
    let failing_manage = ManageAgentsService::new(
        facts.clone(),
        ScopeSkillPlacementResolver::new(targets.clone()),
        targets.clone(),
        payloads.clone(),
        InstalledSkillPayloadAcquirer::new(payloads.clone(), environments.clone()),
        VerifyFailurePlanExecutor {
            environments: environments.clone(),
            facts: facts.clone(),
            recovery_root: recovery_root.clone(),
        },
        Arc::new(EmptyLibraryCandidateSource),
    );
    let failing_preview =
        ready_manage_preview(failing_manage.preview(&refreshed_preview_request).await?);
    let failed = failing_manage
        .execute(
            &ManageAgentsRequest {
                token: failing_preview.token,
                context: source_context.clone(),
                skill_name: "demo".to_string(),
                agent_selection: refreshed_submission.clone(),
                confirm_entity_directories: true,
                original_payload: None,
            },
            CancellationSignal::default(),
        )
        .await?;
    assert_eq!(failed.units.len(), 1, "Manage Agents failure stays atomic");
    assert_eq!(failed.units[0].status, MutationUnitStatus::Failed);
    assert!(source_project.join(".codebuddy/skills/demo").exists());
    assert!(source_project.join(".minimax/skills/demo").exists());
    assert!(source_project.join(".custom/skills/demo").exists());
    assert!(source_project
        .join(".custom/skills/demo/external-change.txt")
        .is_file());
    assert!(source_project
        .join("agent/subagents/research/skills/demo")
        .is_dir());
    assert!(source_project.join("agent/skills/demo").is_dir());
    assert_no_staging_leaks(root)?;
    assert_recovery_graph_is_empty(&recovery_root)?;

    let lock_before_failure = fs::read(source_project.join("skills-lock.json"))?;
    let lock_attempted = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let lock_failing_manage = ManageAgentsService::new(
        facts.clone(),
        ScopeSkillPlacementResolver::new(targets.clone()),
        targets.clone(),
        payloads.clone(),
        InstalledSkillPayloadAcquirer::new(payloads.clone(), environments.clone()),
        LockFailurePlanExecutor {
            facts: facts.clone(),
            recovery_root: recovery_root.clone(),
            attempted: Arc::clone(&lock_attempted),
        },
        Arc::new(EmptyLibraryCandidateSource),
    );
    let lock_failing_preview = ready_manage_preview(
        lock_failing_manage
            .preview(&refreshed_preview_request)
            .await?,
    );
    let lock_failed = lock_failing_manage
        .execute(
            &ManageAgentsRequest {
                token: lock_failing_preview.token,
                context: source_context.clone(),
                skill_name: "demo".to_string(),
                agent_selection: refreshed_submission,
                confirm_entity_directories: true,
                original_payload: None,
            },
            CancellationSignal::default(),
        )
        .await?;
    assert_eq!(lock_failed.units[0].status, MutationUnitStatus::Failed);
    assert!(lock_attempted.load(std::sync::atomic::Ordering::SeqCst));
    assert!(source_project.join(".codebuddy/skills/demo").exists());
    assert!(source_project.join(".minimax/skills/demo").exists());
    assert!(source_project.join(".custom/skills/demo").exists());
    assert!(source_project
        .join("agent/subagents/research/skills/demo")
        .is_dir());
    assert!(source_project.join("agent/skills/demo").is_dir());
    assert_eq!(
        fs::read(source_project.join("skills-lock.json"))?,
        lock_before_failure
    );
    assert_no_staging_leaks(root)?;
    assert_recovery_graph_is_empty(&recovery_root)?;

    let _research_entry_id = observe_skill(&observer, &facts, &targets, &source_context, "demo")
        .await?
        .into_iter()
        .find(|entry| {
            entry
                .public
                .readers
                .iter()
                .any(|reader| reader.logical_target_id == "eve:research")
        })
        .map(|entry| entry.public.entry_id)
        .expect("Eve research entry");
    let research_selection = manage.selection(&source_context, "demo").await?;
    let research_skill_path = source_project
        .join("agent")
        .join("subagents")
        .join("research")
        .join("skills");
    let resolved_research_skill_path = fs::canonicalize(&research_skill_path)?;
    let remove_research_selection = manage_submission(
        &research_selection,
        |item| Path::new(&item.path) != resolved_research_skill_path,
        InstallMode::Copy,
    );
    let remove_research_request = ManageAgentsPreviewRequest {
        context: source_context.clone(),
        skill_name: "demo".to_string(),
        agent_selection: remove_research_selection.clone(),
    };
    let remove_research_preview =
        ready_manage_preview(manage.preview(&remove_research_request).await?);
    let research_removed = manage
        .execute(
            &ManageAgentsRequest {
                token: remove_research_preview.token,
                context: source_context.clone(),
                skill_name: "demo".to_string(),
                agent_selection: remove_research_selection,
                confirm_entity_directories: true,
                original_payload: None,
            },
            CancellationSignal::default(),
        )
        .await?;
    assert_succeeded(&research_removed.units);
    assert!(source_project.join("agent/skills/demo").is_dir());
    assert!(!source_project
        .join("agent/subagents/research/skills/demo")
        .exists());
    assert!(
        read_json(&source_project.join("skills-lock.json"))?["skills"]["demo"]
            .get("subagents")
            .is_none()
    );

    let remaining_entries = observe_skill(&observer, &facts, &targets, &source_context, "demo")
        .await?
        .into_iter()
        .filter(|entry| {
            entry.public.readers.iter().any(|reader| {
                matches!(
                    reader.agent_id.as_str(),
                    "codebuddy" | "minimax-code" | "custom-test"
                ) || reader.logical_target_id == "eve:root"
            })
        })
        .map(|entry| entry.public.entry_id)
        .collect::<Vec<_>>();
    assert_eq!(remaining_entries.len(), 4);
    let final_selection = manage.selection(&source_context, "demo").await?;
    let final_submission = manage_submission(&final_selection, |_| false, InstallMode::Copy);
    let final_manage_request = ManageAgentsPreviewRequest {
        context: source_context.clone(),
        skill_name: "demo".to_string(),
        agent_selection: final_submission.clone(),
    };
    let manage_preview = ready_manage_preview(manage.preview(&final_manage_request).await?);
    let managed = manage
        .execute(
            &ManageAgentsRequest {
                token: manage_preview.token,
                context: source_context.clone(),
                skill_name: "demo".to_string(),
                agent_selection: final_submission,
                confirm_entity_directories: true,
                original_payload: None,
            },
            CancellationSignal::default(),
        )
        .await?;
    assert_eq!(
        managed.units.len(),
        1,
        "Manage Agents must stay atomic per Skill"
    );
    assert_succeeded(&managed.units);
    assert!(!source_project.join(".codebuddy/skills/demo").exists());
    assert!(!source_project.join(".minimax/skills/demo").exists());
    assert!(!source_project.join(".custom/skills/demo").exists());
    assert!(!source_project
        .join("agent/subagents/research/skills/demo")
        .exists());
    let managed_lock = read_json(&source_project.join("skills-lock.json"))?;
    assert!(managed_lock["skills"]["demo"].get("subagents").is_none());

    let update_after_removal_request = UpdateRequest {
        context: source_context.clone(),
        skill_names: vec!["demo".to_string()],
    };
    let update_after_removal_preview = update.preview(&update_after_removal_request).await?;
    assert!(update_after_removal_preview.skills[0]
        .adapter_targets
        .iter()
        .all(|reader| reader.agent_id.as_str() != "eve"));
    let update_after_removal = update
        .execute(
            &UpdateExecutionRequest {
                request: update_after_removal_request,
                overwrite_private_entries: Vec::new(),
            },
            update_after_removal_preview.token,
            CancellationSignal::default(),
        )
        .await?;
    assert_succeeded(
        &update_after_removal
            .skills
            .iter()
            .filter_map(|skill| skill.mutation.clone())
            .collect::<Vec<_>>(),
    );
    assert!(!source_project.join("agent/skills/demo").exists());
    assert!(
        read_json(&source_project.join("skills-lock.json"))?["skills"]["demo"]
            .get("subagents")
            .is_none()
    );

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
        Arc::new(EmptyLibraryCandidateSource),
    );
    let copy_selection = copy.selection(&source_context, "demo").await?.selection;
    let copy_request = CopyRequest {
        skill_name: "demo".to_string(),
        source: source_context.clone(),
        target_environment: EnvironmentRef::Native,
        target_project_ids: vec!["target".to_string()],
        agent_selection: crate::application::agent_selection::AgentSelectionSubmission {
            revision: copy_selection.revision,
            selected_option_ids: copy_selection
                .install_options
                .iter()
                .filter(|option| {
                    option.agent_ids.iter().any(|agent| {
                        matches!(agent.as_str(), "codebuddy" | "minimax-code" | "custom-test")
                    })
                })
                .map(|option| option.id.clone())
                .collect(),
            requested_mode: InstallMode::Copy,
        },
    };
    let copy_preview = match copy.preview(&copy_request).await? {
        CopyPreviewOutcome::Ready { preview } => preview,
        CopyPreviewOutcome::SelectionStale { .. } => {
            return Err(AppError::StaleTarget);
        }
    };
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
    assert_payload_tree(&target_project.join(".codebuddy/skills/demo"), "v2")?;
    assert_payload_tree(&target_project.join(".minimax/skills/demo"), "v2")?;
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
        facts.clone(),
        targets,
        executor(&execution, &environments, &facts),
        Arc::new(EmptyLibraryCandidateSource),
    );
    let remove_preview = remove.preview(&source_context, "demo").await?;
    let removed = remove
        .execute(
            &RemoveRequest {
                token: remove_preview.token,
                context: source_context,
                skill_name: "demo".to_string(),
                intent: RemoveIntent::FullSkill,
            },
            CancellationSignal::default(),
        )
        .await?;
    assert_succeeded(&removed.units);
    assert!(!source_project.join(".agents/skills/demo").exists());
    assert!(!source_project.join(".codebuddy/skills/demo").exists());
    assert!(!source_project.join(".minimax/skills/demo").exists());
    assert_no_staging_leaks(root)?;
    assert_recovery_graph_is_empty(&recovery_root)?;
    Ok(())
}

fn executor(
    execution: &RuntimeExecutionDependencies,
    environments: &Arc<WslRuntime>,
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

fn project_context(id: &str) -> SkillLocationRef {
    SkillLocationRef {
        environment: EnvironmentRef::Native,
        scope: SkillLocation::Project {
            project_id: id.to_string(),
        },
    }
}

pub(crate) fn test_registry() -> AgentRegistrySnapshot {
    let codebuddy = builtin_definition("codebuddy");
    let minimax = builtin_definition("minimax-code");
    let custom = CustomAgentDefinition {
        id: AgentId::parse("custom-test").expect("custom id"),
        display_name: "Custom Test".to_string(),
        global: CustomScopeDefinition {
            enabled: false,
            location: ScopeLocation::Standard,
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
    let eve = builtin_definition("eve");
    AgentRegistry::build(
        vec![codebuddy, minimax, eve],
        vec![CustomAgentRecord::valid(custom)],
    )
    .snapshot()
    .clone()
}

fn builtin_definition(id: &str) -> AgentDefinition {
    builtin_agent_definitions()
        .into_iter()
        .find(|definition| definition.id.as_str() == id)
        .unwrap_or_else(|| panic!("missing built-in Agent definition for {id}"))
}

fn manage_submission(
    snapshot: &ManageAgentSelectionSnapshot,
    keep: impl Fn(&crate::application::agent_selection::AgentInstallOption) -> bool,
    requested_mode: InstallMode,
) -> crate::application::agent_selection::AgentSelectionSubmission {
    crate::application::agent_selection::AgentSelectionSubmission {
        revision: snapshot.selection.revision.clone(),
        selected_option_ids: snapshot
            .selection
            .install_options
            .iter()
            .filter(|option| keep(option))
            .map(|option| option.id.clone())
            .collect(),
        requested_mode,
    }
}

fn ready_manage_preview(outcome: ManageAgentsPreviewOutcome) -> ManageAgentsPreview {
    match outcome {
        ManageAgentsPreviewOutcome::Ready { preview } => preview,
        ManageAgentsPreviewOutcome::SelectionStale { .. } => {
            panic!("expected ready Manage Agents preview")
        }
    }
}

fn metadata(computed_hash: &str, remote_hash: &str) -> PayloadPlanningMetadata {
    PayloadPlanningMetadata {
        skill_name: "demo".to_string(),
        install_dir_name: "demo".to_string(),
        source: "reader/repo".to_string(),
        source_type: "github".to_string(),
        source_url: Some("https://github.com/reader/repo.git".to_string()),
        ref_name: Some("main".to_string()),
        skill_path: "skills/demo".to_string(),
        plugin_name: Some("integration".to_string()),
        computed_hash: computed_hash.to_string(),
        upstream_revision: Some(remote_hash.to_string()),
        well_known: None,
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
#[tokio::test]
async fn native_scope_version_election_survives_a_continuous_product_workflow() {
    run_native_scope_version_election_workflow()
        .await
        .expect("native Scope version-election workflow");
}

#[cfg(all(test, unix))]
#[tokio::test]
async fn native_scope_version_election_recognizes_library_links_across_path_aliases() {
    use std::os::unix::fs::symlink;

    let temp = tempfile::tempdir().expect("path alias fixture");
    let physical_root = temp.path().join("physical");
    let logical_root = temp.path().join("logical");
    fs::create_dir(&physical_root).expect("physical root");
    symlink(&physical_root, &logical_root).expect("logical root alias");

    run_native_scope_version_election_workflow_at(&logical_root)
        .await
        .expect("native Scope version-election workflow through a path alias");
}

#[cfg(test)]
async fn run_native_scope_version_election_workflow() -> Result<(), AppError> {
    let temp = tempfile::tempdir()?;
    run_native_scope_version_election_workflow_at(temp.path()).await
}

#[cfg(test)]
async fn run_native_scope_version_election_workflow_at(root: &Path) -> Result<(), AppError> {
    let project_path = root.join("project");
    let projects_path = root.join("state/projects.json");
    let global_lock_path = root.join("state/global-lock.json");
    let recovery_root = root.join("recovery");
    let home = root.join("home");
    let config_home = root.join("config");
    let members_root = root.join("libraries");
    let first_member = members_root.join("library-one/skills/demo");
    let second_member = members_root.join("library-two/skills/demo");

    fs::create_dir_all(project_path.join(".agents/skills"))?;
    fs::create_dir_all(project_path.join(".codebuddy/skills"))?;
    fs::create_dir_all(project_path.join(".minimax/skills"))?;
    fs::create_dir_all(&home)?;
    fs::create_dir_all(&config_home)?;
    write_json(
        &projects_path,
        &json!({
            "schemaVersion": 1,
            "projects": [project("version-election", &project_path, "Version Election")]
        }),
    )?;
    let _first_payload = create_payload(&first_member, "library-one")?;
    let _second_payload = create_payload(&second_member, "library-two")?;

    let context = project_context("version-election");
    let first_id = LibraryId::parse("library-one");
    let second_id = LibraryId::parse("library-two");
    let repository: Arc<dyn LibraryApplicationRepository> =
        Arc::new(MemoryLibraryApplicationRepository {
            record: Mutex::new(LibraryApplicationRecord::empty(context.clone())),
            catalog: LibraryCatalog {
                schema_version: LIBRARY_SCHEMA_VERSION,
                libraries: vec![
                    test_library_record(first_id.clone(), "Library One", "library-one"),
                    test_library_record(second_id.clone(), "Library Two", "library-two"),
                ],
                extra: serde_json::Map::new(),
            },
            members_root,
        });
    let registry = Arc::new(StaticRegistry(Arc::new(test_registry())));
    let environments = Arc::new(WslRuntime::default());
    let facts = RuntimePlanningFactSource::with_native_snapshot(
        registry,
        environments.clone(),
        NativeRuntimeSnapshot {
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
            max_sessions: 8,
            max_bytes: 8 * 1024 * 1024,
        },
        || 1_000,
    ));
    let execution = RuntimeExecutionDependencies::new(environments.clone(), recovery_root.clone())?;
    let library_application = Arc::new(LibraryApplicationModule::new(
        repository.clone(),
        facts.clone(),
        targets.clone(),
        executor(&execution, &environments, &facts),
    ));
    let library_candidates: Arc<dyn LibraryCandidateSource> = Arc::new(
        RepositoryLibraryCandidateSource::new(repository, targets.clone()),
    );
    let agent_a = AgentId::parse("codebuddy").expect("Agent A id");
    let agent_b = AgentId::parse("minimax-code").expect("Agent B id");
    let canonical = project_path.join(".agents/skills/demo");
    let agent_a_entry = project_path.join(".codebuddy/skills/demo");
    let agent_b_entry = project_path.join(".minimax/skills/demo");

    let initial_application = LibraryApplicationDraft {
        context: context.clone(),
        ordered_library_ids: vec![first_id.clone(), second_id.clone()],
        selected_agent_ids: vec![agent_a.clone(), agent_b.clone()],
    };
    let initial_preview = library_application
        .preview(initial_application.clone())
        .await?;
    let initial_result = library_application
        .apply(
            ApplyLibraryApplicationRequest {
                draft: initial_application,
                expected_token: initial_preview.token,
            },
            CancellationSignal::default(),
        )
        .await?;
    assert_succeeded(&initial_result.units);
    assert_resolves_to(&canonical, &first_member)?;
    assert_resolves_to(&agent_a_entry, &first_member)?;
    assert_resolves_to(&agent_b_entry, &first_member)?;
    let idempotent_draft = LibraryApplicationDraft {
        context: context.clone(),
        ordered_library_ids: vec![first_id.clone(), second_id.clone()],
        selected_agent_ids: vec![agent_a.clone(), agent_b.clone()],
    };
    let idempotent_preview = library_application
        .preview(idempotent_draft.clone())
        .await?;
    let idempotent = library_application
        .apply(
            ApplyLibraryApplicationRequest {
                draft: idempotent_draft,
                expected_token: idempotent_preview.token,
            },
            CancellationSignal::default(),
        )
        .await?;
    assert!(idempotent.units.is_empty());

    let direct_source = root.join("payload-direct/demo");
    let direct_payload = create_payload(&direct_source, "direct")?;
    let discovery = payloads
        .discover(EnvironmentRef::Native, "version-election-direct")
        .await?;
    let direct_handle = payloads
        .acquire_payload_with_metadata(
            &discovery,
            "skills/demo",
            direct_payload,
            metadata("computed-direct", "remote-direct"),
        )
        .await?;
    let selection_facts = ScopePlanningSnapshotSource::snapshot(&facts, &context).await?;
    let direct_selection = test_submission_for_agents_and_own_directories(
        &context,
        &selection_facts.agent_runtime,
        &selection_facts.eve_targets,
        &selection_facts.resolved_context.skill_root,
        &targets,
        &[],
        InstallMode::Copy,
    )
    .await;
    let install_request = InstallRequest {
        context: context.clone(),
        source: "reader/direct".to_string(),
        discovery_session: discovery,
        payloads: vec![direct_handle],
        skills: vec!["demo".to_string()],
        agent_selection: direct_selection,
        acknowledge_redirect: true,
    };
    let install = InstallService::new(
        payloads.clone(),
        ConcreteInstallPlanner::new(
            facts.clone(),
            targets.clone(),
            payloads.clone(),
            fixed_time,
            library_candidates.clone(),
        ),
        executor(&execution, &environments, &facts),
    );
    let InstallPreviewOutcome::Ready {
        preview: install_preview,
    } = install
        .preview(InstallOperation::Install, &install_request)
        .await?
    else {
        panic!("expected ready direct install preview");
    };
    let installed = install
        .execute(
            InstallOperation::Install,
            &install_request,
            install_preview.token,
            CancellationSignal::default(),
        )
        .await?;
    assert_succeeded(&installed.units);
    assert_payload_tree(&canonical, "direct")?;
    assert_resolves_to(&agent_a_entry, &first_member)?;
    assert_resolves_to(&agent_b_entry, &first_member)?;

    let manage = ManageAgentsService::new(
        facts.clone(),
        ScopeSkillPlacementResolver::new(targets.clone()),
        targets.clone(),
        payloads.clone(),
        InstalledSkillPayloadAcquirer::new(payloads.clone(), environments.clone()),
        executor(&execution, &environments, &facts),
        library_candidates.clone(),
    );
    let add_selection = manage.selection(&context, "demo").await?;
    let add_agent_a = manage_submission(
        &add_selection,
        |option| option.agent_ids.contains(&agent_a),
        InstallMode::Copy,
    );
    let add_preview_request = ManageAgentsPreviewRequest {
        context: context.clone(),
        skill_name: "demo".to_string(),
        agent_selection: add_agent_a.clone(),
    };
    let add_preview = ready_manage_preview(manage.preview(&add_preview_request).await?);
    let added = manage
        .execute(
            &ManageAgentsRequest {
                token: add_preview.token,
                context: context.clone(),
                skill_name: "demo".to_string(),
                agent_selection: add_agent_a,
                confirm_entity_directories: false,
                original_payload: add_preview.original_payload,
            },
            CancellationSignal::default(),
        )
        .await?;
    assert_succeeded(&added.units);
    assert_payload_tree(&canonical, "direct")?;
    assert_payload_tree(&agent_a_entry, "direct")?;
    assert_resolves_to(&agent_b_entry, &first_member)?;

    let manage_selection = manage.selection(&context, "demo").await?;
    let remove_agent_a = manage_submission(&manage_selection, |_| false, InstallMode::Copy);
    let manage_preview_request = ManageAgentsPreviewRequest {
        context: context.clone(),
        skill_name: "demo".to_string(),
        agent_selection: remove_agent_a.clone(),
    };
    let manage_preview = ready_manage_preview(manage.preview(&manage_preview_request).await?);
    let managed = manage
        .execute(
            &ManageAgentsRequest {
                token: manage_preview.token,
                context: context.clone(),
                skill_name: "demo".to_string(),
                agent_selection: remove_agent_a,
                confirm_entity_directories: true,
                original_payload: None,
            },
            CancellationSignal::default(),
        )
        .await?;
    assert_succeeded(&managed.units);
    assert_payload_tree(&canonical, "direct")?;
    assert_resolves_to(&agent_a_entry, &first_member)?;
    assert_resolves_to(&agent_b_entry, &first_member)?;

    let reordered_application = LibraryApplicationDraft {
        context: context.clone(),
        ordered_library_ids: vec![second_id.clone(), first_id],
        selected_agent_ids: vec![agent_a, agent_b],
    };
    let reordered_preview = library_application
        .preview(reordered_application.clone())
        .await?;
    let reordered = library_application
        .apply(
            ApplyLibraryApplicationRequest {
                draft: reordered_application,
                expected_token: reordered_preview.token,
            },
            CancellationSignal::default(),
        )
        .await?;
    assert_succeeded(&reordered.units);
    assert_payload_tree(&canonical, "direct")?;
    assert_resolves_to(&agent_a_entry, &second_member)?;
    assert_resolves_to(&agent_b_entry, &second_member)?;

    let remove = RemoveService::new(
        facts.clone(),
        targets,
        executor(&execution, &environments, &facts),
        library_candidates,
    );
    let remove_preview = remove.preview(&context, "demo").await?;
    let removed = remove
        .execute(
            &RemoveRequest {
                token: remove_preview.token,
                context: context.clone(),
                skill_name: "demo".to_string(),
                intent: RemoveIntent::FullSkill,
            },
            CancellationSignal::default(),
        )
        .await?;
    assert_succeeded(&removed.units);
    assert_resolves_to(&canonical, &second_member)?;
    assert_resolves_to(&agent_a_entry, &second_member)?;
    assert_resolves_to(&agent_b_entry, &second_member)?;

    let empty_application = LibraryApplicationDraft {
        context,
        ordered_library_ids: Vec::new(),
        selected_agent_ids: Vec::new(),
    };
    let empty_preview = library_application
        .preview(empty_application.clone())
        .await?;
    let unapplied = library_application
        .apply(
            ApplyLibraryApplicationRequest {
                draft: empty_application,
                expected_token: empty_preview.token,
            },
            CancellationSignal::default(),
        )
        .await?;
    assert_succeeded(&unapplied.units);
    assert!(!canonical.exists());
    assert!(!agent_a_entry.exists());
    assert!(!agent_b_entry.exists());
    assert_payload_tree(&first_member, "library-one")?;
    assert_payload_tree(&second_member, "library-two")?;
    assert_no_staging_leaks(root)?;
    assert_recovery_graph_is_empty(&recovery_root)?;
    Ok(())
}

#[cfg(test)]
fn test_library_record(id: LibraryId, name: &str, version: &str) -> SkillLibraryRecord {
    SkillLibraryRecord {
        id,
        name: name.to_string(),
        skills: vec![LibrarySkillRecord {
            name: "demo".to_string(),
            description: version.to_string(),
            source_record: json!({
                "sourceType": "local",
                "source": version,
                "skillPath": "demo"
            }),
            content_manifest_hash: format!("manifest-{version}"),
            updated_at: Some(fixed_time()),
            extra: serde_json::Map::new(),
        }],
        extra: serde_json::Map::new(),
    }
}

#[cfg(test)]
fn assert_resolves_to(entry: &Path, expected: &Path) -> Result<(), AppError> {
    assert_eq!(
        fs::canonicalize(entry)?,
        fs::canonicalize(expected)?,
        "{} must resolve to {}",
        entry.display(),
        expected.display()
    );
    Ok(())
}

#[cfg(test)]
async fn discover_http_source_returning(
    status: u16,
) -> Result<crate::application::source_acquisition::FetchResult, AppError> {
    use std::thread;
    use std::time::Duration;

    use crate::application::wellknown_access::WellKnownAccess;
    use crate::application::wsl_source_access::UnavailableWslSourceAccess;
    use crate::models::NetworkProxySettings;
    use crate::runtime::download::RuntimeDownloadAccess;
    use crate::runtime::git_source::ProcessGitTransport;
    use crate::runtime::http_transport::HttpTransport;
    use crate::runtime::proxy_settings::ProxySettingsStore;
    use crate::runtime::source_acquisition::SourceDiscoveryService;
    use crate::runtime::wellknown::RuntimeWellKnownAccess;

    let server = tiny_http::Server::http("127.0.0.1:0").expect("HTTP server");
    let source = format!(
        "http://{}/missing",
        server.server_addr().to_ip().expect("server address")
    );
    let worker = thread::spawn(move || {
        for _ in 0..5 {
            let request = server
                .recv_timeout(Duration::from_secs(2))
                .expect("receive request")
                .expect("request");
            request
                .respond(tiny_http::Response::empty(tiny_http::StatusCode(status)))
                .expect("HTTP response");
        }
    });

    let payloads = Arc::new(PayloadSessionManager::in_memory(
        PayloadSessionLimits {
            ttl_ms: 60_000,
            max_sessions: 4,
            max_bytes: 4 * 1024 * 1024,
        },
        || 1_000,
    ));
    let proxy = Arc::new(ProxySettingsStore::new(NetworkProxySettings::default()));
    let http = HttpTransport::new(proxy.clone());
    let result = SourceDiscoveryService::new(
        payloads,
        Arc::new(ProcessGitTransport::new(proxy)),
        Arc::new(RuntimeWellKnownAccess::new(http.clone())) as Arc<dyn WellKnownAccess>,
        RuntimeDownloadAccess::new(http),
        Arc::new(UnavailableWslSourceAccess),
    )
    .discover(EnvironmentRef::Native, source, |_| {})
    .await;
    worker.join().expect("HTTP worker");

    result
}

#[cfg(test)]
async fn discover_unreachable_http_source(
) -> Result<crate::application::source_acquisition::FetchResult, AppError> {
    use crate::application::wellknown_access::WellKnownAccess;
    use crate::application::wsl_source_access::UnavailableWslSourceAccess;
    use crate::models::NetworkProxySettings;
    use crate::runtime::download::RuntimeDownloadAccess;
    use crate::runtime::git_source::ProcessGitTransport;
    use crate::runtime::http_transport::HttpTransport;
    use crate::runtime::proxy_settings::ProxySettingsStore;
    use crate::runtime::source_acquisition::SourceDiscoveryService;
    use crate::runtime::wellknown::RuntimeWellKnownAccess;

    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("reserve local port");
    let address = listener.local_addr().expect("local address");
    drop(listener);

    let payloads = Arc::new(PayloadSessionManager::in_memory(
        PayloadSessionLimits {
            ttl_ms: 60_000,
            max_sessions: 4,
            max_bytes: 4 * 1024 * 1024,
        },
        || 1_000,
    ));
    let proxy = Arc::new(ProxySettingsStore::new(NetworkProxySettings::default()));
    let http = HttpTransport::new(proxy.clone());
    SourceDiscoveryService::new(
        payloads,
        Arc::new(ProcessGitTransport::new(proxy)),
        Arc::new(RuntimeWellKnownAccess::new(http.clone())) as Arc<dyn WellKnownAccess>,
        RuntimeDownloadAccess::new(http),
        Arc::new(UnavailableWslSourceAccess),
    )
    .discover(
        EnvironmentRef::Native,
        format!("http://{address}/unreachable"),
        |_| {},
    )
    .await
}

#[cfg(test)]
#[tokio::test]
async fn http_source_reports_real_failure_reasons_for_both_attempts() {
    use crate::error::SourceAcquisitionFailureReason;

    assert!(matches!(
        discover_http_source_returning(404).await,
        Err(AppError::SourceAcquisitionFailed {
            well_known_reason: SourceAcquisitionFailureReason::NotFound,
            download_reason: SourceAcquisitionFailureReason::NotFound,
        })
    ));
    assert!(matches!(
        discover_http_source_returning(401).await,
        Err(AppError::SourceAcquisitionFailed {
            well_known_reason: SourceAcquisitionFailureReason::AuthenticationRequired,
            download_reason: SourceAcquisitionFailureReason::AuthenticationRequired,
        })
    ));
    assert!(matches!(
        discover_http_source_returning(500).await,
        Err(AppError::SourceAcquisitionFailed {
            well_known_reason: SourceAcquisitionFailureReason::Network,
            download_reason: SourceAcquisitionFailureReason::Network,
        })
    ));
    assert!(matches!(
        discover_unreachable_http_source().await,
        Err(AppError::SourceAcquisitionFailed {
            well_known_reason: SourceAcquisitionFailureReason::Network,
            download_reason: SourceAcquisitionFailureReason::Network,
        })
    ));
}

#[cfg(test)]
#[tokio::test]
async fn direct_download_flows_from_http_discovery_through_install_without_lock() {
    use std::thread;
    use std::time::Duration;

    use crate::application::source_acquisition::{
        AcquireSelectedPayloadsRequest, SelectedPayloadAcquisitionService,
    };
    use crate::application::wellknown_access::WellKnownAccess;
    use crate::application::wsl_source_access::UnavailableWslSourceAccess;
    use crate::models::NetworkProxySettings;
    use crate::runtime::download::RuntimeDownloadAccess;
    use crate::runtime::git_source::ProcessGitTransport;
    use crate::runtime::http_transport::HttpTransport;
    use crate::runtime::proxy_settings::ProxySettingsStore;
    use crate::runtime::source_acquisition::SourceDiscoveryService;
    use crate::runtime::wellknown::RuntimeWellKnownAccess;

    let temp = tempfile::tempdir().expect("download workflow tempdir");
    let project_path = temp.path().join("project");
    let projects_path = temp.path().join("state/projects.json");
    let global_lock_path = temp.path().join("state/global-lock.json");
    let recovery_root = temp.path().join("recovery");
    fs::create_dir_all(project_path.join(".codebuddy")).unwrap();
    fs::create_dir_all(project_path.join(".minimax")).unwrap();
    write_json(
        &projects_path,
        &json!({
            "schemaVersion": 1,
            "projects": [project("source", &project_path, "Source")]
        }),
    )
    .unwrap();

    let server = tiny_http::Server::http("127.0.0.1:0").expect("HTTP server");
    let source = format!(
        "http://{}/artifact",
        server.server_addr().to_ip().expect("server address")
    );
    let mut archive_bytes = Vec::new();
    {
        let mut archive = zip::ZipWriter::new(std::io::Cursor::new(&mut archive_bytes));
        for skill in ["alpha", "beta"] {
            archive
                .start_file(
                    format!("{skill}/SKILL.md"),
                    zip::write::SimpleFileOptions::default(),
                )
                .expect("archive entry");
            std::io::Write::write_all(
                &mut archive,
                format!("---\nname: {skill}\ndescription: Direct {skill}\n---\n# {skill}\n")
                    .as_bytes(),
            )
            .expect("archive content");
        }
        archive.finish().expect("finish archive");
    }
    let worker = thread::spawn(move || {
        for _ in 0..5 {
            let request = server
                .recv_timeout(Duration::from_secs(2))
                .expect("receive request")
                .expect("request");
            if request.url() == "/artifact" {
                request
                    .respond(tiny_http::Response::from_data(archive_bytes))
                    .expect("download response");
                return;
            }
            request
                .respond(tiny_http::Response::empty(404))
                .expect("well-known response");
        }
        panic!("direct download request was not received");
    });

    let environments = Arc::new(WslRuntime::default());
    let registry = Arc::new(StaticRegistry(Arc::new(test_registry())));
    let facts = RuntimePlanningFactSource::with_native_snapshot(
        registry,
        environments.clone(),
        NativeRuntimeSnapshot {
            home: temp.path().join("home"),
            config_home: temp.path().join("config"),
            projects_path,
            global_lock_path,
            environment_variables: BTreeMap::new(),
        },
    );
    let targets = RuntimeTargetFactResolver::new(environments.clone());
    let payloads = Arc::new(PayloadSessionManager::in_memory(
        PayloadSessionLimits {
            ttl_ms: 60_000,
            max_sessions: 4,
            max_bytes: 4 * 1024 * 1024,
        },
        || 1_000,
    ));
    let proxy = Arc::new(ProxySettingsStore::new(NetworkProxySettings::default()));
    let http = HttpTransport::new(proxy.clone());
    let discovery = SourceDiscoveryService::new(
        payloads.clone(),
        Arc::new(ProcessGitTransport::new(proxy)),
        Arc::new(RuntimeWellKnownAccess::new(http.clone())) as Arc<dyn WellKnownAccess>,
        RuntimeDownloadAccess::new(http),
        Arc::new(UnavailableWslSourceAccess),
    )
    .discover(EnvironmentRef::Native, source.clone(), |_| {})
    .await
    .expect("discover direct download");
    worker.join().expect("HTTP worker");
    assert_eq!(discovery.source_type, "download");
    let serialized_discovery =
        serde_json::to_value(&discovery).expect("serialize discovery result");
    assert!(serialized_discovery.get("riskPolicy").is_none());

    let skill_paths = discovery
        .skills
        .iter()
        .map(|skill| skill.relative_path.clone())
        .collect::<Vec<_>>();
    let skill_names = discovery
        .skills
        .iter()
        .map(|skill| skill.name.clone())
        .collect::<Vec<_>>();
    assert_eq!(skill_names, ["alpha", "beta"]);
    let handles = SelectedPayloadAcquisitionService::new(payloads.clone())
        .acquire(AcquireSelectedPayloadsRequest {
            discovery_session: discovery.discovery_session.clone(),
            skill_paths,
        })
        .await
        .expect("pin downloaded payload");
    let context = project_context("source");
    let selection_facts = ScopePlanningSnapshotSource::snapshot(&facts, &context)
        .await
        .expect("Agent selection facts");
    let agent_selection = test_submission_for_agents_and_own_directories(
        &context,
        &selection_facts.agent_runtime,
        &selection_facts.eve_targets,
        &selection_facts.resolved_context.skill_root,
        &targets,
        &[],
        InstallMode::Copy,
    )
    .await;
    let install = InstallService::new(
        payloads.clone(),
        ConcreteInstallPlanner::new(
            facts.clone(),
            targets,
            payloads,
            fixed_time,
            Arc::new(EmptyLibraryCandidateSource),
        ),
        SelectiveVerifyFailurePlanExecutor {
            environments: environments.clone(),
            facts: facts.clone(),
            recovery_root: recovery_root.clone(),
            failing_skill: "beta".to_string(),
        },
    );
    let request = InstallRequest {
        context,
        source,
        discovery_session: discovery.discovery_session,
        payloads: handles,
        skills: skill_names,
        agent_selection,
        acknowledge_redirect: true,
    };
    fs::create_dir_all(project_path.join(".agents/skills/alpha")).unwrap();
    fs::write(
        project_path.join(".agents/skills/alpha/SKILL.md"),
        "existing",
    )
    .unwrap();
    let InstallPreviewOutcome::Ready {
        preview: stale_preview,
    } = install
        .preview(InstallOperation::Install, &request)
        .await
        .expect("preview direct download")
    else {
        panic!("expected ready direct-download preview");
    };
    assert!(!stale_preview.skills[0].blocking_reasons.is_empty());
    fs::remove_dir_all(project_path.join(".agents/skills/alpha")).unwrap();
    assert!(matches!(
        install
            .execute(
                InstallOperation::Install,
                &request,
                stale_preview.token,
                CancellationSignal::default(),
            )
            .await,
        Err(AppError::StaleContext)
    ));

    let InstallPreviewOutcome::Ready { preview } = install
        .preview(InstallOperation::Install, &request)
        .await
        .expect("refresh direct-download preview")
    else {
        panic!("expected refreshed direct-download preview");
    };
    assert!(preview
        .skills
        .iter()
        .all(|skill| skill.blocking_reasons.is_empty()));
    let response = install
        .execute(
            InstallOperation::Install,
            &request,
            preview.token,
            CancellationSignal::default(),
        )
        .await
        .expect("execute direct-download batch");
    assert_eq!(response.units.len(), 2);
    assert_eq!(response.units[0].skill_name, "alpha");
    assert_eq!(response.units[0].status, MutationUnitStatus::Succeeded);
    assert_eq!(response.units[1].skill_name, "beta");
    assert_eq!(response.units[1].status, MutationUnitStatus::Failed);
    assert!(response.units.iter().all(|unit| !unit.lock_committed));
    assert!(project_path.join(".agents/skills/alpha/SKILL.md").is_file());
    assert!(!project_path.join(".agents/skills/beta").exists());
    assert!(!project_path.join("skills-lock.json").exists());
    assert_no_staging_leaks(temp.path()).unwrap();
    assert_recovery_graph_is_empty(&recovery_root).unwrap();
}

#[cfg(test)]
mod update_lifecycle {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    use super::*;
    use crate::application::install::InstallFuture;
    use crate::application::mutation::coordinator::{
        BoxFuture, MutationCoordinator, PreparedEntryExecutor,
    };
    use crate::application::mutation::executor::MutationPlanExecutor;
    use crate::application::mutation::plan::{ExecutionUnit, MutationPlan};
    use crate::application::mutation::result::{MutationUnitStatus, MutationWarning};
    use crate::application::payload_session::{PayloadSessionManager, PinnedPayloadLease};
    use crate::application::resources::SkillIdentity;
    use crate::application::source_acquisition::{
        AcquireSelectedPayloadsRequest, SelectedPayloadAcquisitionService,
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
    use crate::application::update_subjects::InstalledUpdateSubjectProvider;
    use crate::core::mutation::CancellationSignal;
    use crate::core::skill_payload::PayloadId;
    use crate::environment::native::materialize::{
        NativePreparedEntryExecutor, NativePreparedEntrySet,
    };
    use crate::environment::native::recovery::NativeRecoveryMarkerStore;
    use crate::environment::recovery::RecoveryMarkerStore;
    use crate::environment::runtime::ExecutionBackend;
    use crate::error::AppError;
    use crate::models::{ParsedSource, SourceType};
    use crate::runtime::plan_runner::{RuntimeLockCommitter, RuntimePlanExecutor};
    use crate::runtime::source_acquisition::SourceDiscoveryService;
    use crate::runtime::update_service::{RuntimeSkillSourceModule, RuntimeUpdateService};

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
        environments: Arc<WslRuntime>,
        facts: RuntimePlanningFactSource,
        recovery_root: PathBuf,
        private_root: PathBuf,
    }

    impl MutationPlanExecutor for StageFailurePlanExecutor {
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
        remote: FileBareSkillRepo,
        git_transport: Arc<CountingGitTransport>,
        project_path: PathBuf,
        recovery_root: PathBuf,
        environments: Arc<WslRuntime>,
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
            fs::create_dir_all(project_path.join(".codebuddy"))
                .expect("create CodeBuddy fixture root");
            fs::create_dir_all(project_path.join(".minimax"))
                .expect("create MiniMax Code fixture root");
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

            let environments = Arc::new(WslRuntime::default());
            let registry = Arc::new(StaticRegistry(Arc::new(test_registry())));
            let facts = RuntimePlanningFactSource::with_native_snapshot(
                registry,
                environments.clone(),
                NativeRuntimeSnapshot {
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
            let remote = FileBareSkillRepo::new(&skill_names);
            let git_transport = Arc::new(CountingGitTransport::for_repo(&remote));
            let detector = Arc::new(CountingDetector {
                inner: RuntimeSourceEvidenceDetector::with_git_transport(
                    payloads.clone(),
                    snapshots.clone(),
                    git_transport.clone(),
                ),
                calls: detector_calls.clone(),
            });
            let evidence =
                SourceEvidenceCoordinator::with_snapshot_reuse(detector, snapshots.clone());
            let execution =
                RuntimeExecutionDependencies::new(environments.clone(), recovery_root.clone())
                    .expect("lifecycle execution dependencies");
            Self {
                _root: root,
                remote,
                git_transport,
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

        fn context(&self) -> SkillLocationRef {
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
                RuntimeSkillSourceModule::with_git_transport(
                    self.payloads.clone(),
                    self.snapshots.clone(),
                    self.evidence.clone(),
                    self.git_transport.clone(),
                ),
                self.executor(),
            )
        }

        fn check_service(
            &self,
        ) -> UpdateCheckService<
            InstalledUpdateSubjectProvider<RuntimePlanningFactSource, RuntimeTargetFactResolver>,
        > {
            UpdateCheckService::new(
                InstalledUpdateSubjectProvider::new(self.facts.clone(), self.targets.clone()),
                self.evidence.clone(),
            )
        }

        async fn install(&self) {
            let discovery = SourceDiscoveryService::with_git_transport(
                self.payloads.clone(),
                self.git_transport.clone(),
            )
            .discover_parsed_with_cancellation(
                self.context().environment,
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
                    Arc::new(EmptyLibraryCandidateSource),
                ),
                self.executor(),
            );
            for (skill_name, handle) in ["alpha", "beta"].into_iter().zip(handles) {
                let context = self.context();
                let selection_facts = ScopePlanningSnapshotSource::snapshot(&self.facts, &context)
                    .await
                    .expect("load lifecycle Agent selection facts");
                let agent_selection = test_submission_for_agents_and_own_directories(
                    &context,
                    &selection_facts.agent_runtime,
                    &selection_facts.eve_targets,
                    &selection_facts.resolved_context.skill_root,
                    &self.targets,
                    &["codebuddy", "minimax-code", "custom-test"],
                    InstallMode::Copy,
                )
                .await;
                let request = InstallRequest {
                    context,
                    source: self.remote.source(),
                    discovery_session: discovery.discovery_session.clone(),
                    payloads: vec![handle],
                    skills: vec![skill_name.to_string()],
                    agent_selection,
                    acknowledge_redirect: true,
                };
                let preview_outcome = install
                    .preview(InstallOperation::Install, &request)
                    .await
                    .expect("preview lifecycle install");
                let InstallPreviewOutcome::Ready { preview } = preview_outcome else {
                    panic!("expected ready lifecycle install preview");
                };
                let installed = install
                    .execute(
                        InstallOperation::Install,
                        &request,
                        preview.token,
                        CancellationSignal::default(),
                    )
                    .await
                    .expect("execute lifecycle install");
                assert_succeeded(&installed.units);
            }
            assert_eq!(self.clone_count(), 1);
            fs::write(
                self.project_path.join(".codebuddy/skills/alpha/SKILL.md"),
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
                    selection: UpdateCheckSelection::Skills(
                        ["alpha", "beta"]
                            .into_iter()
                            .map(|skill_name| crate::application::resources::SkillIdentity {
                                context: self.context(),
                                skill_name: skill_name.to_string(),
                            })
                            .collect(),
                    ),
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
            self.git_transport.clone_count()
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
            assert_eq!(preview.skills[0].clean_copy_count, 2);
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
                fs::read_to_string(self.project_path.join(".codebuddy/skills/alpha/SKILL.md"),)
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
                RuntimeSkillSourceModule::with_git_transport(
                    self.payloads.clone(),
                    self.snapshots.clone(),
                    self.evidence.clone(),
                    self.git_transport.clone(),
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
            assert_eq!(beta.clean_copy_count, 3);
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

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
    InstallFuture, InstallPreviewOutcome, InstallRequest, InstallService,
};
use crate::application::install_planner::ConcreteInstallPlanner;
use crate::application::installed_skill_payload::InstalledSkillPayloadAcquirer;
use crate::application::installed_skill_resolver::SkillDirectoryName;
use crate::application::library_application::{
    ApplicationInventory, ApplicationRegistry, ApplyLibraryApplicationRequest,
    LibraryApplicationBackend, LibraryApplicationDraft, LibraryApplicationFuture,
    LibraryApplicationModule, LibraryApplicationRecord, LibraryApplicationResources,
    VersionedApplicationRecord,
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
    BoxFuture, MutationCoordinator, PreparedEntryTestDriver, PreparedLockCommitter,
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

pub(crate) fn update_library_repository(
    root: &std::path::Path,
) -> Arc<dyn crate::application::skill_libraries::SkillLibraryRepository> {
    Arc::new(
        crate::runtime::skill_libraries::RuntimeSkillLibraryRepository::new(
            root.join("library-records"),
            Arc::new(WslRuntime::default()),
            Arc::new(crate::core::projects::ProjectMigrationRegistry::new(
                crate::core::projects::ProjectMigrationState::NotNeeded,
            )),
        ),
    )
}
use crate::application::update::{
    AcquiredUpdateSource, UpdateAcquisitionGroup, UpdateFuture, UpdateRequest, UpdateService,
    UpdateSourceAcquisition,
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
use crate::storage::atomic_document::DocumentWriteFailure;
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

struct MemoryApplicationRegistry {
    record: Mutex<LibraryApplicationRecord>,
    catalog: LibraryCatalog,
    members_root: PathBuf,
}

impl ApplicationRegistry for MemoryApplicationRegistry {
    fn load_application<'a>(
        &'a self,
        context: &'a SkillLocationRef,
    ) -> LibraryApplicationFuture<'a, Result<VersionedApplicationRecord, AppError>> {
        Box::pin(async move {
            Ok(VersionedApplicationRecord::in_memory(
                context.clone(),
                self.record.lock().expect("library record lock").clone(),
            ))
        })
    }

    fn save_application_if<'a>(
        &'a self,
        observed: &'a VersionedApplicationRecord,
        record: &'a LibraryApplicationRecord,
    ) -> LibraryApplicationFuture<'a, Result<VersionedApplicationRecord, AppError>> {
        Box::pin(async move {
            *self.record.lock().expect("library record lock") = record.clone();
            Ok(VersionedApplicationRecord::in_memory(
                observed.context.clone(),
                record.clone(),
            ))
        })
    }

    fn enumerate<'a>(
        &'a self,
        environment: &'a EnvironmentRef,
    ) -> LibraryApplicationFuture<'a, Result<ApplicationInventory, AppError>> {
        Box::pin(async move {
            Ok(ApplicationInventory {
                records: vec![VersionedApplicationRecord::in_memory(
                    SkillLocationRef {
                        environment: environment.clone(),
                        scope: SkillLocation::Global,
                    },
                    self.record.lock().expect("library record lock").clone(),
                )],
                problems: Vec::new(),
                complete: true,
            })
        })
    }
}

impl LibraryApplicationResources for MemoryApplicationRegistry {
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

    fn remove_application_if<'a>(
        &'a self,
        _observed: &'a VersionedApplicationRecord,
    ) -> LibraryApplicationFuture<'a, Result<(), AppError>> {
        Box::pin(async move {
            *self.record.lock().expect("library record lock") = LibraryApplicationRecord::empty();
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

impl PreparedEntryTestDriver for SelectiveVerifyFailureEntryExecutor {
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

impl PreparedEntryTestDriver for VerifyFailureEntryExecutor {
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
    facts: RuntimePlanningFactSource,
    recovery_root: PathBuf,
}

struct SelectiveVerifyFailurePlanExecutor {
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
    ) -> BoxFuture<'a, Result<LockCommitReceipt, DocumentWriteFailure>> {
        self.attempted
            .store(true, std::sync::atomic::Ordering::SeqCst);
        Box::pin(async {
            Err(DocumentWriteFailure::not_published(
                AppError::ExecutionFailed {
                    message: "injected Manage Agents lock failure".to_string(),
                },
            ))
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
            MutationCoordinator::from_phases(
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
            MutationCoordinator::from_phases(
                entries,
                crate::runtime::plan_runner::RuntimeLockCommitter::new(),
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
            MutationCoordinator::from_phases(
                entries,
                crate::runtime::plan_runner::RuntimeLockCommitter::new(),
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
                    redirected_download_hosts: Vec::new(),
                    _leases: Vec::new(),
                    discovery_session: DiscoverySessionHandle {
                        session_id: handle.session_id.clone(),
                        environment: handle.environment.clone(),
                        source_fingerprint: handle.source_fingerprint.clone(),
                        expires_at_epoch_ms: handle.expires_at_epoch_ms,
                    },
                    payloads: vec![("demo".to_string(), handle)],
                    skill_errors: Vec::new(),
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
    } = install.preview(&install_request).await?
    else {
        panic!("expected ready install preview");
    };
    let installed = install
        .execute(
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
        ConcreteUpdatePlanner::new(
            facts.clone(),
            targets.clone(),
            payloads.clone(),
            update_library_repository(&source_project),
            fixed_time,
        ),
        FixedUpdateAcquirer { handle: handle_v2 },
        executor(&execution, &environments, &facts),
    );
    let update_request = UpdateRequest {
        context: source_context.clone(),
        skill_names: vec!["demo".to_string()],
    };
    let prepared = update
        .prepare(&update_request, CancellationSignal::default())
        .await?;
    let overwrite_private_entries = prepared.preview.skills[0]
        .overwrite_private_entries
        .iter()
        .map(|entry| entry.entry_id.clone())
        .chain(
            prepared.preview.skills[0]
                .targets
                .iter()
                .filter_map(|target| target.selectable_entry_id.clone()),
        )
        .collect::<Vec<_>>();
    let updated = update
        .execute_prepared(
            prepared,
            &overwrite_private_entries,
            CancellationSignal::default(),
            |_| {},
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
    let prepared = update
        .prepare(&update_after_removal_request, CancellationSignal::default())
        .await?;
    assert!(prepared.preview.skills[0]
        .adapter_targets
        .iter()
        .all(|reader| reader.agent_id.as_str() != "eve"));
    let update_after_removal = update
        .execute_prepared(prepared, &[], CancellationSignal::default(), |_| {})
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

#[cfg(test)]
#[tokio::test]
async fn native_scope_version_election_installs_direct_symlinks_over_library_links() {
    let temp = tempfile::tempdir().expect("direct symlink install fixture");
    run_native_scope_version_election_workflow_at(
        temp.path(),
        &["minimax-code"],
        InstallMode::Symlink,
    )
    .await
    .expect("direct symlink install over applied library links");
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

    run_native_scope_version_election_workflow_at(&logical_root, &[], InstallMode::Copy)
        .await
        .expect("native Scope version-election workflow through a path alias");
}

#[cfg(all(test, unix))]
#[tokio::test]
async fn native_scope_version_election_installs_direct_symlinks_across_path_aliases() {
    use std::os::unix::fs::symlink;

    let temp = tempfile::tempdir().expect("direct symlink path alias fixture");
    let physical_root = temp.path().join("physical");
    let logical_root = temp.path().join("logical");
    fs::create_dir(&physical_root).expect("physical root");
    symlink(&physical_root, &logical_root).expect("logical root alias");

    run_native_scope_version_election_workflow_at(
        &logical_root,
        &["minimax-code"],
        InstallMode::Symlink,
    )
    .await
    .expect("direct symlink install over library links through a path alias");
}

#[cfg(test)]
async fn run_native_scope_version_election_workflow() -> Result<(), AppError> {
    let temp = tempfile::tempdir()?;
    run_native_scope_version_election_workflow_at(temp.path(), &[], InstallMode::Copy).await
}

#[cfg(test)]
async fn run_native_scope_version_election_workflow_at(
    root: &Path,
    direct_agent_ids: &[&str],
    direct_install_mode: InstallMode,
) -> Result<(), AppError> {
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
    let repository: Arc<dyn LibraryApplicationBackend> = Arc::new(MemoryApplicationRegistry {
        record: Mutex::new(LibraryApplicationRecord::empty()),
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
        direct_agent_ids,
        direct_install_mode.clone(),
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
    } = install.preview(&install_request).await?
    else {
        panic!("expected ready direct install preview");
    };
    let installed = install
        .execute(
            &install_request,
            install_preview.token,
            CancellationSignal::default(),
        )
        .await?;
    assert_succeeded(&installed.units);
    assert_payload_tree(&canonical, "direct")?;
    assert_resolves_to(&agent_a_entry, &first_member)?;
    if direct_agent_ids.contains(&agent_b.as_str()) {
        assert_payload_tree(&agent_b_entry, "direct")?;
        if direct_install_mode == InstallMode::Symlink {
            assert_resolves_to(&agent_b_entry, &canonical)?;
        }
    } else {
        assert_resolves_to(&agent_b_entry, &first_member)?;
    }

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
        retired_skills: Vec::new(),
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
        .preview(&request)
        .await
        .expect("preview direct download")
    else {
        panic!("expected ready direct-download preview");
    };
    assert!(!stale_preview.skills[0].blocking_reasons.is_empty());
    fs::remove_dir_all(project_path.join(".agents/skills/alpha")).unwrap();
    assert!(matches!(
        install
            .execute(&request, stale_preview.token, CancellationSignal::default(),)
            .await,
        Err(AppError::StaleContext)
    ));

    let InstallPreviewOutcome::Ready { preview } = install
        .preview(&request)
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
        .execute(&request, preview.token, CancellationSignal::default())
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
        BoxFuture, MutationCoordinator, PreparedEntryTestDriver,
    };
    use crate::application::mutation::executor::MutationPlanExecutor;
    use crate::application::mutation::plan::{ExecutionUnit, MutationPlan};
    use crate::application::mutation::result::{MutationUnitStatus, MutationWarning};
    use crate::application::payload_session::{PayloadSessionManager, PinnedPayloadLease};
    use crate::application::resources::{ResourceService, SkillIdentity};
    use crate::application::source_acquisition::{
        AcquireSelectedPayloadsRequest, SelectedPayloadAcquisitionService,
    };
    use crate::application::source_evidence::{
        EvidenceDetectionRequest, EvidenceFuture, SourceEvidenceCoordinator, SourceEvidenceDetector,
    };
    use crate::application::source_evidence_provider::RuntimeSourceEvidenceDetector;
    use crate::application::source_snapshot_reuse::SourceSnapshotReuseIndex;
    use crate::application::update::{
        PreparedUpdate, UpdateCheckMode, UpdateCheckRequest, UpdateCheckSelection, UpdateOutcome,
        UpdateRequest, UpdateResponse, UpdateSourceStatus, UpdateWarningCode,
    };
    use crate::application::update_check::UpdateCheckService;
    use crate::application::update_records::InstalledUpdateRecordProvider;
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
    use crate::runtime::resource_service::{RuntimeResourceContextSource, RuntimeResourceReader};
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

    impl PreparedEntryTestDriver for StageFailureEntryExecutor {
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
                MutationCoordinator::from_phases(
                    entries,
                    RuntimeLockCommitter::new(),
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
            let evidence = SourceEvidenceCoordinator::new(detector);
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

        async fn install_with_cli(
            &self,
            skills: &[&str],
            agents: &[&str],
            copy: bool,
            subagents: &[&str],
        ) {
            let cli = Path::new(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .expect("repository root")
                .join("node_modules/skills/bin/cli.mjs");
            assert!(
                cli.is_file(),
                "run pnpm install --frozen-lockfile before CLI tests"
            );
            let mut node: tokio::process::Command =
                crate::background_process::std_command("node").into();
            let resolved_node = tokio::time::timeout(
                std::time::Duration::from_secs(10),
                node.args(["-p", "process.execPath"])
                    .kill_on_drop(true)
                    .output(),
            )
            .await
            .expect("Node resolution timed out")
            .expect("resolve the Node executable before isolating home");
            assert!(resolved_node.status.success());
            let node = String::from_utf8(resolved_node.stdout).expect("Node executable path");
            let home = self._root.path().join("home");
            let git_config = home.join("cli-git-config");
            fs::write(
                &git_config,
                format!(
                    "[url \"{}\"]\n\tinsteadOf = {}\n[protocol \"file\"]\n\tallow = always\n",
                    self.remote.local_source(),
                    self.remote.source(),
                ),
            )
            .expect("write isolated CLI Git URL mapping");
            let mut command: tokio::process::Command =
                crate::background_process::std_command(node.trim()).into();
            command
                .arg(cli)
                .arg("add")
                .arg(self.remote.source())
                .arg("--skill")
                .args(skills)
                .arg("--agent")
                .args(agents)
                .arg("--yes")
                .current_dir(&self.project_path)
                .env("HOME", &home)
                .env("USERPROFILE", &home)
                .env("XDG_STATE_HOME", home.join(".local").join("state"))
                .env("GIT_CONFIG_GLOBAL", &git_config)
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .env("DISABLE_TELEMETRY", "1")
                .env("DO_NOT_TRACK", "1")
                .env("NO_COLOR", "1")
                .kill_on_drop(true);
            if copy {
                command.arg("--copy");
            }
            if !subagents.is_empty() {
                command.arg("--subagent").args(subagents);
            }
            let output = tokio::time::timeout(std::time::Duration::from_secs(30), command.output())
                .await
                .expect("CLI installation timed out")
                .expect("run pinned Skills CLI");
            assert!(
                output.status.success(),
                "CLI installation failed: {}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr),
            );
        }

        fn context(&self) -> SkillLocationRef {
            project_context("source")
        }

        async fn read_installed(
            &self,
            document: Option<&crate::core::lossless_lock::LosslessLockDocument>,
        ) -> Result<crate::application::skill_read::ListSkillsResult, AppError> {
            let facts = ScopePlanningSnapshotSource::snapshot(&self.facts, &self.context()).await?;
            let eve_targets = facts
                .eve_targets
                .into_iter()
                .map(|target| crate::models::SkillInstallTargetInfo {
                    target_id: target.target_id,
                    agent: target.agent,
                    display_name: target.display_name,
                    subagent: target.subagent,
                    path: target.path,
                })
                .collect::<Vec<_>>();
            let mut plan = crate::application::skill_read::build_skill_read_plan(
                &facts.resolved_context,
                &facts.agent_runtime,
                &eve_targets,
            )?;
            let bytes = document
                .map(|document| document.to_pretty_bytes())
                .transpose()?;
            plan.set_project_lock(bytes.as_deref());
            let inspector = crate::environment::native::inspection::NativeInspector::new(
                EnvironmentRef::Native,
            );
            let snapshot = crate::environment::inspection::FilesystemInspector::inspect(
                &inspector,
                &plan.read_plan,
            )
            .await?;
            crate::application::skill_read::project_direct_skill_snapshot(
                &plan,
                snapshot,
                &facts.agent_runtime,
                update_library_repository(self._root.path()).as_ref(),
                &self.targets,
                &inspector,
            )
            .await
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
                    update_library_repository(self._root.path()),
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
            InstalledUpdateRecordProvider<RuntimePlanningFactSource, RuntimeTargetFactResolver>,
        > {
            UpdateCheckService::new(
                InstalledUpdateRecordProvider::new(
                    self.facts.clone(),
                    self.targets.clone(),
                    Arc::new(
                        crate::runtime::skill_libraries::RuntimeSkillLibraryRepository::new(
                            self._root.path().join("library-records"),
                            self.environments.clone(),
                            Arc::new(crate::core::projects::ProjectMigrationRegistry::new(
                                crate::core::projects::ProjectMigrationState::NotNeeded,
                            )),
                        ),
                    ),
                ),
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
                    .preview(&request)
                    .await
                    .expect("preview lifecycle install");
                let InstallPreviewOutcome::Ready { preview } = preview_outcome else {
                    panic!("expected ready lifecycle install preview");
                };
                let installed = install
                    .execute(&request, preview.token, CancellationSignal::default())
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

        async fn prepare<const N: usize>(&self, skill_names: [&str; N]) -> PreparedUpdate {
            let before = self.clone_count();
            let preview = self
                .update_service()
                .prepare(
                    &UpdateRequest {
                        context: self.context(),
                        skill_names: skill_names.into_iter().map(ToString::to_string).collect(),
                    },
                    CancellationSignal::default(),
                )
                .await
                .expect("prepare lifecycle update");
            assert_eq!(self.clone_count(), before);
            self.preview_clone_count.store(before, Ordering::SeqCst);
            preview
        }

        async fn cancel(&self, preview: PreparedUpdate) {
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
            let prepared = service
                .prepare(&request, CancellationSignal::default())
                .await
                .expect("prepare update confirmation");
            assert_eq!(prepared.preview.skills[0].clean_copy_count, 2);
            assert_eq!(
                prepared.preview.skills[0].overwrite_private_entries.len(),
                1
            );
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
            let prepared = service
                .prepare(&request, CancellationSignal::default())
                .await
                .expect("source partial preparation");
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
                    update_library_repository(self._root.path()),
                    fixed_time,
                ),
                RuntimeSkillSourceModule::with_git_transport(
                    self.payloads.clone(),
                    self.snapshots.clone(),
                    self.evidence.clone(),
                    self.git_transport.clone(),
                ),
                StageFailurePlanExecutor {
                    facts: self.facts.clone(),
                    recovery_root: self.recovery_root.clone(),
                    private_root: self.project_path.join(".custom/skills"),
                },
            );
            let prepared = service
                .prepare(&request, CancellationSignal::default())
                .await
                .expect("staging partial preparation");
            let alpha = prepared
                .preview
                .skills
                .iter()
                .find(|skill| skill.skill_name == "alpha")
                .expect("alpha update preview");
            let beta = prepared
                .preview
                .skills
                .iter()
                .find(|skill| skill.skill_name == "beta")
                .expect("beta update preview");
            assert_eq!(alpha.overwrite_private_entries.len(), 2);
            assert_eq!(beta.clean_copy_count, 3);
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
    #[ignore = "requires Node and skills@1.5.23 from pnpm install; run this filter with --ignored"]
    async fn native_update_private_only_cli_installation_in_place() {
        assert_private_cli_update(&["codebuddy"]).await;
        assert_private_cli_update(&["codebuddy", "minimax-code"]).await;
    }

    async fn assert_private_cli_update(agents: &[&str]) {
        let fixture = UpdateLifecycleFixture::new(["alpha", "beta"]).await;
        fixture
            .install_with_cli(&["alpha"], agents, agents.len() > 1, &[])
            .await;
        let canonical = fixture
            .project_path
            .join(".agents")
            .join("skills")
            .join("alpha");
        let private = fixture
            .project_path
            .join(".codebuddy")
            .join("skills")
            .join("alpha");
        assert!(!canonical.exists());
        assert!(private.join("SKILL.md").is_file());
        let facts = ScopePlanningSnapshotSource::snapshot(&fixture.facts, &fixture.context())
            .await
            .unwrap();
        let read_plan = crate::application::skill_read::build_skill_read_plan(
            &facts.resolved_context,
            &facts.agent_runtime,
            &[],
        )
        .unwrap();
        let inspector =
            crate::environment::native::inspection::NativeInspector::new(EnvironmentRef::Native);
        let snapshot = crate::environment::inspection::FilesystemInspector::inspect(
            &inspector,
            &read_plan.read_plan,
        )
        .await
        .unwrap();
        let listed = crate::application::skill_read::project_direct_skill_snapshot(
            &read_plan,
            snapshot,
            &facts.agent_runtime,
            update_library_repository(fixture._root.path()).as_ref(),
            &fixture.targets,
            &inspector,
        )
        .await
        .unwrap();
        assert_eq!(listed.skills.len(), 1);
        assert_eq!(
            fs::canonicalize(&listed.skills[0].canonical_path).unwrap(),
            fs::canonicalize(&private).unwrap()
        );
        let identity = SkillIdentity {
            context: fixture.context(),
            skill_name: "alpha".to_string(),
        };
        let check = fixture
            .check_service()
            .check(&UpdateCheckRequest {
                context: fixture.context(),
                mode: UpdateCheckMode::Force,
                selection: UpdateCheckSelection::Skills(vec![identity.clone()]),
            })
            .await
            .expect("check a CLI private-only installation");
        assert_eq!(check.skills.len(), 1, "{check:#?}");
        assert!(check.skills[0].capability.can_run_update);
        let resources = ResourceService::new(
            RuntimeResourceContextSource::new(
                fixture.facts.clone(),
                fixture.targets.clone(),
                update_library_repository(fixture._root.path()),
            ),
            fixture.targets.clone(),
            crate::environment::opener::SystemResourceOpener,
            RuntimeResourceReader::new(fixture.environments.clone()),
        );
        resources
            .read_skill(&identity)
            .await
            .expect("read the actual private installation");

        fixture.remote.publish_change("alpha");
        let service = fixture.update_service();
        let prepared = service
            .prepare(
                &UpdateRequest {
                    context: fixture.context(),
                    skill_names: vec!["alpha".to_string()],
                },
                CancellationSignal::default(),
            )
            .await
            .expect("prepare a CLI private-only update");
        let base = prepared
            .preview
            .path_base
            .as_ref()
            .expect("preview path base");
        let expected = crate::environment::types::display_locator(
            &crate::environment::types::ResourceLocator {
                environment: EnvironmentRef::Native,
                native_path: fs::canonicalize(&fixture.project_path)
                    .unwrap()
                    .to_string_lossy()
                    .into_owned(),
            },
        );
        assert_eq!(base.physical_root, Some(expected));
        assert!(
            prepared.preview.blocked.is_empty(),
            "{:#?}",
            prepared.preview
        );
        assert_eq!(prepared.preview.skills.len(), 1);
        assert_eq!(prepared.preview.skills[0].targets.len(), agents.len());
        let prepared_clone_count = fixture.clone_count();
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
            .expect("execute the prepared private-only update");
        assert_eq!(response.outcome, UpdateOutcome::Succeeded, "{response:#?}");
        assert_eq!(fixture.clone_count(), prepared_clone_count);
        assert_eq!(
            fs::read(private.join("references").join("guide.md")).expect("updated private content"),
            fs::read(
                fixture
                    .remote
                    .work
                    .join("skills")
                    .join("alpha")
                    .join("references")
                    .join("guide.md")
            )
            .expect("upstream content"),
        );
        assert!(!canonical.exists());
        for unused in [".minimax", ".custom"] {
            if unused == ".minimax" && agents.contains(&"minimax-code") {
                assert_eq!(
                    fs::read(
                        fixture
                            .project_path
                            .join(".minimax/skills/alpha/references/guide.md")
                    )
                    .unwrap(),
                    fs::read(private.join("references/guide.md")).unwrap()
                );
                continue;
            }
            assert!(!fixture
                .project_path
                .join(unused)
                .join("skills")
                .join("alpha")
                .exists());
        }
        let lock =
            read_json(&fixture.project_path.join("skills-lock.json")).expect("updated CLI lock");
        assert_eq!(
            lock["skills"]["alpha"]["computedHash"],
            fixture.remote.computed_hash("alpha")
        );
        assert_no_staging_leaks(fixture._root.path()).expect("no staging leaks");
    }

    #[tokio::test]
    #[ignore = "requires Node and skills@1.5.23 from pnpm install; run this filter with --ignored"]
    async fn native_eve_update_restores_recorded_missing_installation() {
        let fixture = UpdateLifecycleFixture::new(["alpha", "beta"]).await;
        let research = fixture
            .project_path
            .join("agent/subagents/research/skills/alpha");
        let writer = fixture
            .project_path
            .join("agent/subagents/writer/skills/alpha");
        for name in ["research", "writer"] {
            fs::create_dir_all(fixture.project_path.join("agent/subagents").join(name)).unwrap();
        }
        write_json(
            &fixture.project_path.join("package.json"),
            &json!({"dependencies":{"eve":"*"}}),
        )
        .unwrap();
        fixture
            .install_with_cli(&["alpha"], &["eve"], false, &["research", "writer"])
            .await;
        assert!(!fixture.project_path.join(".agents/skills/alpha").exists());
        assert!(!fs::read_to_string(research.join("SKILL.md"))
            .unwrap()
            .contains("name:"));
        let facts = ScopePlanningSnapshotSource::snapshot(&fixture.facts, &fixture.context())
            .await
            .unwrap();
        let listed = fixture
            .read_installed(Some(&facts.lock_document))
            .await
            .unwrap();
        assert_eq!(listed.skills.len(), 1);
        assert_eq!(listed.skills[0].name, "alpha");
        assert_eq!(
            listed.skills[0].associated_agents,
            vec![AgentId::parse("eve").unwrap()]
        );
        assert!(fixture
            .read_installed(None)
            .await
            .unwrap()
            .skills
            .is_empty());
        let mut ambiguous = facts.lock_document.clone().into_value();
        ambiguous["skills"]["ALPHA"] = ambiguous["skills"]["alpha"].clone();
        let ambiguous = crate::core::lossless_lock::LosslessLockDocument::parse(
            &serde_json::to_vec(&ambiguous).unwrap(),
        )
        .unwrap();
        assert!(
            matches!(fixture.read_installed(Some(&ambiguous)).await, Err(AppError::CapabilityUnavailable { capability, .. }) if capability == "installedSkillSourceAmbiguous")
        );

        fs::remove_dir_all(&writer).unwrap();
        fixture.remote.publish_change("alpha");
        let service = fixture.update_service();
        let request = UpdateRequest {
            context: fixture.context(),
            skill_names: vec!["alpha".into()],
        };
        let prepared = service
            .prepare(&request, CancellationSignal::default())
            .await
            .unwrap();
        assert!(
            prepared.preview.blocked.is_empty(),
            "{:#?}",
            prepared.preview
        );
        assert_eq!(
            prepared.preview.skills[0]
                .targets
                .iter()
                .filter(|t| t.restoring)
                .count(),
            1
        );
        drop(prepared);
        assert!(!writer.exists());

        let prepared = service
            .prepare(&request, CancellationSignal::default())
            .await
            .unwrap();
        let original = build_skill_payload(&research).unwrap();
        fs::create_dir_all(&writer).unwrap();
        fs::write(writer.join("SKILL.md"), b"newly created content").unwrap();
        let stale = service
            .execute_prepared(prepared, &[], CancellationSignal::default(), |_| {})
            .await
            .unwrap();
        assert_ne!(stale.outcome, UpdateOutcome::Succeeded);
        assert_eq!(build_skill_payload(&research).unwrap(), original);
        assert_eq!(
            fs::read(writer.join("SKILL.md")).unwrap(),
            b"newly created content"
        );
        fs::remove_dir_all(&writer).unwrap();

        let prepared = service
            .prepare(&request, CancellationSignal::default())
            .await
            .unwrap();
        let selected_copies = prepared
            .preview
            .skills
            .iter()
            .flat_map(|skill| &skill.targets)
            .filter_map(|target| target.selectable_entry_id.clone())
            .collect::<Vec<_>>();
        let result = service
            .execute_prepared(
                prepared,
                &selected_copies,
                CancellationSignal::default(),
                |_| {},
            )
            .await
            .unwrap();
        assert_eq!(result.outcome, UpdateOutcome::Succeeded, "{result:#?}");
        for path in [&research, &writer] {
            assert!(!fs::read_to_string(path.join("SKILL.md"))
                .unwrap()
                .contains("name:"));
            assert_eq!(
                fs::read(path.join("references/guide.md")).unwrap(),
                fs::read(fixture.remote.work.join("skills/alpha/references/guide.md")).unwrap()
            );
        }
        let lock_path = fixture.project_path.join("skills-lock.json");
        let lock = fs::read(&lock_path).unwrap();
        assert_eq!(
            serde_json::from_slice::<Value>(&lock).unwrap()["skills"]["alpha"]["subagents"],
            json!(["research", "writer"])
        );
        let original_research = fs::read(research.join("SKILL.md")).unwrap();
        let mut edited = original_research.clone();
        edited.extend_from_slice(b"\nlocal Eve edit\n");
        fs::write(research.join("SKILL.md"), &edited).unwrap();
        let prepared = service
            .prepare(&request, CancellationSignal::default())
            .await
            .unwrap();
        assert!(prepared.preview.skills[0].targets.is_empty());
        assert_eq!(
            prepared.preview.skills[0].overwrite_private_entries.len(),
            2
        );
        let preserved = service
            .execute_prepared(prepared, &[], CancellationSignal::default(), |_| {})
            .await
            .unwrap();
        assert!(preserved.skills[0].mutation.is_none());
        assert_eq!(fs::read(research.join("SKILL.md")).unwrap(), edited);
        assert_eq!(fs::read(&lock_path).unwrap(), lock);
        fs::write(research.join("SKILL.md"), original_research).unwrap();

        let prepared = service
            .prepare(&request, CancellationSignal::default())
            .await
            .unwrap();
        fs::write(writer.with_extension("md"), b"new single-file target").unwrap();
        let stale = service
            .execute_prepared(prepared, &[], CancellationSignal::default(), |_| {})
            .await
            .unwrap();
        assert_ne!(stale.outcome, UpdateOutcome::Succeeded);
        assert_eq!(fs::read(&lock_path).unwrap(), lock);
        assert!(
            matches!(fixture.read_installed(Some(&facts.lock_document)).await, Err(AppError::CapabilityUnavailable { capability, .. }) if capability == "installedSkillSourceAmbiguous")
        );
        fs::remove_file(writer.with_extension("md")).unwrap();
        fs::remove_dir_all(&writer).unwrap();
        let single = writer.with_extension("md");
        fs::write(&single, b"preserve single-file Eve content").unwrap();
        let prepared = service
            .prepare(&request, CancellationSignal::default())
            .await
            .unwrap();
        assert_eq!(prepared.preview.blocked.len(), 1);
        assert!(prepared.preview.skills.is_empty());
        assert_eq!(
            fs::read(&single).unwrap(),
            b"preserve single-file Eve content"
        );
        assert_eq!(fs::read(&lock_path).unwrap(), lock);
        let listed = fixture
            .read_installed(Some(&facts.lock_document))
            .await
            .unwrap();
        assert_eq!(listed.skills.len(), 1);
        assert!(
            matches!(&listed.skills[0].maintenance_error, Some(AppError::CapabilityUnavailable { capability, .. }) if capability == "eveSingleFile")
        );
        drop(prepared);

        fs::remove_file(single).unwrap();
        fs::remove_dir_all(writer.parent().unwrap().parent().unwrap()).unwrap();
        let prepared = service
            .prepare(&request, CancellationSignal::default())
            .await
            .unwrap();
        assert_eq!(prepared.preview.blocked.len(), 1);
        assert!(!writer.parent().unwrap().parent().unwrap().exists());
        fs::remove_dir_all(&research).unwrap();
        assert!(fixture
            .read_installed(Some(&facts.lock_document))
            .await
            .unwrap()
            .skills
            .is_empty());
        let checked = fixture
            .check_service()
            .check(&UpdateCheckRequest {
                context: fixture.context(),
                mode: UpdateCheckMode::Force,
                selection: UpdateCheckSelection::Skills(vec![SkillIdentity {
                    context: fixture.context(),
                    skill_name: "alpha".into(),
                }]),
            })
            .await
            .unwrap();
        assert!(checked.skills.is_empty());
    }

    #[tokio::test]
    async fn native_copy_project_identity_resolves_directory_links_and_rejects_missing_roots() {
        use crate::application::copy::CopyProjectComparator;
        use crate::environment::runtime::PhysicalIdentityComparison;

        let fixture = UpdateLifecycleFixture::new(["alpha", "beta"]).await;
        let facts = ScopePlanningSnapshotSource::snapshot(&fixture.facts, &fixture.context())
            .await
            .unwrap();
        let comparator = RuntimeCopyProjectComparator::new(fixture.environments.clone());
        let source = comparator.capture_source(&facts).await.unwrap();
        let alias = fixture._root.path().join("project-alias");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&fixture.project_path, &alias).unwrap();
        #[cfg(windows)]
        junction::create(&fixture.project_path, &alias).unwrap();
        let nested = fixture.project_path.join("nested");
        let sibling = fixture._root.path().join("project-sibling");
        fs::create_dir_all(&nested).unwrap();
        fs::create_dir_all(&sibling).unwrap();
        for (path, expected) in [
            (
                fixture.project_path.clone(),
                PhysicalIdentityComparison::Same,
            ),
            (alias.clone(), PhysicalIdentityComparison::Same),
            (alias.join("nested"), PhysicalIdentityComparison::Same),
            (nested, PhysicalIdentityComparison::Same),
            (
                fixture._root.path().to_path_buf(),
                PhysicalIdentityComparison::Same,
            ),
            (sibling, PhysicalIdentityComparison::Different),
        ] {
            let mut target = facts.clone();
            target
                .resolved_context
                .project
                .as_mut()
                .unwrap()
                .native_path = path.to_str().unwrap().into();
            assert_eq!(
                comparator
                    .compare(&source, &target)
                    .await
                    .unwrap()
                    .physical_identity,
                expected,
                "{path:?}"
            );
        }
        let mut missing = facts.clone();
        missing
            .resolved_context
            .project
            .as_mut()
            .unwrap()
            .native_path = fixture
            ._root
            .path()
            .join("missing-project")
            .to_str()
            .unwrap()
            .into();
        assert!(comparator.capture_source(&missing).await.is_err());
        assert!(comparator.compare(&source, &missing).await.is_err());
    }

    #[tokio::test]
    async fn native_private_only_copy_manage_and_remove_preserve_source_layout() {
        let fixture = UpdateLifecycleFixture::new(["alpha", "beta"]).await;
        fixture.install().await;
        let context = fixture.context();
        let standard = fixture.project_path.join(".agents/skills/alpha");
        let source = fixture.project_path.join(".codebuddy/skills/alpha");
        let minimax = fixture.project_path.join(".minimax/skills/alpha");
        for path in [
            &standard,
            &minimax,
            &fixture.project_path.join(".custom/skills/alpha"),
        ] {
            fs::remove_dir_all(path).unwrap();
        }
        let markdown = fs::read(fixture.remote.work.join("skills/alpha/SKILL.md")).unwrap();
        fs::write(source.join("SKILL.md"), &markdown).unwrap();
        let destination = fixture._root.path().join("copy-target");
        fs::create_dir_all(&destination).unwrap();
        write_json(&fixture._root.path().join("state/projects.json"), &json!({"schemaVersion":1, "projects":[
            project("source", &fixture.project_path, "Source"), project("copy-target", &destination, "Target")
        ]})).unwrap();
        let copy = CopyService::new(
            fixture.facts.clone(),
            fixture.targets.clone(),
            fixture.payloads.clone(),
            InstalledSkillPayloadAcquirer::new(
                fixture.payloads.clone(),
                fixture.environments.clone(),
            ),
            fixture.executor(),
            RuntimeCopyProjectComparator::new(fixture.environments.clone()),
            Arc::new(EmptyLibraryCandidateSource),
        );
        let selection = copy.selection(&context, "alpha").await.unwrap().selection;
        let mut copy_request = CopyRequest {
            skill_name: "alpha".into(),
            source: context.clone(),
            target_environment: EnvironmentRef::Native,
            target_project_ids: vec!["copy-target".into()],
            agent_selection: crate::application::agent_selection::AgentSelectionSubmission {
                revision: selection.revision,
                selected_option_ids: selection.baseline_selected_option_ids,
                requested_mode: InstallMode::Copy,
            },
        };
        let CopyPreviewOutcome::Ready { preview } = copy.preview(&copy_request).await.unwrap()
        else {
            panic!("copy preview");
        };
        let captured = fixture
            .payloads
            .copy_source_snapshot(&preview.payload)
            .unwrap();
        assert_eq!(
            fs::canonicalize(&captured.standard_identity.destination.native_path).unwrap(),
            fs::canonicalize(&source).unwrap()
        );
        let mut changed = markdown.clone();
        changed.extend_from_slice(b"\nchanged after preview\n");
        fs::write(source.join("SKILL.md"), &changed).unwrap();
        assert!(copy
            .execute(
                &CopyExecutionRequest {
                    request: copy_request.clone(),
                    token: preview.token,
                    payload: preview.payload
                },
                CancellationSignal::default()
            )
            .await
            .is_err());
        assert!(!destination.join(".agents/skills/alpha").exists());
        fs::write(source.join("SKILL.md"), &markdown).unwrap();
        let CopyPreviewOutcome::Ready { preview } = copy.preview(&copy_request).await.unwrap()
        else {
            panic!("copy preview");
        };
        let copied = copy
            .execute(
                &CopyExecutionRequest {
                    request: copy_request.clone(),
                    token: preview.token,
                    payload: preview.payload,
                },
                CancellationSignal::default(),
            )
            .await
            .unwrap();
        assert_succeeded(&copied.units);
        assert_eq!(
            fs::read(destination.join(".agents/skills/alpha/SKILL.md")).unwrap(),
            markdown
        );
        assert!(!standard.exists());

        let manage = ManageAgentsService::new(
            fixture.facts.clone(),
            ScopeSkillPlacementResolver::new(fixture.targets.clone()),
            fixture.targets.clone(),
            fixture.payloads.clone(),
            InstalledSkillPayloadAcquirer::new(
                fixture.payloads.clone(),
                fixture.environments.clone(),
            ),
            fixture.executor(),
            Arc::new(EmptyLibraryCandidateSource),
        );
        let snapshot = manage.selection(&context, "alpha").await.unwrap();
        let mut selection = manage_submission(
            &snapshot,
            |option| {
                option
                    .agent_ids
                    .iter()
                    .any(|id| matches!(id.as_str(), "codebuddy" | "minimax-code"))
            },
            InstallMode::Symlink,
        );
        let mut request = ManageAgentsPreviewRequest {
            context: context.clone(),
            skill_name: "alpha".into(),
            agent_selection: selection.clone(),
        };
        assert!(
            matches!(manage.preview(&request).await, Err(AppError::CapabilityUnavailable { capability, .. }) if capability == "sharedSkillLinkSource")
        );
        selection.requested_mode = InstallMode::Copy;
        request.agent_selection = selection.clone();
        let preview = ready_manage_preview(manage.preview(&request).await.unwrap());
        let added = manage
            .execute(
                &ManageAgentsRequest {
                    token: preview.token,
                    context: context.clone(),
                    skill_name: "alpha".into(),
                    agent_selection: selection,
                    confirm_entity_directories: false,
                    original_payload: preview.original_payload,
                },
                CancellationSignal::default(),
            )
            .await
            .unwrap();
        assert_succeeded(&added.units);
        assert_eq!(fs::read(minimax.join("SKILL.md")).unwrap(), markdown);
        assert!(!standard.exists());

        let selection = copy.selection(&context, "alpha").await.unwrap().selection;
        copy_request.agent_selection.revision = selection.revision;
        copy_request.agent_selection.selected_option_ids = selection.baseline_selected_option_ids;
        let CopyPreviewOutcome::Ready { preview } = copy.preview(&copy_request).await.unwrap()
        else {
            panic!("copy preview");
        };
        let hidden = source.with_file_name("alpha-moved");
        fs::rename(&source, &hidden).unwrap();
        assert!(copy
            .execute(
                &CopyExecutionRequest {
                    request: copy_request.clone(),
                    token: preview.token,
                    payload: preview.payload
                },
                CancellationSignal::default()
            )
            .await
            .is_err());
        assert_eq!(
            fs::read(destination.join(".agents/skills/alpha/SKILL.md")).unwrap(),
            markdown
        );
        fs::rename(&hidden, &source).unwrap();

        fs::write(minimax.join("SKILL.md"), &changed).unwrap();
        let selection = copy.selection(&context, "alpha").await.unwrap().selection;
        copy_request.agent_selection.revision = selection.revision;
        copy_request.agent_selection.selected_option_ids = selection.baseline_selected_option_ids;
        assert!(
            matches!(copy.preview(&copy_request).await, Err(AppError::CapabilityUnavailable { capability, .. }) if capability == "installedSkillSourceAmbiguous")
        );
        let snapshot = manage.selection(&context, "alpha").await.unwrap();
        let add = manage_submission(&snapshot, |_| true, InstallMode::Copy);
        assert!(
            matches!(manage.preview(&ManageAgentsPreviewRequest { context: context.clone(), skill_name: "alpha".into(), agent_selection: add }).await, Err(AppError::CapabilityUnavailable { capability, .. }) if capability == "installedSkillSourceAmbiguous")
        );
        let selection = manage_submission(
            &snapshot,
            |option| option.agent_ids.iter().any(|id| id.as_str() == "codebuddy"),
            InstallMode::Copy,
        );
        let preview = ready_manage_preview(
            manage
                .preview(&ManageAgentsPreviewRequest {
                    context: context.clone(),
                    skill_name: "alpha".into(),
                    agent_selection: selection.clone(),
                })
                .await
                .unwrap(),
        );
        assert!(preview.original_payload.is_none());
        let removed = manage
            .execute(
                &ManageAgentsRequest {
                    token: preview.token,
                    context: context.clone(),
                    skill_name: "alpha".into(),
                    agent_selection: selection,
                    confirm_entity_directories: true,
                    original_payload: None,
                },
                CancellationSignal::default(),
            )
            .await
            .unwrap();
        assert_succeeded(&removed.units);
        assert!(!minimax.exists());
        assert_eq!(fs::read(source.join("SKILL.md")).unwrap(), markdown);
        assert!(!standard.exists());

        let remove = RemoveService::new(
            fixture.facts.clone(),
            fixture.targets.clone(),
            fixture.executor(),
            Arc::new(EmptyLibraryCandidateSource),
        );
        let preview = remove.preview(&context, "alpha").await.unwrap();
        assert_eq!(
            serde_json::to_value(&preview).unwrap().get("standardPath"),
            Some(&serde_json::Value::Null),
        );
        assert_eq!(preview.physical_entries.len(), 1);
        let result = remove
            .execute(
                &RemoveRequest {
                    token: preview.token,
                    context,
                    skill_name: "alpha".into(),
                    intent: RemoveIntent::FullSkill,
                },
                CancellationSignal::default(),
            )
            .await
            .unwrap();
        assert_succeeded(&result.units);
        assert!(!source.exists());
        assert!(!standard.exists());
        let lock = read_json(&fixture.project_path.join("skills-lock.json")).unwrap();
        assert!(lock["skills"]["alpha"].is_null());
        assert!(lock["skills"]["beta"].is_object());
        assert!(fixture
            .project_path
            .join(".agents/skills/beta/SKILL.md")
            .exists());
        assert_eq!(fixture.clone_count(), 1);
    }

    #[tokio::test]
    async fn native_update_private_copies_respects_selection_and_new_standard_directory() {
        let fixture = UpdateLifecycleFixture::new(["alpha", "beta"]).await;
        fixture.install().await;
        let standard = fixture.project_path.join(".agents/skills/alpha");
        fs::remove_dir_all(&standard).unwrap();
        let private = [".codebuddy", ".minimax", ".custom"]
            .map(|root| fixture.project_path.join(root).join("skills/alpha"));
        let before = private
            .iter()
            .map(|path| build_skill_payload(path).unwrap())
            .collect::<Vec<_>>();
        let lock_path = fixture.project_path.join("skills-lock.json");
        let lock_before = fs::read(&lock_path).unwrap();
        fixture.remote.publish_change("alpha");
        fixture.remote.publish_change("beta");
        let service = fixture.update_service();
        let request = UpdateRequest {
            context: fixture.context(),
            skill_names: vec!["alpha".into()],
        };
        let prepared = service
            .prepare(&request, CancellationSignal::default())
            .await
            .unwrap();
        assert!(prepared.preview.skills[0].targets.is_empty());
        assert_eq!(
            prepared.preview.skills[0].overwrite_private_entries.len(),
            3
        );
        let kept = service
            .execute_prepared(prepared, &[], CancellationSignal::default(), |_| {})
            .await
            .unwrap();
        assert!(kept.skills[0].mutation.is_none());
        assert_eq!(fs::read(&lock_path).unwrap(), lock_before);
        for (path, payload) in private.iter().zip(&before) {
            assert_eq!(&build_skill_payload(path).unwrap(), payload);
        }

        let batch = UpdateRequest {
            context: fixture.context(),
            skill_names: vec!["alpha".into(), "beta".into()],
        };
        let prepared = service
            .prepare(&batch, CancellationSignal::default())
            .await
            .unwrap();
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
        assert_eq!(response.outcome, UpdateOutcome::Partial);
        assert!(response
            .skills
            .iter()
            .find(|s| s.skill_identity.skill_name == "alpha")
            .unwrap()
            .mutation
            .is_none());
        assert_eq!(
            read_json(&lock_path).unwrap()["skills"]["alpha"],
            serde_json::from_slice::<Value>(&lock_before).unwrap()["skills"]["alpha"]
        );

        let prepared = service
            .prepare(&request, CancellationSignal::default())
            .await
            .unwrap();
        let selected = prepared.preview.skills[0]
            .overwrite_private_entries
            .iter()
            .find(|entry| {
                entry
                    .readers
                    .iter()
                    .any(|r| r.agent_id.as_str() == "custom-test")
            })
            .unwrap()
            .entry_id
            .clone();
        let response = service
            .execute_prepared(prepared, &[selected], CancellationSignal::default(), |_| {})
            .await
            .unwrap();
        assert_eq!(response.outcome, UpdateOutcome::Succeeded);
        assert_eq!(build_skill_payload(&private[0]).unwrap(), before[0]);
        assert_eq!(build_skill_payload(&private[1]).unwrap(), before[1]);
        assert_eq!(
            fs::read(private[2].join("references/guide.md")).unwrap(),
            fs::read(fixture.remote.work.join("skills/alpha/references/guide.md")).unwrap()
        );
        assert!(!standard.exists());

        let prepared = service
            .prepare(&request, CancellationSignal::default())
            .await
            .unwrap();
        let selected = prepared.preview.skills[0].overwrite_private_entries[0]
            .entry_id
            .clone();
        let contents = private
            .iter()
            .map(|path| build_skill_payload(path).unwrap())
            .collect::<Vec<_>>();
        let lock_before = fs::read(&lock_path).unwrap();
        fs::create_dir_all(&standard).unwrap();
        fs::write(
            standard.join("SKILL.md"),
            b"newly created by another process",
        )
        .unwrap();
        let stale = service
            .execute_prepared(prepared, &[selected], CancellationSignal::default(), |_| {})
            .await
            .unwrap();
        assert_ne!(stale.outcome, UpdateOutcome::Succeeded);
        for (path, payload) in private.iter().zip(&contents) {
            assert_eq!(&build_skill_payload(path).unwrap(), payload);
        }
        assert_eq!(fs::read(lock_path).unwrap(), lock_before);
        assert_eq!(
            fs::read(standard.join("SKILL.md")).unwrap(),
            b"newly created by another process"
        );
    }

    #[tokio::test]
    async fn native_project_update_excludes_library_members_through_leaf_and_root_links() {
        use crate::application::skill_libraries::{LibraryId, LIBRARY_SCHEMA_VERSION};

        for (relative, link_root) in [
            (".agents/skills/alpha", false),
            (".agents/skills", true),
            (".codebuddy/skills", true),
        ] {
            let fixture = UpdateLifecycleFixture::new(["alpha", "beta"]).await;
            fixture.install().await;
            let libraries = update_library_repository(fixture._root.path());
            let catalog = serde_json::from_value(json!({
                "schemaVersion": LIBRARY_SCHEMA_VERSION,
                "libraries": [{"id":"owned-library", "name":"Owned library", "skills":[{
                    "name":"alpha", "description":"Library content", "sourceRecord":{}, "contentManifestHash":"library-baseline"
                }], "retiredSkills":[]}]
            })).unwrap();
            libraries
                .save(&EnvironmentRef::Native, &catalog)
                .await
                .unwrap();
            let library_root = PathBuf::from(
                libraries
                    .resolve_collection(&EnvironmentRef::Native, &LibraryId::parse("owned-library"))
                    .await
                    .unwrap()
                    .root
                    .native_path,
            );
            let member = library_root.join("alpha");
            fs::create_dir_all(&member).unwrap();
            fs::write(
                member.join("SKILL.md"),
                b"---\nname: alpha\ndescription: Library content\n---\nKeep library content\n",
            )
            .unwrap();
            fs::write(member.join("library-only.txt"), b"must remain unchanged").unwrap();
            let before = build_skill_payload(&member).unwrap();
            let entry = fixture.project_path.join(relative);
            fs::remove_dir_all(&entry).unwrap();
            let target = if link_root { &library_root } else { &member };
            #[cfg(unix)]
            std::os::unix::fs::symlink(target, &entry).unwrap();
            #[cfg(windows)]
            junction::create(target, &entry).unwrap();

            let listed = fixture.read_installed(None).await.unwrap();
            let listed_alpha = listed
                .skills
                .iter()
                .find(|skill| skill.name == "alpha")
                .unwrap();
            assert_ne!(
                fs::canonicalize(&listed_alpha.canonical_path).unwrap(),
                fs::canonicalize(&member).unwrap()
            );

            fixture.remote.publish_change("alpha");
            let service = fixture.update_service();
            let prepared = service
                .prepare(
                    &UpdateRequest {
                        context: fixture.context(),
                        skill_names: vec!["alpha".into()],
                    },
                    CancellationSignal::default(),
                )
                .await
                .unwrap();
            assert!(
                prepared.preview.blocked.is_empty(),
                "{relative}: {:#?}",
                prepared.preview
            );
            let preview = &prepared.preview.skills[0];
            let physical_library = fs::canonicalize(&library_root).unwrap();
            for path in preview
                .targets
                .iter()
                .map(|target| &target.display_path)
                .chain(
                    preview
                        .overwrite_private_entries
                        .iter()
                        .map(|target| &target.display_path),
                )
            {
                assert!(
                    !fs::canonicalize(&path.native_path)
                        .unwrap()
                        .starts_with(&physical_library),
                    "{relative}: library content entered update preview"
                );
            }
            let selected = preview
                .overwrite_private_entries
                .iter()
                .map(|entry| entry.entry_id.clone())
                .chain(
                    preview
                        .targets
                        .iter()
                        .filter_map(|target| target.selectable_entry_id.clone()),
                )
                .collect::<Vec<_>>();
            let result = service
                .execute_prepared(prepared, &selected, CancellationSignal::default(), |_| {})
                .await
                .unwrap();
            assert_eq!(
                result.outcome,
                UpdateOutcome::Succeeded,
                "{relative}: {result:#?}"
            );
            assert_eq!(
                build_skill_payload(&member).unwrap(),
                before,
                "{relative}: library content was overwritten"
            );
        }
    }

    #[tokio::test]
    async fn native_project_update_respects_resolved_scope_boundaries() {
        for layout in [
            "external-agent-root",
            "internal-agent-root",
            "project-alias",
        ] {
            let fixture = UpdateLifecycleFixture::new(["alpha", "beta"]).await;
            fixture.install().await;
            let (logical, actual) = match layout {
                "external-agent-root" => (
                    fixture.project_path.join(".codebuddy"),
                    fixture._root.path().join("external-agent-content"),
                ),
                "internal-agent-root" => (
                    fixture.project_path.join(".codebuddy"),
                    fixture.project_path.join("agent-content"),
                ),
                _ => (
                    fixture.project_path.clone(),
                    fixture._root.path().join("actual-project"),
                ),
            };
            fs::rename(&logical, &actual).unwrap();
            #[cfg(unix)]
            std::os::unix::fs::symlink(&actual, &logical).unwrap();
            #[cfg(windows)]
            junction::create(&actual, &logical).unwrap();
            let link_before = fs::read_link(&logical).unwrap();
            let member = if layout == "project-alias" {
                actual.join(".codebuddy/skills/alpha")
            } else {
                actual.join("skills/alpha")
            };
            let before = build_skill_payload(&member).unwrap();
            fixture.remote.publish_change("alpha");
            let service = fixture.update_service();
            let prepared = service
                .prepare(
                    &UpdateRequest {
                        context: fixture.context(),
                        skill_names: vec!["alpha".into()],
                    },
                    CancellationSignal::default(),
                )
                .await
                .unwrap();
            assert!(
                prepared.preview.blocked.is_empty(),
                "{layout}: {:#?}",
                prepared.preview
            );
            let preview = &prepared.preview.skills[0];
            let physical_member = fs::canonicalize(&member).unwrap();
            let includes_member = preview
                .targets
                .iter()
                .map(|target| &target.display_path)
                .chain(
                    preview
                        .overwrite_private_entries
                        .iter()
                        .map(|target| &target.display_path),
                )
                .any(|path| fs::canonicalize(&path.native_path).unwrap() == physical_member);
            let preserves_member = preview.preserved_targets.as_ref().is_some_and(|targets| {
                targets.iter().any(|target| {
                    fs::canonicalize(&target.display_path.native_path).unwrap() == physical_member
                })
            });
            let selected = preview
                .overwrite_private_entries
                .iter()
                .map(|entry| entry.entry_id.clone())
                .chain(
                    preview
                        .targets
                        .iter()
                        .filter_map(|target| target.selectable_entry_id.clone()),
                )
                .collect::<Vec<_>>();
            let result = service
                .execute_prepared(prepared, &selected, CancellationSignal::default(), |_| {})
                .await
                .unwrap();
            assert_eq!(
                result.outcome,
                UpdateOutcome::Succeeded,
                "{layout}: {result:#?}"
            );
            assert_eq!(fs::read_link(&logical).unwrap(), link_before);
            let changed = build_skill_payload(&member).unwrap() != before;
            if layout == "external-agent-root" {
                assert!(result.skills[0].warnings.is_empty());
                assert!(
                    !includes_member && !changed && preserves_member,
                    "external in update: {includes_member}; external changed: {changed}; shown as preserved: {preserves_member}"
                );
            } else {
                assert!(includes_member && changed && !preserves_member, "{layout}");
            }
        }
    }

    #[tokio::test]
    async fn native_update_preview_groups_sources_and_exposes_read_only_link_locations() {
        let fixture = UpdateLifecycleFixture::new(["alpha", "beta"]).await;
        fixture.install().await;
        let common = fixture.project_path.join(".agents/skills/alpha");
        let link = fixture.project_path.join(".codebuddy/skills/alpha");
        fs::remove_dir_all(&link).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(&common, &link).unwrap();
        #[cfg(windows)]
        junction::create(&common, &link).unwrap();
        let before = fixture.clone_count();
        let prepared = fixture
            .update_service()
            .prepare(
                &UpdateRequest {
                    context: fixture.context(),
                    skill_names: vec!["alpha".into(), "beta".into()],
                },
                CancellationSignal::default(),
            )
            .await
            .unwrap();
        assert!(
            prepared.preview.blocked.is_empty(),
            "{:#?}",
            prepared.preview
        );
        assert_eq!(prepared.preview.sources.len(), 1);
        let group = &prepared.preview.sources[0];
        assert_eq!(group.skill_names, vec!["alpha", "beta"]);
        assert_eq!(fixture.clone_count(), before + 1);
        assert!(prepared
            .preview
            .skills
            .iter()
            .all(|skill| skill.source_key.as_ref() == Some(&group.source_key)));
        let alpha = &prepared.preview.skills[0];
        let standard = alpha
            .targets
            .iter()
            .find(|target| target.is_standard == Some(true))
            .unwrap();
        assert_eq!(alpha.linked_targets.len(), 1);
        let linked = &alpha.linked_targets[0];
        let backend = if cfg!(windows) {
            ExecutionBackend::NativeWindows
        } else {
            ExecutionBackend::NativeUnix
        };
        let physical_key = |path: &Path| {
            crate::environment::native::tree::project_target(path, backend.clone())
                .unwrap()
                .key
        };
        assert_eq!(
            physical_key(Path::new(&standard.display_path.native_path)),
            physical_key(&common)
        );
        assert_eq!(
            physical_key(Path::new(&linked.display_path.native_path)),
            physical_key(&link)
        );
        assert_eq!(
            physical_key(Path::new(&linked.target_path.native_path)),
            physical_key(&common)
        );
        assert!(linked.target_copy_entry_id.is_none());
        assert!(alpha.overwrite_private_entries.is_empty());
        assert!(!alpha.targets.iter().any(|target| physical_key(Path::new(
            &target.display_path.native_path
        )) == physical_key(&link)));
    }

    #[tokio::test]
    async fn native_update_optional_copies_skip_content_and_can_be_updated_later() {
        use crate::application::update::UpdateCoverage;
        for keep_standard in [true, false] {
            let fixture = UpdateLifecycleFixture::new(["alpha", "beta"]).await;
            fixture.install().await;
            let standard = fixture.project_path.join(".agents/skills/alpha");
            let copy = fixture.project_path.join(".minimax/skills/alpha");
            let link = fixture.project_path.join(".codebuddy/skills/alpha");
            fs::remove_dir_all(&link).unwrap();
            #[cfg(unix)]
            std::os::unix::fs::symlink(&copy, &link).unwrap();
            #[cfg(windows)]
            junction::create(&copy, &link).unwrap();
            fs::remove_dir_all(fixture.project_path.join(".custom/skills/alpha")).unwrap();
            if !keep_standard {
                fs::remove_dir_all(&standard).unwrap();
            }
            let before = build_skill_payload(&copy).unwrap();
            let lock_path = fixture.project_path.join("skills-lock.json");
            let lock_before = fs::read(&lock_path).unwrap();
            fixture.remote.publish_change("alpha");
            let service = fixture.update_service();
            let request = UpdateRequest {
                context: fixture.context(),
                skill_names: vec!["alpha".into()],
            };
            let prepared = service
                .prepare(&request, CancellationSignal::default())
                .await
                .unwrap();
            assert!(
                prepared.preview.blocked.is_empty(),
                "{:#?}",
                prepared.preview
            );
            let target = prepared.preview.skills[0]
                .targets
                .iter()
                .find(|target| target.selectable_entry_id.is_some())
                .unwrap();
            assert_eq!(
                prepared.preview.skills[0].linked_targets[0].target_copy_entry_id,
                target.selectable_entry_id
            );
            assert!(target
                .readers
                .iter()
                .all(|reader| reader.agent_id.as_str() != "codebuddy"));
            assert!(prepared.preview.skills[0]
                .targets
                .iter()
                .filter(|target| target.is_standard == Some(true))
                .all(|target| target.selectable_entry_id.is_none()));
            let result = service
                .execute_prepared(prepared, &[], CancellationSignal::default(), |_| {})
                .await
                .unwrap();
            assert_eq!(
                build_skill_payload(&copy).unwrap(),
                before,
                "unchecked matching copy was overwritten"
            );
            assert_eq!(build_skill_payload(&link).unwrap(), before);
            assert_eq!(
                result.skills[0].skipped_copy_paths.as_ref().unwrap().len(),
                1
            );
            if keep_standard {
                assert_eq!(result.outcome, UpdateOutcome::Succeeded);
                assert_eq!(
                    result.skills[0].coverage,
                    UpdateCoverage::UpdatedWithSkippedCopies
                );
                assert_ne!(build_skill_payload(&standard).unwrap(), before);
            } else {
                assert!(result.skills[0].mutation.is_none());
                assert_eq!(fs::read(&lock_path).unwrap(), lock_before);
            }
            let prepared = service
                .prepare(&request, CancellationSignal::default())
                .await
                .unwrap();
            let preview = &prepared.preview.skills[0];
            let selected = preview
                .targets
                .iter()
                .filter_map(|target| target.selectable_entry_id.clone())
                .chain(
                    preview
                        .overwrite_private_entries
                        .iter()
                        .map(|entry| entry.entry_id.clone()),
                )
                .collect::<Vec<_>>();
            assert_eq!(selected.len(), 1);
            let result = service
                .execute_prepared(prepared, &selected, CancellationSignal::default(), |_| {})
                .await
                .unwrap();
            assert_eq!(result.outcome, UpdateOutcome::Succeeded, "{result:#?}");
            assert_ne!(build_skill_payload(&copy).unwrap(), before);
            assert_eq!(
                build_skill_payload(&copy).unwrap(),
                build_skill_payload(&link).unwrap()
            );
        }
    }

    #[tokio::test]
    async fn native_project_update_with_only_external_content_does_not_acquire_or_write() {
        use crate::application::mutation::result::OperationErrorCode;
        use crate::application::update::UpdateCoverage;

        let fixture = UpdateLifecycleFixture::new(["alpha", "beta"]).await;
        fixture.install().await;
        let logical = fixture.project_path.join(".codebuddy");
        let external = fixture._root.path().join("external-agent-content");
        fs::rename(&logical, &external).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(&external, &logical).unwrap();
        #[cfg(windows)]
        junction::create(&external, &logical).unwrap();
        for root in [".agents/skills", ".minimax/skills", ".custom/skills"] {
            let path = fixture.project_path.join(root).join("alpha");
            if path.exists() {
                fs::remove_dir_all(path).unwrap();
            }
        }
        let lock_path = fixture.project_path.join("skills-lock.json");
        let lock_before = fs::read(&lock_path).unwrap();
        let member = external.join("skills/alpha");
        let content_before = build_skill_payload(&member).unwrap();
        let clones_before = fixture.clone_count();
        let service = fixture.update_service();
        let prepared = service
            .prepare(
                &UpdateRequest {
                    context: fixture.context(),
                    skill_names: vec!["alpha".into()],
                },
                CancellationSignal::default(),
            )
            .await
            .unwrap();
        assert!(
            prepared.preview.blocked.is_empty(),
            "{:#?}",
            prepared.preview
        );
        let preview = &prepared.preview.skills[0];
        assert!(preview.targets.is_empty());
        assert!(preview.overwrite_private_entries.is_empty());
        assert_eq!(
            preview.blocking_reasons,
            vec![OperationErrorCode::NoUpdateTargets]
        );
        assert!(!preview.preserved_targets.as_ref().unwrap().is_empty());
        assert_eq!(fixture.clone_count(), clones_before);
        let result = service
            .execute_prepared(prepared, &[], CancellationSignal::default(), |_| {})
            .await
            .unwrap();
        assert!(matches!(
            &result.skills[0].coverage,
            UpdateCoverage::NotUpdated { error } if error.code == OperationErrorCode::NoUpdateTargets
        ));
        assert!(result.skills[0].mutation.is_none());
        assert_eq!(fs::read(lock_path).unwrap(), lock_before);
        assert_eq!(build_skill_payload(&member).unwrap(), content_before);
    }

    #[tokio::test]
    async fn native_project_update_rejects_a_retargeted_agent_root_after_confirmation() {
        let fixture = UpdateLifecycleFixture::new(["alpha", "beta"]).await;
        fixture.install().await;
        let logical = fixture.project_path.join(".codebuddy");
        let actual = fixture.project_path.join("agent-content");
        let external = fixture._root.path().join("external-content");
        fs::rename(&logical, &actual).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(&actual, &logical).unwrap();
        #[cfg(windows)]
        junction::create(&actual, &logical).unwrap();
        fs::create_dir_all(external.join("skills/alpha")).unwrap();
        fs::write(external.join("skills/alpha/SKILL.md"), b"external content").unwrap();
        let actual_before = build_skill_payload(&actual.join("skills/alpha")).unwrap();
        let canonical = fixture.project_path.join(".agents/skills/alpha");
        let canonical_before = build_skill_payload(&canonical).unwrap();
        let lock_before = fs::read(fixture.project_path.join("skills-lock.json")).unwrap();
        fixture.remote.publish_change("alpha");
        let service = fixture.update_service();
        let prepared = service
            .prepare(
                &UpdateRequest {
                    context: fixture.context(),
                    skill_names: vec!["alpha".into()],
                },
                CancellationSignal::default(),
            )
            .await
            .unwrap();
        assert!(
            prepared.preview.blocked.is_empty(),
            "{:#?}",
            prepared.preview
        );
        let selected = prepared.preview.skills[0]
            .overwrite_private_entries
            .iter()
            .map(|entry| entry.entry_id.clone())
            .collect::<Vec<_>>();
        #[cfg(unix)]
        fs::remove_file(&logical).unwrap();
        #[cfg(windows)]
        fs::remove_dir(&logical).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(&external, &logical).unwrap();
        #[cfg(windows)]
        junction::create(&external, &logical).unwrap();
        let result = service
            .execute_prepared(prepared, &selected, CancellationSignal::default(), |_| {})
            .await
            .unwrap();
        assert_ne!(result.outcome, UpdateOutcome::Succeeded, "{result:#?}");
        assert_eq!(
            build_skill_payload(&actual.join("skills/alpha")).unwrap(),
            actual_before
        );
        assert_eq!(build_skill_payload(&canonical).unwrap(), canonical_before);
        assert_eq!(
            fs::read(fixture.project_path.join("skills-lock.json")).unwrap(),
            lock_before
        );
        assert_eq!(
            fs::read(external.join("skills/alpha/SKILL.md")).unwrap(),
            b"external content"
        );
    }

    #[tokio::test]
    async fn native_update_replaces_equal_content_with_a_different_existing_marker() {
        let fixture = UpdateLifecycleFixture::new(["alpha", "beta"]).await;
        fixture.install().await;
        let path = fixture.project_path.join(".agents/skills/beta");
        let content = build_skill_payload(&path).unwrap();
        let lock_path = fixture.project_path.join("skills-lock.json");
        let mut lock = read_json(&lock_path).unwrap();
        lock["skills"]["beta"]["computedHash"] = json!("existing-marker-from-another-algorithm");
        write_json(&lock_path, &lock).unwrap();
        let checked = fixture
            .check_service()
            .check(&UpdateCheckRequest {
                context: fixture.context(),
                mode: UpdateCheckMode::Force,
                selection: UpdateCheckSelection::Skills(vec![SkillIdentity {
                    context: fixture.context(),
                    skill_name: "beta".into(),
                }]),
            })
            .await
            .unwrap();
        assert!(checked.skills[0].has_update);
        let service = fixture.update_service();
        let prepared = service
            .prepare(
                &UpdateRequest {
                    context: fixture.context(),
                    skill_names: vec!["beta".into()],
                },
                CancellationSignal::default(),
            )
            .await
            .unwrap();
        let result = service
            .execute_prepared(prepared, &[], CancellationSignal::default(), |_| {})
            .await
            .unwrap();
        assert_eq!(result.outcome, UpdateOutcome::Succeeded);
        assert_eq!(build_skill_payload(&path).unwrap(), content);
        assert_eq!(
            read_json(&lock_path).unwrap()["skills"]["beta"]["computedHash"],
            fixture.remote.computed_hash("beta")
        );
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
        let preview = fixture.prepare(["alpha"]).await;
        fixture.cancel(preview).await;
        let before = fixture.clone_count();
        let response = fixture.confirm_preserving_conflicts(["alpha"]).await;

        assert_eq!(fixture.clone_count(), before);
        assert_eq!(response.outcome, UpdateOutcome::Succeeded);
        assert!(response.skills[0]
            .warnings
            .contains(&UpdateWarningCode::SkippedCopy));

        fixture.assert_source_partial().await;
        fixture.assert_per_skill_staging_partial().await;
        fixture.assert_final_lock_and_no_second_check().await;
    }
}

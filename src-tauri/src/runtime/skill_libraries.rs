use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tokio::sync::{Mutex as AsyncMutex, OwnedMutexGuard};

use crate::application::collection_records::CollectionRecordReader;
use crate::application::installed_skill_resolver::InstalledSkillResolver;
use crate::application::library_application::{
    library_usage_state, validate_application_record, ApplicationInventory,
    ApplicationInventoryProblem, ApplicationRegistry, LibraryApplicationFuture,
    LibraryApplicationRecord, LibraryApplicationResources, LibraryUsageAccumulator,
    VersionedApplicationRecord,
};
use crate::application::library_membership::{apply_membership_change, MembershipChange};
use crate::application::payload_session::{
    PayloadLocalSource, PayloadSessionStorage, PayloadStorageKey,
};
use crate::application::skill_libraries::{
    validate_catalog, CommitLibraryMemberRequest, LibraryCatalog, LibraryFuture, LibraryId,
    LibraryMemberMutation, LibraryUsage, LibraryUsageProvider, LibraryUsageSnapshot,
    LibraryUsageState, PurgeRetiredLibraryMemberRequest, SkillLibraryRepository,
};
use crate::application::skill_paths::{ResolvedSkillRoot, SkillPathObserver};
use crate::core::projects::ProjectMigrationRegistry;
use crate::core::skill_payload::SkillPayload;
use crate::environment::native::atomic_file::NativeAtomicDocumentIo;
use crate::environment::native::entry::{materialize_payload, verify_materialized_payload};
use crate::environment::runtime::PhysicalParentIdentity;
use crate::environment::types::{
    EnvironmentKey, EnvironmentRef, ProjectInfo, RegisteredProject, ResourceLocator,
};
use crate::environment::types::{SkillLocation, SkillLocationRef};
use crate::environment::wsl::operations::acquire::WslPayloadSessionStorage;
use crate::environment::wsl::operations::atomic_file::WslAtomicDocumentIo;
use crate::environment::wsl::WslRuntime;
use crate::error::AppError;
use crate::storage::atomic_document::AtomicDocumentIo;

#[derive(Default)]
struct LibraryIoCoordinator {
    gates: Mutex<HashMap<EnvironmentKey, Arc<AsyncMutex<()>>>>,
}

impl LibraryIoCoordinator {
    async fn acquire(&self, environment: &EnvironmentRef) -> OwnedMutexGuard<()> {
        let gate = self
            .gates
            .lock()
            .expect("Library I/O coordinator lock poisoned")
            .entry(EnvironmentKey::from_ref(environment))
            .or_insert_with(|| Arc::new(AsyncMutex::new(())))
            .clone();
        gate.lock_owned().await
    }
}

pub struct RuntimeSkillLibraryRepository {
    native_root: PathBuf,
    wsl: Arc<WslRuntime>,
    projects: Arc<ProjectMigrationRegistry>,
    io: Arc<LibraryIoCoordinator>,
}

fn project_usage_candidate(
    environment: &EnvironmentRef,
    project: ProjectInfo,
) -> (SkillLocationRef, Option<RegisteredProject>) {
    let binding = project.binding;
    let context = SkillLocationRef {
        environment: environment.clone(),
        scope: SkillLocation::Project {
            project_id: binding.id.clone(),
        },
    };
    (context, Some(binding))
}

impl RuntimeSkillLibraryRepository {
    pub fn new(
        native_root: PathBuf,
        wsl: Arc<WslRuntime>,
        projects: Arc<ProjectMigrationRegistry>,
    ) -> Self {
        Self {
            native_root,
            wsl,
            projects,
            io: Arc::new(LibraryIoCoordinator::default()),
        }
    }

    fn native_skill_root(&self, library_id: &LibraryId) -> PathBuf {
        self.native_root
            .join("libraries")
            .join(library_id.as_str())
            .join("skills")
    }

    fn native_skill_path(
        &self,
        library_id: &LibraryId,
        skill_name: &str,
    ) -> Result<PathBuf, AppError> {
        Ok(self
            .native_skill_root(library_id)
            .join(InstalledSkillResolver::install_dir_name(skill_name)?))
    }

    fn native_application(&self, context: &SkillLocationRef) -> Result<ResourceLocator, AppError> {
        let relative = application_relative_path(&context.scope)?;
        Ok(ResourceLocator {
            environment: EnvironmentRef::Native,
            native_path: self
                .native_root
                .join("applications")
                .join(relative)
                .to_string_lossy()
                .into_owned(),
        })
    }

    async fn usage_candidates(
        &self,
        environment: &EnvironmentRef,
    ) -> Result<Vec<(SkillLocationRef, Option<RegisteredProject>)>, AppError> {
        let projects = crate::environment::project_service::list_environment_projects(
            environment.clone(),
            self.wsl.as_ref(),
            self.projects.as_ref(),
        )
        .await?;
        let mut candidates = vec![(
            SkillLocationRef {
                environment: environment.clone(),
                scope: SkillLocation::Global,
            },
            None,
        )];
        candidates.extend(
            projects
                .into_iter()
                .map(|project| project_usage_candidate(environment, project)),
        );
        Ok(candidates)
    }

    async fn usage_metadata(
        &self,
        environment: &EnvironmentRef,
    ) -> HashMap<SkillLocationRef, Option<RegisteredProject>> {
        match self.usage_candidates(environment).await {
            Ok(candidates) => candidates.into_iter().collect(),
            Err(error) => {
                log::warn!("Skill Library project metadata is unavailable: {error}");
                HashMap::new()
            }
        }
    }
}

impl SkillLibraryRepository for RuntimeSkillLibraryRepository {
    fn resolve_collection<'a>(
        &'a self,
        environment: &'a EnvironmentRef,
        library_id: &'a LibraryId,
    ) -> LibraryFuture<'a, Result<ResolvedSkillRoot, AppError>> {
        Box::pin(async move {
            let (root, revision) = match environment {
                EnvironmentRef::Native => (
                    ResourceLocator {
                        environment: EnvironmentRef::Native,
                        native_path: self
                            .native_skill_root(library_id)
                            .to_string_lossy()
                            .into_owned(),
                    },
                    "native-library-root-v1".to_string(),
                ),
                EnvironmentRef::Wsl { distro_name } => {
                    let distro_name = distro_name.clone();
                    let library_id = library_id.as_str().to_string();
                    self.wsl
                        .with_session_read_retry(&distro_name, move |session| {
                            let library_id = library_id.clone();
                            async move {
                                Ok((
                                    ResourceLocator {
                                        environment: EnvironmentRef::Wsl {
                                            distro_name: session.distro_name.clone(),
                                        },
                                        native_path: format!(
                                            "{}/.skill-deck/skill-libraries/libraries/{library_id}/skills",
                                            session.home.trim_end_matches('/'),
                                        ),
                                    },
                                    format!("wsl-runtime-{}", session.runtime_generation),
                                ))
                            }
                        })
                        .await?
                }
            };
            SkillPathObserver::resolve_collection(environment.clone(), root, &revision)
        })
    }

    fn load<'a>(
        &'a self,
        environment: &'a EnvironmentRef,
    ) -> LibraryFuture<'a, Result<LibraryCatalog, AppError>> {
        Box::pin(async move {
            let _io = self.io.acquire(environment).await;
            let bytes = match environment {
                EnvironmentRef::Native => {
                    let root = self.native_root.clone();
                    tokio::task::spawn_blocking(move || load_native_catalog_bytes(&root))
                        .await
                        .map_err(|error| AppError::ExecutionFailed {
                            message: format!("Skill Library read task failed: {error}"),
                        })??
                }
                EnvironmentRef::Wsl { distro_name } => {
                    self.wsl
                        .workspace(distro_name)?
                        .read_library_catalog()
                        .await?
                }
            };
            bytes
                .as_deref()
                .map(parse_library_catalog)
                .transpose()
                .map(|catalog| catalog.unwrap_or_default())
        })
    }

    fn save<'a>(
        &'a self,
        environment: &'a EnvironmentRef,
        catalog: &'a LibraryCatalog,
    ) -> LibraryFuture<'a, Result<(), AppError>> {
        Box::pin(async move {
            let _io = self.io.acquire(environment).await;
            let bytes = serde_json::to_vec_pretty(catalog)?;
            match environment {
                EnvironmentRef::Native => {
                    let root = self.native_root.clone();
                    let library_ids: Vec<LibraryId> = catalog
                        .libraries
                        .iter()
                        .map(|library| library.id.clone())
                        .collect();
                    tokio::task::spawn_blocking(move || {
                        save_native_catalog(&root, &library_ids, &bytes)
                    })
                    .await
                    .map_err(|error| AppError::ExecutionFailed {
                        message: format!("Skill Library save task failed: {error}"),
                    })?
                }
                EnvironmentRef::Wsl { distro_name } => {
                    let workspace = self.wsl.workspace(distro_name)?;
                    let snapshot = workspace.read_library_catalog_once().await?;
                    workspace
                        .execute_library_operation(
                            snapshot.generation,
                            environment_protocol::LibraryOperationRequest {
                                operation_id: uuid::Uuid::new_v4().simple().to_string(),
                                expected_catalog_revision: snapshot.revision,
                                catalog_bytes: bytes,
                                action: environment_protocol::LibraryOperationAction::SaveCatalog {
                                    library_ids: catalog
                                        .libraries
                                        .iter()
                                        .map(|library| library.id.as_str().to_string())
                                        .collect(),
                                },
                                deadline_millis: 60_000,
                            },
                        )
                        .await
                        .map(|_| ())
                }
            }
        })
    }

    fn commit_member<'a>(
        &'a self,
        request: CommitLibraryMemberRequest,
    ) -> LibraryFuture<'a, Result<(), AppError>> {
        Box::pin(async move {
            let _io = self.io.acquire(&request.environment).await;
            if matches!(request.environment, EnvironmentRef::Native) {
                let root = self.native_root.clone();
                return tokio::task::spawn_blocking(move || commit_native_member(&root, request))
                    .await
                    .map_err(|error| AppError::ExecutionFailed {
                        message: format!("Skill Library commit task failed: {error}"),
                    })?;
            }
            let EnvironmentRef::Wsl { distro_name } = &request.environment else {
                return Err(AppError::StaleEnvironment);
            };
            let distro_name = distro_name.clone();
            let workspace = self.wsl.workspace(&distro_name)?;
            self.wsl
                .with_session(&distro_name, move |session| {
                    let request = request.clone();
                    let workspace = workspace.clone();
                    async move { commit_wsl_member(&session, &workspace, request).await }
                })
                .await
        })
    }

    fn purge_retired<'a>(
        &'a self,
        request: PurgeRetiredLibraryMemberRequest,
    ) -> LibraryFuture<'a, Result<(), AppError>> {
        Box::pin(async move {
            let _io = self.io.acquire(&request.environment).await;
            match &request.environment {
                EnvironmentRef::Native => {
                    let root = self.native_root.clone();
                    tokio::task::spawn_blocking(move || purge_native_retired(&root, request))
                        .await
                        .map_err(|error| AppError::ExecutionFailed {
                            message: format!("retired Library member purge task failed: {error}"),
                        })?
                }
                EnvironmentRef::Wsl { distro_name } => {
                    let distro_name = distro_name.clone();
                    let workspace = self.wsl.workspace(&distro_name)?;
                    self.wsl
                        .with_session(&distro_name, move |session| {
                            let workspace = workspace.clone();
                            let request = request.clone();
                            async move { purge_wsl_retired(&session, &workspace, request).await }
                        })
                        .await
                }
            }
        })
    }

    fn delete_library<'a>(
        &'a self,
        environment: &'a EnvironmentRef,
        library_id: &'a LibraryId,
    ) -> LibraryFuture<'a, Result<LibraryCatalog, AppError>> {
        Box::pin(async move {
            let _io = self.io.acquire(environment).await;
            validate_storage_component(library_id.as_str())?;
            match environment {
                EnvironmentRef::Native => {
                    let root = self.native_root.clone();
                    let library_id = library_id.clone();
                    tokio::task::spawn_blocking(move || delete_native_library(&root, &library_id))
                        .await
                        .map_err(|error| AppError::ExecutionFailed {
                            message: format!("Skill Library deletion task failed: {error}"),
                        })?
                }
                EnvironmentRef::Wsl { distro_name } => {
                    let distro_name = distro_name.clone();
                    let library_id = library_id.as_str().to_string();
                    let workspace = self.wsl.workspace(&distro_name)?;
                    self.wsl
                        .with_session(&distro_name, move |session| {
                            let library_id = library_id.clone();
                            let workspace = workspace.clone();
                            async move {
                                delete_wsl_library(&session, &workspace, &library_id).await
                            }
                        })
                        .await
                }
            }
        })
    }

    fn read_skill_content<'a>(
        &'a self,
        environment: &'a EnvironmentRef,
        library_id: &'a LibraryId,
        skill_name: &'a str,
    ) -> LibraryFuture<'a, Result<String, AppError>> {
        Box::pin(async move {
            let _io = self.io.acquire(environment).await;
            validate_storage_component(library_id.as_str())?;
            let install_dir_name = InstalledSkillResolver::install_dir_name(skill_name)?;
            match environment {
                EnvironmentRef::Native => {
                    let path = self.native_skill_path(library_id, skill_name)?;
                    tokio::task::spawn_blocking(move || {
                        crate::core::skill::read_skill_content(&path.to_string_lossy())
                    })
                    .await
                    .map_err(|error| AppError::ExecutionFailed {
                        message: format!("Skill Library content read task failed: {error}"),
                    })?
                }
                EnvironmentRef::Wsl { distro_name } => {
                    let distro_name = distro_name.clone();
                    let library_id = library_id.as_str().to_string();
                    let skill_name = install_dir_name;
                    let path = self
                        .wsl
                        .with_session_read_retry(&distro_name, move |session| {
                            let library_id = library_id.clone();
                            let skill_name = skill_name.clone();
                            async move {
                                Ok(format!(
                                    "{}/.skill-deck/skill-libraries/libraries/{}/skills/{}",
                                    session.home.trim_end_matches('/'),
                                    library_id,
                                    skill_name
                                ))
                            }
                        })
                        .await?;
                    let workspace = self.wsl.workspace(&distro_name)?;
                    let markdown =
                        crate::environment::wsl::operations::skill_content::read_skill_markdown(
                            &workspace, &path,
                        )
                        .await?;
                    Ok(crate::core::skill::skill_content_from_markdown(&markdown))
                }
            }
        })
    }
}

impl LibraryUsageProvider for RuntimeSkillLibraryRepository {
    fn usages<'a>(
        &'a self,
        environment: &'a EnvironmentRef,
        library_id: &'a LibraryId,
    ) -> LibraryFuture<'a, Result<Vec<LibraryUsage>, AppError>> {
        Box::pin(async move {
            let mut usages = Vec::new();
            let inventory = self.enumerate(environment).await?;
            report_inventory_problems(&inventory);
            require_complete_inventory(&inventory)?;
            let projects = self.usage_metadata(environment).await;
            for application in inventory.records {
                if let Some(state) = library_usage_state(&application.record, library_id) {
                    let context = application.context;
                    usages.push(LibraryUsage {
                        project: projects.get(&context).cloned().flatten(),
                        context,
                        state,
                    });
                }
            }
            Ok(usages)
        })
    }

    fn usage_projection<'a>(
        &'a self,
        environment: &'a EnvironmentRef,
    ) -> LibraryFuture<'a, Result<LibraryUsageSnapshot, AppError>> {
        Box::pin(async move {
            let mut accumulator = LibraryUsageAccumulator::default();
            let inventory = self.enumerate(environment).await?;
            report_inventory_problems(&inventory);
            let inventory_complete = inventory.complete;
            let problem_count = u32::try_from(inventory.problems.len()).unwrap_or(u32::MAX);
            for application in inventory.records {
                accumulator.observe(&application.record);
            }
            Ok(LibraryUsageSnapshot {
                projections: accumulator.finish(),
                inventory_complete,
                problem_count,
            })
        })
    }

    fn agent_usages<'a>(
        &'a self,
        environment: &'a EnvironmentRef,
        agent_id: &'a crate::core::agent_definition::AgentId,
    ) -> LibraryFuture<'a, Result<Vec<LibraryUsage>, AppError>> {
        Box::pin(async move {
            let mut usages = Vec::new();
            let inventory = self.enumerate(environment).await?;
            report_inventory_problems(&inventory);
            require_complete_inventory(&inventory)?;
            let projects = self.usage_metadata(environment).await;
            for application in inventory.records {
                let state = if application
                    .record
                    .current
                    .selected_agent_ids
                    .contains(agent_id)
                {
                    Some(LibraryUsageState::Confirmed)
                } else if application.record.pending.as_ref().is_some_and(|pending| {
                    pending
                        .before_application
                        .selected_agent_ids
                        .contains(agent_id)
                        || pending
                            .target_application
                            .selected_agent_ids
                            .contains(agent_id)
                }) {
                    Some(LibraryUsageState::PendingAdjustment)
                } else {
                    None
                };
                if let Some(state) = state {
                    let context = application.context;
                    usages.push(LibraryUsage {
                        project: projects.get(&context).cloned().flatten(),
                        context,
                        state,
                    });
                }
            }
            Ok(usages)
        })
    }
}

fn report_inventory_problems(inventory: &ApplicationInventory) {
    if inventory.complete {
        return;
    }
    for problem in &inventory.problems {
        log::warn!(
            "Skill Library application inventory skipped {}: {}",
            problem.storage_key,
            problem.error
        );
    }
}

fn require_complete_inventory(inventory: &ApplicationInventory) -> Result<(), AppError> {
    if inventory.complete {
        Ok(())
    } else {
        Err(AppError::ConfigurationCorrupted {
            message: "Skill Library application inventory is incomplete".to_string(),
        })
    }
}

impl ApplicationRegistry for RuntimeSkillLibraryRepository {
    fn load_application<'a>(
        &'a self,
        context: &'a SkillLocationRef,
    ) -> LibraryApplicationFuture<'a, Result<VersionedApplicationRecord, AppError>> {
        Box::pin(async move {
            let (target, snapshot) = match &context.environment {
                EnvironmentRef::Native => {
                    let target = self.native_application(context)?;
                    let snapshot = NativeAtomicDocumentIo
                        .observe(&target, u64::from(environment_protocol::MAX_DOCUMENT_BYTES))
                        .await?;
                    (target, snapshot)
                }
                EnvironmentRef::Wsl { distro_name } => {
                    let distro_name = distro_name.clone();
                    let context = context.clone();
                    let workspace = self.wsl.workspace(&distro_name)?;
                    self.wsl
                        .with_session_read_retry(&distro_name, move |session| {
                            let context = context.clone();
                            let workspace = workspace.clone();
                            async move {
                                let target = wsl_application_locator(&session, &context.scope)?;
                                let snapshot =
                                    WslAtomicDocumentIo::from_active_session(session, workspace)
                                        .observe(
                                            &target,
                                            u64::from(environment_protocol::MAX_DOCUMENT_BYTES),
                                        )
                                        .await?;
                                Ok((target, snapshot))
                            }
                        })
                        .await?
                }
            };
            let record = snapshot
                .bytes
                .as_deref()
                .map(parse_library_application_record)
                .unwrap_or_else(|| Ok(LibraryApplicationRecord::empty()))?;
            Ok(VersionedApplicationRecord {
                context: context.clone(),
                record,
                target,
                snapshot,
            })
        })
    }

    fn save_application_if<'a>(
        &'a self,
        observed: &'a VersionedApplicationRecord,
        record: &'a LibraryApplicationRecord,
    ) -> LibraryApplicationFuture<'a, Result<VersionedApplicationRecord, AppError>> {
        Box::pin(async move {
            validate_application_record(record)?;
            let bytes = serde_json::to_vec_pretty(record)?;
            let snapshot = match &observed.context.environment {
                EnvironmentRef::Native => NativeAtomicDocumentIo
                    .replace(&observed.target, observed.snapshot.clone(), bytes)
                    .await
                    .map_err(crate::storage::atomic_document::DocumentWriteFailure::into_error)?,
                EnvironmentRef::Wsl { distro_name } => {
                    let workspace = self.wsl.workspace(distro_name)?;
                    WslAtomicDocumentIo::new(workspace)
                        .replace(&observed.target, observed.snapshot.clone(), bytes)
                        .await
                        .map_err(
                            crate::storage::atomic_document::DocumentWriteFailure::into_error,
                        )?
                }
            };
            Ok(VersionedApplicationRecord {
                context: observed.context.clone(),
                record: record.clone(),
                target: observed.target.clone(),
                snapshot,
            })
        })
    }

    fn enumerate<'a>(
        &'a self,
        environment: &'a EnvironmentRef,
    ) -> LibraryApplicationFuture<'a, Result<ApplicationInventory, AppError>> {
        Box::pin(async move {
            let contexts = match environment {
                EnvironmentRef::Native => {
                    let root = self.native_root.join("applications");
                    tokio::task::spawn_blocking(move || native_application_contexts(&root))
                        .await
                        .map_err(|error| AppError::ExecutionFailed {
                            message: format!("application inventory task failed: {error}"),
                        })??
                }
                EnvironmentRef::Wsl { distro_name } => {
                    let index = self
                        .wsl
                        .workspace(distro_name)?
                        .list_library_applications()
                        .await?;
                    let mut scopes = vec![("global".to_string(), SkillLocation::Global)];
                    let mut problems = index
                        .problem_keys
                        .into_iter()
                        .map(|storage_key| ApplicationInventoryProblem {
                            storage_key,
                            error: AppError::ConfigurationCorrupted {
                                message: "invalid Skill Library application Scope key".to_string(),
                            },
                        })
                        .collect::<Vec<_>>();
                    for project_id in index.project_ids {
                        if let Err(error) = validate_storage_component(&project_id) {
                            problems.push(ApplicationInventoryProblem {
                                storage_key: format!("projects/{project_id}.json"),
                                error,
                            });
                            continue;
                        }
                        scopes.push((
                            format!("projects/{project_id}.json"),
                            SkillLocation::Project { project_id },
                        ));
                    }
                    if !index.complete && problems.is_empty() {
                        problems.push(ApplicationInventoryProblem {
                            storage_key: "applications".to_string(),
                            error: AppError::ConfigurationCorrupted {
                                message: "incomplete Skill Library application inventory"
                                    .to_string(),
                            },
                        });
                    }
                    ApplicationContexts { scopes, problems }
                }
            };
            let mut records = Vec::new();
            let mut problems = contexts.problems;
            for (storage_key, scope) in contexts.scopes {
                let context = SkillLocationRef {
                    environment: environment.clone(),
                    scope,
                };
                match self.load_application(&context).await {
                    Ok(record) => records.push(record),
                    Err(error) => problems.push(ApplicationInventoryProblem { storage_key, error }),
                }
            }
            records.sort_by(|left, right| {
                application_scope_key(&left.context.scope)
                    .cmp(&application_scope_key(&right.context.scope))
            });
            Ok(ApplicationInventory {
                complete: problems.is_empty(),
                records,
                problems,
            })
        })
    }
}

impl LibraryApplicationResources for RuntimeSkillLibraryRepository {
    fn library_skill_locator<'a>(
        &'a self,
        context: &'a SkillLocationRef,
        library_id: &'a LibraryId,
        skill_name: &'a str,
    ) -> LibraryApplicationFuture<'a, Result<ResourceLocator, AppError>> {
        Box::pin(async move {
            validate_storage_component(library_id.as_str())?;
            let install_dir_name = InstalledSkillResolver::install_dir_name(skill_name)?;
            match &context.environment {
                EnvironmentRef::Native => Ok(ResourceLocator {
                    environment: EnvironmentRef::Native,
                    native_path: self
                        .native_skill_path(library_id, skill_name)?
                        .to_string_lossy()
                        .into_owned(),
                }),
                EnvironmentRef::Wsl { distro_name } => {
                    let distro_name = distro_name.clone();
                    let library_id = library_id.as_str().to_string();
                    let skill_name = install_dir_name;
                    self.wsl
                        .with_session_read_retry(&distro_name, move |session| {
                            let library_id = library_id.clone();
                            let skill_name = skill_name.clone();
                            async move {
                                Ok(ResourceLocator {
                                    environment: EnvironmentRef::Wsl {
                                        distro_name: session.distro_name,
                                    },
                                    native_path: format!(
                                        "{}/.skill-deck/skill-libraries/libraries/{}/skills/{}",
                                        session.home.trim_end_matches('/'),
                                        library_id,
                                        skill_name
                                    ),
                                })
                            }
                        })
                        .await
                }
            }
        })
    }

    fn load_catalog<'a>(
        &'a self,
        context: &'a SkillLocationRef,
    ) -> LibraryApplicationFuture<'a, Result<LibraryCatalog, AppError>> {
        Box::pin(async move { SkillLibraryRepository::load(self, &context.environment).await })
    }

    fn remove_application_if<'a>(
        &'a self,
        observed: &'a VersionedApplicationRecord,
    ) -> LibraryApplicationFuture<'a, Result<(), AppError>> {
        Box::pin(async move {
            let SkillLocation::Project { project_id } = &observed.context.scope else {
                return Err(AppError::Validation {
                    field: Some("context".to_string()),
                    message: "only Project Skill Library applications can be removed".to_string(),
                });
            };
            validate_storage_component(project_id)?;
            let result = match &observed.context.environment {
                EnvironmentRef::Native => {
                    NativeAtomicDocumentIo
                        .remove(&observed.target, observed.snapshot.clone())
                        .await
                }
                EnvironmentRef::Wsl { distro_name } => {
                    WslAtomicDocumentIo::new(self.wsl.workspace(distro_name)?)
                        .remove(&observed.target, observed.snapshot.clone())
                        .await
                }
            };
            result.map_err(crate::storage::atomic_document::DocumentWriteFailure::into_error)
        })
    }
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct NativeLibraryTransaction {
    destination: String,
    phase: NativeLibraryTransactionPhase,
    #[serde(default = "default_true")]
    desired_presence: bool,
    #[serde(default)]
    expected_catalog_hash: Option<String>,
}

#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
enum NativeLibraryTransactionPhase {
    Preparing,
    Staged,
    BackedUp,
    Activated,
    CatalogPrepared,
    CatalogCommitted,
}

fn default_true() -> bool {
    true
}

fn load_native_catalog_bytes(root: &Path) -> Result<Option<Vec<u8>>, AppError> {
    let bytes = match fs::read(root.join("catalog.json")) {
        Ok(bytes) => Some(bytes),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(error.into()),
    };
    let hash = bytes.as_deref().map(bytes_sha256);
    recover_native_library_transactions(root, hash.as_deref())?;
    Ok(bytes)
}

fn parse_library_catalog(bytes: &[u8]) -> Result<LibraryCatalog, AppError> {
    let catalog = serde_json::from_slice(bytes)
        .map_err(|error| invalid_library_document("catalog", error))?;
    validate_catalog(&catalog)?;
    Ok(catalog)
}

fn parse_library_application_record(bytes: &[u8]) -> Result<LibraryApplicationRecord, AppError> {
    let record = serde_json::from_slice(bytes)
        .map_err(|error| invalid_library_document("application record", error))?;
    validate_application_record(&record)?;
    Ok(record)
}

fn invalid_library_document(kind: &str, error: serde_json::Error) -> AppError {
    AppError::ConfigurationCorrupted {
        message: format!("Skill Library {kind} does not match the current data format: {error}"),
    }
}

fn save_native_catalog(
    root: &Path,
    library_ids: &[LibraryId],
    bytes: &[u8],
) -> Result<(), AppError> {
    let current_hash = fs::read(root.join("catalog.json"))
        .ok()
        .map(|bytes| bytes_sha256(&bytes));
    recover_native_library_transactions(root, current_hash.as_deref())?;
    for library_id in library_ids {
        fs::create_dir_all(
            root.join("libraries")
                .join(library_id.as_str())
                .join("skills"),
        )?;
    }
    crate::environment::native::atomic_file::write_native_atomic(&root.join("catalog.json"), bytes)
}

fn save_native_catalog_if_unchanged(
    root: &Path,
    expected: Option<&[u8]>,
    bytes: &[u8],
) -> Result<(), AppError> {
    environment_engine::atomic_document::replace_if_unchanged(
        &root.join("catalog.json"),
        expected,
        bytes,
    )
    .map_err(crate::storage::atomic_document::DocumentWriteFailure::from_engine)
    .map_err(crate::storage::atomic_document::DocumentWriteFailure::into_error)
}

fn delete_native_library(root: &Path, library_id: &LibraryId) -> Result<LibraryCatalog, AppError> {
    let original_bytes = match fs::read(root.join("catalog.json")) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(AppError::PathNotFound {
                path: library_id.as_str().to_string(),
            });
        }
        Err(error) => return Err(error.into()),
    };
    let original_hash = bytes_sha256(&original_bytes);
    recover_native_library_transactions(root, Some(&original_hash))?;
    let mut catalog = parse_library_catalog(&original_bytes)?;
    remove_catalog_library(&mut catalog, library_id.as_str())?;
    let updated_bytes = serde_json::to_vec_pretty(&catalog)?;
    let destination = root.join("libraries").join(library_id.as_str());
    let metadata = match fs::symlink_metadata(&destination) {
        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => Some(metadata),
        Ok(_) => return Err(AppError::StaleTarget),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(error.into()),
    };
    if metadata.is_none() {
        save_native_catalog_if_unchanged(root, Some(&original_bytes), &updated_bytes)?;
        return Ok(catalog);
    }
    let catalog_hash = bytes_sha256(&updated_bytes);
    let commit = (|| {
        stage_native_skill_deletion(root, &destination)?;
        prepare_native_catalog_commit(root, &catalog_hash)?;
        save_native_catalog_if_unchanged(root, Some(&original_bytes), &updated_bytes)?;
        finalize_native_catalog_commit(root, &catalog_hash)
    })();
    if let Err(error) = commit {
        let current_hash = fs::read(root.join("catalog.json"))
            .ok()
            .map(|bytes| bytes_sha256(&bytes));
        recover_native_library_transactions(root, current_hash.as_deref())?;
        if current_hash.as_deref() != Some(&catalog_hash) {
            return Err(error);
        }
    }
    Ok(catalog)
}

async fn delete_wsl_library(
    session: &crate::environment::wsl::WslSession,
    workspace: &crate::environment::wsl::WslWorkspace,
    library_id: &str,
) -> Result<LibraryCatalog, AppError> {
    let snapshot = workspace.read_library_catalog_once().await?;
    if snapshot.generation != session.runtime_generation {
        return Err(AppError::StaleEnvironment);
    }
    let original_bytes = snapshot.bytes.ok_or_else(|| AppError::PathNotFound {
        path: library_id.to_string(),
    })?;
    let mut catalog = parse_library_catalog(&original_bytes)?;
    remove_catalog_library(&mut catalog, library_id)?;
    let updated_bytes = serde_json::to_vec_pretty(&catalog)?;
    let destination = format!(
        "{}/.skill-deck/skill-libraries/libraries/{library_id}",
        session.home.trim_end_matches('/'),
    );
    let target = crate::environment::planning::resolve_wsl_targets(
        session,
        workspace,
        std::slice::from_ref(&destination),
        None,
    )
    .await?
    .pop()
    .ok_or(AppError::StaleTarget)?;
    let (expected_anchor_device, expected_anchor_inode) = match &target.key.physical_parent {
        PhysicalParentIdentity::Wsl {
            distro_name,
            device,
            inode,
        } if distro_name.eq_ignore_ascii_case(&session.distro_name) => (*device, *inode),
        _ => return Err(AppError::StaleTarget),
    };
    workspace
        .execute_library_operation(
            snapshot.generation,
            environment_protocol::LibraryOperationRequest {
                operation_id: uuid::Uuid::new_v4().simple().to_string(),
                expected_catalog_revision: snapshot.revision,
                catalog_bytes: updated_bytes,
                action: environment_protocol::LibraryOperationAction::DeleteLibrary {
                    library_id: library_id.to_string(),
                    expected_anchor_device,
                    expected_anchor_inode,
                    expected_fingerprint: target.fingerprint.0,
                    expected_content_hash: None,
                },
                deadline_millis: 60_000,
            },
        )
        .await?;
    Ok(catalog)
}

fn remove_catalog_library(catalog: &mut LibraryCatalog, library_id: &str) -> Result<(), AppError> {
    let before = catalog.libraries.len();
    catalog
        .libraries
        .retain(|library| library.id.as_str() != library_id);
    if catalog.libraries.len() == before {
        return Err(AppError::PathNotFound {
            path: library_id.to_string(),
        });
    }
    Ok(())
}

fn retired_member<'a>(
    catalog: &'a LibraryCatalog,
    request: &PurgeRetiredLibraryMemberRequest,
) -> Result<&'a crate::application::skill_libraries::RetiredLibrarySkillRecord, AppError> {
    let retired = catalog
        .libraries
        .iter()
        .find(|library| library.id == request.library_id)
        .and_then(|library| {
            library
                .retired_skills
                .iter()
                .find(|retired| retired.member.name == request.skill_name)
        })
        .ok_or_else(|| AppError::PathNotFound {
            path: request.skill_name.clone(),
        })?;
    if retired.retirement_id != request.retirement_id {
        return Err(AppError::StaleTarget);
    }
    Ok(retired)
}

fn purge_native_retired(
    root: &Path,
    request: PurgeRetiredLibraryMemberRequest,
) -> Result<(), AppError> {
    let original_bytes = fs::read(root.join("catalog.json"))?;
    let original_hash = bytes_sha256(&original_bytes);
    recover_native_library_transactions(root, Some(&original_hash))?;
    let mut catalog = parse_library_catalog(&original_bytes)?;
    let expected_hash = retired_member(&catalog, &request)?
        .member
        .content_manifest_hash
        .clone();
    let destination = root
        .join("libraries")
        .join(request.library_id.as_str())
        .join("skills")
        .join(InstalledSkillResolver::install_dir_name(
            &request.skill_name,
        )?);
    let locator = ResourceLocator {
        environment: EnvironmentRef::Native,
        native_path: destination.to_string_lossy().into_owned(),
    };
    let target = crate::environment::planning::resolve_native_targets(&[locator])?
        .pop()
        .ok_or(AppError::StaleTarget)?;
    match target.entry_kind {
        crate::environment::planning::TargetEntryKind::Directory => {
            let actual =
                crate::environment::native::content_manifest::read_directory(&destination)?;
            if actual.hash().as_str() != expected_hash {
                return Err(AppError::StaleTarget);
            }
        }
        crate::environment::planning::TargetEntryKind::Missing => {}
        _ => return Err(AppError::StaleTarget),
    }
    apply_membership_change(
        &mut catalog,
        &request.library_id,
        &request.skill_name,
        MembershipChange::Purge {
            retirement_id: request.retirement_id,
        },
    )?;
    let catalog_bytes = serde_json::to_vec_pretty(&catalog)?;
    let catalog_hash = bytes_sha256(&catalog_bytes);
    if target.entry_kind == crate::environment::planning::TargetEntryKind::Missing {
        return save_native_catalog_if_unchanged(root, Some(&original_bytes), &catalog_bytes);
    }
    let commit = (|| {
        stage_native_skill_deletion(root, &destination)?;
        prepare_native_catalog_commit(root, &catalog_hash)?;
        save_native_catalog_if_unchanged(root, Some(&original_bytes), &catalog_bytes)?;
        finalize_native_catalog_commit(root, &catalog_hash)
    })();
    if let Err(error) = commit {
        let current_hash = fs::read(root.join("catalog.json"))
            .ok()
            .map(|bytes| bytes_sha256(&bytes));
        recover_native_library_transactions(root, current_hash.as_deref())?;
        if current_hash.as_deref() == Some(&catalog_hash) {
            return Ok(());
        }
        return Err(error);
    }
    Ok(())
}

async fn purge_wsl_retired(
    session: &crate::environment::wsl::WslSession,
    workspace: &crate::environment::wsl::WslWorkspace,
    request: PurgeRetiredLibraryMemberRequest,
) -> Result<(), AppError> {
    let snapshot = workspace.read_library_catalog_once().await?;
    if snapshot.generation != session.runtime_generation {
        return Err(AppError::StaleEnvironment);
    }
    let mut catalog = snapshot
        .bytes
        .as_deref()
        .map(parse_library_catalog)
        .transpose()?
        .ok_or_else(|| AppError::PathNotFound {
            path: request.library_id.as_str().to_string(),
        })?;
    let expected_hash = retired_member(&catalog, &request)?
        .member
        .content_manifest_hash
        .clone();
    let destination = format!(
        "{}/.skill-deck/skill-libraries/libraries/{}/skills/{}",
        session.home.trim_end_matches('/'),
        request.library_id.as_str(),
        InstalledSkillResolver::install_dir_name(&request.skill_name)?,
    );
    let target = crate::environment::planning::resolve_wsl_targets(
        session,
        workspace,
        std::slice::from_ref(&destination),
        None,
    )
    .await?
    .pop()
    .ok_or(AppError::StaleTarget)?;
    let expected_content_hash = match target.entry_kind {
        crate::environment::planning::TargetEntryKind::Directory => {
            let manifest = crate::environment::wsl::operations::content_manifest::inspect(
                workspace,
                &crate::environment::content_manifest::ContentManifestTarget {
                    key: target.key.clone(),
                    location: target.destination.clone(),
                },
                None,
            )
            .await?;
            if manifest.hash().as_str() != expected_hash {
                return Err(AppError::StaleTarget);
            }
            Some(expected_hash)
        }
        crate::environment::planning::TargetEntryKind::Missing => None,
        _ => return Err(AppError::StaleTarget),
    };
    apply_membership_change(
        &mut catalog,
        &request.library_id,
        &request.skill_name,
        MembershipChange::Purge {
            retirement_id: request.retirement_id,
        },
    )?;
    let catalog_bytes = serde_json::to_vec_pretty(&catalog)?;
    let action = if target.entry_kind == crate::environment::planning::TargetEntryKind::Missing {
        environment_protocol::LibraryOperationAction::SaveCatalog {
            library_ids: catalog
                .libraries
                .iter()
                .map(|library| library.id.as_str().to_string())
                .collect(),
        }
    } else {
        let (expected_anchor_device, expected_anchor_inode) = match target.key.physical_parent {
            PhysicalParentIdentity::Wsl {
                ref distro_name,
                device,
                inode,
            } if distro_name.eq_ignore_ascii_case(&session.distro_name) => (device, inode),
            _ => return Err(AppError::StaleTarget),
        };
        environment_protocol::LibraryOperationAction::CommitMember {
            library_id: request.library_id.as_str().to_string(),
            skill_name: InstalledSkillResolver::install_dir_name(&request.skill_name)?,
            expected_anchor_device,
            expected_anchor_inode,
            expected_fingerprint: target.fingerprint.0,
            expected_content_hash,
            mutation: environment_protocol::LibraryMemberAction::Delete,
        }
    };
    workspace
        .execute_library_operation(
            snapshot.generation,
            environment_protocol::LibraryOperationRequest {
                operation_id: uuid::Uuid::new_v4().simple().to_string(),
                expected_catalog_revision: snapshot.revision,
                catalog_bytes,
                action,
                deadline_millis: 60_000,
            },
        )
        .await
        .map(|_| ())
}

async fn commit_wsl_member(
    session: &crate::environment::wsl::WslSession,
    workspace: &crate::environment::wsl::WslWorkspace,
    request: CommitLibraryMemberRequest,
) -> Result<(), AppError> {
    validate_storage_component(request.library_id.as_str())?;
    let install_dir_name = InstalledSkillResolver::install_dir_name(&request.skill_name)?;
    let destination = format!(
        "{}/.skill-deck/skill-libraries/libraries/{}/skills/{}",
        session.home.trim_end_matches('/'),
        request.library_id.as_str(),
        install_dir_name,
    );
    let target = crate::environment::planning::resolve_wsl_targets(
        session,
        workspace,
        std::slice::from_ref(&destination),
        None,
    )
    .await?
    .pop()
    .ok_or(AppError::StaleTarget)?;
    let manifest = if target.entry_kind == crate::environment::planning::TargetEntryKind::Directory
    {
        Some(
            crate::environment::wsl::operations::content_manifest::inspect(
                workspace,
                &crate::environment::content_manifest::ContentManifestTarget {
                    key: target.key.clone(),
                    location: target.destination.clone(),
                },
                None,
            )
            .await?
            .hash()
            .clone(),
        )
    } else {
        None
    };
    let (target_revision, content_revision) =
        crate::application::skill_paths::SkillPathObserver::revisions_for_observation(
            &target,
            manifest.clone(),
        )?;
    if target_revision != request.expected.target_revision
        || content_revision != request.expected.content_revision
    {
        return Err(AppError::StaleTarget);
    }

    let (expected_anchor_device, expected_anchor_inode) = match &target.key.physical_parent {
        PhysicalParentIdentity::Wsl {
            distro_name,
            device,
            inode,
        } if distro_name.eq_ignore_ascii_case(&session.distro_name) => (*device, *inode),
        _ => return Err(AppError::StaleTarget),
    };
    let expected_fingerprint = target.fingerprint.0.clone();
    let expected_content_hash = manifest
        .as_ref()
        .map(|manifest| manifest.as_str().to_string());
    let catalog_snapshot = workspace.read_library_catalog_once().await?;
    if catalog_snapshot.generation != session.runtime_generation {
        return Err(AppError::StaleEnvironment);
    }
    let mut catalog = catalog_snapshot
        .bytes
        .as_deref()
        .map(parse_library_catalog)
        .transpose()?
        .unwrap_or_default();
    let snapshot = crate::application::collection_records::LibraryCatalogRecordReader::new(
        &catalog,
        &request.library_id,
    )
    .load_snapshot(std::collections::BTreeSet::from([request
        .skill_name
        .clone()]))?;
    let current = snapshot.records.first().ok_or(AppError::StaleTarget)?;
    if current.source_record_revision != request.expected.source_record_revision {
        return Err(AppError::StaleTarget);
    }
    let _document_changed = snapshot.document_revision != request.expected.document_revision;
    match &request.mutation {
        LibraryMemberMutation::Upsert { record, .. } => {
            apply_membership_change(
                &mut catalog,
                &request.library_id,
                &request.skill_name,
                MembershipChange::Upsert((**record).clone()),
            )?;
        }
        LibraryMemberMutation::Retire {
            retirement_id,
            retired_at,
        } => {
            apply_membership_change(
                &mut catalog,
                &request.library_id,
                &request.skill_name,
                MembershipChange::Retire {
                    retirement_id: retirement_id.clone(),
                    retired_at: retired_at.clone(),
                },
            )?;
        }
    }
    let catalog_bytes = serde_json::to_vec_pretty(&catalog)?;
    if matches!(request.mutation, LibraryMemberMutation::Retire { .. }) {
        return workspace
            .execute_library_operation(
                catalog_snapshot.generation,
                environment_protocol::LibraryOperationRequest {
                    operation_id: uuid::Uuid::new_v4().simple().to_string(),
                    expected_catalog_revision: catalog_snapshot.revision,
                    catalog_bytes,
                    action: environment_protocol::LibraryOperationAction::SaveCatalog {
                        library_ids: catalog
                            .libraries
                            .iter()
                            .map(|library| library.id.as_str().to_string())
                            .collect(),
                    },
                    deadline_millis: 60_000,
                },
            )
            .await
            .map(|_| ());
    }
    let payload_storage = WslPayloadSessionStorage::new(workspace.clone());
    let payload_key = PayloadStorageKey::new(
        format!("library-{}", uuid::Uuid::new_v4().simple()),
        install_dir_name.clone(),
    );
    let mutation = match &request.mutation {
        LibraryMemberMutation::Upsert { content, .. } => {
            payload_storage
                .store(&payload_key, (**content).clone())
                .await?;
            match payload_storage.local_source(&payload_key)? {
                PayloadLocalSource::WslManaged {
                    distro_name,
                    worker_generation,
                    worker_payload_id,
                } if distro_name.eq_ignore_ascii_case(&session.distro_name)
                    && worker_generation == catalog_snapshot.generation =>
                {
                    environment_protocol::LibraryMemberAction::Upsert {
                        payload_id: worker_payload_id,
                    }
                }
                _ => {
                    let _ = payload_storage.remove(&payload_key).await;
                    return Err(AppError::StalePayload);
                }
            }
        }
        LibraryMemberMutation::Retire { .. } => unreachable!("retire returns before payload I/O"),
    };
    let result = workspace
        .execute_library_operation(
            catalog_snapshot.generation,
            environment_protocol::LibraryOperationRequest {
                operation_id: uuid::Uuid::new_v4().simple().to_string(),
                expected_catalog_revision: catalog_snapshot.revision,
                catalog_bytes,
                action: environment_protocol::LibraryOperationAction::CommitMember {
                    library_id: request.library_id.as_str().to_string(),
                    skill_name: install_dir_name,
                    expected_anchor_device,
                    expected_anchor_inode,
                    expected_fingerprint,
                    expected_content_hash,
                    mutation,
                },
                deadline_millis: 60_000,
            },
        )
        .await
        .map(|_| ());
    if matches!(request.mutation, LibraryMemberMutation::Upsert { .. }) {
        let _ = payload_storage.remove(&payload_key).await;
    }
    result
}

fn commit_native_member(root: &Path, request: CommitLibraryMemberRequest) -> Result<(), AppError> {
    let current_catalog_bytes = match fs::read(root.join("catalog.json")) {
        Ok(bytes) => Some(bytes),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(error.into()),
    };
    let current_catalog_hash = current_catalog_bytes.as_deref().map(bytes_sha256);
    recover_native_library_transactions(root, current_catalog_hash.as_deref())?;

    let mut catalog = current_catalog_bytes
        .as_deref()
        .map(parse_library_catalog)
        .transpose()?
        .unwrap_or_default();
    let snapshot = crate::application::collection_records::LibraryCatalogRecordReader::new(
        &catalog,
        &request.library_id,
    )
    .load_snapshot(std::collections::BTreeSet::from([request
        .skill_name
        .clone()]))?;
    let current_record = snapshot.records.first().ok_or(AppError::StaleTarget)?;
    let destination = root
        .join("libraries")
        .join(request.library_id.as_str())
        .join("skills")
        .join(InstalledSkillResolver::install_dir_name(
            &request.skill_name,
        )?);
    let locator = ResourceLocator {
        environment: EnvironmentRef::Native,
        native_path: destination.to_string_lossy().into_owned(),
    };
    let target = crate::environment::planning::resolve_native_targets(&[locator])?
        .pop()
        .ok_or(AppError::StaleTarget)?;
    let manifest = if target.entry_kind == crate::environment::planning::TargetEntryKind::Directory
    {
        Some(
            crate::environment::native::content_manifest::read_directory(&destination)?
                .hash()
                .clone(),
        )
    } else {
        None
    };
    let (target_revision, content_revision) =
        crate::application::skill_paths::SkillPathObserver::revisions_for_observation(
            &target, manifest,
        )?;
    if target_revision != request.expected.target_revision
        || content_revision != request.expected.content_revision
        || current_record.source_record_revision != request.expected.source_record_revision
    {
        return Err(AppError::StaleTarget);
    }
    let _document_changed = snapshot.document_revision != request.expected.document_revision;

    match &request.mutation {
        LibraryMemberMutation::Upsert { record, .. } => {
            apply_membership_change(
                &mut catalog,
                &request.library_id,
                &request.skill_name,
                MembershipChange::Upsert((**record).clone()),
            )?;
        }
        LibraryMemberMutation::Retire {
            retirement_id,
            retired_at,
        } => {
            apply_membership_change(
                &mut catalog,
                &request.library_id,
                &request.skill_name,
                MembershipChange::Retire {
                    retirement_id: retirement_id.clone(),
                    retired_at: retired_at.clone(),
                },
            )?;
        }
    }
    let catalog_bytes = serde_json::to_vec_pretty(&catalog)?;
    let catalog_hash = bytes_sha256(&catalog_bytes);

    if matches!(request.mutation, LibraryMemberMutation::Retire { .. }) {
        return save_native_catalog_if_unchanged(
            root,
            current_catalog_bytes.as_deref(),
            &catalog_bytes,
        );
    }

    let commit = (|| {
        match &request.mutation {
            LibraryMemberMutation::Upsert { content, .. } => {
                replace_native_skill(root, &destination, content)?;
            }
            LibraryMemberMutation::Retire { .. } => unreachable!("retire returns before staging"),
        }
        prepare_native_catalog_commit(root, &catalog_hash)?;
        save_native_catalog_if_unchanged(root, current_catalog_bytes.as_deref(), &catalog_bytes)?;
        finalize_native_catalog_commit(root, &catalog_hash)
    })();
    if let Err(error) = commit {
        let current_hash = fs::read(root.join("catalog.json"))
            .ok()
            .map(|bytes| bytes_sha256(&bytes));
        recover_native_library_transactions(root, current_hash.as_deref())?;
        if current_hash.as_deref() == Some(&catalog_hash) {
            return Ok(());
        }
        return Err(error);
    }
    Ok(())
}

fn stage_native_skill_deletion(root: &Path, destination: &Path) -> Result<(), AppError> {
    let catalog_hash = fs::read(root.join("catalog.json"))
        .ok()
        .map(|bytes| bytes_sha256(&bytes));
    recover_native_library_transactions(root, catalog_hash.as_deref())?;
    let transaction = root
        .join(".transactions")
        .join(uuid::Uuid::new_v4().simple().to_string());
    let backup = transaction.join("backup");
    let marker = transaction.join("transaction.json");
    fs::create_dir_all(&transaction)?;
    write_native_transaction(
        &marker,
        destination,
        NativeLibraryTransactionPhase::Preparing,
        false,
        None,
    )?;
    write_native_transaction(
        &marker,
        destination,
        NativeLibraryTransactionPhase::BackedUp,
        false,
        None,
    )?;
    fs::rename(destination, &backup)?;
    write_native_transaction(
        &marker,
        destination,
        NativeLibraryTransactionPhase::Activated,
        false,
        None,
    )?;
    Ok(())
}

fn replace_native_skill(
    root: &Path,
    destination: &Path,
    payload: &SkillPayload,
) -> Result<(), AppError> {
    let catalog_hash = match fs::read(root.join("catalog.json")) {
        Ok(bytes) => Some(bytes_sha256(&bytes)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(error.into()),
    };
    recover_native_library_transactions(root, catalog_hash.as_deref())?;
    let transactions = root.join(".transactions");
    fs::create_dir_all(&transactions)?;
    let transaction = transactions.join(uuid::Uuid::new_v4().simple().to_string());
    let stage = transaction.join("stage");
    let backup = transaction.join("backup");
    let marker = transaction.join("transaction.json");
    fs::create_dir_all(&transaction)?;
    write_native_transaction(
        &marker,
        destination,
        NativeLibraryTransactionPhase::Preparing,
        true,
        None,
    )?;
    materialize_payload(payload, &stage)?;
    verify_materialized_payload(payload, &stage)?;
    write_native_transaction(
        &marker,
        destination,
        NativeLibraryTransactionPhase::Staged,
        true,
        None,
    )?;
    if fs::symlink_metadata(destination).is_ok() {
        write_native_transaction(
            &marker,
            destination,
            NativeLibraryTransactionPhase::BackedUp,
            true,
            None,
        )?;
        fs::rename(destination, &backup)?;
    }
    fs::create_dir_all(destination.parent().ok_or_else(|| AppError::UnsafePath {
        path: destination.to_string_lossy().into_owned(),
        reason: "Skill Library destination has no parent".to_string(),
    })?)?;
    write_native_transaction(
        &marker,
        destination,
        NativeLibraryTransactionPhase::Activated,
        true,
        None,
    )?;
    fs::rename(&stage, destination)?;
    verify_materialized_payload(payload, destination)?;
    Ok(())
}

fn write_native_transaction(
    marker: &Path,
    destination: &Path,
    phase: NativeLibraryTransactionPhase,
    desired_presence: bool,
    expected_catalog_hash: Option<String>,
) -> Result<(), AppError> {
    let bytes = serde_json::to_vec(&NativeLibraryTransaction {
        destination: destination.to_string_lossy().into_owned(),
        phase,
        desired_presence,
        expected_catalog_hash,
    })?;
    let parent = marker.parent().ok_or_else(|| AppError::UnsafePath {
        path: marker.to_string_lossy().into_owned(),
        reason: "Skill Library transaction marker has no parent".to_string(),
    })?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    temporary.write_all(&bytes)?;
    temporary.as_file().sync_all()?;
    temporary.persist(marker).map_err(|error| AppError::Io {
        message: error.error.to_string(),
    })?;
    Ok(())
}

fn recover_native_library_transactions(
    root: &Path,
    catalog_hash: Option<&str>,
) -> Result<(), AppError> {
    recover_native_transactions(root, catalog_hash).map_err(|error| {
        AppError::LibraryRecoveryIncomplete {
            environment: EnvironmentRef::Native,
            message: error.to_string(),
        }
    })
}

fn recover_native_transactions(root: &Path, catalog_hash: Option<&str>) -> Result<(), AppError> {
    let transactions = root.join(".transactions");
    let Ok(entries) = fs::read_dir(&transactions) else {
        return Ok(());
    };
    for entry in entries {
        let transaction = entry?.path();
        if !transaction.is_dir() {
            continue;
        }
        let marker = transaction.join("transaction.json");
        if !marker.is_file() {
            return Err(AppError::ConfigurationCorrupted {
                message: format!(
                    "Skill Library transaction marker is missing: {}",
                    transaction.display()
                ),
            });
        }
        let record: NativeLibraryTransaction = serde_json::from_slice(&fs::read(&marker)?)?;
        let destination = PathBuf::from(record.destination);
        let stage = transaction.join("stage");
        let backup = transaction.join("backup");
        if matches!(
            record.phase,
            NativeLibraryTransactionPhase::CatalogCommitted
        ) {
            if let Err(error) = cleanup_native_committed_transaction(&transaction) {
                log::warn!(
                    "Skill Library committed transaction cleanup remains pending at {}: {error}",
                    transaction.display()
                );
            }
            continue;
        }
        match record.phase {
            NativeLibraryTransactionPhase::Preparing => {}
            NativeLibraryTransactionPhase::Staged if destination.exists() || stage.exists() => {}
            NativeLibraryTransactionPhase::BackedUp if destination.exists() && !backup.exists() => {
            }
            NativeLibraryTransactionPhase::BackedUp
                if !destination.exists() && backup.exists() && stage.exists() =>
            {
                fs::rename(&backup, &destination)?;
            }
            NativeLibraryTransactionPhase::BackedUp
                if !record.desired_presence
                    && !destination.exists()
                    && backup.exists()
                    && !stage.exists() =>
            {
                fs::rename(&backup, &destination)?;
            }
            NativeLibraryTransactionPhase::BackedUp
                if destination.exists() && backup.exists() && !stage.exists() => {}
            NativeLibraryTransactionPhase::Activated => {
                rollback_native_library_content(&destination, &backup)?;
            }
            NativeLibraryTransactionPhase::CatalogPrepared if destination.exists() => {
                if record.expected_catalog_hash.as_deref() != catalog_hash {
                    rollback_native_library_content(&destination, &backup)?;
                }
            }
            NativeLibraryTransactionPhase::CatalogPrepared
                if !record.desired_presence && !destination.exists() =>
            {
                if record.expected_catalog_hash.as_deref() != catalog_hash {
                    rollback_native_library_content(&destination, &backup)?;
                }
            }
            _ => {
                return Err(AppError::ConfigurationCorrupted {
                    message: format!(
                        "Skill Library transaction cannot be recovered: {}",
                        transaction.display()
                    ),
                })
            }
        }
        if stage.exists() {
            fs::remove_dir_all(&stage)?;
        }
        if backup.exists() {
            fs::remove_dir_all(&backup)?;
        }
        fs::remove_dir_all(&transaction)?;
    }
    Ok(())
}

fn rollback_native_library_content(destination: &Path, backup: &Path) -> Result<(), AppError> {
    if destination.exists() {
        fs::remove_dir_all(destination)?;
    }
    if backup.exists() {
        fs::rename(backup, destination)?;
    }
    Ok(())
}

fn prepare_native_catalog_commit(root: &Path, catalog_hash: &str) -> Result<(), AppError> {
    for transaction in native_transaction_directories(root)? {
        let marker = transaction.join("transaction.json");
        if !marker.is_file() {
            continue;
        }
        let record: NativeLibraryTransaction = serde_json::from_slice(&fs::read(&marker)?)?;
        if matches!(record.phase, NativeLibraryTransactionPhase::Activated) {
            write_native_transaction(
                &marker,
                Path::new(&record.destination),
                NativeLibraryTransactionPhase::CatalogPrepared,
                record.desired_presence,
                Some(catalog_hash.to_string()),
            )?;
        }
    }
    Ok(())
}

fn finalize_native_catalog_commit(root: &Path, catalog_hash: &str) -> Result<(), AppError> {
    for transaction in native_transaction_directories(root)? {
        let marker = transaction.join("transaction.json");
        if !marker.is_file() {
            continue;
        }
        let record: NativeLibraryTransaction = serde_json::from_slice(&fs::read(&marker)?)?;
        if matches!(record.phase, NativeLibraryTransactionPhase::CatalogPrepared) {
            if record.expected_catalog_hash.as_deref() != Some(catalog_hash) {
                return Err(AppError::StaleTarget);
            }
            write_native_transaction(
                &marker,
                Path::new(&record.destination),
                NativeLibraryTransactionPhase::CatalogCommitted,
                record.desired_presence,
                record.expected_catalog_hash,
            )?;
            if let Err(error) = cleanup_native_committed_transaction(&transaction) {
                log::warn!(
                    "Skill Library commit succeeded but cleanup remains pending at {}: {error}",
                    transaction.display()
                );
            }
        }
    }
    Ok(())
}

fn cleanup_native_committed_transaction(transaction: &Path) -> Result<(), AppError> {
    let stage = transaction.join("stage");
    if stage.exists() {
        fs::remove_dir_all(stage)?;
    }
    let backup = transaction.join("backup");
    if backup.exists() {
        fs::remove_dir_all(backup)?;
    }
    fs::remove_dir_all(transaction)?;
    Ok(())
}

fn native_transaction_directories(root: &Path) -> Result<Vec<PathBuf>, AppError> {
    let transactions = root.join(".transactions");
    let Ok(entries) = fs::read_dir(transactions) else {
        return Ok(Vec::new());
    };
    let mut directories = Vec::new();
    for entry in entries {
        let path = entry?.path();
        if path.is_dir() {
            directories.push(path);
        }
    }
    Ok(directories)
}

fn bytes_sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn validate_storage_component(value: &str) -> Result<(), AppError> {
    if value.is_empty() || matches!(value, "." | "..") || value.contains(['/', '\\', '\0']) {
        return Err(AppError::Validation {
            field: Some("libraryStorageComponent".to_string()),
            message: "invalid Skill Library storage component".to_string(),
        });
    }
    Ok(())
}

fn application_relative_path(scope: &SkillLocation) -> Result<PathBuf, AppError> {
    match scope {
        SkillLocation::Global => Ok(PathBuf::from("global.json")),
        SkillLocation::Project { project_id } => {
            validate_storage_component(project_id)?;
            Ok(PathBuf::from("projects").join(format!("{project_id}.json")))
        }
    }
}

struct ApplicationContexts {
    scopes: Vec<(String, SkillLocation)>,
    problems: Vec<ApplicationInventoryProblem>,
}

fn native_application_contexts(root: &Path) -> Result<ApplicationContexts, AppError> {
    let mut scopes = vec![("global".to_string(), SkillLocation::Global)];
    let mut problems = Vec::new();
    match fs::read_dir(root) {
        Ok(entries) => {
            for entry in entries {
                let entry = match entry {
                    Ok(entry) => entry,
                    Err(error) => {
                        problems.push(ApplicationInventoryProblem {
                            storage_key: "applications".to_string(),
                            error: error.into(),
                        });
                        continue;
                    }
                };
                let name = entry.file_name().to_string_lossy().into_owned();
                if name == "global.json" || name == "projects" {
                    continue;
                }
                problems.push(ApplicationInventoryProblem {
                    storage_key: name,
                    error: AppError::ConfigurationCorrupted {
                        message: "unknown Skill Library application entry".to_string(),
                    },
                });
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(ApplicationContexts { scopes, problems });
        }
        Err(error) => {
            problems.push(ApplicationInventoryProblem {
                storage_key: "applications".to_string(),
                error: error.into(),
            });
            return Ok(ApplicationContexts { scopes, problems });
        }
    }

    let projects = root.join("projects");
    match fs::read_dir(&projects) {
        Ok(entries) => {
            for entry in entries {
                let entry = match entry {
                    Ok(entry) => entry,
                    Err(error) => {
                        problems.push(ApplicationInventoryProblem {
                            storage_key: "projects".to_string(),
                            error: error.into(),
                        });
                        continue;
                    }
                };
                let name = entry.file_name();
                let display = name.to_string_lossy().into_owned();
                let project_id = Path::new(&name)
                    .file_stem()
                    .and_then(|value| value.to_str())
                    .filter(|_| {
                        Path::new(&name)
                            .extension()
                            .is_some_and(|value| value == "json")
                    });
                let valid = entry.file_type().is_ok_and(|kind| kind.is_file())
                    && project_id
                        .is_some_and(|project_id| validate_storage_component(project_id).is_ok());
                if let Some(project_id) = project_id.filter(|_| valid) {
                    scopes.push((
                        format!("projects/{display}"),
                        SkillLocation::Project {
                            project_id: project_id.to_string(),
                        },
                    ));
                } else {
                    problems.push(ApplicationInventoryProblem {
                        storage_key: format!("projects/{display}"),
                        error: AppError::ConfigurationCorrupted {
                            message: "invalid Skill Library application Scope key".to_string(),
                        },
                    });
                }
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => problems.push(ApplicationInventoryProblem {
            storage_key: "projects".to_string(),
            error: error.into(),
        }),
    }
    scopes.sort_by(|left, right| left.0.cmp(&right.0));
    problems.sort_by(|left, right| left.storage_key.cmp(&right.storage_key));
    Ok(ApplicationContexts { scopes, problems })
}

fn application_scope_key(scope: &SkillLocation) -> String {
    match scope {
        SkillLocation::Global => "global".to_string(),
        SkillLocation::Project { project_id } => format!("project:{project_id}"),
    }
}

fn wsl_application_locator(
    session: &crate::environment::wsl::WslSession,
    scope: &SkillLocation,
) -> Result<ResourceLocator, AppError> {
    let relative = application_relative_path(scope)?;
    Ok(ResourceLocator {
        environment: EnvironmentRef::Wsl {
            distro_name: session.distro_name.clone(),
        },
        native_path: format!(
            "{}/.skill-deck/skill-libraries/applications/{}",
            session.home.trim_end_matches('/'),
            relative.to_string_lossy().replace('\\', "/")
        ),
    })
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::application::library_application::{
        ApplicationRegistry, LibraryApplicationRecord, LibraryApplicationState,
    };
    use crate::application::skill_libraries::SkillLibraryModule;
    use crate::application::skill_libraries::LIBRARY_SCHEMA_VERSION;
    use crate::application::skill_paths::{SkillPathObserver, SkillTargetRequest};
    use crate::core::projects::ProjectMigrationState;
    use crate::core::skill_payload::build_skill_payload;
    use crate::environment::planning::RuntimeTargetFactResolver;
    use crate::environment::types::{
        ProjectInfo, ProjectStorageInfo, RegisteredProject, SkillLocation, SkillLocationRef,
        StorageAccess,
    };

    fn projects() -> Arc<ProjectMigrationRegistry> {
        Arc::new(ProjectMigrationRegistry::new(
            ProjectMigrationState::NotNeeded,
        ))
    }

    #[test]
    fn project_usage_candidate_preserves_registered_project_identity() {
        let project = ProjectInfo {
            binding: RegisteredProject {
                id: "project-1".to_string(),
                native_path: "/work/skill-deck".to_string(),
                display_name: Some("Skill Deck".to_string()),
                order: None,
                suppress_cross_storage_warning: false,
            },
            storage: ProjectStorageInfo {
                access: StorageAccess::Native,
                owner: Some(EnvironmentRef::Native),
            },
        };

        let (context, binding) = project_usage_candidate(&EnvironmentRef::Native, project.clone());

        assert_eq!(
            context,
            SkillLocationRef {
                environment: EnvironmentRef::Native,
                scope: SkillLocation::Project {
                    project_id: "project-1".to_string(),
                },
            }
        );
        assert_eq!(binding, Some(project.binding));
    }

    #[tokio::test]
    async fn library_io_coordinator_serializes_the_same_environment() {
        let coordinator = Arc::new(LibraryIoCoordinator::default());
        let first = coordinator.acquire(&EnvironmentRef::Native).await;
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (acquired_tx, mut acquired_rx) = tokio::sync::oneshot::channel();
        let contender = coordinator.clone();

        let task = tokio::spawn(async move {
            started_tx.send(()).unwrap();
            let _guard = contender.acquire(&EnvironmentRef::Native).await;
            acquired_tx.send(()).unwrap();
        });
        started_rx.await.unwrap();
        tokio::task::yield_now().await;

        assert!(matches!(
            acquired_rx.try_recv(),
            Err(tokio::sync::oneshot::error::TryRecvError::Empty)
        ));
        drop(first);
        acquired_rx.await.unwrap();
        task.await.unwrap();
    }

    #[tokio::test]
    async fn library_io_coordinator_keeps_different_environments_independent() {
        let coordinator = LibraryIoCoordinator::default();
        let _native = coordinator.acquire(&EnvironmentRef::Native).await;

        let wsl = tokio::time::timeout(
            std::time::Duration::from_secs(1),
            coordinator.acquire(&EnvironmentRef::Wsl {
                distro_name: "Ubuntu".to_string(),
            }),
        )
        .await;

        assert!(wsl.is_ok());
    }

    #[tokio::test]
    async fn library_io_coordinator_normalizes_wsl_environment_identity() {
        let coordinator = Arc::new(LibraryIoCoordinator::default());
        let first = coordinator
            .acquire(&EnvironmentRef::Wsl {
                distro_name: "Ubuntu".to_string(),
            })
            .await;
        let contender = coordinator.clone();
        let (acquired_tx, mut acquired_rx) = tokio::sync::oneshot::channel();

        let task = tokio::spawn(async move {
            let _guard = contender
                .acquire(&EnvironmentRef::Wsl {
                    distro_name: "ubuntu".to_string(),
                })
                .await;
            acquired_tx.send(()).unwrap();
        });
        tokio::task::yield_now().await;

        assert!(matches!(
            acquired_rx.try_recv(),
            Err(tokio::sync::oneshot::error::TryRecvError::Empty)
        ));
        drop(first);
        acquired_rx.await.unwrap();
        task.await.unwrap();
    }

    #[tokio::test]
    async fn native_repository_round_trips_a_library_catalog() {
        let temp = tempfile::tempdir().unwrap();
        let repository = Arc::new(RuntimeSkillLibraryRepository::new(
            temp.path().join("libraries"),
            Arc::new(WslRuntime::new_with_support(false, false)),
            projects(),
        ));
        let module = SkillLibraryModule::new(repository.clone());

        let created = module
            .create(EnvironmentRef::Native, "Backend".to_string())
            .await
            .expect("create");
        let reloaded = SkillLibraryModule::new(repository)
            .workspace(EnvironmentRef::Native)
            .await
            .expect("reload");

        assert_eq!(reloaded, created);
        assert!(fs::metadata(temp.path().join("libraries/catalog.json")).is_ok());
    }

    #[tokio::test]
    async fn native_repository_rejects_catalog_without_retired_members() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("libraries");
        fs::create_dir_all(&root).unwrap();
        fs::write(
            root.join("catalog.json"),
            br#"{
              "schemaVersion": 3,
              "libraries": [{
                "id": "backend",
                "name": "Backend",
                "skills": []
              }]
            }"#,
        )
        .unwrap();
        let repository = RuntimeSkillLibraryRepository::new(
            root,
            Arc::new(WslRuntime::new_with_support(false, false)),
            projects(),
        );

        let error = repository.load(&EnvironmentRef::Native).await.unwrap_err();
        assert!(matches!(
            error,
            AppError::ConfigurationCorrupted { message }
                if message.contains("Skill Library catalog")
                    && message.contains("retiredSkills")
        ));
    }

    #[tokio::test]
    async fn native_repository_deletes_one_library_as_a_single_intent() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("libraries");
        let repository = Arc::new(RuntimeSkillLibraryRepository::new(
            root.clone(),
            Arc::new(WslRuntime::new_with_support(false, false)),
            projects(),
        ));
        let created = SkillLibraryModule::new(repository.clone())
            .create(EnvironmentRef::Native, "Backend".to_string())
            .await
            .unwrap();
        let library_id = created.libraries[0].id.clone();

        let catalog = repository
            .delete_library(&EnvironmentRef::Native, &library_id)
            .await
            .unwrap();

        assert!(catalog.libraries.is_empty());
        assert!(!root.join("libraries").join(library_id.as_str()).exists());
        assert!(repository
            .load(&EnvironmentRef::Native)
            .await
            .unwrap()
            .libraries
            .is_empty());
    }

    #[tokio::test]
    async fn native_repository_restores_catalog_when_library_directory_deletion_fails() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("libraries");
        let repository = Arc::new(RuntimeSkillLibraryRepository::new(
            root.clone(),
            Arc::new(WslRuntime::new_with_support(false, false)),
            projects(),
        ));
        let created = SkillLibraryModule::new(repository.clone())
            .create(EnvironmentRef::Native, "Backend".to_string())
            .await
            .unwrap();
        let library_id = created.libraries[0].id.clone();
        let library_path = root.join("libraries").join(library_id.as_str());
        fs::remove_dir_all(&library_path).unwrap();
        fs::write(&library_path, b"not a directory").unwrap();

        assert!(repository
            .delete_library(&EnvironmentRef::Native, &library_id)
            .await
            .is_err());

        let catalog = repository.load(&EnvironmentRef::Native).await.unwrap();
        assert_eq!(catalog.libraries.len(), 1);
        assert_eq!(catalog.libraries[0].id, library_id);
        assert!(library_path.is_file());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn native_library_delete_stays_committed_when_backup_cleanup_is_blocked() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("libraries");
        let repository = Arc::new(RuntimeSkillLibraryRepository::new(
            root.clone(),
            Arc::new(WslRuntime::new_with_support(false, false)),
            projects(),
        ));
        let created = SkillLibraryModule::new(repository.clone())
            .create(EnvironmentRef::Native, "Backend".to_string())
            .await
            .unwrap();
        let library_id = created.libraries[0].id.clone();
        let library_path = root.join("libraries").join(library_id.as_str());
        let locked = library_path.join("skills/locked");
        fs::create_dir_all(&locked).unwrap();
        fs::write(locked.join("SKILL.md"), b"content").unwrap();
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o500)).unwrap();

        let result = repository
            .delete_library(&EnvironmentRef::Native, &library_id)
            .await;

        assert!(result.is_ok(), "delete must stay committed: {result:?}");
        assert!(repository
            .load(&EnvironmentRef::Native)
            .await
            .expect("load committed catalog")
            .libraries
            .is_empty());
        assert!(!library_path.exists());

        if let Ok(entries) = fs::read_dir(root.join(".transactions")) {
            for entry in entries.flatten() {
                let locked = entry.path().join("backup/skills/locked");
                if locked.exists() {
                    fs::set_permissions(locked, fs::Permissions::from_mode(0o700)).unwrap();
                }
            }
        }
    }

    #[tokio::test]
    async fn committed_cleanup_does_not_depend_on_a_later_destination_state() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("libraries");
        let transaction = root.join(".transactions/committed-cleanup");
        let destination = root.join("libraries/lib-one/skills/demo");
        fs::create_dir_all(transaction.join("backup")).unwrap();
        fs::write(transaction.join("backup/SKILL.md"), b"old").unwrap();
        fs::write(
            transaction.join("transaction.json"),
            serde_json::to_vec(&NativeLibraryTransaction {
                destination: destination.to_string_lossy().into_owned(),
                phase: NativeLibraryTransactionPhase::CatalogCommitted,
                desired_presence: true,
                expected_catalog_hash: Some("committed".to_string()),
            })
            .unwrap(),
        )
        .unwrap();
        let catalog = LibraryCatalog {
            schema_version: LIBRARY_SCHEMA_VERSION,
            libraries: Vec::new(),
            extra: serde_json::Map::new(),
        };
        fs::write(
            root.join("catalog.json"),
            serde_json::to_vec_pretty(&catalog).unwrap(),
        )
        .unwrap();
        let repository = RuntimeSkillLibraryRepository::new(
            root,
            Arc::new(WslRuntime::new_with_support(false, false)),
            projects(),
        );

        let loaded = repository.load(&EnvironmentRef::Native).await;

        assert!(loaded.is_ok());
        assert!(!transaction.exists());
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn native_library_delete_keeps_the_original_while_a_member_file_is_open() {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
        use windows_sys::Win32::Storage::FileSystem::{
            CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_GENERIC_READ, FILE_SHARE_READ, OPEN_EXISTING,
        };

        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("libraries");
        let repository = Arc::new(RuntimeSkillLibraryRepository::new(
            root.clone(),
            Arc::new(WslRuntime::new_with_support(false, false)),
            projects(),
        ));
        let created = SkillLibraryModule::new(repository.clone())
            .create(EnvironmentRef::Native, "Backend".to_string())
            .await
            .unwrap();
        let library_id = created.libraries[0].id.clone();
        let library_path = root.join("libraries").join(library_id.as_str());
        let member_file = library_path.join("skills/locked/SKILL.md");
        fs::create_dir_all(member_file.parent().unwrap()).unwrap();
        fs::write(&member_file, b"content").unwrap();
        let wide = member_file
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect::<Vec<_>>();
        let handle = unsafe {
            CreateFileW(
                wide.as_ptr(),
                FILE_GENERIC_READ,
                FILE_SHARE_READ,
                std::ptr::null(),
                OPEN_EXISTING,
                FILE_ATTRIBUTE_NORMAL,
                std::ptr::null_mut(),
            )
        };
        assert_ne!(handle, INVALID_HANDLE_VALUE);

        let result = repository
            .delete_library(&EnvironmentRef::Native, &library_id)
            .await;

        assert!(result.is_err(), "open member must block the directory move");
        let catalog = repository
            .load(&EnvironmentRef::Native)
            .await
            .expect("load original catalog");
        assert_eq!(catalog.libraries.len(), 1);
        assert_eq!(catalog.libraries[0].id, library_id);
        assert!(member_file.exists());

        unsafe { CloseHandle(handle) };
        repository
            .delete_library(&EnvironmentRef::Native, &library_id)
            .await
            .expect("delete after releasing the file");
        assert!(!library_path.exists());
    }

    #[tokio::test]
    async fn native_repository_rejects_deleting_an_unknown_library() {
        let temp = tempfile::tempdir().unwrap();
        let repository = RuntimeSkillLibraryRepository::new(
            temp.path().join("libraries"),
            Arc::new(WslRuntime::new_with_support(false, false)),
            projects(),
        );

        let error = repository
            .delete_library(
                &EnvironmentRef::Native,
                &LibraryId::parse("missing-library"),
            )
            .await
            .unwrap_err();

        assert_eq!(
            error,
            AppError::PathNotFound {
                path: "missing-library".to_string(),
            }
        );
    }

    #[tokio::test]
    async fn native_repository_rejects_an_unsupported_catalog_schema() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("libraries");
        fs::create_dir_all(&root).unwrap();
        fs::write(
            root.join("catalog.json"),
            br#"{"schemaVersion":999,"libraries":[]}"#,
        )
        .unwrap();
        let repository = RuntimeSkillLibraryRepository::new(
            root,
            Arc::new(WslRuntime::new_with_support(false, false)),
            projects(),
        );

        assert!(matches!(
            repository.load(&EnvironmentRef::Native).await,
            Err(AppError::ConfigurationCorrupted { .. })
        ));
    }

    #[tokio::test]
    async fn native_incomplete_recovery_blocks_library_reads_and_preserves_evidence() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("libraries");
        let broken = root.join(".transactions/broken");
        fs::create_dir_all(&broken).unwrap();
        let repository = RuntimeSkillLibraryRepository::new(
            root,
            Arc::new(WslRuntime::new_with_support(false, false)),
            projects(),
        );

        let error = repository.load(&EnvironmentRef::Native).await.unwrap_err();

        assert!(matches!(
            error,
            AppError::LibraryRecoveryIncomplete {
                environment: EnvironmentRef::Native,
                ..
            }
        ));
        assert!(broken.exists());
    }

    #[tokio::test]
    async fn native_conditional_commit_rejects_target_drift_without_writing() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("libraries");
        let repository = Arc::new(RuntimeSkillLibraryRepository::new(
            root.clone(),
            Arc::new(WslRuntime::new_with_support(false, false)),
            projects(),
        ));
        let created = SkillLibraryModule::new(repository.clone())
            .create(EnvironmentRef::Native, "Backend".to_string())
            .await
            .unwrap();
        let library_id = created.libraries[0].id.clone();
        let collection = repository
            .resolve_collection(&EnvironmentRef::Native, &library_id)
            .await
            .unwrap();
        let target = SkillPathObserver::resolve_skill_targets(
            &RuntimeTargetFactResolver::new(Arc::new(WslRuntime::default())),
            &collection,
            vec![SkillTargetRequest {
                skill_name: "demo".to_string(),
            }],
            None,
        )
        .await
        .unwrap()
        .pop()
        .unwrap();
        let catalog = repository.load(&EnvironmentRef::Native).await.unwrap();
        let snapshot = crate::application::collection_records::LibraryCatalogRecordReader::new(
            &catalog,
            &library_id,
        )
        .load_snapshot(std::collections::BTreeSet::from(["demo".to_string()]))
        .unwrap();
        let source = temp.path().join("source");
        fs::create_dir_all(&source).unwrap();
        fs::write(
            source.join("SKILL.md"),
            b"---\nname: demo\ndescription: New\n---\nnew\n",
        )
        .unwrap();
        let payload = build_skill_payload(&source).unwrap();
        let destination = PathBuf::from(&target.target.destination.native_path);
        fs::create_dir_all(&destination).unwrap();
        fs::write(destination.join("SKILL.md"), b"external").unwrap();

        let result = repository
            .commit_member(CommitLibraryMemberRequest {
                environment: EnvironmentRef::Native,
                library_id: library_id.clone(),
                skill_name: "demo".to_string(),
                expected: crate::application::skill_libraries::LibraryMemberCommitExpectation {
                    document_revision: snapshot.document_revision,
                    source_record_revision: snapshot.records[0].source_record_revision.clone(),
                    target_revision: target.target_revision,
                    content_revision: target.content_revision,
                },
                mutation: LibraryMemberMutation::Upsert {
                    content: Box::new(payload),
                    record: Box::new(crate::application::skill_libraries::LibrarySkillRecord {
                        name: "demo".to_string(),
                        description: "New".to_string(),
                        source_record: serde_json::json!({
                            "sourceType": "local",
                            "source": "/source",
                            "reacquisitionUrl": null,
                            "refName": null,
                            "skillPath": "demo",
                            "installedRevision": null,
                            "computedHash": "new",
                            "artifactUrl": null,
                            "pluginName": null,
                            "wellKnown": null
                        }),
                        content_manifest_hash: "new".to_string(),
                        updated_at: None,
                        extra: serde_json::Map::new(),
                    }),
                },
            })
            .await;

        assert_eq!(result.unwrap_err(), AppError::StaleTarget);
        assert_eq!(fs::read(destination.join("SKILL.md")).unwrap(), b"external");
        assert!(repository
            .load(&EnvironmentRef::Native)
            .await
            .unwrap()
            .libraries[0]
            .skills
            .is_empty());
    }

    #[tokio::test]
    async fn native_conditional_commit_upserts_and_retires_one_complete_member() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("libraries");
        let repository = Arc::new(RuntimeSkillLibraryRepository::new(
            root.clone(),
            Arc::new(WslRuntime::new_with_support(false, false)),
            projects(),
        ));
        let created = SkillLibraryModule::new(repository.clone())
            .create(EnvironmentRef::Native, "Backend".to_string())
            .await
            .unwrap();
        let library_id = created.libraries[0].id.clone();
        let source = temp.path().join("source");
        fs::create_dir_all(&source).unwrap();
        fs::write(
            source.join("SKILL.md"),
            b"---\nname: demo\ndescription: Demo\n---\nbody\n",
        )
        .unwrap();
        let manifest_hash = crate::environment::native::content_manifest::read_directory(&source)
            .unwrap()
            .hash()
            .as_str()
            .to_string();
        let payload = build_skill_payload(&source).unwrap();

        repository
            .commit_member(CommitLibraryMemberRequest {
                environment: EnvironmentRef::Native,
                library_id: library_id.clone(),
                skill_name: "demo".to_string(),
                expected: native_member_expectation(repository.as_ref(), &library_id, "demo").await,
                mutation: LibraryMemberMutation::Upsert {
                    content: Box::new(payload),
                    record: Box::new(crate::application::skill_libraries::LibrarySkillRecord {
                        name: "demo".to_string(),
                        description: "Demo".to_string(),
                        source_record: serde_json::json!({
                            "sourceType": "local",
                            "source": "/source",
                            "reacquisitionUrl": null,
                            "refName": null,
                            "skillPath": "demo",
                            "installedRevision": null,
                            "computedHash": "hash",
                            "artifactUrl": null,
                            "pluginName": null,
                            "wellKnown": null
                        }),
                        content_manifest_hash: manifest_hash,
                        updated_at: None,
                        extra: serde_json::Map::new(),
                    }),
                },
            })
            .await
            .unwrap();
        let destination = root
            .join("libraries")
            .join(library_id.as_str())
            .join("skills/demo");
        assert!(destination.join("SKILL.md").is_file());
        assert_eq!(
            repository
                .load(&EnvironmentRef::Native)
                .await
                .unwrap()
                .libraries[0]
                .skills
                .len(),
            1
        );

        repository
            .commit_member(CommitLibraryMemberRequest {
                environment: EnvironmentRef::Native,
                library_id: library_id.clone(),
                skill_name: "demo".to_string(),
                expected: native_member_expectation(repository.as_ref(), &library_id, "demo").await,
                mutation: LibraryMemberMutation::Retire {
                    retirement_id: crate::application::skill_libraries::RetirementId::parse(
                        "retirement-1",
                    ),
                    retired_at: "2026-09-06T00:00:00Z".to_string(),
                },
            })
            .await
            .unwrap();
        assert!(destination.join("SKILL.md").is_file());
        let catalog = repository.load(&EnvironmentRef::Native).await.unwrap();
        assert!(catalog.libraries[0].skills.is_empty());
        assert_eq!(catalog.libraries[0].retired_skills.len(), 1);

        repository
            .purge_retired(PurgeRetiredLibraryMemberRequest {
                environment: EnvironmentRef::Native,
                library_id: library_id.clone(),
                skill_name: "demo".to_string(),
                retirement_id: crate::application::skill_libraries::RetirementId::parse(
                    "retirement-1",
                ),
            })
            .await
            .unwrap();
        assert!(!destination.exists());
        assert!(repository
            .load(&EnvironmentRef::Native)
            .await
            .unwrap()
            .libraries[0]
            .retired_skills
            .is_empty());
    }

    async fn native_member_expectation(
        repository: &RuntimeSkillLibraryRepository,
        library_id: &crate::application::skill_libraries::LibraryId,
        skill_name: &str,
    ) -> crate::application::skill_libraries::LibraryMemberCommitExpectation {
        let collection = repository
            .resolve_collection(&EnvironmentRef::Native, library_id)
            .await
            .unwrap();
        let target = SkillPathObserver::resolve_skill_targets(
            &RuntimeTargetFactResolver::new(Arc::new(WslRuntime::default())),
            &collection,
            vec![SkillTargetRequest {
                skill_name: skill_name.to_string(),
            }],
            None,
        )
        .await
        .unwrap()
        .pop()
        .unwrap();
        let catalog = repository.load(&EnvironmentRef::Native).await.unwrap();
        let snapshot = crate::application::collection_records::LibraryCatalogRecordReader::new(
            &catalog, library_id,
        )
        .load_snapshot(std::collections::BTreeSet::from([skill_name.to_string()]))
        .unwrap();
        crate::application::skill_libraries::LibraryMemberCommitExpectation {
            document_revision: snapshot.document_revision,
            source_record_revision: snapshot.records[0].source_record_revision.clone(),
            target_revision: target.target_revision,
            content_revision: target.content_revision,
        }
    }

    #[tokio::test]
    async fn native_repository_keeps_global_and_project_applications_independent() {
        let temp = tempfile::tempdir().unwrap();
        let repository = RuntimeSkillLibraryRepository::new(
            temp.path().join("libraries"),
            Arc::new(WslRuntime::new_with_support(false, false)),
            projects(),
        );
        let global = SkillLocationRef {
            environment: EnvironmentRef::Native,
            scope: SkillLocation::Global,
        };
        let project = SkillLocationRef {
            environment: EnvironmentRef::Native,
            scope: SkillLocation::Project {
                project_id: "project-1".to_string(),
            },
        };
        let mut global_record = LibraryApplicationRecord::empty();
        global_record.current = LibraryApplicationState {
            ordered_library_ids: vec![LibraryId::parse("global-library")],
            selected_agent_ids: Vec::new(),
        };
        let mut project_record = LibraryApplicationRecord::empty();
        project_record.current = LibraryApplicationState {
            ordered_library_ids: vec![LibraryId::parse("project-library")],
            selected_agent_ids: Vec::new(),
        };

        let global_snapshot = repository.load_application(&global).await.unwrap();
        repository
            .save_application_if(&global_snapshot, &global_record)
            .await
            .unwrap();
        let project_snapshot = repository.load_application(&project).await.unwrap();
        repository
            .save_application_if(&project_snapshot, &project_record)
            .await
            .unwrap();

        assert_eq!(
            repository.load_application(&global).await.unwrap().record,
            global_record
        );
        assert_eq!(
            repository.load_application(&project).await.unwrap().record,
            project_record
        );
    }

    #[tokio::test]
    async fn project_application_removal_is_bound_to_the_loaded_snapshot() {
        let temp = tempfile::tempdir().unwrap();
        let repository = RuntimeSkillLibraryRepository::new(
            temp.path().join("libraries"),
            Arc::new(WslRuntime::new_with_support(false, false)),
            projects(),
        );
        let project = SkillLocationRef {
            environment: EnvironmentRef::Native,
            scope: SkillLocation::Project {
                project_id: "project-1".to_string(),
            },
        };
        let missing = repository.load_application(&project).await.unwrap();
        let saved = repository
            .save_application_if(&missing, &LibraryApplicationRecord::empty())
            .await
            .unwrap();
        fs::write(&saved.target.native_path, b"external change").unwrap();

        let error = repository.remove_application_if(&saved).await.unwrap_err();

        assert!(matches!(error, AppError::StaleTarget));
        assert_eq!(
            fs::read(&saved.target.native_path).unwrap(),
            b"external change"
        );
    }

    #[test]
    fn existing_schema_one_application_record_is_accepted() {
        let record = parse_library_application_record(
            br#"{
              "schemaVersion": 1,
              "current": { "orderedLibraryIds": [], "selectedAgentIds": [] },
              "checkpoint": { "members": [] },
              "pending": null
            }"#,
        )
        .expect("schema 1 is the current application record format");

        assert_eq!(record.schema_version, 1);
    }

    #[tokio::test]
    async fn application_record_accepts_unknown_route_fields_and_saves_canonical_schema() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("libraries");
        let applications = root.join("applications");
        fs::create_dir_all(&applications).unwrap();
        fs::write(
            applications.join("global.json"),
            br#"{
              "schemaVersion": 1,
              "target": {
                "environment": { "kind": "wsl", "distro_name": "Ubuntu" },
                "scope": { "scope": "global" }
              },
              "current": { "orderedLibraryIds": [], "selectedAgentIds": [] },
              "checkpoint": { "members": [] },
              "pending": null
            }"#,
        )
        .unwrap();
        let repository = RuntimeSkillLibraryRepository::new(
            root,
            Arc::new(WslRuntime::new_with_support(false, false)),
            projects(),
        );
        let context = SkillLocationRef {
            environment: EnvironmentRef::Native,
            scope: SkillLocation::Global,
        };

        let record = repository.load_application(&context).await.unwrap();
        assert_eq!(record.current, LibraryApplicationState::default());
        repository
            .save_application_if(&record, &record.record)
            .await
            .unwrap();

        let stored: serde_json::Value =
            serde_json::from_slice(&fs::read(applications.join("global.json")).unwrap()).unwrap();
        assert!(stored.get("target").is_none());
    }

    #[tokio::test]
    async fn application_record_without_checkpoint_is_rejected() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("libraries");
        let applications = root.join("applications");
        fs::create_dir_all(&applications).unwrap();
        fs::write(
            applications.join("global.json"),
            br#"{
              "schemaVersion": 1,
              "current": { "orderedLibraryIds": [], "selectedAgentIds": [] },
              "pendingOperation": null
            }"#,
        )
        .unwrap();
        let repository = RuntimeSkillLibraryRepository::new(
            root,
            Arc::new(WslRuntime::new_with_support(false, false)),
            projects(),
        );
        let context = SkillLocationRef {
            environment: EnvironmentRef::Native,
            scope: SkillLocation::Global,
        };

        let error = repository.load_application(&context).await.unwrap_err();
        assert!(matches!(
            error,
            AppError::ConfigurationCorrupted { message }
                if message.contains("Skill Library application record")
                    && message.contains("checkpoint")
        ));
    }

    #[tokio::test]
    async fn application_record_with_incomplete_pending_is_rejected() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("libraries");
        let applications = root.join("applications");
        fs::create_dir_all(&applications).unwrap();
        fs::write(
            applications.join("global.json"),
            br#"{
              "schemaVersion": 1,
              "current": { "orderedLibraryIds": [], "selectedAgentIds": [] },
              "checkpoint": { "members": [] },
              "pending": {
                "reconciliationId": "",
                "attention": "pending",
                "reasons": [],
                "beforeApplication": { "orderedLibraryIds": [], "selectedAgentIds": [] },
                "targetApplication": { "orderedLibraryIds": [], "selectedAgentIds": [] },
                "recognizedMembers": [],
                "targetMembers": [{ "libraryId": "library-a", "memberName": "demo" }]
              }
            }"#,
        )
        .unwrap();
        let repository = RuntimeSkillLibraryRepository::new(
            root,
            Arc::new(WslRuntime::new_with_support(false, false)),
            projects(),
        );
        let context = SkillLocationRef {
            environment: EnvironmentRef::Native,
            scope: SkillLocation::Global,
        };

        assert!(matches!(
            repository.load_application(&context).await,
            Err(AppError::ConfigurationCorrupted { .. })
        ));
    }

    #[tokio::test]
    async fn application_commit_rejects_a_changed_snapshot() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("libraries");
        let repository = RuntimeSkillLibraryRepository::new(
            root.clone(),
            Arc::new(WslRuntime::new_with_support(false, false)),
            projects(),
        );
        let context = SkillLocationRef {
            environment: EnvironmentRef::Native,
            scope: SkillLocation::Global,
        };
        let observed = repository.load_application(&context).await.unwrap();
        let external = LibraryApplicationRecord::empty();
        let path = root.join("applications/global.json");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, serde_json::to_vec_pretty(&external).unwrap()).unwrap();
        let mut replacement = LibraryApplicationRecord::empty();
        replacement.current.ordered_library_ids = vec![LibraryId::parse("library-a")];

        assert!(matches!(
            repository
                .save_application_if(&observed, &replacement)
                .await,
            Err(AppError::StaleTarget)
        ));
        assert_eq!(
            serde_json::from_slice::<LibraryApplicationRecord>(&fs::read(path).unwrap()).unwrap(),
            external
        );
    }

    #[tokio::test]
    async fn native_application_inventory_includes_orphans_and_reports_problems() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("libraries");
        let applications = root.join("applications");
        let project_records = applications.join("projects");
        fs::create_dir_all(&project_records).unwrap();
        let bytes = serde_json::to_vec_pretty(&LibraryApplicationRecord::empty()).unwrap();
        fs::write(applications.join("global.json"), &bytes).unwrap();
        fs::write(project_records.join("registered.json"), &bytes).unwrap();
        fs::write(project_records.join("orphan.json"), &bytes).unwrap();
        fs::write(project_records.join("broken.json"), b"{").unwrap();
        let mut future: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        future["schemaVersion"] = serde_json::json!(
            crate::application::library_application::LIBRARY_APPLICATION_SCHEMA_VERSION + 1
        );
        fs::write(
            project_records.join("future.json"),
            serde_json::to_vec_pretty(&future).unwrap(),
        )
        .unwrap();
        fs::create_dir(project_records.join("not-a-record.json")).unwrap();
        fs::write(applications.join("unexpected.json"), &bytes).unwrap();
        let repository = RuntimeSkillLibraryRepository::new(
            root,
            Arc::new(WslRuntime::new_with_support(false, false)),
            projects(),
        );

        let inventory = repository.enumerate(&EnvironmentRef::Native).await.unwrap();

        assert!(!inventory.complete);
        assert_eq!(inventory.records.len(), 3);
        assert!(inventory
            .records
            .iter()
            .any(|entry| { matches!(entry.context.scope, SkillLocation::Global) }));
        assert!(inventory.records.iter().any(|entry| {
            matches!(
                &entry.context.scope,
                SkillLocation::Project { project_id } if project_id == "registered"
            )
        }));
        assert!(inventory.records.iter().any(|entry| {
            matches!(
                &entry.context.scope,
                SkillLocation::Project { project_id } if project_id == "orphan"
            )
        }));
        assert_eq!(inventory.problems.len(), 4);

        let usage = repository
            .usage_projection(&EnvironmentRef::Native)
            .await
            .unwrap();
        assert!(!usage.inventory_complete);
        assert_eq!(usage.problem_count, 4);
    }

    #[tokio::test]
    async fn native_repository_restores_a_retired_member_if_delete_crashes_after_rename() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("libraries");
        let repository = RuntimeSkillLibraryRepository::new(
            root.clone(),
            Arc::new(WslRuntime::new_with_support(false, false)),
            projects(),
        );
        repository
            .save(&EnvironmentRef::Native, &LibraryCatalog::default())
            .await
            .unwrap();
        let destination = root.join("libraries/library-1/skills/demo");
        let transaction = root.join(".transactions/interrupted-delete");
        let backup = transaction.join("backup");
        fs::create_dir_all(destination.parent().unwrap()).unwrap();
        fs::create_dir_all(&backup).unwrap();
        fs::write(backup.join("SKILL.md"), b"retired content").unwrap();
        write_native_transaction(
            &transaction.join("transaction.json"),
            &destination,
            NativeLibraryTransactionPhase::BackedUp,
            false,
            None,
        )
        .unwrap();

        repository.load(&EnvironmentRef::Native).await.unwrap();

        assert_eq!(
            fs::read(destination.join("SKILL.md")).unwrap(),
            b"retired content"
        );
        assert!(!transaction.exists());
    }
}

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
    library_usage_state, LibraryApplicationFuture, LibraryApplicationRecord,
    LibraryApplicationRepository, LibraryUsageAccumulator, LIBRARY_APPLICATION_SCHEMA_VERSION,
};
use crate::application::payload_session::{
    PayloadLocalSource, PayloadSessionStorage, PayloadStorageKey,
};
use crate::application::skill_libraries::{
    validate_catalog, CommitLibraryMemberRequest, LibraryCatalog, LibraryFuture, LibraryId,
    LibraryMemberMutation, LibraryUsage, LibraryUsageProjection, LibraryUsageProvider,
    LibraryUsageState, SkillLibraryRepository,
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
                        .with_session_retry(&distro_name, move |session| {
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
            let catalog = bytes
                .map(|bytes| serde_json::from_slice(&bytes).map_err(AppError::from))
                .unwrap_or_else(|| Ok(LibraryCatalog::default()))?;
            validate_catalog(&catalog)?;
            Ok(catalog)
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
                        .with_session_retry(&distro_name, move |session| {
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
            for (context, project) in self.usage_candidates(environment).await? {
                let record = self.load_application(&context).await?;
                if let Some(state) = library_usage_state(&record, library_id) {
                    usages.push(LibraryUsage {
                        context,
                        project,
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
    ) -> LibraryFuture<'a, Result<Vec<LibraryUsageProjection>, AppError>> {
        Box::pin(async move {
            let mut accumulator = LibraryUsageAccumulator::default();
            for (context, _) in self.usage_candidates(environment).await? {
                accumulator.observe(&self.load_application(&context).await?);
            }
            Ok(accumulator.finish())
        })
    }

    fn agent_usages<'a>(
        &'a self,
        environment: &'a EnvironmentRef,
        agent_id: &'a crate::core::agent_definition::AgentId,
    ) -> LibraryFuture<'a, Result<Vec<LibraryUsage>, AppError>> {
        Box::pin(async move {
            let mut usages = Vec::new();
            for (context, project) in self.usage_candidates(environment).await? {
                let record = self.load_application(&context).await?;
                let state = if record.current.selected_agent_ids.contains(agent_id) {
                    Some(LibraryUsageState::Confirmed)
                } else if record.pending_operation.as_ref().is_some_and(|pending| {
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
                    usages.push(LibraryUsage {
                        context,
                        project,
                        state,
                    });
                }
            }
            Ok(usages)
        })
    }
}

impl LibraryApplicationRepository for RuntimeSkillLibraryRepository {
    fn load_application<'a>(
        &'a self,
        context: &'a SkillLocationRef,
    ) -> LibraryApplicationFuture<'a, Result<LibraryApplicationRecord, AppError>> {
        Box::pin(async move {
            let bytes = match &context.environment {
                EnvironmentRef::Native => {
                    NativeAtomicDocumentIo
                        .read_optional(&self.native_application(context)?)
                        .await?
                }
                EnvironmentRef::Wsl { distro_name } => {
                    let distro_name = distro_name.clone();
                    let context = context.clone();
                    let workspace = self.wsl.workspace(&distro_name)?;
                    self.wsl
                        .with_session_retry(&distro_name, move |session| {
                            let context = context.clone();
                            let workspace = workspace.clone();
                            async move {
                                let target = wsl_application_locator(&session, &context.scope)?;
                                workspace
                                    .read_optional_document(
                                        target.native_path,
                                        environment_protocol::MAX_DOCUMENT_BYTES,
                                    )
                                    .await
                            }
                        })
                        .await?
                }
            };
            let record = bytes
                .map(|bytes| serde_json::from_slice(&bytes).map_err(AppError::from))
                .unwrap_or_else(|| Ok(LibraryApplicationRecord::empty()))?;
            if record.schema_version != LIBRARY_APPLICATION_SCHEMA_VERSION {
                return Err(AppError::ConfigurationCorrupted {
                    message: "invalid Skill Library application record".to_string(),
                });
            }
            Ok(record)
        })
    }

    fn save_application<'a>(
        &'a self,
        context: &'a SkillLocationRef,
        record: &'a LibraryApplicationRecord,
    ) -> LibraryApplicationFuture<'a, Result<(), AppError>> {
        Box::pin(async move {
            let bytes = serde_json::to_vec_pretty(record)?;
            match &context.environment {
                EnvironmentRef::Native => {
                    NativeAtomicDocumentIo
                        .write_atomic(&self.native_application(context)?, bytes)
                        .await
                }
                EnvironmentRef::Wsl { distro_name } => {
                    let distro_name = distro_name.clone();
                    let scope = context.scope.clone();
                    let workspace = self.wsl.workspace(&distro_name)?;
                    self.wsl
                        .with_session(&distro_name, move |session| {
                            let bytes = bytes.clone();
                            let scope = scope.clone();
                            let workspace = workspace.clone();
                            async move {
                                let target = wsl_application_locator(&session, &scope)?;
                                let snapshot = workspace
                                    .read_optional_document_snapshot_once(
                                        target.native_path.clone(),
                                        environment_protocol::MAX_DOCUMENT_BYTES,
                                    )
                                    .await?;
                                workspace
                                    .write_document_atomic(
                                        snapshot.generation,
                                        target.native_path,
                                        snapshot.revision,
                                        bytes,
                                    )
                                    .await
                                    .map(|_| ())
                            }
                        })
                        .await
                }
            }
        })
    }

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
                        .with_session_retry(&distro_name, move |session| {
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

    fn remove_application<'a>(
        &'a self,
        context: &'a SkillLocationRef,
    ) -> LibraryApplicationFuture<'a, Result<(), AppError>> {
        Box::pin(async move {
            let SkillLocation::Project { project_id } = &context.scope else {
                return Err(AppError::Validation {
                    field: Some("context".to_string()),
                    message: "only Project Skill Library applications can be removed".to_string(),
                });
            };
            validate_storage_component(project_id)?;
            match &context.environment {
                EnvironmentRef::Native => {
                    let path = PathBuf::from(self.native_application(context)?.native_path);
                    tokio::task::spawn_blocking(move || match fs::remove_file(path) {
                        Ok(()) => Ok(()),
                        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
                        Err(error) => Err(AppError::from(error)),
                    })
                    .await
                    .map_err(|error| AppError::ExecutionFailed {
                        message: format!("Skill Library application cleanup task failed: {error}"),
                    })?
                }
                EnvironmentRef::Wsl { distro_name } => {
                    let distro_name = distro_name.clone();
                    let scope = context.scope.clone();
                    let workspace = self.wsl.workspace(&distro_name)?;
                    self.wsl
                        .with_session(&distro_name, move |session| {
                            let scope = scope.clone();
                            let workspace = workspace.clone();
                            async move {
                                let target = wsl_application_locator(&session, &scope)?;
                                let snapshot = workspace
                                    .read_optional_document_snapshot_once(
                                        target.native_path.clone(),
                                        environment_protocol::MAX_DOCUMENT_BYTES,
                                    )
                                    .await?;
                                workspace
                                    .remove_document_if_revision(
                                        snapshot.generation,
                                        target.native_path,
                                        snapshot.revision,
                                    )
                                    .await
                            }
                        })
                        .await
                }
            }
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
    let mut catalog: LibraryCatalog = serde_json::from_slice(&original_bytes)?;
    validate_catalog(&catalog)?;
    remove_catalog_library(&mut catalog, library_id.as_str())?;
    let updated_bytes = serde_json::to_vec_pretty(&catalog)?;
    crate::environment::native::atomic_file::write_native_atomic(
        &root.join("catalog.json"),
        &updated_bytes,
    )?;
    let destination = root.join("libraries").join(library_id.as_str());
    match fs::remove_dir_all(destination) {
        Ok(()) => Ok(catalog),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(catalog),
        Err(error) => {
            crate::environment::native::atomic_file::write_native_atomic(
                &root.join("catalog.json"),
                &original_bytes,
            )?;
            Err(error.into())
        }
    }
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
    let mut catalog: LibraryCatalog = serde_json::from_slice(&original_bytes)?;
    validate_catalog(&catalog)?;
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
        .map(serde_json::from_slice)
        .transpose()?
        .unwrap_or_default();
    validate_catalog(&catalog)?;
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
    let library = catalog
        .libraries
        .iter_mut()
        .find(|library| library.id == request.library_id)
        .ok_or_else(|| AppError::PathNotFound {
            path: request.library_id.as_str().to_string(),
        })?;
    match &request.mutation {
        LibraryMemberMutation::Upsert { record, .. } => {
            let mut record = (**record).clone();
            if let Some(current) = library
                .skills
                .iter()
                .find(|skill| skill.name == request.skill_name)
            {
                record.extra = current.extra.clone();
                crate::application::skill_libraries::merge_unknown_source_fields(
                    &mut record.source_record,
                    &current.source_record,
                );
            }
            if let Some(current) = library
                .skills
                .iter_mut()
                .find(|skill| skill.name == request.skill_name)
            {
                *current = record;
            } else {
                library.skills.push(record);
                library
                    .skills
                    .sort_by(|left, right| left.name.cmp(&right.name));
            }
        }
        LibraryMemberMutation::Delete => {
            let before = library.skills.len();
            library
                .skills
                .retain(|skill| skill.name != request.skill_name);
            if before == library.skills.len() {
                return Err(AppError::StaleTarget);
            }
        }
    }
    let catalog_bytes = serde_json::to_vec_pretty(&catalog)?;
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
        LibraryMemberMutation::Delete => environment_protocol::LibraryMemberAction::Delete,
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
        .map(serde_json::from_slice)
        .transpose()?
        .unwrap_or_default();
    validate_catalog(&catalog)?;
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

    let library = catalog
        .libraries
        .iter_mut()
        .find(|library| library.id == request.library_id)
        .ok_or_else(|| AppError::PathNotFound {
            path: request.library_id.as_str().to_string(),
        })?;
    match &request.mutation {
        LibraryMemberMutation::Upsert { record, .. } => {
            let mut record = (**record).clone();
            if let Some(current) = library
                .skills
                .iter()
                .find(|skill| skill.name == request.skill_name)
            {
                record.extra = current.extra.clone();
                crate::application::skill_libraries::merge_unknown_source_fields(
                    &mut record.source_record,
                    &current.source_record,
                );
            }
            if let Some(current) = library
                .skills
                .iter_mut()
                .find(|skill| skill.name == request.skill_name)
            {
                *current = record;
            } else {
                library.skills.push(record);
                library
                    .skills
                    .sort_by(|left, right| left.name.cmp(&right.name));
            }
        }
        LibraryMemberMutation::Delete => {
            let before = library.skills.len();
            library
                .skills
                .retain(|skill| skill.name != request.skill_name);
            if before == library.skills.len() {
                return Err(AppError::StaleTarget);
            }
        }
    }
    let catalog_bytes = serde_json::to_vec_pretty(&catalog)?;
    let catalog_hash = bytes_sha256(&catalog_bytes);

    let commit = (|| {
        match &request.mutation {
            LibraryMemberMutation::Upsert { content, .. } => {
                replace_native_skill(root, &destination, content)?;
            }
            LibraryMemberMutation::Delete => {
                stage_native_skill_deletion(root, &destination)?;
            }
        }
        prepare_native_catalog_commit(root, &catalog_hash)?;
        crate::environment::native::atomic_file::write_native_atomic(
            &root.join("catalog.json"),
            &catalog_bytes,
        )?;
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
            NativeLibraryTransactionPhase::CatalogCommitted
                if destination.exists() == record.desired_presence => {}
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
            let backup = transaction.join("backup");
            if backup.exists() {
                fs::remove_dir_all(backup)?;
            }
            fs::remove_dir_all(transaction)?;
        }
    }
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
        LibraryApplicationRecord, LibraryApplicationRepository, LibraryApplicationState,
    };
    use crate::application::skill_libraries::SkillLibraryModule;
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
    async fn native_conditional_commit_upserts_and_deletes_one_complete_member() {
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
                        content_manifest_hash: "hash".to_string(),
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
                mutation: LibraryMemberMutation::Delete,
            })
            .await
            .unwrap();
        assert!(!destination.exists());
        assert!(repository
            .load(&EnvironmentRef::Native)
            .await
            .unwrap()
            .libraries[0]
            .skills
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

        repository
            .save_application(&global, &global_record)
            .await
            .unwrap();
        repository
            .save_application(&project, &project_record)
            .await
            .unwrap();

        assert_eq!(
            repository.load_application(&global).await.unwrap(),
            global_record
        );
        assert_eq!(
            repository.load_application(&project).await.unwrap(),
            project_record
        );
    }

    #[tokio::test]
    async fn application_record_storage_uses_its_repository_context() {
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

        let record = repository.load_application(&context).await.unwrap();
        assert_eq!(record.current, LibraryApplicationState::default());
        repository
            .save_application(&context, &record)
            .await
            .unwrap();

        let stored: serde_json::Value =
            serde_json::from_slice(&fs::read(applications.join("global.json")).unwrap()).unwrap();
        assert!(stored.get("target").is_none());
    }
}

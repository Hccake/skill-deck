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
use crate::environment::types::{
    EnvironmentKey, EnvironmentRef, ProjectInfo, RegisteredProject, ResourceLocator,
};
use crate::environment::types::{SkillLocation, SkillLocationRef};
use crate::environment::wsl::operations::atomic_file::WslAtomicDocumentIo;
use crate::environment::wsl::operations::library_content::{
    ensure_library_roots, finalize_library_catalog, prepare_library_catalog,
    recover_library_content, remove_library as remove_wsl_library, remove_library_application,
    replace_library_skill, stage_library_skill_deletion,
};
use crate::environment::wsl::WslRuntime;
use crate::error::AppError;
use crate::storage::atomic_document::AtomicDocumentIo;

#[derive(Default)]
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
                    let distro_name = distro_name.clone();
                    self.wsl
                        .with_session_retry(&distro_name, |session| async move {
                            recover_wsl_library_content(&session).await?;
                            let target = wsl_catalog_locator(&session);
                            WslAtomicDocumentIo::from_active_session(session.clone())
                                .read_optional(&target)
                                .await
                        })
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
            let catalog_hash = bytes_sha256(&bytes);
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
                    let distro_name = distro_name.clone();
                    self.wsl
                        .with_session_retry(&distro_name, move |session| {
                            let bytes = bytes.clone();
                            let catalog_hash = catalog_hash.clone();
                            let library_ids = catalog
                                .libraries
                                .iter()
                                .map(|library| library.id.as_str().to_string())
                                .collect::<Vec<_>>();
                            async move {
                                let result = async {
                                    recover_wsl_library_content(&session).await?;
                                    ensure_library_roots(&session, &library_ids).await?;
                                    prepare_library_catalog(&session, &catalog_hash).await?;
                                    let target = wsl_catalog_locator(&session);
                                    WslAtomicDocumentIo::from_active_session(session.clone())
                                        .write_atomic(&target, bytes)
                                        .await?;
                                    finalize_library_catalog(&session, &catalog_hash).await
                                }
                                .await;
                                let result = if result.is_err() {
                                    recover_wsl_library_content(&session).await.and(result)
                                } else {
                                    result
                                };
                                result
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
                    self.wsl
                        .with_session_retry(&distro_name, move |session| {
                            let library_id = library_id.clone();
                            let skill_name = skill_name.clone();
                            async move {
                                let path = format!(
                                    "{}/.skill-deck/skill-libraries/libraries/{}/skills/{}",
                                    session.home.trim_end_matches('/'),
                                    library_id,
                                    skill_name
                                );
                                let markdown = crate::environment::wsl::operations::skill_content::read_skill_markdown(&session, &path).await?;
                                Ok(crate::core::skill::skill_content_from_markdown(&markdown))
                            }
                        })
                        .await
                }
            }
        })
    }
}


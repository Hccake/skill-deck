use serde::Serialize;
use specta::Type;
#[cfg(test)]
use std::future::Future;

use crate::core::app_config::get_config_path;
use crate::core::projects::{
    add_project_binding, migrate_legacy_projects, normalize_project_native_path,
    remove_project_binding, set_project_cross_storage_warning_suppressed, ProjectMigrationRegistry,
    ProjectMigrationState, ProjectPathSemantics, ProjectsStore,
};
use crate::environment::path_mapping::{
    map_windows_path_with_wslpath, map_wsl_input_without_wslpath, windows_storage_owner,
    WindowsStorageOwner,
};
use crate::environment::types::{
    AddProjectResult, EnvironmentRef, EnvironmentStatus, ProjectInfo, ProjectStorageInfo,
    RegisteredProject, StorageAccess,
};
use crate::environment::wsl::operations::projects;
use crate::environment::wsl::{WslRuntime, WslSession};
use crate::error::AppError;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct EnvironmentInfo {
    pub environment: EnvironmentRef,
    pub display_name: String,
    pub status: EnvironmentStatus,
    pub revision: u64,
    pub error: Option<AppError>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct EnvironmentDiscoverySnapshot {
    pub environments: Vec<EnvironmentInfo>,
    pub error: Option<AppError>,
    pub wsl_integration_supported: bool,
    pub wsl_integration_enabled: bool,
    pub wsl_capability_revision: u64,
}

pub fn native_environment_info() -> EnvironmentInfo {
    let display_name = match std::env::consts::OS {
        "windows" => "Windows",
        "macos" => "macOS",
        "linux" => "Linux",
        other => other,
    };
    EnvironmentInfo {
        environment: EnvironmentRef::Native,
        display_name: display_name.to_string(),
        status: EnvironmentStatus::Available,
        revision: 0,
        error: None,
    }
}

fn environment_infos_from_wsl_discovery(
    discovery: Result<Vec<String>, AppError>,
    registry: &WslRuntime,
) -> EnvironmentDiscoverySnapshot {
    let mut environments = vec![native_environment_info()];
    let error = match discovery {
        Ok(distributions) => {
            environments.extend(
                distributions
                    .into_iter()
                    .filter(|distro_name| {
                        !["docker-desktop", "docker-desktop-data"]
                            .iter()
                            .any(|internal| distro_name.eq_ignore_ascii_case(internal))
                    })
                    .map(|distro_name| {
                        let runtime = registry.runtime_status(&distro_name);
                        EnvironmentInfo {
                            display_name: distro_name.clone(),
                            environment: EnvironmentRef::Wsl { distro_name },
                            status: runtime
                                .as_ref()
                                .map(|runtime| runtime.status)
                                .unwrap_or(EnvironmentStatus::Available),
                            revision: runtime
                                .as_ref()
                                .map(|runtime| runtime.revision)
                                .unwrap_or(0),
                            error: runtime.and_then(|runtime| runtime.error),
                        }
                    }),
            );
            None
        }
        Err(error) => {
            log::warn!("WSL discovery unavailable; continuing with native only: {error}");
            Some(error)
        }
    };
    EnvironmentDiscoverySnapshot {
        environments,
        error,
        wsl_integration_supported: cfg!(target_os = "windows"),
        wsl_integration_enabled: cfg!(target_os = "windows") && registry.wsl_integration_enabled(),
        wsl_capability_revision: registry.capability_revision(),
    }
}

#[cfg(test)]
async fn list_environments_with<Discover, DiscoveryFuture>(
    registry: &WslRuntime,
    wsl_integration_supported: bool,
    discover: Discover,
) -> EnvironmentDiscoverySnapshot
where
    Discover: FnOnce() -> DiscoveryFuture,
    DiscoveryFuture: Future<Output = Result<Vec<String>, AppError>>,
{
    if !wsl_integration_supported || !registry.wsl_integration_enabled() {
        return native_only_environment_snapshot(
            wsl_integration_supported,
            registry.capability_revision(),
        );
    }

    let discovered = registry.discover_using(discover).await;
    if !registry.wsl_integration_enabled() {
        return native_only_environment_snapshot(
            wsl_integration_supported,
            registry.capability_revision(),
        );
    }
    environment_infos_from_wsl_discovery(discovered, registry)
}

fn native_only_environment_snapshot(
    wsl_integration_supported: bool,
    wsl_capability_revision: u64,
) -> EnvironmentDiscoverySnapshot {
    EnvironmentDiscoverySnapshot {
        environments: vec![native_environment_info()],
        error: None,
        wsl_integration_supported,
        wsl_integration_enabled: false,
        wsl_capability_revision,
    }
}

pub async fn list_environments(
    registry: &WslRuntime,
) -> Result<EnvironmentDiscoverySnapshot, AppError> {
    let supported = cfg!(target_os = "windows");
    if !supported || !registry.wsl_integration_enabled() {
        return Ok(native_only_environment_snapshot(
            supported,
            registry.capability_revision(),
        ));
    }
    let discovered = registry.discover().await;
    if !registry.wsl_integration_enabled() {
        return Ok(native_only_environment_snapshot(
            supported,
            registry.capability_revision(),
        ));
    }
    Ok(environment_infos_from_wsl_discovery(discovered, registry))
}

pub async fn connect_environment(
    distro_name: String,
    registry: &WslRuntime,
) -> Result<EnvironmentInfo, AppError> {
    let session = registry.connect(&distro_name).await?;
    let runtime = registry.runtime_status(&distro_name);
    Ok(EnvironmentInfo {
        environment: EnvironmentRef::Wsl {
            distro_name: distro_name.clone(),
        },
        display_name: distro_name,
        status: runtime
            .as_ref()
            .map(|runtime| runtime.status)
            .unwrap_or(EnvironmentStatus::Available),
        revision: runtime
            .as_ref()
            .map(|runtime| runtime.revision)
            .unwrap_or(session.runtime_generation),
        error: runtime.and_then(|runtime| runtime.error),
    })
}

pub async fn map_environment_path(
    environment: EnvironmentRef,
    path: String,
    registry: &WslRuntime,
) -> Result<String, AppError> {
    match environment {
        EnvironmentRef::Native => Ok(normalize_project_native_path(
            &path,
            ProjectPathSemantics::native(),
        )),
        EnvironmentRef::Wsl { distro_name } => {
            if let Some(mapped) = registry.map_input_without_process(&distro_name, &path)? {
                return Ok(mapped);
            }
            let workspace = registry.workspace(&distro_name)?;
            map_windows_path_with_wslpath(&workspace, &path).await
        }
    }
}

pub(crate) fn native_projects_store() -> Result<ProjectsStore, AppError> {
    let config_path = get_config_path()?;
    Ok(native_projects_store_from_config(&config_path))
}

fn native_projects_store_from_config(config_path: &std::path::Path) -> ProjectsStore {
    ProjectsStore::new(config_path.with_file_name("projects.json"))
}

fn run_native_project_migration() -> Result<ProjectMigrationState, AppError> {
    let config_path = get_config_path()?;
    let projects_path = config_path.with_file_name("projects.json");
    migrate_legacy_projects(&config_path, &projects_path)
}

pub(crate) fn initialize_native_project_migration() -> ProjectMigrationRegistry {
    let state = run_native_project_migration()
        .unwrap_or_else(|error| ProjectMigrationState::Failed { error });
    ProjectMigrationRegistry::new(state)
}

fn ensure_native_projects_ready(
    migration: &ProjectMigrationRegistry,
) -> Result<ProjectsStore, AppError> {
    migration.ensure_ready()?;
    native_projects_store()
}

fn native_project_info_for_platform(binding: RegisteredProject, windows: bool) -> ProjectInfo {
    let (access, owner) = if windows {
        match windows_storage_owner(&binding.native_path) {
            WindowsStorageOwner::Windows => (StorageAccess::Native, Some(EnvironmentRef::Native)),
            WindowsStorageOwner::Wsl { distro_name } => (
                StorageAccess::CrossStorage,
                Some(EnvironmentRef::Wsl { distro_name }),
            ),
            WindowsStorageOwner::Unknown => (StorageAccess::Unknown, None),
        }
    } else if binding.native_path.starts_with('/') {
        (StorageAccess::Native, Some(EnvironmentRef::Native))
    } else {
        (StorageAccess::Unknown, None)
    };
    ProjectInfo {
        binding,
        storage: ProjectStorageInfo { access, owner },
    }
}

fn native_project_info(binding: RegisteredProject) -> ProjectInfo {
    native_project_info_for_platform(binding, cfg!(target_os = "windows"))
}

fn native_project_infos(bindings: Vec<RegisteredProject>) -> Vec<ProjectInfo> {
    bindings.into_iter().map(native_project_info).collect()
}

fn updated_native_project(
    bindings: Vec<RegisteredProject>,
    project_id: &str,
) -> Result<ProjectInfo, AppError> {
    bindings
        .into_iter()
        .find(|project| project.id == project_id)
        .map(native_project_info)
        .ok_or_else(|| AppError::PathNotFound {
            path: project_id.to_string(),
        })
}

async fn wsl_project_infos(
    session: &WslSession,
    workspace: &crate::environment::wsl::WslWorkspace,
    bindings: Vec<RegisteredProject>,
) -> Result<Vec<ProjectInfo>, AppError> {
    projects::project_infos(session, workspace, bindings).await
}

pub(crate) async fn read_wsl_projects(
    session: &WslSession,
    workspace: &crate::environment::wsl::WslWorkspace,
) -> Result<Vec<crate::environment::types::RegisteredProject>, AppError> {
    projects::read_projects(session, workspace).await
}

async fn write_wsl_projects(
    session: &WslSession,
    workspace: &crate::environment::wsl::WslWorkspace,
    projects: Vec<crate::environment::types::RegisteredProject>,
    generation: u64,
    expected_revision: Option<String>,
) -> Result<(), AppError> {
    projects::write_projects(session, workspace, projects, generation, expected_revision).await
}

pub async fn list_environment_projects(
    environment: EnvironmentRef,
    registry: &WslRuntime,
    migration: &ProjectMigrationRegistry,
) -> Result<Vec<ProjectInfo>, AppError> {
    match environment {
        EnvironmentRef::Native => Ok(native_project_infos(
            ensure_native_projects_ready(migration)?.read()?,
        )),
        EnvironmentRef::Wsl { distro_name } => {
            let workspace = registry.workspace(&distro_name)?;
            registry
                .with_session_retry(&distro_name, move |session| {
                    let workspace = workspace.clone();
                    async move {
                        let projects = read_wsl_projects(&session, &workspace).await?;
                        wsl_project_infos(&session, &workspace, projects).await
                    }
                })
                .await
        }
    }
}

pub async fn add_environment_project(
    environment: EnvironmentRef,
    native_path: String,
    registry: &WslRuntime,
    migration: &ProjectMigrationRegistry,
) -> Result<AddProjectResult, AppError> {
    match environment {
        EnvironmentRef::Native => {
            let result = ensure_native_projects_ready(migration)?.add(native_path)?;
            Ok(AddProjectResult {
                project: native_project_info(result.project),
                created: result.created,
            })
        }
        EnvironmentRef::Wsl { distro_name } => {
            let workspace = registry.workspace(&distro_name)?;
            registry
                .with_session(&distro_name, move |session| {
                    let native_path = native_path.clone();
                    let workspace = workspace.clone();
                    async move {
                        let native_path = match map_wsl_input_without_wslpath(
                            &session.distro_name,
                            &native_path,
                        )? {
                            Some(mapped) => mapped,
                            None => map_windows_path_with_wslpath(&workspace, &native_path).await?,
                        };
                        let snapshot =
                            projects::read_projects_snapshot(&session, &workspace).await?;
                        let result = add_project_binding(
                            snapshot.projects,
                            native_path,
                            ProjectPathSemantics::Posix,
                        );
                        let project =
                            wsl_project_infos(&session, &workspace, vec![result.project.clone()])
                                .await?
                                .pop()
                                .expect("one project info");
                        if result.created {
                            write_wsl_projects(
                                &session,
                                &workspace,
                                result.projects,
                                snapshot.generation,
                                snapshot.revision,
                            )
                            .await?;
                        }
                        Ok(AddProjectResult {
                            project,
                            created: result.created,
                        })
                    }
                })
                .await
        }
    }
}

pub async fn remove_environment_project(
    environment: EnvironmentRef,
    project_id: String,
    registry: &WslRuntime,
    migration: &ProjectMigrationRegistry,
) -> Result<Vec<ProjectInfo>, AppError> {
    match environment {
        EnvironmentRef::Native => Ok(native_project_infos(
            ensure_native_projects_ready(migration)?.remove(&project_id)?,
        )),
        EnvironmentRef::Wsl { distro_name } => {
            let workspace = registry.workspace(&distro_name)?;
            registry
                .with_session(&distro_name, move |session| {
                    let project_id = project_id.clone();
                    let workspace = workspace.clone();
                    async move {
                        let snapshot =
                            projects::read_projects_snapshot(&session, &workspace).await?;
                        let projects = remove_project_binding(snapshot.projects, &project_id);
                        let infos =
                            wsl_project_infos(&session, &workspace, projects.clone()).await?;
                        write_wsl_projects(
                            &session,
                            &workspace,
                            projects,
                            snapshot.generation,
                            snapshot.revision,
                        )
                        .await?;
                        Ok(infos)
                    }
                })
                .await
        }
    }
}

pub async fn set_environment_project_cross_storage_warning(
    environment: EnvironmentRef,
    project_id: String,
    suppressed: bool,
    registry: &WslRuntime,
    migration: &ProjectMigrationRegistry,
) -> Result<ProjectInfo, AppError> {
    match environment {
        EnvironmentRef::Native => {
            let projects = ensure_native_projects_ready(migration)?
                .set_cross_storage_warning_suppressed(&project_id, suppressed)?;
            updated_native_project(projects, &project_id)
        }
        EnvironmentRef::Wsl { distro_name } => {
            let workspace = registry.workspace(&distro_name)?;
            registry
                .with_session(&distro_name, move |session| {
                    let project_id = project_id.clone();
                    let workspace = workspace.clone();
                    async move {
                        let snapshot =
                            projects::read_projects_snapshot(&session, &workspace).await?;
                        let projects = set_project_cross_storage_warning_suppressed(
                            snapshot.projects,
                            &project_id,
                            suppressed,
                        );
                        let project = wsl_project_infos(&session, &workspace, projects.clone())
                            .await?
                            .into_iter()
                            .find(|project| project.binding.id == project_id)
                            .ok_or_else(|| AppError::PathNotFound {
                                path: project_id.clone(),
                            })?;
                        write_wsl_projects(
                            &session,
                            &workspace,
                            projects,
                            snapshot.generation,
                            snapshot.revision,
                        )
                        .await?;
                        Ok(project)
                    }
                })
                .await
        }
    }
}

pub fn retry_native_project_migration(
    migration: &ProjectMigrationRegistry,
) -> Result<Vec<ProjectInfo>, AppError> {
    match run_native_project_migration() {
        Ok(state) => {
            migration.set(state);
            Ok(native_project_infos(native_projects_store()?.read()?))
        }
        Err(error) => {
            let message = error.to_string();
            migration.set(ProjectMigrationState::Failed { error });
            Err(AppError::ProjectMigrationFailed { message })
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    use tempfile::tempdir;

    use super::{
        environment_infos_from_wsl_discovery, list_environments_with, map_environment_path,
        native_environment_info, native_project_info_for_platform,
        native_projects_store_from_config,
    };
    use crate::environment::types::{
        EnvironmentRef, EnvironmentStatus, RegisteredProject, StorageAccess,
    };
    use crate::environment::wsl::WslRuntime;
    use crate::error::AppError;

    #[test]
    fn native_environment_is_always_available() {
        let native = native_environment_info();
        assert_eq!(native.environment, EnvironmentRef::Native);
        assert_eq!(native.status, EnvironmentStatus::Available);
        let expected_name = match std::env::consts::OS {
            "windows" => "Windows",
            "macos" => "macOS",
            "linux" => "Linux",
            other => other,
        };
        assert_eq!(native.display_name, expected_name);
    }

    fn project(id: &str, native_path: &str) -> RegisteredProject {
        RegisteredProject {
            id: id.to_string(),
            native_path: native_path.to_string(),
            display_name: None,
            order: None,
            suppress_cross_storage_warning: false,
        }
    }

    #[test]
    fn native_project_storage_uses_platform_and_wsl_unc_owner() {
        for path in [r"C:\Code\app", r"\\server\share\app"] {
            let info = native_project_info_for_platform(project("windows", path), true);
            assert_eq!(info.storage.access, StorageAccess::Native);
            assert_eq!(info.storage.owner, Some(EnvironmentRef::Native));
        }

        let wsl = native_project_info_for_platform(
            project("wsl", r"\\wsl.localhost\Ubuntu\home\alice\app"),
            true,
        );
        assert_eq!(wsl.storage.access, StorageAccess::CrossStorage);
        assert_eq!(
            wsl.storage.owner,
            Some(EnvironmentRef::Wsl {
                distro_name: "Ubuntu".to_string(),
            })
        );

        let linux = native_project_info_for_platform(project("linux", "/work/app"), false);
        assert_eq!(linux.storage.access, StorageAccess::Native);
        assert_eq!(linux.storage.owner, Some(EnvironmentRef::Native));

        let malformed = native_project_info_for_platform(project("bad", "relative/path"), true);
        assert_eq!(malformed.storage.access, StorageAccess::Unknown);
        assert_eq!(malformed.storage.owner, None);
    }

    #[test]
    fn unavailable_wsl_discovery_keeps_the_native_environment() {
        let registry = WslRuntime::default();
        let snapshot = environment_infos_from_wsl_discovery(
            Err(AppError::EnvironmentDiscoveryFailed {
                message: "wsl.exe was blocked".to_string(),
            }),
            &registry,
        );

        assert_eq!(snapshot.environments, vec![native_environment_info()]);
        assert!(matches!(
            snapshot.error,
            Some(AppError::EnvironmentDiscoveryFailed { .. })
        ));
    }

    #[test]
    fn empty_wsl_discovery_is_normal_native_only_snapshot() {
        let registry = WslRuntime::default();
        let snapshot = environment_infos_from_wsl_discovery(Ok(Vec::new()), &registry);

        assert_eq!(snapshot.environments, vec![native_environment_info()]);
        assert_eq!(snapshot.environments[0].revision, 0);
        assert_eq!(snapshot.error, None);
    }

    #[tokio::test]
    async fn disabled_wsl_integration_returns_native_without_discovery() {
        let registry = WslRuntime::new(false);
        let calls = Arc::new(AtomicUsize::new(0));
        let discovery_calls = Arc::clone(&calls);

        let snapshot = list_environments_with(&registry, cfg!(target_os = "windows"), move || {
            discovery_calls.fetch_add(1, Ordering::SeqCst);
            async { Ok(vec!["Ubuntu".to_string()]) }
        })
        .await;

        assert_eq!(snapshot.environments, vec![native_environment_info()]);
        assert_eq!(snapshot.error, None);
        assert_eq!(
            snapshot.wsl_integration_supported,
            cfg!(target_os = "windows")
        );
        assert!(!snapshot.wsl_integration_enabled);
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn discovery_drops_wsl_results_when_integration_is_disabled_while_waiting() {
        let registry = WslRuntime::default();
        let task_registry = registry.clone();
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = tokio::sync::oneshot::channel();

        let discovery = tokio::spawn(async move {
            list_environments_with(&task_registry, true, move || async move {
                started_tx.send(()).expect("signal discovery start");
                release_rx.await.expect("release discovery");
                Ok(vec!["Ubuntu".to_string()])
            })
            .await
        });
        started_rx.await.expect("discovery started");
        registry.set_wsl_integration_enabled(false);
        release_tx.send(()).expect("release discovery");

        let snapshot = discovery.await.expect("discovery task");
        assert_eq!(snapshot.environments, vec![native_environment_info()]);
        assert!(!snapshot.wsl_integration_enabled);
    }

    #[tokio::test]
    async fn disabled_wsl_integration_rejects_path_mapping_without_wslpath() {
        let registry = WslRuntime::new(false);

        let error = map_environment_path(
            EnvironmentRef::Wsl {
                distro_name: "Ubuntu".to_string(),
            },
            "/home/alice/project".to_string(),
            &registry,
        )
        .await
        .expect_err("disabled WSL path mapping");

        assert!(matches!(
            error,
            AppError::EnvironmentUnavailable {
                environment: EnvironmentRef::Wsl { ref distro_name },
                ..
            } if distro_name == "Ubuntu"
        ));
    }

    #[test]
    fn discovered_distributions_are_flat_environment_entries() {
        let registry = WslRuntime::default();
        let snapshot = environment_infos_from_wsl_discovery(
            Ok(vec![
                "Ubuntu-24.04".to_string(),
                "docker-desktop".to_string(),
                "Debian".to_string(),
                "docker-desktop-data".to_string(),
            ]),
            &registry,
        );

        assert_eq!(snapshot.error, None);
        assert_eq!(snapshot.environments.len(), 3);
        assert_eq!(
            snapshot.environments[1].environment,
            EnvironmentRef::Wsl {
                distro_name: "Ubuntu-24.04".to_string(),
            }
        );
        assert_eq!(snapshot.environments[2].display_name, "Debian");
    }

    #[test]
    fn opening_native_projects_store_does_not_migrate_or_rewrite_config() {
        let temp = tempdir().expect("tempdir");
        let config_path = temp.path().join("config.json");
        let original = br#"{"projects":["C:\\Code\\app"],"futureField":true}"#;
        fs::write(&config_path, original).expect("seed config");

        let projects = native_projects_store_from_config(&config_path)
            .read()
            .expect("read projects");

        assert!(projects.is_empty());
        assert_eq!(fs::read(&config_path).expect("read config"), original);
        assert!(!temp.path().join("projects.json").exists());
        assert_eq!(fs::read_dir(temp.path()).expect("list files").count(), 1);
    }
}

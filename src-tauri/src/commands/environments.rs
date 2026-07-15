use serde::Serialize;
use specta::Type;
use tauri::State;
use tokio::time::Duration;

use crate::core::app_config::get_config_path;
use crate::core::mutation::{MutationGuard, MutationKind, SingleMutationController};
use crate::core::projects::{
    add_project_binding, migrate_legacy_projects, normalize_project_native_path,
    remove_project_binding, set_project_cross_storage_warning_suppressed, ProjectMigrationRegistry,
    ProjectMigrationState, ProjectPathSemantics, ProjectsFile, ProjectsStore,
};
use crate::environment::path_mapping::{
    map_windows_path_with_wslpath, map_wsl_input_without_wslpath, windows_storage_owner,
    WindowsStorageOwner,
};
use crate::environment::types::{
    AddProjectResult, EnvironmentRef, EnvironmentStatus, ProjectBinding, ProjectInfo,
    ProjectStorageInfo, StorageAccess,
};
use crate::environment::wsl::{
    connect_wsl_environment, discover_wsl_distributions, EnvironmentRegistry, WslSession,
};
use crate::environment::wsl_protocol::run_wsl_script;
use crate::error::AppError;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct EnvironmentInfo {
    pub environment: EnvironmentRef,
    pub display_name: String,
    pub status: EnvironmentStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct EnvironmentDiscoverySnapshot {
    pub environments: Vec<EnvironmentInfo>,
    pub error: Option<AppError>,
}

pub fn host_environment_info() -> EnvironmentInfo {
    let display_name = match std::env::consts::OS {
        "windows" => "Windows",
        "macos" => "macOS",
        "linux" => "Linux",
        other => other,
    };
    EnvironmentInfo {
        environment: EnvironmentRef::Host,
        display_name: display_name.to_string(),
        status: EnvironmentStatus::Available,
    }
}

fn environment_infos_from_wsl_discovery(
    discovery: Result<Vec<String>, AppError>,
) -> EnvironmentDiscoverySnapshot {
    let mut environments = vec![host_environment_info()];
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
                    .map(|distro_name| EnvironmentInfo {
                        display_name: distro_name.clone(),
                        environment: EnvironmentRef::Wsl { distro_name },
                        status: EnvironmentStatus::Available,
                    }),
            );
            None
        }
        Err(error) => {
            log::warn!("WSL discovery unavailable; continuing with host only: {error}");
            Some(error)
        }
    };
    EnvironmentDiscoverySnapshot {
        environments,
        error,
    }
}

#[tauri::command]
#[specta::specta]
pub async fn list_environments() -> Result<EnvironmentDiscoverySnapshot, AppError> {
    Ok(environment_infos_from_wsl_discovery(
        discover_wsl_distributions().await,
    ))
}

#[tauri::command]
#[specta::specta]
pub async fn connect_environment(
    distro_name: String,
    registry: State<'_, EnvironmentRegistry>,
) -> Result<WslSession, AppError> {
    let session = connect_wsl_environment(&distro_name).await?;
    registry.insert(session.clone());
    Ok(session)
}

#[tauri::command]
#[specta::specta]
pub async fn map_environment_path(
    environment: EnvironmentRef,
    path: String,
    registry: State<'_, EnvironmentRegistry>,
) -> Result<String, AppError> {
    match environment {
        EnvironmentRef::Host => Ok(normalize_project_native_path(
            &path,
            ProjectPathSemantics::host(),
        )),
        EnvironmentRef::Wsl { distro_name } => {
            if let Some(mapped) = map_wsl_input_without_wslpath(&distro_name, &path)? {
                return Ok(mapped);
            }
            registry
                .with_session_retry(&distro_name, move |session| {
                    let path = path.clone();
                    async move { map_windows_path_with_wslpath(&session, &path).await }
                })
                .await
        }
    }
}

pub(crate) fn host_projects_store() -> Result<ProjectsStore, AppError> {
    let config_path = get_config_path()?;
    Ok(host_projects_store_from_config(&config_path))
}

fn host_projects_store_from_config(config_path: &std::path::Path) -> ProjectsStore {
    ProjectsStore::new(config_path.with_file_name("projects.json"))
}

fn run_host_project_migration() -> Result<ProjectMigrationState, AppError> {
    let config_path = get_config_path()?;
    let projects_path = config_path.with_file_name("projects.json");
    migrate_legacy_projects(&config_path, &projects_path)
}

pub(crate) fn initialize_host_project_migration() -> ProjectMigrationRegistry {
    let state = run_host_project_migration()
        .unwrap_or_else(|error| ProjectMigrationState::Failed { error });
    ProjectMigrationRegistry::new(state)
}

fn ensure_host_projects_ready(
    migration: &ProjectMigrationRegistry,
) -> Result<ProjectsStore, AppError> {
    migration.ensure_ready()?;
    host_projects_store()
}

fn host_project_info_for_platform(binding: ProjectBinding, windows: bool) -> ProjectInfo {
    let (access, owner) = if windows {
        match windows_storage_owner(&binding.native_path) {
            WindowsStorageOwner::Host => (StorageAccess::Native, Some(EnvironmentRef::Host)),
            WindowsStorageOwner::Wsl { distro_name } => (
                StorageAccess::CrossStorage,
                Some(EnvironmentRef::Wsl { distro_name }),
            ),
            WindowsStorageOwner::Unknown => (StorageAccess::Unknown, None),
        }
    } else if binding.native_path.starts_with('/') {
        (StorageAccess::Native, Some(EnvironmentRef::Host))
    } else {
        (StorageAccess::Unknown, None)
    };
    ProjectInfo {
        binding,
        storage: ProjectStorageInfo { access, owner },
    }
}

fn host_project_info(binding: ProjectBinding) -> ProjectInfo {
    host_project_info_for_platform(binding, cfg!(target_os = "windows"))
}

fn host_project_infos(bindings: Vec<ProjectBinding>) -> Vec<ProjectInfo> {
    bindings.into_iter().map(host_project_info).collect()
}

fn updated_host_project(
    bindings: Vec<ProjectBinding>,
    project_id: &str,
) -> Result<ProjectInfo, AppError> {
    bindings
        .into_iter()
        .find(|project| project.id == project_id)
        .map(host_project_info)
        .ok_or_else(|| AppError::PathNotFound {
            path: project_id.to_string(),
        })
}

fn parse_wsl_project_storage(
    environment: &EnvironmentRef,
    project_count: usize,
    bytes: &[u8],
) -> Result<Vec<ProjectStorageInfo>, AppError> {
    let EnvironmentRef::Wsl { distro_name } = environment else {
        return Err(AppError::Custom {
            message: "WSL project storage requires a WSL environment".to_string(),
        });
    };
    let mut fields = bytes.split(|byte| *byte == 0).collect::<Vec<_>>();
    if fields.last().is_some_and(|field| field.is_empty()) {
        fields.pop();
    }
    if fields.first().copied() != Some(b"1".as_slice()) || fields.len() != 1 + project_count * 2 {
        return Err(AppError::Custom {
            message: "invalid WSL project storage response".to_string(),
        });
    }
    fields[1..]
        .chunks_exact(2)
        .map(|record| match record[0] {
            b"error" => Ok(ProjectStorageInfo {
                access: StorageAccess::Unsupported,
                owner: None,
            }),
            b"ok" => {
                let mapped = std::str::from_utf8(record[1]).map_err(|error| AppError::Custom {
                    message: format!("invalid UTF-8 in WSL project storage response: {error}"),
                })?;
                Ok(match windows_storage_owner(mapped) {
                    WindowsStorageOwner::Host => ProjectStorageInfo {
                        access: StorageAccess::CrossStorage,
                        owner: Some(EnvironmentRef::Host),
                    },
                    WindowsStorageOwner::Wsl {
                        distro_name: owner_distro,
                    } if owner_distro.eq_ignore_ascii_case(distro_name) => ProjectStorageInfo {
                        access: StorageAccess::Native,
                        owner: Some(environment.clone()),
                    },
                    WindowsStorageOwner::Wsl {
                        distro_name: owner_distro,
                    } => ProjectStorageInfo {
                        access: StorageAccess::CrossStorage,
                        owner: Some(EnvironmentRef::Wsl {
                            distro_name: owner_distro,
                        }),
                    },
                    WindowsStorageOwner::Unknown => ProjectStorageInfo {
                        access: StorageAccess::Unsupported,
                        owner: None,
                    },
                })
            }
            _ => Err(AppError::Custom {
                message: "invalid WSL project storage record".to_string(),
            }),
        })
        .collect()
}

async fn wsl_project_infos(
    session: &WslSession,
    bindings: Vec<ProjectBinding>,
) -> Result<Vec<ProjectInfo>, AppError> {
    if bindings.is_empty() {
        return Ok(Vec::new());
    }
    const SCRIPT: &str = r#"printf '1\0'; for path do if mapped=$(wslpath -w -- "$path" 2>/dev/null); then printf 'ok\0%s\0' "$mapped"; else printf 'error\0\0'; fi; done"#;
    let args = bindings
        .iter()
        .map(|binding| binding.native_path.clone())
        .collect::<Vec<_>>();
    let output =
        run_wsl_script(session, SCRIPT, &args, Vec::new(), Duration::from_secs(10)).await?;
    let environment = EnvironmentRef::Wsl {
        distro_name: session.distro_name.clone(),
    };
    let storage = parse_wsl_project_storage(&environment, bindings.len(), &output)?;
    Ok(bindings
        .into_iter()
        .zip(storage)
        .map(|(binding, storage)| ProjectInfo { binding, storage })
        .collect())
}

fn begin_project_mutation<'a>(
    controller: &'a SingleMutationController,
    kind: MutationKind,
    environment: EnvironmentRef,
    project_id: Option<&str>,
) -> Result<MutationGuard<'a>, AppError> {
    let scope = project_id.map_or(
        crate::environment::types::ContextScope::Global,
        |project_id| crate::environment::types::ContextScope::Project {
            project_id: project_id.to_string(),
        },
    );
    controller.begin(
        kind,
        crate::environment::types::ContextRef { environment, scope },
    )
}

fn wsl_projects_path(session: &WslSession) -> String {
    format!(
        "{}/.skill-deck/projects.json",
        session.home.trim_end_matches('/')
    )
}

pub(crate) async fn read_wsl_projects(
    session: &WslSession,
) -> Result<Vec<crate::environment::types::ProjectBinding>, AppError> {
    const READ_SCRIPT: &str =
        r#"if [ -f "$1" ]; then cat -- "$1"; else printf '{"schemaVersion":1,"projects":[]}'; fi"#;
    let output = run_wsl_script(
        session,
        READ_SCRIPT,
        &[wsl_projects_path(session)],
        Vec::new(),
        Duration::from_secs(10),
    )
    .await?;
    Ok(serde_json::from_slice::<ProjectsFile>(&output)?.projects)
}

async fn write_wsl_projects(
    session: &WslSession,
    projects: Vec<crate::environment::types::ProjectBinding>,
) -> Result<Vec<crate::environment::types::ProjectBinding>, AppError> {
    const WRITE_SCRIPT: &str = r#"path=$1; dir=${path%/*}; mkdir -p -- "$dir"; tmp=$(mktemp "$dir/.projects.XXXXXX"); trap 'rm -f -- "$tmp"' EXIT HUP INT TERM; cat > "$tmp"; sync "$tmp" 2>/dev/null || true; mv -f -- "$tmp" "$path"; trap - EXIT HUP INT TERM"#;
    let file = ProjectsFile::new(projects, ProjectPathSemantics::Posix);
    run_wsl_script(
        session,
        WRITE_SCRIPT,
        &[wsl_projects_path(session)],
        serde_json::to_vec_pretty(&file)?,
        Duration::from_secs(10),
    )
    .await?;
    read_wsl_projects(session).await
}

#[cfg(all(target_os = "windows", debug_assertions))]
pub(crate) async fn write_wsl_projects_for_integration(
    session: &WslSession,
    projects: Vec<crate::environment::types::ProjectBinding>,
) -> Result<Vec<crate::environment::types::ProjectBinding>, AppError> {
    write_wsl_projects(session, projects).await
}

#[tauri::command]
#[specta::specta]
pub async fn list_environment_projects(
    environment: EnvironmentRef,
    registry: State<'_, EnvironmentRegistry>,
    migration: State<'_, ProjectMigrationRegistry>,
) -> Result<Vec<ProjectInfo>, AppError> {
    match environment {
        EnvironmentRef::Host => Ok(host_project_infos(
            ensure_host_projects_ready(&migration)?.read()?,
        )),
        EnvironmentRef::Wsl { distro_name } => {
            registry
                .with_session_retry(&distro_name, |session| async move {
                    let projects = read_wsl_projects(&session).await?;
                    wsl_project_infos(&session, projects).await
                })
                .await
        }
    }
}

#[tauri::command]
#[specta::specta]
pub async fn add_environment_project(
    environment: EnvironmentRef,
    native_path: String,
    registry: State<'_, EnvironmentRegistry>,
    migration: State<'_, ProjectMigrationRegistry>,
    controller: State<'_, SingleMutationController>,
) -> Result<AddProjectResult, AppError> {
    let _guard = begin_project_mutation(
        controller.inner(),
        MutationKind::AddProject,
        environment.clone(),
        None,
    )?;
    match environment {
        EnvironmentRef::Host => {
            let result = ensure_host_projects_ready(&migration)?.add(native_path)?;
            Ok(AddProjectResult {
                project: host_project_info(result.project),
                created: result.created,
            })
        }
        EnvironmentRef::Wsl { distro_name } => {
            registry
                .with_session_retry(&distro_name, move |session| {
                    let native_path = native_path.clone();
                    async move {
                        let native_path = match map_wsl_input_without_wslpath(
                            &session.distro_name,
                            &native_path,
                        )? {
                            Some(mapped) => mapped,
                            None => map_windows_path_with_wslpath(&session, &native_path).await?,
                        };
                        let result = add_project_binding(
                            read_wsl_projects(&session).await?,
                            native_path,
                            ProjectPathSemantics::Posix,
                        );
                        let project = wsl_project_infos(&session, vec![result.project.clone()])
                            .await?
                            .pop()
                            .expect("one project info");
                        if result.created {
                            write_wsl_projects(&session, result.projects).await?;
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

#[tauri::command]
#[specta::specta]
pub async fn remove_environment_project(
    environment: EnvironmentRef,
    project_id: String,
    registry: State<'_, EnvironmentRegistry>,
    migration: State<'_, ProjectMigrationRegistry>,
    controller: State<'_, SingleMutationController>,
) -> Result<Vec<ProjectInfo>, AppError> {
    let _guard = begin_project_mutation(
        controller.inner(),
        MutationKind::RemoveProject,
        environment.clone(),
        Some(&project_id),
    )?;
    match environment {
        EnvironmentRef::Host => Ok(host_project_infos(
            ensure_host_projects_ready(&migration)?.remove(&project_id)?,
        )),
        EnvironmentRef::Wsl { distro_name } => {
            registry
                .with_session_retry(&distro_name, move |session| {
                    let project_id = project_id.clone();
                    async move {
                        let projects =
                            remove_project_binding(read_wsl_projects(&session).await?, &project_id);
                        let infos = wsl_project_infos(&session, projects.clone()).await?;
                        write_wsl_projects(&session, projects).await?;
                        Ok(infos)
                    }
                })
                .await
        }
    }
}

#[tauri::command]
#[specta::specta]
pub async fn set_environment_project_cross_storage_warning(
    environment: EnvironmentRef,
    project_id: String,
    suppressed: bool,
    registry: State<'_, EnvironmentRegistry>,
    migration: State<'_, ProjectMigrationRegistry>,
    controller: State<'_, SingleMutationController>,
) -> Result<ProjectInfo, AppError> {
    let _guard = begin_project_mutation(
        controller.inner(),
        MutationKind::UpdateProjectPreference,
        environment.clone(),
        Some(&project_id),
    )?;
    match environment {
        EnvironmentRef::Host => {
            let projects = ensure_host_projects_ready(&migration)?
                .set_cross_storage_warning_suppressed(&project_id, suppressed)?;
            updated_host_project(projects, &project_id)
        }
        EnvironmentRef::Wsl { distro_name } => {
            registry
                .with_session_retry(&distro_name, move |session| {
                    let project_id = project_id.clone();
                    async move {
                        let projects = set_project_cross_storage_warning_suppressed(
                            read_wsl_projects(&session).await?,
                            &project_id,
                            suppressed,
                        );
                        let project = wsl_project_infos(&session, projects.clone())
                            .await?
                            .into_iter()
                            .find(|project| project.binding.id == project_id)
                            .ok_or_else(|| AppError::PathNotFound {
                                path: project_id.clone(),
                            })?;
                        write_wsl_projects(&session, projects).await?;
                        Ok(project)
                    }
                })
                .await
        }
    }
}

#[tauri::command]
#[specta::specta]
pub fn retry_host_project_migration(
    migration: State<'_, ProjectMigrationRegistry>,
    controller: State<'_, SingleMutationController>,
) -> Result<Vec<ProjectInfo>, AppError> {
    let context = crate::environment::types::ContextRef {
        environment: EnvironmentRef::Host,
        scope: crate::environment::types::ContextScope::Global,
    };
    let _guard = controller.begin(MutationKind::ProjectMigration, context)?;
    match run_host_project_migration() {
        Ok(state) => {
            migration.set(state);
            Ok(host_project_infos(host_projects_store()?.read()?))
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

    use tempfile::tempdir;

    use super::{
        begin_project_mutation, environment_infos_from_wsl_discovery, host_environment_info,
        host_project_info_for_platform, host_projects_store_from_config, parse_wsl_project_storage,
    };
    use crate::core::mutation::{MutationKind, SingleMutationController};
    use crate::environment::types::{
        EnvironmentRef, EnvironmentStatus, ProjectBinding, StorageAccess,
    };
    use crate::error::AppError;

    #[test]
    fn host_environment_is_always_available() {
        let host = host_environment_info();
        assert_eq!(host.environment, EnvironmentRef::Host);
        assert_eq!(host.status, EnvironmentStatus::Available);
        let expected_name = match std::env::consts::OS {
            "windows" => "Windows",
            "macos" => "macOS",
            "linux" => "Linux",
            other => other,
        };
        assert_eq!(host.display_name, expected_name);
    }

    fn project(id: &str, native_path: &str) -> ProjectBinding {
        ProjectBinding {
            id: id.to_string(),
            native_path: native_path.to_string(),
            display_name: None,
            order: None,
            suppress_cross_storage_warning: false,
        }
    }

    #[test]
    fn host_project_storage_uses_platform_and_wsl_unc_owner() {
        for path in [r"C:\Code\app", r"\\server\share\app"] {
            let info = host_project_info_for_platform(project("windows", path), true);
            assert_eq!(info.storage.access, StorageAccess::Native);
            assert_eq!(info.storage.owner, Some(EnvironmentRef::Host));
        }

        let wsl = host_project_info_for_platform(
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

        let linux = host_project_info_for_platform(project("linux", "/work/app"), false);
        assert_eq!(linux.storage.access, StorageAccess::Native);
        assert_eq!(linux.storage.owner, Some(EnvironmentRef::Host));

        let malformed = host_project_info_for_platform(project("bad", "relative/path"), true);
        assert_eq!(malformed.storage.access, StorageAccess::Unknown);
        assert_eq!(malformed.storage.owner, None);
    }

    #[test]
    fn parses_wsl_project_storage_batch_without_guessing_automount_root() {
        let session_environment = EnvironmentRef::Wsl {
            distro_name: "Ubuntu".to_string(),
        };
        let storage = parse_wsl_project_storage(
            &session_environment,
            3,
            b"1\0ok\0C:\\Code\\app\0ok\0\\\\wsl.localhost\\Ubuntu\\home\\alice\\app\0error\0\0",
        )
        .expect("storage batch");

        assert_eq!(storage.len(), 3);
        assert_eq!(storage[0].access, StorageAccess::CrossStorage);
        assert_eq!(storage[0].owner, Some(EnvironmentRef::Host));
        assert_eq!(storage[1].access, StorageAccess::Native);
        assert_eq!(storage[1].owner, Some(session_environment));
        assert_eq!(storage[2].access, StorageAccess::Unsupported);
        assert_eq!(storage[2].owner, None);
    }

    #[test]
    fn rejects_malformed_wsl_project_storage_batches() {
        let environment = EnvironmentRef::Wsl {
            distro_name: "Ubuntu".to_string(),
        };

        assert!(parse_wsl_project_storage(&environment, 1, b"2\0ok\0C:\\app\0").is_err());
        assert!(parse_wsl_project_storage(&environment, 2, b"1\0ok\0C:\\app\0").is_err());
    }

    #[test]
    fn unavailable_wsl_discovery_keeps_the_host_environment() {
        let snapshot =
            environment_infos_from_wsl_discovery(Err(AppError::EnvironmentDiscoveryFailed {
                message: "wsl.exe was blocked".to_string(),
            }));

        assert_eq!(snapshot.environments, vec![host_environment_info()]);
        assert!(matches!(
            snapshot.error,
            Some(AppError::EnvironmentDiscoveryFailed { .. })
        ));
    }

    #[test]
    fn empty_wsl_discovery_is_normal_host_only_snapshot() {
        let snapshot = environment_infos_from_wsl_discovery(Ok(Vec::new()));

        assert_eq!(snapshot.environments, vec![host_environment_info()]);
        assert_eq!(snapshot.error, None);
    }

    #[test]
    fn discovered_distributions_are_flat_environment_entries() {
        let snapshot = environment_infos_from_wsl_discovery(Ok(vec![
            "Ubuntu-24.04".to_string(),
            "docker-desktop".to_string(),
            "Debian".to_string(),
            "docker-desktop-data".to_string(),
        ]));

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
    fn opening_host_projects_store_does_not_migrate_or_rewrite_config() {
        let temp = tempdir().expect("tempdir");
        let config_path = temp.path().join("config.json");
        let original = br#"{"projects":["C:\\Code\\app"],"futureField":true}"#;
        fs::write(&config_path, original).expect("seed config");

        let projects = host_projects_store_from_config(&config_path)
            .read()
            .expect("read projects");

        assert!(projects.is_empty());
        assert_eq!(fs::read(&config_path).expect("read config"), original);
        assert!(!temp.path().join("projects.json").exists());
        assert_eq!(fs::read_dir(temp.path()).expect("list files").count(), 1);
    }

    #[test]
    fn project_mutation_captures_environment_and_scope() {
        let controller = SingleMutationController::default();
        let environment = EnvironmentRef::Wsl {
            distro_name: "Ubuntu".to_string(),
        };
        let guard = begin_project_mutation(
            &controller,
            MutationKind::RemoveProject,
            environment.clone(),
            Some("project-1"),
        )
        .expect("begin project mutation");

        let active = controller.snapshot().active.expect("active mutation");
        assert_eq!(active.context.environment, environment);
        assert_eq!(
            active.context.scope,
            crate::environment::types::ContextScope::Project {
                project_id: "project-1".to_string(),
            }
        );
        assert!(!active.cancelable);
        drop(guard);
    }

    #[test]
    fn project_mutation_is_rejected_while_another_write_is_active() {
        let controller = SingleMutationController::default();
        let _guard = controller
            .begin(
                MutationKind::Install,
                crate::environment::types::ContextRef {
                    environment: EnvironmentRef::Host,
                    scope: crate::environment::types::ContextScope::Global,
                },
            )
            .expect("begin install");

        assert!(matches!(
            begin_project_mutation(
                &controller,
                MutationKind::AddProject,
                EnvironmentRef::Host,
                None,
            ),
            Err(AppError::MutationBusy)
        ));
    }
}

use serde::Serialize;
use specta::Type;
use tauri::State;
use tokio::time::Duration;

use crate::core::app_config::get_config_path;
use crate::core::projects::{
    add_project_binding, migrate_legacy_projects, remove_project_binding,
    set_project_cross_storage_warning_suppressed, ProjectsFile, ProjectsStore,
};
use crate::environment::path_mapping::{host_path_to_linux_path, wsl_unc_to_linux_path};
use crate::environment::types::{EnvironmentRef, EnvironmentStatus};
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
) -> Vec<EnvironmentInfo> {
    let mut environments = vec![host_environment_info()];
    match discovery {
        Ok(distributions) => {
            environments.extend(
                distributions
                    .into_iter()
                    .map(|distro_name| EnvironmentInfo {
                        display_name: distro_name.clone(),
                        environment: EnvironmentRef::Wsl { distro_name },
                        status: EnvironmentStatus::Available,
                    }),
            )
        }
        Err(error) => log::warn!("WSL discovery unavailable; continuing with host only: {error}"),
    }
    environments
}

#[tauri::command]
#[specta::specta]
pub async fn list_environments_v2() -> Result<Vec<EnvironmentInfo>, AppError> {
    Ok(environment_infos_from_wsl_discovery(
        discover_wsl_distributions().await,
    ))
}

#[tauri::command]
#[specta::specta]
pub async fn connect_environment_v2(
    distro_name: String,
    registry: State<'_, EnvironmentRegistry>,
) -> Result<WslSession, AppError> {
    let session = connect_wsl_environment(&distro_name).await?;
    registry.insert(session.clone());
    Ok(session)
}

#[tauri::command]
#[specta::specta]
pub fn map_environment_path_v2(
    environment: EnvironmentRef,
    path: String,
) -> Result<String, AppError> {
    match environment {
        EnvironmentRef::Host => Ok(path),
        EnvironmentRef::Wsl { distro_name } => host_path_to_linux_path(&path)
            .map(Ok)
            .unwrap_or_else(|| wsl_unc_to_linux_path(&path, &distro_name)),
    }
}

pub(crate) fn host_projects_store() -> Result<ProjectsStore, AppError> {
    let config_path = get_config_path()?;
    let projects_path = config_path.with_file_name("projects.json");
    migrate_legacy_projects(&config_path, &projects_path)?;
    Ok(ProjectsStore::new(projects_path))
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
    let file = ProjectsFile::new(projects);
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
pub async fn list_environment_projects_v2(
    environment: EnvironmentRef,
    registry: State<'_, EnvironmentRegistry>,
) -> Result<Vec<crate::environment::types::ProjectBinding>, AppError> {
    match environment {
        EnvironmentRef::Host => host_projects_store()?.read(),
        EnvironmentRef::Wsl { distro_name } => {
            let session = registry.get(&distro_name).ok_or_else(|| AppError::Custom {
                message: format!("WSL distro '{distro_name}' is not connected"),
            })?;
            read_wsl_projects(&session).await
        }
    }
}

#[tauri::command]
#[specta::specta]
pub async fn add_environment_project_v2(
    environment: EnvironmentRef,
    native_path: String,
    registry: State<'_, EnvironmentRegistry>,
) -> Result<Vec<crate::environment::types::ProjectBinding>, AppError> {
    match environment {
        EnvironmentRef::Host => host_projects_store()?.add(native_path),
        EnvironmentRef::Wsl { distro_name } => {
            let session = registry.get(&distro_name).ok_or_else(|| AppError::Custom {
                message: format!("WSL distro '{distro_name}' is not connected"),
            })?;
            let projects = add_project_binding(read_wsl_projects(&session).await?, native_path);
            write_wsl_projects(&session, projects).await
        }
    }
}

#[tauri::command]
#[specta::specta]
pub async fn remove_environment_project_v2(
    environment: EnvironmentRef,
    project_id: String,
    registry: State<'_, EnvironmentRegistry>,
) -> Result<Vec<crate::environment::types::ProjectBinding>, AppError> {
    match environment {
        EnvironmentRef::Host => host_projects_store()?.remove(&project_id),
        EnvironmentRef::Wsl { distro_name } => {
            let session = registry.get(&distro_name).ok_or_else(|| AppError::Custom {
                message: format!("WSL distro '{distro_name}' is not connected"),
            })?;
            let projects = remove_project_binding(read_wsl_projects(&session).await?, &project_id);
            write_wsl_projects(&session, projects).await
        }
    }
}

#[tauri::command]
#[specta::specta]
pub async fn set_environment_project_cross_storage_warning_v2(
    environment: EnvironmentRef,
    project_id: String,
    suppressed: bool,
    registry: State<'_, EnvironmentRegistry>,
) -> Result<Vec<crate::environment::types::ProjectBinding>, AppError> {
    match environment {
        EnvironmentRef::Host => {
            host_projects_store()?.set_cross_storage_warning_suppressed(&project_id, suppressed)
        }
        EnvironmentRef::Wsl { distro_name } => {
            let session = registry.get(&distro_name).ok_or_else(|| AppError::Custom {
                message: format!("WSL distro '{distro_name}' is not connected"),
            })?;
            let projects = set_project_cross_storage_warning_suppressed(
                read_wsl_projects(&session).await?,
                &project_id,
                suppressed,
            );
            write_wsl_projects(&session, projects).await
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        environment_infos_from_wsl_discovery, host_environment_info, map_environment_path_v2,
    };
    use crate::environment::types::{EnvironmentRef, EnvironmentStatus};
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

    #[test]
    fn maps_windows_project_paths_into_drvfs_for_wsl_environments() {
        let mapped = map_environment_path_v2(
            EnvironmentRef::Wsl {
                distro_name: "Ubuntu".to_string(),
            },
            r"C:\Code\demo".to_string(),
        )
        .expect("map Windows project");

        assert_eq!(mapped, "/mnt/c/Code/demo");
    }

    #[test]
    fn unavailable_wsl_discovery_keeps_the_host_environment() {
        let environments = environment_infos_from_wsl_discovery(Err(AppError::Custom {
            message: "wsl.exe was not found".to_string(),
        }));

        assert_eq!(environments, vec![host_environment_info()]);
    }

    #[test]
    fn discovered_distributions_are_flat_environment_entries() {
        let environments = environment_infos_from_wsl_discovery(Ok(vec![
            "Ubuntu-24.04".to_string(),
            "Debian".to_string(),
        ]));

        assert_eq!(environments.len(), 3);
        assert_eq!(
            environments[1].environment,
            EnvironmentRef::Wsl {
                distro_name: "Ubuntu-24.04".to_string(),
            }
        );
        assert_eq!(environments[2].display_name, "Debian");
    }
}

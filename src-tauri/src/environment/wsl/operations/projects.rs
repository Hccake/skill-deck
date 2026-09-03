use crate::core::projects::{ProjectPathSemantics, ProjectsFile};
use crate::environment::path_mapping::{windows_storage_owner, WindowsStorageOwner};
use crate::environment::types::{
    EnvironmentRef, ProjectInfo, ProjectStorageInfo, RegisteredProject, StorageAccess,
};
use crate::environment::wsl::{WslSession, WslWorkspace};
use crate::error::AppError;

const PROJECT_STORAGE_DEADLINE_MILLIS: u64 = 10_000;

pub struct ProjectsSnapshot {
    pub projects: Vec<RegisteredProject>,
    pub revision: Option<String>,
    pub generation: u64,
}

pub async fn project_infos(
    session: &WslSession,
    workspace: &WslWorkspace,
    bindings: Vec<RegisteredProject>,
) -> Result<Vec<ProjectInfo>, AppError> {
    if bindings.is_empty() {
        return Ok(Vec::new());
    }
    let response: environment_protocol::MapWindowsPathsResponse = workspace
        .request_worker_payload(environment_protocol::Message::MapPathsToWindows {
            request: environment_protocol::MapWindowsPathsRequest {
                paths: bindings
                    .iter()
                    .map(|binding| binding.native_path.clone())
                    .collect(),
                deadline_millis: PROJECT_STORAGE_DEADLINE_MILLIS,
            },
        })
        .await?;
    let environment = EnvironmentRef::Wsl {
        distro_name: session.distro_name.clone(),
    };
    if response.mapped.len() != bindings.len() {
        return Err(protocol_error());
    }
    let storage = project_storage_from_mapped(&environment, response.mapped)?;
    Ok(bindings
        .into_iter()
        .zip(storage)
        .map(|(binding, storage)| ProjectInfo { binding, storage })
        .collect())
}

fn project_storage_from_mapped(
    environment: &EnvironmentRef,
    mapped_paths: Vec<Option<String>>,
) -> Result<Vec<ProjectStorageInfo>, AppError> {
    let EnvironmentRef::Wsl { distro_name } = environment else {
        return Err(AppError::StaleEnvironment);
    };
    mapped_paths
        .into_iter()
        .map(|mapped| match mapped {
            None => Ok(ProjectStorageInfo {
                access: StorageAccess::Unsupported,
                owner: None,
            }),
            Some(mapped) => Ok(match windows_storage_owner(&mapped) {
                WindowsStorageOwner::Windows => ProjectStorageInfo {
                    access: StorageAccess::CrossStorage,
                    owner: Some(EnvironmentRef::Native),
                },
                WindowsStorageOwner::Wsl { distro_name: owner }
                    if owner.eq_ignore_ascii_case(distro_name) =>
                {
                    ProjectStorageInfo {
                        access: StorageAccess::Native,
                        owner: Some(environment.clone()),
                    }
                }
                WindowsStorageOwner::Wsl { distro_name: owner } => ProjectStorageInfo {
                    access: StorageAccess::CrossStorage,
                    owner: Some(EnvironmentRef::Wsl { distro_name: owner }),
                },
                WindowsStorageOwner::Unknown => ProjectStorageInfo {
                    access: StorageAccess::Unsupported,
                    owner: None,
                },
            }),
        })
        .collect()
}

pub async fn read_projects(
    session: &WslSession,
    workspace: &WslWorkspace,
) -> Result<Vec<RegisteredProject>, AppError> {
    Ok(read_projects_snapshot(session, workspace).await?.projects)
}

pub async fn read_projects_snapshot(
    session: &WslSession,
    workspace: &WslWorkspace,
) -> Result<ProjectsSnapshot, AppError> {
    let snapshot = workspace
        .read_optional_document_snapshot_once(
            projects_path(session),
            environment_protocol::MAX_DOCUMENT_BYTES,
        )
        .await?;
    let projects = match snapshot.bytes {
        Some(bytes) => serde_json::from_slice::<ProjectsFile>(&bytes)?.projects,
        None => Vec::new(),
    };
    Ok(ProjectsSnapshot {
        projects,
        revision: snapshot.revision,
        generation: snapshot.generation,
    })
}

pub async fn write_projects(
    session: &WslSession,
    workspace: &WslWorkspace,
    projects: Vec<RegisteredProject>,
    generation: u64,
    expected_revision: Option<String>,
) -> Result<(), AppError> {
    let file = ProjectsFile::new(projects, ProjectPathSemantics::Posix);
    workspace
        .write_document_atomic(
            generation,
            projects_path(session),
            expected_revision,
            serde_json::to_vec_pretty(&file)?,
        )
        .await?;
    Ok(())
}

fn projects_path(session: &WslSession) -> String {
    format!(
        "{}/.skill-deck/projects.json",
        session.home.trim_end_matches('/')
    )
}

fn protocol_error() -> AppError {
    AppError::ConfigurationCorrupted {
        message: "invalid WSL project storage protocol response".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn storage_protocol_preserves_native_and_cross_storage_owners() {
        let environment = EnvironmentRef::Wsl {
            distro_name: "Ubuntu".to_string(),
        };
        let storage = project_storage_from_mapped(
            &environment,
            vec![
                Some(r"C:\Code\app".to_string()),
                Some(r"\\wsl.localhost\Ubuntu\home\alice\app".to_string()),
                None,
            ],
        )
        .unwrap();
        assert_eq!(storage[0].access, StorageAccess::CrossStorage);
        assert_eq!(storage[1].access, StorageAccess::Native);
        assert_eq!(storage[2].access, StorageAccess::Unsupported);
    }
}

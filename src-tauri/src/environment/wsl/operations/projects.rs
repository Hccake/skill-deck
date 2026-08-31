use tokio::time::Duration;

use crate::core::projects::{ProjectPathSemantics, ProjectsFile};
use crate::environment::path_mapping::{windows_storage_owner, WindowsStorageOwner};
use crate::environment::types::{
    EnvironmentRef, ProjectInfo, ProjectStorageInfo, RegisteredProject, ResourceLocator,
    StorageAccess,
};
use crate::environment::wsl::operations::atomic_file::WslAtomicDocumentIo;
use crate::environment::wsl::protocol::{
    wsl_operation, WslOperationDescriptor, WslOperationExecutor, WslOperationRequest,
    DEFAULT_WSL_STDERR_LIMIT,
};
use crate::environment::wsl::WslSession;
use crate::error::AppError;
use crate::storage::atomic_document::AtomicDocumentIo;

const PROJECT_STORAGE_SCRIPT: &str = include_str!("../scripts/projects.sh");
const PROJECT_STORAGE_OPERATION: WslOperationDescriptor =
    wsl_operation("projects", "project-storage", PROJECT_STORAGE_SCRIPT);

pub async fn project_infos(
    session: &WslSession,
    bindings: Vec<RegisteredProject>,
) -> Result<Vec<ProjectInfo>, AppError> {
    if bindings.is_empty() {
        return Ok(Vec::new());
    }
    let output = WslOperationExecutor::execute(
        &PROJECT_STORAGE_OPERATION,
        WslOperationRequest {
            session: session.clone(),
            args: bindings
                .iter()
                .map(|binding| binding.native_path.clone())
                .collect(),
            stdin: Vec::new(),
            timeout: Duration::from_secs(10),
            stdout_limit: bindings.len().saturating_mul(16 * 1024).saturating_add(64),
            stderr_limit: DEFAULT_WSL_STDERR_LIMIT,
            cancellation: None,
        },
    )
    .await?;
    let environment = EnvironmentRef::Wsl {
        distro_name: session.distro_name.clone(),
    };
    let storage = parse_project_storage(&environment, bindings.len(), &output.stdout)?;
    Ok(bindings
        .into_iter()
        .zip(storage)
        .map(|(binding, storage)| ProjectInfo { binding, storage })
        .collect())
}

pub fn parse_project_storage(
    environment: &EnvironmentRef,
    project_count: usize,
    bytes: &[u8],
) -> Result<Vec<ProjectStorageInfo>, AppError> {
    let EnvironmentRef::Wsl { distro_name } = environment else {
        return Err(AppError::StaleEnvironment);
    };
    let mut fields = bytes.split(|byte| *byte == 0).collect::<Vec<_>>();
    if fields.last().is_some_and(|field| field.is_empty()) {
        fields.pop();
    }
    if fields.first().copied() != Some(b"1".as_slice()) || fields.len() != 1 + project_count * 2 {
        return Err(protocol_error());
    }
    let (records, remainder) = fields[1..].as_chunks::<2>();
    debug_assert!(remainder.is_empty());
    records
        .iter()
        .map(|record| match record[0] {
            b"error" => Ok(ProjectStorageInfo {
                access: StorageAccess::Unsupported,
                owner: None,
            }),
            b"ok" => {
                let mapped = std::str::from_utf8(record[1]).map_err(|_| protocol_error())?;
                Ok(match windows_storage_owner(mapped) {
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
                })
            }
            _ => Err(protocol_error()),
        })
        .collect()
}

pub async fn read_projects(session: &WslSession) -> Result<Vec<RegisteredProject>, AppError> {
    let target = projects_locator(session);
    let io = WslAtomicDocumentIo::from_active_session(session.clone());
    match io.read_optional(&target).await? {
        Some(bytes) => Ok(serde_json::from_slice::<ProjectsFile>(&bytes)?.projects),
        None => Ok(Vec::new()),
    }
}

pub async fn write_projects(
    session: &WslSession,
    projects: Vec<RegisteredProject>,
) -> Result<Vec<RegisteredProject>, AppError> {
    let file = ProjectsFile::new(projects, ProjectPathSemantics::Posix);
    WslAtomicDocumentIo::from_active_session(session.clone())
        .write_atomic(
            &projects_locator(session),
            serde_json::to_vec_pretty(&file)?,
        )
        .await?;
    read_projects(session).await
}

fn projects_locator(session: &WslSession) -> ResourceLocator {
    ResourceLocator {
        environment: EnvironmentRef::Wsl {
            distro_name: session.distro_name.clone(),
        },
        native_path: format!(
            "{}/.skill-deck/projects.json",
            session.home.trim_end_matches('/')
        ),
    }
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
        let storage = parse_project_storage(
            &environment,
            3,
            b"1\0ok\0C:\\Code\\app\0ok\0\\\\wsl.localhost\\Ubuntu\\home\\alice\\app\0error\0\0",
        )
        .unwrap();
        assert_eq!(storage[0].access, StorageAccess::CrossStorage);
        assert_eq!(storage[1].access, StorageAccess::Native);
        assert_eq!(storage[2].access, StorageAccess::Unsupported);
    }
}

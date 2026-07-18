use std::path::Path;
use std::sync::Arc;

use crate::application::copy::{
    compare_resolved_projects, CopyFuture, CopyProjectComparator, CopyService, ProjectComparison,
};
use crate::application::install_planner::InstallPlanningFacts;
use crate::application::mutation::coordinator::RuntimeRevisionSource;
use crate::application::payload_session::PayloadSessionManager;
use crate::application::plan_runner::{RuntimeExecutionDependencies, RuntimePlanExecutor};
use crate::application::runtime_facts::{AgentRegistrySnapshotSource, RuntimePlanningFactSource};
use crate::application::skill_entries::InstalledSkillPayloadAcquirer;
use crate::environment::path_mapping::{windows_storage_owner, WindowsStorageOwner};
use crate::environment::planning::{
    resolve_native_targets, resolve_wsl_targets, RuntimeTargetFactResolver,
};
use crate::environment::runtime::PhysicalIdentityComparison;
use crate::environment::types::{EnvironmentRef, ResourceLocator, StorageAccess};
use crate::environment::wsl::operations::path::{map_host_bridge_path, map_storage_path_to_host};
use crate::environment::wsl::EnvironmentRegistry;
use crate::error::AppError;

#[derive(Clone)]
pub struct RuntimeCopyProjectComparator {
    environments: Arc<EnvironmentRegistry>,
}

impl RuntimeCopyProjectComparator {
    pub fn new(environments: Arc<EnvironmentRegistry>) -> Self {
        Self { environments }
    }

    async fn compare_runtime(
        &self,
        source: &InstallPlanningFacts,
        target: &InstallPlanningFacts,
    ) -> Result<ProjectComparison, AppError> {
        let source_project = source
            .resolved_context
            .project
            .as_ref()
            .ok_or(AppError::StaleContext)?;
        let target_project = target
            .resolved_context
            .project
            .as_ref()
            .ok_or(AppError::StaleContext)?;
        let physical_identity = match (
            &source.resolved_context.context.environment,
            &target.resolved_context.context.environment,
        ) {
            (EnvironmentRef::Host, EnvironmentRef::Host) => {
                compare_native_paths(&source_project.native_path, &target_project.native_path)?
            }
            (
                EnvironmentRef::Wsl {
                    distro_name: source_distro,
                },
                EnvironmentRef::Wsl {
                    distro_name: target_distro,
                },
            ) if crate::environment::types::EnvironmentKey::wsl(source_distro)
                == crate::environment::types::EnvironmentKey::wsl(target_distro) =>
            {
                let source_path = source_project.native_path.clone();
                let target_path = target_project.native_path.clone();
                self.environments
                    .with_session_retry(source_distro, move |session| {
                        let source_path = source_path.clone();
                        let target_path = target_path.clone();
                        async move { compare_wsl_paths(&session, &source_path, &target_path).await }
                    })
                    .await?
            }
            (EnvironmentRef::Host, EnvironmentRef::Wsl { distro_name }) => {
                let host_path = source_project.native_path.clone();
                let wsl_path = target_project.native_path.clone();
                self.environments
                    .with_session_retry(distro_name, move |session| {
                        let host_path = host_path.clone();
                        let wsl_path = wsl_path.clone();
                        async move {
                            let mapped = map_host_bridge_path(&session, &host_path, None).await?;
                            compare_wsl_paths(&session, &mapped, &wsl_path).await
                        }
                    })
                    .await?
            }
            (EnvironmentRef::Wsl { distro_name }, EnvironmentRef::Host) => {
                let wsl_path = source_project.native_path.clone();
                let host_path = target_project.native_path.clone();
                self.environments
                    .with_session_retry(distro_name, move |session| {
                        let wsl_path = wsl_path.clone();
                        let host_path = host_path.clone();
                        async move {
                            let mapped = map_host_bridge_path(&session, &host_path, None).await?;
                            compare_wsl_paths(&session, &wsl_path, &mapped).await
                        }
                    })
                    .await?
            }
            (
                EnvironmentRef::Wsl {
                    distro_name: source_distro,
                },
                EnvironmentRef::Wsl {
                    distro_name: target_distro,
                },
            ) => {
                let source_host = self
                    .map_wsl_project_to_host(source_distro, &source_project.native_path)
                    .await?;
                let target_host = self
                    .map_wsl_project_to_host(target_distro, &target_project.native_path)
                    .await?;
                compare_native_paths(&source_host, &target_host)?
            }
        };
        Ok(ProjectComparison {
            physical_identity,
            target_storage_access: self
                .storage_access(
                    &target.resolved_context.context.environment,
                    &target_project.native_path,
                )
                .await,
        })
    }

    async fn map_wsl_project_to_host(
        &self,
        distro_name: &str,
        native_path: &str,
    ) -> Result<String, AppError> {
        let native_path = native_path.to_string();
        self.environments
            .with_session_retry(distro_name, move |session| {
                let native_path = native_path.clone();
                async move { map_storage_path_to_host(&session, &native_path, None).await }
            })
            .await
    }

    async fn storage_access(&self, environment: &EnvironmentRef, path: &str) -> StorageAccess {
        match environment {
            EnvironmentRef::Host => host_storage_access(path),
            EnvironmentRef::Wsl { distro_name } => match self
                .map_wsl_project_to_host(distro_name, path)
                .await
                .map(|host| wsl_storage_access(distro_name, &host))
            {
                Ok(access) => access,
                Err(_) => StorageAccess::Unsupported,
            },
        }
    }
}

impl CopyProjectComparator for RuntimeCopyProjectComparator {
    fn compare<'a>(
        &'a self,
        source: &'a InstallPlanningFacts,
        target: &'a InstallPlanningFacts,
    ) -> CopyFuture<'a, Result<ProjectComparison, AppError>> {
        Box::pin(async move { self.compare_runtime(source, target).await })
    }
}

fn compare_native_paths(left: &str, right: &str) -> Result<PhysicalIdentityComparison, AppError> {
    let facts = resolve_native_targets(&[
        ResourceLocator {
            environment: EnvironmentRef::Host,
            native_path: left.to_string(),
        },
        ResourceLocator {
            environment: EnvironmentRef::Host,
            native_path: right.to_string(),
        },
    ])?;
    compare_resolved_projects(&facts[0], &facts[1])
}

async fn compare_wsl_paths(
    session: &crate::environment::wsl::WslSession,
    left: &str,
    right: &str,
) -> Result<PhysicalIdentityComparison, AppError> {
    let facts = resolve_wsl_targets(session, &[left.to_string(), right.to_string()], None).await?;
    compare_resolved_projects(&facts[0], &facts[1])
}

fn host_storage_access(path: &str) -> StorageAccess {
    if !cfg!(target_os = "windows") {
        return if Path::new(path).is_absolute() {
            StorageAccess::Native
        } else {
            StorageAccess::Unknown
        };
    }
    match windows_storage_owner(path) {
        WindowsStorageOwner::Host => StorageAccess::Native,
        WindowsStorageOwner::Wsl { .. } => StorageAccess::CrossStorage,
        WindowsStorageOwner::Unknown => StorageAccess::Unknown,
    }
}

fn wsl_storage_access(distro_name: &str, host_path: &str) -> StorageAccess {
    match windows_storage_owner(host_path) {
        WindowsStorageOwner::Host => StorageAccess::CrossStorage,
        WindowsStorageOwner::Wsl { distro_name: owner }
            if owner.eq_ignore_ascii_case(distro_name) =>
        {
            StorageAccess::Native
        }
        WindowsStorageOwner::Wsl { .. } => StorageAccess::CrossStorage,
        WindowsStorageOwner::Unknown => StorageAccess::Unsupported,
    }
}

pub type RuntimeCopyService = CopyService<
    RuntimePlanningFactSource,
    RuntimeTargetFactResolver,
    RuntimePlanExecutor,
    RuntimeCopyProjectComparator,
>;

pub fn build_runtime_copy_service(
    payloads: Arc<PayloadSessionManager>,
    environments: Arc<EnvironmentRegistry>,
    registry: Arc<dyn AgentRegistrySnapshotSource>,
    execution: RuntimeExecutionDependencies,
) -> RuntimeCopyService {
    let facts = RuntimePlanningFactSource::for_current_user(registry, environments.clone());
    let targets = RuntimeTargetFactResolver::new(environments.clone());
    let acquirer = InstalledSkillPayloadAcquirer::new(payloads.clone(), environments.clone());
    let revisions: Arc<dyn RuntimeRevisionSource> = Arc::new(facts.clone());
    let executor = execution.executor(environments.clone(), revisions);
    CopyService::new(
        facts,
        targets,
        payloads,
        acquirer,
        executor,
        RuntimeCopyProjectComparator::new(environments),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn storage_access_is_derived_from_physical_owner_not_execution_environment() {
        assert_eq!(
            wsl_storage_access("Ubuntu", r"C:\Code\App"),
            StorageAccess::CrossStorage
        );
        assert_eq!(
            wsl_storage_access("Ubuntu", r"\\wsl.localhost\Ubuntu\home\alice\app"),
            StorageAccess::Native
        );
        assert_eq!(
            wsl_storage_access("Ubuntu", r"\\wsl.localhost\Debian\home\alice\app"),
            StorageAccess::CrossStorage
        );
    }
}

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
    resolve_native_targets, ResolvedTargetFact, RuntimeTargetFactResolver,
};
use crate::environment::types::{EnvironmentRef, ResourceLocator, StorageAccess};
use crate::environment::wsl::operations::path::map_storage_path_to_host;
use crate::environment::wsl::WslRuntime;
use crate::error::AppError;

#[derive(Clone)]
pub struct RuntimeCopyProjectComparator {
    environments: Arc<WslRuntime>,
}

impl RuntimeCopyProjectComparator {
    pub fn new(environments: Arc<WslRuntime>) -> Self {
        Self { environments }
    }

    async fn resolve_project_to_native(
        &self,
        facts: &InstallPlanningFacts,
    ) -> Result<ResolvedTargetFact, AppError> {
        let project = facts
            .resolved_context
            .project
            .as_ref()
            .ok_or(AppError::StaleContext)?;
        let native_path = match &facts.resolved_context.context.environment {
            EnvironmentRef::Native => project.native_path.clone(),
            EnvironmentRef::Wsl { distro_name } => {
                self.map_wsl_project_to_host(distro_name, &project.native_path)
                    .await?
            }
        };
        let mut resolved = resolve_native_targets(&[ResourceLocator {
            environment: EnvironmentRef::Native,
            native_path,
        }])?;
        resolved.pop().ok_or(AppError::StaleTarget)
    }

    async fn compare_runtime(
        &self,
        source: &ResolvedTargetFact,
        target: &InstallPlanningFacts,
    ) -> Result<ProjectComparison, AppError> {
        let target_project = target
            .resolved_context
            .project
            .as_ref()
            .ok_or(AppError::StaleContext)?;
        let target_identity = self.resolve_project_to_native(target).await?;
        let physical_identity = compare_resolved_projects(source, &target_identity)?;
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
            EnvironmentRef::Native => native_storage_access(path),
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
    fn capture_source<'a>(
        &'a self,
        source: &'a InstallPlanningFacts,
    ) -> CopyFuture<'a, Result<ResolvedTargetFact, AppError>> {
        Box::pin(async move { self.resolve_project_to_native(source).await })
    }

    fn compare<'a>(
        &'a self,
        source: &'a ResolvedTargetFact,
        target: &'a InstallPlanningFacts,
    ) -> CopyFuture<'a, Result<ProjectComparison, AppError>> {
        Box::pin(async move { self.compare_runtime(source, target).await })
    }
}

fn native_storage_access(path: &str) -> StorageAccess {
    if !cfg!(target_os = "windows") {
        return if Path::new(path).is_absolute() {
            StorageAccess::Native
        } else {
            StorageAccess::Unknown
        };
    }
    match windows_storage_owner(path) {
        WindowsStorageOwner::Windows => StorageAccess::Native,
        WindowsStorageOwner::Wsl { .. } => StorageAccess::CrossStorage,
        WindowsStorageOwner::Unknown => StorageAccess::Unknown,
    }
}

fn wsl_storage_access(distro_name: &str, host_path: &str) -> StorageAccess {
    match windows_storage_owner(host_path) {
        WindowsStorageOwner::Windows => StorageAccess::CrossStorage,
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
    environments: Arc<WslRuntime>,
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

use std::path::Path;
use std::sync::Arc;

use crate::application::agent_registry_source::AgentRegistrySnapshotSource;
use crate::application::copy::{
    compare_resolved_projects, CopyFuture, CopyProjectComparator, CopyService, ProjectComparison,
};
use crate::application::installed_skill_payload::InstalledSkillPayloadAcquirer;
use crate::application::library_candidates::LibraryCandidateSource;
use crate::application::mutation::coordinator::RuntimeRevisionSource;
use crate::application::payload_session::PayloadSessionManager;
use crate::application::planning_facts::ScopePlanningSnapshot;
use crate::environment::path_mapping::{windows_storage_owner, WindowsStorageOwner};
use crate::environment::planning::{
    resolve_native_targets, ResolvedTargetFact, RuntimeTargetFactResolver, TargetEntryKind,
    TargetFactResolver,
};
use crate::environment::types::{
    same_environment_identity, EnvironmentRef, ResourceLocator, StorageAccess,
};
use crate::environment::wsl::operations::projection::project_targets;
use crate::environment::wsl::WslRuntime;
use crate::error::AppError;
use crate::runtime::plan_runner::{RuntimeExecutionDependencies, RuntimePlanExecutor};
use crate::runtime::planning_facts::RuntimePlanningFactSource;

#[derive(Clone)]
pub struct RuntimeCopyProjectComparator {
    environments: Arc<WslRuntime>,
}

impl RuntimeCopyProjectComparator {
    pub fn new(environments: Arc<WslRuntime>) -> Self {
        Self { environments }
    }

    async fn resolve_project(
        &self,
        facts: &ScopePlanningSnapshot,
    ) -> Result<ResolvedTargetFact, AppError> {
        let project = facts
            .resolved_context
            .project
            .as_ref()
            .ok_or(AppError::StaleContext)?;
        let environment = &facts.resolved_context.context.environment;
        let location = match environment {
            EnvironmentRef::Native => native_project_location(Path::new(&project.native_path))?,
            EnvironmentRef::Wsl { .. } => ResourceLocator {
                environment: environment.clone(),
                native_path: project.native_path.clone(),
            },
        };
        let mut fact = match &location.environment {
            EnvironmentRef::Native => resolve_native_targets(&[location])?
                .pop()
                .ok_or(AppError::StaleTarget)?,
            EnvironmentRef::Wsl { distro_name } => {
                self.resolve_wsl_project(distro_name, &location.native_path)
                    .await?
            }
        };
        if fact.entry_kind != TargetEntryKind::Directory {
            return Err(AppError::StaleTarget);
        }
        if !same_environment_identity(environment, &fact.destination.environment) {
            fact.storage_access = StorageAccess::CrossStorage;
        }
        Ok(fact)
    }

    async fn compare_runtime(
        &self,
        source: &ResolvedTargetFact,
        target: &ScopePlanningSnapshot,
    ) -> Result<ProjectComparison, AppError> {
        let target_identity = self.resolve_project(target).await?;
        let physical_identity = compare_resolved_projects(source, &target_identity)?;
        Ok(ProjectComparison {
            physical_identity,
            target_storage_access: target_identity.storage_access,
        })
    }

    async fn resolve_wsl_project(
        &self,
        distro_name: &str,
        native_path: &str,
    ) -> Result<ResolvedTargetFact, AppError> {
        let workspace = self.environments.workspace(distro_name)?;
        // 投影解析父目录链接；此子路径只用于观察项目根，不创建或写入。
        let probe = format!(
            "{}/.skill-deck-project-identity",
            native_path.trim_end_matches('/')
        );
        let projected = project_targets(&workspace, &[probe], None)
            .await?
            .pop()
            .ok_or(AppError::StaleTarget)?;
        if projected.relative_components.len() != 1 {
            return Err(AppError::StaleTarget);
        }
        let environment = EnvironmentRef::Wsl {
            distro_name: distro_name.to_string(),
        };
        let location = match windows_storage_owner(&projected.storage_projection) {
            WindowsStorageOwner::Windows => {
                native_project_location(Path::new(&projected.storage_projection))?
            }
            WindowsStorageOwner::Wsl { distro_name: owner }
                if owner.eq_ignore_ascii_case(distro_name) =>
            {
                let (root, _) = projected
                    .physical_destination
                    .rsplit_once('/')
                    .ok_or(AppError::StaleTarget)?;
                ResourceLocator {
                    environment: environment.clone(),
                    native_path: if root.is_empty() {
                        "/".to_string()
                    } else {
                        root.to_string()
                    },
                }
            }
            _ => {
                return Err(AppError::StorageMappingUnsupported {
                    path: native_path.to_string(),
                    environment,
                })
            }
        };
        RuntimeTargetFactResolver::new(self.environments.clone())
            .resolve_environment(&location.environment, std::slice::from_ref(&location), None)
            .await?
            .pop()
            .ok_or(AppError::StaleTarget)
    }
}

impl CopyProjectComparator for RuntimeCopyProjectComparator {
    fn capture_source<'a>(
        &'a self,
        source: &'a ScopePlanningSnapshot,
    ) -> CopyFuture<'a, Result<ResolvedTargetFact, AppError>> {
        Box::pin(async move { self.resolve_project(source).await })
    }

    fn compare<'a>(
        &'a self,
        source: &'a ResolvedTargetFact,
        target: &'a ScopePlanningSnapshot,
    ) -> CopyFuture<'a, Result<ProjectComparison, AppError>> {
        Box::pin(async move { self.compare_runtime(source, target).await })
    }
}

fn native_project_location(path: &Path) -> Result<ResourceLocator, AppError> {
    let physical = std::fs::canonicalize(path)?;
    #[cfg(windows)]
    if let Some(location) = wsl_project_from_native(&physical) {
        return Ok(location);
    }
    Ok(ResourceLocator {
        environment: EnvironmentRef::Native,
        native_path: physical.to_str().ok_or(AppError::StaleTarget)?.to_string(),
    })
}

#[cfg(windows)]
fn wsl_project_from_native(path: &Path) -> Option<ResourceLocator> {
    use std::path::{Component, Prefix};

    let mut components = path.components();
    let Component::Prefix(prefix) = components.next()? else {
        return None;
    };
    let (server, distro) = match prefix.kind() {
        Prefix::UNC(server, distro) | Prefix::VerbatimUNC(server, distro) => {
            (server.to_str()?, distro.to_str()?)
        }
        _ => return None,
    };
    if !server.eq_ignore_ascii_case("wsl.localhost") && !server.eq_ignore_ascii_case("wsl$") {
        return None;
    }
    let mut native_path = String::new();
    for component in components {
        match component {
            Component::RootDir => {}
            Component::Normal(value) => {
                native_path.push('/');
                native_path.push_str(value.to_str()?);
            }
            _ => return None,
        }
    }
    Some(ResourceLocator {
        environment: EnvironmentRef::Wsl {
            distro_name: distro.to_string(),
        },
        native_path: if native_path.is_empty() {
            "/".to_string()
        } else {
            native_path
        },
    })
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
    library_candidates: Arc<dyn LibraryCandidateSource>,
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
        library_candidates,
    )
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn copy_project_recognizes_wsl_unc_and_verbatim_unc() {
        for path in [
            r"\\wsl.localhost\Ubuntu\home\alice\项目",
            r"\\wsl$\Ubuntu\home\alice\项目",
            r"\\?\UNC\wsl.localhost\Ubuntu\home\alice\项目",
        ] {
            assert_eq!(
                wsl_project_from_native(Path::new(path)),
                Some(ResourceLocator {
                    environment: EnvironmentRef::Wsl {
                        distro_name: "Ubuntu".into()
                    },
                    native_path: "/home/alice/项目".into(),
                })
            );
        }
        for path in [
            r"C:\Code\App",
            r"\\?\C:\Code\App",
            r"\\server\share\project",
        ] {
            assert!(wsl_project_from_native(Path::new(path)).is_none());
        }
    }
}

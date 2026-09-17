use std::sync::Arc;

use crate::application::agent_registry_source::AgentRegistrySnapshotSource;
use crate::application::agent_selection::build_agent_selection_catalog;
use crate::application::installed_skill_resolver::SkillDirectoryName;
use crate::application::library_candidates::ResolvedLibraryCandidateIndex;
use crate::application::planning_facts::ScopePlanningSnapshotSource;
use crate::application::resources::{
    AuthorizedResourceOpener, AuthorizedResourceReader, ResolvedResourceContext,
    ResourceContextSource, ResourceFuture, ResourceService, SkillIdentity,
};
use crate::application::scope_skill_placements::{
    observe_scope_skill_placements, representative_direct_placement,
};
use crate::application::skill_libraries::SkillLibraryRepository;
use crate::environment::opener::SystemResourceOpener;
use crate::environment::planning::RuntimeTargetFactResolver;
use crate::environment::types::{EnvironmentRef, ResourceLocator, SkillLocationRef};
use crate::environment::wsl::WslRuntime;
use crate::error::AppError;
use crate::runtime::planning_facts::RuntimePlanningFactSource;

#[derive(Clone)]
pub struct RuntimeResourceContextSource {
    facts: RuntimePlanningFactSource,
    targets: RuntimeTargetFactResolver,
    libraries: Arc<dyn SkillLibraryRepository>,
}

impl RuntimeResourceContextSource {
    pub fn new(
        facts: RuntimePlanningFactSource,
        targets: RuntimeTargetFactResolver,
        libraries: Arc<dyn SkillLibraryRepository>,
    ) -> Self {
        Self {
            facts,
            targets,
            libraries,
        }
    }
}

impl ResourceContextSource for RuntimeResourceContextSource {
    fn resolve_skill<'a>(
        &'a self,
        identity: &'a SkillIdentity,
    ) -> ResourceFuture<'a, Result<ResourceLocator, AppError>> {
        Box::pin(async move {
            let facts = self.facts.snapshot(&identity.context).await?;
            let catalog = build_agent_selection_catalog(
                &identity.context,
                &facts.agent_runtime,
                &facts.eve_targets,
                &facts.resolved_context.skill_root,
                &self.targets,
            )
            .await?;
            let skill = SkillDirectoryName::try_from(identity.skill_name.as_str())?;
            let libraries = ResolvedLibraryCandidateIndex::load_known(
                self.libraries.as_ref(),
                &self.targets,
                &identity.context.environment,
                &std::collections::BTreeSet::from([skill]),
            )
            .await?;
            let observed = observe_scope_skill_placements(
                &self.targets,
                &identity.context,
                &identity.skill_name,
                &facts,
                &catalog,
            )
            .await?;
            let placements = observed.describe(&catalog, &libraries)?;
            Ok(representative_direct_placement(&placements)
                .ok_or(AppError::StaleTarget)?
                .entry
                .fact
                .destination
                .clone())
        })
    }

    fn resolve<'a>(
        &'a self,
        context: &'a SkillLocationRef,
    ) -> ResourceFuture<'a, Result<ResolvedResourceContext, AppError>> {
        Box::pin(async move {
            let facts = self.facts.snapshot(context).await?;
            let context_root = facts
                .resolved_context
                .project
                .as_ref()
                .map(|project| ResourceLocator {
                    environment: context.environment.clone(),
                    native_path: project.native_path.clone(),
                })
                .unwrap_or_else(|| facts.resolved_context.home.clone());
            Ok(ResolvedResourceContext {
                context_root,
                canonical_skills_root: facts.resolved_context.skill_root,
            })
        })
    }
}

impl AuthorizedResourceOpener for SystemResourceOpener {
    fn open<'a>(&'a self, target: ResourceLocator) -> ResourceFuture<'a, Result<(), AppError>> {
        Box::pin(async move { crate::environment::opener::open_authorized_resource(&target) })
    }
}

#[derive(Clone)]
pub struct RuntimeResourceReader {
    environments: Arc<WslRuntime>,
}

impl RuntimeResourceReader {
    pub fn new(environments: Arc<WslRuntime>) -> Self {
        Self { environments }
    }
}

impl AuthorizedResourceReader for RuntimeResourceReader {
    fn read_skill<'a>(
        &'a self,
        target: ResourceLocator,
    ) -> ResourceFuture<'a, Result<String, AppError>> {
        Box::pin(async move {
            match &target.environment {
                EnvironmentRef::Native => {
                    crate::core::skill::read_skill_content(&target.native_path)
                }
                EnvironmentRef::Wsl { distro_name } => {
                    let path = target.native_path;
                    let workspace = self.environments.workspace(distro_name)?;
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

pub type RuntimeResourceService = ResourceService<
    RuntimeResourceContextSource,
    RuntimeTargetFactResolver,
    SystemResourceOpener,
    RuntimeResourceReader,
>;

pub fn build_runtime_resource_service(
    environments: Arc<WslRuntime>,
    registry: Arc<dyn AgentRegistrySnapshotSource>,
    libraries: Arc<dyn SkillLibraryRepository>,
) -> RuntimeResourceService {
    let facts = RuntimePlanningFactSource::for_current_user(registry, environments.clone());
    let targets = RuntimeTargetFactResolver::new(environments.clone());
    ResourceService::new(
        RuntimeResourceContextSource::new(facts, targets.clone(), libraries),
        targets,
        SystemResourceOpener,
        RuntimeResourceReader::new(environments),
    )
}

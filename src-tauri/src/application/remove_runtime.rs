use std::sync::Arc;

use crate::application::mutation::coordinator::RuntimeRevisionSource;
use crate::application::plan_runner::{RuntimeExecutionDependencies, RuntimePlanExecutor};
use crate::application::remove::RemoveService;
use crate::application::runtime_facts::{AgentRegistrySnapshotSource, RuntimePlanningFactSource};
use crate::application::skill_entries::SkillEntryObserver;
use crate::environment::planning::RuntimeTargetFactResolver;
use crate::environment::wsl::EnvironmentRegistry;

pub type RuntimeRemoveService =
    RemoveService<RuntimePlanningFactSource, RuntimeTargetFactResolver, RuntimePlanExecutor>;

pub fn build_runtime_remove_service(
    environments: Arc<EnvironmentRegistry>,
    registry: Arc<dyn AgentRegistrySnapshotSource>,
    execution: RuntimeExecutionDependencies,
) -> RuntimeRemoveService {
    let facts = RuntimePlanningFactSource::for_current_user(registry, environments.clone());
    let observer = SkillEntryObserver::new(
        facts.clone(),
        RuntimeTargetFactResolver::new(environments.clone()),
    );
    let revisions: Arc<dyn RuntimeRevisionSource> = Arc::new(facts);
    let executor = execution.executor(environments, revisions);
    RemoveService::new(observer, executor)
}

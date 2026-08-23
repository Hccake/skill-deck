use std::sync::Arc;

use crate::application::agent_registry_source::AgentRegistrySnapshotSource;
use crate::application::library_candidates::LibraryCandidateSource;
use crate::application::mutation::coordinator::RuntimeRevisionSource;
use crate::application::remove::RemoveService;
use crate::environment::planning::RuntimeTargetFactResolver;
use crate::environment::wsl::WslRuntime;
use crate::runtime::plan_runner::{RuntimeExecutionDependencies, RuntimePlanExecutor};
use crate::runtime::planning_facts::RuntimePlanningFactSource;

pub type RuntimeRemoveService =
    RemoveService<RuntimePlanningFactSource, RuntimeTargetFactResolver, RuntimePlanExecutor>;

pub fn build_runtime_remove_service(
    environments: Arc<WslRuntime>,
    registry: Arc<dyn AgentRegistrySnapshotSource>,
    execution: RuntimeExecutionDependencies,
    library_candidates: Arc<dyn LibraryCandidateSource>,
) -> RuntimeRemoveService {
    let facts = RuntimePlanningFactSource::for_current_user(registry, environments.clone());
    let targets = RuntimeTargetFactResolver::new(environments.clone());
    let revisions: Arc<dyn RuntimeRevisionSource> = Arc::new(facts.clone());
    let executor = execution.executor(environments, revisions);
    RemoveService::new(facts, targets, executor, library_candidates)
}

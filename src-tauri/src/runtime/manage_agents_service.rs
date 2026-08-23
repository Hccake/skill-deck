use std::sync::Arc;

use crate::application::agent_registry_source::AgentRegistrySnapshotSource;
use crate::application::installed_skill_payload::InstalledSkillPayloadAcquirer;
use crate::application::library_candidates::LibraryCandidateSource;
use crate::application::manage_agents::ManageAgentsService;
use crate::application::mutation::coordinator::RuntimeRevisionSource;
use crate::application::payload_session::PayloadSessionManager;
use crate::application::scope_skill_placements::ScopeSkillPlacementResolver;
use crate::environment::planning::RuntimeTargetFactResolver;
use crate::environment::wsl::WslRuntime;
use crate::runtime::plan_runner::{RuntimeExecutionDependencies, RuntimePlanExecutor};
use crate::runtime::planning_facts::RuntimePlanningFactSource;

pub type RuntimeManageAgentsService =
    ManageAgentsService<RuntimePlanningFactSource, RuntimeTargetFactResolver, RuntimePlanExecutor>;

pub fn build_runtime_manage_agents_service(
    payloads: Arc<PayloadSessionManager>,
    environments: Arc<WslRuntime>,
    registry: Arc<dyn AgentRegistrySnapshotSource>,
    execution: RuntimeExecutionDependencies,
    library_candidates: Arc<dyn LibraryCandidateSource>,
) -> RuntimeManageAgentsService {
    let facts = RuntimePlanningFactSource::for_current_user(registry, environments.clone());
    let targets = RuntimeTargetFactResolver::new(environments.clone());
    let observer = ScopeSkillPlacementResolver::new(targets.clone());
    let acquirer = InstalledSkillPayloadAcquirer::new(payloads.clone(), environments.clone());
    let revisions: Arc<dyn RuntimeRevisionSource> = Arc::new(facts.clone());
    let executor = execution.executor(environments, revisions);
    ManageAgentsService::new(
        facts,
        observer,
        targets,
        payloads,
        acquirer,
        executor,
        library_candidates,
    )
}

use std::sync::Arc;

use crate::application::manage_agents::ManageAgentsService;
use crate::application::mutation::coordinator::RuntimeRevisionSource;
use crate::application::payload_session::PayloadSessionManager;
use crate::application::plan_runner::{RuntimeExecutionDependencies, RuntimePlanExecutor};
use crate::application::runtime_facts::{AgentRegistrySnapshotSource, RuntimePlanningFactSource};
use crate::application::skill_entries::{InstalledSkillPayloadAcquirer, SkillEntryObserver};
use crate::environment::planning::RuntimeTargetFactResolver;
use crate::environment::wsl::EnvironmentRegistry;

pub type RuntimeManageAgentsService =
    ManageAgentsService<RuntimePlanningFactSource, RuntimeTargetFactResolver, RuntimePlanExecutor>;

pub fn build_runtime_manage_agents_service(
    payloads: Arc<PayloadSessionManager>,
    environments: Arc<EnvironmentRegistry>,
    registry: Arc<dyn AgentRegistrySnapshotSource>,
    execution: RuntimeExecutionDependencies,
) -> RuntimeManageAgentsService {
    let facts = RuntimePlanningFactSource::for_current_user(registry, environments.clone());
    let targets = RuntimeTargetFactResolver::new(environments.clone());
    let observer = SkillEntryObserver::new(facts.clone(), targets.clone());
    let acquirer = InstalledSkillPayloadAcquirer::new(payloads.clone(), environments.clone());
    let revisions: Arc<dyn RuntimeRevisionSource> = Arc::new(facts);
    let executor = execution.executor(environments, revisions);
    ManageAgentsService::new(observer, targets, payloads, acquirer, executor)
}

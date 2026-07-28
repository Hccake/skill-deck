use std::sync::Arc;

use crate::application::install::InstallService;
use crate::application::install_planner::ConcreteInstallPlanner;
use crate::application::mutation::coordinator::RuntimeRevisionSource;
use crate::application::payload_session::PayloadSessionManager;
use crate::application::plan_runner::{RuntimeExecutionDependencies, RuntimePlanExecutor};
use crate::application::runtime_facts::{AgentRegistrySnapshotSource, RuntimePlanningFactSource};
use crate::application::source_evidence::SourceEvidenceCoordinator;
use crate::environment::planning::RuntimeTargetFactResolver;
use crate::environment::wsl::EnvironmentRegistry;

pub type RuntimeInstallService = InstallService<
    ConcreteInstallPlanner<RuntimePlanningFactSource, RuntimeTargetFactResolver>,
    RuntimePlanExecutor,
>;

pub fn build_runtime_install_service(
    payloads: Arc<PayloadSessionManager>,
    environments: Arc<EnvironmentRegistry>,
    registry: Arc<dyn AgentRegistrySnapshotSource>,
    execution: RuntimeExecutionDependencies,
    update_evidence: SourceEvidenceCoordinator,
) -> RuntimeInstallService {
    let facts = RuntimePlanningFactSource::for_current_user(registry, environments.clone());
    let planner = ConcreteInstallPlanner::new(
        facts.clone(),
        RuntimeTargetFactResolver::new(environments.clone()),
        payloads.clone(),
        || {
            chrono::Utc::now()
                .format("%Y-%m-%dT%H:%M:%S%.3fZ")
                .to_string()
        },
    );
    let revisions: Arc<dyn RuntimeRevisionSource> = Arc::new(facts);
    let executor = execution.executor(environments, revisions);
    InstallService::new(payloads, planner, executor).with_source_suppression_clearer(Arc::new(
        move |environment, key| update_evidence.clear_source_suppression(environment, key),
    ))
}

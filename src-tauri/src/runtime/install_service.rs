use std::sync::Arc;

use crate::application::agent_registry_source::AgentRegistrySnapshotSource;
use crate::application::install::InstallService;
use crate::application::install_planner::ConcreteInstallPlanner;
use crate::application::library_candidates::LibraryCandidateSource;
use crate::application::mutation::coordinator::RuntimeRevisionSource;
use crate::application::payload_session::PayloadSessionManager;
use crate::application::source_evidence::SourceEvidenceCoordinator;
use crate::environment::planning::RuntimeTargetFactResolver;
use crate::environment::wsl::WslRuntime;
use crate::runtime::plan_runner::{RuntimeExecutionDependencies, RuntimePlanExecutor};
use crate::runtime::planning_facts::RuntimePlanningFactSource;

pub type RuntimeInstallService = InstallService<
    ConcreteInstallPlanner<RuntimePlanningFactSource, RuntimeTargetFactResolver>,
    RuntimePlanExecutor,
>;

pub fn build_runtime_install_service(
    payloads: Arc<PayloadSessionManager>,
    environments: Arc<WslRuntime>,
    registry: Arc<dyn AgentRegistrySnapshotSource>,
    execution: RuntimeExecutionDependencies,
    update_evidence: SourceEvidenceCoordinator,
    library_candidates: Arc<dyn LibraryCandidateSource>,
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
        library_candidates,
    );
    let revisions: Arc<dyn RuntimeRevisionSource> = Arc::new(facts);
    let executor = execution.executor(environments, revisions);
    InstallService::new(payloads, planner, executor).with_source_suppression_clearer(Arc::new(
        move |environment, key| update_evidence.clear_source_suppression(environment, key),
    ))
}

use std::path::Path;
use std::sync::Arc;

use crate::application::agents::ManagedAgentRegistry;
use crate::application::copy_runtime::{build_runtime_copy_service, RuntimeCopyService};
use crate::application::duplicate_cleanup::DuplicateCleanupService;
use crate::application::github_credentials::{
    resolve_environment_github_token, GithubCredentialService, GithubCredentialWorkflowService,
};
use crate::application::install_runtime::{build_runtime_install_service, RuntimeInstallService};
use crate::application::manage_agents_runtime::{
    build_runtime_manage_agents_service, RuntimeManageAgentsService,
};
use crate::application::payload_session::{PayloadSessionLimits, PayloadSessionManager};
use crate::application::plan_runner::RuntimeExecutionDependencies;
use crate::application::recovery_runtime::RuntimeRecoveryService;
use crate::application::remove_runtime::{build_runtime_remove_service, RuntimeRemoveService};
use crate::application::resources::{build_runtime_resource_service, RuntimeResourceService};
use crate::application::runtime_facts::AgentRegistrySnapshotSource;
use crate::application::update_runtime::{
    build_runtime_source_evidence_coordinator, build_runtime_update_check_service,
    build_runtime_update_service, RuntimeUpdateCheckService, RuntimeUpdateService,
};
use crate::core::mutation::SingleMutationController;
use crate::core::projects::ProjectMigrationRegistry;
use crate::core::{GithubApiClient, GithubTokenProvider};
use crate::environment::native::acquire::NativePayloadSessionStorage;
use crate::environment::project_service::initialize_host_project_migration;
use crate::environment::wsl::EnvironmentRegistry;
use crate::error::AppError;
use crate::storage::github_credentials::KeyringGithubCredentialStore;

pub mod maintenance;

use maintenance::{RuntimeMaintenanceCoordinator, RuntimeMaintenanceTasks};

pub struct RuntimeServiceGraph {
    environments: Arc<EnvironmentRegistry>,
    agents: ManagedAgentRegistry,
    projects: ProjectMigrationRegistry,
    mutation: Arc<SingleMutationController>,
    duplicate_cleanup: DuplicateCleanupService,
    payloads: Arc<PayloadSessionManager>,
    maintenance: Arc<RuntimeMaintenanceCoordinator>,
    recovery: RuntimeRecoveryService,
    install: RuntimeInstallService,
    update_check: RuntimeUpdateCheckService,
    update: RuntimeUpdateService,
    remove: RuntimeRemoveService,
    manage_agents: RuntimeManageAgentsService,
    copy: RuntimeCopyService,
    resources: RuntimeResourceService,
    github_credentials: GithubCredentialWorkflowService,
}

impl RuntimeServiceGraph {
    pub fn new(
        payload_cache_root: &Path,
        recovery_root: std::path::PathBuf,
        agents: ManagedAgentRegistry,
    ) -> Result<Self, AppError> {
        let environments = Arc::new(EnvironmentRegistry::default());
        let (payloads, native_payload_storage) = build_payload_session_manager(payload_cache_root)?;
        let payloads = Arc::new(payloads);
        let mutation = Arc::new(SingleMutationController::default());
        let registry: Arc<dyn AgentRegistrySnapshotSource> = Arc::new(agents.clone());
        let execution = RuntimeExecutionDependencies::new(environments.clone(), recovery_root)?;
        let source_snapshots = Arc::new(
            crate::application::source_snapshot_reuse::SourceSnapshotReuseIndex::default(),
        );
        let github_credentials = Arc::new(GithubCredentialService::new(
            Arc::new(KeyringGithubCredentialStore),
            Arc::new(GithubApiClient::new()),
            Arc::new(resolve_environment_github_token),
        ));
        let github_token_provider: Arc<dyn GithubTokenProvider> = github_credentials.clone();
        let recovery_graph = execution.recovery_graph();
        let maintenance_backend = Arc::new(RuntimeMaintenanceTasks::new(
            payloads.clone(),
            native_payload_storage,
            recovery_graph,
            environments.clone(),
            mutation.clone(),
        ));
        let maintenance = Arc::new(RuntimeMaintenanceCoordinator::new(
            payloads.clone(),
            maintenance_backend,
        ));
        let update_evidence = build_runtime_source_evidence_coordinator(
            payloads.clone(),
            environments.clone(),
            source_snapshots.clone(),
            github_token_provider,
        )?;
        let install = build_runtime_install_service(
            payloads.clone(),
            environments.clone(),
            registry.clone(),
            execution.clone(),
            update_evidence.clone(),
        );
        let update_check = build_runtime_update_check_service(
            environments.clone(),
            registry.clone(),
            update_evidence.clone(),
        );
        let update_evidence_for_update = update_evidence.clone();
        let update = build_runtime_update_service(
            payloads.clone(),
            environments.clone(),
            registry.clone(),
            execution.clone(),
            source_snapshots,
            update_evidence_for_update,
        );
        let remove =
            build_runtime_remove_service(environments.clone(), registry.clone(), execution.clone());
        let manage_agents = build_runtime_manage_agents_service(
            payloads.clone(),
            environments.clone(),
            registry.clone(),
            execution.clone(),
        );
        let resources = build_runtime_resource_service(environments.clone(), registry.clone());
        let copy = build_runtime_copy_service(
            payloads.clone(),
            environments.clone(),
            registry,
            execution.clone(),
        );
        let update_evidence_for_credentials = update_evidence.clone();
        let github_credentials = GithubCredentialWorkflowService::new(
            github_credentials,
            Arc::new(move || update_evidence_for_credentials.clear_host_github_auth_suppression()),
        );
        Ok(Self {
            environments,
            agents,
            projects: initialize_host_project_migration(),
            mutation,
            duplicate_cleanup: DuplicateCleanupService,
            payloads,
            maintenance,
            recovery: execution.recovery_service(),
            install,
            update_check,
            update,
            remove,
            manage_agents,
            copy,
            resources,
            github_credentials,
        })
    }

    pub fn environments(&self) -> &EnvironmentRegistry {
        self.environments.as_ref()
    }

    pub fn environments_arc(&self) -> Arc<EnvironmentRegistry> {
        self.environments.clone()
    }

    pub fn agents(&self) -> &ManagedAgentRegistry {
        &self.agents
    }

    pub fn projects(&self) -> &ProjectMigrationRegistry {
        &self.projects
    }

    pub fn mutation(&self) -> &SingleMutationController {
        self.mutation.as_ref()
    }

    pub fn duplicate_cleanup(&self) -> &DuplicateCleanupService {
        &self.duplicate_cleanup
    }

    pub fn payloads(&self) -> &PayloadSessionManager {
        self.payloads.as_ref()
    }

    pub fn maintenance(&self) -> &Arc<RuntimeMaintenanceCoordinator> {
        &self.maintenance
    }

    pub fn recovery(&self) -> &RuntimeRecoveryService {
        &self.recovery
    }

    pub fn install(&self) -> &RuntimeInstallService {
        &self.install
    }

    pub fn update_check(&self) -> &RuntimeUpdateCheckService {
        &self.update_check
    }

    pub fn update(&self) -> &RuntimeUpdateService {
        &self.update
    }

    pub fn remove(&self) -> &RuntimeRemoveService {
        &self.remove
    }

    pub fn manage_agents(&self) -> &RuntimeManageAgentsService {
        &self.manage_agents
    }

    pub fn copy(&self) -> &RuntimeCopyService {
        &self.copy
    }

    pub fn resources(&self) -> &RuntimeResourceService {
        &self.resources
    }

    pub fn github_credentials(&self) -> &GithubCredentialWorkflowService {
        &self.github_credentials
    }
}

fn build_payload_session_manager(
    cache_root: &Path,
) -> Result<(PayloadSessionManager, Arc<NativePayloadSessionStorage>), AppError> {
    const PAYLOAD_SESSION_TTL_MS: u64 = 30 * 60 * 1_000;
    const MAX_PAYLOAD_SESSIONS: usize = 32;
    const MAX_PAYLOAD_BYTES: u64 = 512 * 1024 * 1024;

    let storage = Arc::new(NativePayloadSessionStorage::new(cache_root)?);
    let manager = PayloadSessionManager::new(
        storage.clone(),
        PayloadSessionLimits {
            ttl_ms: PAYLOAD_SESSION_TTL_MS,
            max_sessions: MAX_PAYLOAD_SESSIONS,
            max_bytes: MAX_PAYLOAD_BYTES,
        },
        || {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|duration| duration.as_millis() as u64)
                .unwrap_or_default()
        },
    );
    Ok((manager, storage))
}

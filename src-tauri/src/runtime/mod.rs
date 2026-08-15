use std::path::Path;
use std::sync::Arc;

use crate::application::agents::ManagedAgentRegistry;
use crate::application::copy_runtime::{build_runtime_copy_service, RuntimeCopyService};
use crate::application::git_transport::GitSourceTransport;
use crate::application::github_access::GithubTreeAccess;
use crate::application::github_credentials::{
    resolve_environment_github_token, GithubCredentialService, GithubCredentialWorkflowService,
};
use crate::application::install_runtime::{build_runtime_install_service, RuntimeInstallService};
use crate::application::install_wizard_workflow::InstallWizardWorkflow;
use crate::application::manage_agents_runtime::{
    build_runtime_manage_agents_service, RuntimeManageAgentsService,
};
use crate::application::payload_session::{PayloadSessionLimits, PayloadSessionManager};
use crate::application::plan_runner::RuntimeExecutionDependencies;
use crate::application::recovery_runtime::RuntimeRecoveryService;
use crate::application::remove_runtime::{build_runtime_remove_service, RuntimeRemoveService};
use crate::application::resources::{build_runtime_resource_service, RuntimeResourceService};
use crate::application::runtime_admission::RuntimeAdmissionCoordinator;
use crate::application::runtime_facts::{AgentRegistrySnapshotSource, RuntimePlanningFactSource};
use crate::application::update_runtime::{
    build_runtime_source_evidence_coordinator, build_runtime_update_check_service,
    build_runtime_update_service, RuntimeUpdateCheckService, RuntimeUpdatePayloadAcquirer,
    RuntimeUpdateService,
};
use crate::application::wellknown_access::WellKnownAccess;
use crate::application::wsl_source_access::WslSourceAccess;
use crate::core::projects::ProjectMigrationRegistry;
use crate::core::GithubTokenProvider;
use crate::environment::native::acquire::NativePayloadSessionStorage;
use crate::environment::planning::RuntimeTargetFactResolver;
use crate::environment::project_service::initialize_native_project_migration;
use crate::environment::wsl::WslRuntime;
use crate::error::AppError;
use crate::runtime::discovery::DiscoveryGateway;
use crate::runtime::github_client::GithubApiClient;
use crate::runtime::http_transport::HttpTransport;
use crate::runtime::proxy_settings::ProxySettingsStore;
use crate::storage::github_credentials::KeyringGithubCredentialStore;

pub(crate) mod application_updater;
pub(crate) mod discovery;
pub(crate) mod download;
pub(crate) mod git_source;
pub(crate) mod github;
pub(crate) mod github_client;
pub(crate) mod http_transport;
pub mod maintenance;
pub(crate) mod network_connection;
pub(crate) mod proxy_settings;
pub(crate) mod source_acquisition;
pub(crate) mod wellknown;
pub(crate) mod wellknown_protocol;
pub(crate) mod wsl_source;

use maintenance::{RuntimeMaintenanceCoordinator, RuntimeMaintenanceTasks};
use source_acquisition::SourceDiscoveryService;

struct RuntimeNetworkServices {
    proxy_settings: Arc<ProxySettingsStore>,
    http: HttpTransport,
    discovery: DiscoveryGateway,
    wellknown: Arc<dyn WellKnownAccess>,
    git_source: Arc<dyn GitSourceTransport>,
}

impl RuntimeNetworkServices {
    fn new(settings: crate::models::NetworkProxySettings) -> Self {
        let proxy_settings = Arc::new(ProxySettingsStore::new(settings));
        let http = HttpTransport::new(proxy_settings.clone());
        let discovery = DiscoveryGateway::new(http.clone());
        let wellknown = Arc::new(wellknown::RuntimeWellKnownAccess::new(http.clone()));
        let git_source = Arc::new(git_source::ProcessGitTransport::new(proxy_settings.clone()));
        Self {
            proxy_settings,
            http,
            discovery,
            wellknown,
            git_source,
        }
    }

    fn proxy_settings(&self) -> Arc<ProxySettingsStore> {
        self.proxy_settings.clone()
    }

    fn http_client(&self) -> HttpTransport {
        self.http.clone()
    }

    fn wellknown(&self) -> Arc<dyn WellKnownAccess> {
        self.wellknown.clone()
    }

    fn git_source(&self) -> Arc<dyn GitSourceTransport> {
        self.git_source.clone()
    }
}

pub struct RuntimeServiceGraph {
    wsl: Arc<WslRuntime>,
    agents: ManagedAgentRegistry,
    projects: ProjectMigrationRegistry,
    admission: Arc<RuntimeAdmissionCoordinator>,
    install_wizard: Arc<InstallWizardWorkflow>,
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
    agent_selection_facts: RuntimePlanningFactSource,
    agent_selection_targets: RuntimeTargetFactResolver,
    network_services: RuntimeNetworkServices,
    source_discovery: SourceDiscoveryService,
    connection_probe: network_connection::RuntimeNetworkConnectionProbe,
}

impl RuntimeServiceGraph {
    pub fn new(
        payload_cache_root: &Path,
        recovery_root: std::path::PathBuf,
        agents: ManagedAgentRegistry,
    ) -> Result<Self, AppError> {
        let config = crate::core::read_config()?;
        let wsl_integration_enabled = cfg!(target_os = "windows") && config.wsl_integration_enabled;
        let network_services = RuntimeNetworkServices::new(config.network_proxy);
        let http = network_services.http_client();
        let download = download::RuntimeDownloadAccess::new(http.clone());
        let git_source = network_services.git_source();
        let wsl = Arc::new(WslRuntime::new_with_support(
            cfg!(target_os = "windows"),
            wsl_integration_enabled,
        ));
        let connection_probe = network_connection::RuntimeNetworkConnectionProbe::new(wsl.clone());
        let (payloads, native_payload_storage) = build_payload_session_manager(payload_cache_root)?;
        let payloads = Arc::new(payloads);
        let wsl_source: Arc<dyn WslSourceAccess> =
            Arc::new(wsl_source::RuntimeWslSourceAccess::new(
                payloads.clone(),
                wsl.clone(),
                network_services.proxy_settings(),
                network_services.wellknown(),
                download.clone(),
            ));
        let source_discovery = SourceDiscoveryService::new(
            payloads.clone(),
            git_source.clone(),
            network_services.wellknown(),
            download,
            wsl_source.clone(),
        );
        let admission = Arc::new(RuntimeAdmissionCoordinator::default());
        let install_wizard = Arc::new(InstallWizardWorkflow::new(admission.clone()));
        let registry: Arc<dyn AgentRegistrySnapshotSource> = Arc::new(agents.clone());
        let execution = RuntimeExecutionDependencies::new(wsl.clone(), recovery_root)?;
        let source_snapshots = Arc::new(
            crate::application::source_snapshot_reuse::SourceSnapshotReuseIndex::default(),
        );
        let github_credentials = Arc::new(GithubCredentialService::new(
            Arc::new(KeyringGithubCredentialStore),
            Arc::new(GithubApiClient::with_network(http.clone())),
            Arc::new(resolve_environment_github_token),
        ));
        let github_token_provider: Arc<dyn GithubTokenProvider> = github_credentials.clone();
        let github_tree_access: Arc<dyn GithubTreeAccess> = Arc::new(
            GithubApiClient::with_token_provider_and_network(github_token_provider, http.clone()),
        );
        let recovery_graph = execution.recovery_graph();
        let maintenance_backend = Arc::new(RuntimeMaintenanceTasks::new(
            payloads.clone(),
            native_payload_storage,
            recovery_graph,
            wsl.clone(),
            admission.clone(),
        ));
        let maintenance = Arc::new(RuntimeMaintenanceCoordinator::new(
            payloads.clone(),
            maintenance_backend,
        ));
        let update_evidence = build_runtime_source_evidence_coordinator(
            payloads.clone(),
            source_snapshots.clone(),
            github_tree_access,
            git_source.clone(),
            wsl_source.clone(),
            network_services.wellknown(),
        )?;
        let agent_selection_facts =
            RuntimePlanningFactSource::for_current_user(registry.clone(), wsl.clone());
        let agent_selection_targets = RuntimeTargetFactResolver::new(wsl.clone());
        let install = build_runtime_install_service(
            payloads.clone(),
            wsl.clone(),
            registry.clone(),
            execution.clone(),
            update_evidence.clone(),
        );
        let update_check = build_runtime_update_check_service(
            wsl.clone(),
            registry.clone(),
            update_evidence.clone(),
        );
        let update_acquirer = RuntimeUpdatePayloadAcquirer::new(
            payloads.clone(),
            source_snapshots.clone(),
            update_evidence.clone(),
            git_source.clone(),
            wsl_source.clone(),
            network_services.wellknown(),
        );
        let update = build_runtime_update_service(
            payloads.clone(),
            wsl.clone(),
            registry.clone(),
            execution.clone(),
            update_acquirer,
        );
        let remove = build_runtime_remove_service(wsl.clone(), registry.clone(), execution.clone());
        let manage_agents = build_runtime_manage_agents_service(
            payloads.clone(),
            wsl.clone(),
            registry.clone(),
            execution.clone(),
        );
        let resources = build_runtime_resource_service(wsl.clone(), registry.clone());
        let copy =
            build_runtime_copy_service(payloads.clone(), wsl.clone(), registry, execution.clone());
        let update_evidence_for_credentials = update_evidence.clone();
        let github_credentials = GithubCredentialWorkflowService::new(
            github_credentials,
            Arc::new(move || {
                update_evidence_for_credentials.clear_native_github_auth_suppression()
            }),
        );
        Ok(Self {
            wsl,
            agents,
            projects: initialize_native_project_migration(),
            admission,
            install_wizard,
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
            agent_selection_facts,
            agent_selection_targets,
            network_services,
            source_discovery,
            connection_probe,
        })
    }

    pub fn wsl(&self) -> &WslRuntime {
        self.wsl.as_ref()
    }

    pub fn wsl_arc(&self) -> Arc<WslRuntime> {
        self.wsl.clone()
    }

    pub fn agents(&self) -> &ManagedAgentRegistry {
        &self.agents
    }

    pub fn projects(&self) -> &ProjectMigrationRegistry {
        &self.projects
    }

    pub fn admission(&self) -> &RuntimeAdmissionCoordinator {
        self.admission.as_ref()
    }

    pub fn install_wizard(&self) -> &Arc<InstallWizardWorkflow> {
        &self.install_wizard
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

    pub fn agent_selection_facts(&self) -> &RuntimePlanningFactSource {
        &self.agent_selection_facts
    }

    pub fn agent_selection_targets(&self) -> &RuntimeTargetFactResolver {
        &self.agent_selection_targets
    }

    pub(crate) fn activate_network_settings(&self, settings: crate::models::NetworkProxySettings) {
        self.network_services
            .proxy_settings
            .replace_settings(settings);
    }

    pub(crate) fn source_discovery(&self) -> &SourceDiscoveryService {
        &self.source_discovery
    }

    pub(crate) fn connection_probe(&self) -> &network_connection::RuntimeNetworkConnectionProbe {
        &self.connection_probe
    }

    pub(crate) fn application_updater<R: tauri::Runtime>(
        &self,
        app: tauri::AppHandle<R>,
    ) -> application_updater::TauriApplicationUpdater<R> {
        application_updater::TauriApplicationUpdater::new(
            app,
            self.network_services.proxy_settings(),
        )
    }

    pub(crate) fn discovery(&self) -> &DiscoveryGateway {
        &self.network_services.discovery
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

#[cfg(test)]
mod tests {
    use super::RuntimeNetworkServices;

    #[test]
    fn runtime_network_services_inject_one_shared_http_pool() {
        let services = RuntimeNetworkServices::new(crate::models::NetworkProxySettings::default());

        let discovery_http = services.http_client();
        let source_http = services.http_client();

        assert!(discovery_http.shares_client_pool_with(&source_http));
    }
}

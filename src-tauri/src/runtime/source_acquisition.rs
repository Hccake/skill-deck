use std::sync::Arc;

use crate::application::download_source::materialize_download;
use crate::application::git_transport::GitSourceTransport;
use crate::application::payload_session::{DiscoverySourceLocation, PayloadSessionManager};
use crate::application::source_acquisition::{
    attempt_wellknown_then_download, invalid_source, redirected_host, retain_discovered_source,
    GitSourceDiscovery, ManagedDownloadedDirectory,
};
use crate::application::wellknown_access::WellKnownAccess;
use crate::application::wsl_source_access::WslSourceAccess;
use crate::core::mutation::CancellationSignal;
use crate::core::{parse_source, CloneProgress};
use crate::environment::types::{EnvironmentRef, SkillLocationRef};
use crate::error::AppError;
use crate::models::{FetchResult, ParsedSource, SourceType};
use crate::runtime::download::RuntimeDownloadAccess;

pub struct SourceDiscoveryService {
    sessions: Arc<PayloadSessionManager>,
    git: GitSourceDiscovery,
    wellknown: Arc<dyn WellKnownAccess>,
    download: RuntimeDownloadAccess,
    wsl_source: Arc<dyn WslSourceAccess>,
}

impl SourceDiscoveryService {
    pub(crate) fn new(
        sessions: Arc<PayloadSessionManager>,
        git_transport: Arc<dyn GitSourceTransport>,
        wellknown: Arc<dyn WellKnownAccess>,
        download: RuntimeDownloadAccess,
        wsl_source: Arc<dyn WslSourceAccess>,
    ) -> Self {
        Self {
            git: GitSourceDiscovery::new(sessions.clone(), git_transport, Arc::clone(&wsl_source)),
            sessions,
            wellknown,
            download,
            wsl_source,
        }
    }

    #[cfg(test)]
    pub(crate) fn with_git_transport(
        sessions: Arc<PayloadSessionManager>,
        git_transport: Arc<dyn GitSourceTransport>,
    ) -> Self {
        use crate::models::NetworkProxySettings;
        use crate::runtime::http_transport::HttpTransport;
        use crate::runtime::proxy_settings::ProxySettingsStore;

        let http = HttpTransport::new(Arc::new(ProxySettingsStore::new(
            NetworkProxySettings::default(),
        )));
        Self {
            git: GitSourceDiscovery::new(
                sessions.clone(),
                git_transport,
                Arc::new(crate::application::wsl_source_access::UnavailableWslSourceAccess),
            ),
            sessions,
            wellknown: Arc::new(crate::application::wellknown_access::UnavailableWellKnownAccess),
            download: RuntimeDownloadAccess::new(http),
            wsl_source: Arc::new(crate::application::wsl_source_access::UnavailableWslSourceAccess),
        }
    }

    pub async fn discover<P>(
        &self,
        context: SkillLocationRef,
        requested_source: String,
        on_progress: P,
    ) -> Result<FetchResult, AppError>
    where
        P: Fn(CloneProgress) + Clone + Send + Sync + 'static,
    {
        self.discover_with_cancellation(
            context,
            requested_source,
            on_progress,
            CancellationSignal::default(),
        )
        .await
    }

    pub async fn discover_with_cancellation<P>(
        &self,
        context: SkillLocationRef,
        requested_source: String,
        on_progress: P,
        cancellation: CancellationSignal,
    ) -> Result<FetchResult, AppError>
    where
        P: Fn(CloneProgress) + Clone + Send + Sync + 'static,
    {
        let parsed = parse_source(&requested_source)?;
        self.discover_parsed_with_cancellation(
            context,
            parsed,
            requested_source,
            on_progress,
            cancellation,
        )
        .await
    }

    pub(crate) async fn discover_parsed_with_cancellation<P>(
        &self,
        context: SkillLocationRef,
        parsed: ParsedSource,
        requested_source: String,
        on_progress: P,
        cancellation: CancellationSignal,
    ) -> Result<FetchResult, AppError>
    where
        P: Fn(CloneProgress) + Clone + Send + Sync + 'static,
    {
        match (&context.environment, parsed.source_type.clone()) {
            (EnvironmentRef::Native, SourceType::Local) => {
                let root = parsed
                    .local_path
                    .clone()
                    .ok_or_else(|| invalid_source("Missing local path"))?;
                retain_discovered_source(
                    self.sessions.clone(),
                    context.environment,
                    parsed,
                    requested_source,
                    DiscoverySourceLocation::Native {
                        root: root.clone(),
                        ref_revision: None,
                    },
                    root,
                    (),
                    None,
                    None,
                    None,
                )
                .await
            }
            (EnvironmentRef::Native, SourceType::WellKnown) => {
                let well_known_environment = context.environment.clone();
                let well_known_parsed = parsed.clone();
                let well_known_requested_source = requested_source.clone();
                let well_known_cancellation = cancellation.clone();
                attempt_wellknown_then_download(
                    async move {
                        let fetched = self
                            .wellknown
                            .fetch(&well_known_parsed.url, &well_known_cancellation)
                            .await?;
                        let root = fetched.repo_path.clone();
                        retain_discovered_source(
                            self.sessions.clone(),
                            well_known_environment,
                            well_known_parsed,
                            well_known_requested_source,
                            DiscoverySourceLocation::Native {
                                root: root.clone(),
                                ref_revision: None,
                            },
                            root.clone(),
                            ManagedDownloadedDirectory::new(root),
                            None,
                            Some(fetched.trust_metadata),
                            None,
                        )
                        .await
                    },
                    || {
                        self.discover_download(
                            context.environment,
                            parsed,
                            requested_source,
                            cancellation,
                        )
                    },
                    "Native",
                )
                .await
            }
            (EnvironmentRef::Native, SourceType::Download) => {
                self.discover_download(context.environment, parsed, requested_source, cancellation)
                    .await
            }
            (EnvironmentRef::Native, _) => {
                self.git
                    .discover(context, parsed, requested_source, on_progress, cancellation)
                    .await
            }
            (EnvironmentRef::Wsl { distro_name }, _) => {
                self.wsl_source
                    .discover(distro_name, parsed, requested_source, cancellation)
                    .await
            }
        }
    }

    async fn discover_download(
        &self,
        environment: EnvironmentRef,
        mut parsed: ParsedSource,
        requested_source: String,
        cancellation: CancellationSignal,
    ) -> Result<FetchResult, AppError> {
        let fetched = self.download.fetch(&parsed.url, &cancellation).await?;
        let materialized = materialize_download(&fetched.bytes)?;
        let root = materialized.keep();
        parsed.source_type = SourceType::Download;
        let redirected_download_host = redirected_host(&parsed.url, &fetched.final_url);
        retain_discovered_source(
            self.sessions.clone(),
            environment,
            parsed,
            requested_source,
            DiscoverySourceLocation::Native {
                root: root.clone(),
                ref_revision: None,
            },
            root.clone(),
            ManagedDownloadedDirectory::new(root),
            None,
            None,
            redirected_download_host,
        )
        .await
    }
}

use std::sync::Arc;

use crate::application::download_source::materialize_download;
use crate::application::git_transport::GitSourceTransport;
use crate::application::payload_session::{DiscoverySourceLocation, PayloadSessionManager};
use crate::application::source_acquisition::{
    attempt_wellknown_then_download, invalid_source, redirected_host, retain_discovered_source,
    GitSourceDiscovery, InternalSkillVisibility, ManagedDownloadedDirectory, RetainedSourceOptions,
    SourceDiscoveryPolicy, SourceSelectionIntent,
};
use crate::application::wellknown_access::{WellKnownAccess, WellKnownFetchError};
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

    #[cfg(test)]
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

    pub async fn discover_with_selection<P>(
        &self,
        context: SkillLocationRef,
        requested_source: String,
        selection: SourceSelectionIntent,
        on_progress: P,
    ) -> Result<FetchResult, AppError>
    where
        P: Fn(CloneProgress) + Clone + Send + Sync + 'static,
    {
        self.discover_with_selection_and_cancellation(
            context,
            requested_source,
            selection,
            on_progress,
            CancellationSignal::default(),
        )
        .await
    }

    #[cfg(test)]
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
        self.discover_with_selection_and_cancellation(
            context,
            requested_source,
            SourceSelectionIntent::default(),
            on_progress,
            cancellation,
        )
        .await
    }

    async fn discover_with_selection_and_cancellation<P>(
        &self,
        context: SkillLocationRef,
        requested_source: String,
        selection: SourceSelectionIntent,
        on_progress: P,
        cancellation: CancellationSignal,
    ) -> Result<FetchResult, AppError>
    where
        P: Fn(CloneProgress) + Clone + Send + Sync + 'static,
    {
        let parsed = parse_source(&requested_source)?;
        let visibility = InternalSkillVisibility::resolve(
            &selection,
            parsed.skill_filter.as_deref(),
            install_internal_skills_enabled(),
        );
        self.discover_parsed_with_visibility(
            context,
            parsed,
            requested_source,
            visibility,
            on_progress,
            cancellation,
        )
        .await
    }

    #[cfg(test)]
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
        let visibility = InternalSkillVisibility::resolve(
            &SourceSelectionIntent::default(),
            parsed.skill_filter.as_deref(),
            install_internal_skills_enabled(),
        );
        self.discover_parsed_with_visibility(
            context,
            parsed,
            requested_source,
            visibility,
            on_progress,
            cancellation,
        )
        .await
    }

    async fn discover_parsed_with_visibility<P>(
        &self,
        context: SkillLocationRef,
        parsed: ParsedSource,
        requested_source: String,
        internal_skill_visibility: InternalSkillVisibility,
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
                    RetainedSourceOptions {
                        internal_skill_visibility,
                        ..Default::default()
                    },
                )
                .await
            }
            (EnvironmentRef::Native, SourceType::WellKnown) => {
                let well_known_environment = context.environment.clone();
                let well_known_parsed = parsed.clone();
                let well_known_requested_source = requested_source.clone();
                let well_known_cancellation = cancellation.clone();
                let well_known_visibility = internal_skill_visibility.clone();
                let download_visibility = internal_skill_visibility;
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
                            RetainedSourceOptions {
                                trust_metadata: Some(fetched.trust_metadata),
                                internal_skill_visibility: well_known_visibility,
                                ..Default::default()
                            },
                        )
                        .await
                        .map_err(WellKnownFetchError::catalog_established)
                    },
                    || {
                        self.discover_download(
                            context.environment,
                            parsed,
                            requested_source,
                            download_visibility,
                            cancellation,
                        )
                    },
                    "Native",
                )
                .await
            }
            (EnvironmentRef::Native, SourceType::Download) => {
                self.discover_download(
                    context.environment,
                    parsed,
                    requested_source,
                    internal_skill_visibility,
                    cancellation,
                )
                .await
            }
            (EnvironmentRef::Native, _) => {
                self.git
                    .discover(
                        context,
                        parsed,
                        requested_source,
                        SourceDiscoveryPolicy {
                            full_depth: false,
                            internal_skill_visibility,
                        },
                        on_progress,
                        cancellation,
                    )
                    .await
            }
            (EnvironmentRef::Wsl { distro_name }, _) => {
                self.wsl_source
                    .discover(
                        distro_name,
                        parsed,
                        requested_source,
                        SourceDiscoveryPolicy {
                            full_depth: false,
                            internal_skill_visibility,
                        },
                        cancellation,
                    )
                    .await
            }
        }
    }

    async fn discover_download(
        &self,
        environment: EnvironmentRef,
        mut parsed: ParsedSource,
        requested_source: String,
        internal_skill_visibility: InternalSkillVisibility,
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
            RetainedSourceOptions {
                redirected_download_host,
                internal_skill_visibility,
                ..Default::default()
            },
        )
        .await
    }
}

fn install_internal_skills_enabled() -> bool {
    std::env::var("INSTALL_INTERNAL_SKILLS")
        .is_ok_and(|value| value == "1" || value.eq_ignore_ascii_case("true"))
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeSet, HashMap};
    use std::fs;
    use std::io::Write;
    use std::path::{Path, PathBuf};
    use std::sync::{Arc, Mutex};

    use super::*;
    use crate::application::git_transport::UnavailableGitSourceTransport;
    use crate::application::payload_session::{PayloadSessionLimits, PayloadSessionManager};
    use crate::application::wellknown_access::{
        WellKnownFetchFuture, WellKnownFetchResult, WellKnownTrustMetadata,
    };
    use crate::application::wsl_source_access::UnavailableWslSourceAccess;
    use crate::git_fixture::{DeterministicGitTransport, SkillTreeFixture};
    use crate::models::NetworkProxySettings;
    use crate::runtime::http_transport::HttpTransport;
    use crate::runtime::proxy_settings::ProxySettingsStore;

    fn sessions() -> Arc<PayloadSessionManager> {
        Arc::new(PayloadSessionManager::in_memory(
            PayloadSessionLimits {
                ttl_ms: 60_000,
                max_sessions: 8,
                max_bytes: 8 * 1024 * 1024,
            },
            || 1_000,
        ))
    }

    fn context() -> SkillLocationRef {
        SkillLocationRef {
            environment: EnvironmentRef::Native,
            scope: crate::environment::types::SkillLocation::Global,
        }
    }

    fn write_visibility_fixture(root: &Path) {
        for (name, internal) in [
            ("public", false),
            ("private", true),
            ("private-other", true),
        ] {
            let directory = root.join(name);
            fs::create_dir_all(&directory).expect("skill directory");
            fs::write(
                directory.join("SKILL.md"),
                format!(
                    "---\nname: {name}\ndescription: Demo\n{}---\n",
                    if internal {
                        "metadata:\n  internal: true\n"
                    } else {
                        ""
                    }
                ),
            )
            .expect("skill document");
        }
    }

    fn names(result: &crate::models::FetchResult) -> BTreeSet<&str> {
        result
            .skills
            .iter()
            .map(|skill| skill.name.as_str())
            .collect()
    }

    fn http_transport() -> HttpTransport {
        HttpTransport::new(Arc::new(ProxySettingsStore::new(
            NetworkProxySettings::default(),
        )))
    }

    struct FixtureWellKnownAccess {
        root: Mutex<Option<PathBuf>>,
    }

    impl WellKnownAccess for FixtureWellKnownAccess {
        fn fetch<'a>(
            &'a self,
            _url: &'a str,
            _cancellation: &'a CancellationSignal,
        ) -> WellKnownFetchFuture<'a> {
            Box::pin(async move {
                Ok(WellKnownFetchResult {
                    repo_path: self
                        .root
                        .lock()
                        .expect("well-known fixture")
                        .take()
                        .expect("well-known root"),
                    trust_metadata: HashMap::<String, WellKnownTrustMetadata>::new(),
                })
            })
        }
    }

    fn service_with_wellknown(root: PathBuf) -> SourceDiscoveryService {
        SourceDiscoveryService::new(
            sessions(),
            Arc::new(UnavailableGitSourceTransport),
            Arc::new(FixtureWellKnownAccess {
                root: Mutex::new(Some(root)),
            }),
            RuntimeDownloadAccess::new(http_transport()),
            Arc::new(UnavailableWslSourceAccess),
        )
    }

    fn zip_visibility_fixture() -> Vec<u8> {
        let mut bytes = std::io::Cursor::new(Vec::new());
        {
            let mut writer = zip::ZipWriter::new(&mut bytes);
            for (path, content) in [
                (
                    "public/SKILL.md",
                    "---\nname: public\ndescription: Demo\n---\n",
                ),
                (
                    "private/SKILL.md",
                    "---\nname: private\ndescription: Demo\nmetadata:\n  internal: true\n---\n",
                ),
                (
                    "private-other/SKILL.md",
                    "---\nname: private-other\ndescription: Demo\nmetadata:\n  internal: true\n---\n",
                ),
            ] {
                writer
                    .start_file(path, zip::write::SimpleFileOptions::default())
                    .expect("zip entry");
                writer.write_all(content.as_bytes()).expect("zip content");
            }
            writer.finish().expect("finish zip");
        }
        bytes.into_inner()
    }

    #[tokio::test]
    async fn git_entry_applies_explicit_internal_visibility() {
        let fixture = SkillTreeFixture::new(&["public"]);
        fixture.add_internal_skill("private");
        fixture.add_internal_skill("private-other");
        let service = SourceDiscoveryService::with_git_transport(
            sessions(),
            Arc::new(DeterministicGitTransport::for_fixture(&fixture)),
        );

        let result = service
            .discover_with_selection(
                context(),
                fixture.source(),
                SourceSelectionIntent {
                    wildcard_requested: false,
                    explicit_skill_names: vec!["private".to_string()],
                },
                |_| {},
            )
            .await
            .expect("Git discovery");

        assert_eq!(names(&result), BTreeSet::from(["private", "public"]));
    }

    #[tokio::test]
    async fn wellknown_entry_applies_explicit_internal_visibility() {
        let source = tempfile::tempdir().expect("well-known source");
        write_visibility_fixture(source.path());
        let root = source.keep();
        let service = service_with_wellknown(root);

        let result = service
            .discover_with_selection(
                context(),
                "https://example.com/team".to_string(),
                SourceSelectionIntent {
                    wildcard_requested: false,
                    explicit_skill_names: vec!["private".to_string()],
                },
                |_| {},
            )
            .await
            .expect("well-known discovery");

        assert_eq!(names(&result), BTreeSet::from(["private", "public"]));
    }

    #[tokio::test]
    async fn download_entry_applies_explicit_internal_visibility() {
        let server = tiny_http::Server::http("127.0.0.1:0").expect("download fixture");
        let address = server.server_addr().to_ip().expect("fixture address");
        let worker = std::thread::spawn(move || {
            let request = server.recv().expect("download request");
            request
                .respond(tiny_http::Response::from_data(zip_visibility_fixture()))
                .expect("download response");
        });
        let url = format!("http://{address}/skills.zip");
        let service = SourceDiscoveryService::new(
            sessions(),
            Arc::new(UnavailableGitSourceTransport),
            Arc::new(crate::application::wellknown_access::UnavailableWellKnownAccess),
            RuntimeDownloadAccess::new(http_transport()),
            Arc::new(UnavailableWslSourceAccess),
        );

        let result = service
            .discover_parsed_with_visibility(
                context(),
                ParsedSource {
                    source_type: SourceType::Download,
                    url: url.clone(),
                    subpath: None,
                    local_path: None,
                    git_ref: None,
                    skill_filter: None,
                },
                url,
                InternalSkillVisibility::Explicit(BTreeSet::from(["private".to_string()])),
                |_| {},
                CancellationSignal::default(),
            )
            .await
            .expect("download discovery");
        worker.join().expect("download fixture worker");

        assert_eq!(names(&result), BTreeSet::from(["private", "public"]));
    }
}

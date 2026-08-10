use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::application::git_transport::GitSourceTransport;
use crate::core::mutation::CancellationSignal;
use crate::core::{
    clone_repo_with_progress_options, probe_remote_ref_revision_options,
    resolve_clone_timeout_secs, CloneProgress, CloneResult,
};
use crate::error::AppError;
use crate::runtime::proxy_settings::ProxySettingsStore;

pub(crate) struct ProcessGitTransport {
    settings: Arc<ProxySettingsStore>,
}

impl ProcessGitTransport {
    pub(crate) fn new(settings: Arc<ProxySettingsStore>) -> Self {
        Self { settings }
    }

    #[cfg(test)]
    pub(crate) fn preserving_existing_config() -> Self {
        Self::new(Arc::new(ProxySettingsStore::new(
            crate::models::NetworkProxySettings::default(),
        )))
    }

    pub(crate) fn probe_ref_revision_with_timeout(
        &self,
        url: &str,
        git_ref: Option<&str>,
        cancellation: CancellationSignal,
        timeout: Duration,
    ) -> Result<String, AppError> {
        let proxy = self.settings.native_git_proxy(url);
        probe_remote_ref_revision_options(url, git_ref, cancellation, proxy.as_deref(), timeout)
    }
}

impl GitSourceTransport for ProcessGitTransport {
    fn clone_source(
        &self,
        url: &str,
        git_ref: Option<&str>,
        on_progress: &(dyn Fn(CloneProgress) + Send + Sync),
        cancellation: CancellationSignal,
    ) -> Result<CloneResult, AppError> {
        let timeout_secs = resolve_clone_timeout_secs();
        let started_at = Instant::now();
        let timeout = Duration::from_secs(timeout_secs);
        let proxy = self.settings.native_git_proxy(url);
        clone_repo_with_progress_options(
            url,
            git_ref,
            |mut progress| {
                progress.elapsed_secs = started_at.elapsed().as_secs();
                progress.timeout_secs = timeout_secs;
                on_progress(progress);
            },
            cancellation,
            proxy.as_deref(),
            timeout,
            timeout_secs,
        )
    }

    fn probe_ref_revision(
        &self,
        url: &str,
        git_ref: Option<&str>,
        cancellation: CancellationSignal,
    ) -> Result<String, AppError> {
        self.probe_ref_revision_with_timeout(
            url,
            git_ref,
            cancellation,
            Duration::from_secs(resolve_clone_timeout_secs()),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git_fixture::BareSkillRepo;
    use crate::models::{GitProxyScope, NativeGitProxySettings, NetworkProxySettings, ProxyMode};

    #[test]
    fn native_proxy_is_selected_only_for_http_sources_in_scope() {
        let transport =
            ProcessGitTransport::new(Arc::new(ProxySettingsStore::new(NetworkProxySettings {
                mode: ProxyMode::Custom,
                custom_proxy_url: Some("http://proxy.example:7890".to_string()),
                native_git: NativeGitProxySettings::UseProxy {
                    proxy_url: "http://proxy.example:7890".to_string(),
                    scope: GitProxyScope::AllHttpHttps,
                },
                ..NetworkProxySettings::default()
            })));

        assert_eq!(
            transport
                .settings
                .native_git_proxy("http://example.com/repo.git"),
            Some("http://proxy.example:7890".to_string())
        );
        assert_eq!(
            transport
                .settings
                .native_git_proxy("https://example.com/repo.git"),
            Some("http://proxy.example:7890".to_string())
        );
        assert_eq!(
            transport
                .settings
                .native_git_proxy("git@example.com:repo.git"),
            None
        );
    }

    #[test]
    fn existing_git_config_mode_does_not_inject_a_proxy() {
        let existing = ProxySettingsStore::new(NetworkProxySettings {
            mode: ProxyMode::Custom,
            custom_proxy_url: Some("http://proxy.example:7890".to_string()),
            native_git: NativeGitProxySettings::UseExistingGitConfig,
            ..NetworkProxySettings::default()
        });

        assert_eq!(
            existing.native_git_proxy("https://example.com/repo.git"),
            None
        );
    }

    #[test]
    fn process_transport_clones_and_probes_a_local_file_remote() {
        let remote = BareSkillRepo::new(&["skills/alpha"]);
        let transport = ProcessGitTransport::preserving_existing_config();
        let source = remote.local_source();

        let initial_revision = transport
            .probe_ref_revision(&source, Some("main"), CancellationSignal::default())
            .expect("probe initial revision");
        let cloned = transport
            .clone_source(
                &source,
                Some("main"),
                &|_| {},
                CancellationSignal::default(),
            )
            .expect("clone local remote");

        assert!(cloned.repo_path.join("skills/alpha/SKILL.md").is_file());
        assert_eq!(
            cloned.ref_revision.as_deref(),
            Some(initial_revision.as_str())
        );

        remote.publish_change("skills/alpha");
        let changed_revision = transport
            .probe_ref_revision(&source, Some("main"), CancellationSignal::default())
            .expect("probe changed revision");
        assert_ne!(changed_revision, initial_revision);
    }
}

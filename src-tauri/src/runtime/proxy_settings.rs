use std::sync::Arc;

use crate::core::app_config::ConfigStore;
use crate::error::AppError;
use crate::models::{
    GitProxyScope, NativeGitProxySettings, NetworkProxySettings, ProxyMode, ProxySettingsSnapshot,
    ProxySettingsTarget, WslGitProxySettings,
};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub(crate) enum ProxySettingsError {
    #[error("proxy configuration must be repaired before this operation can connect")]
    InvalidProxySettings,
}

impl From<ProxySettingsError> for AppError {
    fn from(_: ProxySettingsError) -> Self {
        Self::InvalidProxySettings {
            code: "configurationUnavailable".into(),
        }
    }
}

pub(crate) struct ProxySettingsStore {
    config: Arc<ConfigStore>,
}

impl ProxySettingsStore {
    /// Isolated settings for a draft connection test or a transport fixture.
    pub(crate) fn new(settings: NetworkProxySettings) -> Self {
        Self::from_config(Arc::new(ConfigStore::from_proxy_settings(settings)))
    }

    pub(crate) fn from_config(config: Arc<ConfigStore>) -> Self {
        Self { config }
    }

    #[cfg(test)]
    pub(crate) fn replace_settings(&self, settings: NetworkProxySettings) {
        self.config
            .save_proxy(settings)
            .expect("valid test settings");
    }

    pub(crate) fn clone_timeout_secs(&self) -> u64 {
        crate::core::git::normalize_clone_timeout_secs(self.config.git_clone_timeout_secs().into())
    }

    pub(crate) fn proxy_url(&self) -> Result<Option<String>, ProxySettingsError> {
        let snapshot = self.config.proxy_snapshot();
        require_available(&snapshot, &ProxySettingsTarget::Http)?;
        match snapshot.settings.mode {
            ProxyMode::Direct => Ok(None),
            ProxyMode::Custom => snapshot
                .settings
                .custom_proxy_url
                .map(Some)
                .ok_or(ProxySettingsError::InvalidProxySettings),
        }
    }

    pub(crate) fn native_git_proxy(
        &self,
        target: &str,
    ) -> Result<Option<String>, ProxySettingsError> {
        if !is_http_remote(target) {
            return Ok(None);
        }
        select_native_proxy(&self.config.proxy_snapshot(), target)
    }

    pub(crate) fn wsl_git_proxy(
        &self,
        distro: &str,
        target: &str,
    ) -> Result<Option<String>, ProxySettingsError> {
        if !is_http_remote(target) {
            return Ok(None);
        }
        let snapshot = self.config.proxy_snapshot();
        require_available(
            &snapshot,
            &ProxySettingsTarget::WslGit {
                distro: Some(distro.to_string()),
            },
        )?;
        match snapshot.settings.wsl_git.get(distro) {
            Some(WslGitProxySettings::FollowNativeGit) => select_native_proxy(&snapshot, target),
            None | Some(WslGitProxySettings::UseExistingGitConfig) => Ok(None),
            Some(WslGitProxySettings::UseProxy { proxy_url, scope }) => {
                Ok(git_proxy_for_target(proxy_url, *scope, target))
            }
        }
    }
}

fn require_available(
    snapshot: &ProxySettingsSnapshot,
    target: &ProxySettingsTarget,
) -> Result<(), ProxySettingsError> {
    if snapshot.is_available(target) {
        Ok(())
    } else {
        Err(ProxySettingsError::InvalidProxySettings)
    }
}

fn select_native_proxy(
    snapshot: &ProxySettingsSnapshot,
    target: &str,
) -> Result<Option<String>, ProxySettingsError> {
    require_available(snapshot, &ProxySettingsTarget::NativeGit)?;
    Ok(native_git_proxy_for_target(
        &snapshot.settings.native_git,
        target,
    ))
}

fn is_http_remote(target: &str) -> bool {
    url::Url::parse(target).is_ok_and(|target| matches!(target.scheme(), "http" | "https"))
}

fn native_git_proxy_for_target(settings: &NativeGitProxySettings, target: &str) -> Option<String> {
    match settings {
        NativeGitProxySettings::UseExistingGitConfig => None,
        NativeGitProxySettings::UseProxy { proxy_url, scope } => {
            git_proxy_for_target(proxy_url, *scope, target)
        }
    }
}

fn git_proxy_for_target(proxy_url: &str, scope: GitProxyScope, target: &str) -> Option<String> {
    let target = url::Url::parse(target).ok()?;
    if !matches!(target.scheme(), "http" | "https") {
        return None;
    }
    let matches_scope = match scope {
        GitProxyScope::AllHttpHttps => true,
        GitProxyScope::GithubOnly => target
            .host_str()
            .is_some_and(|host| host == "github.com" || host.ends_with(".github.com")),
    };
    if matches_scope {
        Some(proxy_url.to_string())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use crate::core::app_config::ConfigStore;
    use crate::models::{
        GitProxyScope, NativeGitProxySettings, NetworkProxySettings, ProxyMode, WslGitProxySettings,
    };
    use std::sync::Arc;

    use super::ProxySettingsStore;

    #[test]
    fn invalid_native_git_blocks_affected_remotes_without_disabling_http_or_ssh() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("config.json");
        std::fs::write(&path, br#"{"networkProxy":{"mode":"custom","customProxyUrl":"http://127.0.0.1:7890","nativeGit":"useProxy"}}"#).unwrap();
        let settings = ProxySettingsStore::from_config(Arc::new(ConfigStore::open(path)));

        assert_eq!(
            settings.proxy_url().unwrap().as_deref(),
            Some("http://127.0.0.1:7890")
        );
        assert!(settings
            .native_git_proxy("https://github.com/owner/repo")
            .is_err());
        assert_eq!(
            settings
                .native_git_proxy("git@github.com:owner/repo.git")
                .unwrap(),
            None
        );
    }

    #[test]
    fn wsl_proxy_errors_follow_the_actual_configuration_dependencies() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("config.json");
        std::fs::write(&path, br#"{"networkProxy":{"mode":"direct","nativeGit":"useProxy","wslGit":{"Ubuntu":{"behavior":"followNativeGit"},"Debian":{"behavior":"useProxy","proxyUrl":"http://127.0.0.1:7891","scope":"allHttpHttps"},"Broken":"useProxy"}}}"#).unwrap();
        let settings = ProxySettingsStore::from_config(Arc::new(ConfigStore::open(path)));

        assert!(settings
            .wsl_git_proxy("Ubuntu", "https://github.com/owner/repo")
            .is_err());
        assert!(settings
            .wsl_git_proxy("Broken", "https://github.com/owner/repo")
            .is_err());
        assert_eq!(
            settings
                .wsl_git_proxy("Debian", "https://github.com/owner/repo")
                .unwrap()
                .as_deref(),
            Some("http://127.0.0.1:7891")
        );
        assert_eq!(
            settings
                .wsl_git_proxy("Other", "https://github.com/owner/repo")
                .unwrap(),
            None
        );
    }

    #[test]
    fn network_operations_observe_saved_settings_and_timeout_from_the_shared_snapshot() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("config.json");
        let config = Arc::new(ConfigStore::open(path.clone()));
        let settings = ProxySettingsStore::from_config(config.clone());
        config
            .save_proxy(NetworkProxySettings {
                mode: ProxyMode::Custom,
                custom_proxy_url: Some("http://127.0.0.1:7890".into()),
                ..Default::default()
            })
            .unwrap();
        config
            .update(|config| config.git_clone_timeout_secs = 300)
            .unwrap();
        std::fs::write(path, b"corrupt external change").unwrap();

        assert_eq!(
            settings.proxy_url().unwrap().as_deref(),
            Some("http://127.0.0.1:7890")
        );
        assert_eq!(settings.clone_timeout_secs(), 300);
    }

    #[test]
    fn replacing_settings_changes_the_value_read_by_later_operations() {
        let policy = ProxySettingsStore::new(NetworkProxySettings::default());
        assert_eq!(policy.proxy_url().expect("direct settings"), None);

        policy.replace_settings(NetworkProxySettings {
            mode: ProxyMode::Custom,
            custom_proxy_url: Some("http://127.0.0.1:7890".to_string()),
            ..NetworkProxySettings::default()
        });

        assert_eq!(
            policy.proxy_url().expect("current proxy settings"),
            Some("http://127.0.0.1:7890".to_string())
        );
    }

    #[test]
    fn explicit_wsl_git_proxy_is_selected_only_for_http_transport() {
        let policy = ProxySettingsStore::new(NetworkProxySettings {
            wsl_git: [(
                "Ubuntu".to_string(),
                WslGitProxySettings::UseProxy {
                    proxy_url: "http://wsl.example:7890".to_string(),
                    scope: GitProxyScope::AllHttpHttps,
                },
            )]
            .into_iter()
            .collect(),
            ..NetworkProxySettings::default()
        });

        assert_eq!(
            policy
                .wsl_git_proxy("Ubuntu", "http://github.com/owner/repo.git")
                .expect("valid proxy settings"),
            Some("http://wsl.example:7890".to_string())
        );
        assert_eq!(
            policy
                .wsl_git_proxy("Ubuntu", "git@github.com:owner/repo.git")
                .expect("valid proxy settings"),
            None
        );
    }

    #[test]
    fn github_only_native_proxy_preserves_git_behavior_for_other_http_remotes() {
        let policy = ProxySettingsStore::new(NetworkProxySettings {
            native_git: NativeGitProxySettings::UseProxy {
                proxy_url: "http://native.proxy:7890".to_string(),
                scope: GitProxyScope::GithubOnly,
            },
            ..NetworkProxySettings::default()
        });

        assert_eq!(
            policy
                .native_git_proxy("https://github.com/owner/repo.git")
                .expect("valid proxy settings"),
            Some("http://native.proxy:7890".to_string())
        );
        assert_eq!(
            policy
                .native_git_proxy("https://gitlab.example.cn/owner/repo.git")
                .expect("valid proxy settings"),
            None
        );
        assert_eq!(
            policy
                .native_git_proxy("git@github.com:owner/repo.git")
                .expect("valid proxy settings"),
            None
        );
    }

    #[test]
    fn wsl_can_follow_native_git_or_use_a_distribution_proxy() {
        let policy = ProxySettingsStore::new(NetworkProxySettings {
            native_git: NativeGitProxySettings::UseProxy {
                proxy_url: "http://native.proxy:7890".to_string(),
                scope: GitProxyScope::GithubOnly,
            },
            wsl_git: [
                ("Ubuntu".to_string(), WslGitProxySettings::FollowNativeGit),
                (
                    "Debian".to_string(),
                    WslGitProxySettings::UseProxy {
                        proxy_url: "http://debian.proxy:7890".to_string(),
                        scope: GitProxyScope::AllHttpHttps,
                    },
                ),
            ]
            .into_iter()
            .collect(),
            ..NetworkProxySettings::default()
        });

        assert_eq!(
            policy
                .wsl_git_proxy("Ubuntu", "https://github.com/owner/repo.git")
                .expect("valid proxy settings"),
            Some("http://native.proxy:7890".to_string())
        );
        assert_eq!(
            policy
                .wsl_git_proxy("Ubuntu", "https://gitlab.example.cn/owner/repo.git")
                .expect("valid proxy settings"),
            None
        );
        assert_eq!(
            policy
                .wsl_git_proxy("Debian", "https://gitlab.example.cn/owner/repo.git")
                .expect("valid proxy settings"),
            Some("http://debian.proxy:7890".to_string())
        );
        assert_eq!(
            policy
                .wsl_git_proxy("Fedora", "https://github.com/owner/repo.git")
                .expect("valid proxy settings"),
            None
        );
    }
}

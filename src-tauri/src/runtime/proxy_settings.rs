use std::sync::RwLock;

use crate::models::{
    GitProxyScope, NativeGitProxySettings, NetworkProxySettings, ProxyMode, WslGitProxySettings,
};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub(crate) enum ProxySettingsError {
    #[error("invalid proxy settings")]
    InvalidProxySettings,
}

pub(crate) struct ProxySettingsStore {
    settings: RwLock<NetworkProxySettings>,
}

impl ProxySettingsStore {
    pub(crate) fn new(settings: NetworkProxySettings) -> Self {
        Self {
            settings: RwLock::new(settings),
        }
    }

    pub(crate) fn replace_settings(&self, settings: NetworkProxySettings) {
        *self
            .settings
            .write()
            .expect("network settings lock poisoned") = settings;
    }

    pub(crate) fn proxy_url(&self) -> Result<Option<String>, ProxySettingsError> {
        let settings = self
            .settings
            .read()
            .expect("network settings lock poisoned");
        match settings.mode {
            ProxyMode::Direct => Ok(None),
            ProxyMode::Custom => settings
                .custom_proxy_url
                .clone()
                .map(Some)
                .ok_or(ProxySettingsError::InvalidProxySettings),
        }
    }

    pub(crate) fn native_git_proxy(&self, target: &str) -> Option<String> {
        let settings = self
            .settings
            .read()
            .expect("network settings lock poisoned");
        native_git_proxy_for_target(&settings.native_git, target)
    }

    pub(crate) fn wsl_git_proxy(&self, distro: &str, target: &str) -> Option<String> {
        let settings = self
            .settings
            .read()
            .expect("network settings lock poisoned");
        match settings.wsl_git.get(distro) {
            Some(WslGitProxySettings::FollowNativeGit) => {
                native_git_proxy_for_target(&settings.native_git, target)
            }
            None | Some(WslGitProxySettings::UseExistingGitConfig) => None,
            Some(WslGitProxySettings::UseProxy { proxy_url, scope }) => {
                git_proxy_for_target(proxy_url, *scope, target)
            }
        }
    }
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
    use crate::models::{
        GitProxyScope, NativeGitProxySettings, NetworkProxySettings, ProxyMode, WslGitProxySettings,
    };

    use super::ProxySettingsStore;

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
            policy.wsl_git_proxy("Ubuntu", "http://github.com/owner/repo.git"),
            Some("http://wsl.example:7890".to_string())
        );
        assert_eq!(
            policy.wsl_git_proxy("Ubuntu", "git@github.com:owner/repo.git"),
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
            policy.native_git_proxy("https://github.com/owner/repo.git"),
            Some("http://native.proxy:7890".to_string())
        );
        assert_eq!(
            policy.native_git_proxy("https://gitlab.example.cn/owner/repo.git"),
            None
        );
        assert_eq!(
            policy.native_git_proxy("git@github.com:owner/repo.git"),
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
            policy.wsl_git_proxy("Ubuntu", "https://github.com/owner/repo.git"),
            Some("http://native.proxy:7890".to_string())
        );
        assert_eq!(
            policy.wsl_git_proxy("Ubuntu", "https://gitlab.example.cn/owner/repo.git"),
            None
        );
        assert_eq!(
            policy.wsl_git_proxy("Debian", "https://gitlab.example.cn/owner/repo.git"),
            Some("http://debian.proxy:7890".to_string())
        );
        assert_eq!(
            policy.wsl_git_proxy("Fedora", "https://github.com/owner/repo.git"),
            None
        );
    }
}

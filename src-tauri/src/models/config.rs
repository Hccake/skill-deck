use serde::{Deserialize, Serialize};
use specta::Type;

use crate::environment::types::EnvironmentRef;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub enum ProxyMode {
    Custom,
    Direct,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "behavior", rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
#[specta(tag = "behavior", rename_all = "camelCase")]
pub enum NativeGitProxySettings {
    #[default]
    UseExistingGitConfig,
    UseProxy {
        #[serde(rename = "proxyUrl")]
        #[specta(rename = "proxyUrl")]
        proxy_url: String,
        scope: GitProxyScope,
    },
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub enum GitProxyScope {
    #[default]
    GithubOnly,
    AllHttpHttps,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "behavior", rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
#[specta(tag = "behavior", rename_all = "camelCase")]
pub enum WslGitProxySettings {
    FollowNativeGit,
    #[default]
    UseExistingGitConfig,
    UseProxy {
        #[serde(rename = "proxyUrl")]
        #[specta(rename = "proxyUrl")]
        proxy_url: String,
        scope: GitProxyScope,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
#[specta(rename_all = "camelCase")]
pub struct NetworkProxySettings {
    pub mode: ProxyMode,
    pub custom_proxy_url: Option<String>,
    #[serde(default)]
    pub native_git: NativeGitProxySettings,
    #[serde(default)]
    pub wsl_git: std::collections::BTreeMap<String, WslGitProxySettings>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProxySettingsValidationError {
    code: &'static str,
}

impl ProxySettingsValidationError {
    pub fn code(&self) -> &'static str {
        self.code
    }

    fn invalid_proxy_url() -> Self {
        Self {
            code: "invalidProxyUrl",
        }
    }
}

impl NetworkProxySettings {
    fn default_settings() -> Self {
        Self {
            mode: ProxyMode::Direct,
            custom_proxy_url: None,
            native_git: NativeGitProxySettings::UseExistingGitConfig,
            wsl_git: std::collections::BTreeMap::new(),
        }
    }

    pub fn validate_and_normalize(mut self) -> Result<Self, ProxySettingsValidationError> {
        if self.mode == ProxyMode::Custom {
            self.custom_proxy_url = Some(normalize_proxy_url(
                self.custom_proxy_url
                    .as_deref()
                    .ok_or_else(ProxySettingsValidationError::invalid_proxy_url)?,
            )?);
        } else {
            self.custom_proxy_url = None;
        }
        self.native_git = normalize_native_git_settings(self.native_git)?;
        let mut wsl_git = std::collections::BTreeMap::new();
        for (distro, settings) in std::mem::take(&mut self.wsl_git) {
            wsl_git.insert(
                normalize_distro_name(&distro)?,
                normalize_wsl_git_settings(settings)?,
            );
        }
        self.wsl_git = wsl_git;
        Ok(self)
    }
}

fn normalize_native_git_settings(
    settings: NativeGitProxySettings,
) -> Result<NativeGitProxySettings, ProxySettingsValidationError> {
    match settings {
        NativeGitProxySettings::UseExistingGitConfig => {
            Ok(NativeGitProxySettings::UseExistingGitConfig)
        }
        NativeGitProxySettings::UseProxy { proxy_url, scope } => {
            Ok(NativeGitProxySettings::UseProxy {
                proxy_url: normalize_proxy_url(&proxy_url)?,
                scope,
            })
        }
    }
}

fn normalize_wsl_git_settings(
    settings: WslGitProxySettings,
) -> Result<WslGitProxySettings, ProxySettingsValidationError> {
    match settings {
        WslGitProxySettings::FollowNativeGit => Ok(WslGitProxySettings::FollowNativeGit),
        WslGitProxySettings::UseExistingGitConfig => Ok(WslGitProxySettings::UseExistingGitConfig),
        WslGitProxySettings::UseProxy { proxy_url, scope } => Ok(WslGitProxySettings::UseProxy {
            proxy_url: normalize_proxy_url(&proxy_url)?,
            scope,
        }),
    }
}

fn normalize_distro_name(raw_distro: &str) -> Result<String, ProxySettingsValidationError> {
    let distro = raw_distro.trim();
    if distro.is_empty() {
        Err(ProxySettingsValidationError::invalid_proxy_url())
    } else {
        Ok(distro.to_string())
    }
}

fn normalize_proxy_url(raw_proxy_url: &str) -> Result<String, ProxySettingsValidationError> {
    let parsed = url::Url::parse(raw_proxy_url)
        .map_err(|_| ProxySettingsValidationError::invalid_proxy_url())?;
    let explicit_port = explicit_url_port(raw_proxy_url)
        .ok_or_else(ProxySettingsValidationError::invalid_proxy_url)?;
    let supported_scheme = matches!(parsed.scheme(), "http" | "https");
    let root_path = parsed.path().is_empty() || parsed.path() == "/";
    if !supported_scheme
        || parsed.host_str().is_none()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || !root_path
        || parsed.query().is_some()
        || parsed.fragment().is_some()
    {
        return Err(ProxySettingsValidationError::invalid_proxy_url());
    }
    Ok(format!(
        "{}://{}:{explicit_port}",
        parsed.scheme(),
        parsed
            .host()
            .expect("validated proxy URL must contain a host")
    ))
}

fn explicit_url_port(raw_url: &str) -> Option<u16> {
    let (_, remainder) = raw_url.split_once("://")?;
    let authority = remainder.split(['/', '?', '#']).next()?;
    let port = if authority.starts_with('[') {
        authority.rsplit_once("]:")?.1
    } else {
        authority.rsplit_once(':')?.1
    };
    port.parse().ok()
}

impl Default for NetworkProxySettings {
    fn default() -> Self {
        Self::default_settings()
    }
}

fn default_network_proxy_settings() -> NetworkProxySettings {
    NetworkProxySettings::default_settings()
}

fn default_git_clone_timeout_secs() -> u32 {
    120
}

/// Skill Deck 应用配置
/// 持久化到 ~/.skill-deck/config.json
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct SkillDeckConfig {
    /// 已保存的项目路径列表
    #[serde(default)]
    pub projects: Vec<String>,
    /// Git 仓库拉取超时（秒）
    #[serde(default = "default_git_clone_timeout_secs")]
    pub git_clone_timeout_secs: u32,
    /// 是否允许 Skill Deck 发现和使用 WSL Environment
    #[serde(default)]
    pub wsl_integration_enabled: bool,
    #[serde(default)]
    pub hidden_wsl_distros: Vec<String>,
    #[serde(default)]
    pub last_selected_environment: Option<EnvironmentRef>,
    #[serde(default)]
    pub last_connected_wsl_user_by_distro: std::collections::BTreeMap<String, String>,
    #[serde(default = "default_network_proxy_settings")]
    pub network_proxy: NetworkProxySettings,
}

impl Default for SkillDeckConfig {
    fn default() -> Self {
        Self {
            projects: Vec::new(),
            git_clone_timeout_secs: default_git_clone_timeout_secs(),
            wsl_integration_enabled: false,
            hidden_wsl_distros: Vec::new(),
            last_selected_environment: None,
            last_connected_wsl_user_by_distro: std::collections::BTreeMap::new(),
            network_proxy: NetworkProxySettings::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        GitProxyScope, NativeGitProxySettings, NetworkProxySettings, ProxyMode, SkillDeckConfig,
        WslGitProxySettings,
    };

    #[test]
    fn test_default_config_includes_clone_timeout() {
        let config = SkillDeckConfig::default();
        assert_eq!(config.git_clone_timeout_secs, 120);
        assert!(!config.wsl_integration_enabled);
        assert_eq!(config.network_proxy.mode, ProxyMode::Direct);
        assert_eq!(
            config.network_proxy.native_git,
            NativeGitProxySettings::UseExistingGitConfig
        );
        assert!(config.network_proxy.wsl_git.is_empty());
    }

    #[test]
    fn test_legacy_config_without_timeout_uses_default() {
        let config: SkillDeckConfig =
            serde_json::from_str(r#"{"projects":["/demo"]}"#).expect("config");

        assert_eq!(config.projects, vec!["/demo"]);
        assert_eq!(config.git_clone_timeout_secs, 120);
        assert!(!config.wsl_integration_enabled);
        assert_eq!(config.network_proxy.mode, ProxyMode::Direct);
        assert_eq!(
            config.network_proxy.native_git,
            NativeGitProxySettings::UseExistingGitConfig
        );
        assert!(config.network_proxy.wsl_git.is_empty());
    }

    #[test]
    fn typed_git_proxy_settings_round_trip_as_complete_values() {
        let settings = NetworkProxySettings {
            native_git: NativeGitProxySettings::UseProxy {
                proxy_url: "HTTP://Native.Proxy:7890/".to_string(),
                scope: GitProxyScope::GithubOnly,
            },
            wsl_git: [
                ("Ubuntu".to_string(), WslGitProxySettings::FollowNativeGit),
                (
                    "Debian".to_string(),
                    WslGitProxySettings::UseProxy {
                        proxy_url: "HTTPS://Wsl.Proxy:8443/".to_string(),
                        scope: GitProxyScope::AllHttpHttps,
                    },
                ),
            ]
            .into_iter()
            .collect(),
            ..NetworkProxySettings::default()
        }
        .validate_and_normalize()
        .expect("typed proxy settings");

        let value = serde_json::to_value(&settings).expect("serialized settings");
        assert_eq!(
            value["nativeGit"],
            serde_json::json!({
                "behavior": "useProxy",
                "proxyUrl": "http://native.proxy:7890",
                "scope": "githubOnly"
            })
        );
        assert_eq!(
            value["wslGit"]["Debian"],
            serde_json::json!({
                "behavior": "useProxy",
                "proxyUrl": "https://wsl.proxy:8443",
                "scope": "allHttpHttps"
            })
        );
        assert_eq!(
            serde_json::from_value::<NetworkProxySettings>(value).expect("round trip"),
            settings
        );
    }

    #[test]
    fn custom_proxy_requires_supported_url_with_explicit_port() {
        let valid = NetworkProxySettings {
            mode: ProxyMode::Custom,
            custom_proxy_url: Some("HTTP://Proxy.Example:7890/".to_string()),
            ..NetworkProxySettings::default()
        }
        .validate_and_normalize()
        .expect("valid custom proxy");

        assert_eq!(
            valid.custom_proxy_url.as_deref(),
            Some("http://proxy.example:7890")
        );

        for invalid in [
            "socks5://proxy.example:1080",
            "http://proxy.example",
            "http://user:secret@proxy.example:7890",
            "http://proxy.example:7890/path",
            "http://proxy.example:7890?mode=fast",
            "http://proxy.example:7890/#fragment",
        ] {
            let error = NetworkProxySettings {
                mode: ProxyMode::Custom,
                custom_proxy_url: Some(invalid.to_string()),
                ..NetworkProxySettings::default()
            }
            .validate_and_normalize()
            .expect_err("invalid custom proxy");

            assert_eq!(error.code(), "invalidProxyUrl", "{invalid}");
        }
    }

    #[test]
    fn custom_proxy_accepts_explicit_default_ports() {
        for (raw, normalized) in [
            ("http://proxy.example:80", "http://proxy.example:80"),
            ("https://proxy.example:443", "https://proxy.example:443"),
        ] {
            let settings = NetworkProxySettings {
                mode: ProxyMode::Custom,
                custom_proxy_url: Some(raw.to_string()),
                ..NetworkProxySettings::default()
            }
            .validate_and_normalize()
            .expect("explicit default proxy port");

            assert_eq!(settings.custom_proxy_url.as_deref(), Some(normalized));
        }
    }

    #[test]
    fn native_git_proxy_is_valid_when_http_requests_connect_directly() {
        let settings = NetworkProxySettings {
            mode: ProxyMode::Direct,
            native_git: NativeGitProxySettings::UseProxy {
                proxy_url: "HTTP://Proxy.Example:7890/".to_string(),
                scope: GitProxyScope::GithubOnly,
            },
            ..NetworkProxySettings::default()
        }
        .validate_and_normalize()
        .expect("independent native Git proxy");

        assert_eq!(settings.mode, ProxyMode::Direct);
        assert_eq!(settings.custom_proxy_url, None);
        assert_eq!(
            settings.native_git,
            NativeGitProxySettings::UseProxy {
                proxy_url: "http://proxy.example:7890".to_string(),
                scope: GitProxyScope::GithubOnly,
            }
        );
    }

    #[test]
    fn native_git_proxy_url_is_validated_independently() {
        let error = NetworkProxySettings {
            mode: ProxyMode::Custom,
            custom_proxy_url: Some("http://http.proxy:7890".to_string()),
            native_git: NativeGitProxySettings::UseProxy {
                proxy_url: "http://git.proxy".to_string(),
                scope: GitProxyScope::GithubOnly,
            },
            ..NetworkProxySettings::default()
        }
        .validate_and_normalize()
        .expect_err("new Git proxy settings require their own URL");

        assert_eq!(error.code(), "invalidProxyUrl");
    }

    #[test]
    fn unpublished_git_proxy_settings_are_rejected() {
        for settings in [
            r#"{
                "mode":"custom",
                "customProxyUrl":"http://127.0.0.1:7890",
                "nativeGit":"followProxySettings",
                "wslGitProxyUrls":{
                    "Ubuntu":"http://127.0.0.1:7890",
                    "Debian":"http://172.20.0.1:7890"
                }
            }"#,
            r#"{
                "mode":"direct",
                "customProxyUrl":null,
                "nativeGit":{"behavior":"useExistingGitConfig"},
                "wslGit":{},
                "nativeGitProxyUrl":"http://127.0.0.1:7890"
            }"#,
        ] {
            serde_json::from_str::<NetworkProxySettings>(settings)
                .expect_err("unpublished proxy settings must not be accepted");
        }
    }

    #[test]
    fn proxy_normalization_supports_ipv4_ipv6_and_idn_hosts() {
        for (raw, normalized) in [
            ("http://192.0.2.10:8080", "http://192.0.2.10:8080"),
            ("https://[2001:db8::1]:8443", "https://[2001:db8::1]:8443"),
            (
                "http://代理.example:7890",
                "http://xn--mnq481g.example:7890",
            ),
        ] {
            let settings = NetworkProxySettings {
                mode: ProxyMode::Custom,
                custom_proxy_url: Some(raw.to_string()),
                ..NetworkProxySettings::default()
            }
            .validate_and_normalize()
            .expect("valid proxy host");

            assert_eq!(settings.custom_proxy_url.as_deref(), Some(normalized));
        }
    }

    #[test]
    fn unsupported_system_proxy_mode_is_rejected() {
        let error = serde_json::from_str::<NetworkProxySettings>(
            r#"{
                "mode":"system",
                "customProxyUrl":"http://127.0.0.1:7890",
                "bypassRules":["example.com"],
                "nativeGit":"followProxySettings",
                "wslGitDefault":"followProxySettings",
                "wslGitOverrides":{"Ubuntu":"followProxySettings"}
            }"#,
        )
        .expect_err("system mode was never released and must not be accepted");

        assert!(error.to_string().contains("unknown variant `system`"));
    }

    #[test]
    fn unsupported_legacy_proxy_fields_are_rejected() {
        for (field, value) in [
            ("bypassRules", serde_json::json!(["example.com"])),
            ("wslGitDefault", serde_json::json!("followProxySettings")),
            (
                "wslGitOverrides",
                serde_json::json!({"Ubuntu": "followProxySettings"}),
            ),
        ] {
            let mut settings = serde_json::json!({
                "mode": "direct",
                "customProxyUrl": null,
                "nativeGit": {"behavior": "useExistingGitConfig"},
                "wslGit": {}
            });
            settings
                .as_object_mut()
                .expect("proxy settings object")
                .insert(field.to_string(), value);

            let error = serde_json::from_value::<NetworkProxySettings>(settings)
                .expect_err("unpublished proxy fields must not be accepted");

            assert!(error.to_string().contains("unknown field"), "{field}");
        }
    }

    #[test]
    fn wsl_git_proxy_settings_are_validated_independently() {
        let settings = NetworkProxySettings {
            mode: ProxyMode::Custom,
            custom_proxy_url: Some("http://127.0.0.1:7890".to_string()),
            wsl_git: [
                (
                    "Ubuntu".to_string(),
                    WslGitProxySettings::UseProxy {
                        proxy_url: "HTTP://Proxy.Example:8080/".to_string(),
                        scope: GitProxyScope::GithubOnly,
                    },
                ),
                (
                    "Debian".to_string(),
                    WslGitProxySettings::UseProxy {
                        proxy_url: "https://[::1]:8443".to_string(),
                        scope: GitProxyScope::AllHttpHttps,
                    },
                ),
            ]
            .into_iter()
            .collect(),
            ..NetworkProxySettings::default()
        }
        .validate_and_normalize()
        .expect("valid WSL proxies");

        assert_eq!(
            settings.wsl_git.get("Ubuntu").cloned(),
            Some(WslGitProxySettings::UseProxy {
                proxy_url: "http://proxy.example:8080".to_string(),
                scope: GitProxyScope::GithubOnly,
            })
        );
        assert_eq!(
            settings.wsl_git.get("Debian").cloned(),
            Some(WslGitProxySettings::UseProxy {
                proxy_url: "https://[::1]:8443".to_string(),
                scope: GitProxyScope::AllHttpHttps,
            })
        );

        let error = NetworkProxySettings {
            wsl_git: [(
                "Ubuntu".to_string(),
                WslGitProxySettings::UseProxy {
                    proxy_url: "http://proxy.example".to_string(),
                    scope: GitProxyScope::GithubOnly,
                },
            )]
            .into_iter()
            .collect(),
            ..NetworkProxySettings::default()
        }
        .validate_and_normalize()
        .expect_err("invalid WSL proxy");
        assert_eq!(error.code(), "invalidProxyUrl");
    }
}

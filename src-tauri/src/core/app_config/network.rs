use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::models::{
    NativeGitProxySettings, NetworkProxySettings, ProxySettingsIssue, ProxySettingsIssueCode,
    ProxySettingsTarget, WslGitProxySettings,
};

pub(super) fn decode(value: Option<&Value>) -> (NetworkProxySettings, Vec<ProxySettingsIssue>) {
    let mut settings = NetworkProxySettings::default();
    let Some(value) = value else {
        return (settings, Vec::new());
    };
    let Some(object) = value.as_object() else {
        return (
            settings,
            vec![issue(
                ProxySettingsTarget::All,
                ProxySettingsIssueCode::InvalidFormat,
            )],
        );
    };
    if object.keys().any(|key| {
        !matches!(
            key.as_str(),
            "mode" | "customProxyUrl" | "nativeGit" | "wslGit"
        )
    }) {
        return (
            settings,
            vec![issue(
                ProxySettingsTarget::All,
                ProxySettingsIssueCode::UnsupportedFormat,
            )],
        );
    }

    let mut issues = Vec::new();
    let http = serde_json::json!({
        "mode": object.get("mode"),
        "customProxyUrl": object.get("customProxyUrl"),
    });
    match parse::<NetworkProxySettings>(http).and_then(normalize) {
        Ok(http) => {
            settings.mode = http.mode;
            settings.custom_proxy_url = http.custom_proxy_url;
        }
        Err(code) => issues.push(issue(ProxySettingsTarget::Http, code)),
    }

    if let Some(native) = object.get("nativeGit") {
        let native = parse::<NativeGitProxySettings>(native.clone()).and_then(|native_git| {
            normalize(NetworkProxySettings {
                native_git,
                ..Default::default()
            })
        });
        match native {
            Ok(native) => settings.native_git = native.native_git,
            Err(code) => issues.push(issue(ProxySettingsTarget::NativeGit, code)),
        }
    }

    if let Some(wsl) = object.get("wslGit") {
        if let Some(wsl) = wsl.as_object() {
            for (distro, value) in wsl {
                let target = ProxySettingsTarget::WslGit {
                    distro: Some(distro.trim().to_string()),
                };
                let entry = parse::<WslGitProxySettings>(value.clone()).and_then(|entry| {
                    normalize(NetworkProxySettings {
                        wsl_git: [(distro.clone(), entry)].into_iter().collect(),
                        ..Default::default()
                    })
                });
                match entry {
                    Ok(entry) => settings.wsl_git.extend(entry.wsl_git),
                    Err(code) => issues.push(issue(target, code)),
                }
            }
        } else {
            issues.push(issue(
                ProxySettingsTarget::WslGit { distro: None },
                ProxySettingsIssueCode::InvalidFormat,
            ));
        }
    }
    (settings, issues)
}

fn parse<T: DeserializeOwned>(value: Value) -> Result<T, ProxySettingsIssueCode> {
    serde_json::from_value(value).map_err(|_| ProxySettingsIssueCode::InvalidFormat)
}

fn normalize(
    settings: NetworkProxySettings,
) -> Result<NetworkProxySettings, ProxySettingsIssueCode> {
    settings
        .validate_and_normalize()
        .map_err(|_| ProxySettingsIssueCode::InvalidProxyUrl)
}

pub(super) fn issue(
    target: ProxySettingsTarget,
    code: ProxySettingsIssueCode,
) -> ProxySettingsIssue {
    ProxySettingsIssue {
        target,
        code,
        using_previous: false,
    }
}

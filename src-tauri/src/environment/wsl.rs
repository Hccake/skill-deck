use std::collections::{BTreeMap, HashMap};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use specta::Type;
use tokio::process::Command;
use tokio::time::{timeout, Duration};

use crate::error::AppError;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct WslSession {
    pub distro_name: String,
    pub user: String,
    pub uid: u32,
    pub home: String,
    pub xdg_state_home: Option<String>,
    pub config_home: String,
    pub environment: BTreeMap<String, String>,
    pub git_available: bool,
}

#[derive(Default)]
pub struct EnvironmentRegistry {
    sessions: Mutex<HashMap<String, WslSession>>,
}

impl EnvironmentRegistry {
    pub fn insert(&self, session: WslSession) {
        self.sessions
            .lock()
            .expect("environment registry lock poisoned")
            .insert(session.distro_name.clone(), session);
    }

    pub fn get(&self, distro_name: &str) -> Option<WslSession> {
        self.sessions
            .lock()
            .expect("environment registry lock poisoned")
            .get(distro_name)
            .cloned()
    }
}

pub fn parse_wsl_list_output(bytes: &[u8]) -> Vec<String> {
    let decoded = if bytes.len() >= 2 && bytes.len() % 2 == 0 {
        let utf16 = bytes
            .chunks_exact(2)
            .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
            .collect::<Vec<_>>();
        String::from_utf16(&utf16).unwrap_or_else(|_| String::from_utf8_lossy(bytes).into_owned())
    } else {
        String::from_utf8_lossy(bytes).into_owned()
    };
    decoded
        .lines()
        .map(|line| line.trim_matches(['\0', '\r', ' ', '\t']))
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect()
}

pub fn parse_wsl_session_output(distro_name: &str, bytes: &[u8]) -> Result<WslSession, AppError> {
    let mut fields = bytes
        .split(|byte| *byte == 0)
        .map(|field| String::from_utf8_lossy(field).into_owned())
        .collect::<Vec<_>>();
    if fields.last().is_some_and(String::is_empty) {
        fields.pop();
    }
    if fields.len() != 12 || fields[0] != "1" {
        return Err(AppError::Custom {
            message: "invalid WSL session response".to_string(),
        });
    }
    let environment = [
        ("CODEX_HOME", 6usize),
        ("CLAUDE_CONFIG_DIR", 7),
        ("VIBE_HOME", 8),
        ("HERMES_HOME", 9),
        ("AUTOHAND_HOME", 10),
    ]
    .into_iter()
    .filter_map(|(name, index)| {
        (!fields[index].is_empty()).then(|| (name.to_string(), fields[index].clone()))
    })
    .collect();
    Ok(WslSession {
        distro_name: distro_name.to_string(),
        user: fields[1].clone(),
        uid: fields[2].parse().map_err(|_| AppError::Custom {
            message: "invalid WSL uid".to_string(),
        })?,
        home: fields[3].clone(),
        xdg_state_home: (!fields[4].is_empty()).then(|| fields[4].clone()),
        config_home: fields[5].clone(),
        environment,
        git_available: fields[11] == "1",
    })
}

#[cfg(target_os = "windows")]
pub async fn discover_wsl_distributions() -> Result<Vec<String>, AppError> {
    let mut command = Command::new("wsl.exe");
    command.args(["--list", "--quiet"]);
    let output = timeout(Duration::from_secs(10), command.output())
        .await
        .map_err(|_| AppError::Custom {
            message: "WSL discovery timed out".to_string(),
        })??;
    if !output.status.success() {
        return Err(AppError::Custom {
            message: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        });
    }
    Ok(parse_wsl_list_output(&output.stdout))
}

#[cfg(not(target_os = "windows"))]
pub async fn discover_wsl_distributions() -> Result<Vec<String>, AppError> {
    Ok(Vec::new())
}

#[cfg(target_os = "windows")]
pub async fn connect_wsl_environment(distro_name: &str) -> Result<WslSession, AppError> {
    const SCRIPT: &str = r#"printf '1\0'; id -un | tr -d '\n'; printf '\0'; id -u | tr -d '\n'; printf '\0'; printf '%s\0' "$HOME" "${XDG_STATE_HOME:-}" "${XDG_CONFIG_HOME:-$HOME/.config}" "${CODEX_HOME:-}" "${CLAUDE_CONFIG_DIR:-}" "${VIBE_HOME:-}" "${HERMES_HOME:-}" "${AUTOHAND_HOME:-}"; if command -v git >/dev/null 2>&1; then printf '1\0'; else printf '0\0'; fi"#;
    let mut command = Command::new("wsl.exe");
    command.args([
        "--distribution",
        distro_name,
        "--exec",
        "/bin/sh",
        "-c",
        SCRIPT,
    ]);
    let output = timeout(Duration::from_secs(15), command.output())
        .await
        .map_err(|_| AppError::Custom {
            message: format!("connecting to WSL distro '{distro_name}' timed out"),
        })??;
    if !output.status.success() {
        return Err(AppError::Custom {
            message: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        });
    }
    parse_wsl_session_output(distro_name, &output.stdout)
}

#[cfg(not(target_os = "windows"))]
pub async fn connect_wsl_environment(_distro_name: &str) -> Result<WslSession, AppError> {
    Err(AppError::Custom {
        message: "WSL is only available on Windows".to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::{parse_wsl_list_output, parse_wsl_session_output, EnvironmentRegistry, WslSession};

    #[test]
    fn parses_utf16_wsl_list_and_removes_nul_and_blank_lines() {
        let text = "Ubuntu-24.04\0\r\nDebian\0\r\n\r\n";
        let bytes = text
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>();

        assert_eq!(
            parse_wsl_list_output(&bytes),
            vec!["Ubuntu-24.04", "Debian"]
        );
    }

    #[test]
    fn registry_keeps_successful_sessions_by_distro() {
        let registry = EnvironmentRegistry::default();
        registry.insert(WslSession {
            distro_name: "Ubuntu".to_string(),
            user: "alice".to_string(),
            uid: 1000,
            home: "/home/alice".to_string(),
            xdg_state_home: None,
            config_home: "/home/alice/.config".to_string(),
            environment: std::collections::BTreeMap::new(),
            git_available: true,
        });

        assert_eq!(registry.get("Ubuntu").expect("session").user, "alice");
        assert!(registry.get("Debian").is_none());
    }

    #[test]
    fn parses_versioned_session_output() {
        let output = b"1\0alice\01000\0/home/alice\0/home/alice/.state\0/home/alice/.config\0/opt/codex\0/opt/claude\0\0\0\01\0";
        let session = parse_wsl_session_output("Ubuntu", output).expect("parse session");

        assert_eq!(session.user, "alice");
        assert_eq!(session.uid, 1000);
        assert_eq!(session.home, "/home/alice");
        assert_eq!(
            session.xdg_state_home.as_deref(),
            Some("/home/alice/.state")
        );
        assert_eq!(session.config_home, "/home/alice/.config");
        assert_eq!(session.environment["CODEX_HOME"], "/opt/codex");
        assert_eq!(session.environment["CLAUDE_CONFIG_DIR"], "/opt/claude");
        assert!(session.git_available);
    }

    #[test]
    fn parses_empty_xdg_state_home_without_shifting_fields() {
        let output = b"1\0alice\01000\0/home/alice\0\0/home/alice/.config\0\0\0\0\0\01\0";
        let session = parse_wsl_session_output("Ubuntu", output).expect("parse session");

        assert_eq!(session.xdg_state_home, None);
        assert!(session.git_available);
    }
}

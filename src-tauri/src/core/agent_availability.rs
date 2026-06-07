use serde::{Deserialize, Serialize};
use specta::Type;
use std::path::{Path, PathBuf};

use crate::core::agents::AgentType;
use crate::core::paths::PATHS;
use crate::core::skill::sanitize_name;
use crate::models::{AgentPresenceInfo, AgentSkillPresence};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "kebab-case")]
#[specta(rename_all = "kebab-case")]
pub enum AgentAvailabilityKind {
    SharedOnly,
    SharedCompatible,
    PrivateRequired,
    Unknown,
    Unsupported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "kebab-case")]
#[specta(rename_all = "kebab-case")]
pub enum SharedSupportConfidence {
    Official,
    Inferred,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct AgentAvailability {
    pub supported: bool,
    pub default_available: bool,
    pub kind: AgentAvailabilityKind,
    pub confidence: SharedSupportConfidence,
    pub shared_path: String,
    pub install_path: String,
    pub read_paths: Vec<String>,
    pub private_path: Option<String>,
}

pub fn shared_global_dir() -> PathBuf {
    PATHS.home.join(".agents").join("skills")
}

pub fn shared_project_dir(cwd: &str) -> PathBuf {
    PathBuf::from(cwd).join(".agents").join("skills")
}

pub fn availability_for_agent(agent: AgentType, is_global: bool, cwd: &str) -> AgentAvailability {
    if is_global {
        global_availability_for_agent(agent)
    } else {
        project_availability_for_agent(agent, cwd)
    }
}

pub fn default_available_agents(is_global: bool, cwd: &str) -> Vec<AgentType> {
    AgentType::all()
        .filter(|agent| availability_for_agent(*agent, is_global, cwd).default_available)
        .collect()
}

pub fn detect_agent_presence(
    agent: AgentType,
    skill_name: &str,
    is_global: bool,
    cwd: &str,
) -> AgentPresenceInfo {
    let availability = availability_for_agent(agent, is_global, cwd);
    let sanitized_name = sanitize_name(skill_name);
    let shared_skill_path = PathBuf::from(&availability.shared_path).join(&sanitized_name);
    let private_skill_path = availability
        .private_path
        .as_ref()
        .map(|path| PathBuf::from(path).join(&sanitized_name));
    let shared_exists = shared_skill_path.exists();
    let private_exists = private_skill_path
        .as_ref()
        .is_some_and(|path| path.exists());

    let presence = if shared_exists && private_exists && availability.default_available {
        AgentSkillPresence::DuplicateCopy
    } else if shared_exists && !private_exists && availability.default_available {
        AgentSkillPresence::DefaultActive
    } else if private_exists {
        AgentSkillPresence::PrivateOnly
    } else if shared_exists && !availability.default_available && !private_exists {
        AgentSkillPresence::RequiresPrivateInstall
    } else {
        AgentSkillPresence::NotInstalled
    };

    AgentPresenceInfo {
        agent,
        display_name: agent.config().display_name.to_string(),
        presence: presence.clone(),
        shared_path: path_string(&shared_skill_path),
        private_path: private_skill_path.as_ref().map(|path| path_string(path)),
        can_cleanup_private_copy: matches!(presence, AgentSkillPresence::DuplicateCopy),
    }
}

fn global_availability_for_agent(agent: AgentType) -> AgentAvailability {
    let config = agent.config();
    let shared_path = shared_global_dir();
    let Some(private_path) = config.global_skills_dir else {
        return AgentAvailability {
            supported: false,
            default_available: false,
            kind: AgentAvailabilityKind::Unsupported,
            confidence: SharedSupportConfidence::Unknown,
            shared_path: path_string(&shared_path),
            install_path: path_string(&shared_path),
            read_paths: Vec::new(),
            private_path: None,
        };
    };

    let shared_path_string = path_string(&shared_path);
    let private_path_string = path_string(&private_path);
    let official_support = global_official_support(agent);

    if matches!(official_support, OfficialSharedSupport::No) {
        return AgentAvailability {
            supported: true,
            default_available: false,
            kind: AgentAvailabilityKind::PrivateRequired,
            confidence: SharedSupportConfidence::Official,
            shared_path: shared_path_string,
            install_path: private_path_string.clone(),
            read_paths: vec![private_path_string.clone()],
            private_path: Some(private_path_string),
        };
    }

    if same_normalized_path(&private_path, &shared_path) {
        return AgentAvailability {
            supported: true,
            default_available: true,
            kind: AgentAvailabilityKind::SharedOnly,
            confidence: match official_support {
                OfficialSharedSupport::Yes => SharedSupportConfidence::Official,
                OfficialSharedSupport::Unknown => SharedSupportConfidence::Inferred,
                OfficialSharedSupport::No => SharedSupportConfidence::Official,
            },
            shared_path: shared_path_string.clone(),
            install_path: shared_path_string.clone(),
            read_paths: vec![shared_path_string],
            private_path: None,
        };
    }

    match official_support {
        OfficialSharedSupport::Yes => AgentAvailability {
            supported: true,
            default_available: true,
            kind: AgentAvailabilityKind::SharedCompatible,
            confidence: SharedSupportConfidence::Official,
            shared_path: shared_path_string.clone(),
            install_path: shared_path_string.clone(),
            read_paths: vec![shared_path_string, private_path_string.clone()],
            private_path: Some(private_path_string),
        },
        OfficialSharedSupport::Unknown => AgentAvailability {
            supported: true,
            default_available: false,
            kind: AgentAvailabilityKind::Unknown,
            confidence: SharedSupportConfidence::Unknown,
            shared_path: shared_path_string,
            install_path: private_path_string.clone(),
            read_paths: vec![private_path_string.clone()],
            private_path: Some(private_path_string),
        },
        OfficialSharedSupport::No => {
            unreachable!("official no handled before shared path inference")
        }
    }
}

fn project_availability_for_agent(agent: AgentType, cwd: &str) -> AgentAvailability {
    let config = agent.config();
    let supported = !config.skills_dir.trim().is_empty();
    let shared_path = shared_project_dir(cwd);
    let private_path = PathBuf::from(cwd).join(config.skills_dir);
    let shared_path_string = path_string(&shared_path);
    let private_path_string = path_string(&private_path);

    if !supported {
        return AgentAvailability {
            supported: false,
            default_available: false,
            kind: AgentAvailabilityKind::Unsupported,
            confidence: SharedSupportConfidence::Unknown,
            shared_path: shared_path_string.clone(),
            install_path: shared_path_string,
            read_paths: Vec::new(),
            private_path: None,
        };
    }

    if same_normalized_path(&private_path, &shared_path) {
        return AgentAvailability {
            supported: true,
            default_available: true,
            kind: AgentAvailabilityKind::SharedOnly,
            confidence: SharedSupportConfidence::Inferred,
            shared_path: shared_path_string.clone(),
            install_path: shared_path_string.clone(),
            read_paths: vec![shared_path_string],
            private_path: None,
        };
    }

    AgentAvailability {
        supported: true,
        default_available: false,
        kind: AgentAvailabilityKind::PrivateRequired,
        confidence: SharedSupportConfidence::Inferred,
        shared_path: shared_path_string,
        install_path: private_path_string.clone(),
        read_paths: vec![private_path_string.clone()],
        private_path: Some(private_path_string),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OfficialSharedSupport {
    Yes,
    No,
    Unknown,
}

fn global_official_support(agent: AgentType) -> OfficialSharedSupport {
    match agent {
        AgentType::Codex
        | AgentType::GithubCopilot
        | AgentType::GeminiCli
        | AgentType::Opencode
        | AgentType::Warp
        | AgentType::Zed
        | AgentType::Firebender
        | AgentType::KimiCodeCli => OfficialSharedSupport::Yes,
        AgentType::Amp | AgentType::Antigravity | AgentType::Cline | AgentType::Deepagents => {
            OfficialSharedSupport::No
        }
        _ => OfficialSharedSupport::Unknown,
    }
}

fn same_normalized_path(left: &Path, right: &Path) -> bool {
    // Syntactic comparison for paths constructed here; does not resolve symlinks or canonicalize.
    left.components().collect::<Vec<_>>() == right.components().collect::<Vec<_>>()
}

fn path_string(path: &Path) -> String {
    path.to_string_lossy().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::AgentSkillPresence;
    use tempfile::tempdir;

    fn write_skill(dir: &Path, name: &str) {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(
            dir.join("SKILL.md"),
            format!("---\nname: {}\ndescription: Demo\n---\n", name),
        )
        .unwrap();
    }

    #[test]
    fn test_detect_project_presence_default_active_and_private_only() {
        let project = tempdir().unwrap();
        let cwd = project.path().to_string_lossy().to_string();
        write_skill(
            &project.path().join(".agents").join("skills").join("demo"),
            "demo",
        );
        write_skill(
            &project.path().join(".claude").join("skills").join("demo"),
            "demo",
        );

        let codex = detect_agent_presence(AgentType::Codex, "demo", false, &cwd);
        let claude = detect_agent_presence(AgentType::ClaudeCode, "demo", false, &cwd);
        let kiro = detect_agent_presence(AgentType::KiroCli, "demo", false, &cwd);

        assert_eq!(codex.presence, AgentSkillPresence::DefaultActive);
        assert_eq!(claude.presence, AgentSkillPresence::PrivateOnly);
        assert_eq!(kiro.presence, AgentSkillPresence::RequiresPrivateInstall);
    }
}

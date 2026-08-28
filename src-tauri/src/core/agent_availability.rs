use serde::{Deserialize, Serialize};
use specta::Type;
use std::path::{Path, PathBuf};

use crate::core::agent_definition::AgentId;
use crate::core::skill::sanitize_name;
use crate::environment::agent_environment::{ResolvedAgent, ResolvedAgentScope};
use crate::models::{AgentSkillPresence, SkillAgentPresenceInfo};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "kebab-case")]
#[specta(rename_all = "kebab-case")]
pub enum AgentAvailabilityKind {
    StandardOnly,
    StandardCompatible,
    PrivateRequired,
    Unknown,
    Unsupported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "kebab-case")]
#[specta(rename_all = "kebab-case")]
pub enum StandardSupportConfidence {
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
    pub confidence: StandardSupportConfidence,
    pub standard_path: String,
    pub install_path: String,
    pub read_paths: Vec<String>,
    pub private_path: Option<String>,
}

pub fn availability_for_resolved_scope(scope: &ResolvedAgentScope) -> AgentAvailability {
    let standard_path = scope.standard_path.clone().unwrap_or_default();
    let private_path = scope.private_path.clone();
    let (kind, default_available) = if !scope.enabled {
        (AgentAvailabilityKind::Unsupported, false)
    } else if scope.reads_standard && private_path.is_none() {
        (AgentAvailabilityKind::StandardOnly, true)
    } else if scope.reads_standard {
        (AgentAvailabilityKind::StandardCompatible, true)
    } else if private_path.is_some() {
        (AgentAvailabilityKind::PrivateRequired, false)
    } else {
        (AgentAvailabilityKind::Unknown, false)
    };
    let install_path = if default_available {
        standard_path.clone()
    } else {
        private_path
            .clone()
            .unwrap_or_else(|| standard_path.clone())
    };

    AgentAvailability {
        supported: scope.enabled,
        default_available,
        kind,
        confidence: StandardSupportConfidence::Inferred,
        standard_path,
        install_path,
        read_paths: scope.read_paths.clone(),
        private_path,
    }
}

pub fn resolved_agent_presence_from_paths(
    agent_id: &AgentId,
    resolved: &ResolvedAgent,
    skill_name: &str,
    is_global: bool,
    standard_exists: bool,
    private_exists: bool,
) -> SkillAgentPresenceInfo {
    let scope = if is_global {
        &resolved.global
    } else {
        &resolved.project
    };
    let availability = availability_for_resolved_scope(scope);
    let sanitized_name = sanitize_name(skill_name);
    let standard_path = scope
        .standard_path
        .as_ref()
        .map(|path| PathBuf::from(path).join(&sanitized_name))
        .unwrap_or_default();
    let private_path = scope
        .private_path
        .as_ref()
        .map(|path| PathBuf::from(path).join(&sanitized_name));
    let presence = if standard_exists && private_exists && availability.default_available {
        AgentSkillPresence::DuplicateCopy
    } else if standard_exists && availability.default_available {
        AgentSkillPresence::DefaultActive
    } else if private_exists {
        AgentSkillPresence::PrivateOnly
    } else if standard_exists && scope.enabled && !availability.default_available {
        AgentSkillPresence::RequiresPrivateInstall
    } else {
        AgentSkillPresence::NotInstalled
    };

    SkillAgentPresenceInfo {
        agent: agent_id.clone(),
        display_name: resolved.definition.display_name.clone(),
        presence: presence.clone(),
        standard_path: path_string(&standard_path),
        private_path: private_path.as_ref().map(|path| path_string(path)),
        can_cleanup_private_copy: matches!(presence, AgentSkillPresence::DuplicateCopy),
    }
}

fn path_string(path: &Path) -> String {
    path.to_string_lossy().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::agent_definition::{
        AgentAdapter, AgentDefinition, AgentId, AgentSource, DetectionSpec, PathSpec,
        ScopeDefinition,
    };
    use crate::environment::agent_environment::{
        DetectionState, ResolvedAgent, ResolvedAgentScope,
    };
    use crate::models::AgentSkillPresence;

    fn resolved_custom_agent(detection: DetectionState, private_path: String) -> ResolvedAgent {
        ResolvedAgent {
            definition: AgentDefinition {
                id: AgentId::parse("my-custom-agent").unwrap(),
                display_name: "My Custom Agent".to_string(),
                source: AgentSource::Custom,
                aliases: Vec::new(),
                global: ScopeDefinition {
                    enabled: false,
                    reads_standard: false,
                    private_path: None,
                },
                project: ScopeDefinition {
                    enabled: true,
                    reads_standard: true,
                    private_path: Some(PathSpec::project(".my-custom/skills")),
                },
                detection: DetectionSpec::AnyPathExists {
                    paths: vec![PathSpec::home(".my-custom")],
                },
                legacy_paths: Vec::new(),
                adapter: AgentAdapter::Standard,
            },
            detection,
            detection_reason: None,
            global: ResolvedAgentScope {
                enabled: false,
                reads_standard: false,
                standard_path: None,
                private_path: None,
                read_paths: Vec::new(),
                standard_presence: None,
                private_presence: None,
                legacy_paths: Vec::new(),
            },
            project: ResolvedAgentScope {
                enabled: true,
                reads_standard: true,
                standard_path: Some("/work/app/.agents/skills".to_string()),
                private_path: Some(private_path),
                read_paths: Vec::new(),
                standard_presence: None,
                private_presence: None,
                legacy_paths: Vec::new(),
            },
        }
    }

    #[test]
    fn resolved_presence_preserves_custom_id_when_detection_is_indeterminate() {
        let agent_id = AgentId::parse("my-custom-agent").unwrap();
        let resolved = resolved_custom_agent(
            DetectionState::Indeterminate,
            "/work/app/.my-custom/skills".to_string(),
        );

        let presence =
            resolved_agent_presence_from_paths(&agent_id, &resolved, "demo", false, true, false);

        assert_eq!(presence.agent, agent_id);
        assert_eq!(presence.presence, AgentSkillPresence::DefaultActive);
        assert_eq!(presence.display_name, "My Custom Agent");
    }
}

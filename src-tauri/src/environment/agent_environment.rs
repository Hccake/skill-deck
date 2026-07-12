use std::collections::BTreeMap;
use std::path::Path;

use crate::core::agent_availability::AgentAvailabilityKind;
use crate::core::agents::{AgentScopeTarget, AgentType};
use crate::core::paths::PATHS;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentEnvironmentContext {
    pub home: String,
    pub config_home: String,
    pub env: BTreeMap<String, String>,
}

pub struct AgentEnvironmentResolver {
    context: AgentEnvironmentContext,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentEnvironmentTarget {
    pub agent: AgentType,
    pub display_name: String,
    pub shared_path: String,
    pub private_path: Option<String>,
    pub availability: AgentAvailabilityKind,
    pub default_available: bool,
    pub detection_paths: Vec<String>,
}

impl AgentEnvironmentTarget {
    pub fn scope_target(&self, is_global: bool) -> AgentScopeTarget {
        let supported = self.availability != AgentAvailabilityKind::Unsupported;
        let configured_path = self
            .private_path
            .clone()
            .unwrap_or_else(|| self.shared_path.clone());
        let read_paths = match self.availability {
            AgentAvailabilityKind::SharedOnly => vec![self.shared_path.clone()],
            AgentAvailabilityKind::SharedCompatible => {
                let mut paths = vec![self.shared_path.clone()];
                if let Some(private_path) = &self.private_path {
                    paths.push(private_path.clone());
                }
                paths
            }
            AgentAvailabilityKind::PrivateRequired | AgentAvailabilityKind::Unknown => {
                self.private_path.clone().into_iter().collect()
            }
            AgentAvailabilityKind::Unsupported => Vec::new(),
        };

        AgentScopeTarget {
            supported,
            automatic: self.default_available,
            path: if !supported {
                String::new()
            } else if is_global {
                configured_path.clone()
            } else {
                self.agent.config().skills_dir.to_string()
            },
            availability: self.availability,
            default_available: self.default_available,
            shared_path: self.shared_path.clone(),
            install_path: if self.default_available {
                self.shared_path.clone()
            } else {
                configured_path
            },
            read_paths,
            private_path: self.private_path.clone(),
        }
    }
}

impl AgentEnvironmentResolver {
    pub fn new(context: AgentEnvironmentContext) -> Self {
        Self { context }
    }

    pub fn project_skills_dir(&self, agent: AgentType, project_path: &str) -> String {
        join_posix(project_path, agent.config().skills_dir)
    }

    pub fn global_skills_dir(&self, agent: AgentType) -> Option<String> {
        let override_home = match agent {
            AgentType::Codex => self.env_home("CODEX_HOME", ".codex"),
            AgentType::ClaudeCode => self.env_home("CLAUDE_CONFIG_DIR", ".claude"),
            AgentType::MistralVibe => self.env_home("VIBE_HOME", ".vibe"),
            AgentType::HermesAgent => self.env_home("HERMES_HOME", ".hermes"),
            AgentType::AutohandCode => self.env_home("AUTOHAND_HOME", ".autohand"),
            AgentType::Openclaw => Some(join_posix(&self.context.home, ".openclaw")),
            _ => None,
        };
        if let Some(home) = override_home {
            return Some(join_posix(&home, "skills"));
        }

        let configured = agent.config().global_skills_dir?;
        if let Ok(relative) = configured.strip_prefix(&PATHS.config_home) {
            return Some(join_posix(
                &self.context.config_home,
                &path_to_posix(relative),
            ));
        }
        if let Ok(relative) = configured.strip_prefix(&PATHS.home) {
            return Some(join_posix(&self.context.home, &path_to_posix(relative)));
        }
        None
    }

    pub fn target(
        &self,
        agent: AgentType,
        is_global: bool,
        project_path: &str,
    ) -> AgentEnvironmentTarget {
        let shared_path = if is_global {
            join_posix(&self.context.home, ".agents/skills")
        } else {
            join_posix(project_path, ".agents/skills")
        };
        let configured_private = if is_global {
            self.global_skills_dir(agent)
        } else if agent.config().skills_dir.trim().is_empty() {
            None
        } else {
            Some(self.project_skills_dir(agent, project_path))
        };

        let (supported, default_available, availability) = if is_global {
            match configured_private.as_deref() {
                None => (false, false, AgentAvailabilityKind::Unsupported),
                Some(_) if matches!(global_official_support(agent), OfficialSharedSupport::No) => {
                    (true, false, AgentAvailabilityKind::PrivateRequired)
                }
                Some(private) if same_posix_path(private, &shared_path) => {
                    (true, true, AgentAvailabilityKind::SharedOnly)
                }
                Some(_) if matches!(global_official_support(agent), OfficialSharedSupport::Yes) => {
                    (true, true, AgentAvailabilityKind::SharedCompatible)
                }
                Some(_) => (true, false, AgentAvailabilityKind::Unknown),
            }
        } else {
            match configured_private.as_deref() {
                None => (false, false, AgentAvailabilityKind::Unsupported),
                Some(private) if same_posix_path(private, &shared_path) => {
                    (true, true, AgentAvailabilityKind::SharedOnly)
                }
                Some(_) => (true, false, AgentAvailabilityKind::PrivateRequired),
            }
        };
        let private_path = configured_private.filter(|path| !same_posix_path(path, &shared_path));

        AgentEnvironmentTarget {
            agent,
            display_name: agent.config().display_name.to_string(),
            shared_path,
            private_path: supported.then_some(private_path).flatten(),
            availability,
            default_available,
            detection_paths: self.detection_paths(agent, project_path),
        }
    }

    fn env_home(&self, key: &str, fallback: &str) -> Option<String> {
        Some(
            self.context
                .env
                .get(key)
                .filter(|value| !value.trim().is_empty())
                .cloned()
                .unwrap_or_else(|| join_posix(&self.context.home, fallback)),
        )
    }

    fn detection_paths(&self, agent: AgentType, project_path: &str) -> Vec<String> {
        let home = &self.context.home;
        let config = &self.context.config_home;
        let paths = match agent {
            AgentType::Amp => vec![join_posix(config, "amp")],
            AgentType::Antigravity => vec![join_posix(home, ".gemini/antigravity")],
            AgentType::AntigravityCli => vec![join_posix(home, ".gemini/antigravity-cli")],
            AgentType::Cline => vec![join_posix(home, ".cline")],
            AgentType::Codex => vec![
                self.env_home("CODEX_HOME", ".codex").expect("codex home"),
                "/etc/codex".to_string(),
            ],
            AgentType::Cursor => vec![join_posix(home, ".cursor")],
            AgentType::Deepagents => vec![join_posix(home, ".deepagents")],
            AgentType::Dexto => vec![join_posix(home, ".dexto")],
            AgentType::Eve => vec![
                join_posix(project_path, "agent"),
                join_posix(project_path, "package.json"),
            ],
            AgentType::Firebender => vec![join_posix(home, ".firebender")],
            AgentType::GeminiCli => vec![join_posix(home, ".gemini")],
            AgentType::GithubCopilot => vec![join_posix(home, ".copilot")],
            AgentType::KimiCodeCli => {
                vec![join_posix(home, ".kimi-code"), join_posix(home, ".kimi")]
            }
            AgentType::Loaf => vec![join_posix(home, ".loaf")],
            AgentType::Opencode => vec![join_posix(config, "opencode")],
            AgentType::Promptscript => vec![
                join_posix(project_path, ".promptscript"),
                join_posix(project_path, "promptscript.yaml"),
            ],
            AgentType::Replit => vec![join_posix(project_path, ".replit")],
            AgentType::Warp => vec![join_posix(home, ".warp")],
            AgentType::Zed => vec![join_posix(config, "zed")],
            _ => self
                .global_skills_dir(agent)
                .and_then(|path| parent_posix(&path))
                .into_iter()
                .collect(),
        };
        paths
            .into_iter()
            .filter(|path| !path.trim().is_empty())
            .collect()
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

fn join_posix(base: &str, child: &str) -> String {
    format!(
        "{}/{}",
        base.trim_end_matches('/'),
        child.trim_matches(['/', '\\']).replace('\\', "/")
    )
}

fn path_to_posix(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn same_posix_path(left: &str, right: &str) -> bool {
    left.trim_end_matches('/') == right.trim_end_matches('/')
}

fn parent_posix(path: &str) -> Option<String> {
    path.trim_end_matches('/')
        .rsplit_once('/')
        .map(|(parent, _)| parent.to_string())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::{AgentEnvironmentContext, AgentEnvironmentResolver};
    use crate::core::agent_availability::AgentAvailabilityKind;
    use crate::core::agents::AgentType;

    fn linux_context() -> AgentEnvironmentContext {
        AgentEnvironmentContext {
            home: "/home/alice".to_string(),
            config_home: "/home/alice/.config".to_string(),
            env: BTreeMap::new(),
        }
    }

    #[test]
    fn maps_home_and_config_based_global_paths_into_linux_environment() {
        let resolver = AgentEnvironmentResolver::new(linux_context());

        assert_eq!(
            resolver.global_skills_dir(AgentType::AiderDesk).as_deref(),
            Some("/home/alice/.aider-desk/skills")
        );
        assert_eq!(
            resolver.global_skills_dir(AgentType::Amp).as_deref(),
            Some("/home/alice/.config/agents/skills")
        );
    }

    #[test]
    fn honors_environment_specific_codex_and_claude_homes() {
        let mut context = linux_context();
        context
            .env
            .insert("CODEX_HOME".to_string(), "/opt/codex-profile".to_string());
        context.env.insert(
            "CLAUDE_CONFIG_DIR".to_string(),
            "/opt/claude-profile".to_string(),
        );
        let resolver = AgentEnvironmentResolver::new(context);

        assert_eq!(
            resolver.global_skills_dir(AgentType::Codex).as_deref(),
            Some("/opt/codex-profile/skills")
        );
        assert_eq!(
            resolver.global_skills_dir(AgentType::ClaudeCode).as_deref(),
            Some("/opt/claude-profile/skills")
        );
    }

    #[test]
    fn resolves_project_path_without_using_host_home() {
        let resolver = AgentEnvironmentResolver::new(linux_context());

        assert_eq!(
            resolver.project_skills_dir(AgentType::Codex, "/work/app"),
            "/work/app/.agents/skills"
        );
    }

    #[test]
    fn resolves_environment_specific_agent_targets_without_host_detection() {
        let resolver = AgentEnvironmentResolver::new(linux_context());

        let codex = resolver.target(AgentType::Codex, false, "/work/app");
        assert_eq!(codex.shared_path, "/work/app/.agents/skills");
        assert_eq!(codex.private_path, None);
        assert_eq!(codex.availability, AgentAvailabilityKind::SharedOnly);
        assert!(codex.default_available);
        assert_eq!(codex.detection_paths[0], "/home/alice/.codex");
        assert!(codex.detection_paths.contains(&"/etc/codex".to_string()));

        let claude = resolver.target(AgentType::ClaudeCode, false, "/work/app");
        assert_eq!(
            claude.private_path.as_deref(),
            Some("/work/app/.claude/skills")
        );
        assert_eq!(claude.availability, AgentAvailabilityKind::PrivateRequired);
        assert!(!claude.default_available);

        let amp = resolver.target(AgentType::Amp, true, "/work/app");
        assert_eq!(
            amp.private_path.as_deref(),
            Some("/home/alice/.config/agents/skills")
        );
        assert_eq!(amp.detection_paths, vec!["/home/alice/.config/amp"]);

        let target = codex.scope_target(false);
        assert!(target.supported);
        assert!(target.automatic);
        assert_eq!(target.path, ".agents/skills");
        assert_eq!(target.install_path, "/work/app/.agents/skills");
        assert_eq!(target.read_paths, vec!["/work/app/.agents/skills"]);
    }
}

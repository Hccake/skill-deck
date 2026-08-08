use super::agent_definition::{
    AgentAdapter, AgentDefinition, AgentSource, DetectionSpec, LegacyMigrationTarget, LegacyPath,
    LegacyPathBehavior, LegacyPathScope, PathSpec, ScopeDefinition,
};
use super::agents::{AgentConfig, AgentType};
use super::paths::PATHS;
use std::path::Path;

pub fn builtin_agent_definitions() -> Vec<AgentDefinition> {
    AgentType::all().map(build_definition).collect()
}

fn build_definition(agent: AgentType) -> AgentDefinition {
    let config = agent.config();
    let (global, legacy_paths) = global_definition(agent, &config);
    let project = project_definition(&config);
    let adapter = if agent == AgentType::Eve {
        AgentAdapter::Eve
    } else {
        AgentAdapter::Standard
    };

    AgentDefinition {
        id: agent.to_string().parse().expect("built-in ID is valid"),
        display_name: config.display_name.to_string(),
        source: AgentSource::Builtin,
        aliases: Vec::new(),
        global,
        project,
        detection: detection_definition(agent, &config),
        legacy_paths,
        adapter,
    }
}

fn project_definition(config: &AgentConfig) -> ScopeDefinition {
    if config.skills_dir == ".agents/skills" {
        ScopeDefinition {
            enabled: true,
            reads_standard: true,
            private_path: None,
        }
    } else {
        ScopeDefinition {
            enabled: true,
            reads_standard: false,
            private_path: Some(PathSpec::project(config.skills_dir)),
        }
    }
}

fn global_definition(agent: AgentType, config: &AgentConfig) -> (ScopeDefinition, Vec<LegacyPath>) {
    if agent == AgentType::Cline {
        return (
            ScopeDefinition {
                enabled: true,
                reads_standard: true,
                private_path: None,
            },
            vec![LegacyPath {
                scope: LegacyPathScope::Global,
                path: PathSpec::home(".cline/skills"),
                behavior: LegacyPathBehavior::OfferMigration,
                migration_target: LegacyMigrationTarget::StandardCanonical,
            }],
        );
    }

    let Some(_) = config.global_skills_dir else {
        return (
            ScopeDefinition {
                enabled: false,
                reads_standard: false,
                private_path: None,
            },
            Vec::new(),
        );
    };

    let private_path = global_path(agent, config);
    let same_as_standard = private_path == PathSpec::home(".agents/skills");
    let reads_standard = official_standard_support(agent) || same_as_standard;

    (
        ScopeDefinition {
            enabled: true,
            reads_standard,
            private_path: (!same_as_standard).then_some(private_path),
        },
        Vec::new(),
    )
}

fn global_path(agent: AgentType, config: &AgentConfig) -> PathSpec {
    match agent {
        AgentType::Openclaw => PathSpec::FirstExisting {
            candidates: vec![
                PathSpec::home(".openclaw/skills"),
                PathSpec::home(".clawdbot/skills"),
                PathSpec::home(".moltbot/skills"),
            ],
            fallback: Box::new(PathSpec::home(".openclaw/skills")),
        },
        AgentType::Codex => environment_variable_path("CODEX_HOME", ".codex/skills"),
        AgentType::ClaudeCode => environment_variable_path("CLAUDE_CONFIG_DIR", ".claude/skills"),
        AgentType::MistralVibe => environment_variable_path("VIBE_HOME", ".vibe/skills"),
        AgentType::HermesAgent => environment_variable_path("HERMES_HOME", ".hermes/skills"),
        AgentType::AutohandCode => environment_variable_path("AUTOHAND_HOME", ".autohand/skills"),
        _ => path_from_native(config.global_skills_dir.as_ref().expect("global path")),
    }
}

fn environment_variable_path(name: &str, fallback: &str) -> PathSpec {
    let (relative_path, fallback_path) = match fallback.split_once('/') {
        Some((fallback_base, fallback_relative)) => (
            "skills".to_string(),
            format!("{fallback_base}/{fallback_relative}"),
        ),
        None => (String::new(), fallback.to_string()),
    };
    environment_variable_with_fallback(name, &relative_path, PathSpec::home(fallback_path))
}

fn environment_variable_with_fallback(
    name: &str,
    relative_path: &str,
    fallback: PathSpec,
) -> PathSpec {
    PathSpec::EnvironmentVariable {
        name: name.to_string(),
        relative_path: relative_path.to_string(),
        fallback: Box::new(fallback),
    }
}

fn path_from_native(path: &Path) -> PathSpec {
    if let Ok(relative) = path.strip_prefix(&PATHS.config_home) {
        return PathSpec::config_home(path_to_relative(relative));
    }
    if let Ok(relative) = path.strip_prefix(&PATHS.home) {
        return PathSpec::home(path_to_relative(relative));
    }
    panic!(
        "built-in path must be under home or config home: {}",
        path.display()
    );
}

fn path_to_relative(path: &Path) -> String {
    path.to_string_lossy()
        .replace('\\', "/")
        .trim_matches('/')
        .to_string()
}

fn detection_definition(agent: AgentType, config: &AgentConfig) -> DetectionSpec {
    if agent == AgentType::Eve {
        return DetectionSpec::AnyPathExists {
            paths: vec![
                PathSpec::project("agent"),
                PathSpec::project("package.json"),
            ],
        };
    }

    let paths = match agent {
        AgentType::Openclaw => vec![
            PathSpec::home(".openclaw"),
            PathSpec::home(".clawdbot"),
            PathSpec::home(".moltbot"),
        ],
        AgentType::Amp => vec![PathSpec::config_home("amp")],
        AgentType::Antigravity => vec![PathSpec::home(".gemini/antigravity")],
        AgentType::AntigravityCli => vec![PathSpec::home(".gemini/antigravity-cli")],
        AgentType::Astrbot => vec![PathSpec::project("data/skills"), PathSpec::home(".astrbot")],
        AgentType::Cline => vec![PathSpec::home(".cline")],
        AgentType::Codex => vec![
            environment_variable_path("CODEX_HOME", ".codex"),
            PathSpec::absolute("/etc/codex"),
        ],
        AgentType::Codebuddy => vec![
            PathSpec::project(".codebuddy"),
            PathSpec::home(".codebuddy"),
        ],
        AgentType::Continue => vec![PathSpec::project(".continue"), PathSpec::home(".continue")],
        AgentType::Cursor => vec![PathSpec::home(".cursor")],
        AgentType::Deepagents => vec![PathSpec::home(".deepagents")],
        AgentType::Dexto => vec![PathSpec::home(".dexto")],
        AgentType::Firebender => vec![PathSpec::home(".firebender")],
        AgentType::GeminiCli => vec![PathSpec::home(".gemini")],
        AgentType::GithubCopilot => vec![PathSpec::home(".copilot")],
        AgentType::KimiCodeCli => vec![PathSpec::home(".kimi-code"), PathSpec::home(".kimi")],
        AgentType::Loaf => vec![PathSpec::home(".loaf")],
        AgentType::Opencode => vec![PathSpec::config_home("opencode")],
        AgentType::Promptscript => vec![
            PathSpec::project(".promptscript"),
            PathSpec::project("promptscript.yaml"),
        ],
        AgentType::Replit => vec![PathSpec::project(".replit")],
        AgentType::Warp => vec![PathSpec::home(".warp")],
        AgentType::ClaudeCode => vec![environment_variable_path("CLAUDE_CONFIG_DIR", ".claude")],
        AgentType::MistralVibe => vec![environment_variable_path("VIBE_HOME", ".vibe")],
        AgentType::HermesAgent => vec![environment_variable_path("HERMES_HOME", ".hermes")],
        AgentType::AutohandCode => vec![environment_variable_path("AUTOHAND_HOME", ".autohand")],
        AgentType::Jazz => vec![PathSpec::project(".jazz"), PathSpec::home(".jazz")],
        AgentType::TabnineCli => vec![PathSpec::home(".tabnine")],
        AgentType::Zed => vec![
            PathSpec::config_home("zed"),
            environment_variable_with_fallback("APPDATA", "Zed", PathSpec::config_home("zed")),
            environment_variable_with_fallback(
                "FLATPAK_XDG_CONFIG_HOME",
                "zed",
                PathSpec::config_home("zed"),
            ),
        ],
        _ => vec![parent_path(&path_from_native(
            config.global_skills_dir.as_ref().expect("detection path"),
        ))],
    };

    DetectionSpec::AnyPathExists { paths }
}

fn parent_path(path: &PathSpec) -> PathSpec {
    match path {
        PathSpec::Home { relative_path } => PathSpec::home(parent_relative(relative_path)),
        PathSpec::ConfigHome { relative_path } => {
            PathSpec::config_home(parent_relative(relative_path))
        }
        PathSpec::Project { relative_path } => PathSpec::project(parent_relative(relative_path)),
        PathSpec::EnvironmentVariable {
            name,
            relative_path,
            fallback,
        } => PathSpec::EnvironmentVariable {
            name: name.clone(),
            relative_path: parent_relative(relative_path),
            fallback: Box::new(parent_path(fallback)),
        },
        PathSpec::FirstExisting {
            candidates,
            fallback,
        } => PathSpec::FirstExisting {
            candidates: candidates.iter().map(parent_path).collect(),
            fallback: Box::new(parent_path(fallback)),
        },
        PathSpec::Absolute { path } => PathSpec::absolute(path),
    }
}

fn parent_relative(path: &str) -> String {
    path.rsplit_once('/')
        .map(|(parent, _)| parent)
        .unwrap_or("")
        .to_string()
}

fn official_standard_support(agent: AgentType) -> bool {
    matches!(
        agent,
        AgentType::Codex
            | AgentType::GithubCopilot
            | AgentType::GeminiCli
            | AgentType::Opencode
            | AgentType::Warp
            | AgentType::Zed
            | AgentType::Firebender
            | AgentType::KimiCodeCli
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::agent_definition::{
        AgentAdapter, AgentId, AgentSource, DetectionSpec, LegacyMigrationTarget,
        LegacyPathBehavior, LegacyPathScope, PathSpec,
    };
    use crate::core::agents::AgentType;

    fn definition(agent: AgentType) -> AgentDefinition {
        builtin_agent_definitions()
            .into_iter()
            .find(|definition| definition.id.as_str() == agent.to_string())
            .unwrap_or_else(|| panic!("missing built-in definition for {agent}"))
    }

    #[test]
    fn every_agent_type_has_one_valid_builtin_definition() {
        let definitions = builtin_agent_definitions();
        let all_agents: Vec<_> = AgentType::all().collect();

        assert_eq!(definitions.len(), all_agents.len());
        for definition in &definitions {
            definition
                .validate()
                .expect("built-in definition must be valid");
            assert_eq!(definition.source, AgentSource::Builtin);
            assert!(definition.aliases.is_empty());
        }
        for agent in all_agents {
            let definition = definition(agent);
            let config = agent.config();
            assert_eq!(definition.id, AgentId::parse(agent.to_string()).unwrap());
            assert_eq!(definition.display_name, config.display_name);
        }
    }

    #[test]
    fn project_scope_matches_existing_shared_and_private_classification() {
        for agent in AgentType::all() {
            let definition = definition(agent);
            let config = agent.config();
            let project = &definition.project;

            assert!(project.enabled, "project support missing for {agent}");
            if config.skills_dir == ".agents/skills" {
                assert!(project.reads_standard);
                assert!(project.private_path.is_none());
            } else {
                assert!(!project.reads_standard);
                assert!(project.private_path.is_some());
            }
        }
    }

    #[test]
    fn every_agent_definition_uses_detection_paths() {
        for definition in builtin_agent_definitions() {
            let DetectionSpec::AnyPathExists { paths } = &definition.detection;
            assert!(
                !paths.is_empty(),
                "detection paths missing for {}",
                definition.id
            );
        }
    }

    #[test]
    fn global_scope_support_matches_existing_config_except_declared_cline_migration() {
        for agent in AgentType::all() {
            let definition = definition(agent);
            let config = agent.config();

            if agent == AgentType::Cline {
                assert!(definition.global.enabled);
                assert!(definition.global.reads_standard);
                assert!(definition.global.private_path.is_none());
                continue;
            }

            assert_eq!(
                definition.global.enabled,
                config.global_skills_dir.is_some()
            );
            if config.global_skills_dir.is_none() {
                assert!(!definition.global.reads_standard);
                assert!(definition.global.private_path.is_none());
            }
        }
    }

    #[test]
    fn openclaw_uses_stable_first_existing_fallback_order() {
        let definition = definition(AgentType::Openclaw);
        let Some(PathSpec::FirstExisting {
            candidates,
            fallback,
        }) = definition.global.private_path
        else {
            panic!("OpenClaw must use a first-existing global path");
        };

        assert_eq!(
            candidates,
            vec![
                PathSpec::home(".openclaw/skills"),
                PathSpec::home(".clawdbot/skills"),
                PathSpec::home(".moltbot/skills"),
            ]
        );
        assert_eq!(*fallback, PathSpec::home(".openclaw/skills"));
    }

    #[test]
    fn cline_uses_shared_active_path_and_declares_legacy_migration() {
        let definition = definition(AgentType::Cline);
        assert_eq!(definition.global.private_path, None);
        assert!(definition.global.reads_standard);

        assert_eq!(definition.legacy_paths.len(), 1);
        let legacy = &definition.legacy_paths[0];
        assert_eq!(legacy.scope, LegacyPathScope::Global);
        assert_eq!(legacy.path, PathSpec::home(".cline/skills"));
        assert_eq!(legacy.behavior, LegacyPathBehavior::OfferMigration);
        assert_eq!(
            legacy.migration_target,
            LegacyMigrationTarget::StandardCanonical
        );
    }

    #[test]
    fn eve_uses_detection_paths_with_the_builtin_adapter() {
        let definition = definition(AgentType::Eve);
        assert_eq!(definition.adapter, AgentAdapter::Eve);
        assert_eq!(
            definition.detection,
            DetectionSpec::AnyPathExists {
                paths: vec![
                    PathSpec::project("agent"),
                    PathSpec::project("package.json")
                ]
            }
        );
        assert!(!definition.global.enabled);
        assert!(definition.project.enabled);
    }

    #[test]
    fn builtins_preserve_exact_scope_paths_and_detection_candidates() {
        for agent in AgentType::all() {
            let definition = definition(agent);
            let config = agent.config();

            assert_eq!(definition.project, expected_project_scope(&config));
            assert_eq!(definition.global, expected_global_scope(agent, &config));
            assert_eq!(definition.detection, expected_detection(agent, &config));
        }
    }

    fn expected_project_scope(config: &AgentConfig) -> ScopeDefinition {
        if config.skills_dir == ".agents/skills" {
            ScopeDefinition {
                enabled: true,
                reads_standard: true,
                private_path: None,
            }
        } else {
            ScopeDefinition {
                enabled: true,
                reads_standard: false,
                private_path: Some(PathSpec::project(config.skills_dir)),
            }
        }
    }

    fn expected_global_scope(agent: AgentType, config: &AgentConfig) -> ScopeDefinition {
        if agent == AgentType::Cline {
            return ScopeDefinition {
                enabled: true,
                reads_standard: true,
                private_path: None,
            };
        }
        let Some(_) = config.global_skills_dir else {
            return ScopeDefinition {
                enabled: false,
                reads_standard: false,
                private_path: None,
            };
        };

        let path = expected_global_path(agent, config);
        let same_as_standard = path == PathSpec::home(".agents/skills");
        ScopeDefinition {
            enabled: true,
            reads_standard: same_as_standard || expected_global_standard_support(agent),
            private_path: (!same_as_standard).then_some(path),
        }
    }

    fn expected_global_standard_support(agent: AgentType) -> bool {
        matches!(
            agent,
            AgentType::Codex
                | AgentType::GithubCopilot
                | AgentType::GeminiCli
                | AgentType::Opencode
                | AgentType::Warp
                | AgentType::Zed
                | AgentType::Firebender
                | AgentType::KimiCodeCli
        )
    }

    fn expected_global_path(agent: AgentType, config: &AgentConfig) -> PathSpec {
        match agent {
            AgentType::Openclaw => PathSpec::FirstExisting {
                candidates: vec![
                    PathSpec::home(".openclaw/skills"),
                    PathSpec::home(".clawdbot/skills"),
                    PathSpec::home(".moltbot/skills"),
                ],
                fallback: Box::new(PathSpec::home(".openclaw/skills")),
            },
            AgentType::Codex => expected_environment_path("CODEX_HOME", "skills", ".codex/skills"),
            AgentType::ClaudeCode => {
                expected_environment_path("CLAUDE_CONFIG_DIR", "skills", ".claude/skills")
            }
            AgentType::MistralVibe => {
                expected_environment_path("VIBE_HOME", "skills", ".vibe/skills")
            }
            AgentType::HermesAgent => {
                expected_environment_path("HERMES_HOME", "skills", ".hermes/skills")
            }
            AgentType::AutohandCode => {
                expected_environment_path("AUTOHAND_HOME", "skills", ".autohand/skills")
            }
            _ => native_path_spec(config.global_skills_dir.as_ref().expect("global path")),
        }
    }

    fn expected_detection(agent: AgentType, config: &AgentConfig) -> DetectionSpec {
        if agent == AgentType::Eve {
            return DetectionSpec::AnyPathExists {
                paths: vec![
                    PathSpec::project("agent"),
                    PathSpec::project("package.json"),
                ],
            };
        }
        let paths = match agent {
            AgentType::AiderDesk => vec![PathSpec::home(".aider-desk")],
            AgentType::Amp => vec![PathSpec::config_home("amp")],
            AgentType::Antigravity => vec![PathSpec::home(".gemini/antigravity")],
            AgentType::AntigravityCli => vec![PathSpec::home(".gemini/antigravity-cli")],
            AgentType::Astrbot => {
                vec![PathSpec::project("data/skills"), PathSpec::home(".astrbot")]
            }
            AgentType::Augment => vec![PathSpec::home(".augment")],
            AgentType::AutohandCode => {
                vec![expected_environment_path("AUTOHAND_HOME", "", ".autohand")]
            }
            AgentType::Bob => vec![PathSpec::home(".bob")],
            AgentType::ClaudeCode => vec![expected_environment_path(
                "CLAUDE_CONFIG_DIR",
                "",
                ".claude",
            )],
            AgentType::Openclaw => vec![
                PathSpec::home(".openclaw"),
                PathSpec::home(".clawdbot"),
                PathSpec::home(".moltbot"),
            ],
            AgentType::Cline => vec![PathSpec::home(".cline")],
            AgentType::CodeartsAgent => vec![PathSpec::home(".codeartsdoer")],
            AgentType::Codebuddy => vec![
                PathSpec::project(".codebuddy"),
                PathSpec::home(".codebuddy"),
            ],
            AgentType::Codemaker => vec![PathSpec::home(".codemaker")],
            AgentType::Codestudio => vec![PathSpec::home(".codestudio")],
            AgentType::Codex => vec![
                expected_environment_path("CODEX_HOME", "", ".codex"),
                PathSpec::absolute("/etc/codex"),
            ],
            AgentType::CommandCode => vec![PathSpec::home(".commandcode")],
            AgentType::Continue => {
                vec![PathSpec::project(".continue"), PathSpec::home(".continue")]
            }
            AgentType::Crush => vec![PathSpec::config_home("crush")],
            AgentType::Cursor => vec![PathSpec::home(".cursor")],
            AgentType::Deepagents => vec![PathSpec::home(".deepagents")],
            AgentType::Devin => vec![PathSpec::config_home("devin")],
            AgentType::Dexto => vec![PathSpec::home(".dexto")],
            AgentType::Droid => vec![PathSpec::home(".factory")],
            AgentType::Firebender => vec![PathSpec::home(".firebender")],
            AgentType::Forgecode => vec![PathSpec::home(".forge")],
            AgentType::GeminiCli => vec![PathSpec::home(".gemini")],
            AgentType::GithubCopilot => vec![PathSpec::home(".copilot")],
            AgentType::Goose => vec![PathSpec::config_home("goose")],
            AgentType::HermesAgent => vec![expected_environment_path("HERMES_HOME", "", ".hermes")],
            AgentType::IflowCli => vec![PathSpec::home(".iflow")],
            AgentType::Junie => vec![PathSpec::home(".junie")],
            AgentType::Kilo => vec![PathSpec::home(".kilocode")],
            AgentType::KimiCodeCli => vec![PathSpec::home(".kimi-code"), PathSpec::home(".kimi")],
            AgentType::KiroCli => vec![PathSpec::home(".kiro")],
            AgentType::Kode => vec![PathSpec::home(".kode")],
            AgentType::InferenceSh => vec![PathSpec::home(".inferencesh")],
            AgentType::Jazz => vec![PathSpec::project(".jazz"), PathSpec::home(".jazz")],
            AgentType::Lingma => vec![PathSpec::home(".lingma")],
            AgentType::Loaf => vec![PathSpec::home(".loaf")],
            AgentType::Mcpjam => vec![PathSpec::home(".mcpjam")],
            AgentType::MistralVibe => vec![expected_environment_path("VIBE_HOME", "", ".vibe")],
            AgentType::Moxby => vec![PathSpec::home(".moxby")],
            AgentType::Mux => vec![PathSpec::home(".mux")],
            AgentType::Neovate => vec![PathSpec::home(".neovate")],
            AgentType::Ona => vec![PathSpec::home(".ona")],
            AgentType::Opencode => vec![PathSpec::config_home("opencode")],
            AgentType::Openhands => vec![PathSpec::home(".openhands")],
            AgentType::Pi => vec![PathSpec::home(".pi/agent")],
            AgentType::Promptscript => vec![
                PathSpec::project(".promptscript"),
                PathSpec::project("promptscript.yaml"),
            ],
            AgentType::Qoder => vec![PathSpec::home(".qoder")],
            AgentType::QoderCn => vec![PathSpec::home(".qoder-cn")],
            AgentType::QwenCode => vec![PathSpec::home(".qwen")],
            AgentType::Reasonix => vec![PathSpec::home(".reasonix")],
            AgentType::Replit => vec![PathSpec::project(".replit")],
            AgentType::Rovodev => vec![PathSpec::home(".rovodev")],
            AgentType::Roo => vec![PathSpec::home(".roo")],
            AgentType::TabnineCli => vec![PathSpec::home(".tabnine")],
            AgentType::Trae => vec![PathSpec::home(".trae")],
            AgentType::TraeCn => vec![PathSpec::home(".trae-cn")],
            AgentType::Warp => vec![PathSpec::home(".warp")],
            AgentType::Windsurf => vec![PathSpec::home(".codeium/windsurf")],
            AgentType::Zed => vec![
                PathSpec::config_home("zed"),
                PathSpec::EnvironmentVariable {
                    name: "APPDATA".to_string(),
                    relative_path: "Zed".to_string(),
                    fallback: Box::new(PathSpec::config_home("zed")),
                },
                PathSpec::EnvironmentVariable {
                    name: "FLATPAK_XDG_CONFIG_HOME".to_string(),
                    relative_path: "zed".to_string(),
                    fallback: Box::new(PathSpec::config_home("zed")),
                },
            ],
            AgentType::Zencoder => vec![PathSpec::home(".zencoder")],
            AgentType::Zenflow => vec![PathSpec::home(".zencoder")],
            AgentType::Pochi => vec![PathSpec::home(".pochi")],
            AgentType::Adal => vec![PathSpec::home(".adal")],
            AgentType::Cortex => vec![PathSpec::home(".snowflake/cortex")],
            AgentType::Terramind => vec![PathSpec::home(".terramind")],
            AgentType::Tinycloud => vec![PathSpec::home(".tinycloud")],
            _ => vec![parent_path(&expected_global_path(agent, config))],
        };
        DetectionSpec::AnyPathExists { paths }
    }

    fn expected_environment_path(name: &str, relative_path: &str, fallback: &str) -> PathSpec {
        PathSpec::EnvironmentVariable {
            name: name.to_string(),
            relative_path: relative_path.to_string(),
            fallback: Box::new(PathSpec::home(fallback)),
        }
    }

    fn native_path_spec(path: &std::path::Path) -> PathSpec {
        if let Ok(relative) = path.strip_prefix(&PATHS.config_home) {
            return PathSpec::config_home(relative.to_string_lossy().replace('\\', "/"));
        }
        PathSpec::home(
            path.strip_prefix(&PATHS.home)
                .expect("built-in path under home")
                .to_string_lossy()
                .replace('\\', "/"),
        )
    }

    fn parent_path(path: &PathSpec) -> PathSpec {
        match path {
            PathSpec::Home { relative_path } => PathSpec::home(parent_relative(relative_path)),
            PathSpec::ConfigHome { relative_path } => {
                PathSpec::config_home(parent_relative(relative_path))
            }
            PathSpec::Project { relative_path } => {
                PathSpec::project(parent_relative(relative_path))
            }
            PathSpec::EnvironmentVariable {
                name,
                relative_path,
                fallback,
            } => PathSpec::EnvironmentVariable {
                name: name.clone(),
                relative_path: parent_relative(relative_path),
                fallback: Box::new(parent_path(fallback)),
            },
            PathSpec::FirstExisting {
                candidates,
                fallback,
            } => PathSpec::FirstExisting {
                candidates: candidates.iter().map(parent_path).collect(),
                fallback: Box::new(parent_path(fallback)),
            },
            PathSpec::Absolute { path } => PathSpec::absolute(path),
        }
    }

    fn parent_relative(path: &str) -> String {
        path.rsplit_once('/')
            .map(|(parent, _)| parent)
            .unwrap_or("")
            .to_string()
    }
}

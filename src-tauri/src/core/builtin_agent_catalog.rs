use std::sync::LazyLock;

use super::agent_definition::{
    AgentAdapter, AgentDefinition, AgentId, AgentSource, DetectionSpec, LegacyMigrationTarget,
    LegacyPath, LegacyPathBehavior, LegacyPathScope, PathSpec, ScopeDefinition,
};

const EVE_AGENT_ID_VALUE: &str = "eve";

#[derive(Debug, Clone)]
struct BuiltinAgentSpec {
    id: &'static str,
    display_name: &'static str,
    global: ScopeDefinition,
    project: ScopeDefinition,
    detection_paths: Vec<PathSpec>,
    legacy_paths: Vec<LegacyPath>,
    adapter: AgentAdapter,
    cli_history_eligible: bool,
    cli_aliases: Vec<&'static str>,
    cli_project_discovery_dir: Option<CliProjectDiscoveryDir>,
}

#[derive(Debug, Clone, Copy)]
struct CliProjectDiscoveryDir {
    priority: u8,
    path: &'static str,
}

impl BuiltinAgentSpec {
    fn new(
        id: &'static str,
        display_name: &'static str,
        project_skills_dir: &'static str,
        global_skills_path: Option<PathSpec>,
    ) -> Self {
        let detection_paths = global_skills_path
            .as_ref()
            .map(parent_path)
            .into_iter()
            .collect();
        let project = if project_skills_dir == ".agents/skills" {
            standard_scope()
        } else {
            private_scope(PathSpec::project(project_skills_dir))
        };
        let global = match global_skills_path {
            Some(path) if path == PathSpec::home(".agents/skills") => standard_scope(),
            Some(path) => private_scope(path),
            None => disabled_scope(),
        };

        Self {
            id,
            display_name,
            global,
            project,
            detection_paths,
            legacy_paths: Vec::new(),
            adapter: AgentAdapter::Standard,
            cli_history_eligible: true,
            cli_aliases: Vec::new(),
            cli_project_discovery_dir: None,
        }
    }

    fn reads_standard_globally(mut self) -> Self {
        self.global.reads_standard = true;
        self
    }

    fn reads_standard_in_projects(mut self) -> Self {
        self.project.reads_standard = true;
        self
    }

    fn with_detection(mut self, paths: Vec<PathSpec>) -> Self {
        self.detection_paths = paths;
        self
    }

    fn with_legacy_path(mut self, legacy_path: LegacyPath) -> Self {
        self.legacy_paths.push(legacy_path);
        self
    }

    fn with_adapter(mut self, adapter: AgentAdapter) -> Self {
        self.adapter = adapter;
        self
    }

    fn without_cli_history(mut self) -> Self {
        self.cli_history_eligible = false;
        self
    }

    fn with_cli_alias(mut self, alias: &'static str) -> Self {
        self.cli_aliases.push(alias);
        self
    }

    fn with_cli_discovery_dir(mut self, priority: u8, path: &'static str) -> Self {
        self.cli_project_discovery_dir = Some(CliProjectDiscoveryDir { priority, path });
        self
    }

    fn definition(&self) -> AgentDefinition {
        AgentDefinition {
            id: AgentId::parse(self.id).expect("built-in Agent ID must be valid"),
            display_name: self.display_name.to_string(),
            source: AgentSource::Builtin,
            aliases: Vec::new(),
            global: self.global.clone(),
            project: self.project.clone(),
            detection: DetectionSpec::AnyPathExists {
                paths: self.detection_paths.clone(),
            },
            legacy_paths: self.legacy_paths.clone(),
            adapter: self.adapter,
        }
    }
}

static BUILTIN_AGENT_CATALOG: LazyLock<Vec<BuiltinAgentSpec>> = LazyLock::new(build_catalog);
static EVE_AGENT_ID: LazyLock<AgentId> = LazyLock::new(|| {
    AgentId::parse(EVE_AGENT_ID_VALUE).expect("built-in Eve Agent ID must be valid")
});
static CLI_PROJECT_DISCOVERY_DIRS: LazyLock<Vec<&'static str>> = LazyLock::new(|| {
    let mut directories = vec![CliProjectDiscoveryDir {
        priority: 0,
        path: ".agents/skills",
    }];
    directories.extend(
        BUILTIN_AGENT_CATALOG
            .iter()
            .filter_map(|spec| spec.cli_project_discovery_dir),
    );
    directories.sort_by_key(|directory| directory.priority);
    directories
        .into_iter()
        .map(|directory| directory.path)
        .collect()
});

pub fn builtin_agent_definitions() -> Vec<AgentDefinition> {
    BUILTIN_AGENT_CATALOG
        .iter()
        .map(BuiltinAgentSpec::definition)
        .collect()
}

pub fn eve_agent_id() -> AgentId {
    (*EVE_AGENT_ID).clone()
}

pub fn is_cli_history_agent(agent_id: &AgentId, source: AgentSource) -> bool {
    source == AgentSource::Builtin
        && BUILTIN_AGENT_CATALOG.iter().any(|spec| {
            spec.cli_history_eligible
                && (spec.id == agent_id.as_str()
                    || spec
                        .cli_aliases
                        .iter()
                        .any(|alias| *alias == agent_id.as_str()))
        })
}

pub fn cli_project_discovery_dirs() -> &'static [&'static str] {
    CLI_PROJECT_DISCOVERY_DIRS.as_slice()
}

fn build_catalog() -> Vec<BuiltinAgentSpec> {
    vec![
        agent(
            "aider-desk",
            "AiderDesk",
            ".aider-desk/skills",
            Some(home(".aider-desk/skills")),
        ),
        agent(
            "amp",
            "Amp",
            ".agents/skills",
            Some(config_home("agents/skills")),
        )
        .with_detection(vec![config_home("amp")]),
        agent(
            "antigravity",
            "Antigravity",
            ".agents/skills",
            Some(home(".gemini/antigravity/skills")),
        ),
        agent(
            "antigravity-cli",
            "Antigravity CLI",
            ".agents/skills",
            Some(home(".gemini/antigravity-cli/skills")),
        ),
        agent(
            "astrbot",
            "AstrBot",
            "data/skills",
            Some(home(".astrbot/data/skills")),
        )
        .with_detection(vec![project("data/skills"), home(".astrbot")]),
        agent(
            "augment",
            "Augment",
            ".augment/skills",
            Some(home(".augment/skills")),
        ),
        agent(
            "autohand-code",
            "Autohand Code CLI",
            ".autohand/skills",
            Some(environment_skills_path("AUTOHAND_HOME", ".autohand/skills")),
        ),
        agent("bob", "IBM Bob", ".bob/skills", Some(home(".bob/skills"))),
        agent(
            "claude-code",
            "Claude Code",
            ".claude/skills",
            Some(environment_skills_path(
                "CLAUDE_CONFIG_DIR",
                ".claude/skills",
            )),
        )
        .with_cli_discovery_dir(1, ".claude/skills"),
        agent(
            "openclaw",
            "OpenClaw",
            "skills",
            Some(PathSpec::FirstExisting {
                candidates: vec![
                    home(".openclaw/skills"),
                    home(".clawdbot/skills"),
                    home(".moltbot/skills"),
                ],
                fallback: Box::new(home(".openclaw/skills")),
            }),
        )
        .with_detection(vec![home(".openclaw"), home(".clawdbot"), home(".moltbot")]),
        agent(
            "cline",
            "Cline",
            ".agents/skills",
            Some(home(".agents/skills")),
        )
        .with_detection(vec![home(".cline")])
        .with_legacy_path(LegacyPath {
            scope: LegacyPathScope::Global,
            path: home(".cline/skills"),
            behavior: LegacyPathBehavior::OfferMigration,
            migration_target: LegacyMigrationTarget::StandardCanonical,
        })
        .with_cli_discovery_dir(2, ".cline/skills"),
        agent(
            "codearts-agent",
            "CodeArts Agent",
            ".codeartsdoer/skills",
            Some(home(".codeartsdoer/skills")),
        ),
        agent(
            "codebuddy",
            "CodeBuddy",
            ".codebuddy/skills",
            Some(home(".codebuddy/skills")),
        )
        .with_detection(vec![project(".codebuddy"), home(".codebuddy")])
        .with_cli_discovery_dir(3, ".codebuddy/skills"),
        agent(
            "codemaker",
            "Codemaker",
            ".codemaker/skills",
            Some(home(".codemaker/skills")),
        ),
        agent(
            "codestudio",
            "Code Studio",
            ".codestudio/skills",
            Some(home(".codestudio/skills")),
        ),
        agent(
            "codex",
            "Codex",
            ".agents/skills",
            Some(environment_skills_path("CODEX_HOME", ".codex/skills")),
        )
        .reads_standard_globally()
        .with_detection(vec![
            environment_path("CODEX_HOME", "", home(".codex")),
            PathSpec::absolute("/etc/codex"),
        ])
        .with_cli_discovery_dir(4, ".codex/skills"),
        agent(
            "command-code",
            "Command Code",
            ".commandcode/skills",
            Some(home(".commandcode/skills")),
        )
        .with_cli_discovery_dir(5, ".commandcode/skills"),
        agent(
            "continue",
            "Continue",
            ".continue/skills",
            Some(home(".continue/skills")),
        )
        .with_detection(vec![project(".continue"), home(".continue")])
        .with_cli_discovery_dir(6, ".continue/skills"),
        agent(
            "crush",
            "Crush",
            ".crush/skills",
            Some(config_home("crush/skills")),
        ),
        agent(
            "cursor",
            "Cursor",
            ".agents/skills",
            Some(home(".cursor/skills")),
        ),
        agent(
            "deepagents",
            "Deep Agents",
            ".agents/skills",
            Some(home(".deepagents/agent/skills")),
        )
        .with_detection(vec![home(".deepagents")]),
        agent(
            "devin",
            "Devin for Terminal",
            ".devin/skills",
            Some(config_home("devin/skills")),
        ),
        agent(
            "dexto",
            "Dexto",
            ".agents/skills",
            Some(home(".agents/skills")),
        )
        .with_detection(vec![home(".dexto")]),
        agent(
            "droid",
            "Droid",
            ".factory/skills",
            Some(home(".factory/skills")),
        ),
        agent(EVE_AGENT_ID_VALUE, "Eve", "agent/skills", None)
            .with_detection(vec![project("agent"), project("package.json")])
            .with_adapter(AgentAdapter::Eve)
            .without_cli_history(),
        agent(
            "firebender",
            "Firebender",
            ".agents/skills",
            Some(home(".firebender/skills")),
        )
        .reads_standard_globally(),
        agent(
            "forgecode",
            "ForgeCode",
            ".forge/skills",
            Some(home(".forge/skills")),
        ),
        agent(
            "gemini-cli",
            "Gemini CLI",
            ".agents/skills",
            Some(home(".gemini/skills")),
        )
        .reads_standard_globally(),
        agent(
            "github-copilot",
            "GitHub Copilot",
            ".agents/skills",
            Some(home(".copilot/skills")),
        )
        .reads_standard_globally()
        .with_cli_discovery_dir(7, ".github/skills"),
        agent(
            "goose",
            "Goose",
            ".goose/skills",
            Some(config_home("goose/skills")),
        )
        .with_cli_discovery_dir(8, ".goose/skills"),
        agent(
            "grok",
            "Grok Build",
            ".grok/skills",
            Some(environment_skills_path("GROK_HOME", ".grok/skills")),
        )
        .reads_standard_globally()
        .with_cli_discovery_dir(9, ".grok/skills"),
        agent(
            "hermes-agent",
            "Hermes Agent",
            ".hermes/skills",
            Some(environment_skills_path("HERMES_HOME", ".hermes/skills")),
        ),
        agent(
            "iflow-cli",
            "iFlow CLI",
            ".iflow/skills",
            Some(home(".iflow/skills")),
        )
        .with_cli_discovery_dir(10, ".iflow/skills"),
        agent(
            "junie",
            "Junie",
            ".junie/skills",
            Some(home(".junie/skills")),
        )
        .with_cli_discovery_dir(11, ".junie/skills"),
        agent(
            "kilo",
            "Kilo Code",
            ".kilocode/skills",
            Some(home(".kilocode/skills")),
        )
        .with_cli_discovery_dir(13, ".kilocode/skills"),
        agent(
            "kimchi",
            "Kimchi",
            ".kimchi/skills",
            Some(home(".config/kimchi/harness/skills")),
        )
        .reads_standard_globally()
        .reads_standard_in_projects()
        .with_detection(vec![home(".config/kimchi")])
        .with_cli_discovery_dir(12, ".kimchi/skills"),
        agent(
            "kimi-code-cli",
            "Kimi Code CLI",
            ".agents/skills",
            Some(home(".agents/skills")),
        )
        .with_detection(vec![home(".kimi-code"), home(".kimi")])
        .with_cli_alias("kimi-cli"),
        agent(
            "kiro-cli",
            "Kiro CLI",
            ".kiro/skills",
            Some(home(".kiro/skills")),
        )
        .with_cli_discovery_dir(14, ".kiro/skills"),
        agent("kode", "Kode", ".kode/skills", Some(home(".kode/skills"))),
        agent(
            "inference-sh",
            "inference.sh",
            ".inferencesh/skills",
            Some(home(".inferencesh/skills")),
        ),
        agent("jazz", "Jazz", ".jazz/skills", Some(home(".jazz/skills")))
            .with_detection(vec![project(".jazz"), home(".jazz")]),
        agent(
            "lingma",
            "Lingma",
            ".lingma/skills",
            Some(home(".lingma/skills")),
        ),
        agent(
            "loaf",
            "Loaf",
            ".agents/skills",
            Some(home(".agents/skills")),
        )
        .with_detection(vec![home(".loaf")]),
        agent(
            "mcpjam",
            "MCPJam",
            ".mcpjam/skills",
            Some(home(".mcpjam/skills")),
        ),
        agent(
            "minimax-code",
            "MiniMax Code",
            ".minimax/skills",
            Some(home(".minimax/skills")),
        )
        .with_detection(home_with_macos_application_detection(
            ".minimax",
            "/Applications/MiniMax Code.app",
        ))
        .with_cli_discovery_dir(15, ".minimax/skills"),
        agent(
            "mistral-vibe",
            "Mistral Vibe",
            ".vibe/skills",
            Some(environment_skills_path("VIBE_HOME", ".vibe/skills")),
        ),
        agent(
            "moxby",
            "Moxby",
            ".moxby/skills",
            Some(home(".moxby/skills")),
        ),
        agent("mux", "Mux", ".mux/skills", Some(home(".mux/skills")))
            .with_cli_discovery_dir(16, ".mux/skills"),
        agent(
            "neovate",
            "Neovate",
            ".neovate/skills",
            Some(home(".neovate/skills")),
        )
        .with_cli_discovery_dir(17, ".neovate/skills"),
        agent("ona", "Ona", ".ona/skills", Some(home(".ona/skills"))),
        agent(
            "opencode",
            "OpenCode",
            ".agents/skills",
            Some(config_home("opencode/skills")),
        )
        .reads_standard_globally()
        .with_detection(vec![config_home("opencode")])
        .with_cli_discovery_dir(18, ".opencode/skills"),
        agent(
            "openhands",
            "OpenHands",
            ".openhands/skills",
            Some(home(".openhands/skills")),
        )
        .with_cli_discovery_dir(19, ".openhands/skills"),
        agent("pi", "Pi", ".pi/skills", Some(home(".pi/agent/skills")))
            .with_cli_discovery_dir(20, ".pi/skills"),
        agent(
            "posit-assistant",
            "Posit Assistant",
            ".posit/assistant/skills",
            Some(home(".posit/assistant/skills")),
        )
        .reads_standard_globally()
        .reads_standard_in_projects()
        .with_detection(vec![home(".posit/assistant"), home(".positai")])
        .with_cli_discovery_dir(21, ".posit/assistant/skills"),
        agent("promptscript", "PromptScript", ".agents/skills", None)
            .with_detection(vec![project(".promptscript"), project("promptscript.yaml")]),
        agent(
            "qoder",
            "Qoder",
            ".qoder/skills",
            Some(home(".qoder/skills")),
        )
        .with_cli_discovery_dir(22, ".qoder/skills"),
        agent(
            "qoder-cn",
            "Qoder CN",
            ".qoder/skills",
            Some(home(".qoder-cn/skills")),
        ),
        agent(
            "qwen-code",
            "Qwen Code",
            ".qwen/skills",
            Some(home(".qwen/skills")),
        ),
        agent(
            "reasonix",
            "Reasonix",
            ".reasonix/skills",
            Some(home(".reasonix/skills")),
        ),
        agent(
            "replit",
            "Replit",
            ".agents/skills",
            Some(config_home("agents/skills")),
        )
        .with_detection(vec![project(".replit")]),
        agent(
            "rovodev",
            "Rovo Dev",
            ".rovodev/skills",
            Some(home(".rovodev/skills")),
        ),
        agent("roo", "Roo Code", ".roo/skills", Some(home(".roo/skills")))
            .with_cli_discovery_dir(23, ".roo/skills"),
        agent(
            "tabnine-cli",
            "Tabnine CLI",
            ".tabnine/agent/skills",
            Some(home(".tabnine/agent/skills")),
        )
        .with_detection(vec![home(".tabnine")]),
        agent("trae", "Trae", ".trae/skills", Some(home(".trae/skills")))
            .with_cli_discovery_dir(24, ".trae/skills"),
        agent(
            "trae-cn",
            "Trae CN",
            ".trae/skills",
            Some(home(".trae-cn/skills")),
        ),
        agent(
            "warp",
            "Warp",
            ".agents/skills",
            Some(home(".agents/skills")),
        )
        .with_detection(vec![home(".warp")]),
        agent(
            "windsurf",
            "Windsurf",
            ".windsurf/skills",
            Some(home(".codeium/windsurf/skills")),
        )
        .with_cli_discovery_dir(25, ".windsurf/skills"),
        agent("zed", "Zed", ".agents/skills", Some(home(".agents/skills"))).with_detection(vec![
            config_home("zed"),
            environment_path("APPDATA", "Zed", config_home("zed")),
            environment_path("FLATPAK_XDG_CONFIG_HOME", "zed", config_home("zed")),
        ]),
        agent(
            "zcode",
            "ZCode",
            ".zcode/skills",
            Some(home(".zcode/skills")),
        )
        .with_detection(home_with_macos_application_detection(
            ".zcode",
            "/Applications/ZCode.app",
        ))
        .with_cli_discovery_dir(26, ".zcode/skills"),
        agent(
            "zencoder",
            "Zencoder",
            ".zencoder/skills",
            Some(home(".zencoder/skills")),
        )
        .with_cli_discovery_dir(27, ".zencoder/skills"),
        agent(
            "zenflow",
            "Zenflow",
            ".zencoder/skills",
            Some(home(".zencoder/skills")),
        ),
        agent(
            "pochi",
            "Pochi",
            ".pochi/skills",
            Some(home(".pochi/skills")),
        ),
        agent("adal", "AdaL", ".adal/skills", Some(home(".adal/skills"))),
        agent(
            "cortex",
            "Cortex Code",
            ".cortex/skills",
            Some(home(".snowflake/cortex/skills")),
        ),
        agent(
            "terramind",
            "Terramind",
            ".terramind/skills",
            Some(home(".terramind/skills")),
        ),
        agent(
            "tinycloud",
            "Tinycloud",
            ".tinycloud/skills",
            Some(home(".tinycloud/skills")),
        ),
    ]
}

fn agent(
    id: &'static str,
    display_name: &'static str,
    project_skills_dir: &'static str,
    global_skills_path: Option<PathSpec>,
) -> BuiltinAgentSpec {
    BuiltinAgentSpec::new(id, display_name, project_skills_dir, global_skills_path)
}

fn standard_scope() -> ScopeDefinition {
    ScopeDefinition {
        enabled: true,
        reads_standard: true,
        private_path: None,
    }
}

fn private_scope(path: PathSpec) -> ScopeDefinition {
    ScopeDefinition {
        enabled: true,
        reads_standard: false,
        private_path: Some(path),
    }
}

fn disabled_scope() -> ScopeDefinition {
    ScopeDefinition {
        enabled: false,
        reads_standard: false,
        private_path: None,
    }
}

fn home(relative_path: impl Into<String>) -> PathSpec {
    PathSpec::home(relative_path)
}

fn config_home(relative_path: impl Into<String>) -> PathSpec {
    PathSpec::config_home(relative_path)
}

fn project(relative_path: impl Into<String>) -> PathSpec {
    PathSpec::project(relative_path)
}

fn environment_skills_path(name: &str, fallback: &str) -> PathSpec {
    environment_path(name, "skills", home(fallback))
}

fn environment_path(name: &str, relative_path: &str, fallback: PathSpec) -> PathSpec {
    PathSpec::EnvironmentVariable {
        name: name.to_string(),
        relative_path: relative_path.to_string(),
        fallback: Box::new(fallback),
    }
}

fn home_with_macos_application_detection(home_path: &str, application_path: &str) -> Vec<PathSpec> {
    let paths = vec![home(home_path)];
    #[cfg(target_os = "macos")]
    let paths = {
        let mut paths = paths;
        paths.push(PathSpec::absolute(application_path));
        paths
    };
    #[cfg(not(target_os = "macos"))]
    let _ = application_path;
    paths
}

fn parent_path(path: &PathSpec) -> PathSpec {
    match path {
        PathSpec::Home { relative_path } => home(parent_relative(relative_path)),
        PathSpec::ConfigHome { relative_path } => config_home(parent_relative(relative_path)),
        PathSpec::Project { relative_path } => project(parent_relative(relative_path)),
        PathSpec::EnvironmentVariable {
            name,
            relative_path,
            fallback,
        } => environment_path(name, parent_relative(relative_path), parent_path(fallback)),
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

fn parent_relative(path: &str) -> &str {
    path.rsplit_once('/')
        .map(|(parent, _)| parent)
        .unwrap_or("")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    fn definition(id: &str) -> AgentDefinition {
        builtin_agent_definitions()
            .into_iter()
            .find(|definition| definition.id.as_str() == id)
            .unwrap_or_else(|| panic!("missing built-in definition for {id}"))
    }

    #[test]
    fn catalog_exposes_one_valid_definition_per_builtin_agent() {
        let definitions = builtin_agent_definitions();
        let ids = definitions
            .iter()
            .map(|definition| definition.id.clone())
            .collect::<BTreeSet<_>>();

        assert_eq!(definitions.len(), 76);
        assert_eq!(ids.len(), definitions.len());
        for definition in definitions {
            definition
                .validate()
                .expect("built-in definition must be valid");
            assert_eq!(definition.source, AgentSource::Builtin);
            assert!(definition.aliases.is_empty());
        }
    }

    #[test]
    fn catalog_projects_cli_history_eligibility_from_builtin_metadata() {
        assert!(is_cli_history_agent(
            &AgentId::parse("posit-assistant").unwrap(),
            AgentSource::Builtin,
        ));
        assert!(is_cli_history_agent(
            &AgentId::parse("kimi-cli").unwrap(),
            AgentSource::Builtin,
        ));
        assert!(!is_cli_history_agent(&eve_agent_id(), AgentSource::Builtin));
        assert!(!is_cli_history_agent(
            &AgentId::parse("posit-assistant").unwrap(),
            AgentSource::Custom,
        ));
        assert!(!is_cli_history_agent(
            &AgentId::parse("my-custom-agent").unwrap(),
            AgentSource::Builtin,
        ));
    }

    #[test]
    fn catalog_projects_only_cli_priority_discovery_directories() {
        let directories = cli_project_discovery_dirs();
        assert_eq!(
            directories,
            [
                ".agents/skills",
                ".claude/skills",
                ".cline/skills",
                ".codebuddy/skills",
                ".codex/skills",
                ".commandcode/skills",
                ".continue/skills",
                ".github/skills",
                ".goose/skills",
                ".grok/skills",
                ".iflow/skills",
                ".junie/skills",
                ".kimchi/skills",
                ".kilocode/skills",
                ".kiro/skills",
                ".minimax/skills",
                ".mux/skills",
                ".neovate/skills",
                ".opencode/skills",
                ".openhands/skills",
                ".pi/skills",
                ".posit/assistant/skills",
                ".qoder/skills",
                ".roo/skills",
                ".trae/skills",
                ".windsurf/skills",
                ".zcode/skills",
                ".zencoder/skills",
            ]
        );
    }

    #[test]
    fn posit_assistant_declares_both_scope_support_and_detection_alias() {
        let posit = definition("posit-assistant");
        assert_eq!(
            posit.global,
            ScopeDefinition {
                enabled: true,
                reads_standard: true,
                private_path: Some(home(".posit/assistant/skills")),
            }
        );
        assert_eq!(
            posit.project,
            ScopeDefinition {
                enabled: true,
                reads_standard: true,
                private_path: Some(project(".posit/assistant/skills")),
            }
        );
        assert_eq!(
            posit.detection,
            DetectionSpec::AnyPathExists {
                paths: vec![home(".posit/assistant"), home(".positai")],
            }
        );
    }

    #[test]
    fn cline_declares_standard_scope_with_legacy_migration() {
        let cline = definition("cline");
        assert_eq!(cline.global, standard_scope());
        assert_eq!(
            cline.legacy_paths,
            vec![LegacyPath {
                scope: LegacyPathScope::Global,
                path: home(".cline/skills"),
                behavior: LegacyPathBehavior::OfferMigration,
                migration_target: LegacyMigrationTarget::StandardCanonical,
            }]
        );
    }

    #[test]
    fn openclaw_preserves_first_existing_global_path_order() {
        let openclaw = definition("openclaw");
        let Some(PathSpec::FirstExisting {
            candidates,
            fallback,
        }) = openclaw.global.private_path
        else {
            panic!("OpenClaw must use first-existing path resolution");
        };
        assert_eq!(
            candidates,
            vec![
                home(".openclaw/skills"),
                home(".clawdbot/skills"),
                home(".moltbot/skills")
            ]
        );
        assert_eq!(*fallback, home(".openclaw/skills"));
    }

    #[test]
    fn environment_based_agents_preserve_runtime_path_specs() {
        let codex = definition("codex");
        assert_eq!(
            codex.global.private_path,
            Some(environment_path(
                "CODEX_HOME",
                "skills",
                home(".codex/skills")
            ))
        );
        assert!(codex.global.reads_standard);
        let DetectionSpec::AnyPathExists { paths } = codex.detection;
        assert_eq!(
            paths,
            vec![
                environment_path("CODEX_HOME", "", home(".codex")),
                PathSpec::absolute("/etc/codex")
            ]
        );
    }

    #[test]
    fn eve_uses_project_detection_and_special_adapter() {
        let eve = definition(EVE_AGENT_ID_VALUE);
        assert_eq!(eve.adapter, AgentAdapter::Eve);
        assert!(!eve.global.enabled);
        assert_eq!(
            eve.detection,
            DetectionSpec::AnyPathExists {
                paths: vec![project("agent"), project("package.json")],
            }
        );
    }

    #[test]
    fn macos_application_detection_remains_platform_specific() {
        for (id, home_path, application_path) in [
            ("minimax-code", ".minimax", "/Applications/MiniMax Code.app"),
            ("zcode", ".zcode", "/Applications/ZCode.app"),
        ] {
            let DetectionSpec::AnyPathExists { paths } = definition(id).detection;
            let mut expected = vec![home(home_path)];
            if cfg!(target_os = "macos") {
                expected.push(PathSpec::absolute(application_path));
            }
            assert_eq!(paths, expected);
        }
    }
}

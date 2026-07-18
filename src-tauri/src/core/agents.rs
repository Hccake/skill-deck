// Agent 配置与检测
// 完整对应 CLI: agents.ts

use crate::core::paths::PATHS;
use serde::{Deserialize, Serialize};
use specta::Type;
use std::path::PathBuf;

/// Agent 配置
/// 对应 CLI: AgentConfig (types.ts:51-60)
#[derive(Debug, Clone)]
pub struct AgentConfig {
    #[allow(dead_code)]
    pub name: &'static str,
    pub display_name: &'static str,
    pub skills_dir: &'static str,
    pub global_skills_dir: Option<PathBuf>,
}

/// Agent 类型枚举
/// 完整对应 CLI: types.ts AgentType
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "kebab-case")]
#[specta(rename_all = "kebab-case")]
pub enum AgentType {
    AiderDesk,
    Amp,
    Antigravity,
    AntigravityCli,
    Astrbot,
    Augment,
    AutohandCode,
    Bob,
    ClaudeCode,
    Openclaw,
    Cline,
    CodeartsAgent,
    Codebuddy,
    Codemaker,
    Codestudio,
    Codex,
    CommandCode,
    Continue,
    Crush,
    Cursor,
    Deepagents,
    Devin,
    Dexto,
    Droid,
    Eve,
    Firebender,
    Forgecode,
    GeminiCli,
    GithubCopilot,
    Goose,
    HermesAgent,
    IflowCli,
    Junie,
    Kilo,
    KimiCodeCli,
    KiroCli,
    Kode,
    InferenceSh,
    Jazz,
    Lingma,
    Loaf,
    Mcpjam,
    MistralVibe,
    Moxby,
    Mux,
    Neovate,
    Ona,
    Opencode,
    Openhands,
    Pi,
    Promptscript,
    Qoder,
    QoderCn,
    QwenCode,
    Reasonix,
    Replit,
    Rovodev,
    Roo,
    TabnineCli,
    Trae,
    TraeCn,
    Warp,
    Windsurf,
    Zed,
    Zencoder,
    Zenflow,
    Pochi,
    Adal,
    Cortex,
    Terramind,
    Tinycloud,
}

impl std::fmt::Display for AgentType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = match self {
            Self::AiderDesk => "aider-desk",
            Self::Amp => "amp",
            Self::Antigravity => "antigravity",
            Self::AntigravityCli => "antigravity-cli",
            Self::Astrbot => "astrbot",
            Self::Augment => "augment",
            Self::AutohandCode => "autohand-code",
            Self::Bob => "bob",
            Self::ClaudeCode => "claude-code",
            Self::Openclaw => "openclaw",
            Self::Cline => "cline",
            Self::CodeartsAgent => "codearts-agent",
            Self::Codebuddy => "codebuddy",
            Self::Codemaker => "codemaker",
            Self::Codestudio => "codestudio",
            Self::Codex => "codex",
            Self::CommandCode => "command-code",
            Self::Continue => "continue",
            Self::Crush => "crush",
            Self::Cursor => "cursor",
            Self::Deepagents => "deepagents",
            Self::Devin => "devin",
            Self::Dexto => "dexto",
            Self::Droid => "droid",
            Self::Eve => "eve",
            Self::Firebender => "firebender",
            Self::Forgecode => "forgecode",
            Self::GeminiCli => "gemini-cli",
            Self::GithubCopilot => "github-copilot",
            Self::Goose => "goose",
            Self::HermesAgent => "hermes-agent",
            Self::IflowCli => "iflow-cli",
            Self::Junie => "junie",
            Self::Kilo => "kilo",
            Self::KimiCodeCli => "kimi-code-cli",
            Self::KiroCli => "kiro-cli",
            Self::Kode => "kode",
            Self::InferenceSh => "inference-sh",
            Self::Jazz => "jazz",
            Self::Lingma => "lingma",
            Self::Loaf => "loaf",
            Self::Mcpjam => "mcpjam",
            Self::MistralVibe => "mistral-vibe",
            Self::Moxby => "moxby",
            Self::Mux => "mux",
            Self::Neovate => "neovate",
            Self::Ona => "ona",
            Self::Opencode => "opencode",
            Self::Openhands => "openhands",
            Self::Pi => "pi",
            Self::Promptscript => "promptscript",
            Self::Qoder => "qoder",
            Self::QoderCn => "qoder-cn",
            Self::QwenCode => "qwen-code",
            Self::Reasonix => "reasonix",
            Self::Replit => "replit",
            Self::Rovodev => "rovodev",
            Self::Roo => "roo",
            Self::TabnineCli => "tabnine-cli",
            Self::Trae => "trae",
            Self::TraeCn => "trae-cn",
            Self::Warp => "warp",
            Self::Windsurf => "windsurf",
            Self::Zed => "zed",
            Self::Zencoder => "zencoder",
            Self::Zenflow => "zenflow",
            Self::Pochi => "pochi",
            Self::Adal => "adal",
            Self::Cortex => "cortex",
            Self::Terramind => "terramind",
            Self::Tinycloud => "tinycloud",
        };
        write!(f, "{}", name)
    }
}

impl std::str::FromStr for AgentType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "aider-desk" => Ok(Self::AiderDesk),
            "amp" => Ok(Self::Amp),
            "antigravity" => Ok(Self::Antigravity),
            "antigravity-cli" => Ok(Self::AntigravityCli),
            "astrbot" => Ok(Self::Astrbot),
            "augment" => Ok(Self::Augment),
            "autohand-code" => Ok(Self::AutohandCode),
            "bob" => Ok(Self::Bob),
            "claude-code" => Ok(Self::ClaudeCode),
            "openclaw" => Ok(Self::Openclaw),
            "cline" => Ok(Self::Cline),
            "codearts-agent" => Ok(Self::CodeartsAgent),
            "codebuddy" => Ok(Self::Codebuddy),
            "codemaker" => Ok(Self::Codemaker),
            "codestudio" => Ok(Self::Codestudio),
            "codex" => Ok(Self::Codex),
            "command-code" => Ok(Self::CommandCode),
            "continue" => Ok(Self::Continue),
            "crush" => Ok(Self::Crush),
            "cursor" => Ok(Self::Cursor),
            "deepagents" => Ok(Self::Deepagents),
            "devin" => Ok(Self::Devin),
            "dexto" => Ok(Self::Dexto),
            "droid" => Ok(Self::Droid),
            "eve" => Ok(Self::Eve),
            "firebender" => Ok(Self::Firebender),
            "forgecode" => Ok(Self::Forgecode),
            "gemini-cli" => Ok(Self::GeminiCli),
            "github-copilot" => Ok(Self::GithubCopilot),
            "goose" => Ok(Self::Goose),
            "hermes-agent" => Ok(Self::HermesAgent),
            "iflow-cli" => Ok(Self::IflowCli),
            "junie" => Ok(Self::Junie),
            "kilo" => Ok(Self::Kilo),
            "kimi-code-cli" | "kimi-cli" => Ok(Self::KimiCodeCli),
            "kiro-cli" => Ok(Self::KiroCli),
            "kode" => Ok(Self::Kode),
            "inference-sh" => Ok(Self::InferenceSh),
            "jazz" => Ok(Self::Jazz),
            "lingma" => Ok(Self::Lingma),
            "loaf" => Ok(Self::Loaf),
            "mcpjam" => Ok(Self::Mcpjam),
            "mistral-vibe" => Ok(Self::MistralVibe),
            "moxby" => Ok(Self::Moxby),
            "mux" => Ok(Self::Mux),
            "neovate" => Ok(Self::Neovate),
            "ona" => Ok(Self::Ona),
            "opencode" => Ok(Self::Opencode),
            "openhands" => Ok(Self::Openhands),
            "pi" => Ok(Self::Pi),
            "promptscript" => Ok(Self::Promptscript),
            "qoder" => Ok(Self::Qoder),
            "qoder-cn" => Ok(Self::QoderCn),
            "qwen-code" => Ok(Self::QwenCode),
            "reasonix" => Ok(Self::Reasonix),
            "replit" => Ok(Self::Replit),
            "rovodev" => Ok(Self::Rovodev),
            "roo" => Ok(Self::Roo),
            "tabnine-cli" => Ok(Self::TabnineCli),
            "trae" => Ok(Self::Trae),
            "trae-cn" => Ok(Self::TraeCn),
            "warp" => Ok(Self::Warp),
            "windsurf" => Ok(Self::Windsurf),
            "zed" => Ok(Self::Zed),
            "zencoder" => Ok(Self::Zencoder),
            "zenflow" => Ok(Self::Zenflow),
            "pochi" => Ok(Self::Pochi),
            "adal" => Ok(Self::Adal),
            "cortex" => Ok(Self::Cortex),
            "terramind" => Ok(Self::Terramind),
            "tinycloud" => Ok(Self::Tinycloud),
            _ => Err(format!("Unknown agent type: {}", s)),
        }
    }
}

impl AgentType {
    /// 返回所有 Agent 类型的迭代器
    pub fn all() -> impl Iterator<Item = AgentType> {
        [
            Self::AiderDesk,
            Self::Amp,
            Self::Antigravity,
            Self::AntigravityCli,
            Self::Astrbot,
            Self::Augment,
            Self::AutohandCode,
            Self::Bob,
            Self::ClaudeCode,
            Self::Openclaw,
            Self::Cline,
            Self::CodeartsAgent,
            Self::Codebuddy,
            Self::Codemaker,
            Self::Codestudio,
            Self::Codex,
            Self::CommandCode,
            Self::Continue,
            Self::Crush,
            Self::Cursor,
            Self::Deepagents,
            Self::Devin,
            Self::Dexto,
            Self::Droid,
            Self::Eve,
            Self::Firebender,
            Self::Forgecode,
            Self::GeminiCli,
            Self::GithubCopilot,
            Self::Goose,
            Self::HermesAgent,
            Self::IflowCli,
            Self::Junie,
            Self::Kilo,
            Self::KimiCodeCli,
            Self::KiroCli,
            Self::Kode,
            Self::InferenceSh,
            Self::Jazz,
            Self::Lingma,
            Self::Loaf,
            Self::Mcpjam,
            Self::MistralVibe,
            Self::Moxby,
            Self::Mux,
            Self::Neovate,
            Self::Ona,
            Self::Opencode,
            Self::Openhands,
            Self::Pi,
            Self::Promptscript,
            Self::Qoder,
            Self::QoderCn,
            Self::QwenCode,
            Self::Reasonix,
            Self::Replit,
            Self::Rovodev,
            Self::Roo,
            Self::TabnineCli,
            Self::Trae,
            Self::TraeCn,
            Self::Warp,
            Self::Windsurf,
            Self::Zed,
            Self::Zencoder,
            Self::Zenflow,
            Self::Pochi,
            Self::Adal,
            Self::Cortex,
            Self::Terramind,
            Self::Tinycloud,
        ]
        .into_iter()
    }

    /// 获取 Agent 配置
    /// 完整对应 CLI: agents.ts 中每个 agent 的配置
    pub fn config(&self) -> AgentConfig {
        match self {
            Self::AiderDesk => AgentConfig {
                name: "aider-desk",
                display_name: "AiderDesk",
                skills_dir: ".aider-desk/skills",
                global_skills_dir: Some(PATHS.home.join(".aider-desk").join("skills")),
            },
            Self::Amp => AgentConfig {
                name: "amp",
                display_name: "Amp",
                skills_dir: ".agents/skills",
                global_skills_dir: Some(PATHS.config_home.join("agents").join("skills")),
            },
            Self::Antigravity => AgentConfig {
                name: "antigravity",
                display_name: "Antigravity",
                skills_dir: ".agents/skills",
                global_skills_dir: Some(
                    PATHS
                        .home
                        .join(".gemini")
                        .join("antigravity")
                        .join("skills"),
                ),
            },
            Self::AntigravityCli => AgentConfig {
                name: "antigravity-cli",
                display_name: "Antigravity CLI",
                skills_dir: ".agents/skills",
                global_skills_dir: Some(
                    PATHS
                        .home
                        .join(".gemini")
                        .join("antigravity-cli")
                        .join("skills"),
                ),
            },
            Self::Astrbot => AgentConfig {
                name: "astrbot",
                display_name: "AstrBot",
                skills_dir: "data/skills",
                global_skills_dir: Some(PATHS.home.join(".astrbot").join("data").join("skills")),
            },
            Self::Augment => AgentConfig {
                name: "augment",
                display_name: "Augment",
                skills_dir: ".augment/skills",
                global_skills_dir: Some(PATHS.home.join(".augment").join("skills")),
            },
            Self::AutohandCode => AgentConfig {
                name: "autohand-code",
                display_name: "Autohand Code CLI",
                skills_dir: ".autohand/skills",
                global_skills_dir: Some(Self::autohand_home().join("skills")),
            },
            Self::Bob => AgentConfig {
                name: "bob",
                display_name: "IBM Bob",
                skills_dir: ".bob/skills",
                global_skills_dir: Some(PATHS.home.join(".bob").join("skills")),
            },
            Self::ClaudeCode => AgentConfig {
                name: "claude-code",
                display_name: "Claude Code",
                skills_dir: ".claude/skills",
                global_skills_dir: Some(PATHS.claude_home.join("skills")),
            },
            Self::Openclaw => AgentConfig {
                name: "openclaw",
                display_name: "OpenClaw",
                skills_dir: "skills",
                global_skills_dir: Some(Self::openclaw_global_dir()),
            },
            Self::Cline => AgentConfig {
                name: "cline",
                display_name: "Cline",
                skills_dir: ".agents/skills",
                global_skills_dir: Some(PATHS.home.join(".cline").join("skills")),
            },
            Self::CodeartsAgent => AgentConfig {
                name: "codearts-agent",
                display_name: "CodeArts Agent",
                skills_dir: ".codeartsdoer/skills",
                global_skills_dir: Some(PATHS.home.join(".codeartsdoer").join("skills")),
            },
            Self::Codebuddy => AgentConfig {
                name: "codebuddy",
                display_name: "CodeBuddy",
                skills_dir: ".codebuddy/skills",
                global_skills_dir: Some(PATHS.home.join(".codebuddy").join("skills")),
            },
            Self::Codemaker => AgentConfig {
                name: "codemaker",
                display_name: "Codemaker",
                skills_dir: ".codemaker/skills",
                global_skills_dir: Some(PATHS.home.join(".codemaker").join("skills")),
            },
            Self::Codestudio => AgentConfig {
                name: "codestudio",
                display_name: "Code Studio",
                skills_dir: ".codestudio/skills",
                global_skills_dir: Some(PATHS.home.join(".codestudio").join("skills")),
            },
            Self::Codex => AgentConfig {
                name: "codex",
                display_name: "Codex",
                skills_dir: ".agents/skills",
                global_skills_dir: Some(PATHS.codex_home.join("skills")),
            },
            Self::CommandCode => AgentConfig {
                name: "command-code",
                display_name: "Command Code",
                skills_dir: ".commandcode/skills",
                global_skills_dir: Some(PATHS.home.join(".commandcode").join("skills")),
            },
            Self::Continue => AgentConfig {
                name: "continue",
                display_name: "Continue",
                skills_dir: ".continue/skills",
                global_skills_dir: Some(PATHS.home.join(".continue").join("skills")),
            },
            Self::Crush => AgentConfig {
                name: "crush",
                display_name: "Crush",
                skills_dir: ".crush/skills",
                global_skills_dir: Some(PATHS.config_home.join("crush").join("skills")),
            },
            Self::Cursor => AgentConfig {
                name: "cursor",
                display_name: "Cursor",
                skills_dir: ".agents/skills",
                global_skills_dir: Some(PATHS.home.join(".cursor").join("skills")),
            },
            Self::Deepagents => AgentConfig {
                name: "deepagents",
                display_name: "Deep Agents",
                skills_dir: ".agents/skills",
                global_skills_dir: Some(
                    PATHS.home.join(".deepagents").join("agent").join("skills"),
                ),
            },
            Self::Devin => AgentConfig {
                name: "devin",
                display_name: "Devin for Terminal",
                skills_dir: ".devin/skills",
                global_skills_dir: Some(PATHS.config_home.join("devin").join("skills")),
            },
            Self::Dexto => AgentConfig {
                name: "dexto",
                display_name: "Dexto",
                skills_dir: ".agents/skills",
                global_skills_dir: Some(PATHS.home.join(".agents").join("skills")),
            },
            Self::Droid => AgentConfig {
                name: "droid",
                display_name: "Droid",
                skills_dir: ".factory/skills",
                global_skills_dir: Some(PATHS.home.join(".factory").join("skills")),
            },
            Self::Eve => AgentConfig {
                name: "eve",
                display_name: "Eve",
                skills_dir: "agent/skills",
                global_skills_dir: None,
            },
            Self::Firebender => AgentConfig {
                name: "firebender",
                display_name: "Firebender",
                skills_dir: ".agents/skills",
                global_skills_dir: Some(PATHS.home.join(".firebender").join("skills")),
            },
            Self::Forgecode => AgentConfig {
                name: "forgecode",
                display_name: "ForgeCode",
                skills_dir: ".forge/skills",
                global_skills_dir: Some(PATHS.home.join(".forge").join("skills")),
            },
            Self::GeminiCli => AgentConfig {
                name: "gemini-cli",
                display_name: "Gemini CLI",
                skills_dir: ".agents/skills",
                global_skills_dir: Some(PATHS.home.join(".gemini").join("skills")),
            },
            Self::GithubCopilot => AgentConfig {
                name: "github-copilot",
                display_name: "GitHub Copilot",
                skills_dir: ".agents/skills",
                global_skills_dir: Some(PATHS.home.join(".copilot").join("skills")),
            },
            Self::Goose => AgentConfig {
                name: "goose",
                display_name: "Goose",
                skills_dir: ".goose/skills",
                global_skills_dir: Some(PATHS.config_home.join("goose").join("skills")),
            },
            Self::HermesAgent => AgentConfig {
                name: "hermes-agent",
                display_name: "Hermes Agent",
                skills_dir: ".hermes/skills",
                global_skills_dir: Some(Self::hermes_home().join("skills")),
            },
            Self::IflowCli => AgentConfig {
                name: "iflow-cli",
                display_name: "iFlow CLI",
                skills_dir: ".iflow/skills",
                global_skills_dir: Some(PATHS.home.join(".iflow").join("skills")),
            },
            Self::Junie => AgentConfig {
                name: "junie",
                display_name: "Junie",
                skills_dir: ".junie/skills",
                global_skills_dir: Some(PATHS.home.join(".junie").join("skills")),
            },
            Self::Kilo => AgentConfig {
                name: "kilo",
                display_name: "Kilo Code",
                skills_dir: ".kilocode/skills",
                global_skills_dir: Some(PATHS.home.join(".kilocode").join("skills")),
            },
            Self::KimiCodeCli => AgentConfig {
                name: "kimi-code-cli",
                display_name: "Kimi Code CLI",
                skills_dir: ".agents/skills",
                global_skills_dir: Some(PATHS.home.join(".agents").join("skills")),
            },
            Self::KiroCli => AgentConfig {
                name: "kiro-cli",
                display_name: "Kiro CLI",
                skills_dir: ".kiro/skills",
                global_skills_dir: Some(PATHS.home.join(".kiro").join("skills")),
            },
            Self::Kode => AgentConfig {
                name: "kode",
                display_name: "Kode",
                skills_dir: ".kode/skills",
                global_skills_dir: Some(PATHS.home.join(".kode").join("skills")),
            },
            Self::InferenceSh => AgentConfig {
                name: "inference-sh",
                display_name: "inference.sh",
                skills_dir: ".inferencesh/skills",
                global_skills_dir: Some(PATHS.home.join(".inferencesh").join("skills")),
            },
            Self::Jazz => AgentConfig {
                name: "jazz",
                display_name: "Jazz",
                skills_dir: ".jazz/skills",
                global_skills_dir: Some(PATHS.home.join(".jazz").join("skills")),
            },
            Self::Lingma => AgentConfig {
                name: "lingma",
                display_name: "Lingma",
                skills_dir: ".lingma/skills",
                global_skills_dir: Some(PATHS.home.join(".lingma").join("skills")),
            },
            Self::Loaf => AgentConfig {
                name: "loaf",
                display_name: "Loaf",
                skills_dir: ".agents/skills",
                global_skills_dir: Some(PATHS.home.join(".agents").join("skills")),
            },
            Self::Mcpjam => AgentConfig {
                name: "mcpjam",
                display_name: "MCPJam",
                skills_dir: ".mcpjam/skills",
                global_skills_dir: Some(PATHS.home.join(".mcpjam").join("skills")),
            },
            Self::MistralVibe => AgentConfig {
                name: "mistral-vibe",
                display_name: "Mistral Vibe",
                skills_dir: ".vibe/skills",
                global_skills_dir: Some(Self::mistral_vibe_home().join("skills")),
            },
            Self::Moxby => AgentConfig {
                name: "moxby",
                display_name: "Moxby",
                skills_dir: ".moxby/skills",
                global_skills_dir: Some(PATHS.home.join(".moxby").join("skills")),
            },
            Self::Mux => AgentConfig {
                name: "mux",
                display_name: "Mux",
                skills_dir: ".mux/skills",
                global_skills_dir: Some(PATHS.home.join(".mux").join("skills")),
            },
            Self::Neovate => AgentConfig {
                name: "neovate",
                display_name: "Neovate",
                skills_dir: ".neovate/skills",
                global_skills_dir: Some(PATHS.home.join(".neovate").join("skills")),
            },
            Self::Ona => AgentConfig {
                name: "ona",
                display_name: "Ona",
                skills_dir: ".ona/skills",
                global_skills_dir: Some(PATHS.home.join(".ona").join("skills")),
            },
            Self::Opencode => AgentConfig {
                name: "opencode",
                display_name: "OpenCode",
                skills_dir: ".agents/skills",
                global_skills_dir: Some(PATHS.config_home.join("opencode").join("skills")),
            },
            Self::Openhands => AgentConfig {
                name: "openhands",
                display_name: "OpenHands",
                skills_dir: ".openhands/skills",
                global_skills_dir: Some(PATHS.home.join(".openhands").join("skills")),
            },
            Self::Pi => AgentConfig {
                name: "pi",
                display_name: "Pi",
                skills_dir: ".pi/skills",
                global_skills_dir: Some(PATHS.home.join(".pi").join("agent").join("skills")),
            },
            Self::Promptscript => AgentConfig {
                name: "promptscript",
                display_name: "PromptScript",
                skills_dir: ".agents/skills",
                global_skills_dir: None,
            },
            Self::Qoder => AgentConfig {
                name: "qoder",
                display_name: "Qoder",
                skills_dir: ".qoder/skills",
                global_skills_dir: Some(PATHS.home.join(".qoder").join("skills")),
            },
            Self::QoderCn => AgentConfig {
                name: "qoder-cn",
                display_name: "Qoder CN",
                skills_dir: ".qoder/skills",
                global_skills_dir: Some(PATHS.home.join(".qoder-cn").join("skills")),
            },
            Self::QwenCode => AgentConfig {
                name: "qwen-code",
                display_name: "Qwen Code",
                skills_dir: ".qwen/skills",
                global_skills_dir: Some(PATHS.home.join(".qwen").join("skills")),
            },
            Self::Reasonix => AgentConfig {
                name: "reasonix",
                display_name: "Reasonix",
                skills_dir: ".reasonix/skills",
                global_skills_dir: Some(PATHS.home.join(".reasonix").join("skills")),
            },
            Self::Replit => AgentConfig {
                name: "replit",
                display_name: "Replit",
                skills_dir: ".agents/skills",
                global_skills_dir: Some(PATHS.config_home.join("agents").join("skills")),
            },
            Self::Rovodev => AgentConfig {
                name: "rovodev",
                display_name: "Rovo Dev",
                skills_dir: ".rovodev/skills",
                global_skills_dir: Some(PATHS.home.join(".rovodev").join("skills")),
            },
            Self::Roo => AgentConfig {
                name: "roo",
                display_name: "Roo Code",
                skills_dir: ".roo/skills",
                global_skills_dir: Some(PATHS.home.join(".roo").join("skills")),
            },
            Self::TabnineCli => AgentConfig {
                name: "tabnine-cli",
                display_name: "Tabnine CLI",
                skills_dir: ".tabnine/agent/skills",
                global_skills_dir: Some(PATHS.home.join(".tabnine").join("agent").join("skills")),
            },
            Self::Trae => AgentConfig {
                name: "trae",
                display_name: "Trae",
                skills_dir: ".trae/skills",
                global_skills_dir: Some(PATHS.home.join(".trae").join("skills")),
            },
            Self::TraeCn => AgentConfig {
                name: "trae-cn",
                display_name: "Trae CN",
                skills_dir: ".trae/skills",
                global_skills_dir: Some(PATHS.home.join(".trae-cn").join("skills")),
            },
            Self::Warp => AgentConfig {
                name: "warp",
                display_name: "Warp",
                skills_dir: ".agents/skills",
                global_skills_dir: Some(PATHS.home.join(".agents").join("skills")),
            },
            Self::Windsurf => AgentConfig {
                name: "windsurf",
                display_name: "Windsurf",
                skills_dir: ".windsurf/skills",
                global_skills_dir: Some(
                    PATHS.home.join(".codeium").join("windsurf").join("skills"),
                ),
            },
            Self::Zed => AgentConfig {
                name: "zed",
                display_name: "Zed",
                skills_dir: ".agents/skills",
                global_skills_dir: Some(PATHS.home.join(".agents").join("skills")),
            },
            Self::Zencoder => AgentConfig {
                name: "zencoder",
                display_name: "Zencoder",
                skills_dir: ".zencoder/skills",
                global_skills_dir: Some(PATHS.home.join(".zencoder").join("skills")),
            },
            Self::Zenflow => AgentConfig {
                name: "zenflow",
                display_name: "Zenflow",
                skills_dir: ".zencoder/skills",
                global_skills_dir: Some(PATHS.home.join(".zencoder").join("skills")),
            },
            Self::Pochi => AgentConfig {
                name: "pochi",
                display_name: "Pochi",
                skills_dir: ".pochi/skills",
                global_skills_dir: Some(PATHS.home.join(".pochi").join("skills")),
            },
            Self::Adal => AgentConfig {
                name: "adal",
                display_name: "AdaL",
                skills_dir: ".adal/skills",
                global_skills_dir: Some(PATHS.home.join(".adal").join("skills")),
            },
            // Cortex Code: Snowflake 的 AI 编码助手
            // 对应 CLI: agents.ts cortex 配置
            Self::Cortex => AgentConfig {
                name: "cortex",
                display_name: "Cortex Code",
                skills_dir: ".cortex/skills",
                global_skills_dir: Some(
                    PATHS.home.join(".snowflake").join("cortex").join("skills"),
                ),
            },
            Self::Terramind => AgentConfig {
                name: "terramind",
                display_name: "Terramind",
                skills_dir: ".terramind/skills",
                global_skills_dir: Some(PATHS.home.join(".terramind").join("skills")),
            },
            Self::Tinycloud => AgentConfig {
                name: "tinycloud",
                display_name: "Tinycloud",
                skills_dir: ".tinycloud/skills",
                global_skills_dir: Some(PATHS.home.join(".tinycloud").join("skills")),
            },
        }
    }

    /// OpenClaw 的 global 目录需要检测多个可能位置
    /// 对应 CLI: agents.ts 第 56-60 行
    fn openclaw_global_dir() -> PathBuf {
        if PATHS.home.join(".openclaw").exists() {
            PATHS.home.join(".openclaw").join("skills")
        } else if PATHS.home.join(".clawdbot").exists() {
            PATHS.home.join(".clawdbot").join("skills")
        } else if PATHS.home.join(".moltbot").exists() {
            PATHS.home.join(".moltbot").join("skills")
        } else {
            PATHS.home.join(".openclaw").join("skills")
        }
    }

    fn mistral_vibe_home() -> PathBuf {
        std::env::var("VIBE_HOME")
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| PATHS.home.join(".vibe"))
    }

    fn hermes_home() -> PathBuf {
        std::env::var("HERMES_HOME")
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| PATHS.home.join(".hermes"))
    }

    fn autohand_home() -> PathBuf {
        std::env::var("AUTOHAND_HOME")
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| PATHS.home.join(".autohand"))
    }

    /// 检测 Agent 是否已安装
    /// 完整对应 CLI: 每个 agent 的 detectInstalled 函数
    #[cfg(test)]
    pub fn is_installed(&self) -> bool {
        let cwd = std::env::current_dir().unwrap_or_default();

        match self {
            Self::AiderDesk => PATHS.home.join(".aider-desk").exists(),
            Self::Amp => PATHS.config_home.join("amp").exists(),
            Self::Antigravity => PATHS.home.join(".gemini").join("antigravity").exists(),
            Self::AntigravityCli => PATHS.home.join(".gemini").join("antigravity-cli").exists(),
            Self::Astrbot => {
                cwd.join("data").join("skills").exists() || PATHS.home.join(".astrbot").exists()
            }
            Self::Augment => PATHS.home.join(".augment").exists(),
            Self::AutohandCode => Self::autohand_home().exists(),
            Self::Bob => PATHS.home.join(".bob").exists(),
            Self::ClaudeCode => PATHS.claude_home.exists(),
            Self::Openclaw => {
                PATHS.home.join(".openclaw").exists()
                    || PATHS.home.join(".clawdbot").exists()
                    || PATHS.home.join(".moltbot").exists()
            }
            Self::Cline => PATHS.home.join(".cline").exists(),
            Self::CodeartsAgent => PATHS.home.join(".codeartsdoer").exists(),
            Self::Codebuddy => {
                cwd.join(".codebuddy").exists() || PATHS.home.join(".codebuddy").exists()
            }
            Self::Codemaker => PATHS.home.join(".codemaker").exists(),
            Self::Codestudio => PATHS.home.join(".codestudio").exists(),
            Self::Codex => PATHS.codex_home.exists() || std::path::Path::new("/etc/codex").exists(),
            Self::CommandCode => PATHS.home.join(".commandcode").exists(),
            Self::Continue => {
                cwd.join(".continue").exists() || PATHS.home.join(".continue").exists()
            }
            Self::Crush => PATHS.config_home.join("crush").exists(),
            Self::Cursor => PATHS.home.join(".cursor").exists(),
            Self::Deepagents => PATHS.home.join(".deepagents").exists(),
            Self::Devin => PATHS.config_home.join("devin").exists(),
            Self::Dexto => PATHS.home.join(".dexto").exists(),
            Self::Droid => PATHS.home.join(".factory").exists(),
            Self::Eve => crate::core::eve::is_eve_project(&cwd.to_string_lossy()),
            Self::Firebender => PATHS.home.join(".firebender").exists(),
            Self::Forgecode => PATHS.home.join(".forge").exists(),
            Self::GeminiCli => PATHS.home.join(".gemini").exists(),
            Self::GithubCopilot => PATHS.home.join(".copilot").exists(),
            Self::Goose => PATHS.config_home.join("goose").exists(),
            Self::HermesAgent => Self::hermes_home().exists(),
            Self::IflowCli => PATHS.home.join(".iflow").exists(),
            Self::Junie => PATHS.home.join(".junie").exists(),
            Self::Kilo => PATHS.home.join(".kilocode").exists(),
            Self::KimiCodeCli => {
                PATHS.home.join(".kimi-code").exists() || PATHS.home.join(".kimi").exists()
            }
            Self::KiroCli => PATHS.home.join(".kiro").exists(),
            Self::Kode => PATHS.home.join(".kode").exists(),
            Self::InferenceSh => PATHS.home.join(".inferencesh").exists(),
            Self::Jazz => cwd.join(".jazz").exists() || PATHS.home.join(".jazz").exists(),
            Self::Lingma => PATHS.home.join(".lingma").exists(),
            Self::Loaf => PATHS.home.join(".loaf").exists(),
            Self::Mcpjam => PATHS.home.join(".mcpjam").exists(),
            Self::MistralVibe => Self::mistral_vibe_home().exists(),
            Self::Moxby => PATHS.home.join(".moxby").exists(),
            Self::Mux => PATHS.home.join(".mux").exists(),
            Self::Neovate => PATHS.home.join(".neovate").exists(),
            Self::Ona => PATHS.home.join(".ona").exists(),
            Self::Opencode => PATHS.config_home.join("opencode").exists(),
            Self::Openhands => PATHS.home.join(".openhands").exists(),
            Self::Pi => PATHS.home.join(".pi").join("agent").exists(),
            Self::Promptscript => {
                cwd.join(".promptscript").exists() || cwd.join("promptscript.yaml").exists()
            }
            Self::Qoder => PATHS.home.join(".qoder").exists(),
            Self::QoderCn => PATHS.home.join(".qoder-cn").exists(),
            Self::QwenCode => PATHS.home.join(".qwen").exists(),
            Self::Reasonix => PATHS.home.join(".reasonix").exists(),
            Self::Replit => cwd.join(".replit").exists(),
            Self::Rovodev => PATHS.home.join(".rovodev").exists(),
            Self::Roo => PATHS.home.join(".roo").exists(),
            Self::TabnineCli => PATHS.home.join(".tabnine").exists(),
            Self::Trae => PATHS.home.join(".trae").exists(),
            Self::TraeCn => PATHS.home.join(".trae-cn").exists(),
            Self::Warp => PATHS.home.join(".warp").exists(),
            Self::Windsurf => PATHS.home.join(".codeium").join("windsurf").exists(),
            Self::Zed => {
                PATHS.config_home.join("zed").exists()
                    || std::env::var_os("APPDATA")
                        .filter(|value| !value.is_empty())
                        .map(|value| PathBuf::from(value).join("Zed").exists())
                        .unwrap_or(false)
                    || std::env::var_os("FLATPAK_XDG_CONFIG_HOME")
                        .filter(|value| !value.is_empty())
                        .map(|value| PathBuf::from(value).join("zed").exists())
                        .unwrap_or(false)
            }
            Self::Zencoder => PATHS.home.join(".zencoder").exists(),
            Self::Zenflow => PATHS.home.join(".zencoder").exists(),
            Self::Pochi => PATHS.home.join(".pochi").exists(),
            Self::Adal => PATHS.home.join(".adal").exists(),
            Self::Cortex => PATHS.home.join(".snowflake").join("cortex").exists(),
            Self::Terramind => PATHS.home.join(".terramind").exists(),
            Self::Tinycloud => PATHS.home.join(".tinycloud").exists(),
        }
    }

    /// 检测所有已安装的 Agent
    /// 对应 CLI: detectInstalledAgents (agents.ts:378-386)
    #[cfg(test)]
    pub fn detect_installed() -> Vec<AgentType> {
        Self::all().filter(|agent| agent.is_installed()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    mod cli_1_5_10 {
        use super::*;
        use std::sync::Mutex;

        static ENV_LOCK: Mutex<()> = Mutex::new(());

        #[test]
        fn test_cli_1_5_10_agent_count() {
            assert_eq!(AgentType::all().count(), 71);
        }

        #[test]
        fn test_eve_parse_display_and_config() {
            assert_eq!("eve".parse::<AgentType>().ok(), Some(AgentType::Eve));
            assert_eq!(AgentType::Eve.to_string(), "eve");

            let config = AgentType::Eve.config();
            assert_eq!(config.name, "eve");
            assert_eq!(config.display_name, "Eve");
            assert_eq!(config.skills_dir, "agent/skills");
            assert!(config.global_skills_dir.is_none());
        }

        #[test]
        fn test_kimi_code_cli_parse_and_legacy_alias() {
            assert_eq!(
                "kimi-code-cli".parse::<AgentType>().ok(),
                Some(AgentType::KimiCodeCli)
            );
            assert_eq!(
                "kimi-cli".parse::<AgentType>().ok(),
                Some(AgentType::KimiCodeCli)
            );
            assert_eq!(AgentType::KimiCodeCli.to_string(), "kimi-code-cli");
        }

        #[test]
        fn test_hermes_home_env_is_used_for_config_and_detection_path() {
            let original = std::env::var_os("HERMES_HOME");
            let temp = tempfile::tempdir().unwrap();
            std::env::set_var("HERMES_HOME", temp.path());

            let config = AgentType::HermesAgent.config();
            assert_eq!(
                config.global_skills_dir.as_deref(),
                Some(temp.path().join("skills").as_path())
            );
            assert!(AgentType::HermesAgent.is_installed());

            match original {
                Some(value) => std::env::set_var("HERMES_HOME", value),
                None => std::env::remove_var("HERMES_HOME"),
            }
        }

        #[test]
        fn test_autohand_home_env_is_used_for_config() {
            let _guard = ENV_LOCK.lock().unwrap();
            let original = std::env::var_os("AUTOHAND_HOME");
            let temp = tempfile::tempdir().unwrap();
            std::env::set_var("AUTOHAND_HOME", temp.path());

            let config = AgentType::AutohandCode.config();
            assert_eq!(
                config.global_skills_dir.as_deref(),
                Some(temp.path().join("skills").as_path())
            );

            match original {
                Some(value) => std::env::set_var("AUTOHAND_HOME", value),
                None => std::env::remove_var("AUTOHAND_HOME"),
            }
        }

        #[test]
        fn test_new_agents_are_parseable_and_configured() {
            let _guard = ENV_LOCK.lock().unwrap();
            let original_autohand_home = std::env::var_os("AUTOHAND_HOME");
            std::env::remove_var("AUTOHAND_HOME");

            let agents = [
                (
                    "antigravity-cli",
                    "Antigravity CLI",
                    ".agents/skills",
                    Some(
                        PATHS
                            .home
                            .join(".gemini")
                            .join("antigravity-cli")
                            .join("skills"),
                    ),
                ),
                (
                    "astrbot",
                    "AstrBot",
                    "data/skills",
                    Some(PATHS.home.join(".astrbot").join("data").join("skills")),
                ),
                (
                    "autohand-code",
                    "Autohand Code CLI",
                    ".autohand/skills",
                    Some(PATHS.home.join(".autohand").join("skills")),
                ),
                (
                    "inference-sh",
                    "inference.sh",
                    ".inferencesh/skills",
                    Some(PATHS.home.join(".inferencesh").join("skills")),
                ),
                (
                    "zenflow",
                    "Zenflow",
                    ".zencoder/skills",
                    Some(PATHS.home.join(".zencoder").join("skills")),
                ),
                ("promptscript", "PromptScript", ".agents/skills", None),
            ];

            for (name, display_name, skills_dir, global_skills_dir) in agents {
                let agent: AgentType = name.parse().unwrap();
                let config = agent.config();

                assert_eq!(agent.to_string(), name);
                assert_eq!(config.name, name);
                assert_eq!(config.display_name, display_name);
                assert_eq!(config.skills_dir, skills_dir);
                assert_eq!(config.global_skills_dir, global_skills_dir);
            }

            match original_autohand_home {
                Some(value) => std::env::set_var("AUTOHAND_HOME", value),
                None => std::env::remove_var("AUTOHAND_HOME"),
            }
        }
    }

    #[test]
    fn test_agent_type_all_count() {
        let count = AgentType::all().count();
        assert_eq!(
            count, 71,
            "Should have 71 real agent types after syncing CLI 1.5.13"
        );
    }

    #[test]
    fn test_cli_1_5_7_agents_are_parseable_and_configured() {
        let agents = [
            ("aider-desk", "AiderDesk", ".aider-desk/skills"),
            ("codearts-agent", "CodeArts Agent", ".codeartsdoer/skills"),
            ("codemaker", "Codemaker", ".codemaker/skills"),
            ("codestudio", "Code Studio", ".codestudio/skills"),
            ("devin", "Devin for Terminal", ".devin/skills"),
            ("dexto", "Dexto", ".agents/skills"),
            ("forgecode", "ForgeCode", ".forge/skills"),
            ("hermes-agent", "Hermes Agent", ".hermes/skills"),
            ("rovodev", "Rovo Dev", ".rovodev/skills"),
            ("tabnine-cli", "Tabnine CLI", ".tabnine/agent/skills"),
        ];

        for (name, display_name, skills_dir) in agents {
            let agent: AgentType = name.parse().unwrap();
            let config = agent.config();

            assert_eq!(agent.to_string(), name);
            assert_eq!(config.name, name);
            assert_eq!(config.display_name, display_name);
            assert_eq!(config.skills_dir, skills_dir);
        }
    }

    #[test]
    fn test_mistral_vibe_uses_vibe_home_for_global_skills_dir() {
        let original = std::env::var_os("VIBE_HOME");
        let temp = tempfile::tempdir().unwrap();
        std::env::set_var("VIBE_HOME", temp.path());

        let config = AgentType::MistralVibe.config();
        assert_eq!(
            config.global_skills_dir.as_deref(),
            Some(temp.path().join("skills").as_path())
        );

        match original {
            Some(value) => std::env::set_var("VIBE_HOME", value),
            None => std::env::remove_var("VIBE_HOME"),
        }
    }

    #[test]
    fn test_agent_config_name_matches_serde() {
        let claude = AgentType::ClaudeCode;
        let config = claude.config();
        assert_eq!(config.name, "claude-code");

        let cursor = AgentType::Cursor;
        let config = cursor.config();
        assert_eq!(config.name, "cursor");
    }

    #[test]
    fn test_detect_installed_returns_vec() {
        let installed = AgentType::detect_installed();
        assert!(installed.len() <= AgentType::all().count());
    }

    #[test]
    fn test_cortex_agent() {
        let config = AgentType::Cortex.config();
        assert_eq!(config.name, "cortex");
        assert_eq!(config.skills_dir, ".cortex/skills");
    }

    #[test]
    fn test_replit_detection_changed() {
        // Replit 现在检查 .replit 而非 .agents
        // 这里只能验证不 panic
        let _ = AgentType::Replit.is_installed();
    }

    #[test]
    fn test_warp_agent_is_parseable() {
        let parsed = "warp".parse::<AgentType>();
        assert!(parsed.is_ok(), "warp should be a supported agent");
    }

    #[test]
    fn test_deepagents_agent_is_parseable() {
        let parsed = "deepagents".parse::<AgentType>();
        assert!(parsed.is_ok(), "deepagents should be a supported agent");
    }

    #[test]
    fn test_zed_agent_is_parseable() {
        let parsed = "zed".parse::<AgentType>();
        assert_eq!(parsed.ok(), Some(AgentType::Zed));
        assert_eq!(AgentType::Zed.to_string(), "zed");
    }

    #[test]
    fn test_zed_agent_config_matches_cli() {
        let config = AgentType::Zed.config();
        let expected_global = PATHS.home.join(".agents").join("skills");
        assert_eq!(config.name, "zed");
        assert_eq!(config.display_name, "Zed");
        assert_eq!(config.skills_dir, ".agents/skills");
        assert_eq!(
            config.global_skills_dir.as_deref(),
            Some(expected_global.as_path())
        );
    }

    #[test]
    fn test_antigravity_uses_shared_project_dir() {
        let config = AgentType::Antigravity.config();
        assert_eq!(config.skills_dir, ".agents/skills");
    }
}

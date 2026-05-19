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

/// Agent 在单个安装范围下的目标能力
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct AgentScopeTarget {
    pub supported: bool,
    pub automatic: bool,
    pub path: String,
}

/// Agent 在全局和项目两个安装范围下的目标能力
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct AgentTargets {
    pub global: AgentScopeTarget,
    pub project: AgentScopeTarget,
}

/// Agent 信息（返回给前端）
/// 对应 CLI: 综合 AgentConfig + detectInstalled 结果
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct AgentInfo {
    pub id: AgentType,
    pub name: String,
    pub skills_dir: String,
    pub global_skills_dir: String,
    pub detected: bool,
    /// 按安装范围计算后的目标能力
    pub targets: AgentTargets,
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
    Augment,
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
    Firebender,
    Forgecode,
    GeminiCli,
    GithubCopilot,
    Goose,
    HermesAgent,
    IflowCli,
    Junie,
    Kilo,
    KimiCli,
    KiroCli,
    Kode,
    Mcpjam,
    MistralVibe,
    Mux,
    Neovate,
    Opencode,
    Openhands,
    Pi,
    Qoder,
    QwenCode,
    Replit,
    Rovodev,
    Roo,
    TabnineCli,
    Trae,
    TraeCn,
    Warp,
    Windsurf,
    Zencoder,
    Pochi,
    Adal,
    Cortex,
}

impl std::fmt::Display for AgentType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = match self {
            Self::AiderDesk => "aider-desk",
            Self::Amp => "amp",
            Self::Antigravity => "antigravity",
            Self::Augment => "augment",
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
            Self::Firebender => "firebender",
            Self::Forgecode => "forgecode",
            Self::GeminiCli => "gemini-cli",
            Self::GithubCopilot => "github-copilot",
            Self::Goose => "goose",
            Self::HermesAgent => "hermes-agent",
            Self::IflowCli => "iflow-cli",
            Self::Junie => "junie",
            Self::Kilo => "kilo",
            Self::KimiCli => "kimi-cli",
            Self::KiroCli => "kiro-cli",
            Self::Kode => "kode",
            Self::Mcpjam => "mcpjam",
            Self::MistralVibe => "mistral-vibe",
            Self::Mux => "mux",
            Self::Neovate => "neovate",
            Self::Opencode => "opencode",
            Self::Openhands => "openhands",
            Self::Pi => "pi",
            Self::Qoder => "qoder",
            Self::QwenCode => "qwen-code",
            Self::Replit => "replit",
            Self::Rovodev => "rovodev",
            Self::Roo => "roo",
            Self::TabnineCli => "tabnine-cli",
            Self::Trae => "trae",
            Self::TraeCn => "trae-cn",
            Self::Warp => "warp",
            Self::Windsurf => "windsurf",
            Self::Zencoder => "zencoder",
            Self::Pochi => "pochi",
            Self::Adal => "adal",
            Self::Cortex => "cortex",
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
            "augment" => Ok(Self::Augment),
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
            "firebender" => Ok(Self::Firebender),
            "forgecode" => Ok(Self::Forgecode),
            "gemini-cli" => Ok(Self::GeminiCli),
            "github-copilot" => Ok(Self::GithubCopilot),
            "goose" => Ok(Self::Goose),
            "hermes-agent" => Ok(Self::HermesAgent),
            "iflow-cli" => Ok(Self::IflowCli),
            "junie" => Ok(Self::Junie),
            "kilo" => Ok(Self::Kilo),
            "kimi-cli" => Ok(Self::KimiCli),
            "kiro-cli" => Ok(Self::KiroCli),
            "kode" => Ok(Self::Kode),
            "mcpjam" => Ok(Self::Mcpjam),
            "mistral-vibe" => Ok(Self::MistralVibe),
            "mux" => Ok(Self::Mux),
            "neovate" => Ok(Self::Neovate),
            "opencode" => Ok(Self::Opencode),
            "openhands" => Ok(Self::Openhands),
            "pi" => Ok(Self::Pi),
            "qoder" => Ok(Self::Qoder),
            "qwen-code" => Ok(Self::QwenCode),
            "replit" => Ok(Self::Replit),
            "rovodev" => Ok(Self::Rovodev),
            "roo" => Ok(Self::Roo),
            "tabnine-cli" => Ok(Self::TabnineCli),
            "trae" => Ok(Self::Trae),
            "trae-cn" => Ok(Self::TraeCn),
            "warp" => Ok(Self::Warp),
            "windsurf" => Ok(Self::Windsurf),
            "zencoder" => Ok(Self::Zencoder),
            "pochi" => Ok(Self::Pochi),
            "adal" => Ok(Self::Adal),
            "cortex" => Ok(Self::Cortex),
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
            Self::Augment,
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
            Self::Firebender,
            Self::Forgecode,
            Self::GeminiCli,
            Self::GithubCopilot,
            Self::Goose,
            Self::HermesAgent,
            Self::IflowCli,
            Self::Junie,
            Self::Kilo,
            Self::KimiCli,
            Self::KiroCli,
            Self::Kode,
            Self::Mcpjam,
            Self::MistralVibe,
            Self::Mux,
            Self::Neovate,
            Self::Opencode,
            Self::Openhands,
            Self::Pi,
            Self::Qoder,
            Self::QwenCode,
            Self::Replit,
            Self::Rovodev,
            Self::Roo,
            Self::TabnineCli,
            Self::Trae,
            Self::TraeCn,
            Self::Warp,
            Self::Windsurf,
            Self::Zencoder,
            Self::Pochi,
            Self::Adal,
            Self::Cortex,
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
                global_skills_dir: Some(PATHS.home.join(".aider-desk/skills")),
            },
            Self::Amp => AgentConfig {
                name: "amp",
                display_name: "Amp",
                skills_dir: ".agents/skills",
                global_skills_dir: Some(PATHS.config_home.join("agents/skills")),
            },
            Self::Antigravity => AgentConfig {
                name: "antigravity",
                display_name: "Antigravity",
                skills_dir: ".agents/skills",
                global_skills_dir: Some(PATHS.home.join(".gemini/antigravity/skills")),
            },
            Self::Augment => AgentConfig {
                name: "augment",
                display_name: "Augment",
                skills_dir: ".augment/skills",
                global_skills_dir: Some(PATHS.home.join(".augment/skills")),
            },
            Self::Bob => AgentConfig {
                name: "bob",
                display_name: "IBM Bob",
                skills_dir: ".bob/skills",
                global_skills_dir: Some(PATHS.home.join(".bob/skills")),
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
                global_skills_dir: Some(PATHS.home.join(".agents").join("skills")),
            },
            Self::CodeartsAgent => AgentConfig {
                name: "codearts-agent",
                display_name: "CodeArts Agent",
                skills_dir: ".codeartsdoer/skills",
                global_skills_dir: Some(PATHS.home.join(".codeartsdoer/skills")),
            },
            Self::Codebuddy => AgentConfig {
                name: "codebuddy",
                display_name: "CodeBuddy",
                skills_dir: ".codebuddy/skills",
                global_skills_dir: Some(PATHS.home.join(".codebuddy/skills")),
            },
            Self::Codemaker => AgentConfig {
                name: "codemaker",
                display_name: "Codemaker",
                skills_dir: ".codemaker/skills",
                global_skills_dir: Some(PATHS.home.join(".codemaker/skills")),
            },
            Self::Codestudio => AgentConfig {
                name: "codestudio",
                display_name: "Code Studio",
                skills_dir: ".codestudio/skills",
                global_skills_dir: Some(PATHS.home.join(".codestudio/skills")),
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
                global_skills_dir: Some(PATHS.home.join(".commandcode/skills")),
            },
            Self::Continue => AgentConfig {
                name: "continue",
                display_name: "Continue",
                skills_dir: ".continue/skills",
                global_skills_dir: Some(PATHS.home.join(".continue/skills")),
            },
            Self::Crush => AgentConfig {
                name: "crush",
                display_name: "Crush",
                skills_dir: ".crush/skills",
                global_skills_dir: Some(PATHS.config_home.join("crush/skills")),
            },
            Self::Cursor => AgentConfig {
                name: "cursor",
                display_name: "Cursor",
                skills_dir: ".agents/skills",
                global_skills_dir: Some(PATHS.home.join(".cursor/skills")),
            },
            Self::Deepagents => AgentConfig {
                name: "deepagents",
                display_name: "Deep Agents",
                skills_dir: ".agents/skills",
                global_skills_dir: Some(PATHS.home.join(".deepagents/agent/skills")),
            },
            Self::Devin => AgentConfig {
                name: "devin",
                display_name: "Devin for Terminal",
                skills_dir: ".devin/skills",
                global_skills_dir: Some(PATHS.config_home.join("devin/skills")),
            },
            Self::Dexto => AgentConfig {
                name: "dexto",
                display_name: "Dexto",
                skills_dir: ".agents/skills",
                global_skills_dir: Some(PATHS.home.join(".agents/skills")),
            },
            Self::Droid => AgentConfig {
                name: "droid",
                display_name: "Droid",
                skills_dir: ".factory/skills",
                global_skills_dir: Some(PATHS.home.join(".factory/skills")),
            },
            Self::Firebender => AgentConfig {
                name: "firebender",
                display_name: "Firebender",
                skills_dir: ".agents/skills",
                global_skills_dir: Some(PATHS.home.join(".firebender/skills")),
            },
            Self::Forgecode => AgentConfig {
                name: "forgecode",
                display_name: "ForgeCode",
                skills_dir: ".forge/skills",
                global_skills_dir: Some(PATHS.home.join(".forge/skills")),
            },
            Self::GeminiCli => AgentConfig {
                name: "gemini-cli",
                display_name: "Gemini CLI",
                skills_dir: ".agents/skills",
                global_skills_dir: Some(PATHS.home.join(".gemini/skills")),
            },
            Self::GithubCopilot => AgentConfig {
                name: "github-copilot",
                display_name: "GitHub Copilot",
                skills_dir: ".agents/skills",
                global_skills_dir: Some(PATHS.home.join(".copilot/skills")),
            },
            Self::Goose => AgentConfig {
                name: "goose",
                display_name: "Goose",
                skills_dir: ".goose/skills",
                global_skills_dir: Some(PATHS.config_home.join("goose/skills")),
            },
            Self::HermesAgent => AgentConfig {
                name: "hermes-agent",
                display_name: "Hermes Agent",
                skills_dir: ".hermes/skills",
                global_skills_dir: Some(PATHS.home.join(".hermes/skills")),
            },
            Self::IflowCli => AgentConfig {
                name: "iflow-cli",
                display_name: "iFlow CLI",
                skills_dir: ".iflow/skills",
                global_skills_dir: Some(PATHS.home.join(".iflow/skills")),
            },
            Self::Junie => AgentConfig {
                name: "junie",
                display_name: "Junie",
                skills_dir: ".junie/skills",
                global_skills_dir: Some(PATHS.home.join(".junie/skills")),
            },
            Self::Kilo => AgentConfig {
                name: "kilo",
                display_name: "Kilo Code",
                skills_dir: ".kilocode/skills",
                global_skills_dir: Some(PATHS.home.join(".kilocode/skills")),
            },
            Self::KimiCli => AgentConfig {
                name: "kimi-cli",
                display_name: "Kimi Code CLI",
                skills_dir: ".agents/skills",
                global_skills_dir: Some(PATHS.config_home.join("agents/skills")),
            },
            Self::KiroCli => AgentConfig {
                name: "kiro-cli",
                display_name: "Kiro CLI",
                skills_dir: ".kiro/skills",
                global_skills_dir: Some(PATHS.home.join(".kiro/skills")),
            },
            Self::Kode => AgentConfig {
                name: "kode",
                display_name: "Kode",
                skills_dir: ".kode/skills",
                global_skills_dir: Some(PATHS.home.join(".kode/skills")),
            },
            Self::Mcpjam => AgentConfig {
                name: "mcpjam",
                display_name: "MCPJam",
                skills_dir: ".mcpjam/skills",
                global_skills_dir: Some(PATHS.home.join(".mcpjam/skills")),
            },
            Self::MistralVibe => AgentConfig {
                name: "mistral-vibe",
                display_name: "Mistral Vibe",
                skills_dir: ".vibe/skills",
                global_skills_dir: Some(Self::mistral_vibe_home().join("skills")),
            },
            Self::Mux => AgentConfig {
                name: "mux",
                display_name: "Mux",
                skills_dir: ".mux/skills",
                global_skills_dir: Some(PATHS.home.join(".mux/skills")),
            },
            Self::Neovate => AgentConfig {
                name: "neovate",
                display_name: "Neovate",
                skills_dir: ".neovate/skills",
                global_skills_dir: Some(PATHS.home.join(".neovate/skills")),
            },
            Self::Opencode => AgentConfig {
                name: "opencode",
                display_name: "OpenCode",
                skills_dir: ".agents/skills",
                global_skills_dir: Some(PATHS.config_home.join("opencode/skills")),
            },
            Self::Openhands => AgentConfig {
                name: "openhands",
                display_name: "OpenHands",
                skills_dir: ".openhands/skills",
                global_skills_dir: Some(PATHS.home.join(".openhands/skills")),
            },
            Self::Pi => AgentConfig {
                name: "pi",
                display_name: "Pi",
                skills_dir: ".pi/skills",
                global_skills_dir: Some(PATHS.home.join(".pi/agent/skills")),
            },
            Self::Qoder => AgentConfig {
                name: "qoder",
                display_name: "Qoder",
                skills_dir: ".qoder/skills",
                global_skills_dir: Some(PATHS.home.join(".qoder/skills")),
            },
            Self::QwenCode => AgentConfig {
                name: "qwen-code",
                display_name: "Qwen Code",
                skills_dir: ".qwen/skills",
                global_skills_dir: Some(PATHS.home.join(".qwen/skills")),
            },
            Self::Replit => AgentConfig {
                name: "replit",
                display_name: "Replit",
                skills_dir: ".agents/skills",
                global_skills_dir: Some(PATHS.config_home.join("agents/skills")),
            },
            Self::Rovodev => AgentConfig {
                name: "rovodev",
                display_name: "Rovo Dev",
                skills_dir: ".rovodev/skills",
                global_skills_dir: Some(PATHS.home.join(".rovodev/skills")),
            },
            Self::Roo => AgentConfig {
                name: "roo",
                display_name: "Roo Code",
                skills_dir: ".roo/skills",
                global_skills_dir: Some(PATHS.home.join(".roo/skills")),
            },
            Self::TabnineCli => AgentConfig {
                name: "tabnine-cli",
                display_name: "Tabnine CLI",
                skills_dir: ".tabnine/agent/skills",
                global_skills_dir: Some(PATHS.home.join(".tabnine/agent/skills")),
            },
            Self::Trae => AgentConfig {
                name: "trae",
                display_name: "Trae",
                skills_dir: ".trae/skills",
                global_skills_dir: Some(PATHS.home.join(".trae/skills")),
            },
            Self::TraeCn => AgentConfig {
                name: "trae-cn",
                display_name: "Trae CN",
                skills_dir: ".trae/skills",
                global_skills_dir: Some(PATHS.home.join(".trae-cn/skills")),
            },
            Self::Warp => AgentConfig {
                name: "warp",
                display_name: "Warp",
                skills_dir: ".agents/skills",
                global_skills_dir: Some(PATHS.home.join(".agents/skills")),
            },
            Self::Windsurf => AgentConfig {
                name: "windsurf",
                display_name: "Windsurf",
                skills_dir: ".windsurf/skills",
                global_skills_dir: Some(PATHS.home.join(".codeium/windsurf/skills")),
            },
            Self::Zencoder => AgentConfig {
                name: "zencoder",
                display_name: "Zencoder",
                skills_dir: ".zencoder/skills",
                global_skills_dir: Some(PATHS.home.join(".zencoder/skills")),
            },
            Self::Pochi => AgentConfig {
                name: "pochi",
                display_name: "Pochi",
                skills_dir: ".pochi/skills",
                global_skills_dir: Some(PATHS.home.join(".pochi/skills")),
            },
            Self::Adal => AgentConfig {
                name: "adal",
                display_name: "AdaL",
                skills_dir: ".adal/skills",
                global_skills_dir: Some(PATHS.home.join(".adal/skills")),
            },
            // Cortex Code: Snowflake 的 AI 编码助手
            // 对应 CLI: agents.ts cortex 配置
            Self::Cortex => AgentConfig {
                name: "cortex",
                display_name: "Cortex Code",
                skills_dir: ".cortex/skills",
                global_skills_dir: Some(PATHS.home.join(".snowflake/cortex/skills")),
            },
        }
    }

    /// OpenClaw 的 global 目录需要检测多个可能位置
    /// 对应 CLI: agents.ts 第 56-60 行
    fn openclaw_global_dir() -> PathBuf {
        if PATHS.home.join(".openclaw").exists() {
            PATHS.home.join(".openclaw/skills")
        } else if PATHS.home.join(".clawdbot").exists() {
            PATHS.home.join(".clawdbot/skills")
        } else if PATHS.home.join(".moltbot").exists() {
            PATHS.home.join(".moltbot/skills")
        } else {
            PATHS.home.join(".openclaw/skills")
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

    /// 检测 Agent 是否已安装
    /// 完整对应 CLI: 每个 agent 的 detectInstalled 函数
    pub fn is_installed(&self) -> bool {
        let cwd = std::env::current_dir().unwrap_or_default();

        match self {
            Self::AiderDesk => PATHS.home.join(".aider-desk").exists(),
            Self::Amp => PATHS.config_home.join("amp").exists(),
            Self::Antigravity => PATHS.home.join(".gemini/antigravity").exists(),
            Self::Augment => PATHS.home.join(".augment").exists(),
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
            Self::Firebender => PATHS.home.join(".firebender").exists(),
            Self::Forgecode => PATHS.home.join(".forge").exists(),
            Self::GeminiCli => PATHS.home.join(".gemini").exists(),
            Self::GithubCopilot => PATHS.home.join(".copilot").exists(),
            Self::Goose => PATHS.config_home.join("goose").exists(),
            Self::HermesAgent => PATHS.home.join(".hermes").exists(),
            Self::IflowCli => PATHS.home.join(".iflow").exists(),
            Self::Junie => PATHS.home.join(".junie").exists(),
            Self::Kilo => PATHS.home.join(".kilocode").exists(),
            Self::KimiCli => PATHS.home.join(".kimi").exists(),
            Self::KiroCli => PATHS.home.join(".kiro").exists(),
            Self::Kode => PATHS.home.join(".kode").exists(),
            Self::Mcpjam => PATHS.home.join(".mcpjam").exists(),
            Self::MistralVibe => Self::mistral_vibe_home().exists(),
            Self::Mux => PATHS.home.join(".mux").exists(),
            Self::Neovate => PATHS.home.join(".neovate").exists(),
            Self::Opencode => PATHS.config_home.join("opencode").exists(),
            Self::Openhands => PATHS.home.join(".openhands").exists(),
            Self::Pi => PATHS.home.join(".pi/agent").exists(),
            Self::Qoder => PATHS.home.join(".qoder").exists(),
            Self::QwenCode => PATHS.home.join(".qwen").exists(),
            Self::Replit => cwd.join(".replit").exists(),
            Self::Rovodev => PATHS.home.join(".rovodev").exists(),
            Self::Roo => PATHS.home.join(".roo").exists(),
            Self::TabnineCli => PATHS.home.join(".tabnine").exists(),
            Self::Trae => PATHS.home.join(".trae").exists(),
            Self::TraeCn => PATHS.home.join(".trae-cn").exists(),
            Self::Warp => PATHS.home.join(".warp").exists(),
            Self::Windsurf => PATHS.home.join(".codeium/windsurf").exists(),
            Self::Zencoder => PATHS.home.join(".zencoder").exists(),
            Self::Pochi => PATHS.home.join(".pochi").exists(),
            Self::Adal => PATHS.home.join(".adal").exists(),
            Self::Cortex => PATHS.home.join(".snowflake/cortex").exists(),
        }
    }

    /// 检测所有已安装的 Agent
    /// 对应 CLI: detectInstalledAgents (agents.ts:378-386)
    pub fn detect_installed() -> Vec<AgentType> {
        Self::all().filter(|agent| agent.is_installed()).collect()
    }

    /// 获取指定安装范围下的目标能力
    pub fn scope_target(&self, is_global: bool, cwd: &str) -> AgentScopeTarget {
        let config = self.config();
        let canonical = crate::core::paths::canonical_skills_dir(is_global, cwd);

        let path = if is_global {
            config.global_skills_dir.clone().unwrap_or_default()
        } else {
            std::path::PathBuf::from(cwd).join(config.skills_dir)
        };

        let supported = if is_global {
            config.global_skills_dir.is_some()
        } else {
            !config.skills_dir.trim().is_empty()
        };

        AgentScopeTarget {
            supported,
            automatic: supported && same_normalized_path(&path, &canonical),
            path: if is_global {
                path.to_string_lossy().to_string()
            } else {
                config.skills_dir.to_string()
            },
        }
    }

    /// 判断 Agent 在指定安装范围下是否自动读取共享目录
    pub fn is_automatic_for_scope(&self, is_global: bool, cwd: &str) -> bool {
        self.scope_target(is_global, cwd).automatic
    }

    /// 获取指定安装范围下自动读取共享目录的 Agent
    pub fn get_automatic_agents_for_scope(is_global: bool, cwd: &str) -> Vec<AgentType> {
        Self::all()
            .filter(|agent| agent.is_automatic_for_scope(is_global, cwd))
            .collect()
    }

    /// 转换为 AgentInfo（前端使用）
    pub fn to_agent_info(&self) -> AgentInfo {
        let config = self.config();

        AgentInfo {
            id: *self,
            name: config.display_name.to_string(),
            skills_dir: config.skills_dir.to_string(),
            global_skills_dir: config
                .global_skills_dir
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_default(),
            detected: self.is_installed(),
            targets: AgentTargets {
                global: self.scope_target(true, ""),
                project: self.scope_target(false, "."),
            },
        }
    }
}

fn same_normalized_path(left: &std::path::Path, right: &std::path::Path) -> bool {
    left.components().collect::<Vec<_>>() == right.components().collect::<Vec<_>>()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_agent_type_all_count() {
        let count = AgentType::all().count();
        assert_eq!(
            count, 54,
            "Should have 54 real agent types after removing the hidden shared-directory placeholder"
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
    fn test_project_automatic_agents_include_all_matching_targets() {
        let automatic_agents = AgentType::get_automatic_agents_for_scope(false, ".");

        assert!(
            automatic_agents.contains(&AgentType::Replit),
            "project automatic resolution should depend on the target directory only"
        );
    }

    #[test]
    fn test_agent_info_fields() {
        let info = AgentType::Replit.to_agent_info();
        assert!(info.targets.project.automatic);

        let info = AgentType::Warp.to_agent_info();
        assert!(info.targets.global.automatic);
        assert!(info.targets.project.automatic);
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
        assert!(!AgentType::Cortex.to_agent_info().targets.project.automatic);
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
    fn test_antigravity_uses_shared_project_dir() {
        let config = AgentType::Antigravity.config();
        assert_eq!(config.skills_dir, ".agents/skills");
    }

    #[test]
    fn test_antigravity_is_project_automatic_but_global_additional() {
        let info = AgentType::Antigravity.to_agent_info();

        assert!(info.targets.project.supported);
        assert!(info.targets.project.automatic);
        assert_eq!(info.targets.project.path, ".agents/skills");

        assert!(info.targets.global.supported);
        assert!(!info.targets.global.automatic);
        assert!(info.targets.global.path.contains(".gemini"));
        assert!(info.targets.global.path.contains("antigravity"));
    }

    #[test]
    fn test_warp_is_automatic_for_both_scopes() {
        let info = AgentType::Warp.to_agent_info();

        assert!(info.targets.project.automatic);
        assert!(info.targets.global.automatic);
    }

    #[test]
    fn test_claude_code_is_additional_for_both_scopes() {
        let info = AgentType::ClaudeCode.to_agent_info();

        assert!(info.targets.project.supported);
        assert!(!info.targets.project.automatic);
        assert!(info.targets.global.supported);
        assert!(!info.targets.global.automatic);
    }
}

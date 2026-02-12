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
    pub name: &'static str,
    pub display_name: &'static str,
    pub skills_dir: &'static str,
    pub global_skills_dir: Option<PathBuf>,
    /// 是否在 Universal 列表显示（默认 true）
    /// 对应 CLI: showInUniversalList
    pub show_in_universal_list: bool,
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
    /// 是否是 Universal Agent（安装逻辑用）
    /// 对应 CLI: isUniversalAgent()
    pub is_universal: bool,
    /// 是否在 Universal 列表显示（UI 显示用）
    /// 对应 CLI: getUniversalAgents() 的过滤条件
    pub show_in_universal_list: bool,
}

/// Agent 类型枚举
/// 完整对应 CLI: types.ts AgentType
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "kebab-case")]
#[specta(rename_all = "kebab-case")]
pub enum AgentType {
    Amp,
    Antigravity,
    Augment,
    ClaudeCode,
    Openclaw,
    Cline,
    Codebuddy,
    Codex,
    CommandCode,
    Continue,
    Crush,
    Cursor,
    Droid,
    GeminiCli,
    GithubCopilot,
    Goose,
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
    Roo,
    Trae,
    TraeCn,
    Windsurf,
    Zencoder,
    Pochi,
    Adal,
}

impl std::fmt::Display for AgentType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = match self {
            Self::Amp => "amp",
            Self::Antigravity => "antigravity",
            Self::Augment => "augment",
            Self::ClaudeCode => "claude-code",
            Self::Openclaw => "openclaw",
            Self::Cline => "cline",
            Self::Codebuddy => "codebuddy",
            Self::Codex => "codex",
            Self::CommandCode => "command-code",
            Self::Continue => "continue",
            Self::Crush => "crush",
            Self::Cursor => "cursor",
            Self::Droid => "droid",
            Self::GeminiCli => "gemini-cli",
            Self::GithubCopilot => "github-copilot",
            Self::Goose => "goose",
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
            Self::Roo => "roo",
            Self::Trae => "trae",
            Self::TraeCn => "trae-cn",
            Self::Windsurf => "windsurf",
            Self::Zencoder => "zencoder",
            Self::Pochi => "pochi",
            Self::Adal => "adal",
        };
        write!(f, "{}", name)
    }
}

impl std::str::FromStr for AgentType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "amp" => Ok(Self::Amp),
            "antigravity" => Ok(Self::Antigravity),
            "augment" => Ok(Self::Augment),
            "claude-code" => Ok(Self::ClaudeCode),
            "openclaw" => Ok(Self::Openclaw),
            "cline" => Ok(Self::Cline),
            "codebuddy" => Ok(Self::Codebuddy),
            "codex" => Ok(Self::Codex),
            "command-code" => Ok(Self::CommandCode),
            "continue" => Ok(Self::Continue),
            "crush" => Ok(Self::Crush),
            "cursor" => Ok(Self::Cursor),
            "droid" => Ok(Self::Droid),
            "gemini-cli" => Ok(Self::GeminiCli),
            "github-copilot" => Ok(Self::GithubCopilot),
            "goose" => Ok(Self::Goose),
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
            "roo" => Ok(Self::Roo),
            "trae" => Ok(Self::Trae),
            "trae-cn" => Ok(Self::TraeCn),
            "windsurf" => Ok(Self::Windsurf),
            "zencoder" => Ok(Self::Zencoder),
            "pochi" => Ok(Self::Pochi),
            "adal" => Ok(Self::Adal),
            _ => Err(format!("Unknown agent type: {}", s)),
        }
    }
}

impl AgentType {
    /// 返回所有 Agent 类型的迭代器
    pub fn all() -> impl Iterator<Item = AgentType> {
        [
            Self::Amp,
            Self::Antigravity,
            Self::Augment,
            Self::ClaudeCode,
            Self::Openclaw,
            Self::Cline,
            Self::Codebuddy,
            Self::Codex,
            Self::CommandCode,
            Self::Continue,
            Self::Crush,
            Self::Cursor,
            Self::Droid,
            Self::GeminiCli,
            Self::GithubCopilot,
            Self::Goose,
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
            Self::Roo,
            Self::Trae,
            Self::TraeCn,
            Self::Windsurf,
            Self::Zencoder,
            Self::Pochi,
            Self::Adal,
        ]
        .into_iter()
    }

    /// 获取 Agent 配置
    /// 完整对应 CLI: agents.ts 中每个 agent 的配置
    pub fn config(&self) -> AgentConfig {
        match self {
            Self::Amp => AgentConfig {
                name: "amp",
                display_name: "Amp",
                skills_dir: ".agents/skills",
                global_skills_dir: Some(PATHS.config_home.join("agents/skills")),
                show_in_universal_list: true,
            },
            Self::Antigravity => AgentConfig {
                name: "antigravity",
                display_name: "Antigravity",
                skills_dir: ".agent/skills",
                global_skills_dir: Some(PATHS.home.join(".gemini/antigravity/skills")),
                show_in_universal_list: true,
            },
            Self::Augment => AgentConfig {
                name: "augment",
                display_name: "Augment",
                skills_dir: ".augment/skills",
                global_skills_dir: Some(PATHS.home.join(".augment/skills")),
                show_in_universal_list: true,
            },
            Self::ClaudeCode => AgentConfig {
                name: "claude-code",
                display_name: "Claude Code",
                skills_dir: ".claude/skills",
                global_skills_dir: Some(PATHS.claude_home.join("skills")),
                show_in_universal_list: true,
            },
            Self::Openclaw => AgentConfig {
                name: "openclaw",
                display_name: "OpenClaw",
                skills_dir: "skills",
                global_skills_dir: Some(Self::openclaw_global_dir()),
                show_in_universal_list: true,
            },
            Self::Cline => AgentConfig {
                name: "cline",
                display_name: "Cline",
                skills_dir: ".cline/skills",
                global_skills_dir: Some(PATHS.home.join(".cline/skills")),
                show_in_universal_list: true,
            },
            Self::Codebuddy => AgentConfig {
                name: "codebuddy",
                display_name: "CodeBuddy",
                skills_dir: ".codebuddy/skills",
                global_skills_dir: Some(PATHS.home.join(".codebuddy/skills")),
                show_in_universal_list: true,
            },
            Self::Codex => AgentConfig {
                name: "codex",
                display_name: "Codex",
                skills_dir: ".agents/skills",
                global_skills_dir: Some(PATHS.codex_home.join("skills")),
                show_in_universal_list: true,
            },
            Self::CommandCode => AgentConfig {
                name: "command-code",
                display_name: "Command Code",
                skills_dir: ".commandcode/skills",
                global_skills_dir: Some(PATHS.home.join(".commandcode/skills")),
                show_in_universal_list: true,
            },
            Self::Continue => AgentConfig {
                name: "continue",
                display_name: "Continue",
                skills_dir: ".continue/skills",
                global_skills_dir: Some(PATHS.home.join(".continue/skills")),
                show_in_universal_list: true,
            },
            Self::Crush => AgentConfig {
                name: "crush",
                display_name: "Crush",
                skills_dir: ".crush/skills",
                global_skills_dir: Some(PATHS.config_home.join("crush/skills")),
                show_in_universal_list: true,
            },
            Self::Cursor => AgentConfig {
                name: "cursor",
                display_name: "Cursor",
                skills_dir: ".cursor/skills",
                global_skills_dir: Some(PATHS.home.join(".cursor/skills")),
                show_in_universal_list: true,
            },
            Self::Droid => AgentConfig {
                name: "droid",
                display_name: "Droid",
                skills_dir: ".factory/skills",
                global_skills_dir: Some(PATHS.home.join(".factory/skills")),
                show_in_universal_list: true,
            },
            Self::GeminiCli => AgentConfig {
                name: "gemini-cli",
                display_name: "Gemini CLI",
                skills_dir: ".agents/skills",
                global_skills_dir: Some(PATHS.home.join(".gemini/skills")),
                show_in_universal_list: true,
            },
            Self::GithubCopilot => AgentConfig {
                name: "github-copilot",
                display_name: "GitHub Copilot",
                skills_dir: ".agents/skills",
                global_skills_dir: Some(PATHS.home.join(".copilot/skills")),
                show_in_universal_list: true,
            },
            Self::Goose => AgentConfig {
                name: "goose",
                display_name: "Goose",
                skills_dir: ".goose/skills",
                global_skills_dir: Some(PATHS.config_home.join("goose/skills")),
                show_in_universal_list: true,
            },
            Self::IflowCli => AgentConfig {
                name: "iflow-cli",
                display_name: "iFlow CLI",
                skills_dir: ".iflow/skills",
                global_skills_dir: Some(PATHS.home.join(".iflow/skills")),
                show_in_universal_list: true,
            },
            Self::Junie => AgentConfig {
                name: "junie",
                display_name: "Junie",
                skills_dir: ".junie/skills",
                global_skills_dir: Some(PATHS.home.join(".junie/skills")),
                show_in_universal_list: true,
            },
            Self::Kilo => AgentConfig {
                name: "kilo",
                display_name: "Kilo Code",
                skills_dir: ".kilocode/skills",
                global_skills_dir: Some(PATHS.home.join(".kilocode/skills")),
                show_in_universal_list: true,
            },
            Self::KimiCli => AgentConfig {
                name: "kimi-cli",
                display_name: "Kimi Code CLI",
                skills_dir: ".agents/skills",
                global_skills_dir: Some(PATHS.config_home.join("agents/skills")),
                show_in_universal_list: true,
            },
            Self::KiroCli => AgentConfig {
                name: "kiro-cli",
                display_name: "Kiro CLI",
                skills_dir: ".kiro/skills",
                global_skills_dir: Some(PATHS.home.join(".kiro/skills")),
                show_in_universal_list: true,
            },
            Self::Kode => AgentConfig {
                name: "kode",
                display_name: "Kode",
                skills_dir: ".kode/skills",
                global_skills_dir: Some(PATHS.home.join(".kode/skills")),
                show_in_universal_list: true,
            },
            Self::Mcpjam => AgentConfig {
                name: "mcpjam",
                display_name: "MCPJam",
                skills_dir: ".mcpjam/skills",
                global_skills_dir: Some(PATHS.home.join(".mcpjam/skills")),
                show_in_universal_list: true,
            },
            Self::MistralVibe => AgentConfig {
                name: "mistral-vibe",
                display_name: "Mistral Vibe",
                skills_dir: ".vibe/skills",
                global_skills_dir: Some(PATHS.home.join(".vibe/skills")),
                show_in_universal_list: true,
            },
            Self::Mux => AgentConfig {
                name: "mux",
                display_name: "Mux",
                skills_dir: ".mux/skills",
                global_skills_dir: Some(PATHS.home.join(".mux/skills")),
                show_in_universal_list: true,
            },
            Self::Neovate => AgentConfig {
                name: "neovate",
                display_name: "Neovate",
                skills_dir: ".neovate/skills",
                global_skills_dir: Some(PATHS.home.join(".neovate/skills")),
                show_in_universal_list: true,
            },
            Self::Opencode => AgentConfig {
                name: "opencode",
                display_name: "OpenCode",
                skills_dir: ".agents/skills",
                global_skills_dir: Some(PATHS.config_home.join("opencode/skills")),
                show_in_universal_list: true,
            },
            Self::Openhands => AgentConfig {
                name: "openhands",
                display_name: "OpenHands",
                skills_dir: ".openhands/skills",
                global_skills_dir: Some(PATHS.home.join(".openhands/skills")),
                show_in_universal_list: true,
            },
            Self::Pi => AgentConfig {
                name: "pi",
                display_name: "Pi",
                skills_dir: ".pi/skills",
                global_skills_dir: Some(PATHS.home.join(".pi/agent/skills")),
                show_in_universal_list: true,
            },
            Self::Qoder => AgentConfig {
                name: "qoder",
                display_name: "Qoder",
                skills_dir: ".qoder/skills",
                global_skills_dir: Some(PATHS.home.join(".qoder/skills")),
                show_in_universal_list: true,
            },
            Self::QwenCode => AgentConfig {
                name: "qwen-code",
                display_name: "Qwen Code",
                skills_dir: ".qwen/skills",
                global_skills_dir: Some(PATHS.home.join(".qwen/skills")),
                show_in_universal_list: true,
            },
            // Replit: 使用 .agents/skills 但不显示在 Universal 列表
            // 原因：Replit 是云端 IDE，不支持全局安装，且检测条件容易误判
            Self::Replit => AgentConfig {
                name: "replit",
                display_name: "Replit",
                skills_dir: ".agents/skills",
                global_skills_dir: Some(PATHS.config_home.join("agents/skills")),
                show_in_universal_list: false, // 关键：不显示在 Universal 列表
            },
            Self::Roo => AgentConfig {
                name: "roo",
                display_name: "Roo Code",
                skills_dir: ".roo/skills",
                global_skills_dir: Some(PATHS.home.join(".roo/skills")),
                show_in_universal_list: true,
            },
            Self::Trae => AgentConfig {
                name: "trae",
                display_name: "Trae",
                skills_dir: ".trae/skills",
                global_skills_dir: Some(PATHS.home.join(".trae/skills")),
                show_in_universal_list: true,
            },
            Self::TraeCn => AgentConfig {
                name: "trae-cn",
                display_name: "Trae CN",
                skills_dir: ".trae/skills",
                global_skills_dir: Some(PATHS.home.join(".trae-cn/skills")),
                show_in_universal_list: true,
            },
            Self::Windsurf => AgentConfig {
                name: "windsurf",
                display_name: "Windsurf",
                skills_dir: ".windsurf/skills",
                global_skills_dir: Some(PATHS.home.join(".codeium/windsurf/skills")),
                show_in_universal_list: true,
            },
            Self::Zencoder => AgentConfig {
                name: "zencoder",
                display_name: "Zencoder",
                skills_dir: ".zencoder/skills",
                global_skills_dir: Some(PATHS.home.join(".zencoder/skills")),
                show_in_universal_list: true,
            },
            Self::Pochi => AgentConfig {
                name: "pochi",
                display_name: "Pochi",
                skills_dir: ".pochi/skills",
                global_skills_dir: Some(PATHS.home.join(".pochi/skills")),
                show_in_universal_list: true,
            },
            Self::Adal => AgentConfig {
                name: "adal",
                display_name: "AdaL",
                skills_dir: ".adal/skills",
                global_skills_dir: Some(PATHS.home.join(".adal/skills")),
                show_in_universal_list: true,
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
        } else {
            PATHS.home.join(".moltbot/skills")
        }
    }

    /// 检测 Agent 是否已安装
    /// 完整对应 CLI: 每个 agent 的 detectInstalled 函数
    pub fn is_installed(&self) -> bool {
        let cwd = std::env::current_dir().unwrap_or_default();

        match self {
            Self::Amp => PATHS.config_home.join("amp").exists(),
            Self::Antigravity => {
                cwd.join(".agent").exists() || PATHS.home.join(".gemini/antigravity").exists()
            }
            Self::Augment => PATHS.home.join(".augment").exists(),
            Self::ClaudeCode => PATHS.claude_home.exists(),
            Self::Openclaw => {
                PATHS.home.join(".openclaw").exists()
                    || PATHS.home.join(".clawdbot").exists()
                    || PATHS.home.join(".moltbot").exists()
            }
            Self::Cline => PATHS.home.join(".cline").exists(),
            Self::Codebuddy => {
                cwd.join(".codebuddy").exists() || PATHS.home.join(".codebuddy").exists()
            }
            Self::Codex => {
                PATHS.codex_home.exists() || std::path::Path::new("/etc/codex").exists()
            }
            Self::CommandCode => PATHS.home.join(".commandcode").exists(),
            Self::Continue => cwd.join(".continue").exists() || PATHS.home.join(".continue").exists(),
            Self::Crush => PATHS.config_home.join("crush").exists(),
            Self::Cursor => PATHS.home.join(".cursor").exists(),
            Self::Droid => PATHS.home.join(".factory").exists(),
            Self::GeminiCli => PATHS.home.join(".gemini").exists(),
            Self::GithubCopilot => cwd.join(".github").exists() || PATHS.home.join(".copilot").exists(),
            Self::Goose => PATHS.config_home.join("goose").exists(),
            Self::IflowCli => PATHS.home.join(".iflow").exists(),
            Self::Junie => PATHS.home.join(".junie").exists(),
            Self::Kilo => PATHS.home.join(".kilocode").exists(),
            Self::KimiCli => PATHS.home.join(".kimi").exists(),
            Self::KiroCli => PATHS.home.join(".kiro").exists(),
            Self::Kode => PATHS.home.join(".kode").exists(),
            Self::Mcpjam => PATHS.home.join(".mcpjam").exists(),
            Self::MistralVibe => PATHS.home.join(".vibe").exists(),
            Self::Mux => PATHS.home.join(".mux").exists(),
            Self::Neovate => PATHS.home.join(".neovate").exists(),
            Self::Opencode => {
                PATHS.config_home.join("opencode").exists()
                    || PATHS.claude_home.join("skills").exists()
            }
            Self::Openhands => PATHS.home.join(".openhands").exists(),
            Self::Pi => PATHS.home.join(".pi/agent").exists(),
            Self::Qoder => PATHS.home.join(".qoder").exists(),
            Self::QwenCode => PATHS.home.join(".qwen").exists(),
            Self::Replit => cwd.join(".agents").exists(),
            Self::Roo => PATHS.home.join(".roo").exists(),
            Self::Trae => PATHS.home.join(".trae").exists(),
            Self::TraeCn => PATHS.home.join(".trae-cn").exists(),
            Self::Windsurf => PATHS.home.join(".codeium/windsurf").exists(),
            Self::Zencoder => PATHS.home.join(".zencoder").exists(),
            Self::Pochi => PATHS.home.join(".pochi").exists(),
            Self::Adal => PATHS.home.join(".adal").exists(),
        }
    }

    /// 检测所有已安装的 Agent
    /// 对应 CLI: detectInstalledAgents (agents.ts:378-386)
    pub fn detect_installed() -> Vec<AgentType> {
        Self::all().filter(|agent| agent.is_installed()).collect()
    }

    /// 检查是否是 Universal Agent（使用 .agents/skills 目录）
    /// 对应 CLI: isUniversalAgent (agents.ts:418-420)
    /// 用于安装逻辑判断是否跳过 symlink
    pub fn is_universal(&self) -> bool {
        self.config().skills_dir == ".agents/skills"
    }

    /// 获取 Universal Agents（用于 UI 显示）
    /// 对应 CLI: getUniversalAgents (agents.ts:397-403)
    pub fn get_universal_agents() -> Vec<AgentType> {
        Self::all()
            .filter(|agent| {
                let config = agent.config();
                config.skills_dir == ".agents/skills" && config.show_in_universal_list
            })
            .collect()
    }

    /// 获取 Non-Universal Agents
    /// 对应 CLI: getNonUniversalAgents (agents.ts:409-413)
    pub fn get_non_universal_agents() -> Vec<AgentType> {
        Self::all()
            .filter(|agent| agent.config().skills_dir != ".agents/skills")
            .collect()
    }

    /// 转换为 AgentInfo（前端使用）
    pub fn to_agent_info(&self) -> AgentInfo {
        let config = self.config();
        let is_universal = config.skills_dir == ".agents/skills";

        AgentInfo {
            id: *self,
            name: config.display_name.to_string(),
            skills_dir: config.skills_dir.to_string(),
            global_skills_dir: config
                .global_skills_dir
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_default(),
            detected: self.is_installed(),
            is_universal,
            show_in_universal_list: is_universal && config.show_in_universal_list,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_agent_type_all_count() {
        let count = AgentType::all().count();
        assert_eq!(count, 39, "Should have 39 agent types");
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
    fn test_universal_agents() {
        assert!(AgentType::Amp.is_universal());
        assert!(AgentType::Codex.is_universal());
        assert!(AgentType::GeminiCli.is_universal());
        assert!(AgentType::GithubCopilot.is_universal());
        assert!(AgentType::Replit.is_universal()); // Replit 技术上是 Universal
        assert!(!AgentType::Cursor.is_universal());
        assert!(!AgentType::ClaudeCode.is_universal());
    }

    #[test]
    fn test_replit_not_in_universal_list() {
        let universal_agents = AgentType::get_universal_agents();
        // Replit 不应该出现在 Universal 列表中
        assert!(
            !universal_agents.contains(&AgentType::Replit),
            "Replit should not be in universal list"
        );
        // 但其他 Universal agents 应该在
        assert!(
            universal_agents.contains(&AgentType::Amp),
            "Amp should be in universal list"
        );
    }

    #[test]
    fn test_agent_info_fields() {
        let info = AgentType::Replit.to_agent_info();
        assert!(info.is_universal, "Replit should be universal");
        assert!(
            !info.show_in_universal_list,
            "Replit should not show in universal list"
        );

        let info = AgentType::Amp.to_agent_info();
        assert!(info.is_universal, "Amp should be universal");
        assert!(
            info.show_in_universal_list,
            "Amp should show in universal list"
        );
    }

    #[test]
    fn test_detect_installed_returns_vec() {
        let installed = AgentType::detect_installed();
        assert!(installed.len() <= 39);
    }
}

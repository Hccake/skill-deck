//! 删除详情查询命令
//!
//! 为智能删除对话框提供 agent 安装详情

use crate::core::agent_availability::detect_agent_presence;
use crate::core::agents::AgentType;
use crate::core::paths::canonical_skills_dir;
use crate::core::skill::sanitize_name;
use crate::error::AppError;
use crate::models::{AgentSkillPresence, IndependentAgentInfo, Scope, SkillAgentDetails};
use std::path::PathBuf;

fn independent_agent_info(
    agent: AgentType,
    display_name: &str,
    private_path: &str,
) -> IndependentAgentInfo {
    let skill_path = PathBuf::from(private_path);
    let is_symlink = skill_path
        .symlink_metadata()
        .map(|m| m.file_type().is_symlink())
        .unwrap_or(false);

    #[cfg(windows)]
    let is_symlink = is_symlink
        || skill_path
            .symlink_metadata()
            .map(|m| {
                // Junction 在 Windows 上表现为 dir + reparse point
                m.file_type().is_dir()
                    && std::os::windows::fs::MetadataExt::file_attributes(&m) & 0x400 != 0
            })
            .unwrap_or(false);

    IndependentAgentInfo {
        agent,
        display_name: display_name.to_string(),
        path: private_path.to_string(),
        is_symlink,
    }
}

/// 查询 skill 的 agent 安装详情
///
/// 对话框挂载时调用，返回自动应用/独立安装分组信息
#[tauri::command]
#[specta::specta]
pub async fn get_skill_agent_details(
    scope: Scope,
    name: String,
    project_path: Option<String>,
) -> Result<SkillAgentDetails, AppError> {
    let is_global = matches!(scope, Scope::Global);
    let cwd = project_path.as_deref().unwrap_or(".");
    let sanitized_name = sanitize_name(&name);

    // 1. 计算 canonical 路径
    let canonical_path = canonical_skills_dir(is_global, cwd).join(&sanitized_name);

    // 3. 遍历 agents，按 presence 模型分组；旧 automatic/independent 字段保留兼容。
    let mut automatic_agents: Vec<(AgentType, String)> = Vec::new();
    let mut independent_agents: Vec<IndependentAgentInfo> = Vec::new();
    let mut default_available_agents = Vec::new();
    let mut private_required_agents = Vec::new();
    let mut duplicate_copy_agents = Vec::new();
    let mut private_only_agents = Vec::new();

    for agent in AgentType::all() {
        let config = agent.config();
        let presence = detect_agent_presence(agent, &name, is_global, cwd);

        match presence.presence {
            AgentSkillPresence::DefaultActive => {
                automatic_agents.push((agent, config.display_name.to_string()));
                default_available_agents.push(presence);
            }
            AgentSkillPresence::DuplicateCopy => {
                automatic_agents.push((agent, config.display_name.to_string()));
                if let Some(private_path) = &presence.private_path {
                    independent_agents.push(independent_agent_info(
                        agent,
                        config.display_name,
                        private_path,
                    ));
                }
                default_available_agents.push(presence.clone());
                duplicate_copy_agents.push(presence);
            }
            AgentSkillPresence::PrivateOnly => {
                if let Some(private_path) = &presence.private_path {
                    independent_agents.push(independent_agent_info(
                        agent,
                        config.display_name,
                        private_path,
                    ));
                }
                private_only_agents.push(presence);
            }
            AgentSkillPresence::RequiresPrivateInstall => {
                private_required_agents.push(presence);
            }
            AgentSkillPresence::NotInstalled => {}
        }
    }

    Ok(SkillAgentDetails {
        skill_name: name,
        scope,
        canonical_path: canonical_path.to_string_lossy().to_string(),
        automatic_agents,
        independent_agents,
        default_available_agents,
        private_required_agents,
        duplicate_copy_agents,
        private_only_agents,
    })
}

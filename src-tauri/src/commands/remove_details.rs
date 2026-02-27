//! 删除详情查询命令
//!
//! 为智能删除对话框提供 agent 安装详情

use crate::core::agents::AgentType;
use crate::core::paths::canonical_skills_dir;
use crate::core::skill::sanitize_name;
use crate::error::AppError;
use crate::models::{IndependentAgentInfo, Scope, SkillAgentDetails};
use std::path::PathBuf;

/// 查询 skill 的 agent 安装详情
///
/// 对话框挂载时调用，返回 universal/non-universal 分组信息
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

    // 2. 获取所有已安装的 agents（与当前 remove_skill 一致的逻辑）
    let detected_agents = {
        let detected = AgentType::detect_installed();
        if detected.is_empty() {
            AgentType::all().collect::<Vec<_>>()
        } else {
            detected
        }
    };

    // 3. 遍历 agents，分组为 universal / independent
    let mut universal_agents: Vec<(AgentType, String)> = Vec::new();
    let mut independent_agents: Vec<IndependentAgentInfo> = Vec::new();

    for agent in &detected_agents {
        let config = agent.config();

        // 计算该 agent 的 skill 路径
        let skill_path = if is_global {
            match &config.global_skills_dir {
                Some(global_dir) => global_dir.join(&sanitized_name),
                None => continue, // agent 不支持 global
            }
        } else {
            PathBuf::from(cwd)
                .join(&config.skills_dir)
                .join(&sanitized_name)
        };

        // 检查路径是否存在（包括 symlink）
        let exists = skill_path.symlink_metadata().is_ok();
        if !exists {
            continue;
        }

        if agent.is_universal() {
            // 仅展示 show_in_universal_list 的 agent，与安装向导保持一致
            // Replit/Universal 等隐藏的 universal agent 不参与展示
            if config.show_in_universal_list {
                universal_agents.push((*agent, config.display_name.to_string()));
            }
            // universal agent 不能放入 independent（删除其目录 = 删除 canonical）
            continue;
        } else {
            // 检查是否是 symlink
            let is_symlink = skill_path
                .symlink_metadata()
                .map(|m| m.file_type().is_symlink())
                .unwrap_or(false);

            // Windows junction 也需要检查
            #[cfg(windows)]
            let is_symlink = is_symlink || {
                skill_path
                    .symlink_metadata()
                    .map(|m| {
                        // Junction 在 Windows 上表现为 dir + reparse point
                        m.file_type().is_dir()
                            && std::os::windows::fs::MetadataExt::file_attributes(&m) & 0x400 != 0
                    })
                    .unwrap_or(false)
            };

            independent_agents.push(IndependentAgentInfo {
                agent: *agent,
                display_name: config.display_name.to_string(),
                path: skill_path.to_string_lossy().to_string(),
                is_symlink,
            });
        }
    }

    Ok(SkillAgentDetails {
        skill_name: name,
        scope,
        canonical_path: canonical_path.to_string_lossy().to_string(),
        universal_agents,
        independent_agents,
    })
}

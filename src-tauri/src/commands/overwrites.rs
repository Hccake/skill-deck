//! 覆盖检测命令

use crate::core::agents::AgentType;
use crate::core::installer::{is_private_copy_installed, is_skill_installed};
use crate::error::AppError;
use crate::models::Scope;
use std::collections::HashMap;

/// 检测哪些 skill × agent 组合会被覆盖
///
/// # Arguments
/// * `skills` - 要安装的 skill 名称列表
/// * `agents` - 目标 agent 列表
/// * `scope` - 安装范围
/// * `project_path` - Project scope 时的项目路径
///
/// # Returns
/// * `HashMap<String, Vec<String>>` - { skill_name: [agent_ids that will be overwritten] }
#[tauri::command]
#[specta::specta]
pub async fn check_overwrites(
    skills: Vec<String>,
    agents: Vec<String>,
    private_copy_agents: Vec<String>,
    scope: Scope,
    project_path: Option<String>,
) -> Result<HashMap<String, Vec<String>>, AppError> {
    check_overwrites_inner(skills, agents, private_copy_agents, scope, project_path)
}

fn check_overwrites_inner(
    skills: Vec<String>,
    agents: Vec<String>,
    private_copy_agents: Vec<String>,
    scope: Scope,
    project_path: Option<String>,
) -> Result<HashMap<String, Vec<String>>, AppError> {
    let mut overwrites: HashMap<String, Vec<String>> = HashMap::new();

    for skill_name in &skills {
        let mut overwritten_agents = Vec::new();

        for agent_str in &agents {
            let agent: AgentType = agent_str.parse().map_err(|_| AppError::InvalidAgent {
                agent: agent_str.clone(),
            })?;

            let is_installed =
                is_skill_installed(skill_name, &agent, &scope, project_path.as_deref());

            if is_installed {
                overwritten_agents.push(agent_str.clone());
            }
        }

        for agent_str in &private_copy_agents {
            let agent: AgentType = agent_str.parse().map_err(|_| AppError::InvalidAgent {
                agent: agent_str.clone(),
            })?;

            let is_installed =
                is_private_copy_installed(skill_name, &agent, &scope, project_path.as_deref());

            if is_installed && !overwritten_agents.contains(agent_str) {
                overwritten_agents.push(agent_str.clone());
            }
        }

        if !overwritten_agents.is_empty() {
            overwrites.insert(skill_name.clone(), overwritten_agents);
        }
    }

    Ok(overwrites)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_check_overwrites_detects_private_copy_path() {
        let temp = tempdir().unwrap();
        let project_path = temp.path().to_string_lossy().to_string();
        let private_skill = temp.path().join(".claude").join("skills").join("demo");
        fs::create_dir_all(&private_skill).unwrap();
        fs::write(private_skill.join("SKILL.md"), "---\nname: demo\n---\n").unwrap();

        let result = check_overwrites_inner(
            vec!["demo".to_string()],
            Vec::new(),
            vec!["claude-code".to_string()],
            Scope::Project,
            Some(project_path),
        )
        .unwrap();

        assert_eq!(result.get("demo"), Some(&vec!["claude-code".to_string()]));
    }
}

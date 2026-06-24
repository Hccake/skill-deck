// list_agents command
// 对应 CLI: detectInstalledAgents + getAgentConfig

use crate::core::agents::{AgentInfo, AgentType};
use crate::error::AppError;
use crate::models::InstallTargetInfo;

/// 列出所有 Agents（包括未安装的）
/// 返回完整信息供前端使用，前端无需额外计算
/// 对应前端调用: invoke('list_agents')
#[tauri::command]
#[specta::specta]
pub fn list_agents() -> Result<Vec<AgentInfo>, AppError> {
    let agents: Vec<AgentInfo> = AgentType::all()
        .map(|agent| agent.to_agent_info())
        .collect();

    Ok(agents)
}

/// 按指定项目路径列出 Agents，供 project-only Agent 使用真实项目上下文检测。
#[tauri::command]
#[specta::specta]
pub fn list_agents_for_project(project_path: Option<String>) -> Result<Vec<AgentInfo>, AppError> {
    let cwd = project_path.unwrap_or_else(|| ".".to_string());
    let agents: Vec<AgentInfo> = AgentType::all()
        .map(|agent| agent.to_agent_info_for_project(&cwd))
        .collect();

    Ok(agents)
}

/// 列出指定项目内 Eve 可安装的具体目标：root agent 与已存在 subagents。
#[tauri::command]
#[specta::specta]
pub fn list_eve_install_targets(project_path: String) -> Result<Vec<InstallTargetInfo>, AppError> {
    Ok(crate::core::eve::eve_install_targets_for_project(
        &project_path,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_agents_for_project_detects_eve_from_supplied_project_path() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join("agent")).unwrap();
        std::fs::write(
            temp.path().join("package.json"),
            r#"{"dependencies":{"eve":"^0.11.5"}}"#,
        )
        .unwrap();

        let agents =
            list_agents_for_project(Some(temp.path().to_string_lossy().to_string())).unwrap();
        let eve = agents
            .iter()
            .find(|agent| agent.id == AgentType::Eve)
            .expect("Eve should be present in the agent registry");

        assert!(eve.detected);
        assert_eq!(eve.skills_dir, "agent/skills");
    }

    #[test]
    fn list_eve_install_targets_returns_root_and_subagents() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join("agent/subagents/research")).unwrap();
        std::fs::write(
            temp.path().join("package.json"),
            r#"{"dependencies":{"eve":"^0.11.5"}}"#,
        )
        .unwrap();

        let targets = list_eve_install_targets(temp.path().to_string_lossy().to_string()).unwrap();

        assert_eq!(targets.len(), 2);
        assert_eq!(targets[0].target_id, "eve:root");
        assert_eq!(targets[1].target_id, "eve:research");
    }
}

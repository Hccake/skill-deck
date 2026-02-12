// list_agents command
// 对应 CLI: detectInstalledAgents + getAgentConfig

use crate::core::agents::{AgentInfo, AgentType};
use crate::error::AppError;

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

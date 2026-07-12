// list_agents command
// 对应 CLI: detectInstalledAgents + getAgentConfig

use crate::core::agents::{AgentInfo, AgentTargets, AgentType};
use crate::environment::agent_environment::{
    AgentEnvironmentContext, AgentEnvironmentResolver, AgentEnvironmentTarget,
};
use crate::environment::service::{EnvironmentService, InspectRequest, ResolvedContext};
use crate::environment::types::{ContextRef, ContextScope, EnvironmentRef, ResourceLocator};
use crate::environment::wsl::{EnvironmentRegistry, WslSession};
use crate::error::AppError;
use crate::models::InstallTargetInfo;
use tauri::State;

fn agent_info_from_environment_targets(
    agent: AgentType,
    global: AgentEnvironmentTarget,
    project: AgentEnvironmentTarget,
    detected: bool,
) -> AgentInfo {
    let config = agent.config();
    let global_skills_dir = if global.availability
        == crate::core::agent_availability::AgentAvailabilityKind::Unsupported
    {
        String::new()
    } else {
        global
            .private_path
            .clone()
            .unwrap_or_else(|| global.shared_path.clone())
    };
    AgentInfo {
        id: agent,
        name: config.display_name.to_string(),
        skills_dir: config.skills_dir.to_string(),
        global_skills_dir,
        detected,
        targets: AgentTargets {
            global: global.scope_target(true),
            project: project.scope_target(false),
        },
    }
}

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

#[tauri::command]
#[specta::specta]
pub async fn list_agents_for_project_v2(
    context: ContextRef,
    registry: State<'_, EnvironmentRegistry>,
) -> Result<Vec<AgentInfo>, AppError> {
    match &context.environment {
        EnvironmentRef::Host => {
            let project_path = match &context.scope {
                ContextScope::Global => None,
                ContextScope::Project { project_id } => Some(
                    crate::commands::environments::host_projects_store()?
                        .read()?
                        .into_iter()
                        .find(|project| &project.id == project_id)
                        .ok_or_else(|| AppError::PathNotFound {
                            path: project_id.clone(),
                        })?
                        .native_path,
                ),
            };
            list_agents_for_project(project_path)
        }
        EnvironmentRef::Wsl { distro_name } => {
            let session = registry.get(distro_name).ok_or_else(|| AppError::Custom {
                message: format!("WSL distro '{distro_name}' is not connected"),
            })?;
            list_wsl_agents_for_context(context, session).await
        }
    }
}

async fn list_wsl_agents_for_context(
    context: ContextRef,
    session: WslSession,
) -> Result<Vec<AgentInfo>, AppError> {
    let project = match &context.scope {
        ContextScope::Global => None,
        ContextScope::Project { project_id } => Some(
            crate::commands::environments::read_wsl_projects(&session)
                .await?
                .into_iter()
                .find(|project| &project.id == project_id)
                .ok_or_else(|| AppError::PathNotFound {
                    path: project_id.clone(),
                })?,
        ),
    };
    let project_path = project
        .as_ref()
        .map(|project| project.native_path.clone())
        .unwrap_or_else(|| session.home.clone());
    let environment = context.environment.clone();
    let skill_root = format!("{}/.agents/skills", project_path.trim_end_matches('/'));
    let snapshot = EnvironmentService::Wsl(session.clone())
        .inspect(&InspectRequest {
            context: ResolvedContext {
                context,
                project: project.clone(),
                home: ResourceLocator {
                    environment: environment.clone(),
                    native_path: session.home.clone(),
                },
                skill_root: ResourceLocator {
                    environment: environment.clone(),
                    native_path: skill_root,
                },
                lock: ResourceLocator {
                    environment,
                    native_path: String::new(),
                },
            },
        })
        .await?;
    let resolver = AgentEnvironmentResolver::new(AgentEnvironmentContext {
        home: session.home,
        config_home: session.config_home,
        env: session.environment,
    });
    Ok(AgentType::all()
        .map(|agent| {
            let global = resolver.target(agent, true, &project_path);
            let project = resolver.target(agent, false, &project_path);
            agent_info_from_environment_targets(
                agent,
                global,
                project,
                snapshot.detected_agents.contains(&agent),
            )
        })
        .collect())
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
    use std::collections::BTreeMap;

    use super::*;
    use crate::environment::agent_environment::{
        AgentEnvironmentContext, AgentEnvironmentResolver,
    };

    #[test]
    fn builds_agent_info_with_wsl_native_targets_and_detection() {
        let resolver = AgentEnvironmentResolver::new(AgentEnvironmentContext {
            home: "/home/alice".to_string(),
            config_home: "/home/alice/.config".to_string(),
            env: BTreeMap::new(),
        });
        let global = resolver.target(AgentType::Codex, true, "/work/app");
        let project = resolver.target(AgentType::Codex, false, "/work/app");

        let info = agent_info_from_environment_targets(AgentType::Codex, global, project, true);

        assert!(info.detected);
        assert_eq!(info.global_skills_dir, "/home/alice/.codex/skills");
        assert_eq!(
            info.targets.global.shared_path,
            "/home/alice/.agents/skills"
        );
        assert_eq!(info.targets.project.path, ".agents/skills");
    }

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

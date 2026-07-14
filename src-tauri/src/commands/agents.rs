// list_agents command
// 对应 CLI: detectInstalledAgents + getAgentConfig

use crate::core::agents::{AgentInfo, AgentTargets, AgentType};
use crate::environment::agent_environment::{
    AgentEnvironmentContext, AgentEnvironmentResolver, AgentEnvironmentTarget,
};
use crate::environment::context_resolver::ContextResolver;
use crate::environment::service::{EnvironmentService, InspectRequest, ResolvedContext};
use crate::environment::types::{ContextRef, EnvironmentRef};
#[cfg(test)]
use crate::environment::types::{ContextScope, ResourceLocator};
use crate::environment::wsl::{EnvironmentRegistry, WslSession};
use crate::environment::wsl_protocol::{decode_nul_records, run_wsl_script};
use crate::error::AppError;
use crate::models::InstallTargetInfo;
use tauri::State;
use tokio::time::Duration;

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

/// 按指定项目路径列出 Agents，供 project-only Agent 使用真实项目上下文检测。
pub fn list_agents_host(project_path: Option<String>) -> Result<Vec<AgentInfo>, AppError> {
    let cwd = project_path.unwrap_or_else(|| ".".to_string());
    let agents: Vec<AgentInfo> = AgentType::all()
        .map(|agent| agent.to_agent_info_for_project(&cwd))
        .collect();

    Ok(agents)
}

#[tauri::command]
#[specta::specta]
pub async fn list_agents(
    context: ContextRef,
    registry: State<'_, EnvironmentRegistry>,
) -> Result<Vec<AgentInfo>, AppError> {
    match &context.environment {
        EnvironmentRef::Host => {
            let resolved = ContextResolver::resolve_host(context)?;
            let project_path = resolved.project.map(|project| project.native_path);
            list_agents_host(project_path)
        }
        EnvironmentRef::Wsl { distro_name } => {
            let distro_name = distro_name.clone();
            let retry_context = context.clone();
            registry
                .with_session_retry(&distro_name, move |session| {
                    let context = retry_context.clone();
                    async move { list_wsl_agents_for_context(context, session).await }
                })
                .await
        }
    }
}

async fn list_wsl_agents_for_context(
    context: ContextRef,
    session: WslSession,
) -> Result<Vec<AgentInfo>, AppError> {
    let resolved = ContextResolver::resolve_wsl(context, &session).await?;
    let project_path = resolved
        .project
        .as_ref()
        .map(|project| project.native_path.clone())
        .unwrap_or_else(|| session.home.clone());
    let snapshot = EnvironmentService::Wsl(session.clone())
        .inspect(&InspectRequest { context: resolved })
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
pub async fn list_eve_install_targets(
    context: ContextRef,
    registry: State<'_, EnvironmentRegistry>,
) -> Result<Vec<InstallTargetInfo>, AppError> {
    match &context.environment {
        EnvironmentRef::Host => {
            let resolved = ContextResolver::resolve_host(context)?;
            list_host_eve_install_targets(&resolved)
        }
        EnvironmentRef::Wsl { distro_name } => {
            let distro_name = distro_name.clone();
            let retry_context = context.clone();
            registry
                .with_session_retry(&distro_name, move |session| {
                    let context = retry_context.clone();
                    async move {
                        let resolved = ContextResolver::resolve_wsl(context, &session).await?;
                        list_wsl_eve_install_targets(&resolved, &session).await
                    }
                })
                .await
        }
    }
}

fn list_host_eve_install_targets(
    context: &ResolvedContext,
) -> Result<Vec<InstallTargetInfo>, AppError> {
    let Some(project) = &context.project else {
        return Ok(Vec::new());
    };
    Ok(crate::core::eve::eve_install_targets_for_project(
        &project.native_path,
    ))
}

async fn list_wsl_eve_install_targets(
    context: &ResolvedContext,
    session: &WslSession,
) -> Result<Vec<InstallTargetInfo>, AppError> {
    let Some(project) = &context.project else {
        return Ok(Vec::new());
    };
    const SCRIPT: &str = r#"
project=$1
if [ ! -d "$project/agent" ] || [ ! -f "$project/package.json" ]; then
  printf '0\0'
  exit 0
fi
printf '1\0'
cat -- "$project/package.json"
printf '\0'
for dir in "$project/agent/subagents"/*; do
  [ -d "$dir" ] || continue
  printf '%s\0' "${dir##*/}"
done
"#;
    let output = run_wsl_script(
        session,
        SCRIPT,
        std::slice::from_ref(&project.native_path),
        Vec::new(),
        Duration::from_secs(10),
    )
    .await?;
    let records = decode_nul_records(&output);
    if records.first().map(String::as_str) != Some("1") {
        return Ok(Vec::new());
    }
    let package: serde_json::Value = records
        .get(1)
        .ok_or_else(|| AppError::Custom {
            message: "invalid WSL Eve project response".to_string(),
        })
        .and_then(|raw| Ok(serde_json::from_str(raw)?))?;
    let has_eve = ["dependencies", "devDependencies"]
        .into_iter()
        .any(|section| {
            package
                .get(section)
                .and_then(serde_json::Value::as_object)
                .is_some_and(|dependencies| dependencies.contains_key("eve"))
        });
    if !has_eve {
        return Ok(Vec::new());
    }

    let project_path = project.native_path.trim_end_matches('/');
    let mut targets = vec![InstallTargetInfo {
        target_id: crate::core::eve::eve_target_id(None),
        agent: AgentType::Eve,
        display_name: crate::core::eve::eve_target_label(None),
        subagent: None,
        path: format!("{project_path}/agent/skills"),
    }];
    let mut subagents = records.into_iter().skip(2).collect::<Vec<_>>();
    subagents.sort();
    targets.extend(subagents.into_iter().map(|subagent| {
        let path_name = crate::core::skill::sanitize_name(&subagent);
        InstallTargetInfo {
            target_id: crate::core::eve::eve_target_id(Some(&subagent)),
            agent: AgentType::Eve,
            display_name: crate::core::eve::eve_target_label(Some(&subagent)),
            subagent: Some(subagent),
            path: format!("{project_path}/agent/subagents/{path_name}/skills"),
        }
    }));
    Ok(targets)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::environment::agent_environment::{
        AgentEnvironmentContext, AgentEnvironmentResolver,
    };
    use crate::environment::types::ProjectBinding;

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

        let agents = list_agents_host(Some(temp.path().to_string_lossy().to_string())).unwrap();
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

        let project = ProjectBinding {
            id: "eve-app".to_string(),
            native_path: temp.path().to_string_lossy().to_string(),
            display_name: None,
            order: None,
            suppress_cross_storage_warning: false,
        };
        let resolved = ResolvedContext {
            context: ContextRef {
                environment: EnvironmentRef::Host,
                scope: ContextScope::Project {
                    project_id: project.id.clone(),
                },
            },
            project: Some(project),
            home: ResourceLocator {
                environment: EnvironmentRef::Host,
                native_path: temp.path().to_string_lossy().to_string(),
            },
            skill_root: ResourceLocator {
                environment: EnvironmentRef::Host,
                native_path: temp
                    .path()
                    .join(".agents/skills")
                    .to_string_lossy()
                    .to_string(),
            },
            lock: ResourceLocator {
                environment: EnvironmentRef::Host,
                native_path: temp
                    .path()
                    .join("skills-lock.json")
                    .to_string_lossy()
                    .to_string(),
            },
        };

        let targets = list_host_eve_install_targets(&resolved).unwrap();

        assert_eq!(targets.len(), 2);
        assert_eq!(targets[0].target_id, "eve:root");
        assert_eq!(targets[1].target_id, "eve:research");
    }

    #[test]
    fn list_eve_install_targets_returns_none_for_global_context() {
        let resolved = ResolvedContext {
            context: ContextRef {
                environment: EnvironmentRef::Host,
                scope: ContextScope::Global,
            },
            project: None,
            home: ResourceLocator {
                environment: EnvironmentRef::Host,
                native_path: "/home/alice".to_string(),
            },
            skill_root: ResourceLocator {
                environment: EnvironmentRef::Host,
                native_path: "/home/alice/.agents/skills".to_string(),
            },
            lock: ResourceLocator {
                environment: EnvironmentRef::Host,
                native_path: "/home/alice/.agents/.skill-lock.json".to_string(),
            },
        };

        assert!(list_host_eve_install_targets(&resolved).unwrap().is_empty());
    }
}

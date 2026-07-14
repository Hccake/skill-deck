//! 覆盖检测命令

use crate::core::agents::AgentType;
use crate::core::installer::{is_private_copy_installed, is_skill_installed};
use crate::core::skill::sanitize_name;
use crate::environment::context_resolver::ContextResolver;
use crate::environment::service::{EnvironmentService, EnvironmentSnapshot, InspectRequest};
use crate::environment::types::{ContextRef, ContextScope, EnvironmentRef};
use crate::environment::wsl::{EnvironmentRegistry, WslSession};
use crate::error::AppError;
use crate::models::{InstallTargetSpec, Scope};
use std::collections::HashMap;
use tauri::State;

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
    context: ContextRef,
    skills: Vec<String>,
    agents: Vec<String>,
    private_copy_agents: Vec<String>,
    agent_targets: Vec<InstallTargetSpec>,
    registry: State<'_, EnvironmentRegistry>,
) -> Result<HashMap<String, Vec<String>>, AppError> {
    match &context.environment {
        EnvironmentRef::Host => {
            let resolved = ContextResolver::resolve_host(context)?;
            let (scope, project_path) = match &resolved.context.scope {
                ContextScope::Global => (Scope::Global, None),
                ContextScope::Project { .. } => (
                    Scope::Project,
                    resolved.project.map(|project| project.native_path),
                ),
            };
            check_overwrites_inner(
                skills,
                agents,
                private_copy_agents,
                scope,
                project_path,
                agent_targets,
            )
        }
        EnvironmentRef::Wsl { distro_name } => {
            let distro_name = distro_name.clone();
            let retry_context = context.clone();
            registry
                .with_session_retry(&distro_name, move |session| {
                    let context = retry_context.clone();
                    let skills = skills.clone();
                    let agents = agents.clone();
                    let private_copy_agents = private_copy_agents.clone();
                    let agent_targets = agent_targets.clone();
                    async move {
                        let snapshot = inspect_wsl_context(context, session).await?;
                        check_overwrites_from_snapshot(
                            &snapshot,
                            &skills,
                            &agents,
                            &private_copy_agents,
                            &agent_targets,
                        )
                    }
                })
                .await
        }
    }
}

async fn inspect_wsl_context(
    context: ContextRef,
    session: WslSession,
) -> Result<EnvironmentSnapshot, AppError> {
    let resolved = ContextResolver::resolve_wsl(context, &session).await?;
    EnvironmentService::Wsl(session.clone())
        .inspect(&InspectRequest { context: resolved })
        .await
}

fn check_overwrites_from_snapshot(
    snapshot: &EnvironmentSnapshot,
    skills: &[String],
    agents: &[String],
    private_copy_agents: &[String],
    agent_targets: &[InstallTargetSpec],
) -> Result<HashMap<String, Vec<String>>, AppError> {
    let mut overwrites = HashMap::new();
    for skill_name in skills {
        let Some(skill) = snapshot
            .skills
            .iter()
            .find(|skill| &skill.name == skill_name)
        else {
            continue;
        };
        let mut overwritten_agents = Vec::new();
        for agent_str in agents {
            let agent: AgentType = agent_str.parse().map_err(|_| AppError::InvalidAgent {
                agent: agent_str.clone(),
            })?;
            let installed = if agent == AgentType::Eve {
                skill
                    .eve_targets
                    .iter()
                    .any(|target| target.target_id == "eve:root")
            } else {
                skill.agents.contains(&agent)
            };
            if installed {
                overwritten_agents.push(agent_str.clone());
            }
        }
        for agent_str in private_copy_agents {
            let agent: AgentType = agent_str.parse().map_err(|_| AppError::InvalidAgent {
                agent: agent_str.clone(),
            })?;
            if skill.private_copy_agents.contains(&agent) && !overwritten_agents.contains(agent_str)
            {
                overwritten_agents.push(agent_str.clone());
            }
        }
        for target in agent_targets {
            if target.agent != AgentType::Eve {
                continue;
            }
            let subagent = target
                .subagent
                .as_ref()
                .filter(|value| !value.is_empty() && *value != "root");
            let target_id = crate::core::eve::eve_target_id(subagent.map(String::as_str));
            if skill
                .eve_targets
                .iter()
                .any(|target| target.target_id == target_id)
                && !overwritten_agents.contains(&target_id)
            {
                overwritten_agents.push(target_id);
            }
        }
        if !overwritten_agents.is_empty() {
            overwrites.insert(skill_name.clone(), overwritten_agents);
        }
    }
    Ok(overwrites)
}

fn check_overwrites_inner(
    skills: Vec<String>,
    agents: Vec<String>,
    private_copy_agents: Vec<String>,
    scope: Scope,
    project_path: Option<String>,
    agent_targets: Vec<InstallTargetSpec>,
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

        if matches!(scope, Scope::Project) {
            let cwd = project_path.as_deref().unwrap_or(".");
            let sanitized = sanitize_name(skill_name);
            for target in &agent_targets {
                if target.agent != AgentType::Eve {
                    continue;
                }

                let subagent = target
                    .subagent
                    .as_ref()
                    .filter(|value| !value.is_empty() && *value != "root");
                let path =
                    crate::core::eve::eve_skills_dir_for_target(cwd, subagent.map(String::as_str))
                        .join(&sanitized);
                if path.exists() {
                    let target_id = crate::core::eve::eve_target_id(subagent.map(String::as_str));
                    if !overwritten_agents.contains(&target_id) {
                        overwritten_agents.push(target_id);
                    }
                }
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
    use crate::environment::service::{EnvironmentSnapshot, SkillEntrySnapshot};
    use crate::models::InstallTargetInfo;
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
            Vec::new(),
        )
        .unwrap();

        assert_eq!(result.get("demo"), Some(&vec!["claude-code".to_string()]));
    }

    #[test]
    fn test_check_overwrites_detects_eve_target_path() {
        let temp = tempdir().unwrap();
        let project_path = temp.path().to_string_lossy().to_string();
        let eve_skill = temp
            .path()
            .join("agent")
            .join("subagents")
            .join("research")
            .join("skills")
            .join("demo");
        fs::create_dir_all(&eve_skill).unwrap();
        fs::write(eve_skill.join("SKILL.md"), "---\nname: demo\n---\n").unwrap();

        let result = check_overwrites_inner(
            vec!["demo".to_string()],
            Vec::new(),
            Vec::new(),
            Scope::Project,
            Some(project_path),
            vec![crate::models::InstallTargetSpec {
                agent: AgentType::Eve,
                subagent: Some("research".to_string()),
            }],
        )
        .unwrap();

        assert_eq!(result.get("demo"), Some(&vec!["eve:research".to_string()]));
    }

    #[test]
    fn environment_snapshot_drives_agent_private_copy_and_eve_overwrites() {
        let snapshot = EnvironmentSnapshot {
            path_exists: true,
            detected_agents: vec![AgentType::Codex, AgentType::ClaudeCode, AgentType::Eve],
            skills: vec![SkillEntrySnapshot {
                name: "demo".to_string(),
                description: "Demo".to_string(),
                canonical_path: "/work/app/.agents/skills/demo".to_string(),
                canonical_present: true,
                agents: vec![AgentType::Codex, AgentType::ClaudeCode, AgentType::Eve],
                card_agents: vec![AgentType::Codex, AgentType::ClaudeCode, AgentType::Eve],
                default_available_agents: vec![AgentType::Codex],
                private_adapted_agents: vec![AgentType::Eve],
                duplicate_copy_agents: vec![AgentType::ClaudeCode],
                private_only_agents: vec![AgentType::Eve],
                private_copy_agents: vec![AgentType::ClaudeCode],
                eve_targets: vec![InstallTargetInfo {
                    target_id: "eve:research".to_string(),
                    agent: AgentType::Eve,
                    display_name: "Eve (research)".to_string(),
                    subagent: Some("research".to_string()),
                    path: "/work/app/agent/subagents/research/skills/demo".to_string(),
                }],
            }],
        };

        let result = check_overwrites_from_snapshot(
            &snapshot,
            &["demo".to_string()],
            &["codex".to_string()],
            &["claude-code".to_string()],
            &[InstallTargetSpec {
                agent: AgentType::Eve,
                subagent: Some("research".to_string()),
            }],
        )
        .expect("check overwrites");

        assert_eq!(
            result.get("demo"),
            Some(&vec![
                "codex".to_string(),
                "claude-code".to_string(),
                "eve:research".to_string(),
            ])
        );
    }
}

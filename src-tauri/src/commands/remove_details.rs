//! 删除详情查询命令
//!
//! 为智能删除对话框提供 agent 安装详情

use crate::core::agent_availability::detect_agent_presence;
use crate::core::agents::AgentType;
use crate::core::paths::canonical_skills_dir;
use crate::core::skill::sanitize_name;
use crate::environment::agent_environment::{AgentEnvironmentContext, AgentEnvironmentResolver};
use crate::environment::context_resolver::ContextResolver;
use crate::environment::service::{EnvironmentService, InspectRequest, SkillEntrySnapshot};
use crate::environment::types::{ContextRef, ContextScope, EnvironmentRef};
use crate::environment::wsl::{EnvironmentRegistry, WslSession};
use crate::environment::wsl_protocol::{decode_nul_records, run_wsl_script};
use crate::error::AppError;
use crate::models::{
    AgentPresenceInfo, AgentSkillPresence, IndependentAgentInfo, InstallTargetInfo, Scope,
    SkillAgentDetails,
};
use std::collections::HashSet;
use std::path::PathBuf;
use std::str::FromStr;
use tauri::State;
use tokio::time::Duration;

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

fn build_environment_skill_agent_details(
    skill: &SkillEntrySnapshot,
    resolver: &AgentEnvironmentResolver,
    scope: Scope,
    project_path: &str,
    symlink_agents: &HashSet<AgentType>,
) -> SkillAgentDetails {
    let is_global = matches!(scope, Scope::Global);
    let sanitized_name = sanitize_name(&skill.name);
    let mut automatic_agents = Vec::new();
    let mut independent_agents = Vec::new();
    let mut default_available_agents = Vec::new();
    let mut private_required_agents = Vec::new();
    let mut duplicate_copy_agents = Vec::new();
    let mut private_only_agents = Vec::new();

    for agent in AgentType::all() {
        let target = resolver.target(agent, is_global, project_path);
        let shared_path = format!(
            "{}/{}",
            target.shared_path.trim_end_matches('/'),
            sanitized_name
        );
        let private_path = target
            .private_path
            .as_ref()
            .map(|path| format!("{}/{}", path.trim_end_matches('/'), sanitized_name));
        let presence = if skill.duplicate_copy_agents.contains(&agent) {
            AgentSkillPresence::DuplicateCopy
        } else if skill.default_available_agents.contains(&agent) {
            AgentSkillPresence::DefaultActive
        } else if skill.private_only_agents.contains(&agent) {
            AgentSkillPresence::PrivateOnly
        } else if skill.canonical_present
            && target.availability
                != crate::core::agent_availability::AgentAvailabilityKind::Unsupported
            && !target.default_available
        {
            AgentSkillPresence::RequiresPrivateInstall
        } else {
            AgentSkillPresence::NotInstalled
        };
        let info = AgentPresenceInfo {
            agent,
            display_name: target.display_name.clone(),
            presence: presence.clone(),
            shared_path,
            private_path: private_path.clone(),
            can_cleanup_private_copy: matches!(presence, AgentSkillPresence::DuplicateCopy),
        };

        match presence {
            AgentSkillPresence::DefaultActive => {
                automatic_agents.push((agent, target.display_name));
                default_available_agents.push(info);
            }
            AgentSkillPresence::DuplicateCopy => {
                automatic_agents.push((agent, target.display_name.clone()));
                if let Some(path) = private_path {
                    independent_agents.push(IndependentAgentInfo {
                        agent,
                        display_name: target.display_name,
                        path,
                        is_symlink: symlink_agents.contains(&agent),
                    });
                }
                default_available_agents.push(info.clone());
                duplicate_copy_agents.push(info);
            }
            AgentSkillPresence::PrivateOnly => {
                if let Some(path) = private_path {
                    independent_agents.push(IndependentAgentInfo {
                        agent,
                        display_name: target.display_name,
                        path,
                        is_symlink: symlink_agents.contains(&agent),
                    });
                }
                private_only_agents.push(info);
            }
            AgentSkillPresence::RequiresPrivateInstall => private_required_agents.push(info),
            AgentSkillPresence::NotInstalled => {}
        }
    }

    SkillAgentDetails {
        skill_name: skill.name.clone(),
        scope,
        canonical_path: skill.canonical_path.clone(),
        automatic_agents,
        independent_agents,
        default_available_agents,
        private_required_agents,
        duplicate_copy_agents,
        private_only_agents,
        eve_targets: skill.eve_targets.clone(),
    }
}

fn parse_wsl_symlink_agents(bytes: &[u8]) -> Result<HashSet<AgentType>, AppError> {
    let records = decode_nul_records(bytes);
    if records.first().map(String::as_str) != Some("1") || records[1..].len() % 2 != 0 {
        return Err(AppError::Custom {
            message: "invalid WSL symlink response".to_string(),
        });
    }
    let mut symlinks = HashSet::new();
    for record in records[1..].chunks_exact(2) {
        let agent = AgentType::from_str(&record[0]).map_err(|_| AppError::InvalidAgent {
            agent: record[0].clone(),
        })?;
        match record[1].as_str() {
            "0" => {}
            "1" => {
                symlinks.insert(agent);
            }
            _ => {
                return Err(AppError::Custom {
                    message: "invalid WSL symlink flag".to_string(),
                })
            }
        }
    }
    Ok(symlinks)
}

async fn inspect_wsl_private_symlinks(
    session: &WslSession,
    resolver: &AgentEnvironmentResolver,
    skill: &SkillEntrySnapshot,
    scope: Scope,
    project_path: &str,
) -> Result<HashSet<AgentType>, AppError> {
    let mut agents = skill.duplicate_copy_agents.clone();
    for agent in &skill.private_only_agents {
        if !agents.contains(agent) {
            agents.push(*agent);
        }
    }
    let sanitized_name = sanitize_name(&skill.name);
    let mut args = Vec::new();
    for agent in agents {
        let target = resolver.target(agent, matches!(scope, Scope::Global), project_path);
        let Some(private_root) = target.private_path else {
            continue;
        };
        args.push(agent.to_string());
        args.push(format!(
            "{}/{}",
            private_root.trim_end_matches('/'),
            sanitized_name
        ));
    }
    if args.is_empty() {
        return Ok(HashSet::new());
    }
    const SCRIPT: &str = r#"printf '1\0'; while [ "$#" -ge 2 ]; do agent=$1; path=$2; shift 2; if [ -L "$path" ]; then flag=1; else flag=0; fi; printf '%s\0%s\0' "$agent" "$flag"; done"#;
    let output =
        run_wsl_script(session, SCRIPT, &args, Vec::new(), Duration::from_secs(10)).await?;
    parse_wsl_symlink_agents(&output)
}

/// 查询 skill 的 agent 安装详情
///
/// 对话框挂载时调用，返回自动应用/独立安装分组信息
pub async fn get_skill_agent_details_host(
    scope: Scope,
    name: String,
    project_path: Option<String>,
) -> Result<SkillAgentDetails, AppError> {
    let is_global = matches!(scope, Scope::Global);
    let cwd = project_path.as_deref().unwrap_or(".");
    let sanitized_name = sanitize_name(&name);

    // 1. 计算 canonical 路径
    let canonical_path = canonical_skills_dir(is_global, cwd).join(&sanitized_name);
    let eve_targets = eve_targets_for_skill(is_global, cwd, &sanitized_name);

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
        eve_targets,
    })
}

#[tauri::command]
#[specta::specta]
pub async fn get_skill_agent_details(
    context: ContextRef,
    name: String,
    registry: State<'_, EnvironmentRegistry>,
) -> Result<SkillAgentDetails, AppError> {
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
            get_skill_agent_details_host(scope, name, project_path).await
        }
        EnvironmentRef::Wsl { distro_name } => {
            let distro_name = distro_name.clone();
            let retry_context = context.clone();
            registry
                .with_session_retry(&distro_name, move |session| {
                    let context = retry_context.clone();
                    let name = name.clone();
                    async move { get_wsl_skill_agent_details(context, name, session).await }
                })
                .await
        }
    }
}

async fn get_wsl_skill_agent_details(
    context: ContextRef,
    name: String,
    session: WslSession,
) -> Result<SkillAgentDetails, AppError> {
    let resolved = ContextResolver::resolve_wsl(context, &session).await?;
    let scope = match &resolved.context.scope {
        ContextScope::Global => Scope::Global,
        ContextScope::Project { .. } => Scope::Project,
    };
    let project_path = resolved
        .project
        .as_ref()
        .map(|project| project.native_path.clone())
        .unwrap_or_else(|| session.home.clone());
    let snapshot = EnvironmentService::Wsl(session.clone())
        .inspect(&InspectRequest { context: resolved })
        .await?;
    let skill = snapshot
        .skills
        .iter()
        .find(|skill| skill.name == name)
        .ok_or_else(|| AppError::PathNotFound { path: name.clone() })?;
    let resolver = AgentEnvironmentResolver::new(AgentEnvironmentContext {
        home: session.home.clone(),
        config_home: session.config_home.clone(),
        env: session.environment.clone(),
    });
    let symlink_agents =
        inspect_wsl_private_symlinks(&session, &resolver, skill, scope.clone(), &project_path)
            .await?;
    Ok(build_environment_skill_agent_details(
        skill,
        &resolver,
        scope,
        &project_path,
        &symlink_agents,
    ))
}

fn eve_targets_for_skill(
    is_global: bool,
    cwd: &str,
    sanitized_name: &str,
) -> Vec<InstallTargetInfo> {
    if is_global || !crate::core::eve::is_eve_project(cwd) {
        return Vec::new();
    }

    let mut targets = Vec::new();
    let root_path = crate::core::eve::eve_root_skills_dir(cwd).join(sanitized_name);
    if root_path.exists() {
        targets.push(InstallTargetInfo {
            target_id: crate::core::eve::eve_target_id(None),
            agent: AgentType::Eve,
            display_name: crate::core::eve::eve_target_label(None),
            subagent: None,
            path: root_path.to_string_lossy().to_string(),
        });
    }

    for subagent in crate::core::eve::list_eve_subagents(cwd) {
        let path = crate::core::eve::eve_subagent_skills_dir(cwd, &subagent).join(sanitized_name);
        if path.exists() {
            targets.push(InstallTargetInfo {
                target_id: crate::core::eve::eve_target_id(Some(&subagent)),
                agent: AgentType::Eve,
                display_name: crate::core::eve::eve_target_label(Some(&subagent)),
                subagent: Some(subagent),
                path: path.to_string_lossy().to_string(),
            });
        }
    }

    targets
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{BTreeMap, HashSet};

    use crate::environment::agent_environment::{
        AgentEnvironmentContext, AgentEnvironmentResolver,
    };
    use crate::environment::service::SkillEntrySnapshot;
    use tempfile::tempdir;

    #[test]
    fn builds_wsl_agent_details_from_environment_snapshot() {
        let resolver = AgentEnvironmentResolver::new(AgentEnvironmentContext {
            home: "/home/alice".to_string(),
            config_home: "/home/alice/.config".to_string(),
            env: BTreeMap::new(),
        });
        let skill = SkillEntrySnapshot {
            name: "demo".to_string(),
            description: "Demo".to_string(),
            canonical_path: "/work/app/.agents/skills/demo".to_string(),
            canonical_present: true,
            agents: vec![AgentType::Codex, AgentType::ClaudeCode, AgentType::Eve],
            card_agents: vec![AgentType::Codex, AgentType::ClaudeCode, AgentType::Eve],
            default_available_agents: vec![AgentType::Codex, AgentType::ClaudeCode],
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
        };
        let symlink_agents = HashSet::from([AgentType::ClaudeCode]);

        let details = build_environment_skill_agent_details(
            &skill,
            &resolver,
            Scope::Project,
            "/work/app",
            &symlink_agents,
        );

        assert_eq!(details.canonical_path, "/work/app/.agents/skills/demo");
        assert_eq!(details.automatic_agents.len(), 2);
        assert_eq!(details.independent_agents.len(), 2);
        let claude = details
            .independent_agents
            .iter()
            .find(|agent| agent.agent == AgentType::ClaudeCode)
            .expect("claude details");
        assert!(claude.is_symlink);
        assert_eq!(details.duplicate_copy_agents.len(), 1);
        assert_eq!(details.eve_targets[0].target_id, "eve:research");
    }

    #[test]
    fn parses_wsl_private_symlink_flags() {
        let agents = parse_wsl_symlink_agents(b"1\0claude-code\x001\0eve\x000\0")
            .expect("parse symlink flags");

        assert_eq!(agents, HashSet::from([AgentType::ClaudeCode]));
    }

    #[test]
    fn test_get_skill_agent_details_includes_eve_targets() {
        tauri::async_runtime::block_on(async {
            let project = tempdir().unwrap();
            let cwd = project.path().to_string_lossy().to_string();
            std::fs::create_dir_all(project.path().join("agent/skills/demo")).unwrap();
            std::fs::create_dir_all(project.path().join("agent/subagents/research/skills/demo"))
                .unwrap();
            std::fs::write(
                project.path().join("package.json"),
                r#"{"dependencies":{"eve":"^0.11.5"}}"#,
            )
            .unwrap();

            let details =
                get_skill_agent_details_host(Scope::Project, "demo".to_string(), Some(cwd))
                    .await
                    .unwrap();

            let target_ids: Vec<_> = details
                .eve_targets
                .iter()
                .map(|target| target.target_id.as_str())
                .collect();
            assert_eq!(target_ids, vec!["eve:root", "eve:research"]);
        });
    }
}

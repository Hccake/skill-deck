//! Agent 管理命令
//!
//! 为已安装的 skill 添加或移除 agent 支持。
//! - 添加: 从 canonical dir 创建 symlink 到新 agent 目录
//! - 移除: 复用 uninstaller 的 partial removal

use crate::core::agent_availability::detect_agent_presence;
use crate::core::agents::AgentType;
use crate::core::installer::{
    copy_skill_for_agent, copy_skill_for_agent_private, link_skill_for_agent_without_fallback,
};
use crate::core::mutation::{MutationGuard, MutationKind, MutationPhase, SingleMutationController};
use crate::core::paths::canonical_skills_dir;
use crate::core::skill::sanitize_name;
use crate::core::uninstaller::remove_skill;
use crate::environment::agent_environment::{AgentEnvironmentResolver, AgentEnvironmentTarget};
use crate::environment::context_resolver::ContextResolver;
use crate::environment::materialize::{
    materialize_wsl_agent_targets, WslAgentMaterializeRequest, WslAgentMaterializeTarget,
};
use crate::environment::service::{EnvironmentService, InspectRequest, ResolvedContext};
use crate::environment::types::{ContextRef, ContextScope, EnvironmentRef};
use crate::environment::wsl::{EnvironmentRegistry, WslSession};
use crate::error::AppError;
use crate::models::{AgentSkillPresence, InstallMode, Scope};
use tauri::State;

struct WslAgentAddPlan {
    immediate_results: Vec<ManageAgentOperationResult>,
    targets: Vec<WslAgentMaterializeTarget>,
}

fn unsupported_wsl_agent_result(
    target: &AgentEnvironmentTarget,
    mode: InstallMode,
    reason: &str,
) -> ManageAgentOperationResult {
    ManageAgentOperationResult {
        agent: target.agent.to_string(),
        success: false,
        mode,
        path: target.private_path.clone().unwrap_or_default(),
        error: Some(format!("{}: {reason}", target.display_name)),
    }
}

#[allow(clippy::too_many_arguments)]
fn plan_wsl_agent_additions(
    resolver: &AgentEnvironmentResolver,
    is_global: bool,
    context_root: &str,
    canonical_path: &str,
    skill_name: &str,
    add_agents: &[AgentType],
    private_copy_agents: &[AgentType],
    mode: InstallMode,
) -> WslAgentAddPlan {
    let sanitized = sanitize_name(skill_name);
    let mut immediate_results = Vec::new();
    let mut targets = Vec::new();
    for agent in add_agents {
        let target = resolver.target(*agent, is_global, context_root);
        if target.default_available {
            immediate_results.push(ManageAgentOperationResult {
                agent: agent.to_string(),
                success: true,
                mode: mode.clone(),
                path: canonical_path.to_string(),
                error: None,
            });
            continue;
        }
        let Some(private_root) = target.private_path.clone() else {
            immediate_results.push(unsupported_wsl_agent_result(
                &target,
                mode.clone(),
                "does not support a private Skill directory for this scope",
            ));
            continue;
        };
        targets.push(WslAgentMaterializeTarget {
            target_id: agent.to_string(),
            agent: agent.to_string(),
            target_path: format!("{}/{}", private_root.trim_end_matches('/'), sanitized),
            mode: mode.clone(),
            protect_existing_copy: false,
        });
    }
    for agent in private_copy_agents {
        let target = resolver.target(*agent, is_global, context_root);
        let Some(private_root) = target.private_path.clone() else {
            immediate_results.push(unsupported_wsl_agent_result(
                &target,
                InstallMode::Copy,
                "does not have a separate private Skill directory",
            ));
            continue;
        };
        targets.push(WslAgentMaterializeTarget {
            target_id: agent.to_string(),
            agent: agent.to_string(),
            target_path: format!("{}/{}", private_root.trim_end_matches('/'), sanitized),
            mode: InstallMode::Copy,
            protect_existing_copy: true,
        });
    }
    WslAgentAddPlan {
        immediate_results,
        targets,
    }
}

/// 管理 skill 的 agent 支持（添加/移除）
///
/// # Arguments
/// * `skill_name` - skill 名称
/// * `scope` - 安装范围
/// * `project_path` - Project scope 时的项目路径
/// * `add_agents` - 要添加的 agent 列表
/// * `remove_agents` - 要移除的 agent 列表
/// * `mode` - 添加 agent 时使用的安装模式
pub fn manage_skill_agents_host(
    skill_name: String,
    scope: Scope,
    project_path: Option<String>,
    add_agents: Vec<AgentType>,
    remove_agents: Vec<AgentType>,
    private_copy_agents: Vec<AgentType>,
    mode: InstallMode,
) -> Result<ManageAgentsResult, AppError> {
    let is_global = matches!(scope, Scope::Global);
    let cwd = project_path.as_deref().unwrap_or(".");
    let sanitized = sanitize_name(&skill_name);

    // canonical dir = skill 的源文件目录
    let canonical_dir = canonical_skills_dir(is_global, cwd).join(&sanitized);

    if !canonical_dir.exists() {
        return Err(AppError::PathNotFound {
            path: canonical_dir.to_string_lossy().to_string(),
        });
    }

    let mut added: Vec<String> = Vec::new();
    let mut added_results: Vec<ManageAgentOperationResult> = Vec::new();
    let mut add_errors: Vec<String> = Vec::new();

    // 添加 agents: 从已有 canonical dir 按用户选择投放（不重新复制 canonical）
    for agent in &add_agents {
        let result = match mode {
            InstallMode::Symlink => link_skill_for_agent_without_fallback(
                &canonical_dir,
                &skill_name,
                agent,
                &scope,
                project_path.as_deref(),
            ),
            InstallMode::Copy => copy_skill_for_agent(
                &canonical_dir,
                &skill_name,
                agent,
                &scope,
                project_path.as_deref(),
            ),
        };

        let error = result.error.clone();

        if result.success {
            added.push(agent.to_string());
        } else if let Some(err) = &error {
            add_errors.push(format!("{}: {}", agent, err));
        }

        added_results.push(ManageAgentOperationResult {
            agent: result.agent,
            success: result.success,
            mode: result.mode,
            path: result.path.to_string_lossy().to_string(),
            error,
        });
    }

    for agent in &private_copy_agents {
        let presence = detect_agent_presence(*agent, &skill_name, is_global, cwd);
        if matches!(
            presence.presence,
            AgentSkillPresence::DuplicateCopy | AgentSkillPresence::PrivateOnly
        ) {
            let path = presence.private_path.unwrap_or_default();
            let message = format!(
                "{} already has a private copy at {}. Clean the duplicate copy before creating a new private copy.",
                agent.config().display_name,
                path
            );
            add_errors.push(format!("{}: {}", agent, message));
            added_results.push(ManageAgentOperationResult {
                agent: agent.to_string(),
                success: false,
                mode: InstallMode::Copy,
                path,
                error: Some(message),
            });
            continue;
        }

        let result = copy_skill_for_agent_private(
            &canonical_dir,
            &skill_name,
            agent,
            &scope,
            project_path.as_deref(),
        );
        let error = result.error.clone();

        if result.success {
            added.push(agent.to_string());
        } else if let Some(err) = &error {
            add_errors.push(format!("{}: {}", agent, err));
        }

        added_results.push(ManageAgentOperationResult {
            agent: result.agent,
            success: result.success,
            mode: result.mode,
            path: result.path.to_string_lossy().to_string(),
            error,
        });
    }

    let mut removed: Vec<String> = Vec::new();
    let mut remove_errors: Vec<String> = Vec::new();

    // 移除 agents: partial removal
    if !remove_agents.is_empty() {
        match remove_skill(
            &skill_name,
            &scope,
            project_path.as_deref(),
            false, // partial removal
            Some(&remove_agents),
            None,
        ) {
            Ok(result) => {
                if result.success {
                    removed = remove_agents.iter().map(|a| a.to_string()).collect();
                } else if let Some(err) = result.error {
                    remove_errors.push(err);
                }
            }
            Err(e) => {
                remove_errors.push(e.to_string());
            }
        }
    }

    Ok(ManageAgentsResult {
        added,
        added_results,
        removed,
        errors: [add_errors, remove_errors].concat(),
    })
}

#[allow(clippy::too_many_arguments)]
#[tauri::command]
#[specta::specta]
pub async fn manage_skill_agents(
    context: ContextRef,
    skill_name: String,
    add_agents: Vec<AgentType>,
    remove_agents: Vec<AgentType>,
    private_copy_agents: Vec<AgentType>,
    mode: InstallMode,
    registry: State<'_, EnvironmentRegistry>,
    controller: State<'_, SingleMutationController>,
) -> Result<ManageAgentsResult, AppError> {
    let guard = controller.begin(MutationKind::ManageAgents, context.clone())?;
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
            manage_skill_agents_host(
                skill_name,
                scope,
                project_path,
                add_agents,
                remove_agents,
                private_copy_agents,
                mode,
            )
        }
        EnvironmentRef::Wsl { distro_name } => {
            let distro_name = distro_name.clone();
            let retry_context = context.clone();
            let guard = &guard;
            registry
                .with_session(&distro_name, move |session| {
                    let context = retry_context.clone();
                    let skill_name = skill_name.clone();
                    let add_agents = add_agents.clone();
                    let remove_agents = remove_agents.clone();
                    let private_copy_agents = private_copy_agents.clone();
                    let mode = mode.clone();
                    async move {
                        let resolved = ContextResolver::resolve_wsl(context, &session).await?;
                        manage_skill_agents_wsl(
                            resolved,
                            &session,
                            &skill_name,
                            &add_agents,
                            &remove_agents,
                            &private_copy_agents,
                            mode,
                            guard,
                        )
                        .await
                    }
                })
                .await
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn manage_skill_agents_wsl(
    context: ResolvedContext,
    session: &WslSession,
    skill_name: &str,
    add_agents: &[AgentType],
    remove_agents: &[AgentType],
    private_copy_agents: &[AgentType],
    mode: InstallMode,
    guard: &MutationGuard<'_>,
) -> Result<ManageAgentsResult, AppError> {
    let context_root = context.context_root().to_string();
    let is_global = context.project.is_none();
    let snapshot = EnvironmentService::Wsl(session.clone())
        .inspect(&InspectRequest { context })
        .await?
        .skills
        .into_iter()
        .find(|skill| skill.name == skill_name)
        .filter(|skill| skill.canonical_present)
        .ok_or_else(|| AppError::PathNotFound {
            path: format!(
                "{}/.agents/skills/{}",
                context_root.trim_end_matches('/'),
                sanitize_name(skill_name)
            ),
        })?;
    let resolver = AgentEnvironmentResolver::new(
        crate::environment::agent_environment::AgentEnvironmentContext {
            home: session.home.clone(),
            config_home: session.config_home.clone(),
            env: session.environment.clone(),
        },
    );
    let add_plan = plan_wsl_agent_additions(
        &resolver,
        is_global,
        &context_root,
        &snapshot.canonical_path,
        skill_name,
        add_agents,
        private_copy_agents,
        mode,
    );
    let mut added = Vec::new();
    let mut added_results = add_plan.immediate_results;
    let mut removed = Vec::new();
    let mut errors = Vec::new();
    for result in &added_results {
        if result.success {
            added.push(result.agent.clone());
        } else if let Some(error) = &result.error {
            errors.push(format!("{}: {}", result.agent, error));
        }
    }

    if guard.cancellation().is_cancelled() {
        return Err(AppError::Custom {
            message: "Agent changes were cancelled".to_string(),
        });
    }
    if !add_plan.targets.is_empty() {
        guard.transition(MutationPhase::Materializing, None, false);
        let materialized = materialize_wsl_agent_targets(
            session,
            WslAgentMaterializeRequest {
                canonical_path: snapshot.canonical_path.clone(),
                targets: add_plan.targets,
            },
        )
        .await;
        for result in materialized? {
            if result.success {
                added.push(result.agent.clone());
            } else if let Some(error) = &result.error {
                errors.push(format!("{}: {}", result.agent, error));
            }
            added_results.push(ManageAgentOperationResult {
                agent: result.agent,
                success: result.success,
                mode: result.mode,
                path: result.path,
                error: result.error,
            });
        }
    }

    if guard.cancellation().is_cancelled() {
        errors.push("Agent changes were cancelled before removals".to_string());
        return Ok(ManageAgentsResult {
            added,
            added_results,
            removed,
            errors,
        });
    }
    let sanitized = sanitize_name(skill_name);
    let mut removal_agents = Vec::new();
    let mut removal_paths = Vec::new();
    for agent in remove_agents {
        let target = resolver.target(*agent, is_global, &context_root);
        let Some(private_root) = target.private_path else {
            errors.push(format!(
                "{}: no separate private Skill directory can be removed",
                target.display_name
            ));
            continue;
        };
        removal_agents.push(*agent);
        removal_paths.push(format!(
            "{}/{}",
            private_root.trim_end_matches('/'),
            sanitized
        ));
    }
    if !removal_paths.is_empty() {
        guard.transition(MutationPhase::Materializing, None, false);
        let removal_results =
            crate::commands::remove::remove_wsl_paths(session, &removal_paths).await;
        for (agent, result) in removal_agents.into_iter().zip(removal_results?) {
            if result.success {
                removed.push(agent.to_string());
            } else {
                errors.push(format!(
                    "{}: {}",
                    agent,
                    result
                        .error
                        .unwrap_or_else(|| "failed to remove Agent target".to_string())
                ));
            }
        }
    }

    Ok(ManageAgentsResult {
        added,
        added_results,
        removed,
        errors,
    })
}

/// 单个 agent 添加操作结果
#[derive(Debug, Clone, serde::Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct ManageAgentOperationResult {
    pub agent: String,
    pub success: bool,
    pub mode: InstallMode,
    pub path: String,
    pub error: Option<String>,
}

/// Agent 管理操作结果
#[derive(Debug, Clone, serde::Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct ManageAgentsResult {
    /// 成功添加的 agent IDs
    pub added: Vec<String>,
    /// 每个新增 agent 的详细结果
    pub added_results: Vec<ManageAgentOperationResult>,
    /// 成功移除的 agent IDs
    pub removed: Vec<String>,
    /// 错误信息列表
    pub errors: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    /// 创建模拟的已安装 skill 目录
    fn setup_installed_skill(temp: &std::path::Path, name: &str) -> std::path::PathBuf {
        let canonical = temp.join(".agents").join("skills").join(name);
        fs::create_dir_all(&canonical).unwrap();
        fs::write(
            canonical.join("SKILL.md"),
            format!("---\nname: {}\ndescription: test\n---\n# {}", name, name),
        )
        .unwrap();
        canonical
    }

    #[test]
    fn test_manage_skill_agents_returns_error_for_missing_skill() {
        let temp = tempdir().unwrap();
        let project_path = temp.path().to_string_lossy().to_string();

        let result = manage_skill_agents_host(
            "nonexistent".to_string(),
            Scope::Project,
            Some(project_path),
            vec![],
            vec![],
            vec![],
            crate::models::InstallMode::Symlink,
        );

        assert!(result.is_err());
    }

    #[test]
    fn test_manage_skill_agents_add_agent_preserves_canonical() {
        let temp = tempdir().unwrap();
        let project_path = temp.path().to_string_lossy().to_string();
        let canonical = setup_installed_skill(temp.path(), "my-skill");

        let result = manage_skill_agents_host(
            "my-skill".to_string(),
            Scope::Project,
            Some(project_path),
            vec![AgentType::Cursor],
            vec![],
            vec![],
            crate::models::InstallMode::Symlink,
        )
        .unwrap();

        assert!(result.added.contains(&"cursor".to_string()));
        assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
        // canonical 不应被破坏
        assert!(
            canonical.join("SKILL.md").exists(),
            "canonical must survive"
        );
    }

    #[test]
    fn test_manage_skill_agents_empty_ops_succeeds() {
        let temp = tempdir().unwrap();
        let project_path = temp.path().to_string_lossy().to_string();
        setup_installed_skill(temp.path(), "my-skill");

        let result = manage_skill_agents_host(
            "my-skill".to_string(),
            Scope::Project,
            Some(project_path),
            vec![],
            vec![],
            vec![],
            crate::models::InstallMode::Symlink,
        )
        .unwrap();

        assert!(result.added.is_empty());
        assert!(result.removed.is_empty());
        assert!(result.errors.is_empty());
    }

    #[test]
    fn test_manage_skill_agents_add_agent_with_copy() {
        let temp = tempdir().unwrap();
        let project_path = temp.path().to_string_lossy().to_string();
        let canonical = setup_installed_skill(temp.path(), "copy-agent-skill");

        let result = manage_skill_agents_host(
            "copy-agent-skill".to_string(),
            Scope::Project,
            Some(project_path.clone()),
            vec![AgentType::ClaudeCode],
            vec![],
            vec![],
            crate::models::InstallMode::Copy,
        )
        .unwrap();

        assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
        assert_eq!(result.added_results.len(), 1);
        assert_eq!(
            result.added_results[0].mode,
            crate::models::InstallMode::Copy
        );

        let agent_copy = temp
            .path()
            .join(".claude")
            .join("skills")
            .join("copy-agent-skill");
        assert!(canonical.join("SKILL.md").exists());
        assert!(agent_copy.join("SKILL.md").exists());
    }

    #[test]
    fn test_manage_skill_agents_add_private_copy() {
        let temp = tempdir().unwrap();
        let project_path = temp.path().to_string_lossy().to_string();
        let canonical = setup_installed_skill(temp.path(), "private-copy-agent-skill");

        let result = manage_skill_agents_host(
            "private-copy-agent-skill".to_string(),
            Scope::Project,
            Some(project_path.clone()),
            vec![],
            vec![],
            vec![AgentType::ClaudeCode],
            crate::models::InstallMode::Symlink,
        )
        .unwrap();

        assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
        assert_eq!(result.added, vec!["claude-code".to_string()]);
        assert_eq!(
            result.added_results[0].mode,
            crate::models::InstallMode::Copy
        );

        let agent_copy = temp
            .path()
            .join(".claude")
            .join("skills")
            .join("private-copy-agent-skill");
        assert!(canonical.join("SKILL.md").exists());
        assert!(agent_copy.join("SKILL.md").exists());
    }

    #[test]
    fn test_manage_skill_agents_private_copy_does_not_overwrite_existing_copy() {
        let temp = tempdir().unwrap();
        let project_path = temp.path().to_string_lossy().to_string();
        let canonical = setup_installed_skill(temp.path(), "duplicate-private-copy-skill");
        let agent_copy = temp
            .path()
            .join(".claude")
            .join("skills")
            .join("duplicate-private-copy-skill");
        fs::create_dir_all(&agent_copy).unwrap();
        fs::write(agent_copy.join("SKILL.md"), "# Local edits").unwrap();

        let result = manage_skill_agents_host(
            "duplicate-private-copy-skill".to_string(),
            Scope::Project,
            Some(project_path.clone()),
            vec![],
            vec![],
            vec![AgentType::ClaudeCode],
            crate::models::InstallMode::Symlink,
        )
        .unwrap();

        assert!(result.added.is_empty());
        assert_eq!(result.added_results.len(), 1);
        assert!(!result.added_results[0].success);
        assert!(result.added_results[0]
            .error
            .as_deref()
            .unwrap_or_default()
            .contains("already has a private copy"));
        assert_eq!(
            fs::read_to_string(agent_copy.join("SKILL.md")).unwrap(),
            "# Local edits"
        );
        assert!(canonical.join("SKILL.md").exists());
    }

    #[test]
    fn test_manage_skill_agents_add_agent_with_symlink() {
        let temp = tempdir().unwrap();
        let project_path = temp.path().to_string_lossy().to_string();
        let canonical = setup_installed_skill(temp.path(), "link-agent-skill");

        let result = manage_skill_agents_host(
            "link-agent-skill".to_string(),
            Scope::Project,
            Some(project_path.clone()),
            vec![AgentType::ClaudeCode],
            vec![],
            vec![],
            crate::models::InstallMode::Symlink,
        )
        .unwrap();

        assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
        assert_eq!(result.added_results.len(), 1);
        assert_eq!(
            result.added_results[0].mode,
            crate::models::InstallMode::Symlink
        );
        assert!(canonical.join("SKILL.md").exists());
    }

    #[test]
    fn wsl_add_plan_uses_noop_for_default_agent_and_private_path_for_required_agent() {
        let resolver = crate::environment::agent_environment::AgentEnvironmentResolver::new(
            crate::environment::agent_environment::AgentEnvironmentContext {
                home: "/home/alice".to_string(),
                config_home: "/home/alice/.config".to_string(),
                env: Default::default(),
            },
        );

        let global = plan_wsl_agent_additions(
            &resolver,
            true,
            "/home/alice",
            "/home/alice/.agents/skills/demo",
            "demo",
            &[AgentType::Codex],
            &[],
            InstallMode::Symlink,
        );
        assert_eq!(global.immediate_results.len(), 1);
        assert!(global.immediate_results[0].success);
        assert!(global.targets.is_empty());

        let project = plan_wsl_agent_additions(
            &resolver,
            false,
            "/work/app",
            "/work/app/.agents/skills/demo",
            "demo",
            &[AgentType::ClaudeCode],
            &[],
            InstallMode::Symlink,
        );
        assert!(project.immediate_results.is_empty());
        assert_eq!(
            project.targets[0].target_path,
            "/work/app/.claude/skills/demo"
        );
        assert!(!project.targets[0].protect_existing_copy);
    }

    #[test]
    fn wsl_private_copy_plan_protects_existing_private_path() {
        let resolver = crate::environment::agent_environment::AgentEnvironmentResolver::new(
            crate::environment::agent_environment::AgentEnvironmentContext {
                home: "/home/alice".to_string(),
                config_home: "/home/alice/.config".to_string(),
                env: Default::default(),
            },
        );

        let plan = plan_wsl_agent_additions(
            &resolver,
            true,
            "/home/alice",
            "/home/alice/.agents/skills/demo",
            "demo",
            &[],
            &[AgentType::Codex],
            InstallMode::Symlink,
        );

        assert_eq!(plan.targets.len(), 1);
        assert_eq!(plan.targets[0].mode, InstallMode::Copy);
        assert!(plan.targets[0].protect_existing_copy);
        assert_eq!(
            plan.targets[0].target_path,
            "/home/alice/.codex/skills/demo"
        );
    }
}

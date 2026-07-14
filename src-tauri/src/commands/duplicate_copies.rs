//! 重复私有副本清理命令

use crate::core::agent_availability::detect_agent_presence;
use crate::core::agents::AgentType;
use crate::core::mutation::{MutationGuard, MutationKind, MutationPhase, SingleMutationController};
use crate::environment::agent_environment::{AgentEnvironmentContext, AgentEnvironmentResolver};
use crate::environment::context_resolver::ContextResolver;
use crate::environment::service::{EnvironmentService, InspectRequest, ResolvedContext};
use crate::environment::types::{ContextRef, ContextScope, EnvironmentRef};
use crate::environment::wsl::{EnvironmentRegistry, WslSession};
use crate::error::AppError;
use crate::models::{AgentSkillPresence, Scope};
use std::path::PathBuf;
use tauri::State;

#[derive(Debug, Clone, serde::Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct DuplicateCleanupResult {
    pub agent: AgentType,
    pub success: bool,
    pub skipped: bool,
    pub path: String,
    pub error: Option<String>,
}

#[tauri::command]
#[specta::specta]
pub async fn cleanup_duplicate_agent_copy(
    context: ContextRef,
    skill_name: String,
    agent: AgentType,
    registry: State<'_, EnvironmentRegistry>,
    controller: State<'_, SingleMutationController>,
) -> Result<DuplicateCleanupResult, AppError> {
    let guard = controller.begin(MutationKind::DuplicateCleanup, context.clone())?;
    let mut results =
        cleanup_duplicate_agent_copies_inner(&context, &skill_name, &[agent], &registry, &guard)
            .await?;
    Ok(results.pop().expect("one cleanup result"))
}

#[tauri::command]
#[specta::specta]
pub async fn cleanup_duplicate_agent_copies(
    context: ContextRef,
    skill_name: String,
    agents: Vec<AgentType>,
    registry: State<'_, EnvironmentRegistry>,
    controller: State<'_, SingleMutationController>,
) -> Result<Vec<DuplicateCleanupResult>, AppError> {
    let guard = controller.begin(MutationKind::DuplicateCleanup, context.clone())?;
    cleanup_duplicate_agent_copies_inner(&context, &skill_name, &agents, &registry, &guard).await
}

async fn cleanup_duplicate_agent_copies_inner(
    context: &ContextRef,
    skill_name: &str,
    agents: &[AgentType],
    registry: &EnvironmentRegistry,
    guard: &MutationGuard<'_>,
) -> Result<Vec<DuplicateCleanupResult>, AppError> {
    match &context.environment {
        EnvironmentRef::Host => {
            let resolved = ContextResolver::resolve_host(context.clone())?;
            let (scope, project_path) = match &resolved.context.scope {
                ContextScope::Global => (Scope::Global, None),
                ContextScope::Project { .. } => (
                    Scope::Project,
                    resolved.project.map(|project| project.native_path),
                ),
            };
            Ok(agents
                .iter()
                .copied()
                .map(|agent| {
                    cleanup_duplicate_agent_copy_inner(
                        skill_name,
                        agent,
                        &scope,
                        project_path.as_deref(),
                    )
                    .unwrap_or_else(|error| DuplicateCleanupResult {
                        agent,
                        success: false,
                        skipped: false,
                        path: String::new(),
                        error: Some(error.to_string()),
                    })
                })
                .collect())
        }
        EnvironmentRef::Wsl { distro_name } => {
            let distro_name = distro_name.clone();
            let retry_context = context.clone();
            let skill_name = skill_name.to_string();
            let agents = agents.to_vec();
            registry
                .with_session(&distro_name, move |session| {
                    let context = retry_context.clone();
                    let skill_name = skill_name.clone();
                    let agents = agents.clone();
                    async move {
                        let resolved = ContextResolver::resolve_wsl(context, &session).await?;
                        cleanup_duplicate_agent_copies_wsl(
                            resolved,
                            &session,
                            &skill_name,
                            &agents,
                            guard,
                        )
                        .await
                    }
                })
                .await
        }
    }
}

fn wsl_duplicate_skill_path(
    agent: AgentType,
    is_global: bool,
    home: &str,
    config_home: &str,
    environment: &std::collections::BTreeMap<String, String>,
    context_root: &str,
    skill_name: &str,
) -> Option<String> {
    let resolver = AgentEnvironmentResolver::new(AgentEnvironmentContext {
        home: home.to_string(),
        config_home: config_home.to_string(),
        env: environment.clone(),
    });
    resolver
        .target(agent, is_global, context_root)
        .private_path
        .map(|root| {
            format!(
                "{}/{}",
                root.trim_end_matches('/'),
                crate::core::skill::sanitize_name(skill_name)
            )
        })
}

async fn cleanup_duplicate_agent_copies_wsl(
    context: ResolvedContext,
    session: &WslSession,
    skill_name: &str,
    agents: &[AgentType],
    guard: &MutationGuard<'_>,
) -> Result<Vec<DuplicateCleanupResult>, AppError> {
    let context_root = context.context_root().to_string();
    let is_global = context.project.is_none();
    let snapshot = EnvironmentService::Wsl(session.clone())
        .inspect(&InspectRequest { context })
        .await?
        .skills
        .into_iter()
        .find(|skill| skill.name == skill_name);
    let duplicate_agents = snapshot
        .map(|snapshot| snapshot.duplicate_copy_agents)
        .unwrap_or_default();
    let mut results = Vec::new();
    for agent in agents {
        let path = wsl_duplicate_skill_path(
            *agent,
            is_global,
            &session.home,
            &session.config_home,
            &session.environment,
            &context_root,
            skill_name,
        )
        .unwrap_or_default();
        if !duplicate_agents.contains(agent) {
            results.push(DuplicateCleanupResult {
                agent: *agent,
                success: false,
                skipped: true,
                path,
                error: None,
            });
            continue;
        }
        if guard.cancellation().is_cancelled() {
            return Err(AppError::Custom {
                message: "Duplicate cleanup was cancelled".to_string(),
            });
        }
        guard.transition(MutationPhase::Materializing, None, false);
        let removal =
            crate::commands::remove::remove_wsl_paths(session, std::slice::from_ref(&path))
                .await?
                .into_iter()
                .next()
                .expect("one removal result");
        results.push(DuplicateCleanupResult {
            agent: *agent,
            success: removal.success,
            skipped: false,
            path: removal.path,
            error: removal.error,
        });
    }
    Ok(results)
}

fn cleanup_duplicate_agent_copy_inner(
    skill_name: &str,
    agent: AgentType,
    scope: &Scope,
    project_path: Option<&str>,
) -> Result<DuplicateCleanupResult, AppError> {
    let is_global = matches!(scope, Scope::Global);
    let cwd = project_path.unwrap_or(".");
    let presence = detect_agent_presence(agent, skill_name, is_global, cwd);
    let private_path = presence.private_path.clone().unwrap_or_default();

    if presence.presence != AgentSkillPresence::DuplicateCopy {
        return Ok(DuplicateCleanupResult {
            agent,
            success: false,
            skipped: true,
            path: private_path,
            error: None,
        });
    }

    let private_path_buf = PathBuf::from(&private_path);
    if private_path_buf.is_dir() {
        std::fs::remove_dir_all(&private_path_buf).map_err(|error| AppError::InstallFailed {
            message: format!("Failed to remove duplicate private copy: {}", error),
        })?;
    } else if private_path_buf.exists() || private_path_buf.symlink_metadata().is_ok() {
        std::fs::remove_file(&private_path_buf).map_err(|error| AppError::InstallFailed {
            message: format!("Failed to remove duplicate private copy: {}", error),
        })?;
    }

    Ok(DuplicateCleanupResult {
        agent,
        success: true,
        skipped: false,
        path: private_path,
        error: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::paths::PATHS;

    #[test]
    fn test_cleanup_duplicate_private_copy_keeps_canonical() {
        let skill_name = format!("skill-deck-cleanup-test-{}", std::process::id());
        let canonical = PATHS.home.join(".agents").join("skills").join(&skill_name);
        let private = PATHS
            .home
            .join(".firebender")
            .join("skills")
            .join(&skill_name);

        let _ = std::fs::remove_dir_all(&canonical);
        let _ = std::fs::remove_dir_all(&private);
        std::fs::create_dir_all(&canonical).unwrap();
        std::fs::create_dir_all(&private).unwrap();
        std::fs::write(canonical.join("SKILL.md"), "---\nname: demo\n---\n").unwrap();
        std::fs::write(private.join("SKILL.md"), "---\nname: demo\n---\n").unwrap();

        let result = cleanup_duplicate_agent_copy_inner(
            &skill_name,
            AgentType::Firebender,
            &Scope::Global,
            None,
        )
        .unwrap();

        assert!(result.success);
        assert!(canonical.exists());
        assert!(!private.exists());

        let _ = std::fs::remove_dir_all(&canonical);
    }

    #[test]
    fn wsl_duplicate_cleanup_targets_private_path_only() {
        let path = wsl_duplicate_skill_path(
            AgentType::ClaudeCode,
            false,
            "/home/alice",
            "/home/alice/.config",
            &std::collections::BTreeMap::new(),
            "/work/app",
            "demo",
        )
        .expect("private path");

        assert_eq!(path, "/work/app/.claude/skills/demo");
        assert_ne!(path, "/work/app/.agents/skills/demo");
    }
}

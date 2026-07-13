//! 重复私有副本清理命令

use crate::core::agent_availability::detect_agent_presence;
use crate::core::agents::AgentType;
use crate::core::mutation::{MutationGuard, MutationKind, SingleMutationController};
use crate::environment::agent_environment::{AgentEnvironmentContext, AgentEnvironmentResolver};
use crate::environment::service::{EnvironmentService, InspectRequest, ResolvedContext};
use crate::environment::types::{ContextRef, ContextScope, EnvironmentRef, ResourceLocator};
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
pub fn cleanup_duplicate_agent_copy(
    skill_name: String,
    agent: AgentType,
    scope: Scope,
    project_path: Option<String>,
) -> Result<DuplicateCleanupResult, AppError> {
    cleanup_duplicate_agent_copy_inner(&skill_name, agent, &scope, project_path.as_deref())
}

#[tauri::command]
#[specta::specta]
pub fn cleanup_duplicate_agent_copies(
    skill_name: String,
    scope: Scope,
    project_path: Option<String>,
    agents: Vec<AgentType>,
) -> Result<Vec<DuplicateCleanupResult>, AppError> {
    Ok(agents
        .into_iter()
        .map(|agent| {
            cleanup_duplicate_agent_copy_inner(&skill_name, agent, &scope, project_path.as_deref())
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

#[tauri::command]
#[specta::specta]
pub async fn cleanup_duplicate_agent_copy_v2(
    context: ContextRef,
    skill_name: String,
    agent: AgentType,
    registry: State<'_, EnvironmentRegistry>,
    controller: State<'_, SingleMutationController>,
) -> Result<DuplicateCleanupResult, AppError> {
    let guard = controller.begin(
        MutationKind::DuplicateCleanup,
        context.clone(),
        "Preparing duplicate cleanup",
    )?;
    let mut results =
        cleanup_duplicate_agent_copies_v2_inner(&context, &skill_name, &[agent], &registry, &guard)
            .await?;
    Ok(results.pop().expect("one cleanup result"))
}

#[tauri::command]
#[specta::specta]
pub async fn cleanup_duplicate_agent_copies_v2(
    context: ContextRef,
    skill_name: String,
    agents: Vec<AgentType>,
    registry: State<'_, EnvironmentRegistry>,
    controller: State<'_, SingleMutationController>,
) -> Result<Vec<DuplicateCleanupResult>, AppError> {
    let guard = controller.begin(
        MutationKind::DuplicateCleanup,
        context.clone(),
        "Preparing duplicate cleanup",
    )?;
    cleanup_duplicate_agent_copies_v2_inner(&context, &skill_name, &agents, &registry, &guard).await
}

async fn cleanup_duplicate_agent_copies_v2_inner(
    context: &ContextRef,
    skill_name: &str,
    agents: &[AgentType],
    registry: &EnvironmentRegistry,
    guard: &MutationGuard<'_>,
) -> Result<Vec<DuplicateCleanupResult>, AppError> {
    match &context.environment {
        EnvironmentRef::Host => {
            let (scope, project_path) = match &context.scope {
                ContextScope::Global => (Scope::Global, None),
                ContextScope::Project { project_id } => {
                    let project = crate::commands::environments::host_projects_store()?
                        .read()?
                        .into_iter()
                        .find(|project| &project.id == project_id)
                        .ok_or_else(|| AppError::PathNotFound {
                            path: project_id.clone(),
                        })?;
                    (Scope::Project, Some(project.native_path))
                }
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
            let session = registry.get(distro_name).ok_or_else(|| AppError::Custom {
                message: format!("WSL distro '{distro_name}' is not connected"),
            })?;
            cleanup_duplicate_agent_copies_wsl(context, &session, skill_name, agents, guard).await
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
    context: &ContextRef,
    session: &WslSession,
    skill_name: &str,
    agents: &[AgentType],
    guard: &MutationGuard<'_>,
) -> Result<Vec<DuplicateCleanupResult>, AppError> {
    let (project, context_root) = match &context.scope {
        ContextScope::Global => (None, session.home.clone()),
        ContextScope::Project { project_id } => {
            let project = crate::commands::environments::read_wsl_projects(session)
                .await?
                .into_iter()
                .find(|project| &project.id == project_id)
                .ok_or_else(|| AppError::PathNotFound {
                    path: project_id.clone(),
                })?;
            let root = project.native_path.clone();
            (Some(project), root)
        }
    };
    let is_global = project.is_none();
    let (lock, _) = crate::commands::remove::wsl_remove_lock_locators(
        context,
        session,
        project.as_ref().map(|project| project.native_path.as_str()),
    );
    let canonical_root = format!("{}/.agents/skills", context_root.trim_end_matches('/'));
    let snapshot = EnvironmentService::Wsl(session.clone())
        .inspect(&InspectRequest {
            context: ResolvedContext {
                context: context.clone(),
                project,
                home: ResourceLocator {
                    environment: context.environment.clone(),
                    native_path: session.home.clone(),
                },
                skill_root: ResourceLocator {
                    environment: context.environment.clone(),
                    native_path: canonical_root,
                },
                lock,
            },
        })
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
        guard.set_cancelable(false);
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
        guard.set_cancelable(true);
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

        let result = cleanup_duplicate_agent_copy(
            skill_name.clone(),
            AgentType::Firebender,
            Scope::Global,
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

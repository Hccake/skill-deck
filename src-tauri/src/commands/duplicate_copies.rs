//! 重复私有副本清理命令

use crate::core::agent_availability::detect_agent_presence;
use crate::core::agents::AgentType;
use crate::error::AppError;
use crate::models::{AgentSkillPresence, Scope};
use std::path::PathBuf;

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
}

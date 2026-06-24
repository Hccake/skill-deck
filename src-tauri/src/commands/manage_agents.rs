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
use crate::core::paths::canonical_skills_dir;
use crate::core::skill::sanitize_name;
use crate::core::uninstaller::remove_skill;
use crate::error::AppError;
use crate::models::{AgentSkillPresence, InstallMode, Scope};

/// 管理 skill 的 agent 支持（添加/移除）
///
/// # Arguments
/// * `skill_name` - skill 名称
/// * `scope` - 安装范围
/// * `project_path` - Project scope 时的项目路径
/// * `add_agents` - 要添加的 agent 列表
/// * `remove_agents` - 要移除的 agent 列表
/// * `mode` - 添加 agent 时使用的安装模式
#[tauri::command]
#[specta::specta]
pub fn manage_skill_agents(
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

        let result = manage_skill_agents(
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

        let result = manage_skill_agents(
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

        let result = manage_skill_agents(
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

        let result = manage_skill_agents(
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

        let result = manage_skill_agents(
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

        let result = manage_skill_agents(
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

        let result = manage_skill_agents(
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
}

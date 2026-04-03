//! Agent 管理命令
//!
//! 为已安装的 skill 添加或移除 agent 支持。
//! - 添加: 从 canonical dir 创建 symlink 到新 agent 目录
//! - 移除: 复用 uninstaller 的 partial removal

use crate::core::agents::AgentType;
use crate::core::installer::link_skill_for_agent;
use crate::core::paths::canonical_skills_dir;
use crate::core::skill::sanitize_name;
use crate::core::uninstaller::remove_skill;
use crate::error::AppError;
use crate::models::Scope;

/// 管理 skill 的 agent 支持（添加/移除）
///
/// # Arguments
/// * `skill_name` - skill 名称
/// * `scope` - 安装范围
/// * `project_path` - Project scope 时的项目路径
/// * `add_agents` - 要添加的 agent 列表
/// * `remove_agents` - 要移除的 agent 列表
#[tauri::command]
#[specta::specta]
pub fn manage_skill_agents(
    skill_name: String,
    scope: Scope,
    project_path: Option<String>,
    add_agents: Vec<AgentType>,
    remove_agents: Vec<AgentType>,
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
    let mut add_errors: Vec<String> = Vec::new();

    // 添加 agents: 从已有 canonical dir 创建 symlink（不重新复制 canonical）
    for agent in &add_agents {
        let result = link_skill_for_agent(
            &canonical_dir,
            &skill_name,
            agent,
            &scope,
            project_path.as_deref(),
        );
        if result.success {
            added.push(agent.to_string());
        } else if let Some(err) = result.error {
            add_errors.push(format!("{}: {}", agent, err));
        }
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
        removed,
        errors: [add_errors, remove_errors].concat(),
    })
}

/// Agent 管理操作结果
#[derive(Debug, Clone, serde::Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct ManageAgentsResult {
    /// 成功添加的 agent IDs
    pub added: Vec<String>,
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
        ).unwrap();
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
        ).unwrap();

        assert!(result.added.contains(&"cursor".to_string()));
        assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
        // canonical 不应被破坏
        assert!(canonical.join("SKILL.md").exists(), "canonical must survive");
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
        ).unwrap();

        assert!(result.added.is_empty());
        assert!(result.removed.is_empty());
        assert!(result.errors.is_empty());
    }
}

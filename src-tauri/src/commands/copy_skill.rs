//! 复制 skill 到其他项目
//!
//! 将项目级 skill 复制到其他项目，保持相同的 agent 配置。
//! 复用 installer 的 install_skill_to_agents 完成实际安装。

use crate::core::agents::AgentType;
use crate::core::installer::install_skill_to_agents;
use crate::core::local_lock::{add_skill_to_local_lock, compute_skill_folder_hash, read_local_lock};
use crate::core::paths::canonical_skills_dir;
use crate::core::skill::sanitize_name;
use crate::error::AppError;
use crate::models::{InstallMode, Scope};

/// 检查 skill 在哪些项目中已存在
///
/// 返回每个项目路径是否存在该 skill 的 canonical dir
#[tauri::command]
#[specta::specta]
pub fn check_skill_in_projects(
    skill_name: String,
    project_paths: Vec<String>,
) -> Vec<ProjectSkillStatus> {
    let sanitized = sanitize_name(&skill_name);
    project_paths
        .into_iter()
        .map(|path| {
            let exists = canonical_skills_dir(false, &path)
                .join(&sanitized)
                .exists();
            ProjectSkillStatus {
                project_path: path,
                has_skill: exists,
            }
        })
        .collect()
}

/// 项目中 skill 存在状态
#[derive(Debug, Clone, serde::Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct ProjectSkillStatus {
    pub project_path: String,
    pub has_skill: bool,
}

/// 单个目标项目的复制结果
#[derive(Debug, Clone, serde::Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct CopyProjectResult {
    /// 目标项目路径
    pub project_path: String,
    /// 是否成功
    pub success: bool,
    /// 错误信息
    pub error: Option<String>,
}

/// 复制结果汇总
#[derive(Debug, Clone, serde::Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct CopySkillResult {
    pub results: Vec<CopyProjectResult>,
}

/// 复制项目级 skill 到其他项目
///
/// # Arguments
/// * `skill_name` - skill 名称
/// * `source_project_path` - 源项目路径
/// * `target_project_paths` - 目标项目路径列表
/// * `agents` - 要安装的 agent 列表（与源 skill 相同）
#[tauri::command]
#[specta::specta]
pub fn copy_skill_to_projects(
    skill_name: String,
    source_project_path: String,
    target_project_paths: Vec<String>,
    agents: Vec<String>,
) -> Result<CopySkillResult, AppError> {
    let sanitized = sanitize_name(&skill_name);

    // 1. 确认源 canonical dir 存在
    let source_canonical = canonical_skills_dir(false, &source_project_path).join(&sanitized);
    if !source_canonical.exists() {
        return Err(AppError::PathNotFound {
            path: source_canonical.to_string_lossy().to_string(),
        });
    }

    // 2. 读取源项目的 lock entry
    let source_lock_entry = match read_local_lock(&source_project_path) {
        Ok(lock) => lock.skills.get(&skill_name).cloned(),
        Err(e) => {
            log::warn!("Failed to read source local lock for copy: {}", e);
            None
        }
    };

    // 3. 解析 agent types
    let agent_types: Vec<AgentType> = agents
        .iter()
        .filter_map(|s| s.parse::<AgentType>().ok())
        .collect();

    // 4. 对每个目标项目执行复制
    let mut results = Vec::with_capacity(target_project_paths.len());

    for target_path in &target_project_paths {
        let result = copy_to_single_project(
            &skill_name,
            &source_canonical,
            target_path,
            &agent_types,
            source_lock_entry.as_ref(),
        );
        match result {
            Ok(()) => results.push(CopyProjectResult {
                project_path: target_path.clone(),
                success: true,
                error: None,
            }),
            Err(e) => results.push(CopyProjectResult {
                project_path: target_path.clone(),
                success: false,
                error: Some(e.to_string()),
            }),
        }
    }

    Ok(CopySkillResult { results })
}

fn copy_to_single_project(
    skill_name: &str,
    source_canonical: &std::path::Path,
    target_path: &str,
    agent_types: &[AgentType],
    source_lock_entry: Option<&crate::core::local_lock::LocalSkillLockEntry>,
) -> Result<(), AppError> {
    // 安装 skill 到目标项目的 agents
    let per_agent_results = install_skill_to_agents(
        source_canonical,
        skill_name,
        agent_types,
        &Scope::Project,
        Some(target_path),
        &InstallMode::Symlink,
    );

    // 检查是否至少有一个成功
    let any_success = per_agent_results.iter().any(|r| r.success);
    if !any_success {
        let errors: Vec<String> = per_agent_results
            .iter()
            .filter_map(|r| r.error.clone())
            .collect();
        return Err(AppError::InstallFailed {
            message: errors.join("; "),
        });
    }

    // 写入目标项目的 local lock
    if let Some(entry) = source_lock_entry {
        // 重新计算目标项目的本地哈希
        let target_canonical = canonical_skills_dir(false, target_path)
            .join(crate::core::skill::sanitize_name(skill_name));
        let computed_hash = compute_skill_folder_hash(&target_canonical).unwrap_or_default();

        let mut new_entry = entry.clone();
        new_entry.computed_hash = computed_hash;

        let _ = add_skill_to_local_lock(skill_name, new_entry, target_path);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    fn setup_source_skill(project: &std::path::Path, name: &str) {
        let canonical = project.join(".agents").join("skills").join(name);
        fs::create_dir_all(&canonical).unwrap();
        fs::write(
            canonical.join("SKILL.md"),
            format!("---\nname: {}\ndescription: test\n---\n", name),
        ).unwrap();
    }

    #[test]
    fn test_copy_skill_returns_error_for_missing_source() {
        let source = tempdir().unwrap();
        let target = tempdir().unwrap();

        let result = copy_skill_to_projects(
            "nonexistent".to_string(),
            source.path().to_string_lossy().to_string(),
            vec![target.path().to_string_lossy().to_string()],
            vec![],
        );

        assert!(result.is_err());
    }

    #[test]
    fn test_copy_skill_copies_to_target_canonical() {
        let source = tempdir().unwrap();
        let target = tempdir().unwrap();
        setup_source_skill(source.path(), "my-skill");

        let result = copy_skill_to_projects(
            "my-skill".to_string(),
            source.path().to_string_lossy().to_string(),
            vec![target.path().to_string_lossy().to_string()],
            vec!["cursor".to_string()],
        ).unwrap();

        assert_eq!(result.results.len(), 1);
        assert!(result.results[0].success, "copy should succeed: {:?}", result.results[0].error);

        // 目标项目的 canonical dir 应该存在
        let target_canonical = target.path().join(".agents").join("skills").join("my-skill");
        assert!(target_canonical.join("SKILL.md").exists());

        // 源项目的 canonical dir 不应受影响
        let source_canonical = source.path().join(".agents").join("skills").join("my-skill");
        assert!(source_canonical.join("SKILL.md").exists());
    }

    #[test]
    fn test_copy_skill_overwrites_existing_in_target() {
        let source = tempdir().unwrap();
        let target = tempdir().unwrap();
        setup_source_skill(source.path(), "my-skill");

        // 在目标项目先安装一个旧版本
        let target_canonical = target.path().join(".agents").join("skills").join("my-skill");
        fs::create_dir_all(&target_canonical).unwrap();
        fs::write(target_canonical.join("SKILL.md"), "old content").unwrap();
        fs::write(target_canonical.join("old-file.txt"), "should be gone").unwrap();

        let result = copy_skill_to_projects(
            "my-skill".to_string(),
            source.path().to_string_lossy().to_string(),
            vec![target.path().to_string_lossy().to_string()],
            vec!["cursor".to_string()],
        ).unwrap();

        assert!(result.results[0].success);
        // 新内容应覆盖旧内容
        let content = fs::read_to_string(target_canonical.join("SKILL.md")).unwrap();
        assert!(content.contains("name: my-skill"), "should have new content");
        // 旧文件应被清理
        assert!(!target_canonical.join("old-file.txt").exists());
    }

    #[test]
    fn test_copy_skill_multiple_targets() {
        let source = tempdir().unwrap();
        let target_a = tempdir().unwrap();
        let target_b = tempdir().unwrap();
        setup_source_skill(source.path(), "my-skill");

        let result = copy_skill_to_projects(
            "my-skill".to_string(),
            source.path().to_string_lossy().to_string(),
            vec![
                target_a.path().to_string_lossy().to_string(),
                target_b.path().to_string_lossy().to_string(),
            ],
            vec!["cursor".to_string()],
        ).unwrap();

        assert_eq!(result.results.len(), 2);
        assert!(result.results.iter().all(|r| r.success));
    }
}

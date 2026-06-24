//! 复制 skill 到其他项目
//!
//! 将项目级 skill 复制到其他项目，保持相同的 agent 配置。
//! 复用 installer 的 install_skill_to_agents 完成实际安装。

use crate::core::agent_availability::{
    availability_for_agent, default_available_agents, AgentAvailabilityKind,
};
use crate::core::agents::AgentType;
use crate::core::installer::{install_skill_to_agent_groups, PerAgentInstallResult};
use crate::core::local_lock::{
    add_skill_to_local_lock, compute_skill_folder_hash, read_local_lock,
};
use crate::core::paths::canonical_skills_dir;
use crate::core::skill::sanitize_name;
use crate::error::AppError;
use crate::models::{InstallMode, Scope};
use std::collections::HashSet;

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
            let exists = canonical_skills_dir(false, &path).join(&sanitized).exists();
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
    /// 默认可用的 agents
    #[serde(default)]
    pub default_available_agents: Vec<String>,
    /// 需要单独适配且写入成功的 agents
    #[serde(default)]
    pub private_adapted_agents: Vec<String>,
    /// 明确写入独立副本的 agents
    #[serde(default)]
    pub private_copy_agents: Vec<String>,
    /// 因目标项目缺少 agent 根目录而跳过的 agents
    #[serde(default)]
    pub skipped_agents: Vec<String>,
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
    private_copy_agents: Vec<String>,
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
    let agent_types = parse_agent_ids(&agents);
    let private_copy_agent_types = parse_agent_ids(&private_copy_agents);

    // 4. 对每个目标项目执行复制
    let mut results = Vec::with_capacity(target_project_paths.len());

    for target_path in &target_project_paths {
        let result = copy_to_single_project(
            &skill_name,
            &source_canonical,
            target_path,
            &agent_types,
            &private_copy_agent_types,
            source_lock_entry.as_ref(),
        );
        match result {
            Ok(project_result) => results.push(project_result),
            Err(e) => results.push(CopyProjectResult {
                project_path: target_path.clone(),
                success: false,
                error: Some(e.to_string()),
                default_available_agents: Vec::new(),
                private_adapted_agents: Vec::new(),
                private_copy_agents: Vec::new(),
                skipped_agents: Vec::new(),
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
    private_copy_agent_types: &[AgentType],
    source_lock_entry: Option<&crate::core::local_lock::LocalSkillLockEntry>,
) -> Result<CopyProjectResult, AppError> {
    let default_available = default_available_agents(false, target_path);
    let private_required_agents = agent_types
        .iter()
        .copied()
        .filter(|agent| {
            availability_for_agent(*agent, false, target_path).kind
                == AgentAvailabilityKind::PrivateRequired
        })
        .collect::<Vec<_>>();
    let private_copy_agents = private_copy_agent_types
        .iter()
        .copied()
        .filter(|agent| {
            availability_for_agent(*agent, false, target_path).kind
                == AgentAvailabilityKind::SharedCompatible
        })
        .collect::<Vec<_>>();
    let include_canonical_result = !default_available.is_empty();

    // 安装 skill 到目标项目的 canonical 和需要单独适配的 agents。
    let per_agent_results = install_skill_to_agent_groups(
        source_canonical,
        skill_name,
        &private_required_agents,
        &private_copy_agents,
        &Scope::Project,
        Some(target_path),
        &InstallMode::Symlink,
        include_canonical_result,
    );

    let failed_results = per_agent_results
        .iter()
        .filter(|result| !result.success)
        .collect::<Vec<_>>();
    if !failed_results.is_empty() {
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

    Ok(build_copy_project_result(
        target_path,
        &default_available,
        &private_required_agents,
        &private_copy_agents,
        &per_agent_results,
    ))
}

fn parse_agent_ids(agent_ids: &[String]) -> Vec<AgentType> {
    let mut agents = Vec::new();
    for agent_id in agent_ids {
        if let Ok(agent) = agent_id.parse::<AgentType>() {
            if !agents.contains(&agent) {
                agents.push(agent);
            }
        }
    }
    agents
}

fn build_copy_project_result(
    target_path: &str,
    default_available_agents: &[AgentType],
    private_required_agents: &[AgentType],
    private_copy_agents: &[AgentType],
    per_agent_results: &[PerAgentInstallResult],
) -> CopyProjectResult {
    let skipped_agents = per_agent_results
        .iter()
        .filter(|result| result.skipped)
        .map(|result| result.agent.clone())
        .collect::<Vec<_>>();
    let successful_agents = per_agent_results
        .iter()
        .filter(|result| result.success && !result.skipped)
        .map(|result| result.agent.clone())
        .collect::<HashSet<_>>();

    CopyProjectResult {
        project_path: target_path.to_string(),
        success: per_agent_results
            .iter()
            .any(|result| result.success && !result.skipped)
            || per_agent_results
                .iter()
                .any(|result| result.agent == "__canonical__" && result.success),
        error: None,
        default_available_agents: default_available_agents
            .iter()
            .map(ToString::to_string)
            .collect(),
        private_adapted_agents: private_required_agents
            .iter()
            .filter(|agent| successful_agents.contains(&agent.to_string()))
            .map(ToString::to_string)
            .collect(),
        private_copy_agents: private_copy_agents
            .iter()
            .filter(|agent| successful_agents.contains(&agent.to_string()))
            .map(ToString::to_string)
            .collect(),
        skipped_agents,
    }
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
        )
        .unwrap();
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
            vec![],
        )
        .unwrap();

        assert_eq!(result.results.len(), 1);
        assert!(
            result.results[0].success,
            "copy should succeed: {:?}",
            result.results[0].error
        );

        // 目标项目的 canonical dir 应该存在
        let target_canonical = target
            .path()
            .join(".agents")
            .join("skills")
            .join("my-skill");
        assert!(target_canonical.join("SKILL.md").exists());

        // 源项目的 canonical dir 不应受影响
        let source_canonical = source
            .path()
            .join(".agents")
            .join("skills")
            .join("my-skill");
        assert!(source_canonical.join("SKILL.md").exists());
    }

    #[test]
    fn test_copy_skill_overwrites_existing_in_target() {
        let source = tempdir().unwrap();
        let target = tempdir().unwrap();
        setup_source_skill(source.path(), "my-skill");

        // 在目标项目先安装一个旧版本
        let target_canonical = target
            .path()
            .join(".agents")
            .join("skills")
            .join("my-skill");
        fs::create_dir_all(&target_canonical).unwrap();
        fs::write(target_canonical.join("SKILL.md"), "old content").unwrap();
        fs::write(target_canonical.join("old-file.txt"), "should be gone").unwrap();

        let result = copy_skill_to_projects(
            "my-skill".to_string(),
            source.path().to_string_lossy().to_string(),
            vec![target.path().to_string_lossy().to_string()],
            vec!["cursor".to_string()],
            vec![],
        )
        .unwrap();

        assert!(result.results[0].success);
        // 新内容应覆盖旧内容
        let content = fs::read_to_string(target_canonical.join("SKILL.md")).unwrap();
        assert!(
            content.contains("name: my-skill"),
            "should have new content"
        );
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
            vec![],
        )
        .unwrap();

        assert_eq!(result.results.len(), 2);
        assert!(result.results.iter().all(|r| r.success));
    }

    #[test]
    fn test_copy_skill_reports_default_private_and_skipped_targets() {
        let source = tempdir().unwrap();
        let target = tempdir().unwrap();
        setup_source_skill(source.path(), "my-skill");

        let result = copy_skill_to_projects(
            "my-skill".to_string(),
            source.path().to_string_lossy().to_string(),
            vec![target.path().to_string_lossy().to_string()],
            vec!["antigravity".to_string(), "kiro-cli".to_string()],
            vec![],
        )
        .unwrap();

        assert_eq!(result.results.len(), 1);
        let project_result = &result.results[0];
        assert!(project_result.success);
        assert!(project_result
            .default_available_agents
            .contains(&"antigravity".to_string()));
        assert!(project_result.private_adapted_agents.is_empty());
        assert!(project_result.private_copy_agents.is_empty());
        assert!(project_result
            .skipped_agents
            .iter()
            .any(|agent| agent == "kiro-cli"));
    }
}

//! 安装相关的 Tauri Commands
//!
//! 提供两个命令：
//! - fetch_available: 从来源获取可用的 skills 列表
//! - install_skills: 安装选中的 skills

use crate::core::agents::AgentType;
use crate::core::local_lock::{add_skill_to_local_lock, compute_skill_folder_hash, LocalSkillLockEntry};
use crate::core::skill_lock::{add_skill_to_lock, save_selected_agents};
use crate::core::{
    clone_repo_with_progress, discover_skills, fetch_skill_folder_hash, get_owner_repo,
    install_skill_for_agent, parse_source, CloneProgress, DiscoverOptions,
};
use crate::error::AppError;
use crate::models::{
    AvailableSkill, FetchResult, InstallParams, InstallResults, SourceType,
};
use tauri::{AppHandle, Emitter};

/// 安装进度事件（发送到前端）
#[derive(serde::Serialize, Clone)]
struct InstallProgress {
    /// 当前阶段: "installing" | "writing_lock"
    phase: String,
    /// 当前正在处理的 skill 名称
    current_skill: String,
    /// 已完成的 skill 数量
    completed: usize,
    /// 总 skill 数量
    total: usize,
}

/// 从来源获取可用的 skills 列表
///
/// # Arguments
/// * `source` - 来源字符串（支持 9 种格式）
///
/// # Returns
/// * `FetchResult` - 包含来源信息和可用 skills 列表
#[tauri::command]
#[specta::specta]
pub async fn fetch_available(app: AppHandle, source: String) -> Result<FetchResult, AppError> {
    fetch_available_inner(&app, &source)
}

fn fetch_available_inner(app: &AppHandle, source: &str) -> Result<FetchResult, AppError> {
    // 1. 解析来源
    let parsed = parse_source(source)?;

    // 2. 确定 skills 目录
    let (skills_dir, _clone_result) = match parsed.source_type {
        SourceType::Local => {
            let path = parsed
                .local_path
                .as_ref()
                .ok_or_else(|| AppError::InvalidSource { value: "Missing local path".to_string() })?;
            (path.clone(), None)
        }
        SourceType::GitHub | SourceType::GitLab | SourceType::Git => {
            // 克隆仓库（带进度事件）
            let app_clone = app.clone();
            let clone_result = clone_repo_with_progress(
                &parsed.url,
                parsed.git_ref.as_deref(),
                move |progress: CloneProgress| {
                    // 发送进度事件到前端
                    let _ = app_clone.emit("clone-progress", &progress);
                },
            )?;
            let repo_path = clone_result.repo_path.clone();
            (repo_path, Some(clone_result))
        }
        SourceType::DirectUrl | SourceType::WellKnown => {
            // 这些类型需要特殊处理，暂时返回空列表
            return Ok(FetchResult {
                source_type: parsed.source_type.to_string(),
                source_url: parsed.url.clone(),
                skill_filter: parsed.skill_filter.clone(),
                skills: vec![],
            });
        }
    };

    // 3. 发现并构建结果（复用纯逻辑函数）
    discover_and_build_result(&parsed, &skills_dir)
}

/// 从已有的 skills 目录发现 skills 并构建 FetchResult
///
/// 抽取为独立函数，不依赖 AppHandle，便于单元测试
fn discover_and_build_result(
    parsed: &crate::models::ParsedSource,
    skills_dir: &std::path::Path,
) -> Result<FetchResult, AppError> {
    // 如果有 @skill 语法，包含 internal skills（用户明确请求）
    let include_internal = parsed.skill_filter.is_some();
    let options = DiscoverOptions {
        include_internal,
        full_depth: false,
    };

    let discovered = discover_skills(skills_dir, parsed.subpath.as_deref(), options)?;

    let skills: Vec<AvailableSkill> = discovered.into_iter().map(|s| s.into()).collect();

    Ok(FetchResult {
        source_type: parsed.source_type.to_string(),
        source_url: parsed.url.clone(),
        skill_filter: parsed.skill_filter.clone(),
        skills,
    })
}

/// 安装选中的 skills
///
/// # Arguments
/// * `params` - 安装参数（来源、选中的 skills、agents、scope、mode）
///
/// # Returns
/// * `InstallResults` - 安装结果汇总
#[tauri::command]
#[specta::specta]
pub async fn install_skills(app: AppHandle, params: InstallParams) -> Result<InstallResults, AppError> {
    install_skills_inner(&app, params).await
}

async fn install_skills_inner(app: &AppHandle, params: InstallParams) -> Result<InstallResults, AppError> {
    // 1. 解析来源
    let parsed = parse_source(&params.source)?;

    // 2. 克隆或获取本地路径
    let (skills_dir, _clone_result) = match parsed.source_type {
        SourceType::Local => {
            let path = parsed
                .local_path
                .as_ref()
                .ok_or_else(|| AppError::InvalidSource { value: "Missing local path".to_string() })?;
            (path.clone(), None)
        }
        _ => {
            let app_clone = app.clone();
            let clone_result = clone_repo_with_progress(
                &parsed.url,
                parsed.git_ref.as_deref(),
                move |progress: CloneProgress| {
                    let _ = app_clone.emit("clone-progress", &progress);
                },
            )?;
            let repo_path = clone_result.repo_path.clone();
            (repo_path, Some(clone_result))
        }
    };

    // 3. 发现所有 skills
    let options = DiscoverOptions {
        include_internal: true, // 安装时包含 internal（用户已明确选择）
        full_depth: false,
    };
    let discovered = discover_skills(&skills_dir, parsed.subpath.as_deref(), options)?;

    // 4. 过滤用户选择的 skills
    let selected_skills: Vec<_> = discovered
        .into_iter()
        .filter(|s| params.skills.contains(&s.name))
        .collect();

    if selected_skills.is_empty() {
        return Err(AppError::NoSkillsFound);
    }

    // 5. 确保包含 Universal Agents（动态获取）
    let mut target_agents = params.agents.clone();
    let universal_agents = AgentType::get_universal_agents();

    for ua in universal_agents {
        let ua_str = ua.to_string();
        if !target_agents.contains(&ua_str) {
            target_agents.push(ua_str);
        }
    }

    // 6. 执行安装
    let mut successful = Vec::new();
    let mut failed = Vec::new();
    let mut symlink_fallback_agents = Vec::new();
    let total_skills = selected_skills.len();

    for (idx, skill) in selected_skills.iter().enumerate() {
        // 发送安装进度事件
        let _ = app.emit("install-progress", &InstallProgress {
            phase: "installing".to_string(),
            current_skill: skill.name.clone(),
            completed: idx,
            total: total_skills,
        });

        for agent_str in &target_agents {
            let agent: AgentType = agent_str
                .parse()
                .map_err(|_| AppError::InvalidAgent { agent: agent_str.clone() })?;

            let result = install_skill_for_agent(
                &skill.path,
                &skill.name,
                &agent,
                &params.scope,
                params.project_path.as_deref(),
                &params.mode,
            );

            if result.success {
                if result.symlink_failed && !symlink_fallback_agents.contains(agent_str) {
                    symlink_fallback_agents.push(agent_str.clone());
                }
                successful.push(result);
            } else {
                failed.push(result);
            }
        }
    }

    // 7. 写入 lock 文件
    if !successful.is_empty() {
        let _ = app.emit("install-progress", &InstallProgress {
            phase: "writing_lock".to_string(),
            current_skill: String::new(),
            completed: total_skills,
            total: total_skills,
        });

        let owner_repo = get_owner_repo(&parsed);

        for skill in &selected_skills {
            let installed = successful.iter().any(|r| r.skill_name == skill.name);
            if !installed {
                continue;
            }

            // 获取 skill folder hash（仅 GitHub 来源）
            let skill_folder_hash = if parsed.source_type == SourceType::GitHub {
                if let Some(ref repo) = owner_repo {
                    fetch_skill_folder_hash(repo, &skill.relative_path, None)
                        .await
                        .unwrap_or(None)
                        .unwrap_or_default()
                } else {
                    String::new()
                }
            } else {
                String::new()
            };

            let source = owner_repo.as_deref().unwrap_or(&params.source);
            let source_type_str = &parsed.source_type.to_string();
            let source_url = &parsed.url;
            let skill_path = Some(skill.relative_path.as_str());

            // 根据 scope 写入对应的 lock 文件
            match params.scope {
                crate::models::Scope::Global => {
                    let _ = add_skill_to_lock(
                        &skill.name, source, source_type_str, source_url,
                        skill_path, &skill_folder_hash,
                        skill.plugin_name.as_deref(),
                    );
                }
                crate::models::Scope::Project => {
                    if let Some(ref project_path) = params.project_path {
                        // 计算安装后的本地文件 SHA-256
                        let install_dir = crate::core::paths::canonical_skills_dir(false, project_path)
                            .join(crate::core::skill::sanitize_name(&skill.name));
                        let computed_hash = compute_skill_folder_hash(&install_dir)
                            .unwrap_or_default();

                        let entry = LocalSkillLockEntry {
                            source: source.to_string(),
                            source_type: source_type_str.to_string(),
                            computed_hash,
                            remote_hash: if skill_folder_hash.is_empty() {
                                None
                            } else {
                                Some(skill_folder_hash.clone())
                            },
                            skill_path: skill_path.map(|s| s.to_string()),
                            plugin_name: skill.plugin_name.clone(),
                        };
                        let _ = add_skill_to_local_lock(&skill.name, entry, project_path);
                    }
                }
            }
        }
    }

    // 8. 保存选择的 agents
    let _ = save_selected_agents(&target_agents);

    Ok(InstallResults {
        successful,
        failed,
        symlink_fallback_agents,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_fetch_available_local() {
        let temp = tempdir().unwrap();
        let skill_dir = temp.path().join("my-skill");
        fs::create_dir_all(&skill_dir).unwrap();

        let skill_md = skill_dir.join("SKILL.md");
        fs::write(
            &skill_md,
            "---\nname: test-skill\ndescription: A test skill\n---\n",
        )
        .unwrap();

        let source = temp.path().to_string_lossy().to_string();
        let parsed = parse_source(&source).unwrap();
        let result = discover_and_build_result(&parsed, temp.path()).unwrap();

        assert_eq!(result.source_type, "local");
        assert_eq!(result.skills.len(), 1);
        assert_eq!(result.skills[0].name, "test-skill");
    }

    #[test]
    fn test_fetch_available_with_skill_filter() {
        let temp = tempdir().unwrap();

        // 创建一个普通 skill
        let skill_dir = temp.path().join("normal-skill");
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: normal\ndescription: Normal skill\n---\n",
        )
        .unwrap();

        // 创建一个 internal skill
        let internal_dir = temp.path().join("internal-skill");
        fs::create_dir_all(&internal_dir).unwrap();
        fs::write(
            internal_dir.join("SKILL.md"),
            "---\nname: internal\ndescription: Internal skill\nmetadata:\n  internal: true\n---\n",
        )
        .unwrap();

        // 不带 @skill 语法，不应包含 internal
        let source = temp.path().to_string_lossy().to_string();
        let parsed = parse_source(&source).unwrap();
        let result = discover_and_build_result(&parsed, temp.path()).unwrap();
        assert_eq!(result.skills.len(), 1);
        assert_eq!(result.skills[0].name, "normal");
    }
}

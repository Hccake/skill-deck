//! 更新检测相关的 Tauri Commands
//!
//! 提供命令：
//! - check_updates: 检测指定 scope 的 skills 是否有更新

use crate::core::agents::AgentType;
use crate::core::fetch_skill_folder_hash;
use crate::core::skill_lock::{
    add_skill_to_lock, add_skill_to_scoped_lock, read_scoped_lock,
};
use crate::core::{
    clone_repo_with_progress, discover_skills, install_skill_for_agent,
    parse_source, CloneProgress, DiscoverOptions,
};
use crate::models::{InstallMode, Scope};
use serde::Serialize;
use specta::Type;
use std::collections::HashMap;
use crate::error::AppError;

/// 更新检测结果
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct SkillUpdateInfo {
    pub name: String,
    pub source: String,
    pub has_update: bool,
}

/// 检测指定 scope 的 skills 是否有更新
///
/// 流程：
/// 1. 读取对应 scope 的 .skill-lock.json
/// 2. 过滤出 sourceType == "github" 且有 skillFolderHash 和 skillPath 的 skills
/// 3. 按 source 分组，对每组调用 GitHub Trees API
/// 4. 比对本地 hash 与远程 hash
#[tauri::command]
#[specta::specta]
pub async fn check_updates(
    scope: Scope,
    project_path: Option<String>,
) -> Result<Vec<SkillUpdateInfo>, AppError> {
    check_updates_inner(scope, project_path.as_deref()).await
}

async fn check_updates_inner(
    scope: Scope,
    project_path: Option<&str>,
) -> Result<Vec<SkillUpdateInfo>, AppError> {
    // 1. 确定 lock 文件路径
    let lock_project_path = match scope {
        Scope::Global => None,
        Scope::Project => project_path,
    };

    // 2. 读取 lock 文件
    let lock = read_scoped_lock(lock_project_path)?;

    // 3. 过滤并按 source 分组
    // value: Vec<(skill_name, skill_path, local_hash)>
    let mut skills_by_source: HashMap<String, Vec<(String, String, String)>> = HashMap::new();

    for (name, entry) in &lock.skills {
        if entry.source_type != "github" {
            continue;
        }
        if entry.skill_folder_hash.is_empty() {
            continue;
        }
        let skill_path = match &entry.skill_path {
            Some(p) if !p.is_empty() => p.clone(),
            _ => continue,
        };

        skills_by_source
            .entry(entry.source.clone())
            .or_default()
            .push((name.clone(), skill_path, entry.skill_folder_hash.clone()));
    }

    // 4. 对每组 source 调用 GitHub Trees API
    let mut results = Vec::new();

    for (source, skills) in &skills_by_source {
        for (name, skill_path, local_hash) in skills {
            match fetch_skill_folder_hash(source, skill_path, None).await {
                Ok(Some(remote_hash)) => {
                    results.push(SkillUpdateInfo {
                        name: name.clone(),
                        source: source.clone(),
                        has_update: remote_hash != *local_hash,
                    });
                }
                Ok(None) => {
                    // 远程找不到，不误报
                    results.push(SkillUpdateInfo {
                        name: name.clone(),
                        source: source.clone(),
                        has_update: false,
                    });
                }
                Err(_) => {
                    // API 失败，静默跳过
                }
            }
        }
    }

    Ok(results)
}

/// 更新指定 skill
///
/// 本质是"重新安装"：从 lock 文件读取来源信息，构造安装 URL，复用安装逻辑。
/// 与 CLI update 命令行为一致。
#[tauri::command]
#[specta::specta]
pub async fn update_skill(
    app: tauri::AppHandle,
    scope: Scope,
    name: String,
    project_path: Option<String>,
) -> Result<(), AppError> {
    update_skill_inner(&app, scope, &name, project_path.as_deref()).await
}

async fn update_skill_inner(
    app: &tauri::AppHandle,
    scope: Scope,
    skill_name: &str,
    project_path: Option<&str>,
) -> Result<(), AppError> {
    use tauri::Emitter;

    // 1. 从 lock 文件读取 skill 的来源信息
    let lock_project_path = match scope {
        Scope::Global => None,
        Scope::Project => project_path,
    };
    let lock = read_scoped_lock(lock_project_path)?;
    let entry = lock.skills.get(skill_name).ok_or_else(|| {
        AppError::InvalidSource {
            value: format!("Skill '{}' not found in lock file", skill_name),
        }
    })?;
    // Clone needed fields before moving
    let entry_source = entry.source.clone();
    let entry_source_type = entry.source_type.clone();
    let entry_source_url = entry.source_url.clone();
    let entry_skill_path = entry.skill_path.clone();

    // 2. 构造安装 URL（与 CLI runUpdate 逻辑一致）
    let install_url = build_install_url(entry);

    // 3. 解析来源
    let parsed = parse_source(&install_url)?;

    // 4. 克隆仓库
    let app_clone = app.clone();
    let clone_result = clone_repo_with_progress(
        &parsed.url,
        parsed.git_ref.as_deref(),
        move |progress: CloneProgress| {
            let _ = app_clone.emit("clone-progress", &progress);
        },
    )?;

    // 5. 发现 skills
    let options = DiscoverOptions {
        include_internal: true,
        full_depth: false,
    };
    let discovered = discover_skills(&clone_result.repo_path, parsed.subpath.as_deref(), options)?;

    // 6. 找到目标 skill
    let skill = discovered
        .iter()
        .find(|s| s.name == skill_name)
        .ok_or_else(|| AppError::NoSkillsFound)?;

    // 7. 检测已安装的 agents + universal agents
    let mut target_agents = AgentType::detect_installed();
    let universal_agents = AgentType::get_universal_agents();
    for ua in universal_agents {
        if !target_agents.contains(&ua) {
            target_agents.push(ua);
        }
    }

    // 8. 执行安装（覆盖现有文件）
    let install_scope = match scope {
        Scope::Global => crate::models::Scope::Global,
        Scope::Project => crate::models::Scope::Project,
    };
    for agent in &target_agents {
        let _ = install_skill_for_agent(
            &skill.path,
            &skill.name,
            agent,
            &install_scope,
            project_path,
            &InstallMode::Symlink,
        );
    }

    // 9. 更新 lock 文件（获取新的 hash）
    let new_hash = if entry_source_type == "github" {
        fetch_skill_folder_hash(
            &entry_source,
            entry_skill_path.as_deref().unwrap_or(""),
            None,
        )
        .await
        .unwrap_or(None)
        .unwrap_or_default()
    } else {
        String::new()
    };

    match scope {
        Scope::Global => {
            let _ = add_skill_to_lock(
                skill_name,
                &entry_source,
                &entry_source_type,
                &entry_source_url,
                entry_skill_path.as_deref(),
                &new_hash,
            );
        }
        Scope::Project => {
            let _ = add_skill_to_scoped_lock(
                skill_name,
                &entry_source,
                &entry_source_type,
                &entry_source_url,
                entry_skill_path.as_deref(),
                &new_hash,
                project_path,
            );
        }
    }

    Ok(())
}

/// 从 lock entry 构造安装 URL
///
/// 与 CLI cli.ts runUpdate() 中构造 installUrl 的逻辑一致：
/// 1. 基础 URL = entry.sourceUrl
/// 2. 如果有 skillPath，去掉 SKILL.md 后缀，拼接为 GitHub tree URL
fn build_install_url(entry: &crate::core::skill_lock::SkillLockEntry) -> String {
    let mut install_url = entry.source_url.clone();

    if let Some(ref skill_path) = entry.skill_path {
        let mut skill_folder = skill_path.clone();

        // 去掉 /SKILL.md 或 SKILL.md 后缀
        if skill_folder.ends_with("/SKILL.md") {
            skill_folder = skill_folder[..skill_folder.len() - 9].to_string();
        } else if skill_folder.ends_with("SKILL.md") {
            skill_folder = skill_folder[..skill_folder.len() - 8].to_string();
        }

        // 去掉尾部斜杠
        skill_folder = skill_folder.trim_end_matches('/').to_string();

        if !skill_folder.is_empty() {
            // 去掉 sourceUrl 的 .git 后缀和尾部斜杠
            install_url = install_url
                .trim_end_matches(".git")
                .trim_end_matches('/')
                .to_string();

            // 拼接 GitHub tree URL（硬编码 main 分支，与 CLI 一致）
            install_url = format!("{}/tree/main/{}", install_url, skill_folder);
        }
    }

    install_url
}

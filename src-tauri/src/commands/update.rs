//! 更新检测相关的 Tauri Commands
//!
//! 提供命令：
//! - check_updates: 检测指定 scope 的 skills 是否有更新

use crate::core::agents::AgentType;
use crate::core::{fetch_skill_folder_hash, fetch_skill_folder_hashes_batch};
use crate::core::installer::{detect_install_mode, detect_installed_agents_for_skill, install_skill_to_agents};
use crate::core::local_lock::{
    add_skill_to_local_lock, compute_skill_folder_hash, read_local_lock, LocalSkillLockEntry,
};
use crate::core::skill_lock::{add_skill_to_lock, read_scoped_lock, SkillLockFile};
use crate::core::{
    clone_repo_with_progress, discover_skills, parse_source,
    CloneProgress, DiscoverOptions,
};
use crate::error::AppError;
use crate::models::{
    InstallMode, Scope, UpdateSkillAgentResult, UpdateSkillAgentStatus, UpdateSkillItemResult,
    UpdateSkillResponse, UpdateSkillStatus, UpdateSkillSummary,
};
use serde::Serialize;
use specta::Type;
use std::collections::HashMap;
use std::time::Instant;

/// 更新进度事件（发送到前端）
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct UpdateProgress {
    skill_name: String,
    /// "cloning" | "installing" | "writing_lock"
    phase: String,
}

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
    // 1-2. 根据 scope 读取对应的 lock 文件
    let lock = match scope {
        Scope::Global => read_scoped_lock(None)?,
        Scope::Project => {
            if let Some(pp) = project_path {
                let local_lock = read_local_lock(pp)?;
                // 转换 LocalSkillLockFile -> SkillLockFile 用于统一流程
                let mut skills = HashMap::new();
                for (name, entry) in local_lock.skills {
                    let source = entry.source.clone();
                    let source_type = entry.source_type.clone();
                    let source_url = entry.source_url.unwrap_or_else(|| {
                        if source_type == "github" {
                            format!("https://github.com/{}", source)
                        } else {
                            source.clone()
                        }
                    });
                    skills.insert(
                        name,
                        crate::core::skill_lock::SkillLockEntry {
                            source,
                            source_type,
                            source_url,
                            skill_path: entry.skill_path,
                            skill_folder_hash: entry.remote_hash.unwrap_or_default(),
                            installed_at: String::new(),
                            updated_at: String::new(),
                            plugin_name: entry.plugin_name,
                        },
                    );
                }
                SkillLockFile {
                    version: 3,
                    skills,
                    dismissed: None,
                    last_selected_agents: None,
                }
            } else {
                read_scoped_lock(None)?
            }
        }
    };

    // 3. 过滤并按 source 分组
    // value: Vec<(skill_name, skill_path, local_hash)>
    let mut skills_by_source: HashMap<String, Vec<(String, String, String)>> = HashMap::new();

    for (name, entry) in &lock.skills {
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

    // 4. 对每组 source 批量查询 hash（单次 API 请求 per source）
    let mut results = Vec::new();

    for (source, skills) in &skills_by_source {
        let paths: Vec<(String, String)> = skills
            .iter()
            .map(|(name, skill_path, _)| (name.clone(), skill_path.clone()))
            .collect();
        match fetch_skill_folder_hashes_batch(source, &paths, None).await {
            Ok(hashes) => {
                for (name, _, local_hash) in skills {
                    let has_update = hashes
                        .get(name)
                        .and_then(|h| h.as_ref())
                        .map(|remote| remote != local_hash)
                        .unwrap_or(false);
                    results.push(SkillUpdateInfo {
                        name: name.clone(),
                        source: source.clone(),
                        has_update,
                    });
                }
            }
            Err(_) => {
                // API 失败，不误报
                for (name, _, _) in skills {
                    results.push(SkillUpdateInfo {
                        name: name.clone(),
                        source: source.clone(),
                        has_update: false,
                    });
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
) -> Result<UpdateSkillResponse, AppError> {
    Ok(update_skill_inner(&app, scope, &name, project_path.as_deref()).await)
}

async fn update_skill_inner(
    app: &tauri::AppHandle,
    scope: Scope,
    skill_name: &str,
    project_path: Option<&str>,
) -> UpdateSkillResponse {
    let start = Instant::now();

    let mut item = match update_skill_single(app, scope, skill_name, project_path).await {
        Ok(item) => item,
        Err(err) => UpdateSkillItemResult {
            name: skill_name.to_string(),
            status: UpdateSkillStatus::Failed,
            error: Some(err.to_string()),
            warnings: Vec::new(),
            duration_ms: None,
            agent_results: Vec::new(),
        },
    };
    item.duration_ms = Some(elapsed_ms(&start));

    let results = vec![item];
    UpdateSkillResponse {
        summary: summarize_results(&results),
        results,
    }
}

async fn update_skill_single(
    app: &tauri::AppHandle,
    scope: Scope,
    skill_name: &str,
    project_path: Option<&str>,
) -> Result<UpdateSkillItemResult, AppError> {
    use tauri::Emitter;

    let mut warnings = Vec::new();

    // 1. 根据 scope 读取对应的 lock 文件
    let (entry_source, entry_source_type, entry_source_url, entry_skill_path, entry_plugin_name) =
        match scope {
            Scope::Global => {
                let lock = read_scoped_lock(None)?;
                let entry =
                    lock.skills.get(skill_name).ok_or_else(|| AppError::InvalidSource {
                        value: format!("Skill '{}' not found in lock file", skill_name),
                    })?;
                (
                    entry.source.clone(),
                    entry.source_type.clone(),
                    entry.source_url.clone(),
                    entry.skill_path.clone(),
                    entry.plugin_name.clone(),
                )
            }
            Scope::Project => {
                let pp = project_path.ok_or_else(|| AppError::InvalidSource {
                    value: "Project path is required for project scope".to_string(),
                })?;
                let local_lock = read_local_lock(pp)?;
                let entry = local_lock
                    .skills
                    .get(skill_name)
                    .ok_or_else(|| AppError::InvalidSource {
                        value: format!(
                            "Skill '{}' not found in project lock file",
                            skill_name
                        ),
                    })?;
                let source_url = entry.source_url.clone().unwrap_or_else(|| {
                    if entry.source_type == "github" {
                        format!("https://github.com/{}", entry.source)
                    } else {
                        entry.source.clone()
                    }
                });
                (
                    entry.source.clone(),
                    entry.source_type.clone(),
                    source_url,
                    entry.skill_path.clone(),
                    entry.plugin_name.clone(),
                )
            }
        };

    // 2. 构造安装 URL（与 CLI runUpdate 逻辑一致）
    let install_url = build_install_url_from_parts(
        &entry_source_url,
        entry_skill_path.as_deref(),
    );

    // 3. 解析来源
    let parsed = parse_source(&install_url)?;

    // 4. 克隆仓库
    let _ = app.emit("update-progress", &UpdateProgress {
        skill_name: skill_name.to_string(),
        phase: "cloning".to_string(),
    });
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

    // 7. 检测已安装的 agents（通过文件系统检测，fallback 到 detect_installed + universal）
    let install_scope = match scope {
        Scope::Global => crate::models::Scope::Global,
        Scope::Project => crate::models::Scope::Project,
    };
    let mut target_agents = detect_installed_agents_for_skill(
        skill_name, &install_scope, project_path,
    );
    if target_agents.is_empty() {
        target_agents = AgentType::detect_installed();
        let universal_agents = AgentType::get_universal_agents();
        for ua in universal_agents {
            if !target_agents.contains(&ua) {
                target_agents.push(ua);
            }
        }
    }

    // 8. 检测安装模式（通过文件系统检测）
    let install_mode = if let Some(first_agent) = target_agents.first() {
        detect_install_mode(skill_name, first_agent, &install_scope, project_path)
    } else {
        InstallMode::Symlink
    };

    // 9. 执行安装（覆盖现有文件）
    let _ = app.emit("update-progress", &UpdateProgress {
        skill_name: skill_name.to_string(),
        phase: "installing".to_string(),
    });
    let per_agent_results = install_skill_to_agents(
        &skill.path, &skill.name, &target_agents,
        &install_scope, project_path, &install_mode,
    );
    let agent_results: Vec<UpdateSkillAgentResult> = per_agent_results
        .into_iter()
        .map(|r| UpdateSkillAgentResult {
            agent: r.agent,
            status: if r.success { UpdateSkillAgentStatus::Success } else { UpdateSkillAgentStatus::Failed },
            error: r.error,
            duration_ms: r.duration_ms,
        })
        .collect();

    // 10. 更新 lock 文件（获取新的 hash）
    let _ = app.emit("update-progress", &UpdateProgress {
        skill_name: skill_name.to_string(),
        phase: "writing_lock".to_string(),
    });
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
            if let Err(err) = add_skill_to_lock(
                skill_name,
                &entry_source,
                &entry_source_type,
                &entry_source_url,
                entry_skill_path.as_deref(),
                &new_hash,
                entry_plugin_name.as_deref(),
            ) {
                warnings.push(format!("Failed to write global lock: {}", err));
            }
        }
        Scope::Project => {
            if let Some(pp) = project_path {
                let install_dir = crate::core::paths::canonical_skills_dir(false, pp)
                    .join(crate::core::skill::sanitize_name(skill_name));
                let computed_hash = compute_skill_folder_hash(&install_dir).unwrap_or_default();
                let entry = LocalSkillLockEntry {
                    source: entry_source.clone(),
                    source_type: entry_source_type.clone(),
                    source_url: Some(entry_source_url.clone()),
                    computed_hash,
                    remote_hash: if new_hash.is_empty() {
                        None
                    } else {
                        Some(new_hash.clone())
                    },
                    skill_path: entry_skill_path.clone(),
                    plugin_name: entry_plugin_name.clone(),
                };
                if let Err(err) = add_skill_to_local_lock(skill_name, entry, pp) {
                    warnings.push(format!("Failed to write project lock: {}", err));
                }
            }
        }
    }

    let status = derive_skill_status(&agent_results);
    let error = match status {
        UpdateSkillStatus::Failed | UpdateSkillStatus::Partial => agent_results
            .iter()
            .find(|r| r.status == UpdateSkillAgentStatus::Failed)
            .and_then(|r| r.error.clone())
            .or_else(|| Some("Some agents failed to update".to_string())),
        _ => None,
    };

    Ok(UpdateSkillItemResult {
        name: skill_name.to_string(),
        status,
        error,
        warnings,
        duration_ms: None,
        agent_results,
    })
}

/// 批量更新多个 skills（同源 clone 合并）
///
/// 按 source 分组，每组只 clone 一次仓库，然后从同一 clone 中安装所有该组的 skills。
/// 对于 N 个同源 skills，从 clone N 次降为 clone 1 次。
#[tauri::command]
#[specta::specta]
pub async fn update_skills_batch(
    app: tauri::AppHandle,
    scope: Scope,
    names: Vec<String>,
    project_path: Option<String>,
) -> Result<UpdateSkillResponse, AppError> {
    Ok(update_skills_batch_inner(&app, scope, &names, project_path.as_deref()).await)
}

async fn update_skills_batch_inner(
    app: &tauri::AppHandle,
    scope: Scope,
    names: &[String],
    project_path: Option<&str>,
) -> UpdateSkillResponse {
    use tauri::Emitter;

    let start = Instant::now();

    // 1. 读取 lock 文件，按 source 分组
    struct SkillEntry {
        name: String,
        source: String,
        source_type: String,
        source_url: String,
        skill_path: Option<String>,
        plugin_name: Option<String>,
    }

    let names_set: std::collections::HashSet<&str> = names.iter().map(|s| s.as_str()).collect();
    let mut entries: Vec<SkillEntry> = Vec::new();

    match scope {
        Scope::Global => {
            if let Ok(lock) = read_scoped_lock(None) {
                for (name, entry) in &lock.skills {
                    if names_set.contains(name.as_str()) {
                        entries.push(SkillEntry {
                            name: name.clone(),
                            source: entry.source.clone(),
                            source_type: entry.source_type.clone(),
                            source_url: entry.source_url.clone(),
                            skill_path: entry.skill_path.clone(),
                            plugin_name: entry.plugin_name.clone(),
                        });
                    }
                }
            }
        }
        Scope::Project => {
            if let Some(pp) = project_path {
                if let Ok(local_lock) = read_local_lock(pp) {
                    for (name, entry) in &local_lock.skills {
                        if names_set.contains(name.as_str()) {
                            let source_url = entry.source_url.clone().unwrap_or_else(|| {
                                if entry.source_type == "github" {
                                    format!("https://github.com/{}", entry.source)
                                } else {
                                    entry.source.clone()
                                }
                            });
                            entries.push(SkillEntry {
                                name: name.clone(),
                                source: entry.source.clone(),
                                source_type: entry.source_type.clone(),
                                source_url,
                                skill_path: entry.skill_path.clone(),
                                plugin_name: entry.plugin_name.clone(),
                            });
                        }
                    }
                }
            }
        }
    }

    // 按 source_url 分组
    let mut by_source: HashMap<String, Vec<SkillEntry>> = HashMap::new();
    for entry in entries {
        by_source.entry(entry.source_url.clone()).or_default().push(entry);
    }

    let mut all_results: Vec<UpdateSkillItemResult> = Vec::new();
    let install_scope = match scope {
        Scope::Global => crate::models::Scope::Global,
        Scope::Project => crate::models::Scope::Project,
    };

    // 2. 每组 source 只 clone 一次
    for (source_url, group) in &by_source {
        // 用第一个 entry 构造安装 URL（取顶层 URL，不含 skill 子路径，以获取整个仓库）
        let parsed = match parse_source(source_url) {
            Ok(p) => p,
            Err(err) => {
                // 整组失败
                for entry in group {
                    all_results.push(UpdateSkillItemResult {
                        name: entry.name.clone(),
                        status: UpdateSkillStatus::Failed,
                        error: Some(err.to_string()),
                        warnings: Vec::new(),
                        duration_ms: None,
                        agent_results: Vec::new(),
                    });
                }
                continue;
            }
        };

        // emit cloning progress for first skill in group
        let _ = app.emit("update-progress", &UpdateProgress {
            skill_name: group[0].name.clone(),
            phase: "cloning".to_string(),
        });

        let app_clone = app.clone();
        let clone_result = match clone_repo_with_progress(
            &parsed.url,
            parsed.git_ref.as_deref(),
            move |progress: CloneProgress| {
                let _ = app_clone.emit("clone-progress", &progress);
            },
        ) {
            Ok(r) => r,
            Err(err) => {
                for entry in group {
                    all_results.push(UpdateSkillItemResult {
                        name: entry.name.clone(),
                        status: UpdateSkillStatus::Failed,
                        error: Some(err.to_string()),
                        warnings: Vec::new(),
                        duration_ms: None,
                        agent_results: Vec::new(),
                    });
                }
                continue;
            }
        };

        // discover all skills from the clone
        let options = DiscoverOptions {
            include_internal: true,
            full_depth: false,
        };
        let discovered = match discover_skills(&clone_result.repo_path, parsed.subpath.as_deref(), options) {
            Ok(d) => d,
            Err(err) => {
                for entry in group {
                    all_results.push(UpdateSkillItemResult {
                        name: entry.name.clone(),
                        status: UpdateSkillStatus::Failed,
                        error: Some(err.to_string()),
                        warnings: Vec::new(),
                        duration_ms: None,
                        agent_results: Vec::new(),
                    });
                }
                continue;
            }
        };

        // 3. 逐个安装该组的 skills（共享同一个 clone）
        for entry in group {
            let mut warnings = Vec::new();

            let _ = app.emit("update-progress", &UpdateProgress {
                skill_name: entry.name.clone(),
                phase: "installing".to_string(),
            });

            let skill = match discovered.iter().find(|s| s.name == entry.name) {
                Some(s) => s,
                None => {
                    all_results.push(UpdateSkillItemResult {
                        name: entry.name.clone(),
                        status: UpdateSkillStatus::Failed,
                        error: Some(format!("Skill '{}' not found in cloned repository", entry.name)),
                        warnings: Vec::new(),
                        duration_ms: None,
                        agent_results: Vec::new(),
                    });
                    continue;
                }
            };

            // detect agents
            let mut target_agents = detect_installed_agents_for_skill(
                &entry.name, &install_scope, project_path,
            );
            if target_agents.is_empty() {
                target_agents = AgentType::detect_installed();
                let universal_agents = AgentType::get_universal_agents();
                for ua in universal_agents {
                    if !target_agents.contains(&ua) {
                        target_agents.push(ua);
                    }
                }
            }

            // detect install mode
            let install_mode = if let Some(first_agent) = target_agents.first() {
                detect_install_mode(&entry.name, first_agent, &install_scope, project_path)
            } else {
                InstallMode::Symlink
            };

            // install
            let per_agent_results = install_skill_to_agents(
                &skill.path, &skill.name, &target_agents,
                &install_scope, project_path, &install_mode,
            );
            let agent_results: Vec<UpdateSkillAgentResult> = per_agent_results
                .into_iter()
                .map(|r| UpdateSkillAgentResult {
                    agent: r.agent,
                    status: if r.success { UpdateSkillAgentStatus::Success } else { UpdateSkillAgentStatus::Failed },
                    error: r.error,
                    duration_ms: r.duration_ms,
                })
                .collect();

            // write lock
            let _ = app.emit("update-progress", &UpdateProgress {
                skill_name: entry.name.clone(),
                phase: "writing_lock".to_string(),
            });
            let new_hash = if entry.source_type == "github" {
                fetch_skill_folder_hash(
                    &entry.source,
                    entry.skill_path.as_deref().unwrap_or(""),
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
                    if let Err(err) = add_skill_to_lock(
                        &entry.name,
                        &entry.source,
                        &entry.source_type,
                        &entry.source_url,
                        entry.skill_path.as_deref(),
                        &new_hash,
                        entry.plugin_name.as_deref(),
                    ) {
                        warnings.push(format!("Failed to write global lock: {}", err));
                    }
                }
                Scope::Project => {
                    if let Some(pp) = project_path {
                        let install_dir = crate::core::paths::canonical_skills_dir(false, pp)
                            .join(crate::core::skill::sanitize_name(&entry.name));
                        let computed_hash = compute_skill_folder_hash(&install_dir).unwrap_or_default();
                        let lock_entry = LocalSkillLockEntry {
                            source: entry.source.clone(),
                            source_type: entry.source_type.clone(),
                            source_url: Some(entry.source_url.clone()),
                            computed_hash,
                            remote_hash: if new_hash.is_empty() { None } else { Some(new_hash.clone()) },
                            skill_path: entry.skill_path.clone(),
                            plugin_name: entry.plugin_name.clone(),
                        };
                        if let Err(err) = add_skill_to_local_lock(&entry.name, lock_entry, pp) {
                            warnings.push(format!("Failed to write project lock: {}", err));
                        }
                    }
                }
            }

            let status = derive_skill_status(&agent_results);
            let error = match status {
                UpdateSkillStatus::Failed | UpdateSkillStatus::Partial => agent_results
                    .iter()
                    .find(|r| r.status == UpdateSkillAgentStatus::Failed)
                    .and_then(|r| r.error.clone())
                    .or_else(|| Some("Some agents failed to update".to_string())),
                _ => None,
            };

            all_results.push(UpdateSkillItemResult {
                name: entry.name.clone(),
                status,
                error,
                warnings,
                duration_ms: Some(elapsed_ms(&start)),
                agent_results,
            });
        }
    }

    // 对于不在 lock 文件中的 names，标记为 failed
    let found_names: std::collections::HashSet<String> = all_results.iter().map(|r| r.name.clone()).collect();
    for name in names {
        if !found_names.contains(name) {
            all_results.push(UpdateSkillItemResult {
                name: name.clone(),
                status: UpdateSkillStatus::Failed,
                error: Some(format!("Skill '{}' not found in lock file", name)),
                warnings: Vec::new(),
                duration_ms: None,
                agent_results: Vec::new(),
            });
        }
    }

    UpdateSkillResponse {
        summary: summarize_results(&all_results),
        results: all_results,
    }
}

fn elapsed_ms(start: &Instant) -> u32 {
    let ms = start.elapsed().as_millis();
    if ms > u32::MAX as u128 {
        u32::MAX
    } else {
        ms as u32
    }
}

fn derive_skill_status(agent_results: &[UpdateSkillAgentResult]) -> UpdateSkillStatus {
    if agent_results.is_empty() {
        return UpdateSkillStatus::Skipped;
    }

    let mut success = 0;
    let mut failed = 0;
    for result in agent_results {
        match result.status {
            UpdateSkillAgentStatus::Success => success += 1,
            UpdateSkillAgentStatus::Failed => failed += 1,
            UpdateSkillAgentStatus::Skipped => {}
        }
    }

    if success > 0 && failed > 0 {
        UpdateSkillStatus::Partial
    } else if failed > 0 {
        UpdateSkillStatus::Failed
    } else if success > 0 {
        UpdateSkillStatus::Success
    } else {
        UpdateSkillStatus::Skipped
    }
}

fn summarize_results(results: &[UpdateSkillItemResult]) -> UpdateSkillSummary {
    let mut summary = UpdateSkillSummary {
        total: results.len() as u32,
        succeeded: 0,
        partial: 0,
        failed: 0,
        skipped: 0,
    };

    for result in results {
        match result.status {
            UpdateSkillStatus::Success => summary.succeeded += 1,
            UpdateSkillStatus::Partial => summary.partial += 1,
            UpdateSkillStatus::Failed => summary.failed += 1,
            UpdateSkillStatus::Skipped => summary.skipped += 1,
        }
    }

    summary
}

/// 从来源信息构造安装 URL
///
/// 与 CLI cli.ts runUpdate() 中构造 installUrl 的逻辑一致：
/// 1. 基础 URL = source_url
/// 2. 如果有 skillPath，去掉 SKILL.md 后缀，拼接为 GitHub tree URL
fn build_install_url_from_parts(source_url: &str, skill_path: Option<&str>) -> String {
    let mut install_url = source_url.to_string();

    if let Some(sp) = skill_path {
        let mut skill_folder = sp.to_string();

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_skill_status_partial_when_some_agents_failed() {
        let agent_results = vec![
            UpdateSkillAgentResult {
                agent: "cursor".to_string(),
                status: UpdateSkillAgentStatus::Success,
                error: None,
                duration_ms: Some(5),
            },
            UpdateSkillAgentResult {
                agent: "claude-code".to_string(),
                status: UpdateSkillAgentStatus::Failed,
                error: Some("copy failed".to_string()),
                duration_ms: Some(7),
            },
        ];
        let status = derive_skill_status(&agent_results);
        assert_eq!(status, UpdateSkillStatus::Partial);
    }

    #[test]
    fn test_summarize_results_counts_all_statuses() {
        let results = vec![
            UpdateSkillItemResult {
                name: "a".to_string(),
                status: UpdateSkillStatus::Success,
                error: None,
                warnings: vec![],
                duration_ms: None,
                agent_results: vec![],
            },
            UpdateSkillItemResult {
                name: "b".to_string(),
                status: UpdateSkillStatus::Partial,
                error: None,
                warnings: vec![],
                duration_ms: None,
                agent_results: vec![],
            },
            UpdateSkillItemResult {
                name: "c".to_string(),
                status: UpdateSkillStatus::Failed,
                error: Some("x".to_string()),
                warnings: vec![],
                duration_ms: None,
                agent_results: vec![],
            },
            UpdateSkillItemResult {
                name: "d".to_string(),
                status: UpdateSkillStatus::Skipped,
                error: None,
                warnings: vec![],
                duration_ms: None,
                agent_results: vec![],
            },
        ];
        let summary = summarize_results(&results);
        assert_eq!(summary.total, 4);
        assert_eq!(summary.succeeded, 1);
        assert_eq!(summary.partial, 1);
        assert_eq!(summary.failed, 1);
        assert_eq!(summary.skipped, 1);
    }

    #[test]
    fn test_skill_status_success_when_all_agents_succeeded() {
        let agent_results = vec![
            UpdateSkillAgentResult {
                agent: "cursor".to_string(),
                status: UpdateSkillAgentStatus::Success,
                error: None,
                duration_ms: Some(5),
            },
            UpdateSkillAgentResult {
                agent: "claude-code".to_string(),
                status: UpdateSkillAgentStatus::Success,
                error: None,
                duration_ms: Some(3),
            },
        ];
        assert_eq!(derive_skill_status(&agent_results), UpdateSkillStatus::Success);
    }

    #[test]
    fn test_skill_status_failed_when_all_agents_failed() {
        let agent_results = vec![
            UpdateSkillAgentResult {
                agent: "cursor".to_string(),
                status: UpdateSkillAgentStatus::Failed,
                error: Some("err".to_string()),
                duration_ms: None,
            },
        ];
        assert_eq!(derive_skill_status(&agent_results), UpdateSkillStatus::Failed);
    }

    #[test]
    fn test_skill_status_skipped_when_empty() {
        assert_eq!(derive_skill_status(&[]), UpdateSkillStatus::Skipped);
    }

    #[test]
    fn test_skill_status_skipped_when_all_agents_skipped() {
        let agent_results = vec![
            UpdateSkillAgentResult {
                agent: "cursor".to_string(),
                status: UpdateSkillAgentStatus::Skipped,
                error: None,
                duration_ms: None,
            },
        ];
        assert_eq!(derive_skill_status(&agent_results), UpdateSkillStatus::Skipped);
    }
}

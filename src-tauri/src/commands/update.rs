//! 更新检测相关的 Tauri Commands
//!
//! 提供命令：
//! - check_updates: 检测指定 scope 的 skills 是否有更新

use crate::core::agents::AgentType;
use crate::core::installer::{
    detect_install_mode, detect_installed_agents_for_skill, install_skill_to_agents_with_modes,
};
use crate::core::local_lock::{
    add_skill_to_local_lock, compute_skill_folder_hash, read_local_lock, LocalSkillLockEntry,
};
use crate::core::skill_lock::{add_skill_to_lock, read_scoped_lock};
use crate::core::wellknown::fetch_wellknown_skills;
use crate::core::{
    build_update_group_key, build_update_target, clone_repo_with_progress, compute_local_tree_sha,
    discover_skills, CloneProgress, DiscoverOptions, DiscoveredSkill, UpdateSourceParts,
};
use crate::core::{
    derive_update_capability, normalize_global_lock_entry, normalize_local_lock_entry,
};
use crate::core::{fetch_skill_folder_hash, fetch_skill_folder_hashes_batch};
use crate::error::AppError;
use crate::models::{
    InstallMode, Scope, UpdateSkillAgentResult, UpdateSkillAgentStatus, UpdateSkillItemResult,
    UpdateSkillResponse, UpdateSkillStatus, UpdateSkillSummary,
};
use serde::Serialize;
use specta::Type;
use std::collections::HashMap;
use std::path::Path;
use std::time::Instant;

/// 更新进度事件（发送到前端）
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct UpdateProgress {
    skill_name: String,
    scope: Scope,
    #[serde(skip_serializing_if = "Option::is_none")]
    project_path: Option<String>,
    /// "cloning" | "installing" | "writing_lock"
    phase: String,
}

fn update_progress_payload(
    skill_name: &str,
    scope: &Scope,
    project_path: Option<&str>,
    phase: &str,
) -> UpdateProgress {
    UpdateProgress {
        skill_name: skill_name.to_string(),
        scope: scope.clone(),
        project_path: project_path.map(str::to_owned),
        phase: phase.to_string(),
    }
}

/// 更新检测结果
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "kebab-case")]
#[specta(rename_all = "kebab-case")]
pub enum SkillUpdateCheckStatus {
    UpdateAvailable,
    UpToDate,
    CannotCheck,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct SkillUpdateInfo {
    pub name: String,
    pub source: String,
    pub has_update: bool,
    pub status: SkillUpdateCheckStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub git_ref: Option<String>,
}

/// 检测指定 scope 的 skills 是否有更新
///
/// 流程：
/// 1. 读取对应 scope 的 lock 文件
/// 2. 从 lock entry 派生更新能力；不可检查的 entry 直接返回 cannot-check
/// 3. 对可检查的来源按 source/ref 分组，目前 GitHub 走 Trees API
/// 4. 比对记录的版本 hash 与远端 hash
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
    let mut skills_by_source: HashMap<(String, Option<String>), Vec<(String, String, String)>> =
        HashMap::new();
    let mut results = Vec::new();

    match scope {
        Scope::Global => {
            let lock = read_scoped_lock(None)?;
            for (name, entry) in &lock.skills {
                let metadata = normalize_global_lock_entry(entry);
                let capability = derive_update_capability(&metadata);

                if !capability.can_check_for_updates {
                    results.push(SkillUpdateInfo {
                        name: name.clone(),
                        source: metadata.source.clone(),
                        has_update: false,
                        status: SkillUpdateCheckStatus::CannotCheck,
                        reason: capability.reason.clone(),
                        git_ref: metadata.ref_name.clone(),
                    });
                    continue;
                }

                skills_by_source
                    .entry((metadata.source.clone(), metadata.ref_name.clone()))
                    .or_default()
                    .push((
                        name.clone(),
                        metadata.skill_path.unwrap(),
                        metadata.remote_hash.unwrap(),
                    ));
            }
        }
        Scope::Project => {
            if let Some(pp) = project_path {
                let local_lock = read_local_lock(pp)?;
                for (name, entry) in &local_lock.skills {
                    let metadata = normalize_local_lock_entry(entry);
                    let capability = derive_update_capability(&metadata);

                    if !capability.can_check_for_updates {
                        results.push(SkillUpdateInfo {
                            name: name.clone(),
                            source: metadata.source.clone(),
                            has_update: false,
                            status: SkillUpdateCheckStatus::CannotCheck,
                            reason: capability.reason.clone(),
                            git_ref: metadata.ref_name.clone(),
                        });
                        continue;
                    }

                    skills_by_source
                        .entry((metadata.source.clone(), metadata.ref_name.clone()))
                        .or_default()
                        .push((
                            name.clone(),
                            metadata.skill_path.unwrap(),
                            metadata.remote_hash.unwrap(),
                        ));
                }
            }
        }
    }

    for ((source, ref_name), skills) in &skills_by_source {
        let paths: Vec<(String, String)> = skills
            .iter()
            .map(|(name, skill_path, _)| (name.clone(), skill_path.clone()))
            .collect();
        match fetch_skill_folder_hashes_batch(source, &paths, ref_name.as_deref()).await {
            Ok(hashes) => {
                for (name, _, local_hash) in skills {
                    results.push(build_batch_check_result(
                        name,
                        source,
                        ref_name.as_deref(),
                        local_hash,
                        hashes.get(name).and_then(|h| h.as_deref()),
                    ));
                }
            }
            Err(err) => {
                // API 失败,不误报。reason 优先走 GitHubApiError 自带的精细分类
                // (`rate-limited` / `auth` / `network-error` / `http-<code>`),
                // 让前端能针对性提示用户(例如 rate-limited 时引导设置 GITHUB_TOKEN)。
                let reason = match &err {
                    AppError::GitHubApiError { reason, .. } => reason.clone(),
                    _ => "upstream-unavailable".to_string(),
                };
                for (name, _, _) in skills {
                    results.push(SkillUpdateInfo {
                        name: name.clone(),
                        source: source.clone(),
                        has_update: false,
                        status: SkillUpdateCheckStatus::CannotCheck,
                        reason: Some(reason.clone()),
                        git_ref: ref_name.clone(),
                    });
                }
            }
        }
    }

    Ok(results)
}

/// 入口校验：仅当 metadata 满足"可执行更新"时返回 Ok，否则给出具体原因
fn ensure_can_run_update(metadata: &crate::core::NormalizedUpdateMetadata) -> Result<(), AppError> {
    let capability = crate::core::derive_update_capability(metadata);
    if !capability.can_run_update {
        return Err(AppError::InstallFailed {
            message: format!(
                "Skill cannot be updated: {}",
                capability.reason.unwrap_or_else(|| "unknown".to_string())
            ),
        });
    }
    Ok(())
}

fn build_skipped_update_result(
    name: &str,
    metadata: &crate::core::NormalizedUpdateMetadata,
    reason: &str,
) -> UpdateSkillItemResult {
    UpdateSkillItemResult {
        name: name.to_string(),
        status: UpdateSkillStatus::Skipped,
        error: Some(reason.to_string()),
        reason: Some(reason.to_string()),
        source: Some(metadata.source.clone()),
        source_url: metadata.source_url.clone(),
        git_ref: metadata.ref_name.clone(),
        skill_path: metadata.skill_path.clone(),
        warnings: Vec::new(),
        duration_ms: None,
        agent_results: Vec::new(),
    }
}

fn build_batch_check_result(
    name: &str,
    source: &str,
    ref_name: Option<&str>,
    local_hash: &str,
    remote_hash: Option<&str>,
) -> SkillUpdateInfo {
    match remote_hash {
        Some(remote_hash) => {
            let has_update = remote_hash != local_hash;
            SkillUpdateInfo {
                name: name.to_string(),
                source: source.to_string(),
                has_update,
                status: if has_update {
                    SkillUpdateCheckStatus::UpdateAvailable
                } else {
                    SkillUpdateCheckStatus::UpToDate
                },
                reason: None,
                git_ref: ref_name.map(str::to_string),
            }
        }
        None => SkillUpdateInfo {
            name: name.to_string(),
            source: source.to_string(),
            has_update: false,
            status: SkillUpdateCheckStatus::CannotCheck,
            reason: Some("upstream-unavailable".to_string()),
            git_ref: ref_name.map(str::to_string),
        },
    }
}

fn normalize_skill_path(path: &str) -> String {
    path.replace('\\', "/")
}

fn find_update_skill<'a>(
    discovered: &'a [DiscoveredSkill],
    name: &str,
    skill_path: Option<&str>,
) -> Option<&'a DiscoveredSkill> {
    if let Some(skill_path) = skill_path {
        let normalized_skill_path = normalize_skill_path(skill_path);
        return discovered.iter().find(|skill| {
            skill.name == name
                && normalize_skill_path(&skill.relative_path) == normalized_skill_path
        });
    }

    discovered.iter().find(|skill| skill.name == name)
}

fn discover_update_candidates(
    skills_dir: &Path,
    skill_path: Option<&str>,
) -> Result<Vec<DiscoveredSkill>, AppError> {
    let update_target = build_update_target(UpdateSourceParts {
        source_type: String::new(),
        source_url: String::new(),
        ref_name: None,
        skill_path: skill_path.map(str::to_string),
    });
    let options = DiscoverOptions {
        include_internal: true,
        full_depth: false,
    };

    discover_skills(
        skills_dir,
        update_target.discover_subpath.as_deref(),
        options,
    )
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
            reason: None,
            source: None,
            source_url: None,
            git_ref: None,
            skill_path: None,
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
    let (
        entry_source,
        entry_source_type,
        entry_source_url,
        entry_skill_path,
        entry_plugin_name,
        entry_ref_name,
    ) = match scope {
        Scope::Global => {
            let lock = read_scoped_lock(None)?;
            let entry = lock
                .skills
                .get(skill_name)
                .ok_or_else(|| AppError::InvalidSource {
                    value: format!("Skill '{}' not found in lock file", skill_name),
                })?;
            (
                entry.source.clone(),
                entry.source_type.clone(),
                entry.source_url.clone(),
                entry.skill_path.clone(),
                entry.plugin_name.clone(),
                entry.ref_name.clone(),
            )
        }
        Scope::Project => {
            let pp = project_path.ok_or_else(|| AppError::InvalidSource {
                value: "Project path is required for project scope".to_string(),
            })?;
            let local_lock = read_local_lock(pp)?;
            let entry =
                local_lock
                    .skills
                    .get(skill_name)
                    .ok_or_else(|| AppError::InvalidSource {
                        value: format!("Skill '{}' not found in project lock file", skill_name),
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
                entry.ref_name.clone(),
            )
        }
    };

    // 2. 入口校验：提前拒绝不可执行的更新（如 local 类型、缺失 skillPath 等）
    let metadata = crate::core::NormalizedUpdateMetadata {
        source: entry_source.clone(),
        source_type: entry_source_type.clone(),
        source_url: if entry_source_url.is_empty() {
            None
        } else {
            Some(entry_source_url.clone())
        },
        ref_name: entry_ref_name.clone(),
        skill_path: entry_skill_path.clone(),
        remote_hash: None,
    };
    if ensure_can_run_update(&metadata).is_err() {
        let capability = crate::core::derive_update_capability(&metadata);
        return Ok(build_skipped_update_result(
            skill_name,
            &metadata,
            capability.reason.as_deref().unwrap_or("cannot-update"),
        ));
    }

    // 3. 直接从 lock 元数据构造更新目标，避免 round-trip 成 source 字符串后丢失来源类型
    let update_target = build_update_target(UpdateSourceParts {
        source_type: entry_source_type.clone(),
        source_url: if entry_source_url.is_empty() {
            entry_source.clone()
        } else {
            entry_source_url.clone()
        },
        ref_name: entry_ref_name.clone(),
        skill_path: entry_skill_path.clone(),
    });

    // 3. 获取更新来源
    let _ = app.emit(
        "update-progress",
        &update_progress_payload(skill_name, &scope, project_path, "cloning"),
    );
    let (skills_dir, _clone_result) = match entry_source_type.as_str() {
        "local" => (
            std::path::PathBuf::from(&update_target.fetch_source_url),
            None,
        ),
        "github" | "gitlab" | "git" => {
            let app_clone = app.clone();
            let clone_result = clone_repo_with_progress(
                &update_target.fetch_source_url,
                update_target.git_ref.as_deref(),
                move |progress: CloneProgress| {
                    let _ = app_clone.emit("clone-progress", &progress);
                },
            )?;
            let repo_path = clone_result.repo_path.clone();
            (repo_path, Some(clone_result))
        }
        "well-known" | "wellknown" | "direct-url" => {
            let result = fetch_wellknown_skills(&update_target.fetch_source_url).await?;
            (result.repo_path, None)
        }
        other => {
            return Err(AppError::InvalidSource {
                value: format!("Unsupported update source type: {}", other),
            });
        }
    };

    // 4. 发现 skills
    let options = DiscoverOptions {
        include_internal: true,
        full_depth: false,
    };
    let discovered = discover_skills(
        &skills_dir,
        update_target.discover_subpath.as_deref(),
        options,
    )?;

    // 5. 找到目标 skill
    let skill = find_update_skill(&discovered, skill_name, entry_skill_path.as_deref())
        .ok_or_else(|| AppError::NoSkillsFound)?;

    // 6. 检测已安装的 agents (通过文件系统检测,fallback 仅保留 universal agents)。
    //    注意:fallback 故意不再合并 `AgentType::detect_installed()` —— 否则会把 skill
    //    装到从未链接过的 agent 上 (例如用户原本只装在 cursor、之后手动卸了 cursor 时)。
    //    canonical 已经被 install_skill_to_agents_with_modes 写为新内容,universal agents
    //    直接读 canonical 即可。
    let install_scope = match scope {
        Scope::Global => crate::models::Scope::Global,
        Scope::Project => crate::models::Scope::Project,
    };
    let mut target_agents =
        detect_installed_agents_for_skill(skill_name, &install_scope, project_path);
    if target_agents.is_empty() {
        target_agents = AgentType::get_universal_agents();
    }

    // 7. 按 agent 检测安装模式（通过文件系统检测）
    let target_agent_modes: Vec<(AgentType, InstallMode)> = target_agents
        .iter()
        .map(|agent| {
            (
                *agent,
                detect_install_mode(skill_name, agent, &install_scope, project_path),
            )
        })
        .collect();

    // 8. 执行安装（覆盖现有文件）
    let _ = app.emit(
        "update-progress",
        &update_progress_payload(skill_name, &scope, project_path, "installing"),
    );
    let per_agent_results = install_skill_to_agents_with_modes(
        &skill.path,
        &skill.name,
        &target_agent_modes,
        &install_scope,
        project_path,
    );
    let agent_results: Vec<UpdateSkillAgentResult> = per_agent_results
        .into_iter()
        .map(|r| UpdateSkillAgentResult {
            agent: r.agent,
            status: if r.skipped {
                UpdateSkillAgentStatus::Skipped
            } else if r.success {
                UpdateSkillAgentStatus::Success
            } else {
                UpdateSkillAgentStatus::Failed
            },
            mode: Some(r.mode),
            error: r.error,
            duration_ms: r.duration_ms,
        })
        .collect();

    // 9. 仅当所有 agent 都成功时,才把新 hash 写入 lock。
    //    Partial / Failed 时保留旧 hash —— 这样下次 check_updates 仍会提示 update-available,
    //    用户重试只会重装失败的 agent,失败信息不会从 UI 上彻底消失。
    let status = derive_skill_status(&agent_results);
    let _ = app.emit(
        "update-progress",
        &update_progress_payload(skill_name, &scope, project_path, "writing_lock"),
    );

    if matches!(status, UpdateSkillStatus::Success) {
        let (final_hash, hash_warning) = resolve_post_update_hash(
            &scope,
            skill_name,
            project_path,
            &entry_source_type,
            &entry_source,
            entry_skill_path.as_deref(),
            entry_ref_name.as_deref(),
            Some(skills_dir.as_path()),
        )
        .await;
        if let Some(w) = hash_warning {
            warnings.push(w);
        }

        match scope {
            Scope::Global => {
                if let Err(err) = add_skill_to_lock(
                    skill_name,
                    &entry_source,
                    &entry_source_type,
                    &entry_source_url,
                    entry_ref_name.as_deref(),
                    entry_skill_path.as_deref(),
                    &final_hash,
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
                        ref_name: entry_ref_name.clone(),
                        source_type: entry_source_type.clone(),
                        source_url: Some(entry_source_url.clone()),
                        computed_hash,
                        remote_hash: if final_hash.is_empty() {
                            None
                        } else {
                            Some(final_hash.clone())
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
    }
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
        reason: None,
        source: Some(entry_source.clone()),
        source_url: if entry_source_url.is_empty() {
            None
        } else {
            Some(entry_source_url.clone())
        },
        git_ref: entry_ref_name.clone(),
        skill_path: entry_skill_path.clone(),
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
        ref_name: Option<String>,
    }

    let names_set: std::collections::HashSet<&str> = names.iter().map(|s| s.as_str()).collect();
    let mut entries: Vec<SkillEntry> = Vec::new();
    let mut all_results: Vec<UpdateSkillItemResult> = Vec::new();

    match scope {
        Scope::Global => {
            if let Ok(lock) = read_scoped_lock(None) {
                for (name, entry) in &lock.skills {
                    if names_set.contains(name.as_str()) {
                        let metadata = normalize_global_lock_entry(entry);
                        let capability = derive_update_capability(&metadata);
                        if !capability.can_run_update {
                            all_results.push(build_skipped_update_result(
                                name,
                                &metadata,
                                capability.reason.as_deref().unwrap_or("cannot-update"),
                            ));
                            continue;
                        }
                        entries.push(SkillEntry {
                            name: name.clone(),
                            source: entry.source.clone(),
                            source_type: entry.source_type.clone(),
                            source_url: entry.source_url.clone(),
                            skill_path: entry.skill_path.clone(),
                            plugin_name: entry.plugin_name.clone(),
                            ref_name: entry.ref_name.clone(),
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
                            let metadata = normalize_local_lock_entry(entry);
                            let capability = derive_update_capability(&metadata);
                            if !capability.can_run_update {
                                all_results.push(build_skipped_update_result(
                                    name,
                                    &metadata,
                                    capability.reason.as_deref().unwrap_or("cannot-update"),
                                ));
                                continue;
                            }
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
                                ref_name: entry.ref_name.clone(),
                            });
                        }
                    }
                }
            }
        }
    }

    // 按 source_url + ref_name 分组，避免同仓库不同分支共享错误的 clone
    let mut by_source: HashMap<crate::core::UpdateGroupKey, Vec<SkillEntry>> = HashMap::new();
    for entry in entries {
        by_source
            .entry(build_update_group_key(
                &entry.source_type,
                &entry.source_url,
                entry.ref_name.as_deref(),
            ))
            .or_default()
            .push(entry);
    }

    let install_scope = match scope {
        Scope::Global => crate::models::Scope::Global,
        Scope::Project => crate::models::Scope::Project,
    };

    // 2. 每组 source 只 clone 一次
    for (_group_key, group) in &by_source {
        let update_target = build_update_target(UpdateSourceParts {
            source_type: group[0].source_type.clone(),
            source_url: if group[0].source_url.is_empty() {
                group[0].source.clone()
            } else {
                group[0].source_url.clone()
            },
            ref_name: group[0].ref_name.clone(),
            skill_path: None,
        });

        // emit cloning progress for first skill in group
        let _ = app.emit(
            "update-progress",
            &update_progress_payload(&group[0].name, &scope, project_path, "cloning"),
        );

        let (skills_dir, _clone_result) = match group[0].source_type.as_str() {
            "local" => (
                std::path::PathBuf::from(&update_target.fetch_source_url),
                None,
            ),
            "github" | "gitlab" | "git" => {
                let app_clone = app.clone();
                match clone_repo_with_progress(
                    &update_target.fetch_source_url,
                    update_target.git_ref.as_deref(),
                    move |progress: CloneProgress| {
                        let _ = app_clone.emit("clone-progress", &progress);
                    },
                ) {
                    Ok(clone_result) => {
                        let repo_path = clone_result.repo_path.clone();
                        (repo_path, Some(clone_result))
                    }
                    Err(err) => {
                        for entry in group {
                            all_results.push(UpdateSkillItemResult {
                                name: entry.name.clone(),
                                status: UpdateSkillStatus::Failed,
                                error: Some(err.to_string()),
                                reason: None,
                                source: Some(entry.source.clone()),
                                source_url: Some(entry.source_url.clone()),
                                git_ref: entry.ref_name.clone(),
                                skill_path: entry.skill_path.clone(),
                                warnings: Vec::new(),
                                duration_ms: None,
                                agent_results: Vec::new(),
                            });
                        }
                        continue;
                    }
                }
            }
            "well-known" | "wellknown" | "direct-url" => {
                match fetch_wellknown_skills(&update_target.fetch_source_url).await {
                    Ok(result) => (result.repo_path, None),
                    Err(err) => {
                        for entry in group {
                            all_results.push(UpdateSkillItemResult {
                                name: entry.name.clone(),
                                status: UpdateSkillStatus::Failed,
                                error: Some(err.to_string()),
                                reason: None,
                                source: Some(entry.source.clone()),
                                source_url: Some(entry.source_url.clone()),
                                git_ref: entry.ref_name.clone(),
                                skill_path: entry.skill_path.clone(),
                                warnings: Vec::new(),
                                duration_ms: None,
                                agent_results: Vec::new(),
                            });
                        }
                        continue;
                    }
                }
            }
            other => {
                for entry in group {
                    all_results.push(UpdateSkillItemResult {
                        name: entry.name.clone(),
                        status: UpdateSkillStatus::Failed,
                        error: Some(format!("Unsupported update source type: {}", other)),
                        reason: Some("unsupported-source-type".to_string()),
                        source: Some(entry.source.clone()),
                        source_url: Some(entry.source_url.clone()),
                        git_ref: entry.ref_name.clone(),
                        skill_path: entry.skill_path.clone(),
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

            let _ = app.emit(
                "update-progress",
                &update_progress_payload(&entry.name, &scope, project_path, "installing"),
            );

            let discovered =
                match discover_update_candidates(&skills_dir, entry.skill_path.as_deref()) {
                    Ok(d) => d,
                    Err(err) => {
                        all_results.push(UpdateSkillItemResult {
                            name: entry.name.clone(),
                            status: UpdateSkillStatus::Failed,
                            error: Some(err.to_string()),
                            reason: None,
                            source: Some(entry.source.clone()),
                            source_url: Some(entry.source_url.clone()),
                            git_ref: entry.ref_name.clone(),
                            skill_path: entry.skill_path.clone(),
                            warnings: Vec::new(),
                            duration_ms: None,
                            agent_results: Vec::new(),
                        });
                        continue;
                    }
                };

            let skill =
                match find_update_skill(&discovered, &entry.name, entry.skill_path.as_deref()) {
                    Some(s) => s,
                    None => {
                        all_results.push(UpdateSkillItemResult {
                            name: entry.name.clone(),
                            status: UpdateSkillStatus::Failed,
                            error: Some(format!(
                                "Skill '{}' not found in cloned repository",
                                entry.name
                            )),
                            reason: Some("skill-not-found".to_string()),
                            source: Some(entry.source.clone()),
                            source_url: Some(entry.source_url.clone()),
                            git_ref: entry.ref_name.clone(),
                            skill_path: entry.skill_path.clone(),
                            warnings: Vec::new(),
                            duration_ms: None,
                            agent_results: Vec::new(),
                        });
                        continue;
                    }
                };

            // detect agents (fallback 仅保留 universal agents,见单个 update 路径的注释)
            let mut target_agents =
                detect_installed_agents_for_skill(&entry.name, &install_scope, project_path);
            if target_agents.is_empty() {
                target_agents = AgentType::get_universal_agents();
            }

            // detect install mode per agent
            let target_agent_modes: Vec<(AgentType, InstallMode)> = target_agents
                .iter()
                .map(|agent| {
                    (
                        *agent,
                        detect_install_mode(&entry.name, agent, &install_scope, project_path),
                    )
                })
                .collect();

            // install
            let per_agent_results = install_skill_to_agents_with_modes(
                &skill.path,
                &skill.name,
                &target_agent_modes,
                &install_scope,
                project_path,
            );
            let agent_results: Vec<UpdateSkillAgentResult> = per_agent_results
                .into_iter()
                .map(|r| UpdateSkillAgentResult {
                    agent: r.agent,
                    status: if r.skipped {
                        UpdateSkillAgentStatus::Skipped
                    } else if r.success {
                        UpdateSkillAgentStatus::Success
                    } else {
                        UpdateSkillAgentStatus::Failed
                    },
                    mode: Some(r.mode),
                    error: r.error,
                    duration_ms: r.duration_ms,
                })
                .collect();

            // 仅当所有 agent 都成功时,才把新 hash 写入 lock。
            // Partial / Failed 时保留旧 hash —— 与单个 update 路径行为一致,
            // 让用户重试只重装失败的 agent。
            let status = derive_skill_status(&agent_results);
            let _ = app.emit(
                "update-progress",
                &update_progress_payload(&entry.name, &scope, project_path, "writing_lock"),
            );

            if matches!(status, UpdateSkillStatus::Success) {
                let (final_hash, hash_warning) = resolve_post_update_hash(
                    &scope,
                    &entry.name,
                    project_path,
                    &entry.source_type,
                    &entry.source,
                    entry.skill_path.as_deref(),
                    entry.ref_name.as_deref(),
                    Some(skills_dir.as_path()),
                )
                .await;
                if let Some(w) = hash_warning {
                    warnings.push(w);
                }

                match scope {
                    Scope::Global => {
                        if let Err(err) = add_skill_to_lock(
                            &entry.name,
                            &entry.source,
                            &entry.source_type,
                            &entry.source_url,
                            entry.ref_name.as_deref(),
                            entry.skill_path.as_deref(),
                            &final_hash,
                            entry.plugin_name.as_deref(),
                        ) {
                            warnings.push(format!("Failed to write global lock: {}", err));
                        }
                    }
                    Scope::Project => {
                        if let Some(pp) = project_path {
                            let install_dir = crate::core::paths::canonical_skills_dir(false, pp)
                                .join(crate::core::skill::sanitize_name(&entry.name));
                            let computed_hash =
                                compute_skill_folder_hash(&install_dir).unwrap_or_default();
                            let lock_entry = LocalSkillLockEntry {
                                source: entry.source.clone(),
                                ref_name: entry.ref_name.clone(),
                                source_type: entry.source_type.clone(),
                                source_url: Some(entry.source_url.clone()),
                                computed_hash,
                                remote_hash: if final_hash.is_empty() {
                                    None
                                } else {
                                    Some(final_hash.clone())
                                },
                                skill_path: entry.skill_path.clone(),
                                plugin_name: entry.plugin_name.clone(),
                            };
                            if let Err(err) = add_skill_to_local_lock(&entry.name, lock_entry, pp) {
                                warnings.push(format!("Failed to write project lock: {}", err));
                            }
                        }
                    }
                }
            }

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
                reason: None,
                source: Some(entry.source.clone()),
                source_url: Some(entry.source_url.clone()),
                git_ref: entry.ref_name.clone(),
                skill_path: entry.skill_path.clone(),
                warnings,
                duration_ms: Some(elapsed_ms(&start)),
                agent_results,
            });
        }
    }

    // 对于不在 lock 文件中的 names，标记为 failed
    let found_names: std::collections::HashSet<String> =
        all_results.iter().map(|r| r.name.clone()).collect();
    for name in names {
        if !found_names.contains(name) {
            all_results.push(UpdateSkillItemResult {
                name: name.clone(),
                status: UpdateSkillStatus::Failed,
                error: Some(format!("Skill '{}' not found in lock file", name)),
                reason: Some("missing-lock-entry".to_string()),
                source: None,
                source_url: None,
                git_ref: None,
                skill_path: None,
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

/// 读取 lock 中已有的版本追踪 hash（global 用 `skill_folder_hash`，project 用 GUI 扩展 `remote_hash`）。
/// 用于 update 后 hash 刷新失败时保留旧值，避免写入空串导致 capability 永久降级。
fn read_existing_hash(
    scope: &Scope,
    skill_name: &str,
    project_path: Option<&str>,
) -> Option<String> {
    match scope {
        Scope::Global => {
            let lock = read_scoped_lock(None).ok()?;
            let entry = lock.skills.get(skill_name)?;
            if entry.skill_folder_hash.is_empty() {
                None
            } else {
                Some(entry.skill_folder_hash.clone())
            }
        }
        Scope::Project => {
            let pp = project_path?;
            let lock = read_local_lock(pp).ok()?;
            let entry = lock.skills.get(skill_name)?;
            entry.remote_hash.clone().filter(|s| !s.is_empty())
        }
    }
}

/// 解析 update 完成后要写入 lock 的版本追踪 hash。
///
/// 优先级（每一级失败才进入下一级）：
///   1. 从本地新 clone 的仓库直接 `git rev-parse` 计算 tree SHA — 零额外网络调用
///   2. 远端 GitHub Trees API — 兜底
///   3. 保留 lock 中已有的旧 hash — 绝不写入空串
///
/// 返回 `(final_hash, warning)`，只有当 1 / 2 都失败、且需要保留旧 hash 时才会附带 warning。
async fn resolve_post_update_hash(
    scope: &Scope,
    skill_name: &str,
    project_path: Option<&str>,
    source_type: &str,
    source: &str,
    skill_path: Option<&str>,
    ref_name: Option<&str>,
    clone_repo_path: Option<&Path>,
) -> (String, Option<String>) {
    if source_type != "github" {
        return (String::new(), None);
    }
    let path_str = skill_path.unwrap_or("");

    // 1. 本地 git 仓库
    if let Some(repo_path) = clone_repo_path {
        if let Some(sha) = compute_local_tree_sha(repo_path, path_str) {
            return (sha, None);
        }
    }

    // 2. 远端 API 兜底
    if let Ok(Some(sha)) = fetch_skill_folder_hash(source, path_str, ref_name).await {
        return (sha, None);
    }

    // 3. 保留旧 hash
    if let Some(old) = read_existing_hash(scope, skill_name, project_path) {
        return (
            old,
            Some(format!(
                "Could not refresh remote hash for '{}', kept previous value",
                skill_name
            )),
        );
    }

    (
        String::new(),
        Some(format!(
            "Could not refresh remote hash for '{}'; lock entry will lack a remote hash",
            skill_name
        )),
    )
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::skill_lock::SkillLockEntry;
    use tempfile::tempdir;

    #[test]
    fn test_normalize_global_lock_entry_maps_skill_folder_hash_to_remote_hash() {
        let entry = SkillLockEntry {
            source: "owner/repo".to_string(),
            source_type: "github".to_string(),
            source_url: "https://github.com/owner/repo".to_string(),
            ref_name: Some("main".to_string()),
            skill_path: Some("skills/demo/SKILL.md".to_string()),
            skill_folder_hash: "tree123".to_string(),
            installed_at: "2026-01-01T00:00:00.000Z".to_string(),
            updated_at: "2026-01-01T00:00:00.000Z".to_string(),
            plugin_name: None,
        };

        let normalized = crate::core::normalize_global_lock_entry(&entry);
        assert_eq!(normalized.remote_hash.as_deref(), Some("tree123"));
    }

    #[test]
    fn test_build_update_target_extracts_discover_subpath() {
        let target = crate::core::build_update_target(crate::core::UpdateSourceParts {
            source_type: "github".to_string(),
            source_url: "https://github.com/owner/repo".to_string(),
            ref_name: Some("feature/my-branch".to_string()),
            skill_path: Some("skills/demo/SKILL.md".to_string()),
        });

        assert_eq!(target.discover_subpath.as_deref(), Some("skills/demo"));
        assert_eq!(target.git_ref.as_deref(), Some("feature/my-branch"));
    }

    #[test]
    fn test_find_update_skill_prefers_locked_skill_path_over_duplicate_name() {
        let priority_skill = crate::core::DiscoveredSkill {
            name: "demo".to_string(),
            description: "Priority".to_string(),
            path: std::path::PathBuf::from("skills/demo"),
            relative_path: "skills/demo/SKILL.md".to_string(),
            plugin_name: None,
        };
        let locked_skill = crate::core::DiscoveredSkill {
            name: "demo".to_string(),
            description: "Locked".to_string(),
            path: std::path::PathBuf::from("examples/demo"),
            relative_path: "examples/demo/SKILL.md".to_string(),
            plugin_name: None,
        };
        let discovered = vec![priority_skill, locked_skill];

        let selected = find_update_skill(&discovered, "demo", Some("examples/demo/SKILL.md"))
            .expect("locked skill path should match");

        assert_eq!(selected.relative_path, "examples/demo/SKILL.md");
    }

    #[test]
    fn test_discover_update_candidates_uses_locked_nonstandard_subpath() {
        let temp = tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join("skills/other")).unwrap();
        std::fs::write(
            temp.path().join("skills/other/SKILL.md"),
            "---\nname: other\ndescription: Priority dir\n---\n",
        )
        .unwrap();
        std::fs::create_dir_all(temp.path().join("examples/demo")).unwrap();
        std::fs::write(
            temp.path().join("examples/demo/SKILL.md"),
            "---\nname: demo\ndescription: Nonstandard dir\n---\n",
        )
        .unwrap();

        let discovered = discover_update_candidates(temp.path(), Some("examples/demo/SKILL.md"))
            .expect("discover locked skill path");

        assert_eq!(discovered.len(), 1);
        assert_eq!(discovered[0].name, "demo");
        assert_eq!(discovered[0].relative_path, "examples/demo/SKILL.md");
    }

    #[test]
    fn test_capability_downgrades_when_skill_path_missing() {
        let capability =
            crate::core::derive_update_capability(&crate::core::NormalizedUpdateMetadata {
                source: "owner/repo".to_string(),
                source_type: "github".to_string(),
                source_url: Some("https://github.com/owner/repo".to_string()),
                ref_name: None,
                skill_path: None,
                remote_hash: Some("tree123".to_string()),
            });

        assert!(!capability.can_check_for_updates);
        assert!(!capability.can_run_update);
        assert_eq!(capability.reason.as_deref(), Some("missing-skill-path"));
    }

    #[test]
    fn test_check_updates_marks_missing_metadata_as_cannot_check_for_project() {
        tauri::async_runtime::block_on(async {
            let temp = tempdir().unwrap();
            std::fs::write(
                temp.path().join("skills-lock.json"),
                r#"{
  "version": 1,
  "skills": {
    "broken-project": {
      "source": "owner/repo",
      "ref": "main",
      "sourceType": "github",
      "computedHash": "abc123",
      "remoteHash": "tree123"
    }
  }
}"#,
            )
            .unwrap();

            let updates = check_updates_inner(Scope::Project, Some(temp.path().to_str().unwrap()))
                .await
                .expect("updates");
            let item = updates
                .into_iter()
                .find(|u| u.name == "broken-project")
                .expect("item");

            assert_eq!(item.status, SkillUpdateCheckStatus::CannotCheck);
            assert_eq!(item.reason.as_deref(), Some("missing-skill-path"));
        });
    }

    #[test]
    fn test_batch_hash_result_marks_missing_remote_hash_as_cannot_check() {
        let item = build_batch_check_result("demo", "owner/repo", Some("main"), "local-hash", None);

        assert_eq!(item.status, SkillUpdateCheckStatus::CannotCheck);
        assert!(!item.has_update);
        assert_eq!(item.reason.as_deref(), Some("upstream-unavailable"));
    }

    #[test]
    fn test_ensure_can_run_update_rejects_local_source() {
        let metadata = crate::core::NormalizedUpdateMetadata {
            source: "/some/local/path".to_string(),
            source_type: "local".to_string(),
            source_url: None,
            ref_name: None,
            skill_path: None,
            remote_hash: None,
        };
        let result = ensure_can_run_update(&metadata);
        assert!(matches!(result, Err(AppError::InstallFailed { .. })));
    }

    #[test]
    fn test_ensure_can_run_update_rejects_missing_skill_path() {
        let metadata = crate::core::NormalizedUpdateMetadata {
            source: "owner/repo".to_string(),
            source_type: "github".to_string(),
            source_url: Some("https://github.com/owner/repo".to_string()),
            ref_name: Some("main".to_string()),
            skill_path: None,
            remote_hash: Some("abc123".to_string()),
        };
        let result = ensure_can_run_update(&metadata);
        assert!(matches!(result, Err(AppError::InstallFailed { .. })));
    }

    #[test]
    fn test_ensure_can_run_update_accepts_github_with_skill_path() {
        let metadata = crate::core::NormalizedUpdateMetadata {
            source: "owner/repo".to_string(),
            source_type: "github".to_string(),
            source_url: Some("https://github.com/owner/repo".to_string()),
            ref_name: Some("main".to_string()),
            skill_path: Some("skills/demo/SKILL.md".to_string()),
            remote_hash: Some("abc123".to_string()),
        };
        assert!(ensure_can_run_update(&metadata).is_ok());
    }

    #[test]
    fn test_skipped_update_result_carries_repair_metadata() {
        let metadata = crate::core::NormalizedUpdateMetadata {
            source: "owner/repo".to_string(),
            source_type: "github".to_string(),
            source_url: Some("https://github.com/owner/repo".to_string()),
            ref_name: Some("main".to_string()),
            skill_path: None,
            remote_hash: Some("tree123".to_string()),
        };

        let result = build_skipped_update_result("demo", &metadata, "missing-skill-path");

        assert_eq!(result.status, UpdateSkillStatus::Skipped);
        assert_eq!(result.reason.as_deref(), Some("missing-skill-path"));
        assert_eq!(result.error.as_deref(), Some("missing-skill-path"));
        assert_eq!(result.source.as_deref(), Some("owner/repo"));
        assert_eq!(
            result.source_url.as_deref(),
            Some("https://github.com/owner/repo")
        );
        assert_eq!(result.git_ref.as_deref(), Some("main"));
    }

    #[test]
    fn test_skill_status_partial_when_some_agents_failed() {
        let agent_results = vec![
            UpdateSkillAgentResult {
                agent: "cursor".to_string(),
                status: UpdateSkillAgentStatus::Success,
                mode: None,
                error: None,
                duration_ms: Some(5),
            },
            UpdateSkillAgentResult {
                agent: "claude-code".to_string(),
                status: UpdateSkillAgentStatus::Failed,
                mode: None,
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
                reason: None,
                source: None,
                source_url: None,
                git_ref: None,
                skill_path: None,
                warnings: vec![],
                duration_ms: None,
                agent_results: vec![],
            },
            UpdateSkillItemResult {
                name: "b".to_string(),
                status: UpdateSkillStatus::Partial,
                error: None,
                reason: None,
                source: None,
                source_url: None,
                git_ref: None,
                skill_path: None,
                warnings: vec![],
                duration_ms: None,
                agent_results: vec![],
            },
            UpdateSkillItemResult {
                name: "c".to_string(),
                status: UpdateSkillStatus::Failed,
                error: Some("x".to_string()),
                reason: None,
                source: None,
                source_url: None,
                git_ref: None,
                skill_path: None,
                warnings: vec![],
                duration_ms: None,
                agent_results: vec![],
            },
            UpdateSkillItemResult {
                name: "d".to_string(),
                status: UpdateSkillStatus::Skipped,
                error: None,
                reason: None,
                source: None,
                source_url: None,
                git_ref: None,
                skill_path: None,
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
                mode: None,
                error: None,
                duration_ms: Some(5),
            },
            UpdateSkillAgentResult {
                agent: "claude-code".to_string(),
                status: UpdateSkillAgentStatus::Success,
                mode: None,
                error: None,
                duration_ms: Some(3),
            },
        ];
        assert_eq!(
            derive_skill_status(&agent_results),
            UpdateSkillStatus::Success
        );
    }

    #[test]
    fn test_skill_status_failed_when_all_agents_failed() {
        let agent_results = vec![UpdateSkillAgentResult {
            agent: "cursor".to_string(),
            status: UpdateSkillAgentStatus::Failed,
            mode: None,
            error: Some("err".to_string()),
            duration_ms: None,
        }];
        assert_eq!(
            derive_skill_status(&agent_results),
            UpdateSkillStatus::Failed
        );
    }

    #[test]
    fn test_skill_status_skipped_when_empty() {
        assert_eq!(derive_skill_status(&[]), UpdateSkillStatus::Skipped);
    }

    #[test]
    fn test_skill_status_skipped_when_all_agents_skipped() {
        let agent_results = vec![UpdateSkillAgentResult {
            agent: "cursor".to_string(),
            status: UpdateSkillAgentStatus::Skipped,
            mode: None,
            error: None,
            duration_ms: None,
        }];
        assert_eq!(
            derive_skill_status(&agent_results),
            UpdateSkillStatus::Skipped
        );
    }

    /// 在 tempdir 里写一份合法的 project lock 文件。
    fn write_project_lock(tmp: &std::path::Path, skill_name: &str, remote_hash: Option<&str>) {
        let hash_field = match remote_hash {
            Some(h) => format!(",\n      \"remoteHash\": \"{}\"", h),
            None => String::new(),
        };
        let content = format!(
            r#"{{
  "version": 1,
  "skills": {{
    "{name}": {{
      "source": "owner/repo",
      "ref": "main",
      "sourceType": "github",
      "computedHash": "abc123",
      "skillPath": "skills/{name}/SKILL.md"{hash_field}
    }}
  }}
}}"#,
            name = skill_name,
            hash_field = hash_field
        );
        std::fs::write(tmp.join("skills-lock.json"), content).unwrap();
    }

    #[test]
    fn test_read_existing_hash_returns_remote_hash_for_project_scope() {
        let tmp = tempdir().unwrap();
        write_project_lock(tmp.path(), "demo", Some("tree-old"));
        let result =
            read_existing_hash(&Scope::Project, "demo", Some(tmp.path().to_str().unwrap()));
        assert_eq!(result.as_deref(), Some("tree-old"));
    }

    #[test]
    fn test_read_existing_hash_returns_none_when_remote_hash_missing() {
        let tmp = tempdir().unwrap();
        write_project_lock(tmp.path(), "demo", None);
        let result =
            read_existing_hash(&Scope::Project, "demo", Some(tmp.path().to_str().unwrap()));
        assert_eq!(result, None);
    }

    #[test]
    fn test_read_existing_hash_returns_none_when_skill_missing() {
        let tmp = tempdir().unwrap();
        write_project_lock(tmp.path(), "demo", Some("tree-old"));
        let result = read_existing_hash(
            &Scope::Project,
            "not-installed",
            Some(tmp.path().to_str().unwrap()),
        );
        assert_eq!(result, None);
    }

    /// 关键回归:本地 git 算不出来 + API 也失败时,绝不能返回空串覆盖旧 hash。
    /// 没有 clone_repo_path 时本地路径直接 short-circuit;API 走真实网络可能仍成功,
    /// 这里用一个不存在的 repo 来强制失败。
    #[test]
    fn test_resolve_post_update_hash_keeps_existing_hash_when_refresh_fails() {
        tauri::async_runtime::block_on(async {
            let tmp = tempdir().unwrap();
            write_project_lock(tmp.path(), "demo", Some("tree-old"));
            let (final_hash, warning) = resolve_post_update_hash(
                &Scope::Project,
                "demo",
                Some(tmp.path().to_str().unwrap()),
                "github",
                // 不存在的 repo:确保 fetch_skill_folder_hash 拿不到内容
                "this-org-does-not-exist-skill-deck/no-repo",
                Some("skills/demo/SKILL.md"),
                Some("nonexistent-branch-xyz"),
                None,
            )
            .await;
            // 必须保留旧 hash,绝不写空串
            assert_eq!(final_hash, "tree-old");
            assert!(warning.is_some(), "应当 push 一条 warning 提示用户");
        });
    }

    #[test]
    fn test_resolve_post_update_hash_returns_empty_for_non_github_source() {
        tauri::async_runtime::block_on(async {
            let (final_hash, warning) = resolve_post_update_hash(
                &Scope::Project,
                "demo",
                None,
                "local",
                "/some/path",
                None,
                None,
                None,
            )
            .await;
            assert_eq!(final_hash, "");
            assert!(warning.is_none());
        });
    }

    /// Fix 1 核心:本地 clone 的 git 仓库可以直接算 tree SHA,不再发 API。
    #[test]
    fn test_resolve_post_update_hash_uses_local_git_when_clone_path_provided() {
        tauri::async_runtime::block_on(async {
            // 在 tempdir 构造一个真实 git 仓库
            let repo_dir = tempdir().unwrap();
            let repo = repo_dir.path();
            let run = |args: &[&str]| -> bool {
                std::process::Command::new("git")
                    .arg("-C")
                    .arg(repo)
                    .args(args)
                    .status()
                    .map(|s| s.success())
                    .unwrap_or(false)
            };
            if !run(&["init", "-q"]) {
                eprintln!("git not available, skipping");
                return;
            }
            run(&["config", "user.email", "test@example.com"]);
            run(&["config", "user.name", "Test"]);
            run(&["config", "commit.gpgsign", "false"]);
            std::fs::create_dir_all(repo.join("skills/demo")).unwrap();
            std::fs::write(repo.join("skills/demo/SKILL.md"), "x").unwrap();
            run(&["add", "-A"]);
            run(&["commit", "-q", "-m", "init"]);

            let expected = std::process::Command::new("git")
                .arg("-C")
                .arg(repo)
                .args(["rev-parse", "HEAD:skills/demo"])
                .output()
                .unwrap();
            let expected_sha = String::from_utf8_lossy(&expected.stdout).trim().to_string();

            // 即使 source 写一个不存在的 repo,只要本地 clone 在,就走本地路径,不发 API
            let lock_dir = tempdir().unwrap();
            let (final_hash, warning) = resolve_post_update_hash(
                &Scope::Project,
                "demo",
                Some(lock_dir.path().to_str().unwrap()),
                "github",
                "this-does-not-matter/because-local-wins",
                Some("skills/demo/SKILL.md"),
                Some("nonexistent"),
                Some(repo),
            )
            .await;
            assert_eq!(final_hash, expected_sha);
            assert!(warning.is_none());
        });
    }
}

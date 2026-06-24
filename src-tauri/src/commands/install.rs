//! 安装相关的 Tauri Commands
//!
//! 提供两个命令：
//! - fetch_available: 从来源获取可用的 skills 列表
//! - install_skills: 安装选中的 skills

use crate::core::agent_availability::{availability_for_agent, AgentAvailabilityKind};
use crate::core::agents::AgentType;
use crate::core::local_lock::{
    add_skill_to_local_lock, compute_skill_folder_hash, LocalSkillLockEntry,
};
use crate::core::skill_lock::add_skill_to_lock;
use crate::core::wellknown::fetch_wellknown_skills;
use crate::core::{
    clone_repo_with_progress, compute_local_tree_sha, discover_skills,
    ensure_install_risk_acknowledged, fetch_skill_folder_hash, get_owner_repo,
    install_skill_to_agent_groups, install_skill_to_agent_groups_with_modes, parse_source,
    source_risk_policy, CloneProgress, DiscoverOptions, PerAgentInstallResult,
};
use crate::error::AppError;
use crate::models::{
    AvailableSkill, FetchResult, InstallMode, InstallParams, InstallResult, InstallResultCategory,
    InstallResults, InstallTargetInfo, ParsedSource, SourceType,
};
use std::collections::HashSet;
use std::path::Path;
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

#[derive(Debug, Clone, Copy)]
struct InstallBehavior {
    autofill_automatic_agents: bool,
}

#[derive(Debug, Clone)]
struct InstallTargetPlan {
    default_available_agents: Vec<AgentType>,
    private_required_targets: Vec<AgentType>,
    private_copy_targets: Vec<AgentType>,
    install_targets: Vec<AgentType>,
}

fn compute_install_behavior(retry: bool) -> InstallBehavior {
    if retry {
        InstallBehavior {
            autofill_automatic_agents: false,
        }
    } else {
        InstallBehavior {
            autofill_automatic_agents: true,
        }
    }
}

fn parse_agent_ids(agent_ids: &[String]) -> Result<Vec<AgentType>, AppError> {
    let mut agents = Vec::new();
    for agent_id in agent_ids {
        let agent = agent_id
            .parse::<AgentType>()
            .map_err(|_| AppError::InvalidAgent {
                agent: agent_id.clone(),
            })?;
        if !agents.contains(&agent) {
            agents.push(agent);
        }
    }
    Ok(agents)
}

fn validate_private_required_targets(
    agents: &[AgentType],
    is_global: bool,
    cwd: &str,
) -> Result<(), AppError> {
    for agent in agents {
        let availability = availability_for_agent(*agent, is_global, cwd);
        if availability.kind != AgentAvailabilityKind::PrivateRequired {
            return Err(AppError::InstallFailed {
                message: format!(
                    "{} does not require separate setup for this scope.",
                    agent.config().display_name
                ),
            });
        }
    }
    Ok(())
}

fn validate_private_copy_targets(
    agents: &[AgentType],
    is_global: bool,
    cwd: &str,
) -> Result<(), AppError> {
    for agent in agents {
        let availability = availability_for_agent(*agent, is_global, cwd);
        if availability.kind != AgentAvailabilityKind::SharedCompatible {
            return Err(AppError::InstallFailed {
                message: format!(
                    "cannot create an independent copy for {} in this scope.",
                    agent.config().display_name
                ),
            });
        }
    }
    Ok(())
}

fn resolve_install_target_plan(
    params: &InstallParams,
    behavior: InstallBehavior,
    cwd: &str,
) -> Result<InstallTargetPlan, AppError> {
    let is_global = matches!(params.scope, crate::models::Scope::Global);
    let default_available_agents = if behavior.autofill_automatic_agents {
        crate::core::agent_availability::default_available_agents(is_global, cwd)
    } else {
        Vec::new()
    };
    let private_copy_targets = parse_agent_ids(&params.private_copy_agents)?;
    let private_required_targets: Vec<AgentType> = parse_agent_ids(&params.agents)?
        .into_iter()
        .filter(|agent| !private_copy_targets.contains(agent))
        .collect();
    validate_private_required_targets(&private_required_targets, is_global, cwd)?;
    validate_private_copy_targets(&private_copy_targets, is_global, cwd)?;
    let mut install_targets = Vec::new();
    for agent in private_required_targets
        .iter()
        .chain(private_copy_targets.iter())
        .copied()
    {
        if !install_targets.contains(&agent) {
            install_targets.push(agent);
        }
    }

    Ok(InstallTargetPlan {
        default_available_agents,
        private_required_targets,
        private_copy_targets,
        install_targets,
    })
}

fn resolve_eve_subagents_from_targets(params: &InstallParams) -> Vec<Option<String>> {
    let mut result = Vec::new();
    for target in &params.agent_targets {
        if target.agent != AgentType::Eve {
            continue;
        }

        let value = target
            .subagent
            .as_ref()
            .filter(|value| !value.is_empty() && *value != "root")
            .cloned();
        if !result.contains(&value) {
            result.push(value);
        }
    }
    result
}

fn lock_subagents_from_eve_targets(eve_targets: &[Option<String>]) -> Option<Vec<String>> {
    let values: Vec<String> = eve_targets
        .iter()
        .map(|value| crate::core::eve::lock_subagent_value(value.as_deref()))
        .collect();

    if values.is_empty() || (values.len() == 1 && values[0].is_empty()) {
        None
    } else {
        Some(values)
    }
}

fn eve_target_details(
    project_path: &str,
    eve_targets: &[Option<String>],
) -> Vec<InstallTargetInfo> {
    eve_targets
        .iter()
        .map(|subagent| InstallTargetInfo {
            target_id: crate::core::eve::eve_target_id(subagent.as_deref()),
            agent: AgentType::Eve,
            display_name: crate::core::eve::eve_target_label(subagent.as_deref()),
            subagent: subagent.clone(),
            path: crate::core::eve::eve_skills_dir_for_target(project_path, subagent.as_deref())
                .to_string_lossy()
                .to_string(),
        })
        .collect()
}

fn install_result_category(
    result: &PerAgentInstallResult,
    private_copy_targets: &[AgentType],
) -> InstallResultCategory {
    if !result.success {
        return InstallResultCategory::Failed;
    }
    if result.skipped {
        return InstallResultCategory::Skipped;
    }
    if result.agent == "__canonical__" {
        return InstallResultCategory::DefaultAvailable;
    }
    if private_copy_targets
        .iter()
        .any(|agent| agent.to_string() == result.agent)
    {
        return InstallResultCategory::PrivateCopy;
    }
    InstallResultCategory::PrivateAdapted
}

async fn resolve_install_hash(
    repo_path: Option<&Path>,
    source_type: &SourceType,
    owner_repo: Option<&str>,
    skill_path: &str,
    git_ref: Option<&str>,
    installed_skill_dir: Option<&Path>,
) -> String {
    if source_type == &SourceType::GitHub {
        if let Some(repo_path) = repo_path {
            if let Some(sha) = compute_local_tree_sha(repo_path, skill_path) {
                return sha;
            }
        }

        if let Some(repo) = owner_repo {
            if let Ok(Some(sha)) = fetch_skill_folder_hash(repo, skill_path, git_ref).await {
                return sha;
            }
        }

        return String::new();
    }

    if matches!(
        source_type,
        SourceType::Git | SourceType::GitLab | SourceType::Local | SourceType::WellKnown
    ) {
        if let Some(dir) = installed_skill_dir {
            return compute_skill_folder_hash(dir).unwrap_or_default();
        }
    }

    String::new()
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
    fetch_available_inner(&app, &source).await
}

async fn fetch_available_inner(app: &AppHandle, source: &str) -> Result<FetchResult, AppError> {
    // 1. 解析来源
    let parsed = parse_source(source)?;

    // 2. 确定 skills 目录
    let (skills_dir, _clone_result) = match parsed.source_type {
        SourceType::Local => {
            let path = parsed
                .local_path
                .as_ref()
                .ok_or_else(|| AppError::InvalidSource {
                    value: "Missing local path".to_string(),
                })?;
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
        SourceType::WellKnown => {
            let result = fetch_wellknown_skills(&parsed.url).await?;
            let mut fetch_result = discover_and_build_result(&parsed, &result.repo_path)?;
            apply_wellknown_trust_metadata(&mut fetch_result, &result.trust_metadata);
            return Ok(fetch_result);
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
        git_ref: parsed.git_ref.clone(),
        skill_filter: parsed.skill_filter.clone(),
        risk_policy: source_risk_policy(parsed),
        skills,
    })
}

fn apply_wellknown_trust_metadata(
    result: &mut FetchResult,
    metadata: &std::collections::HashMap<String, crate::core::wellknown::WellKnownTrustMetadata>,
) {
    for skill in &mut result.skills {
        if let Some(meta) = metadata.get(&skill.name) {
            skill.well_known_version = meta.well_known_version.clone();
            skill.well_known_entry_type = meta.well_known_entry_type.clone();
            skill.artifact_url_host = meta.artifact_url_host.clone();
            skill.digest_verified = meta.digest_verified;
            skill.trust_reason = meta.trust_reason.clone();
        }
    }
}

fn should_write_lock_for_skill(
    successful: &[InstallResult],
    failed: &[InstallResult],
    skill_name: &str,
    require_complete_success: bool,
    expected_target_count: usize,
) -> bool {
    if require_complete_success {
        let completed_agents: HashSet<&str> = successful
            .iter()
            .filter(|result| result.skill_name == skill_name && !result.skipped)
            .filter(|result| expected_target_count == 0 || result.agent != "__canonical__")
            .map(|result| result.target_id.as_deref().unwrap_or(result.agent.as_str()))
            .collect();
        let has_failure = failed.iter().any(|result| result.skill_name == skill_name);
        if expected_target_count == 0 {
            return !completed_agents.is_empty() && !has_failure;
        }
        return expected_target_count > 0
            && completed_agents.len() == expected_target_count
            && !has_failure;
    }

    successful.iter().any(|result| {
        result.skill_name == skill_name
            && result.success
            && (!result.skipped || result.canonical_path.is_some())
    })
}

fn lock_source_for_parsed_source(parsed: &ParsedSource, requested_source: &str) -> String {
    if parsed.source_type == SourceType::WellKnown {
        return crate::core::wellknown::extract_hostname(&parsed.url)
            .unwrap_or_else(|| requested_source.to_string());
    }
    if parsed.url.starts_with("git@") || parsed.url.starts_with("ssh://") {
        return parsed.url.clone();
    }
    get_owner_repo(parsed).unwrap_or_else(|| requested_source.to_string())
}

fn canonical_only_install_result(
    skill_path: &Path,
    skill_name: &str,
    scope: &crate::models::Scope,
    project_path: Option<&str>,
    mode: &InstallMode,
) -> PerAgentInstallResult {
    let is_global = matches!(scope, crate::models::Scope::Global);
    let cwd = project_path.unwrap_or(".");
    let sanitized_name = crate::core::skill::sanitize_name(skill_name);
    let canonical_dir =
        crate::core::paths::canonical_skills_dir(is_global, cwd).join(sanitized_name);
    let started = std::time::Instant::now();
    let result = (|| -> Result<(), AppError> {
        if canonical_dir.exists() || canonical_dir.symlink_metadata().is_ok() {
            let _ = std::fs::remove_dir_all(&canonical_dir);
            let _ = std::fs::remove_file(&canonical_dir);
        }
        std::fs::create_dir_all(&canonical_dir).map_err(|e| AppError::InstallFailed {
            message: format!("Failed to create dir: {}", e),
        })?;

        crate::core::copy_skill_files(skill_path, &canonical_dir)
    })();
    let elapsed = started.elapsed().as_millis();
    let duration_ms = if elapsed > u32::MAX as u128 {
        u32::MAX
    } else {
        elapsed as u32
    };

    match result {
        Ok(()) => PerAgentInstallResult {
            agent: "__canonical__".to_string(),
            success: true,
            skipped: false,
            error: None,
            duration_ms: Some(duration_ms),
            symlink_failed: false,
            path: canonical_dir.clone(),
            canonical_path: Some(canonical_dir),
            mode: mode.clone(),
        },
        Err(err) => PerAgentInstallResult {
            agent: "__canonical__".to_string(),
            success: false,
            skipped: false,
            error: Some(err.to_string()),
            duration_ms: Some(duration_ms),
            symlink_failed: false,
            path: std::path::PathBuf::new(),
            canonical_path: None,
            mode: mode.clone(),
        },
    }
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
pub async fn install_skills(
    app: AppHandle,
    params: InstallParams,
) -> Result<InstallResults, AppError> {
    install_skills_inner(&app, params).await
}

async fn install_skills_inner(
    app: &AppHandle,
    params: InstallParams,
) -> Result<InstallResults, AppError> {
    let behavior = compute_install_behavior(params.retry);

    // 1. 解析来源
    let parsed = parse_source(&params.source)?;
    let risk_policy = source_risk_policy(&parsed);
    ensure_install_risk_acknowledged(&risk_policy, params.acknowledge_risk)?;

    // 2. 克隆或获取本地路径
    let (skills_dir, _clone_result) = match parsed.source_type {
        SourceType::Local => {
            let path = parsed
                .local_path
                .as_ref()
                .ok_or_else(|| AppError::InvalidSource {
                    value: "Missing local path".to_string(),
                })?;
            (path.clone(), None)
        }
        SourceType::GitHub | SourceType::GitLab | SourceType::Git => {
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
        SourceType::WellKnown => {
            let result = fetch_wellknown_skills(&parsed.url).await?;
            (result.repo_path, None)
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

    // 5. 拆分默认可用和独立写入目标。默认可用只写 canonical 共享目录。
    let cwd = params.project_path.as_deref().unwrap_or(".");
    let target_plan = resolve_install_target_plan(&params, behavior, cwd)?;
    let eve_targets = if matches!(params.scope, crate::models::Scope::Project) {
        resolve_eve_subagents_from_targets(&params)
    } else {
        Vec::new()
    };
    let expected_target_count = target_plan.install_targets.len() + eve_targets.len();
    let lock_subagents = lock_subagents_from_eve_targets(&eve_targets);
    let target_details = eve_target_details(cwd, &eve_targets);
    let include_canonical_result = !target_plan.default_available_agents.is_empty();

    // 7. 执行安装
    let mut successful = Vec::new();
    let mut failed = Vec::new();
    let mut symlink_fallback_agents = Vec::new();
    let total_skills = selected_skills.len();

    for (idx, skill) in selected_skills.iter().enumerate() {
        // 发送安装进度事件
        let _ = app.emit(
            "install-progress",
            &InstallProgress {
                phase: "installing".to_string(),
                current_skill: skill.name.clone(),
                completed: idx,
                total: total_skills,
            },
        );

        let per_agent_results = if target_plan.install_targets.is_empty() && eve_targets.is_empty()
        {
            vec![canonical_only_install_result(
                &skill.path,
                &skill.name,
                &params.scope,
                params.project_path.as_deref(),
                &params.mode,
            )]
        } else if params.preserve_existing_modes {
            let target_agent_modes: Vec<(AgentType, InstallMode)> = target_plan
                .private_required_targets
                .iter()
                .map(|agent| {
                    (
                        *agent,
                        crate::core::detect_install_mode(
                            &skill.name,
                            agent,
                            &params.scope,
                            params.project_path.as_deref(),
                        ),
                    )
                })
                .collect();
            install_skill_to_agent_groups_with_modes(
                &skill.path,
                &skill.name,
                &target_agent_modes,
                &target_plan.private_copy_targets,
                &params.scope,
                params.project_path.as_deref(),
                include_canonical_result,
            )
        } else {
            install_skill_to_agent_groups(
                &skill.path,
                &skill.name,
                &target_plan.private_required_targets,
                &target_plan.private_copy_targets,
                &params.scope,
                params.project_path.as_deref(),
                &params.mode,
                include_canonical_result,
            )
        };

        for par in per_agent_results {
            let category = install_result_category(&par, &target_plan.private_copy_targets);
            let install_result = InstallResult {
                skill_name: skill.name.clone(),
                agent: par.agent.clone(),
                target_id: None,
                subagent: None,
                success: par.success,
                path: par.path,
                canonical_path: par.canonical_path,
                mode: par.mode,
                symlink_failed: par.symlink_failed,
                skipped: par.skipped,
                error: par.error,
                category,
            };

            if install_result.success {
                if install_result.symlink_failed && !symlink_fallback_agents.contains(&par.agent) {
                    symlink_fallback_agents.push(par.agent.clone());
                }
                successful.push(install_result);
            } else {
                failed.push(install_result);
            }
        }

        for subagent in &eve_targets {
            let eve_result = crate::core::install_skill_for_eve_target(
                &skill.path,
                &skill.name,
                cwd,
                subagent.as_deref(),
            );

            if eve_result.success {
                successful.push(eve_result);
            } else {
                failed.push(eve_result);
            }
        }
    }

    // 8. 写入 lock 文件
    if selected_skills.iter().any(|skill| {
        should_write_lock_for_skill(
            &successful,
            &failed,
            &skill.name,
            params.preserve_existing_modes,
            expected_target_count,
        )
    }) {
        let _ = app.emit(
            "install-progress",
            &InstallProgress {
                phase: "writing_lock".to_string(),
                current_skill: String::new(),
                completed: total_skills,
                total: total_skills,
            },
        );

        let owner_repo = get_owner_repo(&parsed);

        for skill in &selected_skills {
            let installed = should_write_lock_for_skill(
                &successful,
                &failed,
                &skill.name,
                params.preserve_existing_modes,
                expected_target_count,
            );
            if !installed {
                continue;
            }

            let source = lock_source_for_parsed_source(&parsed, &params.source);
            let source_type_str = &parsed.source_type.to_string();
            let source_url = &parsed.url;
            let skill_path = Some(skill.relative_path.as_str());
            let canonical_skill_dir = match params.scope {
                crate::models::Scope::Global => crate::core::paths::canonical_skills_dir(true, "")
                    .join(crate::core::skill::sanitize_name(&skill.name)),
                crate::models::Scope::Project => params
                    .project_path
                    .as_ref()
                    .map(|project_path| {
                        crate::core::paths::canonical_skills_dir(false, project_path)
                            .join(crate::core::skill::sanitize_name(&skill.name))
                    })
                    .unwrap_or_else(|| {
                        crate::core::paths::canonical_skills_dir(false, "")
                            .join(crate::core::skill::sanitize_name(&skill.name))
                    }),
            };
            let installed_skill_dir = if canonical_skill_dir.exists() {
                canonical_skill_dir
            } else {
                successful
                    .iter()
                    .find(|result| {
                        result.skill_name == skill.name && result.success && !result.skipped
                    })
                    .map(|result| result.path.clone())
                    .unwrap_or(canonical_skill_dir)
            };
            let skill_folder_hash = resolve_install_hash(
                Some(skills_dir.as_path()),
                &parsed.source_type,
                owner_repo.as_deref(),
                &skill.relative_path,
                parsed.git_ref.as_deref(),
                Some(installed_skill_dir.as_path()),
            )
            .await;

            // 根据 scope 写入对应的 lock 文件
            match params.scope {
                crate::models::Scope::Global => {
                    let _ = add_skill_to_lock(
                        &skill.name,
                        &source,
                        source_type_str,
                        source_url,
                        parsed.git_ref.as_deref(),
                        skill_path,
                        &skill_folder_hash,
                        skill.plugin_name.as_deref(),
                    );
                }
                crate::models::Scope::Project => {
                    if let Some(ref project_path) = params.project_path {
                        // 计算安装后的本地文件 SHA-256
                        let computed_hash =
                            compute_skill_folder_hash(&installed_skill_dir).unwrap_or_default();

                        let entry = LocalSkillLockEntry {
                            source: source.clone(),
                            ref_name: parsed.git_ref.clone(),
                            source_type: source_type_str.to_string(),
                            source_url: Some(source_url.clone()),
                            computed_hash,
                            remote_hash: if skill_folder_hash.is_empty() {
                                None
                            } else {
                                Some(skill_folder_hash.clone())
                            },
                            skill_path: skill_path.map(|s| s.to_string()),
                            subagents: lock_subagents.clone(),
                            plugin_name: skill.plugin_name.clone(),
                        };
                        let _ = add_skill_to_local_lock(&skill.name, entry, project_path);
                    }
                }
            }
        }
    }

    Ok(InstallResults {
        successful,
        failed,
        symlink_fallback_agents,
        default_available_agents: target_plan
            .default_available_agents
            .iter()
            .map(ToString::to_string)
            .collect(),
        private_adapted_agents: target_plan
            .private_required_targets
            .iter()
            .map(ToString::to_string)
            .collect(),
        private_copy_agents: target_plan
            .private_copy_targets
            .iter()
            .map(ToString::to_string)
            .collect(),
        target_details,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    fn install_result(
        skill_name: &str,
        agent: &str,
        success: bool,
        path: std::path::PathBuf,
        canonical_path: Option<std::path::PathBuf>,
        mode: InstallMode,
        skipped: bool,
        error: Option<String>,
    ) -> InstallResult {
        InstallResult {
            skill_name: skill_name.to_string(),
            agent: agent.to_string(),
            target_id: None,
            subagent: None,
            success,
            path,
            canonical_path,
            mode,
            symlink_failed: false,
            skipped,
            error,
            category: InstallResultCategory::PrivateAdapted,
        }
    }

    #[test]
    fn test_retry_mode_disables_automatic_agent_autofill_and_agent_persistence() {
        let behavior = compute_install_behavior(true);
        assert!(!behavior.autofill_automatic_agents);
    }

    #[test]
    fn test_default_mode_keeps_automatic_autofill_without_agent_persistence() {
        let behavior = compute_install_behavior(false);
        assert!(behavior.autofill_automatic_agents);
    }

    #[test]
    fn test_default_install_behavior_uses_scope_automatic_agents() {
        let automatic_global = AgentType::get_automatic_agents_for_scope(true, ".");
        let automatic_project = AgentType::get_automatic_agents_for_scope(false, ".");

        assert!(
            !automatic_global.contains(&AgentType::Antigravity),
            "Antigravity global target is agent-specific, so it must be selectable"
        );
        assert!(
            automatic_project.contains(&AgentType::Antigravity),
            "Antigravity project target is .agents/skills, so it is automatic"
        );
    }

    #[test]
    fn test_default_agent_selection_uses_default_available_not_private_paths() {
        let defaults = crate::core::agent_availability::default_available_agents(true, ".");
        assert!(defaults.contains(&AgentType::Firebender));
        let firebender = AgentType::Firebender.availability_for_scope(true, ".");
        assert_eq!(
            firebender.kind,
            crate::core::agent_availability::AgentAvailabilityKind::SharedCompatible
        );
    }

    #[test]
    fn test_resolve_install_targets_excludes_default_available_from_private_targets() {
        let params = InstallParams {
            source: "owner/repo".to_string(),
            skills: vec!["demo".to_string()],
            agents: vec!["antigravity".to_string()],
            agent_targets: Vec::new(),
            private_copy_agents: vec!["firebender".to_string()],
            scope: crate::models::Scope::Global,
            project_path: None,
            mode: InstallMode::Copy,
            retry: false,
            preserve_existing_modes: false,
            acknowledge_risk: false,
        };

        let plan = resolve_install_target_plan(&params, compute_install_behavior(false), ".")
            .expect("target plan");

        assert!(plan
            .default_available_agents
            .contains(&AgentType::Firebender));
        assert_eq!(plan.private_required_targets, vec![AgentType::Antigravity]);
        assert_eq!(plan.private_copy_targets, vec![AgentType::Firebender]);
        assert_eq!(
            plan.install_targets,
            vec![AgentType::Antigravity, AgentType::Firebender]
        );
    }

    #[test]
    fn test_resolve_install_targets_rejects_default_available_regular_targets() {
        let params = InstallParams {
            source: "owner/repo".to_string(),
            skills: vec!["demo".to_string()],
            agents: vec!["firebender".to_string()],
            agent_targets: Vec::new(),
            private_copy_agents: vec![],
            scope: crate::models::Scope::Global,
            project_path: None,
            mode: InstallMode::Copy,
            retry: false,
            preserve_existing_modes: false,
            acknowledge_risk: false,
        };

        let err = resolve_install_target_plan(&params, compute_install_behavior(false), ".")
            .expect_err("shared-compatible agents must not be regular private targets");

        assert!(err
            .to_string()
            .contains("does not require separate setup"));
    }

    #[test]
    fn test_resolve_install_targets_rejects_private_required_private_copy_targets() {
        let params = InstallParams {
            source: "owner/repo".to_string(),
            skills: vec!["demo".to_string()],
            agents: vec![],
            agent_targets: Vec::new(),
            private_copy_agents: vec!["antigravity".to_string()],
            scope: crate::models::Scope::Global,
            project_path: None,
            mode: InstallMode::Copy,
            retry: false,
            preserve_existing_modes: false,
            acknowledge_risk: false,
        };

        let err = resolve_install_target_plan(&params, compute_install_behavior(false), ".")
            .expect_err("private-required agents must not be private-copy targets");

        assert!(err
            .to_string()
            .contains("cannot create an independent copy"));
    }

    #[test]
    fn test_resolve_eve_subagents_from_targets_deduplicates_root_and_named_subagents() {
        let params = InstallParams {
            source: "owner/repo".to_string(),
            skills: vec!["demo".to_string()],
            agents: vec![],
            agent_targets: vec![
                crate::models::InstallTargetSpec {
                    agent: AgentType::Eve,
                    subagent: None,
                },
                crate::models::InstallTargetSpec {
                    agent: AgentType::Eve,
                    subagent: Some("root".to_string()),
                },
                crate::models::InstallTargetSpec {
                    agent: AgentType::Eve,
                    subagent: Some("research".to_string()),
                },
                crate::models::InstallTargetSpec {
                    agent: AgentType::Eve,
                    subagent: Some("research".to_string()),
                },
                crate::models::InstallTargetSpec {
                    agent: AgentType::ClaudeCode,
                    subagent: Some("ignored".to_string()),
                },
            ],
            private_copy_agents: vec![],
            scope: crate::models::Scope::Project,
            project_path: Some("/tmp/project".to_string()),
            mode: InstallMode::Copy,
            retry: false,
            preserve_existing_modes: false,
            acknowledge_risk: false,
        };

        assert_eq!(
            resolve_eve_subagents_from_targets(&params),
            vec![None, Some("research".to_string())]
        );
    }

    #[test]
    fn test_install_result_category_marks_canonical_as_default_available() {
        let result = PerAgentInstallResult {
            agent: "__canonical__".to_string(),
            success: true,
            skipped: false,
            error: None,
            duration_ms: None,
            symlink_failed: false,
            path: std::path::PathBuf::from("/canonical/demo"),
            canonical_path: Some(std::path::PathBuf::from("/canonical/demo")),
            mode: InstallMode::Copy,
        };

        assert_eq!(
            install_result_category(&result, &[]),
            InstallResultCategory::DefaultAvailable
        );
    }

    #[test]
    fn test_install_result_category_marks_explicit_copy_and_failures() {
        let copied = PerAgentInstallResult {
            agent: "firebender".to_string(),
            success: true,
            skipped: false,
            error: None,
            duration_ms: None,
            symlink_failed: false,
            path: std::path::PathBuf::from("/firebender/demo"),
            canonical_path: Some(std::path::PathBuf::from("/canonical/demo")),
            mode: InstallMode::Copy,
        };
        let failed = PerAgentInstallResult {
            success: false,
            error: Some("failed".to_string()),
            ..copied.clone()
        };

        assert_eq!(
            install_result_category(&copied, &[AgentType::Firebender]),
            InstallResultCategory::PrivateCopy
        );
        assert_eq!(
            install_result_category(&failed, &[AgentType::Firebender]),
            InstallResultCategory::Failed
        );
    }

    #[test]
    fn test_regular_install_lock_write_accepts_skipped_canonical_results() {
        let results = vec![install_result(
            "demo",
            "windsurf",
            true,
            std::path::PathBuf::from("/canonical/demo"),
            Some(std::path::PathBuf::from("/canonical/demo")),
            InstallMode::Symlink,
            true,
            None,
        )];

        assert!(should_write_lock_for_skill(&results, &[], "demo", false, 1));
    }

    #[test]
    fn test_regular_install_lock_write_rejects_skipped_eve_target_only() {
        let results = vec![InstallResult {
            skill_name: "demo".to_string(),
            agent: "eve".to_string(),
            target_id: Some("eve:root".to_string()),
            subagent: None,
            success: true,
            path: std::path::PathBuf::from("/project/agent/skills/demo"),
            canonical_path: None,
            mode: InstallMode::Copy,
            symlink_failed: false,
            skipped: true,
            error: Some("source-overlaps-target".to_string()),
            category: InstallResultCategory::Skipped,
        }];

        assert!(!should_write_lock_for_skill(
            &results,
            &[],
            "demo",
            false,
            1
        ));
    }

    #[test]
    fn test_preserve_mode_lock_write_requires_all_target_agents_to_install() {
        let successful = vec![install_result(
            "demo",
            "claude-code",
            true,
            std::path::PathBuf::from("/agent/demo"),
            Some(std::path::PathBuf::from("/canonical/demo")),
            InstallMode::Copy,
            false,
            None,
        )];
        let failed = vec![install_result(
            "demo",
            "cursor",
            false,
            std::path::PathBuf::new(),
            None,
            InstallMode::Copy,
            false,
            Some("copy failed".to_string()),
        )];

        assert!(!should_write_lock_for_skill(
            &successful,
            &failed,
            "demo",
            true,
            2
        ));
    }

    #[test]
    fn test_preserve_mode_lock_write_requires_expected_target_count() {
        let successful = vec![install_result(
            "demo",
            "claude-code",
            true,
            std::path::PathBuf::from("/agent/demo"),
            Some(std::path::PathBuf::from("/canonical/demo")),
            InstallMode::Copy,
            false,
            None,
        )];

        assert!(!should_write_lock_for_skill(
            &successful,
            &[],
            "demo",
            true,
            2
        ));
    }

    #[test]
    fn test_preserve_mode_lock_write_requires_distinct_target_agents() {
        let successful = vec![
            install_result(
                "demo",
                "claude-code",
                true,
                std::path::PathBuf::from("/agent/demo"),
                Some(std::path::PathBuf::from("/canonical/demo")),
                InstallMode::Copy,
                false,
                None,
            ),
            install_result(
                "demo",
                "claude-code",
                true,
                std::path::PathBuf::from("/agent/demo-duplicate"),
                Some(std::path::PathBuf::from("/canonical/demo")),
                InstallMode::Copy,
                false,
                None,
            ),
        ];

        assert!(!should_write_lock_for_skill(
            &successful,
            &[],
            "demo",
            true,
            2
        ));
    }

    #[test]
    fn test_preserve_mode_lock_write_deduplicates_target_agents() {
        let successful = vec![install_result(
            "demo",
            "claude-code",
            true,
            std::path::PathBuf::from("/agent/demo"),
            Some(std::path::PathBuf::from("/canonical/demo")),
            InstallMode::Copy,
            false,
            None,
        )];

        assert!(should_write_lock_for_skill(
            &successful,
            &[],
            "demo",
            true,
            1
        ));
    }

    #[test]
    fn test_preserve_mode_lock_write_accepts_canonical_only_repair() {
        let successful = vec![install_result(
            "demo",
            "__canonical__",
            true,
            std::path::PathBuf::from("/canonical/demo"),
            Some(std::path::PathBuf::from("/canonical/demo")),
            InstallMode::Copy,
            false,
            None,
        )];

        assert!(should_write_lock_for_skill(
            &successful,
            &[],
            "demo",
            true,
            0
        ));
    }

    #[test]
    fn test_lock_source_preserves_git_ssh_url() {
        let parsed = parse_source("git@github.com:owner/private.git").unwrap();
        assert_eq!(
            lock_source_for_parsed_source(&parsed, "git@github.com:owner/private.git"),
            "git@github.com:owner/private.git"
        );
    }

    #[test]
    fn test_lock_source_preserves_ssh_url() {
        let parsed = parse_source("ssh://git@git.example.com:7999/owner/private.git#main").unwrap();
        assert_eq!(
            lock_source_for_parsed_source(
                &parsed,
                "ssh://git@git.example.com:7999/owner/private.git#main"
            ),
            "ssh://git@git.example.com:7999/owner/private.git"
        );
    }

    #[test]
    fn test_lock_source_preserves_ssh_url_without_git_suffix_and_excludes_fragment() {
        let parsed = parse_source("ssh://git@git.example.com/owner/private#main").unwrap();
        assert_eq!(parsed.git_ref.as_deref(), Some("main"));
        assert_eq!(
            lock_source_for_parsed_source(&parsed, "ssh://git@git.example.com/owner/private#main"),
            "ssh://git@git.example.com/owner/private"
        );
    }

    #[test]
    fn test_lock_source_keeps_public_github_normalized_owner_repo() {
        let parsed = parse_source("https://github.com/owner/repo").unwrap();
        assert_eq!(
            lock_source_for_parsed_source(&parsed, "https://github.com/owner/repo"),
            "owner/repo"
        );
    }

    #[test]
    fn test_canonical_only_install_result_represents_successful_repair() {
        let temp = tempdir().unwrap();
        let src = temp.path().join("source-skill");
        fs::create_dir_all(&src).unwrap();
        fs::write(
            src.join("SKILL.md"),
            "---\nname: demo\ndescription: test\n---\n",
        )
        .unwrap();
        let project_path = temp.path().to_string_lossy().to_string();

        let result = canonical_only_install_result(
            &src,
            "demo",
            &crate::models::Scope::Project,
            Some(&project_path),
            &InstallMode::Copy,
        );

        assert!(result.success, "result: {:?}", result);
        assert_eq!(result.agent, "__canonical__");
        assert!(result
            .canonical_path
            .as_ref()
            .unwrap()
            .join("SKILL.md")
            .exists());
        let install_result = InstallResult {
            skill_name: "demo".to_string(),
            agent: result.agent,
            target_id: None,
            subagent: None,
            success: result.success,
            path: result.path,
            canonical_path: result.canonical_path,
            mode: result.mode,
            symlink_failed: result.symlink_failed,
            skipped: result.skipped,
            error: result.error,
            category: InstallResultCategory::PrivateAdapted,
        };
        assert!(should_write_lock_for_skill(
            &[install_result],
            &[],
            "demo",
            true,
            0
        ));
    }

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

    #[test]
    fn test_discover_and_build_result_includes_openclaw_risk_policy() {
        let temp = tempdir().unwrap();
        let skill_dir = temp.path().join("my-skill");
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: demo\ndescription: Demo skill\n---\n",
        )
        .unwrap();

        let parsed = parse_source("openclaw/community-skills").unwrap();
        let result = discover_and_build_result(&parsed, temp.path()).unwrap();

        assert_eq!(
            result.risk_policy.kind,
            crate::models::InstallRiskKind::RequireConfirmation
        );
        assert_eq!(result.risk_policy.code.as_deref(), Some("openclaw"));
    }

    #[test]
    fn test_install_risk_acknowledgement_rejects_unconfirmed_guarded_source() {
        let error = crate::core::ensure_install_risk_acknowledged(
            &crate::models::InstallRiskPolicy {
                kind: crate::models::InstallRiskKind::RequireConfirmation,
                code: Some("openclaw".to_string()),
            },
            false,
        )
        .expect_err("risk confirmation should be required");

        assert!(matches!(
            error,
            AppError::InstallRiskConfirmationRequired { .. }
        ));
    }

    fn make_git_repo_with_skill() -> Option<(tempfile::TempDir, std::path::PathBuf, String)> {
        let temp = tempdir().ok()?;
        let repo = temp.path().to_path_buf();
        let run = |args: &[&str]| -> Option<()> {
            let status = std::process::Command::new("git")
                .arg("-C")
                .arg(&repo)
                .args(args)
                .status()
                .ok()?;
            status.success().then_some(())
        };

        run(&["init", "-q"])?;
        run(&["config", "user.email", "test@example.com"])?;
        run(&["config", "user.name", "Test"])?;
        run(&["config", "commit.gpgsign", "false"])?;
        run(&["config", "core.autocrlf", "false"])?;
        fs::create_dir_all(repo.join("skills/demo")).ok()?;
        fs::write(
            repo.join("skills/demo/SKILL.md"),
            "---\nname: demo\ndescription: Demo\n---\n",
        )
        .ok()?;
        run(&["add", "-A"])?;
        run(&["commit", "-q", "-m", "init"])?;

        let output = std::process::Command::new("git")
            .arg("-C")
            .arg(&repo)
            .args(["rev-parse", "HEAD:skills/demo"])
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        let sha = String::from_utf8_lossy(&output.stdout).trim().to_string();
        Some((temp, repo, sha))
    }

    #[test]
    fn test_resolve_install_hash_prefers_local_tree_sha_for_github() {
        tauri::async_runtime::block_on(async {
            let Some((_temp, repo, expected_sha)) = make_git_repo_with_skill() else {
                eprintln!("git not available, skipping");
                return;
            };

            let hash = resolve_install_hash(
                Some(repo.as_path()),
                &SourceType::GitHub,
                Some("this-owner-should-not-be-used/this-repo-should-not-be-used"),
                "skills/demo/SKILL.md",
                Some("missing-ref"),
                None,
            )
            .await;

            assert_eq!(hash, expected_sha);
        });
    }

    #[test]
    fn test_resolve_install_hash_uses_content_hash_for_non_github_git_source() {
        tauri::async_runtime::block_on(async {
            let temp = tempdir().unwrap();
            let skill_dir = temp.path().join("demo");
            fs::create_dir_all(&skill_dir).unwrap();
            fs::write(
                skill_dir.join("SKILL.md"),
                "---\nname: demo\ndescription: Demo\n---\n",
            )
            .unwrap();
            let expected_hash = compute_skill_folder_hash(&skill_dir).unwrap();

            let hash = resolve_install_hash(
                None,
                &SourceType::Git,
                None,
                "skills/demo/SKILL.md",
                None,
                Some(skill_dir.as_path()),
            )
            .await;

            assert_eq!(hash, expected_hash);
        });
    }
}

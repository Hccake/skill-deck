//! 安装相关的 Tauri Commands
//!
//! 提供两个命令：
//! - fetch_available: 从来源获取可用的 skills 列表
//! - install_skills: 安装选中的 skills

use crate::core::agent_availability::{availability_for_agent, AgentAvailabilityKind};
use crate::core::agents::AgentType;
use crate::core::local_lock::{
    add_skill_to_local_lock, compute_skill_folder_hash, LocalSkillLockEntry, LocalSkillLockFile,
};
use crate::core::lossless_lock::{LockEntrySnapshot, LosslessLockDocument};
use crate::core::mutation::{
    CancellationSignal, MutationGuard, MutationKind, SingleMutationController,
};
use crate::core::skill_lock::{add_skill_to_lock, SkillLockEntry, SkillLockFile};
use crate::core::skill_paths::find_skill_md_case_insensitive;
use crate::core::wellknown::{fetch_wellknown_skills, WellKnownFetchResult};
use crate::core::{
    clone_repo_with_progress, compute_local_tree_sha, discover_skills,
    ensure_install_risk_acknowledged, fetch_skill_folder_hash, get_owner_repo,
    install_skill_to_agent_groups, install_skill_to_agent_groups_with_modes, parse_source,
    source_risk_policy, CloneProgress, DiscoverOptions, PerAgentInstallResult,
};
use crate::environment::acquisition::{stage_wsl_source, StagedWslSource, WslAcquisitionSource};
use crate::environment::agent_environment::{AgentEnvironmentContext, AgentEnvironmentResolver};
use crate::environment::lock_io::EnvironmentLockIo;
use crate::environment::materialize::{
    materialize_wsl_skill, WslMaterializeRequest, WslMaterializeTarget,
};
use crate::environment::path_mapping::host_path_to_linux_path;
use crate::environment::types::{ContextRef, ContextScope, EnvironmentRef, ResourceLocator};
use crate::environment::wsl::{EnvironmentRegistry, WslSession};
use crate::environment::wsl_protocol::run_wsl_script;
use crate::error::AppError;
use crate::models::{
    AvailableSkill, FetchResult, InstallMode, InstallParams, InstallResult, InstallResultCategory,
    InstallResults, InstallTargetInfo, ParsedSource, SourceType,
};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use tauri::{AppHandle, Emitter, State};

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

#[derive(Debug, Clone)]
struct WslInstallTargetPlan {
    default_available_agents: Vec<AgentType>,
    private_required_targets: Vec<AgentType>,
    private_copy_targets: Vec<AgentType>,
    materialize_targets: Vec<WslMaterializeTarget>,
    target_details: Vec<InstallTargetInfo>,
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
    let concrete_target_agents: Vec<AgentType> = params
        .agent_targets
        .iter()
        .map(|target| target.agent)
        .collect();
    let private_required_targets: Vec<AgentType> = parse_agent_ids(&params.agents)?
        .into_iter()
        .filter(|agent| !private_copy_targets.contains(agent))
        .filter(|agent| !concrete_target_agents.contains(agent))
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

fn posix_parent(path: &str) -> Option<String> {
    path.trim_end_matches('/')
        .rsplit_once('/')
        .map(|(parent, _)| parent.to_string())
}

fn resolve_wsl_install_target_plan(
    params: &InstallParams,
    session: &crate::environment::wsl::WslSession,
    project_path: &str,
) -> Result<WslInstallTargetPlan, AppError> {
    let is_global = matches!(params.scope, crate::models::Scope::Global);
    let resolver = AgentEnvironmentResolver::new(AgentEnvironmentContext {
        home: session.home.clone(),
        config_home: session.config_home.clone(),
        env: session.environment.clone(),
    });
    let behavior = compute_install_behavior(params.retry);
    let default_available_agents = if behavior.autofill_automatic_agents {
        AgentType::all()
            .filter(|agent| {
                resolver
                    .target(*agent, is_global, project_path)
                    .default_available
            })
            .collect()
    } else {
        Vec::new()
    };
    let private_copy_targets = parse_agent_ids(&params.private_copy_agents)?;
    let concrete_target_agents: Vec<AgentType> = params
        .agent_targets
        .iter()
        .map(|target| target.agent)
        .collect();
    let private_required_targets: Vec<AgentType> = parse_agent_ids(&params.agents)?
        .into_iter()
        .filter(|agent| !private_copy_targets.contains(agent))
        .filter(|agent| !concrete_target_agents.contains(agent))
        .collect();
    let mut materialize_targets = Vec::new();
    for agent in &private_required_targets {
        let target = resolver.target(*agent, is_global, project_path);
        let skills_root = target.private_path.ok_or_else(|| AppError::InstallFailed {
            message: format!(
                "{} does not require or support a private Skill directory for this scope.",
                target.display_name
            ),
        })?;
        materialize_targets.push(WslMaterializeTarget {
            target_id: agent.to_string(),
            agent: agent.to_string(),
            required_root: (!is_global).then(|| posix_parent(&skills_root)).flatten(),
            skills_root,
            mode: params.mode.clone(),
            preserve_existing_mode: params.preserve_existing_modes,
        });
    }
    for agent in &private_copy_targets {
        let target = resolver.target(*agent, is_global, project_path);
        if target.availability != AgentAvailabilityKind::SharedCompatible {
            return Err(AppError::InstallFailed {
                message: format!(
                    "cannot create an independent copy for {} in this scope.",
                    target.display_name
                ),
            });
        }
        let skills_root = target.private_path.ok_or_else(|| AppError::InstallFailed {
            message: format!("{} has no private Skill directory.", target.display_name),
        })?;
        materialize_targets.push(WslMaterializeTarget {
            target_id: agent.to_string(),
            agent: agent.to_string(),
            required_root: (!is_global).then(|| posix_parent(&skills_root)).flatten(),
            skills_root,
            mode: InstallMode::Copy,
            preserve_existing_mode: false,
        });
    }

    let mut target_details = Vec::new();
    let mut concrete_target_ids = HashSet::new();
    if !is_global {
        for target in &params.agent_targets {
            if target.agent != AgentType::Eve {
                continue;
            }
            let subagent = target
                .subagent
                .as_ref()
                .filter(|value| !value.is_empty() && *value != "root")
                .cloned();
            let skills_root = match &subagent {
                Some(name) => format!(
                    "{}/agent/subagents/{}/skills",
                    project_path.trim_end_matches('/'),
                    crate::core::skill::sanitize_name(name)
                ),
                None => format!("{}/agent/skills", project_path.trim_end_matches('/')),
            };
            let target_id = crate::core::eve::eve_target_id(subagent.as_deref());
            if !concrete_target_ids.insert(target_id.clone()) {
                continue;
            }
            let display_name = crate::core::eve::eve_target_label(subagent.as_deref());
            let required_root = posix_parent(&skills_root);
            materialize_targets.push(WslMaterializeTarget {
                target_id: target_id.clone(),
                agent: AgentType::Eve.to_string(),
                skills_root: skills_root.clone(),
                mode: InstallMode::Copy,
                required_root,
                preserve_existing_mode: false,
            });
            target_details.push(InstallTargetInfo {
                target_id,
                agent: AgentType::Eve,
                display_name,
                subagent,
                path: skills_root,
            });
        }
    }

    Ok(WslInstallTargetPlan {
        default_available_agents,
        private_required_targets,
        private_copy_targets,
        materialize_targets,
        target_details,
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

#[derive(Debug, Clone, PartialEq, Eq)]
enum FetchAcquisitionRoute {
    Host,
    Wsl(WslAcquisitionSource),
}

fn fetch_acquisition_route(
    environment: &EnvironmentRef,
    parsed: &ParsedSource,
) -> Result<FetchAcquisitionRoute, AppError> {
    if matches!(environment, EnvironmentRef::Host) || parsed.source_type == SourceType::WellKnown {
        return Ok(FetchAcquisitionRoute::Host);
    }
    match parsed.source_type {
        SourceType::Local => {
            let native_path = parsed
                .local_path
                .as_ref()
                .ok_or_else(|| AppError::InvalidSource {
                    value: "Missing local path".to_string(),
                })?
                .to_string_lossy()
                .to_string();
            Ok(FetchAcquisitionRoute::Wsl(WslAcquisitionSource::Local {
                native_path,
            }))
        }
        SourceType::GitHub | SourceType::GitLab | SourceType::Git => {
            Ok(FetchAcquisitionRoute::Wsl(WslAcquisitionSource::Git {
                url: parsed.url.clone(),
                git_ref: parsed.git_ref.clone(),
            }))
        }
        SourceType::WellKnown => Ok(FetchAcquisitionRoute::Host),
    }
}

fn bind_install_params_to_context(
    mut params: InstallParams,
    project_path: Option<String>,
) -> InstallParams {
    match project_path {
        Some(project_path) => {
            params.scope = crate::models::Scope::Project;
            params.project_path = Some(project_path);
        }
        None => {
            params.scope = crate::models::Scope::Global;
            params.project_path = None;
        }
    }
    params
}

fn begin_install_mutation<'a>(
    controller: &'a SingleMutationController,
    context: ContextRef,
) -> Result<MutationGuard<'a>, AppError> {
    controller.begin(MutationKind::Install, context, "Preparing installation")
}

#[tauri::command]
#[specta::specta]
pub async fn fetch_available_v2(
    app: AppHandle,
    context: ContextRef,
    source: String,
    registry: State<'_, EnvironmentRegistry>,
) -> Result<FetchResult, AppError> {
    if matches!(context.environment, EnvironmentRef::Host) {
        return fetch_available_inner(&app, &source).await;
    }

    let parsed = parse_source(&source)?;
    match fetch_acquisition_route(&context.environment, &parsed)? {
        FetchAcquisitionRoute::Host => {
            let result = fetch_wellknown_skills(&parsed.url).await?;
            let mut fetch_result = discover_and_build_result(&parsed, &result.repo_path)?;
            apply_wellknown_trust_metadata(&mut fetch_result, &result.trust_metadata);
            Ok(fetch_result)
        }
        FetchAcquisitionRoute::Wsl(source) => {
            let EnvironmentRef::Wsl { distro_name } = &context.environment else {
                unreachable!("Host route returned above")
            };
            let session = registry.get(distro_name).ok_or_else(|| AppError::Custom {
                message: format!("WSL distro '{distro_name}' is not connected"),
            })?;
            let staged = stage_wsl_source(&session, source, CancellationSignal::default()).await?;
            discover_and_build_result(&parsed, staged.host_repo_path())
        }
    }
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

#[allow(clippy::too_many_arguments)]
fn build_wsl_lock_entry(
    scope: &crate::models::Scope,
    parsed: &ParsedSource,
    requested_source: &str,
    skill: &crate::core::discovery::DiscoveredSkill,
    remote_hash: &str,
    computed_hash: &str,
    subagents: Option<Vec<String>>,
    existing: Option<&serde_json::Value>,
) -> serde_json::Value {
    let source = lock_source_for_parsed_source(parsed, requested_source);
    let (replacement, known_fields) = match scope {
        crate::models::Scope::Global => {
            let now = chrono::Utc::now()
                .format("%Y-%m-%dT%H:%M:%S%.3fZ")
                .to_string();
            let installed_at = existing
                .and_then(|entry| entry.get("installedAt"))
                .and_then(serde_json::Value::as_str)
                .filter(|value| !value.is_empty())
                .unwrap_or(&now)
                .to_string();
            (
                serde_json::to_value(SkillLockEntry {
                    source,
                    source_type: parsed.source_type.to_string(),
                    source_url: parsed.url.clone(),
                    ref_name: parsed.git_ref.clone(),
                    skill_path: Some(skill.relative_path.clone()),
                    skill_folder_hash: remote_hash.to_string(),
                    installed_at,
                    updated_at: now,
                    plugin_name: skill.plugin_name.clone(),
                })
                .expect("SkillLockEntry serialization cannot fail"),
                &[
                    "source",
                    "sourceType",
                    "sourceUrl",
                    "ref",
                    "skillPath",
                    "skillFolderHash",
                    "installedAt",
                    "updatedAt",
                    "pluginName",
                ][..],
            )
        }
        crate::models::Scope::Project => (
            serde_json::to_value(LocalSkillLockEntry {
                source,
                ref_name: parsed.git_ref.clone(),
                source_type: parsed.source_type.to_string(),
                source_url: Some(parsed.url.clone()),
                computed_hash: computed_hash.to_string(),
                remote_hash: (!remote_hash.is_empty()).then(|| remote_hash.to_string()),
                skill_path: Some(skill.relative_path.clone()),
                subagents,
                plugin_name: skill.plugin_name.clone(),
            })
            .expect("LocalSkillLockEntry serialization cannot fail"),
            &[
                "source",
                "ref",
                "sourceType",
                "sourceUrl",
                "computedHash",
                "remoteHash",
                "skillPath",
                "subagents",
                "pluginName",
            ][..],
        ),
    };
    let mut merged = existing
        .and_then(serde_json::Value::as_object)
        .cloned()
        .unwrap_or_default();
    for field in known_fields {
        merged.remove(*field);
    }
    if let Some(replacement) = replacement.as_object() {
        merged.extend(replacement.clone());
    }
    serde_json::Value::Object(merged)
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

pub(crate) enum PreparedWslInstallSource {
    Staged(StagedWslSource),
    WellKnown(WellKnownFetchResult),
}

impl PreparedWslInstallSource {
    fn host_repo_path(&self) -> &Path {
        match self {
            Self::Staged(source) => source.host_repo_path(),
            Self::WellKnown(source) => &source.repo_path,
        }
    }

    fn linux_skill_path(&self, host_skill_path: &Path) -> Result<String, AppError> {
        match self {
            Self::Staged(source) => source.linux_path_for_host_path(host_skill_path),
            Self::WellKnown(_) => host_path_to_linux_path(&host_skill_path.to_string_lossy())
                .ok_or_else(|| AppError::Path {
                    message: format!(
                        "Well-Known staging path is not available through WSL DrvFS: {}",
                        host_skill_path.display()
                    ),
                }),
        }
    }
}

fn wsl_lock_locator(
    context: &ContextRef,
    session: &WslSession,
    project_path: Option<&str>,
) -> ResourceLocator {
    let native_path = match project_path {
        Some(project_path) => format!("{}/skills-lock.json", project_path.trim_end_matches('/')),
        None => session
            .xdg_state_home
            .as_ref()
            .filter(|path| !path.trim().is_empty())
            .map(|path| format!("{}/skills/.skill-lock.json", path.trim_end_matches('/')))
            .unwrap_or_else(|| {
                format!(
                    "{}/.agents/.skill-lock.json",
                    session.home.trim_end_matches('/')
                )
            }),
    };
    ResourceLocator {
        environment: context.environment.clone(),
        native_path,
    }
}

fn empty_wsl_lock_document(scope: &crate::models::Scope) -> LosslessLockDocument {
    let bytes = match scope {
        crate::models::Scope::Global => serde_json::to_vec(&SkillLockFile::empty()),
        crate::models::Scope::Project => serde_json::to_vec(&LocalSkillLockFile::empty()),
    }
    .expect("empty lock serialization cannot fail");
    LosslessLockDocument::parse(&bytes).expect("empty lock document is valid")
}

fn normalize_initial_wsl_lock_bytes(
    scope: &crate::models::Scope,
    current: Option<&[u8]>,
    legacy_project: Option<&[u8]>,
) -> Result<Vec<u8>, AppError> {
    if let Some(current) = current {
        return Ok(current.to_vec());
    }
    if matches!(scope, crate::models::Scope::Project) {
        if let Some(legacy_project) = legacy_project {
            let legacy: SkillLockFile = serde_json::from_slice(legacy_project)?;
            let mut migrated = LocalSkillLockFile::empty();
            for (name, entry) in legacy.skills {
                migrated.skills.insert(
                    name,
                    LocalSkillLockEntry {
                        source: entry.source,
                        ref_name: entry.ref_name,
                        source_type: entry.source_type,
                        source_url: (!entry.source_url.is_empty()).then_some(entry.source_url),
                        computed_hash: String::new(),
                        remote_hash: (!entry.skill_folder_hash.is_empty())
                            .then_some(entry.skill_folder_hash),
                        skill_path: entry.skill_path,
                        subagents: None,
                        plugin_name: entry.plugin_name,
                    },
                );
            }
            return Ok(serde_json::to_vec_pretty(&migrated)?);
        }
    }
    Ok(empty_wsl_lock_document(scope).to_pretty_bytes()?)
}

fn wsl_legacy_project_lock_locator(
    context: &ContextRef,
    project_path: Option<&str>,
) -> Option<ResourceLocator> {
    project_path.map(|project_path| ResourceLocator {
        environment: context.environment.clone(),
        native_path: format!(
            "{}/.agents/.skill-lock.json",
            project_path.trim_end_matches('/')
        ),
    })
}

fn install_cancelled() -> AppError {
    AppError::Custom {
        message: "Skill installation was cancelled".to_string(),
    }
}

fn build_wsl_eve_normalization_payload(
    host_skill_path: &Path,
    target_dir: &str,
) -> Result<(String, Vec<u8>), AppError> {
    let skill_md = find_skill_md_case_insensitive(host_skill_path).ok_or_else(|| {
        AppError::InvalidSkillMd {
            message: format!("SKILL.md not found in {}", host_skill_path.display()),
        }
    })?;
    let raw = std::fs::read_to_string(skill_md)?;
    let normalized = crate::core::eve::normalize_eve_skill_md(&raw);
    Ok((
        format!("{}/SKILL.md", target_dir.trim_end_matches('/')),
        normalized.into_bytes(),
    ))
}

async fn normalize_wsl_eve_target(
    session: &WslSession,
    host_skill_path: &Path,
    target_dir: &str,
) -> Result<(), AppError> {
    const SCRIPT: &str = r#"
path=$1
dir=${path%/*}
mkdir -p -- "$dir"
for candidate in "$dir"/*; do
  [ -f "$candidate" ] || continue
  base=${candidate##*/}
  lower=$(printf '%s' "$base" | tr '[:upper:]' '[:lower:]')
  if [ "$lower" = 'skill.md' ] && [ "$candidate" != "$path" ]; then
    rm -f -- "$candidate" || exit 20
  fi
done
tmp=$(mktemp "$dir/.skill-md.XXXXXX") || exit 21
trap 'rm -f -- "$tmp"' EXIT HUP INT TERM
cat > "$tmp" || exit 22
mv -f -- "$tmp" "$path" || exit 23
trap - EXIT HUP INT TERM
"#;
    let (target_path, payload) = build_wsl_eve_normalization_payload(host_skill_path, target_dir)?;
    run_wsl_script(
        session,
        SCRIPT,
        &[target_path],
        payload,
        tokio::time::Duration::from_secs(10),
    )
    .await?;
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InstallCancellationDecision {
    Continue,
    AbortNow,
    CommitCompletedThenAbort,
}

fn install_cancellation_decision(
    cancelled: bool,
    completed_skills: usize,
) -> InstallCancellationDecision {
    match (cancelled, completed_skills) {
        (false, _) => InstallCancellationDecision::Continue,
        (true, 0) => InstallCancellationDecision::AbortNow,
        (true, _) => InstallCancellationDecision::CommitCompletedThenAbort,
    }
}

#[derive(Clone, Default)]
pub(crate) struct WslInstallExecutionOptions<'a> {
    pub expected_skill_paths: HashMap<String, String>,
    pub require_complete_targets_for_lock: bool,
    pub prepared_source: Option<&'a PreparedWslInstallSource>,
}

fn select_wsl_install_skills(
    repo_path: &Path,
    requested_names: &[String],
    parsed_subpath: Option<&str>,
    expected_skill_paths: &HashMap<String, String>,
) -> Result<Vec<crate::core::discovery::DiscoveredSkill>, AppError> {
    let options = DiscoverOptions {
        include_internal: true,
        full_depth: false,
    };
    if expected_skill_paths.is_empty() {
        return Ok(discover_skills(repo_path, parsed_subpath, options)?
            .into_iter()
            .filter(|skill| requested_names.contains(&skill.name))
            .collect());
    }

    let mut selected = Vec::new();
    for name in requested_names {
        let expected_path = expected_skill_paths
            .get(name)
            .ok_or_else(|| AppError::InvalidSource {
                value: format!("Missing locked skill path for '{name}'"),
            })?
            .replace('\\', "/");
        let discover_subpath = expected_path
            .rsplit_once('/')
            .map(|(parent, _)| parent)
            .filter(|parent| !parent.is_empty());
        let discovered = discover_skills(
            repo_path,
            discover_subpath,
            DiscoverOptions {
                include_internal: true,
                full_depth: false,
            },
        )?;
        let skill = discovered
            .into_iter()
            .find(|skill| {
                skill.name == *name && skill.relative_path.replace('\\', "/") == expected_path
            })
            .ok_or(AppError::NoSkillsFound)?;
        selected.push(skill);
    }
    Ok(selected)
}

fn should_commit_wsl_lock(
    skill_name: &str,
    failed: &[InstallResult],
    require_complete_targets: bool,
) -> bool {
    !require_complete_targets || !failed.iter().any(|result| result.skill_name == skill_name)
}

pub(crate) async fn prepare_wsl_install_source(
    session: &WslSession,
    parsed: &ParsedSource,
    cancellation: CancellationSignal,
) -> Result<PreparedWslInstallSource, AppError> {
    match parsed.source_type {
        SourceType::Local => {
            let native_path = parsed
                .local_path
                .as_ref()
                .ok_or_else(|| AppError::InvalidSource {
                    value: "Missing local path".to_string(),
                })?
                .to_string_lossy()
                .to_string();
            Ok(PreparedWslInstallSource::Staged(
                stage_wsl_source(
                    session,
                    WslAcquisitionSource::Local { native_path },
                    cancellation,
                )
                .await?,
            ))
        }
        SourceType::GitHub | SourceType::GitLab | SourceType::Git => {
            Ok(PreparedWslInstallSource::Staged(
                stage_wsl_source(
                    session,
                    WslAcquisitionSource::Git {
                        url: parsed.url.clone(),
                        git_ref: parsed.git_ref.clone(),
                    },
                    cancellation,
                )
                .await?,
            ))
        }
        SourceType::WellKnown => {
            let fetch = fetch_wellknown_skills(&parsed.url);
            tokio::pin!(fetch);
            let cancel = async move {
                while !cancellation.is_cancelled() {
                    tokio::time::sleep(tokio::time::Duration::from_millis(20)).await;
                }
            };
            tokio::pin!(cancel);
            tokio::select! {
                result = &mut fetch => Ok(PreparedWslInstallSource::WellKnown(result?)),
                () = &mut cancel => Err(install_cancelled()),
            }
        }
    }
}

async fn resolve_v2_project_path(
    context: &ContextRef,
    session: Option<&WslSession>,
) -> Result<Option<String>, AppError> {
    match &context.scope {
        ContextScope::Global => Ok(None),
        ContextScope::Project { project_id } => match (&context.environment, session) {
            (EnvironmentRef::Host, _) => crate::commands::environments::host_projects_store()?
                .read()?
                .into_iter()
                .find(|project| &project.id == project_id)
                .map(|project| Some(project.native_path))
                .ok_or_else(|| AppError::PathNotFound {
                    path: project_id.clone(),
                }),
            (EnvironmentRef::Wsl { .. }, Some(session)) => {
                crate::commands::environments::read_wsl_projects(session)
                    .await?
                    .into_iter()
                    .find(|project| &project.id == project_id)
                    .map(|project| Some(project.native_path))
                    .ok_or_else(|| AppError::PathNotFound {
                        path: project_id.clone(),
                    })
            }
            (EnvironmentRef::Wsl { distro_name }, None) => Err(AppError::Custom {
                message: format!("WSL distro '{distro_name}' is not connected"),
            }),
        },
    }
}

#[tauri::command]
#[specta::specta]
pub async fn install_skills_v2(
    app: AppHandle,
    context: ContextRef,
    params: InstallParams,
    registry: State<'_, EnvironmentRegistry>,
    controller: State<'_, SingleMutationController>,
) -> Result<InstallResults, AppError> {
    let guard = begin_install_mutation(&controller, context.clone())?;
    match &context.environment {
        EnvironmentRef::Host => {
            let project_path = resolve_v2_project_path(&context, None).await?;
            install_skills_inner(&app, bind_install_params_to_context(params, project_path)).await
        }
        EnvironmentRef::Wsl { distro_name } => {
            let session = registry.get(distro_name).ok_or_else(|| AppError::Custom {
                message: format!("WSL distro '{distro_name}' is not connected"),
            })?;
            let project_path = resolve_v2_project_path(&context, Some(&session)).await?;
            install_skills_wsl_inner(
                &app,
                &context,
                &session,
                bind_install_params_to_context(params, project_path),
                &guard,
            )
            .await
        }
    }
}

async fn install_skills_wsl_inner(
    app: &AppHandle,
    context: &ContextRef,
    session: &WslSession,
    params: InstallParams,
    guard: &MutationGuard<'_>,
) -> Result<InstallResults, AppError> {
    install_skills_wsl_inner_with_options(
        app,
        context,
        session,
        params,
        guard,
        WslInstallExecutionOptions::default(),
    )
    .await
}

pub(crate) async fn install_skills_wsl_inner_with_options(
    app: &AppHandle,
    context: &ContextRef,
    session: &WslSession,
    params: InstallParams,
    guard: &MutationGuard<'_>,
    execution_options: WslInstallExecutionOptions<'_>,
) -> Result<InstallResults, AppError> {
    let parsed = parse_source(&params.source)?;
    let risk_policy = source_risk_policy(&parsed);
    ensure_install_risk_acknowledged(&risk_policy, params.acknowledge_risk)?;
    let project_path = params.project_path.as_deref();
    let cwd = project_path.unwrap_or(session.home.as_str());
    let canonical_root = if matches!(params.scope, crate::models::Scope::Global) {
        format!("{}/.agents/skills", session.home.trim_end_matches('/'))
    } else {
        format!("{}/.agents/skills", cwd.trim_end_matches('/'))
    };
    let lock_locator = wsl_lock_locator(context, session, project_path);
    let legacy_lock_locator = wsl_legacy_project_lock_locator(context, project_path);
    let lock_io = EnvironmentLockIo::Wsl(session.clone());
    let initial_lock_bytes = lock_io.read_optional(&lock_locator).await?;
    let initial_legacy_bytes = match &legacy_lock_locator {
        Some(locator) if initial_lock_bytes.is_none() => lock_io.read_optional(locator).await?,
        _ => None,
    };
    let normalized_initial_lock = normalize_initial_wsl_lock_bytes(
        &params.scope,
        initial_lock_bytes.as_deref(),
        initial_legacy_bytes.as_deref(),
    )?;
    let initial_lock_value: serde_json::Value = serde_json::from_slice(&normalized_initial_lock)?;
    let initial_document = LosslessLockDocument::parse(&normalized_initial_lock)?;
    let initial_snapshots: HashMap<String, LockEntrySnapshot> = params
        .skills
        .iter()
        .map(|name| (name.clone(), initial_document.snapshot(name)))
        .collect();

    let cancellation = guard.cancellation();
    let owned_prepared;
    let prepared = match execution_options.prepared_source {
        Some(prepared) => prepared,
        None => {
            owned_prepared =
                prepare_wsl_install_source(session, &parsed, cancellation.clone()).await?;
            &owned_prepared
        }
    };
    let selected_skills = select_wsl_install_skills(
        prepared.host_repo_path(),
        &params.skills,
        parsed.subpath.as_deref(),
        &execution_options.expected_skill_paths,
    )?;
    if selected_skills.is_empty() {
        return Err(AppError::NoSkillsFound);
    }
    let target_plan = resolve_wsl_install_target_plan(&params, session, cwd)?;
    let include_canonical_result = !target_plan.default_available_agents.is_empty()
        || target_plan.materialize_targets.is_empty();
    let mut successful = Vec::new();
    let mut failed = Vec::new();
    let mut symlink_fallback_agents = Vec::new();
    let mut completed_skill_names = HashSet::new();
    let mut cancel_after_commit = false;
    let total_skills = selected_skills.len();

    for (index, skill) in selected_skills.iter().enumerate() {
        match install_cancellation_decision(
            cancellation.is_cancelled(),
            completed_skill_names.len(),
        ) {
            InstallCancellationDecision::Continue => {}
            InstallCancellationDecision::AbortNow => return Err(install_cancelled()),
            InstallCancellationDecision::CommitCompletedThenAbort => {
                cancel_after_commit = true;
                break;
            }
        }
        let _ = app.emit(
            "install-progress",
            &InstallProgress {
                phase: "installing".to_string(),
                current_skill: skill.name.clone(),
                completed: index,
                total: total_skills,
            },
        );
        let result = materialize_wsl_skill(
            session,
            WslMaterializeRequest {
                source_skill_path: prepared.linux_skill_path(&skill.path)?,
                canonical_root: canonical_root.clone(),
                install_dir_name: skill.install_dir_name.clone(),
                context_root: cwd.to_string(),
                canonical_mode: params.mode.clone(),
                targets: target_plan.materialize_targets.clone(),
            },
        )
        .await?;
        let canonical_path = std::path::PathBuf::from(&result.canonical_path);
        if include_canonical_result {
            successful.push(InstallResult {
                skill_name: skill.name.clone(),
                agent: "__canonical__".to_string(),
                target_id: None,
                subagent: None,
                success: true,
                path: canonical_path.clone(),
                canonical_path: Some(canonical_path.clone()),
                mode: result.canonical_mode,
                symlink_failed: false,
                skipped: false,
                error: None,
                category: InstallResultCategory::DefaultAvailable,
            });
        }
        for mut target in result.targets {
            if target.agent == AgentType::Eve.to_string() && target.success && !target.skipped {
                if let Err(error) =
                    normalize_wsl_eve_target(session, &skill.path, &target.path).await
                {
                    target.success = false;
                    target.error = Some(error.to_string());
                }
            }
            let is_private_copy = target_plan
                .private_copy_targets
                .iter()
                .any(|agent| agent.to_string() == target.agent);
            let category = if !target.success {
                InstallResultCategory::Failed
            } else if target.skipped {
                InstallResultCategory::Skipped
            } else if is_private_copy {
                InstallResultCategory::PrivateCopy
            } else {
                InstallResultCategory::PrivateAdapted
            };
            let subagent = target
                .target_id
                .strip_prefix("eve:")
                .filter(|value| *value != "root")
                .map(str::to_string);
            let install_result = InstallResult {
                skill_name: skill.name.clone(),
                agent: target.agent.clone(),
                target_id: Some(target.target_id),
                subagent,
                success: target.success,
                path: std::path::PathBuf::from(target.path),
                canonical_path: Some(canonical_path.clone()),
                mode: target.mode,
                symlink_failed: target.symlink_failed,
                skipped: target.skipped,
                error: target.error,
                category,
            };
            if install_result.success {
                if install_result.symlink_failed
                    && !symlink_fallback_agents.contains(&install_result.agent)
                {
                    symlink_fallback_agents.push(install_result.agent.clone());
                }
                successful.push(install_result);
            } else {
                failed.push(install_result);
            }
        }
        completed_skill_names.insert(skill.name.clone());
    }

    guard.set_cancelable(false);
    let _ = app.emit(
        "install-progress",
        &InstallProgress {
            phase: "writing_lock".to_string(),
            current_skill: String::new(),
            completed: total_skills,
            total: total_skills,
        },
    );
    let latest_bytes = lock_io.read_optional(&lock_locator).await?;
    let latest_legacy_bytes = match &legacy_lock_locator {
        Some(locator) if latest_bytes.is_none() => lock_io.read_optional(locator).await?,
        _ => None,
    };
    let normalized_latest_lock = normalize_initial_wsl_lock_bytes(
        &params.scope,
        latest_bytes.as_deref(),
        latest_legacy_bytes.as_deref(),
    )?;
    let mut latest_document = LosslessLockDocument::parse(&normalized_latest_lock)?;
    let owner_repo = get_owner_repo(&parsed);
    let lock_subagents = lock_subagents_from_eve_targets(
        &target_plan
            .target_details
            .iter()
            .map(|target| target.subagent.clone())
            .collect::<Vec<_>>(),
    );
    for skill in selected_skills
        .iter()
        .filter(|skill| completed_skill_names.contains(&skill.name))
        .filter(|skill| {
            should_commit_wsl_lock(
                &skill.name,
                &failed,
                execution_options.require_complete_targets_for_lock,
            )
        })
    {
        let remote_hash = resolve_install_hash(
            Some(prepared.host_repo_path()),
            &parsed.source_type,
            owner_repo.as_deref(),
            &skill.relative_path,
            parsed.git_ref.as_deref(),
            Some(skill.path.as_path()),
        )
        .await;
        let computed_hash = compute_skill_folder_hash(&skill.path).unwrap_or_default();
        let existing = initial_lock_value
            .get("skills")
            .and_then(serde_json::Value::as_object)
            .and_then(|skills| skills.get(&skill.name));
        let replacement = build_wsl_lock_entry(
            &params.scope,
            &parsed,
            &params.source,
            skill,
            &remote_hash,
            &computed_hash,
            lock_subagents.clone(),
            existing,
        );
        latest_document.replace_entry(
            &skill.name,
            initial_snapshots
                .get(&skill.name)
                .expect("selected skill snapshot exists"),
            replacement,
        )?;
    }
    lock_io
        .write_atomic(&lock_locator, latest_document.to_pretty_bytes()?)
        .await?;

    if cancel_after_commit {
        return Err(install_cancelled());
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
        target_details: target_plan.target_details,
    })
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
    use crate::environment::acquisition::WslAcquisitionSource;
    use crate::environment::types::EnvironmentRef;
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
    fn fetch_v2_routes_host_sources_to_existing_host_adapter() {
        let parsed = parse_source("owner/repo").expect("parse source");

        assert_eq!(
            fetch_acquisition_route(&EnvironmentRef::Host, &parsed).expect("route source"),
            FetchAcquisitionRoute::Host
        );
    }

    #[test]
    fn fetch_v2_routes_wsl_git_and_local_sources_into_target_distro() {
        let environment = EnvironmentRef::Wsl {
            distro_name: "Ubuntu-24.04".to_string(),
        };
        let git = parse_source("owner/repo#main").expect("parse git");
        let local = parse_source("/home/alice/code/skills").expect("parse local");

        assert_eq!(
            fetch_acquisition_route(&environment, &git).expect("route git"),
            FetchAcquisitionRoute::Wsl(WslAcquisitionSource::Git {
                url: git.url.clone(),
                git_ref: Some("main".to_string()),
            })
        );
        assert_eq!(
            fetch_acquisition_route(&environment, &local).expect("route local"),
            FetchAcquisitionRoute::Wsl(WslAcquisitionSource::Local {
                native_path: "/home/alice/code/skills".to_string(),
            })
        );
    }

    #[test]
    fn fetch_v2_keeps_well_known_http_on_host() {
        let parsed = parse_source("https://skills.example.com").expect("parse source");
        let environment = EnvironmentRef::Wsl {
            distro_name: "Debian".to_string(),
        };

        assert_eq!(
            fetch_acquisition_route(&environment, &parsed).expect("route source"),
            FetchAcquisitionRoute::Host
        );
    }

    fn sample_install_params() -> InstallParams {
        InstallParams {
            source: "owner/repo".to_string(),
            skills: vec!["toolkit".to_string()],
            agents: Vec::new(),
            agent_targets: Vec::new(),
            private_copy_agents: Vec::new(),
            scope: crate::models::Scope::Project,
            project_path: Some("C:\\wrong-project".to_string()),
            mode: InstallMode::Symlink,
            retry: false,
            preserve_existing_modes: false,
            acknowledge_risk: false,
        }
    }

    fn wsl_session() -> crate::environment::wsl::WslSession {
        crate::environment::wsl::WslSession {
            distro_name: "Ubuntu-24.04".to_string(),
            user: "alice".to_string(),
            uid: 1000,
            home: "/home/alice".to_string(),
            xdg_state_home: None,
            config_home: "/home/alice/.config".to_string(),
            environment: Default::default(),
            git_available: true,
        }
    }

    #[test]
    fn install_v2_overrides_legacy_scope_and_path_with_fixed_context() {
        let global = bind_install_params_to_context(sample_install_params(), None);
        assert_eq!(global.scope, crate::models::Scope::Global);
        assert_eq!(global.project_path, None);

        let project = bind_install_params_to_context(
            sample_install_params(),
            Some("/home/alice/code/cgp-be".to_string()),
        );
        assert_eq!(project.scope, crate::models::Scope::Project);
        assert_eq!(
            project.project_path.as_deref(),
            Some("/home/alice/code/cgp-be")
        );
    }

    #[test]
    fn install_v2_rejects_second_mutation_instead_of_queueing() {
        let controller = crate::core::mutation::SingleMutationController::default();
        let context = ContextRef {
            environment: EnvironmentRef::Wsl {
                distro_name: "Ubuntu-24.04".to_string(),
            },
            scope: crate::environment::types::ContextScope::Global,
        };
        let _active = begin_install_mutation(&controller, context.clone()).expect("first install");

        let error = match begin_install_mutation(&controller, context) {
            Ok(_) => panic!("second install must not be queued"),
            Err(error) => error,
        };

        assert!(matches!(error, AppError::MutationBusy));
    }

    #[test]
    fn wsl_install_plan_uses_linux_agent_paths_and_keeps_default_agents_on_canonical() {
        let mut params = sample_install_params();
        params.agents = vec![AgentType::ClaudeCode.to_string()];
        params.private_copy_agents = vec![AgentType::Codex.to_string()];
        params.scope = crate::models::Scope::Global;
        params.project_path = None;

        let plan = resolve_wsl_install_target_plan(&params, &wsl_session(), "/home/alice")
            .expect("resolve targets");

        assert!(plan.default_available_agents.contains(&AgentType::Codex));
        assert_eq!(plan.materialize_targets.len(), 2);
        let claude = plan
            .materialize_targets
            .iter()
            .find(|target| target.agent == AgentType::ClaudeCode.to_string())
            .expect("claude target");
        assert_eq!(claude.skills_root, "/home/alice/.claude/skills");
        assert_eq!(claude.mode, InstallMode::Symlink);
        let codex_copy = plan
            .materialize_targets
            .iter()
            .find(|target| target.agent == AgentType::Codex.to_string())
            .expect("codex copy target");
        assert_eq!(codex_copy.skills_root, "/home/alice/.codex/skills");
        assert_eq!(codex_copy.mode, InstallMode::Copy);
    }

    #[test]
    fn wsl_project_agent_target_skips_when_agent_root_is_absent() {
        let mut params = sample_install_params();
        params.agents = vec![AgentType::ClaudeCode.to_string()];
        params.scope = crate::models::Scope::Project;
        params.project_path = Some("/home/alice/code/app".to_string());

        let plan = resolve_wsl_install_target_plan(&params, &wsl_session(), "/home/alice/code/app")
            .expect("resolve targets");

        assert_eq!(plan.materialize_targets.len(), 1);
        assert_eq!(
            plan.materialize_targets[0].required_root.as_deref(),
            Some("/home/alice/code/app/.claude")
        );
    }

    #[test]
    fn wsl_eve_targets_are_deduplicated_by_concrete_target_id() {
        let mut params = sample_install_params();
        params.agent_targets = vec![
            crate::models::InstallTargetSpec {
                agent: AgentType::Eve,
                subagent: Some("research".to_string()),
            },
            crate::models::InstallTargetSpec {
                agent: AgentType::Eve,
                subagent: Some("research".to_string()),
            },
        ];
        params.scope = crate::models::Scope::Project;
        params.project_path = Some("/home/alice/code/app".to_string());

        let plan = resolve_wsl_install_target_plan(&params, &wsl_session(), "/home/alice/code/app")
            .expect("resolve targets");

        assert_eq!(plan.target_details.len(), 1);
        assert_eq!(plan.materialize_targets.len(), 1);
        assert_eq!(plan.materialize_targets[0].target_id, "eve:research");
    }

    #[test]
    fn wsl_global_lock_entry_keeps_cli_fields_and_existing_install_time() {
        let parsed = parse_source("owner/repo#main").expect("parse source");
        let skill = crate::core::discovery::DiscoveredSkill {
            name: "toolkit".to_string(),
            install_dir_name: "toolkit".to_string(),
            description: "Toolkit".to_string(),
            path: std::path::PathBuf::from("/tmp/toolkit"),
            relative_path: "skills/toolkit".to_string(),
            plugin_name: Some("core".to_string()),
        };
        let existing = serde_json::json!({
            "installedAt":"2026-01-01T00:00:00.000Z",
            "futureEntry": {"enabled": true}
        });

        let entry = build_wsl_lock_entry(
            &crate::models::Scope::Global,
            &parsed,
            "owner/repo#main",
            &skill,
            "remote-hash",
            "computed-hash",
            None,
            Some(&existing),
        );

        assert_eq!(entry["source"], "owner/repo");
        assert_eq!(entry["sourceType"], "github");
        assert_eq!(entry["sourceUrl"], "https://github.com/owner/repo");
        assert_eq!(entry["ref"], "main");
        assert_eq!(entry["skillPath"], "skills/toolkit");
        assert_eq!(entry["skillFolderHash"], "remote-hash");
        assert_eq!(entry["installedAt"], "2026-01-01T00:00:00.000Z");
        assert!(entry["updatedAt"]
            .as_str()
            .is_some_and(|value| !value.is_empty()));
        assert_eq!(entry["pluginName"], "core");
        assert_eq!(entry["futureEntry"]["enabled"], true);
    }

    #[test]
    fn wsl_project_lock_entry_remains_skills_cli_compatible() {
        let parsed = parse_source("/home/alice/source").expect("parse source");
        let skill = crate::core::discovery::DiscoveredSkill {
            name: "toolkit".to_string(),
            install_dir_name: "toolkit".to_string(),
            description: "Toolkit".to_string(),
            path: std::path::PathBuf::from("/tmp/toolkit"),
            relative_path: "toolkit".to_string(),
            plugin_name: None,
        };

        let existing = serde_json::json!({
            "pluginName": "stale-plugin",
            "futureEntry": 42
        });
        let entry = build_wsl_lock_entry(
            &crate::models::Scope::Project,
            &parsed,
            "/home/alice/source",
            &skill,
            "remote-hash",
            "computed-hash",
            Some(vec!["research".to_string()]),
            Some(&existing),
        );

        assert_eq!(entry["source"], "/home/alice/source");
        assert_eq!(entry["sourceType"], "local");
        assert_eq!(entry["computedHash"], "computed-hash");
        assert_eq!(entry["remoteHash"], "remote-hash");
        assert_eq!(entry["skillPath"], "toolkit");
        assert_eq!(entry["subagents"], serde_json::json!(["research"]));
        assert!(entry.get("pluginName").is_none());
        assert_eq!(entry["futureEntry"], 42);
    }

    #[test]
    fn cancellation_before_first_skill_aborts_but_mid_batch_commits_completed_locks() {
        assert_eq!(
            install_cancellation_decision(false, 0),
            InstallCancellationDecision::Continue
        );
        assert_eq!(
            install_cancellation_decision(true, 0),
            InstallCancellationDecision::AbortNow
        );
        assert_eq!(
            install_cancellation_decision(true, 1),
            InstallCancellationDecision::CommitCompletedThenAbort
        );
    }

    #[test]
    fn wsl_project_legacy_lock_is_migrated_before_adding_new_entries() {
        let legacy = br#"{
          "version": 3,
          "skills": {
            "existing": {
              "source": "owner/existing",
              "sourceType": "github",
              "sourceUrl": "https://github.com/owner/existing",
              "skillFolderHash": "tree-hash",
              "installedAt": "2026-01-01T00:00:00.000Z",
              "updatedAt": "2026-01-01T00:00:00.000Z"
            }
          }
        }"#;

        let bytes =
            normalize_initial_wsl_lock_bytes(&crate::models::Scope::Project, None, Some(legacy))
                .expect("migrate legacy lock");
        let value: serde_json::Value = serde_json::from_slice(&bytes).expect("parse migrated lock");

        assert_eq!(value["version"], 1);
        assert_eq!(value["skills"]["existing"]["source"], "owner/existing");
        assert_eq!(value["skills"]["existing"]["remoteHash"], "tree-hash");
        assert_eq!(value["skills"]["existing"]["computedHash"], "");
    }

    #[test]
    fn wsl_eve_target_reuses_existing_frontmatter_normalizer() {
        let temp = tempdir().expect("tempdir");
        fs::write(
            temp.path().join("SKILL.md"),
            "---\nname: toolkit\ndescription: useful\nlicense: MIT\nunknown: drop\n---\nBody\n",
        )
        .expect("write skill");

        let (target_path, payload) = build_wsl_eve_normalization_payload(
            temp.path(),
            "/home/alice/app/agent/skills/toolkit",
        )
        .expect("build Eve payload");
        let normalized = String::from_utf8(payload).expect("utf8 payload");

        assert_eq!(target_path, "/home/alice/app/agent/skills/toolkit/SKILL.md");
        assert!(normalized.contains("description: useful"));
        assert!(normalized.contains("license: MIT"));
        assert!(!normalized.contains("name: toolkit"));
        assert!(!normalized.contains("unknown: drop"));
        assert!(normalized.ends_with("Body\n"));
    }

    #[test]
    fn wsl_update_selection_prefers_exact_locked_path_for_duplicate_name() {
        let temp = tempdir().expect("tempdir");
        fs::create_dir_all(temp.path().join("skills/demo")).expect("priority dir");
        fs::write(
            temp.path().join("skills/demo/SKILL.md"),
            "---\nname: demo\ndescription: priority\n---\n",
        )
        .expect("priority skill");
        fs::create_dir_all(temp.path().join("examples/demo")).expect("locked dir");
        fs::write(
            temp.path().join("examples/demo/SKILL.md"),
            "---\nname: demo\ndescription: locked\n---\n",
        )
        .expect("locked skill");
        let expected = HashMap::from([("demo".to_string(), "examples/demo/SKILL.md".to_string())]);

        let selected =
            select_wsl_install_skills(temp.path(), &["demo".to_string()], None, &expected)
                .expect("select exact skill");

        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].relative_path, "examples/demo/SKILL.md");
    }

    #[test]
    fn wsl_update_lock_policy_keeps_old_hash_on_agent_partial() {
        let failed = vec![InstallResult {
            skill_name: "demo".to_string(),
            agent: "claude-code".to_string(),
            target_id: Some("claude-code".to_string()),
            subagent: None,
            success: false,
            path: std::path::PathBuf::new(),
            canonical_path: None,
            mode: InstallMode::Symlink,
            symlink_failed: false,
            skipped: false,
            error: Some("permission denied".to_string()),
            category: InstallResultCategory::Failed,
        }];

        assert!(should_commit_wsl_lock("demo", &failed, false));
        assert!(!should_commit_wsl_lock("demo", &failed, true));
        assert!(should_commit_wsl_lock("other", &failed, true));
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

        assert!(err.to_string().contains("does not require separate setup"));
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
    fn test_resolve_install_targets_excludes_eve_regular_target_when_concrete_targets_exist() {
        let params = InstallParams {
            source: "owner/repo".to_string(),
            skills: vec!["demo".to_string()],
            agents: vec!["eve".to_string()],
            agent_targets: vec![crate::models::InstallTargetSpec {
                agent: AgentType::Eve,
                subagent: Some("research".to_string()),
            }],
            private_copy_agents: vec![],
            scope: crate::models::Scope::Project,
            project_path: Some("/tmp/project".to_string()),
            mode: InstallMode::Copy,
            retry: false,
            preserve_existing_modes: false,
            acknowledge_risk: false,
        };

        let plan =
            resolve_install_target_plan(&params, compute_install_behavior(false), "/tmp/project")
                .unwrap();

        assert!(!plan.install_targets.contains(&AgentType::Eve));
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

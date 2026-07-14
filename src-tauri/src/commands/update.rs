//! 更新检测相关的 Tauri Commands
//!
//! 提供命令：
//! - check_updates: 检测指定 scope 的 skills 是否有更新

use crate::core::agents::AgentType;
use crate::core::installer::{
    detect_install_mode, detect_installed_agents_for_skill, install_skill_to_agents_with_modes,
};
use crate::core::local_lock::{
    compute_skill_folder_hash, read_local_lock, LocalSkillLockEntry, LocalSkillLockFile,
};
use crate::core::lock_repository::{LockMutationTargets, LockRepository, LockTarget};
use crate::core::lossless_lock::LockSchema;
use crate::core::mutation::{MutationGuard, MutationKind, MutationPhase, SingleMutationController};
use crate::core::skill_lock::{read_scoped_lock, SkillLockEntry, SkillLockFile};
use crate::core::wellknown::fetch_wellknown_skills;
use crate::core::{
    build_update_group_key, build_update_target, clone_repo_with_progress, compute_local_tree_sha,
    discover_skills, CloneProgress, DiscoverOptions, DiscoveredSkill, UpdateSourceParts,
};
use crate::core::{
    derive_update_capability, normalize_global_lock_entry, normalize_local_lock_entry,
};
use crate::core::{fetch_skill_folder_hash, fetch_skill_folder_hashes_batch};
use crate::environment::context_resolver::ContextResolver;
use crate::environment::lock_io::EnvironmentLockIo;
use crate::environment::service::{EnvironmentService, InspectRequest, ResolvedContext};
#[cfg(test)]
use crate::environment::types::ContextScope;
use crate::environment::types::{ContextRef, EnvironmentRef, ResourceLocator};
use crate::environment::wsl::EnvironmentRegistry;
use crate::error::AppError;
use crate::models::{
    InstallMode, InstallParams, InstallTargetSpec, Scope, UpdateSkillAgentResult,
    UpdateSkillAgentStatus, UpdateSkillItemResult, UpdateSkillResponse, UpdateSkillStatus,
    UpdateSkillSummary,
};
use serde::Serialize;
use specta::Type;
use std::collections::HashMap;
use std::path::Path;
use std::time::Instant;
use tauri::{AppHandle, State};

type UpdateCheckGroupKey = (String, Option<String>);

#[derive(Debug, Clone)]
struct UpdateCheckSkill {
    name: String,
    source: String,
    source_url: Option<String>,
    ref_name: Option<String>,
    skill_path: String,
    local_hash: String,
}

struct PreparedUpdateChecks {
    groups: HashMap<UpdateCheckGroupKey, Vec<UpdateCheckSkill>>,
    immediate_results: Vec<SkillUpdateInfo>,
}

fn prepare_update_checks(
    entries: Vec<(String, crate::core::NormalizedUpdateMetadata)>,
) -> PreparedUpdateChecks {
    let mut groups: HashMap<UpdateCheckGroupKey, Vec<UpdateCheckSkill>> = HashMap::new();
    let mut immediate_results = Vec::new();
    for (name, metadata) in entries {
        let capability = derive_update_capability(&metadata);
        if !capability.can_check_for_updates {
            immediate_results.push(SkillUpdateInfo {
                name,
                source: metadata.source,
                has_update: false,
                status: SkillUpdateCheckStatus::CannotCheck,
                reason: capability.reason,
                git_ref: metadata.ref_name,
                source_url: metadata.source_url,
                skill_path: metadata.skill_path,
            });
            continue;
        }
        groups
            .entry((metadata.source.clone(), metadata.ref_name.clone()))
            .or_default()
            .push(UpdateCheckSkill {
                name,
                source: metadata.source,
                source_url: metadata.source_url,
                ref_name: metadata.ref_name,
                skill_path: metadata
                    .skill_path
                    .expect("checkable metadata has skill path"),
                local_hash: metadata
                    .remote_hash
                    .expect("checkable metadata has remote hash"),
            });
    }
    PreparedUpdateChecks {
        groups,
        immediate_results,
    }
}

async fn check_updates_from_metadata(
    entries: Vec<(String, crate::core::NormalizedUpdateMetadata)>,
) -> Result<Vec<SkillUpdateInfo>, AppError> {
    let prepared = prepare_update_checks(entries);
    let mut results = prepared.immediate_results;
    for ((source, ref_name), skills) in &prepared.groups {
        let paths: Vec<(String, String)> = skills
            .iter()
            .map(|skill| (skill.name.clone(), skill.skill_path.clone()))
            .collect();
        match fetch_skill_folder_hashes_batch(source, &paths, ref_name.as_deref()).await {
            Ok(hashes) => {
                for skill in skills {
                    results.push(build_batch_check_result(
                        skill,
                        hashes.get(&skill.name).and_then(|hash| hash.as_deref()),
                    ));
                }
            }
            Err(error) => {
                let reason = match &error {
                    AppError::GitHubApiError { reason, .. } => reason.clone(),
                    _ => "upstream-unavailable".to_string(),
                };
                for skill in skills {
                    results.push(SkillUpdateInfo {
                        name: skill.name.clone(),
                        source: skill.source.clone(),
                        has_update: false,
                        status: SkillUpdateCheckStatus::CannotCheck,
                        reason: Some(reason.clone()),
                        git_ref: skill.ref_name.clone(),
                        source_url: skill.source_url.clone(),
                        skill_path: Some(skill.skill_path.clone()),
                    });
                }
            }
        }
    }
    Ok(results)
}

fn parse_wsl_update_metadata(
    scope: &Scope,
    current: Option<&[u8]>,
    legacy_project: Option<&[u8]>,
) -> Result<Vec<(String, crate::core::NormalizedUpdateMetadata)>, AppError> {
    match scope {
        Scope::Global => {
            let lock = match current {
                Some(bytes) => serde_json::from_slice::<SkillLockFile>(bytes)?,
                None => SkillLockFile::empty(),
            };
            Ok(lock
                .skills
                .into_iter()
                .map(|(name, entry)| (name, normalize_global_lock_entry(&entry)))
                .collect())
        }
        Scope::Project => {
            if let Some(bytes) = current {
                let lock = serde_json::from_slice::<LocalSkillLockFile>(bytes)?;
                return Ok(lock
                    .skills
                    .into_iter()
                    .map(|(name, entry)| (name, normalize_local_lock_entry(&entry)))
                    .collect());
            }
            let lock = match legacy_project {
                Some(bytes) => serde_json::from_slice::<SkillLockFile>(bytes)?,
                None => SkillLockFile::empty(),
            };
            Ok(lock
                .skills
                .into_iter()
                .map(|(name, entry)| (name, normalize_global_lock_entry(&entry)))
                .collect())
        }
    }
}

fn build_wsl_update_install_params(
    scope: Scope,
    project_path: Option<&str>,
    skill_name: &str,
    metadata: &crate::core::NormalizedUpdateMetadata,
    snapshot: &crate::environment::service::SkillEntrySnapshot,
) -> InstallParams {
    let source_base = metadata
        .source_url
        .clone()
        .unwrap_or_else(|| metadata.source.clone());
    let source = metadata
        .ref_name
        .as_ref()
        .map(|git_ref| format!("{source_base}#{git_ref}"))
        .unwrap_or(source_base);
    let mut agents = Vec::new();
    for agent in snapshot
        .private_adapted_agents
        .iter()
        .chain(snapshot.private_only_agents.iter())
    {
        let value = agent.to_string();
        if !agents.contains(&value) {
            agents.push(value);
        }
    }
    InstallParams {
        source,
        skills: vec![skill_name.to_string()],
        agents,
        agent_targets: snapshot
            .eve_targets
            .iter()
            .map(|target| InstallTargetSpec {
                agent: AgentType::Eve,
                subagent: target.subagent.clone(),
            })
            .collect(),
        private_copy_agents: snapshot
            .private_copy_agents
            .iter()
            .map(ToString::to_string)
            .collect(),
        scope,
        project_path: project_path.map(str::to_string),
        mode: InstallMode::Symlink,
        retry: false,
        preserve_existing_modes: true,
        acknowledge_risk: true,
    }
}

fn build_wsl_update_item(
    skill_name: &str,
    metadata: &crate::core::NormalizedUpdateMetadata,
    install: crate::models::InstallResults,
    duration_ms: u32,
) -> UpdateSkillItemResult {
    let explicit_agents: std::collections::HashSet<String> = install
        .successful
        .iter()
        .chain(install.failed.iter())
        .filter(|result| result.agent != "__canonical__")
        .map(|result| result.agent.clone())
        .collect();
    let canonical_mode = install
        .successful
        .iter()
        .find(|result| result.agent == "__canonical__")
        .map(|result| result.mode.clone());
    let mut agent_results = Vec::new();
    for agent in &install.default_available_agents {
        if explicit_agents.contains(agent) {
            continue;
        }
        agent_results.push(UpdateSkillAgentResult {
            agent: agent.clone(),
            target_id: None,
            subagent: None,
            status: UpdateSkillAgentStatus::Success,
            mode: canonical_mode.clone(),
            error: None,
            duration_ms: None,
        });
    }
    for result in install.successful.into_iter().chain(install.failed) {
        if result.agent == "__canonical__" {
            continue;
        }
        agent_results.push(UpdateSkillAgentResult {
            agent: result.agent,
            target_id: result.target_id,
            subagent: result.subagent,
            status: if result.skipped {
                UpdateSkillAgentStatus::Skipped
            } else if result.success {
                UpdateSkillAgentStatus::Success
            } else {
                UpdateSkillAgentStatus::Failed
            },
            mode: Some(result.mode),
            error: result.error,
            duration_ms: None,
        });
    }
    let status = derive_skill_status(&agent_results);
    let error = matches!(
        status,
        UpdateSkillStatus::Failed | UpdateSkillStatus::Partial
    )
    .then(|| {
        agent_results
            .iter()
            .find(|result| result.status == UpdateSkillAgentStatus::Failed)
            .and_then(|result| result.error.clone())
            .unwrap_or_else(|| "Some agents failed to update".to_string())
    });
    UpdateSkillItemResult {
        name: skill_name.to_string(),
        status,
        error,
        reason: None,
        source: Some(metadata.source.clone()),
        source_url: metadata.source_url.clone(),
        git_ref: metadata.ref_name.clone(),
        skill_path: metadata.skill_path.clone(),
        warnings: install
            .symlink_fallback_agents
            .into_iter()
            .map(|agent| format!("Symlink failed for {agent}; used copy mode"))
            .collect(),
        duration_ms: Some(duration_ms),
        agent_results,
    }
}

fn update_lock_locators(context: &ResolvedContext) -> (ResourceLocator, Option<ResourceLocator>) {
    let legacy = context.project.as_ref().map(|project| ResourceLocator {
        environment: context.context.environment.clone(),
        native_path: format!(
            "{}/.agents/.skill-lock.json",
            project.native_path.trim_end_matches('/')
        ),
    });
    (context.lock.clone(), legacy)
}

fn update_lock_schema(scope: &Scope) -> LockSchema {
    match scope {
        Scope::Global => LockSchema::Global,
        Scope::Project => LockSchema::Project,
    }
}

fn resolved_update_lock_target(context: &ResolvedContext, scope: &Scope) -> LockTarget {
    let (primary, legacy) = update_lock_locators(context);
    LockTarget {
        primary,
        legacy,
        schema: update_lock_schema(scope),
    }
}

fn host_update_lock_target(
    scope: &Scope,
    project_path: Option<&str>,
) -> Result<LockTarget, AppError> {
    match scope {
        Scope::Global => Ok(LockTarget {
            primary: ResourceLocator {
                environment: EnvironmentRef::Host,
                native_path: crate::core::skill_lock::get_skill_lock_path()
                    .to_string_lossy()
                    .to_string(),
            },
            legacy: None,
            schema: LockSchema::Global,
        }),
        Scope::Project => {
            let project_path = project_path.ok_or_else(|| AppError::InvalidSource {
                value: "Project path is required for project scope".to_string(),
            })?;
            let project_path = Path::new(project_path);
            Ok(LockTarget {
                primary: ResourceLocator {
                    environment: EnvironmentRef::Host,
                    native_path: project_path
                        .join("skills-lock.json")
                        .to_string_lossy()
                        .to_string(),
                },
                legacy: Some(ResourceLocator {
                    environment: EnvironmentRef::Host,
                    native_path: project_path
                        .join(".agents/.skill-lock.json")
                        .to_string_lossy()
                        .to_string(),
                }),
                schema: LockSchema::Project,
            })
        }
    }
}

fn normalized_update_metadata_from_raw(
    scope: &Scope,
    raw: &serde_json::Value,
) -> Result<crate::core::NormalizedUpdateMetadata, AppError> {
    match scope {
        Scope::Global => {
            let entry: SkillLockEntry = serde_json::from_value(raw.clone())?;
            Ok(normalize_global_lock_entry(&entry))
        }
        Scope::Project => {
            let entry: LocalSkillLockEntry = serde_json::from_value(raw.clone())?;
            Ok(normalize_local_lock_entry(&entry))
        }
    }
}

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
    DeletedUpstream,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skill_path: Option<String>,
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
    context: ContextRef,
    registry: State<'_, EnvironmentRegistry>,
) -> Result<Vec<SkillUpdateInfo>, AppError> {
    match &context.environment {
        EnvironmentRef::Host => {
            let resolved = ContextResolver::resolve_host(context)?;
            let scope = if resolved.project.is_some() {
                Scope::Project
            } else {
                Scope::Global
            };
            check_updates_inner(
                scope,
                resolved
                    .project
                    .as_ref()
                    .map(|project| project.native_path.as_str()),
            )
            .await
        }
        EnvironmentRef::Wsl { distro_name } => {
            let distro_name = distro_name.clone();
            let retry_context = context.clone();
            registry
                .with_session_retry(&distro_name, move |session| {
                    let context = retry_context.clone();
                    async move {
                        let resolved = ContextResolver::resolve_wsl(context, &session).await?;
                        let scope = if resolved.project.is_some() {
                            Scope::Project
                        } else {
                            Scope::Global
                        };
                        let io = EnvironmentLockIo::Wsl(session.clone());
                        let (locator, legacy_locator) = update_lock_locators(&resolved);
                        let current = io.read_optional(&locator).await?;
                        let legacy = match legacy_locator {
                            Some(locator) if current.is_none() => {
                                io.read_optional(&locator).await?
                            }
                            _ => None,
                        };
                        check_updates_from_metadata(parse_wsl_update_metadata(
                            &scope,
                            current.as_deref(),
                            legacy.as_deref(),
                        )?)
                        .await
                    }
                })
                .await
        }
    }
}

async fn check_updates_inner(
    scope: Scope,
    project_path: Option<&str>,
) -> Result<Vec<SkillUpdateInfo>, AppError> {
    let entries = match scope {
        Scope::Global => {
            let lock = read_scoped_lock(None)?;
            lock.skills
                .into_iter()
                .map(|(name, entry)| (name, normalize_global_lock_entry(&entry)))
                .collect()
        }
        Scope::Project => {
            if let Some(pp) = project_path {
                let local_lock = read_local_lock(pp)?;
                local_lock
                    .skills
                    .into_iter()
                    .map(|(name, entry)| (name, normalize_local_lock_entry(&entry)))
                    .collect()
            } else {
                Vec::new()
            }
        }
    };
    check_updates_from_metadata(entries).await
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

#[derive(Debug, Clone)]
struct WslBatchUpdatePlanItem {
    name: String,
    metadata: crate::core::NormalizedUpdateMetadata,
    source_key: crate::core::UpdateGroupKey,
}

struct WslBatchUpdatePlan {
    ready: Vec<WslBatchUpdatePlanItem>,
    immediate_results: Vec<UpdateSkillItemResult>,
}

fn missing_batch_lock_result(name: &str) -> UpdateSkillItemResult {
    UpdateSkillItemResult {
        name: name.to_string(),
        status: UpdateSkillStatus::Failed,
        error: Some(format!("Skill '{name}' not found in lock file")),
        reason: Some("missing-lock-entry".to_string()),
        source: None,
        source_url: None,
        git_ref: None,
        skill_path: None,
        warnings: Vec::new(),
        duration_ms: None,
        agent_results: Vec::new(),
    }
}

fn prepare_wsl_batch_plan(
    names: &[String],
    entries: Vec<(String, crate::core::NormalizedUpdateMetadata)>,
) -> WslBatchUpdatePlan {
    let metadata_by_name: HashMap<String, crate::core::NormalizedUpdateMetadata> =
        entries.into_iter().collect();
    let mut seen = std::collections::HashSet::new();
    let mut ready = Vec::new();
    let mut immediate_results = Vec::new();

    for name in names {
        if !seen.insert(name.clone()) {
            continue;
        }
        let Some(metadata) = metadata_by_name.get(name).cloned() else {
            immediate_results.push(missing_batch_lock_result(name));
            continue;
        };
        let capability = derive_update_capability(&metadata);
        if !capability.can_run_update {
            immediate_results.push(build_skipped_update_result(
                name,
                &metadata,
                capability.reason.as_deref().unwrap_or("cannot-update"),
            ));
            continue;
        }
        let source_url = metadata
            .source_url
            .clone()
            .unwrap_or_else(|| metadata.source.clone());
        ready.push(WslBatchUpdatePlanItem {
            name: name.clone(),
            source_key: build_update_group_key(
                &metadata.source_type,
                &source_url,
                metadata.ref_name.as_deref(),
            ),
            metadata,
        });
    }

    WslBatchUpdatePlan {
        ready,
        immediate_results,
    }
}

fn batch_source_needs_acquisition<T>(
    prepared: &HashMap<crate::core::UpdateGroupKey, T>,
    failed: &HashMap<crate::core::UpdateGroupKey, String>,
    key: &crate::core::UpdateGroupKey,
) -> bool {
    !prepared.contains_key(key) && !failed.contains_key(key)
}

fn append_cancelled_batch_results(
    results: &mut Vec<UpdateSkillItemResult>,
    pending: &[WslBatchUpdatePlanItem],
) {
    results.extend(
        pending
            .iter()
            .map(|item| build_skipped_update_result(&item.name, &item.metadata, "cancelled")),
    );
}

fn failed_wsl_batch_result(
    item: &WslBatchUpdatePlanItem,
    error: impl Into<String>,
    duration_ms: Option<u32>,
) -> UpdateSkillItemResult {
    UpdateSkillItemResult {
        name: item.name.clone(),
        status: UpdateSkillStatus::Failed,
        error: Some(error.into()),
        reason: None,
        source: Some(item.metadata.source.clone()),
        source_url: item.metadata.source_url.clone(),
        git_ref: item.metadata.ref_name.clone(),
        skill_path: item.metadata.skill_path.clone(),
        warnings: Vec::new(),
        duration_ms,
        agent_results: Vec::new(),
    }
}

fn order_batch_results(names: &[String], results: &mut [UpdateSkillItemResult]) {
    let positions: HashMap<&str, usize> = names
        .iter()
        .enumerate()
        .map(|(index, name)| (name.as_str(), index))
        .collect();
    results.sort_by_key(|result| {
        positions
            .get(result.name.as_str())
            .copied()
            .unwrap_or(usize::MAX)
    });
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct UpdateLockEntry {
    source: String,
    source_type: String,
    source_url: String,
    skill_path: Option<String>,
    plugin_name: Option<String>,
    ref_name: Option<String>,
}

fn global_update_entry_from_lock_entry(
    entry: &crate::core::skill_lock::SkillLockEntry,
) -> UpdateLockEntry {
    let metadata = normalize_global_lock_entry(entry);
    UpdateLockEntry {
        source: metadata.source.clone(),
        source_type: metadata.source_type.clone(),
        source_url: metadata
            .source_url
            .unwrap_or_else(|| metadata.source.clone()),
        skill_path: metadata.skill_path,
        plugin_name: entry.plugin_name.clone(),
        ref_name: metadata.ref_name,
    }
}

fn build_batch_check_result(
    skill: &UpdateCheckSkill,
    remote_hash: Option<&str>,
) -> SkillUpdateInfo {
    match remote_hash {
        Some(remote_hash) => {
            let has_update = remote_hash != skill.local_hash;
            SkillUpdateInfo {
                name: skill.name.clone(),
                source: skill.source.clone(),
                has_update,
                status: if has_update {
                    SkillUpdateCheckStatus::UpdateAvailable
                } else {
                    SkillUpdateCheckStatus::UpToDate
                },
                reason: None,
                git_ref: skill.ref_name.clone(),
                source_url: skill.source_url.clone(),
                skill_path: Some(skill.skill_path.clone()),
            }
        }
        None => SkillUpdateInfo {
            name: skill.name.clone(),
            source: skill.source.clone(),
            has_update: false,
            status: SkillUpdateCheckStatus::DeletedUpstream,
            reason: Some("deleted-upstream".to_string()),
            git_ref: skill.ref_name.clone(),
            source_url: skill.source_url.clone(),
            skill_path: Some(skill.skill_path.clone()),
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

fn eve_targets_from_source_or_root_install(
    source_type: &str,
    subagents: Option<&[String]>,
    project_path: &str,
    skill_name: &str,
) -> Vec<Option<String>> {
    if !matches!(
        source_type,
        "github" | "git" | "gitlab" | "well-known" | "wellknown" | "direct-url" | "directurl"
    ) {
        return Vec::new();
    }

    if let Some(subagents) = subagents {
        return subagents
            .iter()
            .map(|value| {
                if value.is_empty() {
                    None
                } else {
                    Some(value.clone())
                }
            })
            .collect();
    }

    let root_path = crate::core::eve::eve_root_skills_dir(project_path)
        .join(crate::core::skill::sanitize_name(skill_name));
    if root_path.exists() {
        vec![None]
    } else {
        Vec::new()
    }
}

fn eve_targets_from_lock_or_root_install(
    entry: &LocalSkillLockEntry,
    project_path: &str,
    skill_name: &str,
) -> Vec<Option<String>> {
    eve_targets_from_source_or_root_install(
        &entry.source_type,
        entry.subagents.as_deref(),
        project_path,
        skill_name,
    )
}

/// 更新指定 skill
///
/// 本质是"重新安装"：从 lock 文件读取来源信息，构造安装 URL，复用安装逻辑。
/// 与 CLI update 命令行为一致。
#[tauri::command]
#[specta::specta]
pub async fn update_skill(
    app: AppHandle,
    context: ContextRef,
    name: String,
    registry: State<'_, EnvironmentRegistry>,
    controller: State<'_, SingleMutationController>,
) -> Result<UpdateSkillResponse, AppError> {
    let guard = controller.begin(MutationKind::Update, context.clone())?;
    match &context.environment {
        EnvironmentRef::Host => {
            let resolved = ContextResolver::resolve_host(context)?;
            let (scope, project_path) = match resolved.project {
                Some(project) => (Scope::Project, Some(project.native_path)),
                None => (Scope::Global, None),
            };
            Ok(update_skill_inner(&app, scope, &name, project_path.as_deref()).await)
        }
        EnvironmentRef::Wsl { distro_name } => {
            let distro_name = distro_name.clone();
            let retry_app = app.clone();
            let retry_context = context.clone();
            let guard = &guard;
            registry
                .with_session(&distro_name, move |session| {
                    let app = retry_app.clone();
                    let context = retry_context.clone();
                    let name = name.clone();
                    async move {
                        let started = Instant::now();
                        let resolved =
                            ContextResolver::resolve_wsl(context.clone(), &session).await?;
                        let scope = if resolved.project.is_some() {
                            Scope::Project
                        } else {
                            Scope::Global
                        };
                        let project_path = resolved
                            .project
                            .as_ref()
                            .map(|project| project.native_path.as_str());
                        let lock_repository =
                            LockRepository::new(EnvironmentLockIo::Wsl(session.clone()));
                        let document = lock_repository
                            .read_document(&resolved_update_lock_target(&resolved, &scope))
                            .await?
                            .into_value();
                        let metadata = document["skills"]
                            .get(&name)
                            .map(|raw| normalized_update_metadata_from_raw(&scope, raw))
                            .transpose()?
                            .ok_or_else(|| AppError::InvalidSource {
                                value: format!("Skill '{name}' not found in lock file"),
                            })?;
                        if ensure_can_run_update(&metadata).is_err() {
                            let capability = derive_update_capability(&metadata);
                            let item = build_skipped_update_result(
                                &name,
                                &metadata,
                                capability.reason.as_deref().unwrap_or("cannot-update"),
                            );
                            return Ok(UpdateSkillResponse {
                                summary: summarize_results(std::slice::from_ref(&item)),
                                results: vec![item],
                            });
                        }
                        let context_root = resolved.context_root();
                        let snapshot = EnvironmentService::Wsl(session.clone())
                            .inspect(&InspectRequest {
                                context: resolved.clone(),
                            })
                            .await?
                            .skills
                            .into_iter()
                            .find(|skill| skill.name == name)
                            .unwrap_or_else(|| {
                                fallback_wsl_update_snapshot(&session, &scope, context_root, &name)
                            });
                        let params = build_wsl_update_install_params(
                            scope.clone(),
                            project_path,
                            &name,
                            &metadata,
                            &snapshot,
                        );
                        let expected_path =
                            metadata
                                .skill_path
                                .clone()
                                .ok_or_else(|| AppError::InvalidSource {
                                    value: "Missing locked skill path".to_string(),
                                })?;
                        let install =
                            crate::commands::install::install_skills_wsl_inner_with_options(
                                &app,
                                &context,
                                &session,
                                params,
                                guard,
                                crate::commands::install::WslInstallExecutionOptions {
                                    expected_skill_paths: HashMap::from([(
                                        name.clone(),
                                        expected_path,
                                    )]),
                                    require_complete_targets_for_lock: true,
                                    defer_lock_commit: false,
                                    prepared_source: None,
                                    resolved_context: Some(resolved),
                                },
                            )
                            .await?;
                        let item = build_wsl_update_item(
                            &name,
                            &metadata,
                            install.results,
                            elapsed_ms(&started),
                        );
                        Ok(UpdateSkillResponse {
                            summary: summarize_results(std::slice::from_ref(&item)),
                            results: vec![item],
                        })
                    }
                })
                .await
        }
    }
}

fn fallback_wsl_update_snapshot(
    session: &crate::environment::wsl::WslSession,
    scope: &Scope,
    context_root: &str,
    skill_name: &str,
) -> crate::environment::service::SkillEntrySnapshot {
    let resolver = crate::environment::agent_environment::AgentEnvironmentResolver::new(
        crate::environment::agent_environment::AgentEnvironmentContext {
            home: session.home.clone(),
            config_home: session.config_home.clone(),
            env: session.environment.clone(),
        },
    );
    let is_global = matches!(scope, Scope::Global);
    crate::environment::service::SkillEntrySnapshot {
        name: skill_name.to_string(),
        description: String::new(),
        canonical_path: format!(
            "{}/.agents/skills/{}",
            context_root.trim_end_matches('/'),
            crate::core::skill::sanitize_name(skill_name)
        ),
        canonical_present: false,
        agents: Vec::new(),
        card_agents: Vec::new(),
        default_available_agents: AgentType::all()
            .filter(|agent| {
                resolver
                    .target(*agent, is_global, context_root)
                    .default_available
            })
            .collect(),
        private_adapted_agents: Vec::new(),
        duplicate_copy_agents: Vec::new(),
        private_only_agents: Vec::new(),
        private_copy_agents: Vec::new(),
        eve_targets: Vec::new(),
    }
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
    let lock_repository = LockRepository::new(EnvironmentLockIo::Host);
    let mut lock_transaction = lock_repository
        .begin(
            host_update_lock_target(&scope, project_path)?,
            LockMutationTargets {
                entries: vec![skill_name.to_string()],
                default_target_agents: false,
            },
        )
        .await?;
    let raw_entry = lock_transaction
        .initial_entry(skill_name)
        .cloned()
        .ok_or_else(|| AppError::InvalidSource {
            value: format!("Skill '{skill_name}' not found in lock file"),
        })?;

    // 1. 根据 scope 读取对应的 lock 文件
    let (
        entry_source,
        entry_source_type,
        entry_source_url,
        entry_skill_path,
        entry_plugin_name,
        entry_ref_name,
        global_lock_entry,
        project_lock_entry,
    ) = match scope {
        Scope::Global => {
            let entry: SkillLockEntry = serde_json::from_value(raw_entry)?;
            let update_entry = global_update_entry_from_lock_entry(&entry);
            (
                update_entry.source,
                update_entry.source_type,
                update_entry.source_url,
                update_entry.skill_path,
                update_entry.plugin_name,
                update_entry.ref_name,
                Some(entry),
                None,
            )
        }
        Scope::Project => {
            let entry: LocalSkillLockEntry = serde_json::from_value(raw_entry)?;
            let metadata = normalize_local_lock_entry(&entry);
            (
                entry.source.clone(),
                entry.source_type.clone(),
                metadata.source_url.unwrap_or_default(),
                entry.skill_path.clone(),
                entry.plugin_name.clone(),
                entry.ref_name.clone(),
                None,
                Some(entry),
            )
        }
    };

    // 2. 入口校验：提前拒绝不可执行的更新（如 local 类型、缺失 skillPath 等）
    let metadata = crate::core::NormalizedUpdateMetadata {
        source: entry_source.clone(),
        source_type: entry_source_type.clone(),
        source_url: crate::core::recover_source_url(
            &entry_source,
            &entry_source_type,
            Some(entry_source_url.as_str()),
        ),
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
        source_url: metadata
            .source_url
            .clone()
            .unwrap_or_else(|| entry_source.clone()),
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
        .ok_or(AppError::NoSkillsFound)?;

    let eve_update_targets = match (&scope, project_path, project_lock_entry.as_ref()) {
        (Scope::Project, Some(pp), Some(entry)) => {
            eve_targets_from_lock_or_root_install(entry, pp, skill_name)
        }
        _ => Vec::new(),
    };

    // 6. 检测已安装的 agents (通过文件系统检测,fallback 仅保留当前 scope 自动应用的 agents)。
    //    注意:fallback 故意不再合并 `AgentType::detect_installed()` —— 否则会把 skill
    //    装到从未链接过的 agent 上 (例如用户原本只装在 cursor、之后手动卸了 cursor 时)。
    //    canonical 已经被 install_skill_to_agents_with_modes 写为新内容，自动应用 agents
    //    直接读对应 scope 的 canonical 即可。
    let install_scope = match scope {
        Scope::Global => crate::models::Scope::Global,
        Scope::Project => crate::models::Scope::Project,
    };
    let mut target_agents =
        detect_installed_agents_for_skill(skill_name, &install_scope, project_path);
    let had_detected_targets = !target_agents.is_empty();
    if matches!(scope, Scope::Project) {
        target_agents.retain(|agent| *agent != AgentType::Eve);
    }
    if target_agents.is_empty() && !had_detected_targets {
        let is_global = matches!(install_scope, crate::models::Scope::Global);
        let cwd = project_path.unwrap_or(".");
        target_agents = AgentType::get_automatic_agents_for_scope(is_global, cwd);
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
    let mut agent_results: Vec<UpdateSkillAgentResult> = per_agent_results
        .into_iter()
        .map(|r| UpdateSkillAgentResult {
            agent: r.agent,
            target_id: None,
            subagent: None,
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

    for subagent in eve_update_targets {
        let result = crate::core::installer::install_skill_for_eve_target(
            &skill.path,
            &skill.name,
            project_path.unwrap_or("."),
            subagent.as_deref(),
        );
        agent_results.push(UpdateSkillAgentResult {
            agent: "eve".to_string(),
            target_id: result.target_id,
            subagent: result.subagent,
            status: if result.skipped {
                UpdateSkillAgentStatus::Skipped
            } else if result.success {
                UpdateSkillAgentStatus::Success
            } else {
                UpdateSkillAgentStatus::Failed
            },
            mode: Some(result.mode),
            error: result.error,
            duration_ms: None,
        });
    }

    // 9. 仅当所有 agent 都成功时,才把新 hash 写入 lock。
    //    Partial / Failed 时保留旧 hash —— 这样下次 check_updates 仍会提示 update-available,
    //    用户重试只会重装失败的 agent,失败信息不会从 UI 上彻底消失。
    let status = derive_skill_status(&agent_results);
    let _ = app.emit(
        "update-progress",
        &update_progress_payload(skill_name, &scope, project_path, "writing_lock"),
    );

    if matches!(status, UpdateSkillStatus::Success) {
        let (final_hash, hash_warning) = resolve_post_update_hash(PostUpdateHashRequest {
            scope: &scope,
            skill_name,
            project_path,
            source_type: &entry_source_type,
            source: &entry_source,
            skill_path: entry_skill_path.as_deref(),
            ref_name: entry_ref_name.as_deref(),
            clone_repo_path: Some(skills_dir.as_path()),
        })
        .await;
        if let Some(w) = hash_warning {
            warnings.push(w);
        }

        let replacement = match scope {
            Scope::Global => {
                let now = chrono::Utc::now()
                    .format("%Y-%m-%dT%H:%M:%S%.3fZ")
                    .to_string();
                let installed_at = global_lock_entry
                    .as_ref()
                    .map(|entry| entry.installed_at.clone())
                    .filter(|installed_at| !installed_at.is_empty())
                    .unwrap_or_else(|| now.clone());
                Some(serde_json::to_value(SkillLockEntry {
                    source: entry_source.clone(),
                    source_type: entry_source_type.clone(),
                    source_url: entry_source_url.clone(),
                    ref_name: entry_ref_name.clone(),
                    skill_path: entry_skill_path.clone(),
                    skill_folder_hash: final_hash,
                    installed_at,
                    updated_at: now,
                    plugin_name: entry_plugin_name.clone(),
                })?)
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
                        subagents: project_lock_entry
                            .as_ref()
                            .and_then(|entry| entry.subagents.clone()),
                        plugin_name: entry_plugin_name.clone(),
                    };
                    Some(serde_json::to_value(entry)?)
                } else {
                    None
                }
            }
        };
        if let Some(replacement) = replacement {
            lock_transaction.replace_entry(skill_name, replacement)?;
            lock_transaction.commit().await?;
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
    app: AppHandle,
    context: ContextRef,
    names: Vec<String>,
    registry: State<'_, EnvironmentRegistry>,
    controller: State<'_, SingleMutationController>,
) -> Result<UpdateSkillResponse, AppError> {
    let guard = controller.begin(MutationKind::BatchUpdate, context.clone())?;
    match &context.environment {
        EnvironmentRef::Host => {
            let resolved = ContextResolver::resolve_host(context)?;
            let (scope, project_path) = match resolved.project {
                Some(project) => (Scope::Project, Some(project.native_path)),
                None => (Scope::Global, None),
            };
            update_skills_batch_inner(&app, scope, &names, project_path.as_deref()).await
        }
        EnvironmentRef::Wsl { distro_name } => {
            let distro_name = distro_name.clone();
            let retry_app = app.clone();
            let retry_context = context.clone();
            let guard = &guard;
            registry
                .with_session(&distro_name, move |session| {
                    let app = retry_app.clone();
                    let context = retry_context.clone();
                    let names = names.clone();
                    async move {
                        let resolved = ContextResolver::resolve_wsl(context, &session).await?;
                        update_skills_batch_wsl(&app, resolved, &names, &session, guard).await
                    }
                })
                .await
        }
    }
}

async fn update_skills_batch_wsl(
    app: &AppHandle,
    context: ResolvedContext,
    names: &[String],
    session: &crate::environment::wsl::WslSession,
    guard: &MutationGuard<'_>,
) -> Result<UpdateSkillResponse, AppError> {
    let scope = if context.project.is_some() {
        Scope::Project
    } else {
        Scope::Global
    };
    let project_path = context
        .project
        .as_ref()
        .map(|project| project.native_path.as_str());
    let lock_repository = LockRepository::new(EnvironmentLockIo::Wsl(session.clone()));
    let mut lock_transaction = lock_repository
        .begin(
            resolved_update_lock_target(&context, &scope),
            LockMutationTargets {
                entries: names.to_vec(),
                default_target_agents: false,
            },
        )
        .await?;
    let mut metadata_entries = Vec::new();
    for name in names {
        if let Some(raw) = lock_transaction.initial_entry(name) {
            metadata_entries.push((
                name.clone(),
                normalized_update_metadata_from_raw(&scope, raw)?,
            ));
        }
    }
    let plan = prepare_wsl_batch_plan(names, metadata_entries);
    let context_root = context.context_root();
    let snapshots: HashMap<String, crate::environment::service::SkillEntrySnapshot> =
        EnvironmentService::Wsl(session.clone())
            .inspect(&InspectRequest {
                context: context.clone(),
            })
            .await?
            .skills
            .into_iter()
            .map(|snapshot| (snapshot.name.clone(), snapshot))
            .collect();

    let cancellation = guard.cancellation();
    guard.transition(MutationPhase::Acquiring, None, true);
    let mut prepared_sources: HashMap<
        crate::core::UpdateGroupKey,
        crate::commands::install::PreparedWslInstallSource,
    > = HashMap::new();
    let mut acquisition_failures: HashMap<crate::core::UpdateGroupKey, String> = HashMap::new();
    let mut results = plan.immediate_results;
    let mut has_lock_replacements = false;

    for (index, item) in plan.ready.iter().enumerate() {
        if cancellation.is_cancelled() {
            append_cancelled_batch_results(&mut results, &plan.ready[index..]);
            break;
        }
        let snapshot = snapshots.get(&item.name).cloned().unwrap_or_else(|| {
            fallback_wsl_update_snapshot(session, &scope, context_root, &item.name)
        });
        let params = build_wsl_update_install_params(
            scope.clone(),
            project_path,
            &item.name,
            &item.metadata,
            &snapshot,
        );

        if batch_source_needs_acquisition(
            &prepared_sources,
            &acquisition_failures,
            &item.source_key,
        ) {
            guard.transition(MutationPhase::Acquiring, None, true);
            let acquisition = match crate::core::parse_source(&params.source) {
                Ok(parsed) => {
                    crate::commands::install::prepare_wsl_install_source(
                        session,
                        &parsed,
                        cancellation.clone(),
                    )
                    .await
                }
                Err(error) => Err(error),
            };
            match acquisition {
                Ok(prepared) => {
                    prepared_sources.insert(item.source_key.clone(), prepared);
                }
                Err(_error) if cancellation.is_cancelled() => {
                    append_cancelled_batch_results(&mut results, &plan.ready[index..]);
                    break;
                }
                Err(error) => {
                    acquisition_failures.insert(item.source_key.clone(), error.to_string());
                }
            }
        }

        if let Some(error) = acquisition_failures.get(&item.source_key) {
            results.push(failed_wsl_batch_result(item, error.clone(), None));
            continue;
        }
        let prepared_source = prepared_sources
            .get(&item.source_key)
            .expect("successful acquisition is cached");
        let expected_path =
            item.metadata
                .skill_path
                .clone()
                .ok_or_else(|| AppError::InvalidSource {
                    value: format!("Missing locked skill path for '{}'", item.name),
                })?;
        let started = Instant::now();
        let install = crate::commands::install::install_skills_wsl_inner_with_options(
            app,
            &context.context,
            session,
            params,
            guard,
            crate::commands::install::WslInstallExecutionOptions {
                expected_skill_paths: HashMap::from([(item.name.clone(), expected_path)]),
                require_complete_targets_for_lock: true,
                defer_lock_commit: true,
                prepared_source: Some(prepared_source),
                resolved_context: Some(context.clone()),
            },
        )
        .await;

        match install {
            Ok(install) => {
                for (skill_name, replacement) in install.lock_replacements {
                    lock_transaction.replace_entry(&skill_name, replacement)?;
                    has_lock_replacements = true;
                }
                results.push(build_wsl_update_item(
                    &item.name,
                    &item.metadata,
                    install.results,
                    elapsed_ms(&started),
                ));
            }
            Err(_) if cancellation.is_cancelled() => {
                append_cancelled_batch_results(&mut results, &plan.ready[index..]);
                break;
            }
            Err(error) => results.push(failed_wsl_batch_result(
                item,
                error.to_string(),
                Some(elapsed_ms(&started)),
            )),
        }
    }

    if has_lock_replacements {
        guard.transition(MutationPhase::Committing, None, false);
        lock_transaction.commit().await?;
    }

    order_batch_results(names, &mut results);
    Ok(UpdateSkillResponse {
        summary: summarize_results(&results),
        results,
    })
}

async fn update_skills_batch_inner(
    app: &tauri::AppHandle,
    scope: Scope,
    names: &[String],
    project_path: Option<&str>,
) -> Result<UpdateSkillResponse, AppError> {
    use tauri::Emitter;

    let start = Instant::now();
    let lock_repository = LockRepository::new(EnvironmentLockIo::Host);
    let mut lock_transaction = lock_repository
        .begin(
            host_update_lock_target(&scope, project_path)?,
            LockMutationTargets {
                entries: names.to_vec(),
                default_target_agents: false,
            },
        )
        .await?;

    // 1. 读取 lock 文件，按 source 分组
    struct SkillEntry {
        name: String,
        source: String,
        source_type: String,
        source_url: String,
        skill_path: Option<String>,
        plugin_name: Option<String>,
        ref_name: Option<String>,
        subagents: Option<Vec<String>>,
        installed_at: Option<String>,
    }

    let mut entries: Vec<SkillEntry> = Vec::new();
    let mut all_results: Vec<UpdateSkillItemResult> = Vec::new();

    for name in names {
        let Some(raw) = lock_transaction.initial_entry(name) else {
            continue;
        };
        let (metadata, plugin_name, subagents, installed_at) = match scope {
            Scope::Global => {
                let entry: SkillLockEntry = serde_json::from_value(raw.clone())?;
                let metadata = normalize_global_lock_entry(&entry);
                (metadata, entry.plugin_name, None, Some(entry.installed_at))
            }
            Scope::Project => {
                let entry: LocalSkillLockEntry = serde_json::from_value(raw.clone())?;
                let metadata = normalize_local_lock_entry(&entry);
                (metadata, entry.plugin_name, entry.subagents, None)
            }
        };
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
            source: metadata.source.clone(),
            source_type: metadata.source_type.clone(),
            source_url: metadata
                .source_url
                .clone()
                .unwrap_or_else(|| metadata.source.clone()),
            skill_path: metadata.skill_path.clone(),
            plugin_name,
            ref_name: metadata.ref_name.clone(),
            subagents,
            installed_at,
        });
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
    let mut lock_replacements = Vec::new();

    // 2. 每组 source 只 clone 一次
    for group in by_source.values() {
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

            // detect agents (fallback 仅保留当前 scope 自动应用的 agents,见单个 update 路径的注释)
            let mut target_agents =
                detect_installed_agents_for_skill(&entry.name, &install_scope, project_path);
            let had_detected_targets = !target_agents.is_empty();
            if matches!(scope, Scope::Project) {
                target_agents.retain(|agent| *agent != AgentType::Eve);
            }
            if target_agents.is_empty() && !had_detected_targets {
                let is_global = matches!(install_scope, crate::models::Scope::Global);
                let cwd = project_path.unwrap_or(".");
                target_agents = AgentType::get_automatic_agents_for_scope(is_global, cwd);
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
            let mut agent_results: Vec<UpdateSkillAgentResult> = per_agent_results
                .into_iter()
                .map(|r| UpdateSkillAgentResult {
                    agent: r.agent,
                    target_id: None,
                    subagent: None,
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

            let eve_update_targets = match (&scope, project_path) {
                (Scope::Project, Some(pp)) => eve_targets_from_source_or_root_install(
                    &entry.source_type,
                    entry.subagents.as_deref(),
                    pp,
                    &entry.name,
                ),
                _ => Vec::new(),
            };

            for subagent in eve_update_targets {
                let result = crate::core::installer::install_skill_for_eve_target(
                    &skill.path,
                    &skill.name,
                    project_path.unwrap_or("."),
                    subagent.as_deref(),
                );
                agent_results.push(UpdateSkillAgentResult {
                    agent: "eve".to_string(),
                    target_id: result.target_id,
                    subagent: result.subagent,
                    status: if result.skipped {
                        UpdateSkillAgentStatus::Skipped
                    } else if result.success {
                        UpdateSkillAgentStatus::Success
                    } else {
                        UpdateSkillAgentStatus::Failed
                    },
                    mode: Some(result.mode),
                    error: result.error,
                    duration_ms: None,
                });
            }

            // 仅当所有 agent 都成功时,才把新 hash 写入 lock。
            // Partial / Failed 时保留旧 hash —— 与单个 update 路径行为一致,
            // 让用户重试只重装失败的 agent。
            let status = derive_skill_status(&agent_results);
            let _ = app.emit(
                "update-progress",
                &update_progress_payload(&entry.name, &scope, project_path, "writing_lock"),
            );

            if matches!(status, UpdateSkillStatus::Success) {
                let (final_hash, hash_warning) = resolve_post_update_hash(PostUpdateHashRequest {
                    scope: &scope,
                    skill_name: &entry.name,
                    project_path,
                    source_type: &entry.source_type,
                    source: &entry.source,
                    skill_path: entry.skill_path.as_deref(),
                    ref_name: entry.ref_name.as_deref(),
                    clone_repo_path: Some(skills_dir.as_path()),
                })
                .await;
                if let Some(w) = hash_warning {
                    warnings.push(w);
                }

                match scope {
                    Scope::Global => {
                        let now = chrono::Utc::now()
                            .format("%Y-%m-%dT%H:%M:%S%.3fZ")
                            .to_string();
                        let installed_at = entry
                            .installed_at
                            .clone()
                            .filter(|installed_at| !installed_at.is_empty())
                            .unwrap_or_else(|| now.clone());
                        lock_replacements.push((
                            entry.name.clone(),
                            serde_json::to_value(SkillLockEntry {
                                source: entry.source.clone(),
                                source_type: entry.source_type.clone(),
                                source_url: entry.source_url.clone(),
                                ref_name: entry.ref_name.clone(),
                                skill_path: entry.skill_path.clone(),
                                skill_folder_hash: final_hash,
                                installed_at,
                                updated_at: now,
                                plugin_name: entry.plugin_name.clone(),
                            })?,
                        ));
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
                                subagents: entry.subagents.clone(),
                                plugin_name: entry.plugin_name.clone(),
                            };
                            lock_replacements
                                .push((entry.name.clone(), serde_json::to_value(lock_entry)?));
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

    if !lock_replacements.is_empty() {
        for (skill_name, replacement) in lock_replacements {
            lock_transaction.replace_entry(&skill_name, replacement)?;
        }
        lock_transaction.commit().await?;
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

    Ok(UpdateSkillResponse {
        summary: summarize_results(&all_results),
        results: all_results,
    })
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

struct PostUpdateHashRequest<'a> {
    scope: &'a Scope,
    skill_name: &'a str,
    project_path: Option<&'a str>,
    source_type: &'a str,
    source: &'a str,
    skill_path: Option<&'a str>,
    ref_name: Option<&'a str>,
    clone_repo_path: Option<&'a Path>,
}

/// 解析 update 完成后要写入 lock 的版本追踪 hash。
///
/// 优先级（每一级失败才进入下一级）：
///   1. 从本地新 clone 的仓库直接 `git rev-parse` 计算 tree SHA — 零额外网络调用
///   2. 远端 GitHub Trees API — 兜底
///   3. 保留 lock 中已有的旧 hash — 绝不写入空串
///
/// 返回 `(final_hash, warning)`，只有当 1 / 2 都失败、且需要保留旧 hash 时才会附带 warning。
async fn resolve_post_update_hash(request: PostUpdateHashRequest<'_>) -> (String, Option<String>) {
    if request.source_type != "github" {
        return (String::new(), None);
    }
    let path_str = request.skill_path.unwrap_or("");

    // 1. 本地 git 仓库
    if let Some(repo_path) = request.clone_repo_path {
        if let Some(sha) = compute_local_tree_sha(repo_path, path_str) {
            return (sha, None);
        }
    }

    // 2. 远端 API 兜底
    if let Ok(Some(sha)) = fetch_skill_folder_hash(request.source, path_str, request.ref_name).await
    {
        return (sha, None);
    }

    // 3. 保留旧 hash
    if let Some(old) = read_existing_hash(request.scope, request.skill_name, request.project_path) {
        return (
            old,
            Some(format!(
                "Could not refresh remote hash for '{}', kept previous value",
                request.skill_name
            )),
        );
    }

    (
        String::new(),
        Some(format!(
            "Could not refresh remote hash for '{}'; lock entry will lack a remote hash",
            request.skill_name
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

    #[test]
    fn update_lock_locators_use_resolved_primary_and_project_legacy() {
        let environment = EnvironmentRef::Wsl {
            distro_name: "Ubuntu".to_string(),
        };
        let project = crate::environment::types::ProjectBinding {
            id: "app".to_string(),
            native_path: "/work/app".to_string(),
            display_name: None,
            order: None,
            suppress_cross_storage_warning: false,
        };
        let resolved = ResolvedContext {
            context: ContextRef {
                environment: environment.clone(),
                scope: ContextScope::Project {
                    project_id: project.id.clone(),
                },
            },
            project: Some(project),
            home: ResourceLocator {
                environment: environment.clone(),
                native_path: "/home/alice".to_string(),
            },
            skill_root: ResourceLocator {
                environment: environment.clone(),
                native_path: "/work/app/.agents/skills".to_string(),
            },
            lock: ResourceLocator {
                environment,
                native_path: "/work/app/skills-lock.json".to_string(),
            },
        };

        let (primary, legacy) = update_lock_locators(&resolved);

        assert_eq!(primary.native_path, "/work/app/skills-lock.json");
        assert_eq!(
            legacy.unwrap().native_path,
            "/work/app/.agents/.skill-lock.json"
        );
    }
    use crate::core::skill_lock::SkillLockEntry;
    use tempfile::tempdir;

    #[test]
    fn shared_update_check_preparation_groups_checkable_entries_and_keeps_cannot_check() {
        let prepared = prepare_update_checks(vec![
            (
                "toolkit".to_string(),
                crate::core::NormalizedUpdateMetadata {
                    source: "owner/repo".to_string(),
                    source_type: "github".to_string(),
                    source_url: Some("https://github.com/owner/repo".to_string()),
                    ref_name: Some("main".to_string()),
                    skill_path: Some("skills/toolkit/SKILL.md".to_string()),
                    remote_hash: Some("old-hash".to_string()),
                },
            ),
            (
                "local-only".to_string(),
                crate::core::NormalizedUpdateMetadata {
                    source: "/home/alice/local".to_string(),
                    source_type: "local".to_string(),
                    source_url: Some("/home/alice/local".to_string()),
                    ref_name: None,
                    skill_path: Some("SKILL.md".to_string()),
                    remote_hash: None,
                },
            ),
        ]);

        assert_eq!(prepared.groups.len(), 1);
        assert_eq!(
            prepared
                .groups
                .get(&("owner/repo".to_string(), Some("main".to_string())))
                .expect("github group")[0]
                .name,
            "toolkit"
        );
        assert_eq!(prepared.immediate_results.len(), 1);
        assert_eq!(
            prepared.immediate_results[0].status,
            SkillUpdateCheckStatus::CannotCheck
        );
        assert_eq!(
            prepared.immediate_results[0].reason.as_deref(),
            Some("local-source")
        );
    }

    #[test]
    fn wsl_update_metadata_reads_project_lock_and_falls_back_to_legacy() {
        let project = br#"{
          "version": 1,
          "skills": {
            "toolkit": {
              "source": "owner/repo",
              "sourceType": "github",
              "sourceUrl": "https://github.com/owner/repo",
              "computedHash": "local",
              "remoteHash": "remote",
              "skillPath": "skills/toolkit/SKILL.md"
            }
          }
        }"#;
        let current = parse_wsl_update_metadata(&Scope::Project, Some(project), None)
            .expect("parse project lock");
        assert_eq!(current.len(), 1);
        assert_eq!(current[0].0, "toolkit");
        assert_eq!(current[0].1.remote_hash.as_deref(), Some("remote"));

        let legacy = br#"{
          "version": 3,
          "skills": {
            "legacy": {
              "source": "owner/legacy",
              "sourceType": "github",
              "sourceUrl": "https://github.com/owner/legacy",
              "skillPath": "SKILL.md",
              "skillFolderHash": "legacy-hash",
              "installedAt": "",
              "updatedAt": ""
            }
          }
        }"#;
        let fallback = parse_wsl_update_metadata(&Scope::Project, None, Some(legacy))
            .expect("parse legacy lock");
        assert_eq!(fallback[0].0, "legacy");
        assert_eq!(fallback[0].1.remote_hash.as_deref(), Some("legacy-hash"));
    }

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
    fn test_normalize_global_lock_entry_recovers_missing_source_url() {
        let entry = SkillLockEntry {
            source: "owner/repo".to_string(),
            source_type: "github".to_string(),
            source_url: String::new(),
            ref_name: None,
            skill_path: Some("skills/demo/SKILL.md".to_string()),
            skill_folder_hash: "tree123".to_string(),
            installed_at: "2026-01-01T00:00:00.000Z".to_string(),
            updated_at: "2026-01-01T00:00:00.000Z".to_string(),
            plugin_name: None,
        };

        let normalized = crate::core::normalize_global_lock_entry(&entry);
        assert_eq!(
            normalized.source_url.as_deref(),
            Some("https://github.com/owner/repo")
        );
        assert!(crate::core::derive_update_capability(&normalized).can_check_for_updates);
    }

    #[test]
    fn test_global_update_entry_recovers_missing_source_url() {
        let entry = SkillLockEntry {
            source: "owner/repo".to_string(),
            source_type: "github".to_string(),
            source_url: String::new(),
            ref_name: Some("main".to_string()),
            skill_path: Some("skills/demo/SKILL.md".to_string()),
            skill_folder_hash: "tree123".to_string(),
            installed_at: "2026-01-01T00:00:00.000Z".to_string(),
            updated_at: "2026-01-01T00:00:00.000Z".to_string(),
            plugin_name: Some("plugin-a".to_string()),
        };

        let update_entry = global_update_entry_from_lock_entry(&entry);

        assert_eq!(update_entry.source, "owner/repo");
        assert_eq!(update_entry.source_type, "github");
        assert_eq!(update_entry.source_url, "https://github.com/owner/repo");
        assert_eq!(
            update_entry.skill_path.as_deref(),
            Some("skills/demo/SKILL.md")
        );
        assert_eq!(update_entry.plugin_name.as_deref(), Some("plugin-a"));
        assert_eq!(update_entry.ref_name.as_deref(), Some("main"));
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
            install_dir_name: "demo".to_string(),
            description: "Priority".to_string(),
            path: std::path::PathBuf::from("skills/demo"),
            relative_path: "skills/demo/SKILL.md".to_string(),
            plugin_name: None,
        };
        let locked_skill = crate::core::DiscoveredSkill {
            name: "demo".to_string(),
            install_dir_name: "demo".to_string(),
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
            assert_eq!(
                item.source_url.as_deref(),
                Some("https://github.com/owner/repo")
            );
            assert_eq!(item.git_ref.as_deref(), Some("main"));
            assert_eq!(item.skill_path, None);
        });
    }

    #[test]
    fn test_update_check_status_deleted_upstream_serializes_kebab_case() {
        let value = serde_json::to_value(SkillUpdateCheckStatus::DeletedUpstream).unwrap();
        assert_eq!(value, serde_json::json!("deleted-upstream"));
    }

    #[test]
    fn test_update_fallback_uses_default_available_agents() {
        let fallback = AgentType::get_automatic_agents_for_scope(true, ".");
        assert!(fallback.contains(&AgentType::Firebender));
        assert!(!fallback.contains(&AgentType::Antigravity));
    }

    #[test]
    fn test_batch_hash_result_marks_missing_remote_hash_as_deleted_upstream() {
        let skill = UpdateCheckSkill {
            name: "demo".to_string(),
            source: "owner/repo".to_string(),
            source_url: Some("https://github.com/owner/repo".to_string()),
            ref_name: Some("main".to_string()),
            skill_path: "skills/demo/SKILL.md".to_string(),
            local_hash: "local-hash".to_string(),
        };

        let item = build_batch_check_result(&skill, None);

        assert_eq!(item.status, SkillUpdateCheckStatus::DeletedUpstream);
        assert!(!item.has_update);
        assert_eq!(item.reason.as_deref(), Some("deleted-upstream"));
        assert_eq!(
            item.source_url.as_deref(),
            Some("https://github.com/owner/repo")
        );
        assert_eq!(item.git_ref.as_deref(), Some("main"));
        assert_eq!(item.skill_path.as_deref(), Some("skills/demo/SKILL.md"));
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
    fn test_eve_targets_from_lock_maps_empty_string_to_root() {
        let entry = LocalSkillLockEntry {
            source: "vercel/eve".to_string(),
            ref_name: None,
            source_type: "github".to_string(),
            source_url: None,
            computed_hash: "abc".to_string(),
            remote_hash: None,
            skill_path: Some("SKILL.md".to_string()),
            subagents: Some(vec!["".to_string(), "research".to_string()]),
            plugin_name: None,
        };

        assert_eq!(
            eve_targets_from_lock_or_root_install(&entry, ".", "demo"),
            vec![None, Some("research".to_string())]
        );
    }

    #[test]
    fn wsl_update_install_params_reuse_current_agent_targets_and_locked_source() {
        let metadata = crate::core::NormalizedUpdateMetadata {
            source: "owner/repo".to_string(),
            source_type: "github".to_string(),
            source_url: Some("https://github.com/owner/repo".to_string()),
            ref_name: Some("feature/test".to_string()),
            skill_path: Some("examples/toolkit/SKILL.md".to_string()),
            remote_hash: Some("old-hash".to_string()),
        };
        let snapshot = crate::environment::service::SkillEntrySnapshot {
            name: "toolkit".to_string(),
            description: String::new(),
            canonical_path: "/work/app/.agents/skills/toolkit".to_string(),
            canonical_present: true,
            agents: vec![],
            card_agents: vec![],
            default_available_agents: vec![AgentType::Codex],
            private_adapted_agents: vec![AgentType::ClaudeCode],
            duplicate_copy_agents: vec![],
            private_only_agents: vec![AgentType::Amp],
            private_copy_agents: vec![AgentType::GithubCopilot],
            eve_targets: vec![crate::models::InstallTargetInfo {
                target_id: "eve:research".to_string(),
                agent: AgentType::Eve,
                display_name: "Eve (research)".to_string(),
                subagent: Some("research".to_string()),
                path: "/work/app/agent/subagents/research/skills".to_string(),
            }],
        };

        let params = build_wsl_update_install_params(
            Scope::Project,
            Some("/work/app"),
            "toolkit",
            &metadata,
            &snapshot,
        );

        assert_eq!(params.source, "https://github.com/owner/repo#feature/test");
        assert_eq!(
            params.agents,
            vec![
                AgentType::ClaudeCode.to_string(),
                AgentType::Amp.to_string()
            ]
        );
        assert_eq!(
            params.private_copy_agents,
            vec![AgentType::GithubCopilot.to_string()]
        );
        assert_eq!(params.agent_targets.len(), 1);
        assert_eq!(
            params.agent_targets[0].subagent.as_deref(),
            Some("research")
        );
        assert!(params.preserve_existing_modes);
        assert!(params.acknowledge_risk);
    }

    #[test]
    fn wsl_update_result_expands_canonical_to_default_agents_and_reports_partial() {
        let metadata = crate::core::NormalizedUpdateMetadata {
            source: "owner/repo".to_string(),
            source_type: "github".to_string(),
            source_url: Some("https://github.com/owner/repo".to_string()),
            ref_name: None,
            skill_path: Some("skills/toolkit/SKILL.md".to_string()),
            remote_hash: Some("old".to_string()),
        };
        let install = crate::models::InstallResults {
            successful: vec![crate::models::InstallResult {
                skill_name: "toolkit".to_string(),
                agent: "__canonical__".to_string(),
                target_id: None,
                subagent: None,
                success: true,
                path: std::path::PathBuf::from("/home/alice/.agents/skills/toolkit"),
                canonical_path: Some(std::path::PathBuf::from(
                    "/home/alice/.agents/skills/toolkit",
                )),
                mode: InstallMode::Copy,
                symlink_failed: false,
                skipped: false,
                error: None,
                category: crate::models::InstallResultCategory::DefaultAvailable,
            }],
            failed: vec![crate::models::InstallResult {
                skill_name: "toolkit".to_string(),
                agent: AgentType::ClaudeCode.to_string(),
                target_id: Some(AgentType::ClaudeCode.to_string()),
                subagent: None,
                success: false,
                path: std::path::PathBuf::new(),
                canonical_path: None,
                mode: InstallMode::Symlink,
                symlink_failed: false,
                skipped: false,
                error: Some("permission denied".to_string()),
                category: crate::models::InstallResultCategory::Failed,
            }],
            symlink_fallback_agents: vec![],
            default_available_agents: vec![AgentType::Codex.to_string()],
            private_adapted_agents: vec![AgentType::ClaudeCode.to_string()],
            private_copy_agents: vec![],
            target_details: vec![],
        };

        let item = build_wsl_update_item("toolkit", &metadata, install, 12);

        assert_eq!(item.status, UpdateSkillStatus::Partial);
        assert_eq!(item.agent_results.len(), 2);
        assert!(item.agent_results.iter().any(|result| {
            result.agent == AgentType::Codex.to_string()
                && result.status == UpdateSkillAgentStatus::Success
        }));
        assert_eq!(item.error.as_deref(), Some("permission denied"));
        assert_eq!(item.duration_ms, Some(12));
    }

    #[test]
    fn wsl_batch_plan_preserves_requested_order_and_exact_source_groups() {
        let names = vec![
            "second".to_string(),
            "first".to_string(),
            "missing".to_string(),
        ];
        let plan = prepare_wsl_batch_plan(
            &names,
            vec![
                (
                    "first".to_string(),
                    crate::core::NormalizedUpdateMetadata {
                        source: "owner/repo".to_string(),
                        source_type: "github".to_string(),
                        source_url: Some("https://github.com/owner/repo".to_string()),
                        ref_name: Some("main".to_string()),
                        skill_path: Some("skills/first/SKILL.md".to_string()),
                        remote_hash: Some("old-first".to_string()),
                    },
                ),
                (
                    "second".to_string(),
                    crate::core::NormalizedUpdateMetadata {
                        source: "owner/repo".to_string(),
                        source_type: "github".to_string(),
                        source_url: Some("https://github.com/owner/repo".to_string()),
                        ref_name: Some("main".to_string()),
                        skill_path: Some("examples/second/SKILL.md".to_string()),
                        remote_hash: Some("old-second".to_string()),
                    },
                ),
            ],
        );

        assert_eq!(
            plan.ready
                .iter()
                .map(|item| item.name.as_str())
                .collect::<Vec<_>>(),
            vec!["second", "first"]
        );
        assert_eq!(plan.ready[0].source_key, plan.ready[1].source_key);
        assert_eq!(plan.immediate_results.len(), 1);
        assert_eq!(plan.immediate_results[0].name, "missing");
        assert_eq!(
            plan.immediate_results[0].reason.as_deref(),
            Some("missing-lock-entry")
        );
    }

    #[test]
    fn batch_source_is_acquired_only_until_success_or_failure_is_cached() {
        let key = build_update_group_key("github", "https://github.com/owner/repo", Some("main"));
        let mut prepared = HashMap::new();
        let mut failed = HashMap::new();

        assert!(batch_source_needs_acquisition(&prepared, &failed, &key));

        prepared.insert(key.clone(), ());
        assert!(!batch_source_needs_acquisition(&prepared, &failed, &key));

        prepared.clear();
        failed.insert(key.clone(), "clone failed".to_string());
        assert!(!batch_source_needs_acquisition(&prepared, &failed, &key));
    }

    #[test]
    fn cancelled_batch_marks_only_unstarted_skills_in_request_order() {
        let plan = prepare_wsl_batch_plan(
            &["one".to_string(), "two".to_string()],
            vec![
                (
                    "one".to_string(),
                    crate::core::NormalizedUpdateMetadata {
                        source: "owner/repo".to_string(),
                        source_type: "github".to_string(),
                        source_url: Some("https://github.com/owner/repo".to_string()),
                        ref_name: None,
                        skill_path: Some("skills/one/SKILL.md".to_string()),
                        remote_hash: Some("old-one".to_string()),
                    },
                ),
                (
                    "two".to_string(),
                    crate::core::NormalizedUpdateMetadata {
                        source: "owner/repo".to_string(),
                        source_type: "github".to_string(),
                        source_url: Some("https://github.com/owner/repo".to_string()),
                        ref_name: None,
                        skill_path: Some("skills/two/SKILL.md".to_string()),
                        remote_hash: Some("old-two".to_string()),
                    },
                ),
            ],
        );
        let mut results = Vec::new();

        append_cancelled_batch_results(&mut results, &plan.ready[1..]);

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "two");
        assert_eq!(results[0].status, UpdateSkillStatus::Skipped);
        assert_eq!(results[0].reason.as_deref(), Some("cancelled"));
    }

    #[test]
    fn test_eve_targets_from_well_known_lock_maps_subagents() {
        let entry = LocalSkillLockEntry {
            source: "https://example.com".to_string(),
            ref_name: None,
            source_type: "well-known".to_string(),
            source_url: Some("https://example.com".to_string()),
            computed_hash: "abc".to_string(),
            remote_hash: None,
            skill_path: Some("SKILL.md".to_string()),
            subagents: Some(vec!["".to_string(), "research".to_string()]),
            plugin_name: None,
        };

        assert_eq!(
            eve_targets_from_lock_or_root_install(&entry, ".", "demo"),
            vec![None, Some("research".to_string())]
        );
    }

    #[test]
    fn test_eve_targets_from_direct_url_lock_maps_subagents() {
        let entry = LocalSkillLockEntry {
            source: "https://example.com/skill.md".to_string(),
            ref_name: None,
            source_type: "direct-url".to_string(),
            source_url: Some("https://example.com/skill.md".to_string()),
            computed_hash: "abc".to_string(),
            remote_hash: None,
            skill_path: Some("SKILL.md".to_string()),
            subagents: Some(vec!["research".to_string()]),
            plugin_name: None,
        };

        assert_eq!(
            eve_targets_from_lock_or_root_install(&entry, ".", "demo"),
            vec![Some("research".to_string())]
        );
    }

    #[test]
    fn test_eve_targets_from_lock_falls_back_to_existing_root_only() {
        let temp = tempdir().unwrap();
        let root = temp.path().join("agent/skills/demo");
        let sub = temp.path().join("agent/subagents/research/skills/demo");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&sub).unwrap();

        let entry = LocalSkillLockEntry {
            source: "vercel/eve".to_string(),
            ref_name: None,
            source_type: "github".to_string(),
            source_url: None,
            computed_hash: "abc".to_string(),
            remote_hash: None,
            skill_path: Some("SKILL.md".to_string()),
            subagents: None,
            plugin_name: None,
        };

        assert_eq!(
            eve_targets_from_lock_or_root_install(&entry, &temp.path().to_string_lossy(), "demo"),
            vec![None]
        );
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
                target_id: None,
                subagent: None,
                status: UpdateSkillAgentStatus::Success,
                mode: None,
                error: None,
                duration_ms: Some(5),
            },
            UpdateSkillAgentResult {
                agent: "claude-code".to_string(),
                target_id: None,
                subagent: None,
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
                target_id: None,
                subagent: None,
                status: UpdateSkillAgentStatus::Success,
                mode: None,
                error: None,
                duration_ms: Some(5),
            },
            UpdateSkillAgentResult {
                agent: "claude-code".to_string(),
                target_id: None,
                subagent: None,
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
            target_id: None,
            subagent: None,
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
            target_id: None,
            subagent: None,
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
            let (final_hash, warning) = resolve_post_update_hash(PostUpdateHashRequest {
                scope: &Scope::Project,
                skill_name: "demo",
                project_path: Some(tmp.path().to_str().unwrap()),
                source_type: "github",
                // 不存在的 repo:确保 fetch_skill_folder_hash 拿不到内容
                source: "this-org-does-not-exist-skill-deck/no-repo",
                skill_path: Some("skills/demo/SKILL.md"),
                ref_name: Some("nonexistent-branch-xyz"),
                clone_repo_path: None,
            })
            .await;
            // 必须保留旧 hash,绝不写空串
            assert_eq!(final_hash, "tree-old");
            assert!(warning.is_some(), "应当 push 一条 warning 提示用户");
        });
    }

    #[test]
    fn test_resolve_post_update_hash_returns_empty_for_non_github_source() {
        tauri::async_runtime::block_on(async {
            let (final_hash, warning) = resolve_post_update_hash(PostUpdateHashRequest {
                scope: &Scope::Project,
                skill_name: "demo",
                project_path: None,
                source_type: "local",
                source: "/some/path",
                skill_path: None,
                ref_name: None,
                clone_repo_path: None,
            })
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
            let (final_hash, warning) = resolve_post_update_hash(PostUpdateHashRequest {
                scope: &Scope::Project,
                skill_name: "demo",
                project_path: Some(lock_dir.path().to_str().unwrap()),
                source_type: "github",
                source: "this-does-not-matter/because-local-wins",
                skill_path: Some("skills/demo/SKILL.md"),
                ref_name: Some("nonexistent"),
                clone_repo_path: Some(repo),
            })
            .await;
            assert_eq!(final_hash, expected_sha);
            assert!(warning.is_none());
        });
    }
}

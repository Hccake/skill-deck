//! 复制 skill 到其他项目
//!
//! 将项目级 skill 复制到其他项目，保持相同的 agent 配置。
//! 复用 installer 的 install_skill_to_agents 完成实际安装。

use crate::core::agent_availability::{
    availability_for_agent, default_available_agents, AgentAvailabilityKind,
};
use crate::core::agents::AgentType;
use crate::core::installer::{
    copy_skill_files, install_skill_to_agent_groups, PerAgentInstallResult,
};
use crate::core::local_lock::{
    add_skill_to_local_lock, compute_skill_folder_hash, read_local_lock, LocalSkillLockEntry,
    LocalSkillLockFile,
};
use crate::core::lossless_lock::{LockEntrySnapshot, LosslessLockDocument};
use crate::core::mutation::{MutationGuard, MutationKind, SingleMutationController};
use crate::core::paths::canonical_skills_dir;
use crate::core::skill::sanitize_name;
use crate::core::skill_lock::SkillLockFile;
#[cfg(target_os = "windows")]
use crate::environment::acquisition::HostStagingDir;
use crate::environment::acquisition::{stage_wsl_source, StagedWslSource, WslAcquisitionSource};
use crate::environment::agent_environment::{AgentEnvironmentContext, AgentEnvironmentResolver};
use crate::environment::lock_io::EnvironmentLockIo;
use crate::environment::materialize::{
    materialize_wsl_skill, WslMaterializeRequest, WslMaterializeResult, WslMaterializeTarget,
};
use crate::environment::types::{ContextRef, ContextScope, EnvironmentRef, ResourceLocator};
use crate::environment::wsl::{EnvironmentRegistry, WslSession};
use crate::error::AppError;
use crate::models::{InstallMode, Scope};
use serde_json::Value;
use std::collections::HashSet;
use std::fs;
use std::path::Path;
#[cfg(not(target_os = "windows"))]
use std::path::PathBuf;
use tauri::State;

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

/// 复制后目标项目的更新信息保留状态
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, specta::Type)]
#[serde(rename_all = "kebab-case")]
#[specta(rename_all = "kebab-case")]
pub enum CopyUpdateMetadataStatus {
    Preserved,
    Incomplete,
    Missing,
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
    /// 复制后目标项目的更新信息保留状态
    pub update_metadata_status: CopyUpdateMetadataStatus,
    /// 更新信息降级原因
    #[serde(default)]
    pub update_metadata_reason: Option<String>,
}

/// 复制结果汇总
#[derive(Debug, Clone, serde::Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct CopySkillResult {
    pub results: Vec<CopyProjectResult>,
}

#[derive(Debug, Clone)]
struct WslCopyTargetPlan {
    default_available_agents: Vec<AgentType>,
    private_required_agents: Vec<AgentType>,
    private_copy_agents: Vec<AgentType>,
    materialize_targets: Vec<WslMaterializeTarget>,
}

#[derive(Debug, Clone)]
struct ResolvedCopyProject {
    context: ContextRef,
    project_path: String,
    session: Option<WslSession>,
}

#[derive(Debug, Clone)]
struct CopySourceMetadata {
    raw_entry: Value,
    normalized_entry: LocalSkillLockEntry,
}

enum PreparedCopySource {
    #[cfg(target_os = "windows")]
    HostDrvFs(HostStagingDir),
    #[cfg(not(target_os = "windows"))]
    HostNative {
        _temp_dir: tempfile::TempDir,
        host_repo_path: PathBuf,
        linux_repo_path: String,
    },
    Wsl(StagedWslSource),
}

impl PreparedCopySource {
    fn host_repo_path(&self) -> &Path {
        match self {
            #[cfg(target_os = "windows")]
            Self::HostDrvFs(staging) => staging.host_repo_path(),
            #[cfg(not(target_os = "windows"))]
            Self::HostNative { host_repo_path, .. } => host_repo_path,
            Self::Wsl(staging) => staging.host_repo_path(),
        }
    }

    fn linux_repo_path(&self) -> &str {
        match self {
            #[cfg(target_os = "windows")]
            Self::HostDrvFs(staging) => staging.linux_repo_path(),
            #[cfg(not(target_os = "windows"))]
            Self::HostNative {
                linux_repo_path, ..
            } => linux_repo_path,
            Self::Wsl(staging) => staging.linux_repo_path(),
        }
    }
}

fn create_host_copy_staging() -> Result<PreparedCopySource, AppError> {
    #[cfg(target_os = "windows")]
    {
        let staging = HostStagingDir::new()?;
        fs::create_dir_all(staging.host_repo_path())?;
        Ok(PreparedCopySource::HostDrvFs(staging))
    }
    #[cfg(not(target_os = "windows"))]
    {
        let temp_dir = tempfile::Builder::new()
            .prefix("skill-deck-copy-")
            .tempdir()
            .map_err(|error| AppError::Io {
                message: format!("failed to create Host copy staging directory: {error}"),
            })?;
        let host_repo_path = temp_dir.path().join("repo");
        fs::create_dir_all(&host_repo_path)?;
        let linux_repo_path = host_repo_path.to_string_lossy().to_string();
        Ok(PreparedCopySource::HostNative {
            _temp_dir: temp_dir,
            host_repo_path,
            linux_repo_path,
        })
    }
}

struct TargetLockState {
    io: EnvironmentLockIo,
    primary: ResourceLocator,
    legacy: ResourceLocator,
    expected: LockEntrySnapshot,
}

#[derive(Clone, Copy)]
struct CopyTargetRequest<'a> {
    skill_name: &'a str,
    staged: &'a PreparedCopySource,
    agents: &'a [AgentType],
    private_copy_agents: &'a [AgentType],
    source_metadata: Option<&'a CopySourceMetadata>,
    staged_hash: &'a str,
}

fn validate_copy_contexts(
    source: &ContextRef,
    targets: &[ContextRef],
) -> Result<Vec<ContextRef>, AppError> {
    if !matches!(source.scope, ContextScope::Project { .. }) {
        return Err(AppError::Custom {
            message: "Copy source must be a project context".to_string(),
        });
    }
    if targets
        .iter()
        .any(|target| !matches!(target.scope, ContextScope::Project { .. }))
    {
        return Err(AppError::Custom {
            message: "Copy targets must be project contexts".to_string(),
        });
    }
    Ok(targets.to_vec())
}

fn posix_parent(path: &str) -> Option<String> {
    path.trim_end_matches('/')
        .rsplit_once('/')
        .map(|(parent, _)| parent.to_string())
}

fn build_wsl_copy_target_plan(
    resolver: &AgentEnvironmentResolver,
    target_project_path: &str,
    _skill_name: &str,
    agents: &[AgentType],
    private_copy_agents: &[AgentType],
) -> WslCopyTargetPlan {
    let default_available_agents = AgentType::all()
        .filter(|agent| {
            resolver
                .target(*agent, false, target_project_path)
                .default_available
        })
        .collect::<Vec<_>>();
    let mut private_required = Vec::new();
    let mut private_copies = Vec::new();
    let mut materialize_targets = Vec::new();
    for agent in agents {
        let target = resolver.target(*agent, false, target_project_path);
        if target.availability != AgentAvailabilityKind::PrivateRequired {
            continue;
        }
        let Some(skills_root) = target.private_path else {
            continue;
        };
        private_required.push(*agent);
        materialize_targets.push(WslMaterializeTarget {
            target_id: agent.to_string(),
            agent: agent.to_string(),
            required_root: posix_parent(&skills_root),
            skills_root,
            mode: InstallMode::Symlink,
            preserve_existing_mode: false,
        });
    }
    for agent in private_copy_agents {
        let target = resolver.target(*agent, false, target_project_path);
        if target.availability != AgentAvailabilityKind::SharedCompatible {
            continue;
        }
        let Some(skills_root) = target.private_path else {
            continue;
        };
        private_copies.push(*agent);
        materialize_targets.push(WslMaterializeTarget {
            target_id: agent.to_string(),
            agent: agent.to_string(),
            required_root: posix_parent(&skills_root),
            skills_root,
            mode: InstallMode::Copy,
            preserve_existing_mode: false,
        });
    }
    WslCopyTargetPlan {
        default_available_agents,
        private_required_agents: private_required,
        private_copy_agents: private_copies,
        materialize_targets,
    }
}

fn build_copied_lock_entry(source: &Value, computed_hash: &str) -> Result<Value, AppError> {
    let mut copied = source.as_object().cloned().ok_or_else(|| AppError::Json {
        message: "project lock entry must be a JSON object".to_string(),
    })?;
    copied.insert(
        "computedHash".to_string(),
        Value::String(computed_hash.to_string()),
    );
    copied.remove("subagents");
    Ok(Value::Object(copied))
}

fn normalize_project_copy_lock_bytes(
    current: Option<&[u8]>,
    legacy: Option<&[u8]>,
) -> Result<Vec<u8>, AppError> {
    if let Some(current) = current {
        return Ok(current.to_vec());
    }
    let lock = if let Some(legacy) = legacy {
        let legacy: SkillLockFile = serde_json::from_slice(legacy)?;
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
        migrated
    } else {
        LocalSkillLockFile::empty()
    };
    let mut bytes = serde_json::to_vec_pretty(&lock)?;
    bytes.push(b'\n');
    Ok(bytes)
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
                update_metadata_status: CopyUpdateMetadataStatus::Missing,
                update_metadata_reason: Some("copy-failed".to_string()),
            }),
        }
    }

    Ok(CopySkillResult { results })
}

#[tauri::command]
#[specta::specta]
pub async fn copy_skill_to_projects_v2(
    skill_name: String,
    source: ContextRef,
    targets: Vec<ContextRef>,
    agents: Vec<String>,
    private_copy_agents: Vec<String>,
    registry: State<'_, EnvironmentRegistry>,
    controller: State<'_, SingleMutationController>,
) -> Result<CopySkillResult, AppError> {
    let targets = validate_copy_contexts(&source, &targets)?;
    let guard = controller.begin(MutationKind::Copy, source.clone(), "Preparing copy")?;
    let source_project = resolve_copy_project(&source, &registry).await?;
    let source_metadata = read_copy_source_metadata(&source_project, &skill_name).await;
    let staged = prepare_copy_source(&source_project, &skill_name, &guard).await?;
    let staged_hash = compute_skill_folder_hash(staged.host_repo_path()).unwrap_or_default();
    let agent_types = parse_agent_ids(&agents);
    let private_copy_agent_types = parse_agent_ids(&private_copy_agents);
    let request = CopyTargetRequest {
        skill_name: &skill_name,
        staged: &staged,
        agents: &agent_types,
        private_copy_agents: &private_copy_agent_types,
        source_metadata: source_metadata.as_ref(),
        staged_hash: &staged_hash,
    };
    let mut results = Vec::with_capacity(targets.len());

    for target in targets {
        if guard.cancellation().is_cancelled() {
            results.push(copy_failure_result(
                project_id(&target).unwrap_or_default(),
                "Copy was cancelled before this target".to_string(),
            ));
            continue;
        }
        let target_project = match resolve_copy_project(&target, &registry).await {
            Ok(project) => project,
            Err(error) => {
                results.push(copy_failure_result(
                    project_id(&target).unwrap_or_default(),
                    error.to_string(),
                ));
                continue;
            }
        };
        let result = match &target_project.context.environment {
            EnvironmentRef::Host => copy_to_host_project_v2(request, &target_project).await,
            EnvironmentRef::Wsl { .. } => {
                copy_to_wsl_project_v2(request, &target_project, &guard).await
            }
        };
        results.push(result.unwrap_or_else(|error| {
            copy_failure_result(&target_project.project_path, error.to_string())
        }));
    }

    Ok(CopySkillResult { results })
}

fn project_id(context: &ContextRef) -> Option<&str> {
    match &context.scope {
        ContextScope::Project { project_id } => Some(project_id),
        ContextScope::Global => None,
    }
}

async fn resolve_copy_project(
    context: &ContextRef,
    registry: &EnvironmentRegistry,
) -> Result<ResolvedCopyProject, AppError> {
    let project_id = project_id(context).ok_or_else(|| AppError::Custom {
        message: "Copy requires a project context".to_string(),
    })?;
    match &context.environment {
        EnvironmentRef::Host => {
            let project = crate::commands::environments::host_projects_store()?
                .read()?
                .into_iter()
                .find(|project| project.id == project_id)
                .ok_or_else(|| AppError::PathNotFound {
                    path: project_id.to_string(),
                })?;
            Ok(ResolvedCopyProject {
                context: context.clone(),
                project_path: project.native_path,
                session: None,
            })
        }
        EnvironmentRef::Wsl { distro_name } => {
            let session = registry.get(distro_name).ok_or_else(|| AppError::Custom {
                message: format!("WSL distro '{distro_name}' is not connected"),
            })?;
            let project = crate::commands::environments::read_wsl_projects(&session)
                .await?
                .into_iter()
                .find(|project| project.id == project_id)
                .ok_or_else(|| AppError::PathNotFound {
                    path: project_id.to_string(),
                })?;
            Ok(ResolvedCopyProject {
                context: context.clone(),
                project_path: project.native_path,
                session: Some(session),
            })
        }
    }
}

async fn prepare_copy_source(
    source: &ResolvedCopyProject,
    skill_name: &str,
    guard: &MutationGuard<'_>,
) -> Result<PreparedCopySource, AppError> {
    let sanitized = sanitize_name(skill_name);
    match &source.context.environment {
        EnvironmentRef::Host => {
            let source_canonical =
                canonical_skills_dir(false, &source.project_path).join(sanitized);
            if !source_canonical.is_dir() {
                return Err(AppError::PathNotFound {
                    path: source_canonical.to_string_lossy().to_string(),
                });
            }
            let staging = create_host_copy_staging()?;
            copy_skill_files(&source_canonical, staging.host_repo_path())?;
            Ok(staging)
        }
        EnvironmentRef::Wsl { .. } => {
            let session = source.session.as_ref().expect("WSL project has a session");
            let source_canonical = format!(
                "{}/.agents/skills/{}",
                source.project_path.trim_end_matches('/'),
                sanitized
            );
            Ok(PreparedCopySource::Wsl(
                stage_wsl_source(
                    session,
                    WslAcquisitionSource::Local {
                        native_path: source_canonical,
                    },
                    guard.cancellation(),
                )
                .await?,
            ))
        }
    }
}

fn project_lock_locators(project: &ResolvedCopyProject) -> (ResourceLocator, ResourceLocator) {
    let root = project.project_path.trim_end_matches(['/', '\\']);
    (
        ResourceLocator {
            environment: project.context.environment.clone(),
            native_path: format!("{root}/skills-lock.json"),
        },
        ResourceLocator {
            environment: project.context.environment.clone(),
            native_path: format!("{root}/.agents/.skill-lock.json"),
        },
    )
}

fn project_lock_io(project: &ResolvedCopyProject) -> EnvironmentLockIo {
    match &project.context.environment {
        EnvironmentRef::Host => EnvironmentLockIo::Host,
        EnvironmentRef::Wsl { .. } => {
            EnvironmentLockIo::Wsl(project.session.clone().expect("WSL project has a session"))
        }
    }
}

async fn read_copy_source_metadata(
    source: &ResolvedCopyProject,
    skill_name: &str,
) -> Option<CopySourceMetadata> {
    let io = project_lock_io(source);
    let (primary, legacy) = project_lock_locators(source);
    let loaded = async {
        let current = io.read_optional(&primary).await?;
        let legacy_bytes = if current.is_none() {
            io.read_optional(&legacy).await?
        } else {
            None
        };
        let normalized =
            normalize_project_copy_lock_bytes(current.as_deref(), legacy_bytes.as_deref())?;
        let value: Value = serde_json::from_slice(&normalized)?;
        let raw_entry = value
            .get("skills")
            .and_then(Value::as_object)
            .and_then(|skills| skills.get(skill_name))
            .cloned()
            .ok_or_else(|| AppError::InvalidSource {
                value: format!("Skill '{skill_name}' not found in source lock"),
            })?;
        let normalized_entry = serde_json::from_value(raw_entry.clone())?;
        Ok::<_, AppError>(CopySourceMetadata {
            raw_entry,
            normalized_entry,
        })
    }
    .await;
    match loaded {
        Ok(metadata) => Some(metadata),
        Err(error) => {
            log::warn!("Failed to read source project lock metadata for copy: {error}");
            None
        }
    }
}

async fn prepare_target_lock(
    target: &ResolvedCopyProject,
    skill_name: &str,
) -> Result<TargetLockState, AppError> {
    let io = project_lock_io(target);
    let (primary, legacy) = project_lock_locators(target);
    let current = io.read_optional(&primary).await?;
    let legacy_bytes = if current.is_none() {
        io.read_optional(&legacy).await?
    } else {
        None
    };
    let normalized =
        normalize_project_copy_lock_bytes(current.as_deref(), legacy_bytes.as_deref())?;
    let document = LosslessLockDocument::parse(&normalized)?;
    Ok(TargetLockState {
        io,
        primary,
        legacy,
        expected: document.snapshot(skill_name),
    })
}

async fn commit_target_lock(
    state: TargetLockState,
    skill_name: &str,
    source_entry: &Value,
    computed_hash: &str,
) -> Result<(), AppError> {
    let current = state.io.read_optional(&state.primary).await?;
    let legacy = if current.is_none() {
        state.io.read_optional(&state.legacy).await?
    } else {
        None
    };
    let normalized = normalize_project_copy_lock_bytes(current.as_deref(), legacy.as_deref())?;
    let mut document = LosslessLockDocument::parse(&normalized)?;
    document.replace_entry(
        skill_name,
        &state.expected,
        build_copied_lock_entry(source_entry, computed_hash)?,
    )?;
    state
        .io
        .write_atomic(&state.primary, document.to_pretty_bytes()?)
        .await
}

async fn copy_to_host_project_v2(
    request: CopyTargetRequest<'_>,
    target: &ResolvedCopyProject,
) -> Result<CopyProjectResult, AppError> {
    let lock_state = if request.source_metadata.is_some() {
        Some(prepare_target_lock(target, request.skill_name).await)
    } else {
        None
    };
    let mut result = copy_to_single_project(
        request.skill_name,
        request.staged.host_repo_path(),
        &target.project_path,
        request.agents,
        request.private_copy_agents,
        None,
    )?;
    let (status, reason) = classify_copy_update_metadata(
        request
            .source_metadata
            .map(|metadata| &metadata.normalized_entry),
    );
    result.update_metadata_status = status;
    result.update_metadata_reason = reason;
    apply_copy_metadata_result(
        &mut result,
        lock_state,
        request.source_metadata,
        request.skill_name,
        request.staged_hash,
    )
    .await;
    Ok(result)
}

async fn copy_to_wsl_project_v2(
    request: CopyTargetRequest<'_>,
    target: &ResolvedCopyProject,
    guard: &MutationGuard<'_>,
) -> Result<CopyProjectResult, AppError> {
    let session = target.session.as_ref().expect("WSL project has a session");
    let resolver = AgentEnvironmentResolver::new(AgentEnvironmentContext {
        home: session.home.clone(),
        config_home: session.config_home.clone(),
        env: session.environment.clone(),
    });
    let plan = build_wsl_copy_target_plan(
        &resolver,
        &target.project_path,
        request.skill_name,
        request.agents,
        request.private_copy_agents,
    );
    let lock_state = if request.source_metadata.is_some() {
        Some(prepare_target_lock(target, request.skill_name).await)
    } else {
        None
    };
    if guard.cancellation().is_cancelled() {
        return Err(AppError::Custom {
            message: "Copy was cancelled before materializing the target".to_string(),
        });
    }
    guard.set_cancelable(false);
    let materialized = materialize_wsl_skill(
        session,
        WslMaterializeRequest {
            source_skill_path: request.staged.linux_repo_path().to_string(),
            canonical_root: format!(
                "{}/.agents/skills",
                target.project_path.trim_end_matches('/')
            ),
            install_dir_name: sanitize_name(request.skill_name),
            context_root: target.project_path.clone(),
            canonical_mode: InstallMode::Copy,
            targets: plan.materialize_targets.clone(),
        },
    )
    .await;
    guard.set_cancelable(true);
    let materialized = materialized?;
    let mut result = build_wsl_copy_project_result(
        &target.project_path,
        &plan,
        &materialized,
        request
            .source_metadata
            .map(|metadata| &metadata.normalized_entry),
    );
    if !result.success {
        return Ok(result);
    }
    apply_copy_metadata_result(
        &mut result,
        lock_state,
        request.source_metadata,
        request.skill_name,
        request.staged_hash,
    )
    .await;
    Ok(result)
}

async fn apply_copy_metadata_result(
    result: &mut CopyProjectResult,
    lock_state: Option<Result<TargetLockState, AppError>>,
    source_metadata: Option<&CopySourceMetadata>,
    skill_name: &str,
    computed_hash: &str,
) {
    let (Some(metadata), Some(lock_state)) = (source_metadata, lock_state) else {
        return;
    };
    let write_result = match lock_state {
        Ok(state) => {
            commit_target_lock(state, skill_name, &metadata.raw_entry, computed_hash).await
        }
        Err(error) => Err(error),
    };
    if let Err(error) = write_result {
        log::warn!("Failed to write copied skill metadata: {error}");
        result.update_metadata_status = CopyUpdateMetadataStatus::Missing;
        result.update_metadata_reason = Some("lock-write-failed".to_string());
    }
}

fn build_wsl_copy_project_result(
    target_path: &str,
    plan: &WslCopyTargetPlan,
    materialized: &WslMaterializeResult,
    source_lock_entry: Option<&LocalSkillLockEntry>,
) -> CopyProjectResult {
    let failed = materialized
        .targets
        .iter()
        .filter(|target| !target.success)
        .collect::<Vec<_>>();
    let successful = materialized
        .targets
        .iter()
        .filter(|target| target.success && !target.skipped)
        .map(|target| target.agent.clone())
        .collect::<HashSet<_>>();
    let (update_metadata_status, update_metadata_reason) =
        classify_copy_update_metadata(source_lock_entry);
    CopyProjectResult {
        project_path: target_path.to_string(),
        success: failed.is_empty(),
        error: (!failed.is_empty()).then(|| {
            failed
                .iter()
                .map(|target| {
                    target
                        .error
                        .clone()
                        .unwrap_or_else(|| format!("Failed to materialize {}", target.agent))
                })
                .collect::<Vec<_>>()
                .join("; ")
        }),
        default_available_agents: plan
            .default_available_agents
            .iter()
            .map(ToString::to_string)
            .collect(),
        private_adapted_agents: plan
            .private_required_agents
            .iter()
            .filter(|agent| successful.contains(&agent.to_string()))
            .map(ToString::to_string)
            .collect(),
        private_copy_agents: plan
            .private_copy_agents
            .iter()
            .filter(|agent| successful.contains(&agent.to_string()))
            .map(ToString::to_string)
            .collect(),
        skipped_agents: materialized
            .targets
            .iter()
            .filter(|target| target.skipped)
            .map(|target| target.agent.clone())
            .collect(),
        update_metadata_status,
        update_metadata_reason,
    }
}

fn copy_failure_result(project_path: &str, error: String) -> CopyProjectResult {
    CopyProjectResult {
        project_path: project_path.to_string(),
        success: false,
        error: Some(error),
        default_available_agents: Vec::new(),
        private_adapted_agents: Vec::new(),
        private_copy_agents: Vec::new(),
        skipped_agents: Vec::new(),
        update_metadata_status: CopyUpdateMetadataStatus::Missing,
        update_metadata_reason: Some("copy-failed".to_string()),
    }
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

    let (mut update_metadata_status, mut update_metadata_reason) =
        classify_copy_update_metadata(source_lock_entry);

    if let Some(entry) = source_lock_entry {
        let target_canonical = canonical_skills_dir(false, target_path)
            .join(crate::core::skill::sanitize_name(skill_name));
        let computed_hash = compute_skill_folder_hash(&target_canonical).unwrap_or_default();

        let mut new_entry = entry.clone();
        new_entry.computed_hash = computed_hash;
        new_entry.subagents = None;

        if let Err(err) = add_skill_to_local_lock(skill_name, new_entry, target_path) {
            log::warn!("Failed to write copied skill metadata: {}", err);
            update_metadata_status = CopyUpdateMetadataStatus::Missing;
            update_metadata_reason = Some("lock-write-failed".to_string());
        }
    }

    Ok(build_copy_project_result(
        target_path,
        &default_available,
        &private_required_agents,
        &private_copy_agents,
        &per_agent_results,
        update_metadata_status,
        update_metadata_reason,
    ))
}

fn classify_copy_update_metadata(
    source_lock_entry: Option<&crate::core::local_lock::LocalSkillLockEntry>,
) -> (CopyUpdateMetadataStatus, Option<String>) {
    let Some(entry) = source_lock_entry else {
        return (
            CopyUpdateMetadataStatus::Missing,
            Some("missing-source".to_string()),
        );
    };

    if entry.source.trim().is_empty() {
        return (
            CopyUpdateMetadataStatus::Missing,
            Some("missing-source".to_string()),
        );
    }

    let metadata = crate::core::normalize_local_lock_entry(entry);
    let capability = crate::core::derive_update_capability(&metadata);

    if capability.can_check_for_updates {
        (CopyUpdateMetadataStatus::Preserved, None)
    } else {
        (
            CopyUpdateMetadataStatus::Incomplete,
            capability
                .reason
                .or_else(|| Some("unsupported-source-type".to_string())),
        )
    }
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
    update_metadata_status: CopyUpdateMetadataStatus,
    update_metadata_reason: Option<String>,
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
        update_metadata_status,
        update_metadata_reason,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::local_lock::{add_skill_to_local_lock, read_local_lock, LocalSkillLockEntry};
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

    fn remote_lock_entry(
        source: &str,
        skill_path: Option<&str>,
        remote_hash: Option<&str>,
    ) -> LocalSkillLockEntry {
        LocalSkillLockEntry {
            source: source.to_string(),
            ref_name: Some("main".to_string()),
            source_type: "github".to_string(),
            source_url: Some(format!("https://github.com/{}", source)),
            computed_hash: "source-computed-hash".to_string(),
            remote_hash: remote_hash.map(str::to_string),
            skill_path: skill_path.map(str::to_string),
            subagents: Some(vec!["researcher".to_string()]),
            plugin_name: Some("plugin-a".to_string()),
        }
    }

    fn write_source_lock(project: &std::path::Path, name: &str, entry: LocalSkillLockEntry) {
        add_skill_to_local_lock(name, entry, project.to_string_lossy().as_ref()).unwrap();
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

    #[test]
    fn test_copy_skill_preserves_remote_update_metadata() {
        let source = tempdir().unwrap();
        let target = tempdir().unwrap();
        setup_source_skill(source.path(), "my-skill");
        write_source_lock(
            source.path(),
            "my-skill",
            remote_lock_entry(
                "owner/repo",
                Some("skills/my-skill/SKILL.md"),
                Some("remote123"),
            ),
        );

        let result = copy_skill_to_projects(
            "my-skill".to_string(),
            source.path().to_string_lossy().to_string(),
            vec![target.path().to_string_lossy().to_string()],
            vec!["antigravity".to_string()],
            vec![],
        )
        .unwrap();

        let project_result = &result.results[0];
        assert!(project_result.success);
        assert_eq!(
            project_result.update_metadata_status,
            CopyUpdateMetadataStatus::Preserved
        );
        assert_eq!(project_result.update_metadata_reason, None);

        let target_lock = read_local_lock(target.path().to_string_lossy().as_ref()).unwrap();
        let copied = target_lock.skills.get("my-skill").unwrap();
        assert_eq!(copied.source, "owner/repo");
        assert_eq!(copied.source_type, "github");
        assert_eq!(
            copied.source_url.as_deref(),
            Some("https://github.com/owner/repo")
        );
        assert_eq!(copied.ref_name.as_deref(), Some("main"));
        assert_eq!(
            copied.skill_path.as_deref(),
            Some("skills/my-skill/SKILL.md")
        );
        assert_eq!(copied.remote_hash.as_deref(), Some("remote123"));
        assert_eq!(copied.plugin_name.as_deref(), Some("plugin-a"));
        assert_eq!(copied.subagents, None);
        assert_ne!(copied.computed_hash, "source-computed-hash");
        assert!(!copied.computed_hash.is_empty());
    }

    #[test]
    fn test_copy_skill_does_not_inherit_source_eve_subagents() {
        let source = tempdir().unwrap();
        let target = tempdir().unwrap();
        setup_source_skill(source.path(), "my-skill");
        write_source_lock(
            source.path(),
            "my-skill",
            remote_lock_entry(
                "owner/repo",
                Some("skills/my-skill/SKILL.md"),
                Some("remote123"),
            ),
        );

        let result = copy_skill_to_projects(
            "my-skill".to_string(),
            source.path().to_string_lossy().to_string(),
            vec![target.path().to_string_lossy().to_string()],
            vec!["antigravity".to_string()],
            vec![],
        )
        .unwrap();

        assert!(result.results[0].success);
        let target_lock = read_local_lock(target.path().to_string_lossy().as_ref()).unwrap();
        assert_eq!(target_lock.skills["my-skill"].subagents, None);
    }

    #[test]
    fn test_copy_skill_keeps_success_when_update_metadata_cannot_be_written() {
        let source = tempdir().unwrap();
        let target = tempdir().unwrap();
        setup_source_skill(source.path(), "my-skill");
        write_source_lock(
            source.path(),
            "my-skill",
            remote_lock_entry(
                "owner/repo",
                Some("skills/my-skill/SKILL.md"),
                Some("remote123"),
            ),
        );
        fs::create_dir(target.path().join("skills-lock.json")).unwrap();

        let result = copy_skill_to_projects(
            "my-skill".to_string(),
            source.path().to_string_lossy().to_string(),
            vec![target.path().to_string_lossy().to_string()],
            vec!["antigravity".to_string()],
            vec![],
        )
        .unwrap();

        let project_result = &result.results[0];
        assert!(project_result.success);
        assert_eq!(project_result.error, None);
        assert_eq!(
            project_result.update_metadata_status,
            CopyUpdateMetadataStatus::Missing
        );
        assert_eq!(
            project_result.update_metadata_reason.as_deref(),
            Some("lock-write-failed")
        );
        assert!(target
            .path()
            .join(".agents")
            .join("skills")
            .join("my-skill")
            .join("SKILL.md")
            .exists());
    }

    #[test]
    fn test_copy_skill_reports_missing_update_metadata_without_source_lock() {
        let source = tempdir().unwrap();
        let target = tempdir().unwrap();
        setup_source_skill(source.path(), "my-skill");

        let result = copy_skill_to_projects(
            "my-skill".to_string(),
            source.path().to_string_lossy().to_string(),
            vec![target.path().to_string_lossy().to_string()],
            vec!["antigravity".to_string()],
            vec![],
        )
        .unwrap();

        let project_result = &result.results[0];
        assert!(project_result.success);
        assert_eq!(
            project_result.update_metadata_status,
            CopyUpdateMetadataStatus::Missing
        );
        assert_eq!(
            project_result.update_metadata_reason.as_deref(),
            Some("missing-source")
        );

        let target_lock = read_local_lock(target.path().to_string_lossy().as_ref()).unwrap();
        assert!(!target_lock.skills.contains_key("my-skill"));
    }

    #[test]
    fn test_copy_skill_reports_incomplete_update_metadata_for_missing_remote_hash() {
        let source = tempdir().unwrap();
        let target = tempdir().unwrap();
        setup_source_skill(source.path(), "my-skill");
        write_source_lock(
            source.path(),
            "my-skill",
            remote_lock_entry("owner/repo", Some("skills/my-skill/SKILL.md"), None),
        );

        let result = copy_skill_to_projects(
            "my-skill".to_string(),
            source.path().to_string_lossy().to_string(),
            vec![target.path().to_string_lossy().to_string()],
            vec!["antigravity".to_string()],
            vec![],
        )
        .unwrap();

        let project_result = &result.results[0];
        assert!(project_result.success);
        assert_eq!(
            project_result.update_metadata_status,
            CopyUpdateMetadataStatus::Incomplete
        );
        assert_eq!(
            project_result.update_metadata_reason.as_deref(),
            Some("missing-remote-hash")
        );
    }

    #[test]
    fn copy_v2_requires_explicit_project_contexts_without_reordering_targets() {
        let source = crate::environment::types::ContextRef {
            environment: crate::environment::types::EnvironmentRef::Wsl {
                distro_name: "Ubuntu".to_string(),
            },
            scope: crate::environment::types::ContextScope::Project {
                project_id: "source".to_string(),
            },
        };
        let targets = vec![
            crate::environment::types::ContextRef {
                environment: crate::environment::types::EnvironmentRef::Host,
                scope: crate::environment::types::ContextScope::Project {
                    project_id: "windows-target".to_string(),
                },
            },
            crate::environment::types::ContextRef {
                environment: crate::environment::types::EnvironmentRef::Wsl {
                    distro_name: "Debian".to_string(),
                },
                scope: crate::environment::types::ContextScope::Project {
                    project_id: "debian-target".to_string(),
                },
            },
        ];

        let validated = validate_copy_contexts(&source, &targets).expect("valid contexts");

        assert_eq!(validated, targets);
        let global = crate::environment::types::ContextRef {
            environment: crate::environment::types::EnvironmentRef::Host,
            scope: crate::environment::types::ContextScope::Global,
        };
        assert!(validate_copy_contexts(&global, &targets).is_err());
        assert!(validate_copy_contexts(&source, &[global]).is_err());
    }

    #[test]
    fn wsl_copy_plan_separates_default_and_private_required_agents() {
        let resolver = crate::environment::agent_environment::AgentEnvironmentResolver::new(
            crate::environment::agent_environment::AgentEnvironmentContext {
                home: "/home/alice".to_string(),
                config_home: "/home/alice/.config".to_string(),
                env: Default::default(),
            },
        );

        let plan = build_wsl_copy_target_plan(
            &resolver,
            "/work/target",
            "toolkit",
            &[AgentType::Firebender, AgentType::ClaudeCode],
            &[],
        );

        assert!(plan
            .default_available_agents
            .contains(&AgentType::Firebender));
        assert_eq!(plan.materialize_targets.len(), 1);
        assert_eq!(plan.materialize_targets[0].agent, "claude-code");
        assert_eq!(
            plan.materialize_targets[0].skills_root,
            "/work/target/.claude/skills"
        );
    }

    #[test]
    fn copied_lock_entry_preserves_unknown_fields_and_clears_eve_subagents() {
        let source = serde_json::json!({
            "source": "owner/repo",
            "sourceType": "github",
            "computedHash": "source-hash",
            "subagents": ["researcher"],
            "futureField": { "enabled": true }
        });

        let copied = build_copied_lock_entry(&source, "target-hash").expect("copy entry");

        assert_eq!(copied["computedHash"], "target-hash");
        assert!(copied.get("subagents").is_none());
        assert_eq!(copied["futureField"]["enabled"], true);
    }

    #[test]
    fn project_copy_lock_normalization_migrates_legacy_metadata() {
        let legacy = br#"{
          "version": 3,
          "skills": {
            "toolkit": {
              "source": "owner/repo",
              "sourceType": "github",
              "sourceUrl": "https://github.com/owner/repo",
              "skillPath": "skills/toolkit/SKILL.md",
              "skillFolderHash": "remote-hash",
              "installedAt": "",
              "updatedAt": ""
            }
          }
        }"#;

        let normalized = normalize_project_copy_lock_bytes(None, Some(legacy)).expect("migrate");
        let value: serde_json::Value = serde_json::from_slice(&normalized).expect("parse");

        assert_eq!(value["version"], 1);
        assert_eq!(value["skills"]["toolkit"]["remoteHash"], "remote-hash");
        assert_eq!(value["skills"]["toolkit"]["computedHash"], "");
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn native_host_copy_staging_does_not_require_drvfs_mapping() {
        let staging = create_host_copy_staging().expect("native staging");

        assert!(staging.host_repo_path().is_dir());
        assert_eq!(
            staging.linux_repo_path(),
            staging.host_repo_path().to_string_lossy()
        );
    }
}

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
use crate::core::local_lock::{compute_skill_folder_hash, LocalSkillLockEntry};
use crate::core::lock_repository::{
    LockMutationTargets, LockRepository, LockTarget, LockTransaction,
};
use crate::core::lossless_lock::LockSchema;
use crate::core::mutation::{MutationGuard, MutationKind, MutationPhase, SingleMutationController};
use crate::core::paths::canonical_skills_dir;
use crate::core::skill::sanitize_name;
use crate::environment::acquisition::{stage_wsl_source, StagedWslSource, WslAcquisitionSource};
use crate::environment::agent_environment::{AgentEnvironmentContext, AgentEnvironmentResolver};
use crate::environment::context_resolver::ContextResolver;
use crate::environment::lock_io::EnvironmentLockIo;
use crate::environment::materialize::{
    materialize_wsl_skill, WslMaterializeRequest, WslMaterializeResult, WslMaterializeTarget,
};
use crate::environment::path_mapping::map_windows_path_with_wslpath;
use crate::environment::service::ResolvedContext;
use crate::environment::types::{ContextRef, ContextScope, EnvironmentRef, ResourceLocator};
use crate::environment::wsl::{EnvironmentRegistry, WslSession};
use crate::error::AppError;
use crate::models::{InstallMode, Scope};
use serde_json::Value;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use tauri::State;

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
    resolved: ResolvedContext,
    session: Option<WslSession>,
}

impl ResolvedCopyProject {
    fn context(&self) -> &ContextRef {
        &self.resolved.context
    }

    fn project_path(&self) -> &str {
        self.resolved.context_root()
    }
}

#[derive(Debug, Clone)]
struct CopySourceMetadata {
    raw_entry: Value,
    normalized_entry: LocalSkillLockEntry,
}

enum PreparedCopySource {
    Host {
        _temp_dir: tempfile::TempDir,
        host_repo_path: PathBuf,
    },
    Wsl(StagedWslSource),
}

impl PreparedCopySource {
    fn host_repo_path(&self) -> &Path {
        match self {
            Self::Host { host_repo_path, .. } => host_repo_path,
            Self::Wsl(staging) => staging.host_repo_path(),
        }
    }

    async fn linux_repo_path(&self, session: &WslSession) -> Result<String, AppError> {
        map_windows_path_with_wslpath(session, &self.host_repo_path().to_string_lossy()).await
    }
}

fn create_host_copy_staging() -> Result<PreparedCopySource, AppError> {
    let temp_dir = tempfile::Builder::new()
        .prefix("skill-deck-copy-")
        .tempdir()
        .map_err(|error| AppError::Io {
            message: format!("failed to create Host copy staging directory: {error}"),
        })?;
    let host_repo_path = temp_dir.path().join("repo");
    fs::create_dir_all(&host_repo_path)?;
    Ok(PreparedCopySource::Host {
        _temp_dir: temp_dir,
        host_repo_path,
    })
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

/// 复制项目级 skill 到其他项目
///
/// # Arguments
/// * `skill_name` - skill 名称
/// * `source_project_path` - 源项目路径
/// * `target_project_paths` - 目标项目路径列表
/// * `agents` - 要安装的 agent 列表（与源 skill 相同）
#[cfg(test)]
pub async fn copy_skill_to_projects_host(
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
    let source_metadata = read_host_copy_source_metadata(&source_project_path, &skill_name).await;

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
            source_metadata.as_ref(),
        )
        .await;
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
pub async fn copy_skill_to_projects(
    skill_name: String,
    source: ContextRef,
    targets: Vec<ContextRef>,
    agents: Vec<String>,
    private_copy_agents: Vec<String>,
    registry: State<'_, EnvironmentRegistry>,
    controller: State<'_, SingleMutationController>,
) -> Result<CopySkillResult, AppError> {
    let targets = validate_copy_contexts(&source, &targets)?;
    let guard = controller.begin(MutationKind::Copy, source.clone())?;
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
        let result = match &target_project.context().environment {
            EnvironmentRef::Host => copy_to_host_project(request, &target_project).await,
            EnvironmentRef::Wsl { .. } => {
                copy_to_wsl_project(request, &target_project, &guard).await
            }
        };
        results.push(result.unwrap_or_else(|error| {
            copy_failure_result(target_project.project_path(), error.to_string())
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
    project_id(context).ok_or_else(|| AppError::Custom {
        message: "Copy requires a project context".to_string(),
    })?;
    match &context.environment {
        EnvironmentRef::Host => {
            let resolved = ContextResolver::resolve_host(context.clone())?;
            Ok(ResolvedCopyProject {
                resolved,
                session: None,
            })
        }
        EnvironmentRef::Wsl { distro_name } => {
            let distro_name = distro_name.clone();
            let retry_context = context.clone();
            registry
                .with_session_retry(&distro_name, move |session| {
                    let context = retry_context.clone();
                    async move {
                        let resolved = ContextResolver::resolve_wsl(context, &session).await?;
                        Ok(ResolvedCopyProject {
                            resolved,
                            session: Some(session),
                        })
                    }
                })
                .await
        }
    }
}

async fn prepare_copy_source(
    source: &ResolvedCopyProject,
    skill_name: &str,
    guard: &MutationGuard<'_>,
) -> Result<PreparedCopySource, AppError> {
    let sanitized = sanitize_name(skill_name);
    match &source.context().environment {
        EnvironmentRef::Host => {
            let source_canonical =
                canonical_skills_dir(false, source.project_path()).join(sanitized);
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
                source.project_path().trim_end_matches('/'),
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
    let root = project.project_path().trim_end_matches(['/', '\\']);
    (
        project.resolved.lock.clone(),
        ResourceLocator {
            environment: project.context().environment.clone(),
            native_path: format!("{root}/.agents/.skill-lock.json"),
        },
    )
}

fn project_lock_io(project: &ResolvedCopyProject) -> EnvironmentLockIo {
    match &project.context().environment {
        EnvironmentRef::Host => EnvironmentLockIo::Host,
        EnvironmentRef::Wsl { .. } => {
            EnvironmentLockIo::Wsl(project.session.clone().expect("WSL project has a session"))
        }
    }
}

fn project_lock_target(project: &ResolvedCopyProject) -> LockTarget {
    let (primary, legacy) = project_lock_locators(project);
    LockTarget {
        primary,
        legacy: Some(legacy),
        schema: LockSchema::Project,
    }
}

fn host_project_lock_target(project_path: &str) -> LockTarget {
    let project_path = Path::new(project_path);
    LockTarget {
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
    }
}

async fn load_copy_source_metadata(
    repository: &LockRepository,
    target: &LockTarget,
    skill_name: &str,
) -> Result<CopySourceMetadata, AppError> {
    let value = repository.read_document(target).await?.into_value();
    let raw_entry = value
        .get("skills")
        .and_then(Value::as_object)
        .and_then(|skills| skills.get(skill_name))
        .cloned()
        .ok_or_else(|| AppError::InvalidSource {
            value: format!("Skill '{skill_name}' not found in source lock"),
        })?;
    let normalized_entry = serde_json::from_value(raw_entry.clone())?;
    Ok(CopySourceMetadata {
        raw_entry,
        normalized_entry,
    })
}

#[cfg(test)]
async fn read_host_copy_source_metadata(
    project_path: &str,
    skill_name: &str,
) -> Option<CopySourceMetadata> {
    let repository = LockRepository::new(EnvironmentLockIo::Host);
    match load_copy_source_metadata(
        &repository,
        &host_project_lock_target(project_path),
        skill_name,
    )
    .await
    {
        Ok(metadata) => Some(metadata),
        Err(error) => {
            log::warn!("Failed to read source project lock metadata for copy: {error}");
            None
        }
    }
}

async fn read_copy_source_metadata(
    source: &ResolvedCopyProject,
    skill_name: &str,
) -> Option<CopySourceMetadata> {
    let repository = LockRepository::new(project_lock_io(source));
    let loaded =
        load_copy_source_metadata(&repository, &project_lock_target(source), skill_name).await;
    match loaded {
        Ok(metadata) => Some(metadata),
        Err(error) => {
            log::warn!("Failed to read source project lock metadata for copy: {error}");
            None
        }
    }
}

async fn copy_to_host_project(
    request: CopyTargetRequest<'_>,
    target: &ResolvedCopyProject,
) -> Result<CopyProjectResult, AppError> {
    copy_to_single_project(
        request.skill_name,
        request.staged.host_repo_path(),
        target.project_path(),
        request.agents,
        request.private_copy_agents,
        request.source_metadata,
    )
    .await
}

async fn copy_to_wsl_project(
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
        target.project_path(),
        request.skill_name,
        request.agents,
        request.private_copy_agents,
    );
    let lock_repository = LockRepository::new(project_lock_io(target));
    let lock_transaction = if request.source_metadata.is_some() {
        Some(
            lock_repository
                .begin(
                    project_lock_target(target),
                    LockMutationTargets {
                        entries: vec![request.skill_name.to_string()],
                        default_target_agents: false,
                    },
                )
                .await,
        )
    } else {
        None
    };
    if guard.cancellation().is_cancelled() {
        return Err(AppError::Custom {
            message: "Copy was cancelled before materializing the target".to_string(),
        });
    }
    guard.transition(MutationPhase::Materializing, None, false);
    let materialized = materialize_wsl_skill(
        session,
        WslMaterializeRequest {
            source_skill_path: request.staged.linux_repo_path(session).await?,
            canonical_root: target.resolved.skill_root.native_path.clone(),
            install_dir_name: sanitize_name(request.skill_name),
            context_root: target.project_path().to_string(),
            canonical_mode: InstallMode::Copy,
            targets: plan.materialize_targets.clone(),
        },
    )
    .await;
    let materialized = materialized?;
    let mut result = build_wsl_copy_project_result(
        target.project_path(),
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
        lock_transaction,
        request.source_metadata,
        request.skill_name,
        request.staged_hash,
    )
    .await;
    Ok(result)
}

async fn apply_copy_metadata_result(
    result: &mut CopyProjectResult,
    lock_transaction: Option<Result<LockTransaction<'_>, AppError>>,
    source_metadata: Option<&CopySourceMetadata>,
    skill_name: &str,
    computed_hash: &str,
) {
    let (Some(metadata), Some(lock_transaction)) = (source_metadata, lock_transaction) else {
        return;
    };
    let write_result = match lock_transaction {
        Ok(mut transaction) => match build_copied_lock_entry(&metadata.raw_entry, computed_hash) {
            Ok(replacement) => match transaction.replace_entry(skill_name, replacement) {
                Ok(()) => transaction.commit().await,
                Err(error) => Err(error),
            },
            Err(error) => Err(error),
        },
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

async fn copy_to_single_project(
    skill_name: &str,
    source_canonical: &std::path::Path,
    target_path: &str,
    agent_types: &[AgentType],
    private_copy_agent_types: &[AgentType],
    source_metadata: Option<&CopySourceMetadata>,
) -> Result<CopyProjectResult, AppError> {
    let lock_repository = LockRepository::new(EnvironmentLockIo::Host);
    let lock_transaction = if source_metadata.is_some() {
        Some(
            lock_repository
                .begin(
                    host_project_lock_target(target_path),
                    LockMutationTargets {
                        entries: vec![skill_name.to_string()],
                        default_target_agents: false,
                    },
                )
                .await,
        )
    } else {
        None
    };
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

    let (update_metadata_status, update_metadata_reason) =
        classify_copy_update_metadata(source_metadata.map(|metadata| &metadata.normalized_entry));

    let computed_hash = if source_metadata.is_some() {
        let target_canonical = canonical_skills_dir(false, target_path)
            .join(crate::core::skill::sanitize_name(skill_name));
        compute_skill_folder_hash(&target_canonical).unwrap_or_default()
    } else {
        String::new()
    };

    let mut result = build_copy_project_result(
        target_path,
        &default_available,
        &private_required_agents,
        &private_copy_agents,
        &per_agent_results,
        update_metadata_status,
        update_metadata_reason,
    );
    apply_copy_metadata_result(
        &mut result,
        lock_transaction,
        source_metadata,
        skill_name,
        &computed_hash,
    )
    .await;
    Ok(result)
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
    use crate::core::local_lock::{read_local_lock, LocalSkillLockEntry, LocalSkillLockFile};
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
        let mut lock = LocalSkillLockFile::empty();
        lock.skills.insert(name.to_string(), entry);
        fs::write(
            project.join("skills-lock.json"),
            serde_json::to_string_pretty(&lock).unwrap() + "\n",
        )
        .unwrap();
    }

    #[tokio::test]
    async fn test_copy_skill_returns_error_for_missing_source() {
        let source = tempdir().unwrap();
        let target = tempdir().unwrap();

        let result = copy_skill_to_projects_host(
            "nonexistent".to_string(),
            source.path().to_string_lossy().to_string(),
            vec![target.path().to_string_lossy().to_string()],
            vec![],
            vec![],
        )
        .await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_copy_skill_copies_to_target_canonical() {
        let source = tempdir().unwrap();
        let target = tempdir().unwrap();
        setup_source_skill(source.path(), "my-skill");

        let result = copy_skill_to_projects_host(
            "my-skill".to_string(),
            source.path().to_string_lossy().to_string(),
            vec![target.path().to_string_lossy().to_string()],
            vec!["cursor".to_string()],
            vec![],
        )
        .await
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

    #[tokio::test]
    async fn test_copy_skill_overwrites_existing_in_target() {
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

        let result = copy_skill_to_projects_host(
            "my-skill".to_string(),
            source.path().to_string_lossy().to_string(),
            vec![target.path().to_string_lossy().to_string()],
            vec!["cursor".to_string()],
            vec![],
        )
        .await
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

    #[tokio::test]
    async fn test_copy_skill_multiple_targets() {
        let source = tempdir().unwrap();
        let target_a = tempdir().unwrap();
        let target_b = tempdir().unwrap();
        setup_source_skill(source.path(), "my-skill");

        let result = copy_skill_to_projects_host(
            "my-skill".to_string(),
            source.path().to_string_lossy().to_string(),
            vec![
                target_a.path().to_string_lossy().to_string(),
                target_b.path().to_string_lossy().to_string(),
            ],
            vec!["cursor".to_string()],
            vec![],
        )
        .await
        .unwrap();

        assert_eq!(result.results.len(), 2);
        assert!(result.results.iter().all(|r| r.success));
    }

    #[tokio::test]
    async fn test_copy_skill_reports_default_private_and_skipped_targets() {
        let source = tempdir().unwrap();
        let target = tempdir().unwrap();
        setup_source_skill(source.path(), "my-skill");

        let result = copy_skill_to_projects_host(
            "my-skill".to_string(),
            source.path().to_string_lossy().to_string(),
            vec![target.path().to_string_lossy().to_string()],
            vec!["antigravity".to_string(), "kiro-cli".to_string()],
            vec![],
        )
        .await
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

    #[tokio::test]
    async fn test_copy_skill_preserves_remote_update_metadata() {
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

        let result = copy_skill_to_projects_host(
            "my-skill".to_string(),
            source.path().to_string_lossy().to_string(),
            vec![target.path().to_string_lossy().to_string()],
            vec!["antigravity".to_string()],
            vec![],
        )
        .await
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

    #[tokio::test]
    async fn test_copy_skill_does_not_inherit_source_eve_subagents() {
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

        let result = copy_skill_to_projects_host(
            "my-skill".to_string(),
            source.path().to_string_lossy().to_string(),
            vec![target.path().to_string_lossy().to_string()],
            vec!["antigravity".to_string()],
            vec![],
        )
        .await
        .unwrap();

        assert!(result.results[0].success);
        let target_lock = read_local_lock(target.path().to_string_lossy().as_ref()).unwrap();
        assert_eq!(target_lock.skills["my-skill"].subagents, None);
    }

    #[tokio::test]
    async fn test_copy_skill_keeps_success_when_update_metadata_cannot_be_written() {
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

        let result = copy_skill_to_projects_host(
            "my-skill".to_string(),
            source.path().to_string_lossy().to_string(),
            vec![target.path().to_string_lossy().to_string()],
            vec!["antigravity".to_string()],
            vec![],
        )
        .await
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

    #[tokio::test]
    async fn test_copy_skill_reports_missing_update_metadata_without_source_lock() {
        let source = tempdir().unwrap();
        let target = tempdir().unwrap();
        setup_source_skill(source.path(), "my-skill");

        let result = copy_skill_to_projects_host(
            "my-skill".to_string(),
            source.path().to_string_lossy().to_string(),
            vec![target.path().to_string_lossy().to_string()],
            vec!["antigravity".to_string()],
            vec![],
        )
        .await
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

    #[tokio::test]
    async fn test_copy_skill_reports_incomplete_update_metadata_for_missing_remote_hash() {
        let source = tempdir().unwrap();
        let target = tempdir().unwrap();
        setup_source_skill(source.path(), "my-skill");
        write_source_lock(
            source.path(),
            "my-skill",
            remote_lock_entry("owner/repo", Some("skills/my-skill/SKILL.md"), None),
        );

        let result = copy_skill_to_projects_host(
            "my-skill".to_string(),
            source.path().to_string_lossy().to_string(),
            vec![target.path().to_string_lossy().to_string()],
            vec!["antigravity".to_string()],
            vec![],
        )
        .await
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
    fn copy_requires_explicit_project_contexts_without_reordering_targets() {
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
    fn copy_project_lock_locators_use_resolved_primary_lock() {
        let environment = EnvironmentRef::Host;
        let project = crate::environment::types::ProjectBinding {
            id: "app".to_string(),
            native_path: "/work/app".to_string(),
            display_name: None,
            order: None,
            suppress_cross_storage_warning: false,
        };
        let resolved = ResolvedCopyProject {
            resolved: crate::environment::service::ResolvedContext {
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
            },
            session: None,
        };

        let (primary, legacy) = project_lock_locators(&resolved);

        assert_eq!(primary.native_path, "/work/app/skills-lock.json");
        assert_eq!(legacy.native_path, "/work/app/.agents/.skill-lock.json");
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

    #[tokio::test]
    async fn project_copy_lock_normalization_migrates_legacy_metadata() {
        let project = tempdir().expect("project");
        let legacy_path = project.path().join(".agents/.skill-lock.json");
        fs::create_dir_all(legacy_path.parent().expect("legacy parent"))
            .expect("create legacy parent");
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
        fs::write(&legacy_path, legacy).expect("write legacy lock");
        let repository = LockRepository::new(EnvironmentLockIo::Host);
        let value = repository
            .read_document(&host_project_lock_target(
                project.path().to_string_lossy().as_ref(),
            ))
            .await
            .expect("read legacy lock")
            .into_value();

        assert_eq!(value["version"], 1);
        assert_eq!(value["skills"]["toolkit"]["remoteHash"], "remote-hash");
        assert_eq!(value["skills"]["toolkit"]["computedHash"], "");
        assert!(!project.path().join("skills-lock.json").exists());
        assert_eq!(fs::read(legacy_path).expect("read legacy lock"), legacy);
    }

    #[test]
    fn host_copy_staging_does_not_require_eager_wsl_mapping() {
        let staging = create_host_copy_staging().expect("native staging");

        assert!(staging.host_repo_path().is_dir());
    }
}

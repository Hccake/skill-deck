//! 删除相关的 Tauri Command
//!
//! 提供命令：
//! - remove_skill: 删除指定 skill（支持完全删除和部分移除）
//!
//! 对应 CLI: remove.ts 的 removeCommand()
//! GUI 增强：支持 full_removal（完全删除）和 agents 指定（部分移除）

use crate::core::agents::AgentType;
use crate::core::lock_repository::{LockMutationTargets, LockRepository, LockTarget};
use crate::core::lossless_lock::LockSchema;
use crate::core::mutation::{MutationKind, MutationPhase, SingleMutationController};
use crate::core::uninstaller;
use crate::environment::agent_environment::{AgentEnvironmentContext, AgentEnvironmentResolver};
use crate::environment::context_resolver::ContextResolver;
use crate::environment::lock_io::EnvironmentLockIo;
use crate::environment::service::{EnvironmentService, InspectRequest, ResolvedContext};
use crate::environment::types::{ContextRef, ContextScope, EnvironmentRef, ResourceLocator};
use crate::environment::wsl::{EnvironmentRegistry, WslSession};
use crate::environment::wsl_protocol::run_wsl_script;
use crate::error::AppError;
use crate::models::{InstallTargetSpec, RemoveResult, Scope};
use std::collections::HashSet;
use tauri::State;
use tokio::time::Duration;

const WSL_REMOVE_PATHS_SCRIPT: &str = r#"
printf '1\0'
for path in "$@"; do
  error=
  if rm -rf -- "$path" 2>/dev/null; then
    status=success
  else
    status=failed
    error='failed to remove path'
  fi
  printf 'path\0%s\0%s\0%s\0' "$path" "$status" "$error"
done
"#;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WslRemovePathResult {
    pub path: String,
    pub success: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct WslRemovePlan {
    agent_paths: Vec<String>,
    canonical_path: Option<String>,
}

impl WslRemovePlan {
    #[cfg(test)]
    fn ordered_paths(&self) -> Vec<&str> {
        self.agent_paths
            .iter()
            .map(String::as_str)
            .chain(self.canonical_path.iter().map(String::as_str))
            .collect()
    }
}

fn parse_wsl_remove_output(bytes: &[u8]) -> Result<Vec<WslRemovePathResult>, AppError> {
    let mut records = bytes
        .split(|byte| *byte == 0)
        .map(|record| String::from_utf8_lossy(record).into_owned())
        .collect::<Vec<_>>();
    if records.last().is_some_and(String::is_empty) {
        records.pop();
    }
    if records.first().map(String::as_str) != Some("1") {
        return Err(AppError::Custom {
            message: "invalid WSL remove response version".to_string(),
        });
    }
    let mut results = Vec::new();
    let mut index = 1;
    while index < records.len() {
        if records.get(index).map(String::as_str) != Some("path") || index + 3 >= records.len() {
            return Err(AppError::Custom {
                message: "invalid WSL remove response".to_string(),
            });
        }
        let success = match records[index + 2].as_str() {
            "success" => true,
            "failed" => false,
            _ => {
                return Err(AppError::Custom {
                    message: "invalid WSL remove status".to_string(),
                })
            }
        };
        results.push(WslRemovePathResult {
            path: records[index + 1].clone(),
            success,
            error: (!records[index + 3].is_empty()).then(|| records[index + 3].clone()),
        });
        index += 4;
    }
    Ok(results)
}

pub(crate) async fn remove_wsl_paths(
    session: &WslSession,
    paths: &[String],
) -> Result<Vec<WslRemovePathResult>, AppError> {
    if paths.is_empty() {
        return Ok(Vec::new());
    }
    let output = run_wsl_script(
        session,
        WSL_REMOVE_PATHS_SCRIPT,
        paths,
        Vec::new(),
        Duration::from_secs(30),
    )
    .await?;
    parse_wsl_remove_output(&output)
}

/// 删除指定 skill
///
/// # Arguments
/// * `scope` - 删除范围（global/project）
/// * `name` - skill 名称
/// * `project_path` - Project scope 时的项目路径
/// * `agents` - 部分移除时指定的 agent 列表（None 或空 = 完全删除）
/// * `full_removal` - 是否完全删除（true = 删除一切，false = 仅删除指定 agents 的 symlink）
/// * `agent_targets` - 具体目标列表；目前用于 Eve root/subagent 删除
#[tauri::command]
#[specta::specta]
pub async fn remove_skill(
    context: ContextRef,
    name: String,
    agents: Option<Vec<AgentType>>,
    full_removal: Option<bool>,
    agent_targets: Option<Vec<InstallTargetSpec>>,
    registry: State<'_, EnvironmentRegistry>,
    controller: State<'_, SingleMutationController>,
) -> Result<RemoveResult, AppError> {
    let guard = controller.begin(MutationKind::Remove, context.clone())?;
    let full = full_removal.unwrap_or(true);
    let eve_targets = resolve_eve_targets(agent_targets.as_deref());
    match &context.environment {
        EnvironmentRef::Host => {
            let resolved = ContextResolver::resolve_host(context)?;
            let (scope, project_path) = match resolved.context.scope {
                ContextScope::Global => (Scope::Global, None),
                ContextScope::Project { .. } => (
                    Scope::Project,
                    resolved.project.map(|project| project.native_path),
                ),
            };
            let lock_repository = LockRepository::new(EnvironmentLockIo::Host);
            let mut lock_transaction = if full {
                Some(
                    lock_repository
                        .begin(
                            host_remove_lock_target(&scope, project_path.as_deref())?,
                            LockMutationTargets {
                                entries: vec![name.clone()],
                                default_target_agents: false,
                            },
                        )
                        .await?,
                )
            } else {
                None
            };
            let (source, source_type) = lock_transaction
                .as_ref()
                .and_then(|transaction| transaction.initial_entry(&name))
                .map(lock_entry_source)
                .unwrap_or((None, None));
            let mut result = uninstaller::remove_skill(
                &name,
                &scope,
                project_path.as_deref(),
                full,
                agents.as_deref(),
                eve_targets.as_deref(),
            )?;
            if full && result.success {
                let transaction = lock_transaction
                    .as_mut()
                    .expect("full removal captured lock transaction");
                transaction.remove_entry(&name)?;
                guard.transition(MutationPhase::Committing, None, false);
                lock_transaction
                    .take()
                    .expect("full removal captured lock transaction")
                    .commit()
                    .await?;
                result.source = source;
                result.source_type = source_type;
            }
            Ok(result)
        }
        EnvironmentRef::Wsl { distro_name } => {
            let distro_name = distro_name.clone();
            let retry_context = context.clone();
            let guard = &guard;
            registry
                .with_session(&distro_name, move |session| {
                    let context = retry_context.clone();
                    let name = name.clone();
                    let agents = agents.clone();
                    let eve_targets = eve_targets.clone();
                    async move {
                        let resolved = ContextResolver::resolve_wsl(context, &session).await?;
                        remove_skill_wsl(
                            resolved,
                            &session,
                            &name,
                            agents.as_deref(),
                            full,
                            eve_targets.as_deref(),
                            guard,
                        )
                        .await
                    }
                })
                .await
        }
    }
}

fn host_remove_lock_target(
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
            let project_path = std::path::Path::new(project_path);
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

fn lock_entry_source(entry: &serde_json::Value) -> (Option<String>, Option<String>) {
    (
        entry
            .get("source")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string),
        entry
            .get("sourceType")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string),
    )
}

fn wsl_remove_resources(
    context: &ResolvedContext,
) -> (Scope, String, ResourceLocator, Option<ResourceLocator>) {
    let (scope, root) = match &context.context.scope {
        ContextScope::Global => (Scope::Global, context.home.native_path.clone()),
        ContextScope::Project { .. } => (
            Scope::Project,
            context
                .project
                .as_ref()
                .map(|project| project.native_path.clone())
                .unwrap_or_else(|| context.home.native_path.clone()),
        ),
    };
    let legacy = context.project.as_ref().map(|project| ResourceLocator {
        environment: context.context.environment.clone(),
        native_path: format!(
            "{}/.agents/.skill-lock.json",
            project.native_path.trim_end_matches('/')
        ),
    });
    (scope, root, context.lock.clone(), legacy)
}

#[allow(clippy::too_many_arguments)]
fn build_wsl_remove_plan(
    session: &WslSession,
    scope: &Scope,
    context_root: &str,
    skill_name: &str,
    agents: Option<&[AgentType]>,
    full: bool,
    eve_targets: Option<&[Option<String>]>,
    snapshot: &crate::environment::service::SkillEntrySnapshot,
) -> WslRemovePlan {
    let is_global = matches!(scope, Scope::Global);
    let sanitized = crate::core::skill::sanitize_name(skill_name);
    let resolver = AgentEnvironmentResolver::new(AgentEnvironmentContext {
        home: session.home.clone(),
        config_home: session.config_home.clone(),
        env: session.environment.clone(),
    });
    let mut seen = HashSet::new();
    let mut agent_paths = Vec::new();
    let target_agents: Vec<AgentType> = agents
        .map(|agents| agents.to_vec())
        .unwrap_or_else(|| AgentType::all().collect());
    for agent in target_agents {
        if agent == AgentType::Eve {
            continue;
        }
        if let Some(root) = resolver.target(agent, is_global, context_root).private_path {
            let path = format!("{}/{}", root.trim_end_matches('/'), sanitized);
            if seen.insert(path.clone()) {
                agent_paths.push(path);
            }
        }
    }

    let resolved_eve_targets: Vec<Option<String>> = match eve_targets {
        Some(targets) => targets.to_vec(),
        None if full => snapshot
            .eve_targets
            .iter()
            .map(|target| target.subagent.clone())
            .collect(),
        None if agents.is_some_and(|agents| agents.contains(&AgentType::Eve)) => vec![None],
        None => Vec::new(),
    };
    if !is_global {
        for subagent in resolved_eve_targets {
            let root = match subagent.as_deref() {
                Some(subagent) => format!(
                    "{}/agent/subagents/{}/skills",
                    context_root.trim_end_matches('/'),
                    crate::core::skill::sanitize_name(subagent)
                ),
                None => format!("{}/agent/skills", context_root.trim_end_matches('/')),
            };
            let path = format!("{}/{}", root, sanitized);
            if seen.insert(path.clone()) {
                agent_paths.push(path);
            }
        }
    }

    WslRemovePlan {
        agent_paths,
        canonical_path: should_remove_wsl_canonical(full, agents, &snapshot.private_adapted_agents)
            .then(|| snapshot.canonical_path.clone()),
    }
}

fn should_remove_wsl_canonical(
    full: bool,
    selected_agents: Option<&[AgentType]>,
    linked_agents: &[AgentType],
) -> bool {
    full && selected_agents.map_or(true, |selected| {
        linked_agents.iter().all(|agent| selected.contains(agent))
    })
}

#[allow(clippy::too_many_arguments)]
async fn remove_skill_wsl(
    context: ResolvedContext,
    session: &WslSession,
    name: &str,
    agents: Option<&[AgentType]>,
    full: bool,
    eve_targets: Option<&[Option<String>]>,
    guard: &crate::core::mutation::MutationGuard<'_>,
) -> Result<RemoveResult, AppError> {
    let (scope, context_root, primary_lock, legacy_lock) = wsl_remove_resources(&context);
    let lock_schema = match scope {
        Scope::Global => LockSchema::Global,
        Scope::Project => LockSchema::Project,
    };
    let lock_repository = LockRepository::new(EnvironmentLockIo::Wsl(session.clone()));
    let mut lock_transaction = if full {
        Some(
            lock_repository
                .begin(
                    LockTarget {
                        primary: primary_lock,
                        legacy: legacy_lock,
                        schema: lock_schema,
                    },
                    LockMutationTargets {
                        entries: vec![name.to_string()],
                        default_target_agents: false,
                    },
                )
                .await?,
        )
    } else {
        None
    };
    let (source, source_type) = lock_transaction
        .as_ref()
        .and_then(|transaction| transaction.initial_entry(name))
        .map(lock_entry_source)
        .unwrap_or((None, None));
    let canonical_root = context.skill_root.native_path.clone();
    let snapshot = EnvironmentService::Wsl(session.clone())
        .inspect(&InspectRequest { context })
        .await?
        .skills
        .into_iter()
        .find(|skill| skill.name == name)
        .unwrap_or(crate::environment::service::SkillEntrySnapshot {
            name: name.to_string(),
            description: String::new(),
            canonical_path: format!(
                "{}/{}",
                canonical_root,
                crate::core::skill::sanitize_name(name)
            ),
            canonical_present: false,
            agents: Vec::new(),
            card_agents: Vec::new(),
            default_available_agents: Vec::new(),
            private_adapted_agents: Vec::new(),
            duplicate_copy_agents: Vec::new(),
            private_only_agents: Vec::new(),
            private_copy_agents: Vec::new(),
            eve_targets: Vec::new(),
        });
    let plan = build_wsl_remove_plan(
        session,
        &scope,
        &context_root,
        name,
        agents,
        full,
        eve_targets,
        &snapshot,
    );
    if guard.cancellation().is_cancelled() {
        return Err(AppError::Custom {
            message: "Skill removal was cancelled".to_string(),
        });
    }
    guard.transition(MutationPhase::Materializing, None, false);
    let agent_results = remove_wsl_paths(session, &plan.agent_paths).await?;
    let failures: Vec<_> = agent_results
        .iter()
        .filter(|result| !result.success)
        .collect();
    let mut removed_paths: Vec<String> = agent_results
        .iter()
        .filter(|result| result.success)
        .map(|result| result.path.clone())
        .collect();
    if !failures.is_empty() {
        return Ok(RemoveResult {
            skill_name: name.to_string(),
            success: false,
            removed_paths,
            source: None,
            source_type: None,
            error: Some(
                failures
                    .iter()
                    .filter_map(|result| result.error.clone())
                    .collect::<Vec<_>>()
                    .join("; "),
            ),
        });
    }
    if let Some(canonical_path) = &plan.canonical_path {
        let canonical_results =
            remove_wsl_paths(session, std::slice::from_ref(canonical_path)).await?;
        if let Some(result) = canonical_results.into_iter().next() {
            if !result.success {
                return Ok(RemoveResult {
                    skill_name: name.to_string(),
                    success: false,
                    removed_paths,
                    source: None,
                    source_type: None,
                    error: result.error,
                });
            }
            removed_paths.push(result.path);
        }
    }
    if full {
        let transaction = lock_transaction
            .as_mut()
            .expect("full removal captured lock transaction");
        transaction.remove_entry(name)?;
        guard.transition(MutationPhase::Committing, None, false);
        lock_transaction
            .take()
            .expect("full removal captured lock transaction")
            .commit()
            .await?;
    }
    Ok(RemoveResult {
        skill_name: name.to_string(),
        success: true,
        removed_paths,
        source,
        source_type,
        error: None,
    })
}

fn resolve_eve_targets(agent_targets: Option<&[InstallTargetSpec]>) -> Option<Vec<Option<String>>> {
    let agent_targets = agent_targets?;
    let mut targets = Vec::new();
    for target in agent_targets {
        if target.agent != AgentType::Eve {
            continue;
        }

        let subagent = target
            .subagent
            .as_ref()
            .filter(|value| !value.is_empty() && *value != "root")
            .cloned();
        if !targets.contains(&subagent) {
            targets.push(subagent);
        }
    }

    Some(targets)
}

#[cfg(test)]
mod environment_tests {
    use super::*;

    #[test]
    fn wsl_remove_resources_keep_primary_and_project_legacy_locks() {
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

        let (_, root, primary, legacy) = wsl_remove_resources(&resolved);

        assert_eq!(root, "/work/app");
        assert_eq!(primary.native_path, "/work/app/skills-lock.json");
        assert_eq!(
            legacy.unwrap().native_path,
            "/work/app/.agents/.skill-lock.json"
        );
    }

    #[test]
    fn wsl_remove_plan_orders_agent_paths_before_canonical() {
        let plan = WslRemovePlan {
            agent_paths: vec![
                "/work/.claude/skills/demo".to_string(),
                "/work/agent/skills/demo".to_string(),
            ],
            canonical_path: Some("/work/.agents/skills/demo".to_string()),
        };

        assert_eq!(
            plan.ordered_paths(),
            vec![
                "/work/.claude/skills/demo",
                "/work/agent/skills/demo",
                "/work/.agents/skills/demo",
            ]
        );
    }

    #[test]
    fn wsl_remove_output_keeps_partial_path_failures() {
        let output = b"1\0path\0/work/a\0success\0\0path\0/work/b\0failed\0permission denied\0";

        let results = parse_wsl_remove_output(output).expect("parse removal output");

        assert_eq!(results.len(), 2);
        assert!(results[0].success);
        assert!(!results[1].success);
        assert_eq!(results[1].error.as_deref(), Some("permission denied"));
    }

    #[test]
    fn wsl_full_remove_keeps_canonical_when_unselected_symlink_agent_remains() {
        assert!(!should_remove_wsl_canonical(
            true,
            Some(&[AgentType::ClaudeCode]),
            &[AgentType::ClaudeCode, AgentType::Cursor],
        ));
        assert!(should_remove_wsl_canonical(
            true,
            Some(&[AgentType::ClaudeCode, AgentType::Cursor]),
            &[AgentType::ClaudeCode, AgentType::Cursor],
        ));
    }
}

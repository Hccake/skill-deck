use crate::application::agents::ManagedAgentRegistry;
use crate::application::runtime_facts::AgentRegistrySnapshotSource;
use crate::core::agent_definition::AgentId;
use crate::core::agent_registry::AgentRegistrySnapshot;
use crate::core::lock_repository::{LockMutationTargets, LockRepository, LockTarget};
use crate::core::lossless_lock::LockSchema;
use crate::core::skill_lock;
use crate::environment::context_resolver::ContextResolver;
use crate::environment::lock_io::EnvironmentLockIo;
use crate::environment::types::{ContextRef, ContextScope, EnvironmentRef, ResourceLocator};
use crate::environment::wsl::WslRuntime;
use crate::error::AppError;

/// 获取 GUI scope-aware 默认安装目标
pub async fn get_default_target_agents_host(
    snapshot: &AgentRegistrySnapshot,
) -> Result<Option<skill_lock::DefaultTargetAgents>, AppError> {
    read_effective_default_target_agents(
        EnvironmentLockIo::Host,
        &host_default_target_agents_lock_target(),
        snapshot,
    )
    .await
}

pub async fn get_default_target_agents(
    context: ContextRef,
    registry: &WslRuntime,
    agent_registry: &ManagedAgentRegistry,
) -> Result<Option<skill_lock::DefaultTargetAgents>, AppError> {
    ensure_global_context(&context)?;
    let snapshot = agent_registry_snapshot_for_defaults(agent_registry);
    match &context.environment {
        EnvironmentRef::Host => {
            ContextResolver::resolve_host(context)?;
            get_default_target_agents_host(&snapshot).await
        }
        EnvironmentRef::Wsl { distro_name } => {
            let distro_name = distro_name.clone();
            let retry_context = context.clone();
            registry
                .with_session_retry(&distro_name, move |session| {
                    let context = retry_context.clone();
                    let snapshot = snapshot.clone();
                    async move {
                        let locator = ContextResolver::resolve_wsl(context, &session).await?.lock;
                        read_effective_default_target_agents(
                            EnvironmentLockIo::ActiveWsl(session),
                            &LockTarget {
                                primary: locator,
                                legacy: None,
                                schema: LockSchema::Global,
                            },
                            &snapshot,
                        )
                        .await
                    }
                })
                .await
        }
    }
}

async fn read_effective_default_target_agents(
    io: EnvironmentLockIo,
    target: &LockTarget,
    snapshot: &AgentRegistrySnapshot,
) -> Result<Option<skill_lock::DefaultTargetAgents>, AppError> {
    let document = LockRepository::new(io).read_document(target).await?;
    let stored = document
        .into_value()
        .get("defaultTargetAgents")
        .cloned()
        .map(serde_json::from_value)
        .transpose()?;
    Ok(stored.map(|stored| skill_lock::effective_default_target_agents(&stored, snapshot)))
}

pub(crate) async fn remove_default_target_agent_reference(
    context: ContextRef,
    id: &AgentId,
    environment_registry: &WslRuntime,
    post_delete_registry: &AgentRegistrySnapshot,
) -> Result<(), AppError> {
    let global_context = ContextRef {
        environment: context.environment.clone(),
        scope: ContextScope::Global,
    };
    match &global_context.environment {
        EnvironmentRef::Host => {
            let resolved = ContextResolver::resolve_host(global_context)?;
            remove_default_target_agent_reference_with_io(
                EnvironmentLockIo::Host,
                global_lock_target(resolved.lock),
                id,
                post_delete_registry,
            )
            .await
        }
        EnvironmentRef::Wsl { distro_name } => {
            let distro_name = distro_name.clone();
            environment_registry
                .with_session_retry(&distro_name, move |session| {
                    let context = global_context.clone();
                    let id = id.clone();
                    let post_delete_registry = post_delete_registry.clone();
                    async move {
                        let resolved = ContextResolver::resolve_wsl(context, &session).await?;
                        remove_default_target_agent_reference_with_io(
                            EnvironmentLockIo::ActiveWsl(session),
                            global_lock_target(resolved.lock),
                            &id,
                            &post_delete_registry,
                        )
                        .await
                    }
                })
                .await
        }
    }
}

pub(crate) async fn read_raw_default_target_agents(
    context: ContextRef,
    environment_registry: &WslRuntime,
) -> Result<Option<skill_lock::DefaultTargetAgents>, AppError> {
    let global_context = ContextRef {
        environment: context.environment.clone(),
        scope: ContextScope::Global,
    };
    match &global_context.environment {
        EnvironmentRef::Host => {
            let resolved = ContextResolver::resolve_host(global_context)?;
            read_raw_default_target_agents_with_io(
                EnvironmentLockIo::Host,
                global_lock_target(resolved.lock),
            )
            .await
        }
        EnvironmentRef::Wsl { distro_name } => {
            let distro_name = distro_name.clone();
            environment_registry
                .with_session_retry(&distro_name, move |session| {
                    let context = global_context.clone();
                    async move {
                        let resolved = ContextResolver::resolve_wsl(context, &session).await?;
                        read_raw_default_target_agents_with_io(
                            EnvironmentLockIo::ActiveWsl(session),
                            global_lock_target(resolved.lock),
                        )
                        .await
                    }
                })
                .await
        }
    }
}

fn global_lock_target(primary: ResourceLocator) -> LockTarget {
    LockTarget {
        primary,
        legacy: None,
        schema: LockSchema::Global,
    }
}

pub(crate) async fn read_raw_default_target_agents_with_io(
    io: EnvironmentLockIo,
    target: LockTarget,
) -> Result<Option<skill_lock::DefaultTargetAgents>, AppError> {
    let Some(bytes) = io.read_optional(&target.primary).await? else {
        return Ok(None);
    };
    let document = crate::core::lossless_lock::LosslessLockDocument::parse(&bytes)?;
    document
        .into_value()
        .get("defaultTargetAgents")
        .cloned()
        .map(serde_json::from_value)
        .transpose()
        .map_err(AppError::from)
}

async fn remove_default_target_agent_reference_with_io(
    io: EnvironmentLockIo,
    target: LockTarget,
    id: &AgentId,
    post_delete_registry: &AgentRegistrySnapshot,
) -> Result<(), AppError> {
    let repository = LockRepository::new(io);
    let Some(mut transaction) = repository
        .begin_if_present(
            target,
            LockMutationTargets {
                entries: Vec::new(),
                default_target_agents: true,
            },
        )
        .await?
    else {
        return Ok(());
    };
    let Some(raw_defaults) = transaction.initial_root("defaultTargetAgents").cloned() else {
        return Ok(());
    };
    let mut defaults: skill_lock::DefaultTargetAgents = serde_json::from_value(raw_defaults)?;
    defaults.global.retain(|candidate| candidate != id.as_str());
    defaults
        .project
        .retain(|candidate| candidate != id.as_str());
    let effective = skill_lock::effective_default_target_agents(&defaults, post_delete_registry);
    let last_selected_agents =
        skill_lock::builtin_last_selected_projection(&effective, post_delete_registry);

    transaction.set_default_target_agents(
        serde_json::to_value(defaults)?,
        serde_json::to_value(last_selected_agents)?,
    )?;
    transaction.commit().await
}

fn agent_registry_snapshot_for_defaults(registry: &ManagedAgentRegistry) -> AgentRegistrySnapshot {
    registry.snapshot().as_ref().clone()
}

fn host_default_target_agents_lock_target() -> LockTarget {
    LockTarget {
        primary: ResourceLocator {
            environment: EnvironmentRef::Host,
            native_path: crate::core::skill_lock::get_skill_lock_path()
                .to_string_lossy()
                .to_string(),
        },
        legacy: None,
        schema: LockSchema::Global,
    }
}

fn ensure_global_context(context: &ContextRef) -> Result<(), AppError> {
    if matches!(
        context.scope,
        crate::environment::types::ContextScope::Global
    ) {
        Ok(())
    } else {
        Err(AppError::Custom {
            message: "default Agent settings require global context".to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::agent_definition::{
        AgentAdapter, AgentDefinition, AgentId, AgentSource, DetectionSpec, PathSpec,
        ScopeDefinition,
    };
    use crate::core::agent_registry::AgentRegistrySnapshot;
    use std::collections::BTreeMap;

    fn scope(enabled: bool, reads_shared: bool, private_path: bool) -> ScopeDefinition {
        ScopeDefinition {
            enabled,
            reads_shared,
            private_path: private_path.then(|| PathSpec::home(".agent/skills")),
        }
    }

    fn definition(
        id: &str,
        source: AgentSource,
        global: ScopeDefinition,
        project: ScopeDefinition,
    ) -> AgentDefinition {
        AgentDefinition {
            id: AgentId::parse(id).unwrap(),
            display_name: id.to_string(),
            source,
            aliases: Vec::new(),
            global,
            project,
            detection: DetectionSpec::AnyPathExists {
                paths: vec![PathSpec::home(format!(".{id}"))],
            },
            legacy_paths: Vec::new(),
            adapter: AgentAdapter::Standard,
        }
    }

    fn registry_snapshot(definitions: Vec<AgentDefinition>) -> AgentRegistrySnapshot {
        AgentRegistrySnapshot {
            revision: "registry-revision".to_string(),
            active_definitions: definitions
                .into_iter()
                .map(|definition| (definition.id.clone(), definition))
                .collect::<BTreeMap<_, _>>(),
        }
    }

    fn lock_target(path: &std::path::Path) -> LockTarget {
        LockTarget {
            primary: ResourceLocator {
                environment: EnvironmentRef::Host,
                native_path: path.to_string_lossy().to_string(),
            },
            legacy: None,
            schema: LockSchema::Global,
        }
    }

    #[tokio::test]
    async fn effective_default_agents_read_does_not_write_the_lock() {
        let temp = tempfile::tempdir().expect("tempdir");
        let lock_path = temp.path().join("skill-lock.json");
        let original = br#"{
          "version": 3,
          "futureRoot": { "keep": true },
          "skills": {},
          "defaultTargetAgents": {
            "global": ["private-agent", "deleted-agent", "private-agent"],
            "project": []
          },
          "lastSelectedAgents": ["deleted-agent"]
        }"#;
        std::fs::write(&lock_path, original).expect("write lock");
        let snapshot = registry_snapshot(vec![definition(
            "private-agent",
            AgentSource::Builtin,
            scope(true, false, true),
            scope(true, true, false),
        )]);

        let effective = read_effective_default_target_agents(
            EnvironmentLockIo::Host,
            &lock_target(&lock_path),
            &snapshot,
        )
        .await
        .expect("read defaults");

        assert_eq!(
            effective,
            Some(skill_lock::DefaultTargetAgents {
                global: vec!["private-agent".to_string()],
                project: Vec::new(),
            })
        );
        assert_eq!(std::fs::read(lock_path).unwrap(), original);
    }

    #[tokio::test]
    async fn removing_deleted_default_target_preserves_unowned_lock_fields() {
        let temp = tempfile::tempdir().expect("tempdir");
        let lock_path = temp.path().join("skill-lock.json");
        std::fs::write(
            &lock_path,
            br#"{
              "version": 3,
              "futureRoot": { "keep": true },
              "skills": { "demo": { "future": 7 } },
              "defaultTargetAgents": {
                "global": ["codex", "deleted-agent"],
                "project": ["deleted-agent"]
              },
              "lastSelectedAgents": ["codex", "deleted-agent"]
            }"#,
        )
        .expect("write lock fixture");
        let snapshot = registry_snapshot(vec![definition(
            "codex",
            AgentSource::Builtin,
            scope(true, false, true),
            scope(true, false, true),
        )]);

        remove_default_target_agent_reference_with_io(
            EnvironmentLockIo::Host,
            lock_target(&lock_path),
            &AgentId::parse("deleted-agent").unwrap(),
            &snapshot,
        )
        .await
        .expect("cleanup deleted default");

        let value: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&lock_path).unwrap()).unwrap();
        assert_eq!(value["futureRoot"], serde_json::json!({ "keep": true }));
        assert_eq!(value["skills"]["demo"]["future"], 7);
        assert_eq!(
            value["defaultTargetAgents"]["global"],
            serde_json::json!(["codex"])
        );
        assert_eq!(
            value["defaultTargetAgents"]["project"],
            serde_json::json!([])
        );
        assert_eq!(value["lastSelectedAgents"], serde_json::json!(["codex"]));
    }

    #[tokio::test]
    async fn removing_deleted_default_target_keeps_an_absent_lock_absent() {
        let temp = tempfile::tempdir().expect("tempdir");
        let lock_path = temp.path().join("skill-lock.json");
        let snapshot = registry_snapshot(Vec::new());

        remove_default_target_agent_reference_with_io(
            EnvironmentLockIo::Host,
            lock_target(&lock_path),
            &AgentId::parse("deleted-agent").unwrap(),
            &snapshot,
        )
        .await
        .expect("absent lock is a no-op");

        assert!(!lock_path.exists());
    }

    #[test]
    fn default_agent_settings_reject_project_context() {
        let context = ContextRef {
            environment: EnvironmentRef::Host,
            scope: crate::environment::types::ContextScope::Project {
                project_id: "app".to_string(),
            },
        };

        let error = ensure_global_context(&context).unwrap_err();

        assert!(matches!(error, AppError::Custom { .. }));
    }
}

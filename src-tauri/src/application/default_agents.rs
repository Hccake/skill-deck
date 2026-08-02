use crate::application::agents::{AgentCommandError, ManagedAgentRegistry};
use crate::application::runtime_admission::{MutationPermit, RuntimeAdmissionCoordinator};
use crate::application::runtime_facts::AgentRegistrySnapshotSource;
use crate::core::agent_definition::AgentId;
use crate::core::agent_registry::AgentRegistrySnapshot;
use crate::core::lock_repository::{LockMutationTargets, LockRepository, LockTarget};
use crate::core::lossless_lock::LockSchema;
use crate::core::mutation::{MutationKind, MutationPhase};
use crate::core::skill_lock;
use crate::environment::context_resolver::ContextResolver;
use crate::environment::lock_io::EnvironmentLockIo;
use crate::environment::types::{ContextRef, ContextScope, EnvironmentRef, ResourceLocator};
use crate::environment::wsl::EnvironmentRegistry;
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
    registry: &EnvironmentRegistry,
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
                            EnvironmentLockIo::Wsl(session),
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

fn prepare_default_target_agents_save(
    supplied: &skill_lock::DefaultTargetAgents,
    expected_registry_revision: &str,
    snapshot: &AgentRegistrySnapshot,
) -> Result<(skill_lock::DefaultTargetAgents, Vec<String>), AgentCommandError> {
    validate_default_target_agents_revision(expected_registry_revision, snapshot)?;
    let effective = skill_lock::effective_default_target_agents(supplied, snapshot);
    let last_selected = skill_lock::builtin_last_selected_projection(&effective, snapshot);
    Ok((effective, last_selected))
}

fn validate_default_target_agents_revision(
    expected_registry_revision: &str,
    snapshot: &AgentRegistrySnapshot,
) -> Result<(), AgentCommandError> {
    if expected_registry_revision != snapshot.revision {
        return Err(AgentCommandError::StaleRegistryRevision {
            expected: expected_registry_revision.to_string(),
            actual: snapshot.revision.clone(),
        });
    }
    Ok(())
}

fn begin_default_target_agents_save(
    controller: &RuntimeAdmissionCoordinator,
    context: ContextRef,
    expected_registry_revision: &str,
    initial_snapshot: &AgentRegistrySnapshot,
) -> Result<MutationPermit, AgentCommandError> {
    validate_default_target_agents_revision(expected_registry_revision, initial_snapshot)?;
    controller
        .begin_mutation(MutationKind::SaveAgentDefaults, context)
        .map_err(AgentCommandError::from)
}

pub(crate) async fn commit_default_target_agents(
    io: EnvironmentLockIo,
    target: LockTarget,
    defaults: skill_lock::DefaultTargetAgents,
    last_selected_agents: Vec<String>,
) -> Result<(), AppError> {
    let repository = LockRepository::new(io);
    let mut transaction = repository
        .begin(
            target,
            LockMutationTargets {
                entries: Vec::new(),
                default_target_agents: true,
            },
        )
        .await?;
    transaction.set_default_target_agents(
        serde_json::to_value(defaults)?,
        serde_json::to_value(last_selected_agents)?,
    )?;
    transaction.commit().await
}

pub(crate) async fn remove_default_target_agent_reference(
    context: ContextRef,
    id: &AgentId,
    environment_registry: &EnvironmentRegistry,
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
                            EnvironmentLockIo::Wsl(session),
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
    environment_registry: &EnvironmentRegistry,
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
                            EnvironmentLockIo::Wsl(session),
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

pub async fn save_default_target_agents(
    context: ContextRef,
    defaults: skill_lock::DefaultTargetAgents,
    expected_registry_revision: String,
    registry: &EnvironmentRegistry,
    agent_registry: &ManagedAgentRegistry,
    controller: &RuntimeAdmissionCoordinator,
) -> Result<(), AgentCommandError> {
    ensure_global_context(&context)?;
    let initial_snapshot = agent_registry_snapshot_for_defaults(agent_registry);
    let guard = begin_default_target_agents_save(
        controller,
        context.clone(),
        &expected_registry_revision,
        &initial_snapshot,
    )?;
    let guarded_snapshot = agent_registry_snapshot_for_defaults(agent_registry);
    let (defaults, last_selected_agents) = prepare_default_target_agents_save(
        &defaults,
        &expected_registry_revision,
        &guarded_snapshot,
    )?;
    match &context.environment {
        EnvironmentRef::Host => {
            ContextResolver::resolve_host(context)?;
            guard.transition(MutationPhase::Committing, None, false);
            commit_default_target_agents(
                EnvironmentLockIo::Host,
                host_default_target_agents_lock_target(),
                defaults,
                last_selected_agents,
            )
            .await
            .map_err(AgentCommandError::from)
        }
        EnvironmentRef::Wsl { distro_name } => {
            let distro_name = distro_name.clone();
            let retry_context = context.clone();
            let guard = &guard;
            registry
                .with_session_retry(&distro_name, move |session| {
                    let context = retry_context.clone();
                    let defaults = defaults.clone();
                    let last_selected_agents = last_selected_agents.clone();
                    async move {
                        let locator = ContextResolver::resolve_wsl(context, &session).await?.lock;
                        guard.transition(MutationPhase::Committing, None, false);
                        commit_default_target_agents(
                            EnvironmentLockIo::Wsl(session),
                            LockTarget {
                                primary: locator,
                                legacy: None,
                                schema: LockSchema::Global,
                            },
                            defaults,
                            last_selected_agents,
                        )
                        .await
                    }
                })
                .await
                .map_err(AgentCommandError::from)
        }
    }
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

#[cfg(test)]
async fn save_effective_default_target_agents(
    io: EnvironmentLockIo,
    target: LockTarget,
    supplied: skill_lock::DefaultTargetAgents,
    expected_registry_revision: &str,
    snapshot: &AgentRegistrySnapshot,
) -> Result<(), AgentCommandError> {
    let (effective, last_selected_agents) =
        prepare_default_target_agents_save(&supplied, expected_registry_revision, snapshot)?;
    commit_default_target_agents(io, target, effective, last_selected_agents)
        .await
        .map_err(AgentCommandError::from)
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
    use crate::application::agents::AgentCommandError;
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
    async fn environment_default_agents_update_preserves_unknown_fields_and_projects_builtins() {
        let temp = tempfile::tempdir().expect("tempdir");
        let lock_path = temp.path().join("skill-lock.json");
        let current = br#"{
          "version": 3,
          "customRoot": { "keep": true },
          "skills": { "demo": { "source": "owner/repo", "future": 7 } },
          "defaultTargetAgents": { "global": ["codex"], "project": [] },
          "lastSelectedAgents": ["codex"]
        }"#;
        std::fs::write(&lock_path, current).expect("write lock");
        let private = scope(true, false, true);
        let snapshot = registry_snapshot(vec![
            definition(
                "claude-code",
                AgentSource::Builtin,
                private.clone(),
                private.clone(),
            ),
            definition(
                "my-custom-agent",
                AgentSource::Custom,
                private.clone(),
                private,
            ),
        ]);
        let supplied = skill_lock::DefaultTargetAgents {
            global: vec![
                "claude-code".to_string(),
                "my-custom-agent".to_string(),
                "deleted-agent".to_string(),
            ],
            project: vec!["my-custom-agent".to_string(), "claude-code".to_string()],
        };

        save_effective_default_target_agents(
            EnvironmentLockIo::Host,
            lock_target(&lock_path),
            supplied,
            "registry-revision",
            &snapshot,
        )
        .await
        .expect("update defaults losslessly");
        let value: serde_json::Value =
            serde_json::from_slice(&std::fs::read(lock_path).unwrap()).unwrap();

        assert_eq!(value["customRoot"]["keep"], true);
        assert_eq!(value["skills"]["demo"]["future"], 7);
        assert_eq!(value["defaultTargetAgents"]["global"][0], "claude-code");
        assert_eq!(value["defaultTargetAgents"]["global"][1], "my-custom-agent");
        assert_eq!(
            value["defaultTargetAgents"]["project"][0],
            "my-custom-agent"
        );
        assert_eq!(
            value["lastSelectedAgents"],
            serde_json::json!(["claude-code"])
        );
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

    #[tokio::test]
    async fn stale_registry_revision_rejects_save_without_writing() {
        let temp = tempfile::tempdir().expect("tempdir");
        let lock_path = temp.path().join("skill-lock.json");
        let original = br#"{"version":3,"skills":{},"futureRoot":{"keep":true}}"#;
        std::fs::write(&lock_path, original).expect("write lock");
        let snapshot = registry_snapshot(vec![definition(
            "private-agent",
            AgentSource::Builtin,
            scope(true, false, true),
            scope(true, false, true),
        )]);

        let error = save_effective_default_target_agents(
            EnvironmentLockIo::Host,
            lock_target(&lock_path),
            skill_lock::DefaultTargetAgents {
                global: vec!["private-agent".to_string()],
                project: Vec::new(),
            },
            "stale-revision",
            &snapshot,
        )
        .await
        .expect_err("reject stale revision");

        assert_eq!(
            error,
            AgentCommandError::StaleRegistryRevision {
                expected: "stale-revision".to_string(),
                actual: "registry-revision".to_string(),
            }
        );
        assert_eq!(std::fs::read(lock_path).unwrap(), original);
    }

    #[test]
    fn guarded_recheck_rejects_registry_advance_without_writing() {
        let temp = tempfile::tempdir().expect("tempdir");
        let lock_path = temp.path().join("skill-lock.json");
        let original = br#"{"version":3,"skills":{},"futureRoot":{"keep":true}}"#;
        std::fs::write(&lock_path, original).expect("write lock");
        let supplied = skill_lock::DefaultTargetAgents {
            global: vec!["private-agent".to_string()],
            project: Vec::new(),
        };
        let r1 = registry_snapshot(vec![definition(
            "private-agent",
            AgentSource::Custom,
            scope(true, false, true),
            scope(true, false, true),
        )]);
        let mut r2 = r1.clone();
        r2.revision = "registry-revision-r2".to_string();
        r2.active_definitions.clear();
        let controller = RuntimeAdmissionCoordinator::default();
        let context = ContextRef {
            environment: EnvironmentRef::Host,
            scope: crate::environment::types::ContextScope::Global,
        };

        let guard =
            begin_default_target_agents_save(&controller, context, "registry-revision", &r1)
                .expect("R1 is initially current");
        let error = prepare_default_target_agents_save(&supplied, "registry-revision", &r2)
            .expect_err("guarded recheck rejects R2");

        assert_eq!(
            error,
            AgentCommandError::StaleRegistryRevision {
                expected: "registry-revision".to_string(),
                actual: "registry-revision-r2".to_string(),
            }
        );
        assert_eq!(std::fs::read(lock_path).unwrap(), original);
        drop(guard);
        assert!(controller.active().is_none());
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

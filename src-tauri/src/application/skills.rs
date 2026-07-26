// list_skills command

use std::sync::Arc;

use crate::application::agents::{AgentCommandError, ManagedAgentRegistry};
use crate::application::skill_read::{
    build_skill_read_plan, discover_eve_skill_targets, project_skill_snapshot, ListSkillsResult,
};
use crate::core::local_lock::LocalSkillLockEntry;
use crate::core::skill::InstalledSkill;
use crate::core::skill_lock::SkillLockEntry;
use crate::environment::context_resolver::{ContextResolver, ResolvedContext};
use crate::environment::lock_io::EnvironmentLockIo;
use crate::environment::native::inspection::NativeInspector;
use crate::environment::read_service::ReadService;
use crate::environment::types::{ContextRef, EnvironmentRef, ResourceLocator};
use crate::environment::wsl::operations::inspection::WslInspector;
use crate::environment::wsl::EnvironmentRegistry;
use crate::error::AppError;

fn agent_command_error(error: AgentCommandError) -> AppError {
    match error {
        AgentCommandError::Application { error } => error,
        error => AppError::Custom {
            message: format!("agent runtime unavailable: {error:?}"),
        },
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LockKind {
    Global,
    Project,
    LegacyProject,
}

fn enrich_environment_skills_from_lock(
    mut skills: Vec<InstalledSkill>,
    bytes: Option<&[u8]>,
    kind: LockKind,
) -> Vec<InstalledSkill> {
    let Some(bytes) = bytes else {
        return skills;
    };
    let Ok(root) = serde_json::from_slice::<serde_json::Value>(bytes) else {
        return skills;
    };
    let minimum_version = match kind {
        LockKind::Global => 3,
        LockKind::Project => 1,
        LockKind::LegacyProject => 0,
    };
    let Some(version) = root.get("version").and_then(serde_json::Value::as_u64) else {
        return skills;
    };
    if version < minimum_version {
        return skills;
    }
    let Some(entries) = root.get("skills").and_then(serde_json::Value::as_object) else {
        return skills;
    };

    for skill in &mut skills {
        let Some(value) = entries.get(&skill.name).cloned() else {
            continue;
        };
        let enriched =
            match kind {
                LockKind::Global => serde_json::from_value::<SkillLockEntry>(value)
                    .ok()
                    .map(|entry| skill.clone().with_lock_entry(Some(&entry))),
                LockKind::Project => serde_json::from_value::<LocalSkillLockEntry>(value)
                    .ok()
                    .map(|entry| skill.clone().with_local_lock_entry(Some(&entry))),
                LockKind::LegacyProject => serde_json::from_value::<SkillLockEntry>(value)
                    .ok()
                    .map(|entry| {
                        let local = LocalSkillLockEntry {
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
                        };
                        skill.clone().with_local_lock_entry(Some(&local))
                    }),
            };
        if let Some(enriched) = enriched {
            *skill = enriched;
        }
    }
    skills
}

#[cfg(test)]
#[expect(
    clippy::items_after_test_module,
    reason = "lock enrichment tests stay adjacent to their private projection helper"
)]
mod environment_tests {
    use super::{enrich_environment_skills_from_lock, LockKind};
    use crate::core::agent_definition::AgentId;
    use crate::core::skill::{InstalledSkill, SkillScope};

    fn installed_skill(
        name: &str,
        description: &str,
        canonical_path: &str,
        scope: SkillScope,
        agents: Vec<AgentId>,
    ) -> InstalledSkill {
        InstalledSkill {
            name: name.to_string(),
            description: description.to_string(),
            path: canonical_path.to_string(),
            canonical_path: canonical_path.to_string(),
            scope,
            associated_agents: agents.clone(),
            default_available_agent_count: Some(agents.len() as u32),
            private_adapted_agent_count: Some(0),
            duplicate_copy_count: Some(0),
            default_available_agents: Some(agents.clone()),
            private_adapted_agents: Some(Vec::new()),
            duplicate_copy_agents: Some(Vec::new()),
            private_only_agents: Some(Vec::new()),
            private_copy_agents: Some(Vec::new()),
            agents,
            source: None,
            source_url: None,
            installed_at: None,
            updated_at: None,
            has_update: None,
            can_run_update: None,
            can_check_for_updates: None,
            update_reason: None,
            plugin_name: None,
            git_ref: None,
        }
    }

    #[test]
    fn converts_environment_snapshot_to_project_skill_without_inventing_metadata() {
        let skill = installed_skill(
            "toolkit",
            "",
            "/work/app/.agents/skills/toolkit",
            SkillScope::Project,
            Vec::new(),
        );

        assert_eq!(skill.name, "toolkit");
        assert_eq!(skill.scope, SkillScope::Project);
        assert!(skill.agents.is_empty());
        assert_eq!(skill.source, None);
    }

    #[test]
    fn preserves_environment_presence_summary_and_global_lock_metadata() {
        let skill = installed_skill(
            "toolkit",
            "Shared toolkit",
            "/home/alice/.agents/skills/toolkit",
            SkillScope::Global,
            vec![AgentId::parse("codex").unwrap()],
        );
        let lock = br#"{
          "version": 3,
          "futureRoot": true,
          "skills": {
            "toolkit": {
              "source": "owner/repo",
              "sourceType": "github",
              "sourceUrl": "https://github.com/owner/repo",
              "skillPath": "skills/toolkit/SKILL.md",
              "skillFolderHash": "abc",
              "installedAt": "2026-07-01T00:00:00Z",
              "updatedAt": "2026-07-02T00:00:00Z",
              "futureEntry": 42
            }
          }
        }"#;

        let skill = enrich_environment_skills_from_lock(vec![skill], Some(lock), LockKind::Global)
            .pop()
            .expect("enriched skill");

        assert_eq!(skill.description, "Shared toolkit");
        assert_eq!(
            skill.associated_agents,
            vec![AgentId::parse("codex").unwrap()]
        );
        assert_eq!(skill.source.as_deref(), Some("owner/repo"));
        assert_eq!(
            skill.source_url.as_deref(),
            Some("https://github.com/owner/repo")
        );
        assert_eq!(skill.installed_at.as_deref(), Some("2026-07-01T00:00:00Z"));
        assert_eq!(skill.can_run_update, Some(true));
        assert_eq!(skill.agents, vec![AgentId::parse("codex").unwrap()]);
    }

    #[test]
    fn ignores_project_lock_with_unsupported_version() {
        let skill = installed_skill(
            "toolkit",
            "Toolkit",
            "/work/app/.agents/skills/toolkit",
            SkillScope::Project,
            Vec::new(),
        );
        let lock = br#"{
          "version": 0,
          "skills": {
            "toolkit": {
              "source": "owner/repo",
              "sourceType": "github",
              "computedHash": "abc"
            }
          }
        }"#;

        let skill = enrich_environment_skills_from_lock(vec![skill], Some(lock), LockKind::Project)
            .pop()
            .expect("skill");

        assert_eq!(skill.source, None);
    }
}

pub async fn list_skills(
    context: ContextRef,
    environment_registry: &EnvironmentRegistry,
    agent_registry: &ManagedAgentRegistry,
) -> Result<ListSkillsResult, AppError> {
    let runtime = crate::application::agents::list_agents(
        context.clone(),
        environment_registry,
        agent_registry,
    )
    .await
    .map_err(agent_command_error)?;
    match &context.environment {
        EnvironmentRef::Host => {
            let resolved = ContextResolver::resolve_host(context)?;
            let eve_targets = discover_eve_skill_targets(&resolved, &runtime, None).await?;
            let plan = build_skill_read_plan(&resolved, &runtime, &eve_targets)?;
            let read_service =
                ReadService::new(vec![Arc::new(NativeInspector::new(EnvironmentRef::Host))]);
            let snapshot = read_service.execute(&plan.read_plan).await?;
            let result = project_skill_snapshot(&plan, snapshot, &runtime)?;
            enrich_from_context_lock(result, &resolved, EnvironmentLockIo::Host).await
        }
        EnvironmentRef::Wsl { distro_name } => {
            let distro_name = distro_name.clone();
            let retry_context = context.clone();
            environment_registry
                .with_session_retry(&distro_name, move |session| {
                    let context = retry_context.clone();
                    let runtime = runtime.clone();
                    async move {
                        let resolved = ContextResolver::resolve_wsl(context, &session).await?;
                        let eve_targets =
                            discover_eve_skill_targets(&resolved, &runtime, Some(&session)).await?;
                        let plan = build_skill_read_plan(&resolved, &runtime, &eve_targets)?;
                        let read_service =
                            ReadService::new(vec![Arc::new(WslInspector::new(session.clone()))]);
                        let snapshot = read_service.execute(&plan.read_plan).await?;
                        let result = project_skill_snapshot(&plan, snapshot, &runtime)?;
                        enrich_from_context_lock(result, &resolved, EnvironmentLockIo::Wsl(session))
                            .await
                    }
                })
                .await
        }
    }
}

async fn enrich_from_context_lock(
    mut result: ListSkillsResult,
    context: &ResolvedContext,
    lock_io: EnvironmentLockIo,
) -> Result<ListSkillsResult, AppError> {
    let mut lock_bytes = lock_io.read_optional(&context.lock).await.ok().flatten();
    let mut lock_kind = if context.project.is_some() {
        LockKind::Project
    } else {
        LockKind::Global
    };
    if lock_bytes.is_none() {
        if let Some(project) = &context.project {
            let legacy_locator = ResourceLocator {
                environment: context.context.environment.clone(),
                native_path: format!(
                    "{}/.agents/.skill-lock.json",
                    project.native_path.trim_end_matches('/')
                ),
            };
            lock_bytes = lock_io.read_optional(&legacy_locator).await.ok().flatten();
            if lock_bytes.is_some() {
                lock_kind = LockKind::LegacyProject;
            }
        }
    }
    result.skills =
        enrich_environment_skills_from_lock(result.skills, lock_bytes.as_deref(), lock_kind);
    Ok(result)
}

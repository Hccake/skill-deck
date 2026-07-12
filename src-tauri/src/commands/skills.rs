// list_skills command

use serde::Deserialize;
use specta::Type;

use crate::core::local_lock::LocalSkillLockEntry;
use crate::core::skill::{list_installed_skills, InstalledSkill, ListSkillsResult, SkillScope};
use crate::core::skill_lock::SkillLockEntry;
use crate::environment::lock_io::EnvironmentLockIo;
use crate::environment::service::SkillEntrySnapshot;
use crate::environment::service::{EnvironmentService, InspectRequest, ResolvedContext};
use crate::environment::types::{ContextRef, ContextScope, EnvironmentRef, ResourceLocator};
use crate::environment::wsl::EnvironmentRegistry;
use crate::error::AppError;
use tauri::State;

fn installed_skill_from_snapshot(
    snapshot: SkillEntrySnapshot,
    scope: SkillScope,
) -> InstalledSkill {
    let default_available_agent_count = snapshot.default_available_agents.len() as u32;
    let private_adapted_agent_count = snapshot.private_adapted_agents.len() as u32;
    let duplicate_copy_count = snapshot.duplicate_copy_agents.len() as u32;
    InstalledSkill {
        name: snapshot.name,
        description: snapshot.description,
        path: snapshot.canonical_path.clone(),
        canonical_path: snapshot.canonical_path,
        scope,
        agents: snapshot.agents,
        card_agents: Some(snapshot.card_agents),
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
        default_available_agent_count: Some(default_available_agent_count),
        private_adapted_agent_count: Some(private_adapted_agent_count),
        duplicate_copy_count: Some(duplicate_copy_count),
        default_available_agents: Some(snapshot.default_available_agents),
        private_adapted_agents: Some(snapshot.private_adapted_agents),
        duplicate_copy_agents: Some(snapshot.duplicate_copy_agents),
        private_only_agents: Some(snapshot.private_only_agents),
        private_copy_agents: Some(snapshot.private_copy_agents),
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
mod environment_tests {
    use super::{enrich_environment_skills_from_lock, installed_skill_from_snapshot, LockKind};
    use crate::core::agents::AgentType;
    use crate::core::skill::SkillScope;
    use crate::environment::service::SkillEntrySnapshot;

    #[test]
    fn converts_environment_snapshot_to_project_skill_without_inventing_metadata() {
        let skill = installed_skill_from_snapshot(
            SkillEntrySnapshot {
                name: "toolkit".to_string(),
                description: String::new(),
                canonical_path: "/work/app/.agents/skills/toolkit".to_string(),
                canonical_present: true,
                agents: Vec::new(),
                card_agents: Vec::new(),
                default_available_agents: Vec::new(),
                private_adapted_agents: Vec::new(),
                duplicate_copy_agents: Vec::new(),
                private_only_agents: Vec::new(),
                private_copy_agents: Vec::new(),
                eve_targets: Vec::new(),
            },
            SkillScope::Project,
        );

        assert_eq!(skill.name, "toolkit");
        assert_eq!(skill.scope, SkillScope::Project);
        assert!(skill.agents.is_empty());
        assert_eq!(skill.source, None);
    }

    #[test]
    fn preserves_environment_presence_summary_and_global_lock_metadata() {
        let skill = installed_skill_from_snapshot(
            SkillEntrySnapshot {
                name: "toolkit".to_string(),
                description: "Shared toolkit".to_string(),
                canonical_path: "/home/alice/.agents/skills/toolkit".to_string(),
                canonical_present: true,
                agents: vec![AgentType::Codex],
                card_agents: vec![AgentType::Codex],
                default_available_agents: vec![AgentType::Codex],
                private_adapted_agents: Vec::new(),
                duplicate_copy_agents: Vec::new(),
                private_only_agents: Vec::new(),
                private_copy_agents: Vec::new(),
                eve_targets: Vec::new(),
            },
            SkillScope::Global,
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
        assert_eq!(skill.card_agents, Some(vec![AgentType::Codex]));
        assert_eq!(skill.source.as_deref(), Some("owner/repo"));
        assert_eq!(
            skill.source_url.as_deref(),
            Some("https://github.com/owner/repo")
        );
        assert_eq!(skill.installed_at.as_deref(), Some("2026-07-01T00:00:00Z"));
        assert_eq!(skill.can_run_update, Some(true));
    }

    #[test]
    fn ignores_project_lock_with_unsupported_version() {
        let skill = installed_skill_from_snapshot(
            SkillEntrySnapshot {
                name: "toolkit".to_string(),
                description: "Toolkit".to_string(),
                canonical_path: "/work/app/.agents/skills/toolkit".to_string(),
                canonical_present: true,
                agents: Vec::new(),
                card_agents: Vec::new(),
                default_available_agents: Vec::new(),
                private_adapted_agents: Vec::new(),
                duplicate_copy_agents: Vec::new(),
                private_only_agents: Vec::new(),
                private_copy_agents: Vec::new(),
                eve_targets: Vec::new(),
            },
            SkillScope::Project,
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

/// list_skills 参数
#[derive(Debug, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct ListSkillsParams {
    /// 范围: "global" | "project" | null (返回全部)
    pub scope: Option<String>,
    /// 项目路径（用于 project scope）
    pub project_path: Option<String>,
}

/// 列出已安装的 skills
/// 对应前端调用: invoke('list_skills', { params })
#[tauri::command]
#[specta::specta]
pub fn list_skills(params: ListSkillsParams) -> Result<ListSkillsResult, AppError> {
    let scope = match params.scope.as_deref() {
        Some("global") => Some(SkillScope::Global),
        Some("project") => Some(SkillScope::Project),
        _ => None,
    };

    let cwd = params.project_path.unwrap_or_else(|| {
        std::env::current_dir()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| ".".to_string())
    });

    // 检查路径存在性
    let path_exists = match scope {
        Some(SkillScope::Project) => std::path::Path::new(&cwd).is_dir(),
        _ => true, // global 始终为 true
    };

    let skills = if path_exists {
        list_installed_skills(scope, &cwd)?
    } else {
        Vec::new()
    };

    Ok(ListSkillsResult {
        skills,
        path_exists,
    })
}

#[tauri::command]
#[specta::specta]
pub async fn list_skills_v2(
    context: ContextRef,
    registry: State<'_, EnvironmentRegistry>,
) -> Result<ListSkillsResult, AppError> {
    match &context.environment {
        EnvironmentRef::Host => list_host_skills_v2(&context),
        EnvironmentRef::Wsl { distro_name } => {
            let session = registry.get(distro_name).ok_or_else(|| AppError::Custom {
                message: format!("WSL distro '{distro_name}' is not connected"),
            })?;
            let (project, scope, skill_root) = match &context.scope {
                ContextScope::Global => (
                    None,
                    SkillScope::Global,
                    format!("{}/.agents/skills", session.home.trim_end_matches('/')),
                ),
                ContextScope::Project { project_id } => {
                    let project = crate::commands::environments::read_wsl_projects(&session)
                        .await?
                        .into_iter()
                        .find(|project| &project.id == project_id)
                        .ok_or_else(|| AppError::PathNotFound {
                            path: project_id.clone(),
                        })?;
                    let root = format!(
                        "{}/.agents/skills",
                        project.native_path.trim_end_matches('/')
                    );
                    (Some(project), SkillScope::Project, root)
                }
            };
            let environment = context.environment.clone();
            let lock_path = match &project {
                Some(project) => format!(
                    "{}/skills-lock.json",
                    project.native_path.trim_end_matches('/')
                ),
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
            let lock_locator = ResourceLocator {
                environment: environment.clone(),
                native_path: lock_path,
            };
            let snapshot = EnvironmentService::Wsl(session.clone())
                .inspect(&InspectRequest {
                    context: ResolvedContext {
                        context,
                        project: project.clone(),
                        home: ResourceLocator {
                            environment: environment.clone(),
                            native_path: session.home.clone(),
                        },
                        skill_root: ResourceLocator {
                            environment: environment.clone(),
                            native_path: skill_root,
                        },
                        lock: ResourceLocator {
                            environment: environment.clone(),
                            native_path: lock_locator.native_path.clone(),
                        },
                    },
                })
                .await?;
            let lock_io = EnvironmentLockIo::Wsl(session.clone());
            let mut lock_bytes = lock_io.read_optional(&lock_locator).await.ok().flatten();
            let mut lock_kind = if project.is_some() {
                LockKind::Project
            } else {
                LockKind::Global
            };
            if lock_bytes.is_none() {
                if let Some(project) = &project {
                    let legacy_locator = ResourceLocator {
                        environment,
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
            let skills = snapshot
                .skills
                .into_iter()
                .map(|skill| installed_skill_from_snapshot(skill, scope))
                .collect();
            Ok(ListSkillsResult {
                path_exists: snapshot.path_exists,
                skills: enrich_environment_skills_from_lock(
                    skills,
                    lock_bytes.as_deref(),
                    lock_kind,
                ),
            })
        }
    }
}

fn list_host_skills_v2(context: &ContextRef) -> Result<ListSkillsResult, AppError> {
    match &context.scope {
        ContextScope::Global => Ok(ListSkillsResult {
            skills: list_installed_skills(Some(SkillScope::Global), ".")?,
            path_exists: true,
        }),
        ContextScope::Project { project_id } => {
            let project = crate::commands::environments::host_projects_store()?
                .read()?
                .into_iter()
                .find(|project| &project.id == project_id)
                .ok_or_else(|| AppError::PathNotFound {
                    path: project_id.clone(),
                })?;
            let path_exists = std::path::Path::new(&project.native_path).is_dir();
            let skills = if path_exists {
                list_installed_skills(Some(SkillScope::Project), &project.native_path)?
            } else {
                Vec::new()
            };
            Ok(ListSkillsResult {
                skills,
                path_exists,
            })
        }
    }
}

use crate::core::skill::read_skill_content as core_read_skill_content;

/// Read the markdown body of a skill's SKILL.md
#[tauri::command]
#[specta::specta]
pub fn read_skill_content(canonical_path: String) -> Result<String, AppError> {
    core_read_skill_content(&canonical_path)
}

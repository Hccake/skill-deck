use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use serde::Serialize;
use sha2::{Digest, Sha256};
use specta::Type;

use crate::core::agent_availability::{
    availability_for_resolved_scope, resolved_agent_presence_from_paths, AgentAvailabilityKind,
};
use crate::core::agent_definition::{AgentAdapter, AgentId};
use crate::core::skill::{InstalledSkill, SkillFrontmatter, SkillScope};
use crate::environment::agent_environment::{AgentRuntimeSnapshot, DetectionState, ResolvedAgent};
use crate::environment::context_resolver::ResolvedContext;
use crate::environment::inspection::{
    FilesystemEntryKind, RawFilesystemSnapshot, ReadPlan, ReadPlanBuilder, ReadRootPurpose,
};
use crate::environment::runtime::ContextSnapshotRevision;
use crate::environment::types::{
    same_environment_identity, ContextScope, EnvironmentRef, ResourceLocator,
};
use crate::environment::wsl::operations::eve::inspect_eve_project;
use crate::environment::wsl::WslSession;
use crate::error::AppError;
use crate::models::{AgentSkillPresence, SkillInstallTargetInfo};

#[derive(Debug, Clone, PartialEq, Eq)]
enum SkillReadOwner {
    Canonical,
    Agent(AgentId),
    Eve(SkillInstallTargetInfo),
}

#[derive(Debug, Clone)]
pub struct SkillReadPlan {
    pub read_plan: ReadPlan,
    context_root: String,
    owners: BTreeMap<String, Vec<SkillReadOwner>>,
}

/// `list_skills` 的运行时读取结果。
/// Skill 与 scope Agents 来自同一次 Agent runtime snapshot，避免 Frontend 拼接不同 revision。
#[derive(Debug, Serialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct ListSkillsResult {
    pub skills: Vec<InstalledSkill>,
    pub agents: Vec<ResolvedAgent>,
    /// 项目目录是否存在（project scope 时有意义，global 始终为 true）
    pub path_exists: bool,
}

#[derive(Debug)]
struct SkillCandidate {
    description: String,
    canonical_path: String,
    canonical_present: bool,
    canonical_is_symlink: bool,
    private_agents: BTreeSet<AgentId>,
    private_symlink_agents: BTreeSet<AgentId>,
    eve_targets: Vec<SkillInstallTargetInfo>,
}

pub async fn discover_eve_skill_targets(
    context: &ResolvedContext,
    runtime: &AgentRuntimeSnapshot,
    wsl_session: Option<&WslSession>,
) -> Result<Vec<SkillInstallTargetInfo>, AppError> {
    let Some(project) = context.project.as_ref() else {
        return Ok(Vec::new());
    };
    let Some((eve_id, eve)) = runtime
        .agents
        .iter()
        .find(|(_, agent)| agent.definition.adapter == AgentAdapter::Eve)
    else {
        return Ok(Vec::new());
    };
    match (&context.context.environment, wsl_session) {
        (EnvironmentRef::Host, None) => Ok(crate::core::eve::eve_install_targets_for_project(
            &project.native_path,
        )
        .into_iter()
        .map(|target| SkillInstallTargetInfo {
            target_id: target.target_id,
            agent: target.agent,
            display_name: target.display_name,
            subagent: target.subagent,
            path: target.path,
        })
        .collect()),
        (EnvironmentRef::Wsl { .. }, Some(session)) => {
            let inspected = inspect_eve_project(session, &project.native_path).await?;
            if !inspected.has_eve {
                return Ok(Vec::new());
            }
            let project_root = project.native_path.trim_end_matches('/');
            let mut targets = vec![SkillInstallTargetInfo {
                target_id: format!("{}:root", eve_id),
                agent: eve_id.clone(),
                display_name: eve.definition.display_name.clone(),
                subagent: None,
                path: format!("{project_root}/agent/skills"),
            }];
            targets.extend(inspected.subagents.into_iter().map(|subagent| {
                let path_name = crate::core::skill::sanitize_name(&subagent);
                SkillInstallTargetInfo {
                    target_id: format!("{}:{path_name}", eve_id),
                    agent: eve_id.clone(),
                    display_name: format!("{} ({subagent})", eve.definition.display_name),
                    subagent: Some(subagent),
                    path: format!("{project_root}/agent/subagents/{path_name}/skills"),
                }
            }));
            Ok(targets)
        }
        _ => Err(AppError::EnvironmentUnavailable {
            environment: context.context.environment.clone(),
            message: "Skill read inspector does not match the selected Environment".to_string(),
        }),
    }
}

pub fn build_skill_read_plan(
    context: &ResolvedContext,
    runtime: &AgentRuntimeSnapshot,
    eve_targets: &[SkillInstallTargetInfo],
) -> Result<SkillReadPlan, AppError> {
    let context_revision = read_context_revision(context, runtime)?;
    let mut builder = ReadPlanBuilder::new(
        context.context.clone(),
        runtime.registry_revision.clone(),
        runtime.environment_revision.clone(),
        context_revision,
    );
    let context_root = context.context_root().to_string();
    builder.add_root(
        locator(context, &context_root),
        ReadRootPurpose::Context,
        None,
    )?;

    let mut owners = BTreeMap::<String, Vec<SkillReadOwner>>::new();
    add_owned_root(
        &mut builder,
        context,
        &mut owners,
        &context.skill_root.native_path,
        ReadRootPurpose::Canonical,
        SkillReadOwner::Canonical,
        None,
    )?;
    let is_global = matches!(context.context.scope, ContextScope::Global);
    for (agent_id, resolved) in &runtime.agents {
        let scope = if is_global {
            &resolved.global
        } else {
            &resolved.project
        };
        if !scope.enabled {
            continue;
        }
        let Some(private_root) = scope.private_path.as_deref() else {
            continue;
        };
        if resolved.definition.adapter == AgentAdapter::Eve {
            let root_target = SkillInstallTargetInfo {
                target_id: format!("{}:root", agent_id),
                agent: agent_id.clone(),
                display_name: resolved.definition.display_name.clone(),
                subagent: None,
                path: private_root.to_string(),
            };
            add_owned_root(
                &mut builder,
                context,
                &mut owners,
                private_root,
                ReadRootPurpose::Adapter,
                SkillReadOwner::Eve(root_target),
                Some(agent_id.clone()),
            )?;
        } else {
            add_owned_root(
                &mut builder,
                context,
                &mut owners,
                private_root,
                ReadRootPurpose::Private,
                SkillReadOwner::Agent(agent_id.clone()),
                Some(agent_id.clone()),
            )?;
        }
    }
    for target in eve_targets {
        add_owned_root(
            &mut builder,
            context,
            &mut owners,
            &target.path,
            ReadRootPurpose::Adapter,
            SkillReadOwner::Eve(target.clone()),
            Some(target.agent.clone()),
        )?;
    }

    Ok(SkillReadPlan {
        read_plan: builder.build()?,
        context_root,
        owners,
    })
}

pub fn project_skill_snapshot(
    plan: &SkillReadPlan,
    snapshot: RawFilesystemSnapshot,
    runtime: &AgentRuntimeSnapshot,
) -> Result<ListSkillsResult, AppError> {
    if !same_environment_identity(&snapshot.environment, &plan.read_plan.context.environment) {
        return Err(AppError::ConfigurationCorrupted {
            message: "Skill read snapshot belongs to another Environment".to_string(),
        });
    }
    let is_global = matches!(plan.read_plan.context.scope, ContextScope::Global);
    let mut directory_kinds = BTreeMap::<(u32, String), FilesystemEntryKind>::new();
    let mut path_exists = false;
    for fact in &snapshot.facts {
        let root = plan
            .read_plan
            .roots
            .get(fact.root_index as usize)
            .ok_or_else(|| AppError::ConfigurationCorrupted {
                message: "Skill read snapshot contains an unknown root".to_string(),
            })?;
        if root.locator.native_path == plan.context_root
            && fact.relative_path.is_empty()
            && matches!(
                fact.kind,
                FilesystemEntryKind::Directory | FilesystemEntryKind::Symlink
            )
        {
            path_exists = true;
        }
        if !fact.relative_path.is_empty() && !fact.relative_path.ends_with("/SKILL.md") {
            directory_kinds.insert((fact.root_index, fact.relative_path.clone()), fact.kind);
        }
    }

    let mut candidates = BTreeMap::<String, SkillCandidate>::new();
    for fact in &snapshot.facts {
        let Some(relative_dir) = fact.relative_path.strip_suffix("/SKILL.md") else {
            continue;
        };
        if fact.truncated || fact.error_code.is_some() {
            continue;
        }
        let Some(frontmatter) = parse_skill_frontmatter(&fact.frontmatter_bytes) else {
            continue;
        };
        if frontmatter
            .metadata
            .as_ref()
            .is_some_and(|metadata| metadata.internal)
        {
            continue;
        }
        let root = &plan.read_plan.roots[fact.root_index as usize]
            .locator
            .native_path;
        let Some(owners) = plan.owners.get(root) else {
            continue;
        };
        let skill_path = join_native_path(&snapshot.environment, root, relative_dir);
        let is_symlink = matches!(
            directory_kinds.get(&(fact.root_index, relative_dir.to_string())),
            Some(FilesystemEntryKind::Symlink | FilesystemEntryKind::ReparsePoint)
        );
        let canonical_owner = owners.contains(&SkillReadOwner::Canonical);
        let candidate = candidates
            .entry(frontmatter.name.clone())
            .or_insert_with(|| SkillCandidate {
                description: frontmatter.description.clone(),
                canonical_path: skill_path.clone(),
                canonical_present: canonical_owner,
                canonical_is_symlink: canonical_owner && is_symlink,
                private_agents: BTreeSet::new(),
                private_symlink_agents: BTreeSet::new(),
                eve_targets: Vec::new(),
            });
        if canonical_owner {
            candidate.description = frontmatter.description;
            candidate.canonical_path = skill_path.clone();
            candidate.canonical_present = true;
            candidate.canonical_is_symlink = is_symlink;
        }
        for owner in owners {
            match owner {
                SkillReadOwner::Canonical => {}
                SkillReadOwner::Agent(agent_id) => {
                    candidate.private_agents.insert(agent_id.clone());
                    if is_symlink {
                        candidate.private_symlink_agents.insert(agent_id.clone());
                    }
                }
                SkillReadOwner::Eve(target) => {
                    let mut target = target.clone();
                    target.path = skill_path.clone();
                    if !candidate
                        .eve_targets
                        .iter()
                        .any(|existing| existing.target_id == target.target_id)
                    {
                        if target.subagent.is_none() {
                            candidate.private_agents.insert(target.agent.clone());
                            if is_symlink {
                                candidate
                                    .private_symlink_agents
                                    .insert(target.agent.clone());
                            }
                        }
                        candidate.eve_targets.push(target);
                    }
                }
            }
        }
    }

    let mut skills = candidates
        .into_iter()
        .map(|(name, candidate)| project_candidate(name, candidate, runtime, is_global))
        .collect::<Vec<_>>();
    skills.sort_by(|left, right| left.name.cmp(&right.name));
    let agents = runtime
        .agents
        .values()
        .filter(|agent| {
            if is_global {
                agent.global.enabled
            } else {
                agent.project.enabled
            }
        })
        .cloned()
        .collect();
    Ok(ListSkillsResult {
        skills,
        agents,
        path_exists,
    })
}

fn add_owned_root(
    builder: &mut ReadPlanBuilder,
    context: &ResolvedContext,
    owners: &mut BTreeMap<String, Vec<SkillReadOwner>>,
    path: &str,
    purpose: ReadRootPurpose,
    owner: SkillReadOwner,
    consumer: Option<AgentId>,
) -> Result<(), AppError> {
    builder.add_root(locator(context, path), purpose, consumer)?;
    let root_owners = owners.entry(path.to_string()).or_default();
    if !root_owners.contains(&owner) {
        root_owners.push(owner);
    }
    Ok(())
}

fn locator(context: &ResolvedContext, path: &str) -> ResourceLocator {
    ResourceLocator {
        environment: context.context.environment.clone(),
        native_path: path.to_string(),
    }
}

fn read_context_revision(
    context: &ResolvedContext,
    runtime: &AgentRuntimeSnapshot,
) -> Result<ContextSnapshotRevision, AppError> {
    let encoded = serde_json::to_vec(&(
        &context.context,
        &context.project,
        &context.skill_root,
        &context.lock,
        &runtime.registry_revision,
        &runtime.environment_revision,
    ))?;
    ContextSnapshotRevision::parse(format!("read-context-v1-{:x}", Sha256::digest(encoded)))
}

fn join_native_path(environment: &EnvironmentRef, root: &str, relative: &str) -> String {
    match environment {
        EnvironmentRef::Wsl { .. } => {
            format!("{}/{}", root.trim_end_matches('/'), relative)
        }
        EnvironmentRef::Host => PathBuf::from(root)
            .join(relative)
            .to_string_lossy()
            .into_owned(),
    }
}

fn parse_skill_frontmatter(bytes: &[u8]) -> Option<SkillFrontmatter> {
    let content = std::str::from_utf8(bytes).ok()?;
    let rest = content.strip_prefix("---")?;
    let end = rest.find("---")?;
    let frontmatter: SkillFrontmatter = serde_yaml::from_str(rest[..end].trim()).ok()?;
    (!frontmatter.name.is_empty() && !frontmatter.description.is_empty()).then_some(frontmatter)
}

fn project_candidate(
    name: String,
    candidate: SkillCandidate,
    runtime: &AgentRuntimeSnapshot,
    is_global: bool,
) -> InstalledSkill {
    let mut agents = Vec::new();
    let mut associated_agents = Vec::new();
    let mut default_available_agents = Vec::new();
    let mut private_adapted_agents = Vec::new();
    let mut duplicate_copy_agents = Vec::new();
    let mut private_only_agents = Vec::new();
    let mut private_copy_agents = Vec::new();

    for (agent_id, resolved) in &runtime.agents {
        let scope = if is_global {
            &resolved.global
        } else {
            &resolved.project
        };
        let canonical_is_private = candidate.canonical_present
            && scope
                .private_path
                .as_ref()
                .is_some_and(|private_path| scope.shared_path.as_ref() == Some(private_path));
        let presence = resolved_agent_presence_from_paths(
            agent_id,
            resolved,
            &name,
            is_global,
            candidate.canonical_present,
            canonical_is_private || candidate.private_agents.contains(agent_id),
        );
        let effective = match presence.presence {
            AgentSkillPresence::DefaultActive => {
                default_available_agents.push(agent_id.clone());
                true
            }
            AgentSkillPresence::DuplicateCopy => {
                default_available_agents.push(agent_id.clone());
                duplicate_copy_agents.push(agent_id.clone());
                private_copy_agents.push(agent_id.clone());
                true
            }
            AgentSkillPresence::PrivateOnly => {
                private_only_agents.push(agent_id.clone());
                if availability_for_resolved_scope(scope).kind
                    == AgentAvailabilityKind::SharedCompatible
                {
                    private_copy_agents.push(agent_id.clone());
                } else {
                    private_adapted_agents.push(agent_id.clone());
                }
                true
            }
            AgentSkillPresence::RequiresPrivateInstall | AgentSkillPresence::NotInstalled => false,
        };
        if effective {
            agents.push(agent_id.clone());
            if resolved.detection == DetectionState::Detected {
                associated_agents.push(agent_id.clone());
            }
        }
    }

    for target in &candidate.eve_targets {
        if !agents.contains(&target.agent) {
            agents.push(target.agent.clone());
            private_adapted_agents.push(target.agent.clone());
            if runtime
                .agents
                .get(&target.agent)
                .is_some_and(|agent| agent.detection == DetectionState::Detected)
            {
                associated_agents.push(target.agent.clone());
            }
        }
    }

    InstalledSkill {
        name,
        description: candidate.description,
        path: candidate.canonical_path.clone(),
        canonical_path: candidate.canonical_path,
        scope: if is_global {
            SkillScope::Global
        } else {
            SkillScope::Project
        },
        agents,
        associated_agents,
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
        default_available_agent_count: Some(default_available_agents.len() as u32),
        private_adapted_agent_count: Some(private_adapted_agents.len() as u32),
        duplicate_copy_count: Some(duplicate_copy_agents.len() as u32),
        default_available_agents: Some(default_available_agents),
        private_adapted_agents: Some(private_adapted_agents),
        duplicate_copy_agents: Some(duplicate_copy_agents),
        private_only_agents: Some(private_only_agents),
        private_copy_agents: Some(private_copy_agents),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::{build_skill_read_plan, project_skill_snapshot};
    use crate::core::agent_definition::{
        AgentAdapter, AgentDefinition, AgentId, AgentSource, DetectionSpec, PathSpec,
        ScopeDefinition,
    };
    use crate::environment::agent_environment::{
        AgentRuntimeSnapshot, DetectionState, ResolvedAgent, ResolvedAgentScope,
    };
    use crate::environment::context_resolver::ResolvedContext;
    #[cfg(unix)]
    use crate::environment::inspection::FilesystemInspector;
    use crate::environment::inspection::{FilesystemEntryKind, RawFilesystemSnapshot, RawPathFact};
    #[cfg(unix)]
    use crate::environment::native::inspection::NativeInspector;
    use crate::environment::types::{
        ContextRef, ContextScope, EnvironmentRef, EnvironmentStatus, ProjectBinding,
        ResourceLocator,
    };

    fn resolved_scope(shared_root: &str, private_root: Option<&str>) -> ResolvedAgentScope {
        ResolvedAgentScope {
            enabled: true,
            reads_shared: true,
            shared_path: Some(shared_root.to_string()),
            private_path: private_root.map(str::to_string),
            read_paths: Vec::new(),
            shared_presence: None,
            private_presence: None,
            legacy_paths: Vec::new(),
        }
    }

    fn runtime(environment: EnvironmentRef) -> AgentRuntimeSnapshot {
        let id = AgentId::parse("custom-both").unwrap();
        let shared_root = "/work/app/.agents/skills";
        let private_root = "/work/app/.custom/skills";
        let resolved = ResolvedAgent {
            definition: AgentDefinition {
                id: id.clone(),
                display_name: "Custom Both".to_string(),
                source: AgentSource::Custom,
                aliases: Vec::new(),
                global: ScopeDefinition {
                    enabled: false,
                    reads_shared: false,
                    private_path: None,
                },
                project: ScopeDefinition {
                    enabled: true,
                    reads_shared: true,
                    private_path: Some(PathSpec::project(".custom/skills")),
                },
                detection: DetectionSpec::AnyPathExists {
                    paths: vec![PathSpec::project(".custom")],
                },
                legacy_paths: Vec::new(),
                adapter: AgentAdapter::Standard,
            },
            detection: DetectionState::Detected,
            detection_reason: None,
            global: ResolvedAgentScope {
                enabled: false,
                reads_shared: false,
                shared_path: None,
                private_path: None,
                read_paths: Vec::new(),
                shared_presence: None,
                private_presence: None,
                legacy_paths: Vec::new(),
            },
            project: resolved_scope(shared_root, Some(private_root)),
        };
        AgentRuntimeSnapshot {
            registry_revision: "registry-v1".to_string(),
            environment_revision: "environment-v1".to_string(),
            environment,
            availability: EnvironmentStatus::Available,
            project_path: Some("/work/app".to_string()),
            agents: BTreeMap::from([(id, resolved)]),
        }
    }

    fn context(environment: EnvironmentRef) -> ResolvedContext {
        ResolvedContext {
            context: ContextRef {
                environment: environment.clone(),
                scope: ContextScope::Project {
                    project_id: "project-1".to_string(),
                },
            },
            project: Some(ProjectBinding {
                id: "project-1".to_string(),
                native_path: "/work/app".to_string(),
                display_name: None,
                order: None,
                suppress_cross_storage_warning: false,
            }),
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
        }
    }

    fn root_index(plan: &super::SkillReadPlan, path: &str) -> u32 {
        plan.read_plan
            .roots
            .iter()
            .position(|root| root.locator.native_path == path)
            .unwrap() as u32
    }

    fn root_fact(root_index: u32) -> RawPathFact {
        RawPathFact {
            root_index,
            relative_path: String::new(),
            kind: FilesystemEntryKind::Directory,
            resolved_target: None,
            frontmatter_bytes: Vec::new(),
            truncated: false,
            error_code: None,
        }
    }

    fn skill_facts(root_index: u32) -> [RawPathFact; 2] {
        [
            RawPathFact {
                root_index,
                relative_path: "toolkit".to_string(),
                kind: FilesystemEntryKind::Directory,
                resolved_target: None,
                frontmatter_bytes: Vec::new(),
                truncated: false,
                error_code: None,
            },
            RawPathFact {
                root_index,
                relative_path: "toolkit/SKILL.md".to_string(),
                kind: FilesystemEntryKind::File,
                resolved_target: None,
                frontmatter_bytes: b"---\nname: toolkit\ndescription: Toolkit\n---\n".to_vec(),
                truncated: false,
                error_code: None,
            },
        ]
    }

    #[test]
    fn open_agent_roots_are_deduplicated_and_projected_with_duplicate_copy_semantics() {
        let environment = EnvironmentRef::Wsl {
            distro_name: "Ubuntu".to_string(),
        };
        let context = context(environment.clone());
        let runtime = runtime(environment.clone());
        let plan = build_skill_read_plan(&context, &runtime, &[]).unwrap();

        assert_eq!(
            plan.read_plan
                .roots
                .iter()
                .filter(|root| root.locator.native_path == "/work/app/.agents/skills")
                .count(),
            1
        );
        let context_index = root_index(&plan, "/work/app");
        let canonical_index = root_index(&plan, "/work/app/.agents/skills");
        let private_index = root_index(&plan, "/work/app/.custom/skills");
        let mut facts = vec![
            root_fact(context_index),
            root_fact(canonical_index),
            root_fact(private_index),
        ];
        facts.extend(skill_facts(canonical_index));
        facts.extend(skill_facts(private_index));
        let total_content_bytes = facts
            .iter()
            .map(|fact| fact.frontmatter_bytes.len() as u32)
            .sum();

        let result = project_skill_snapshot(
            &plan,
            RawFilesystemSnapshot {
                environment,
                facts,
                total_content_bytes,
            },
            &runtime,
        )
        .unwrap();

        assert!(result.path_exists);
        assert_eq!(result.skills.len(), 1);
        let skill = &result.skills[0];
        assert_eq!(skill.name, "toolkit");
        assert_eq!(skill.agents[0].as_str(), "custom-both");
        assert_eq!(skill.duplicate_copy_count, Some(1));
        assert_eq!(skill.associated_agents[0].as_str(), "custom-both");
    }

    #[test]
    fn skill_snapshot_returns_the_scope_agents_used_for_projection() {
        let environment = EnvironmentRef::Host;
        let context = context(environment.clone());
        let runtime = runtime(environment.clone());
        let plan = build_skill_read_plan(&context, &runtime, &[]).unwrap();

        let result = project_skill_snapshot(
            &plan,
            RawFilesystemSnapshot {
                environment,
                facts: Vec::new(),
                total_content_bytes: 0,
            },
            &runtime,
        )
        .unwrap();

        assert_eq!(result.agents.len(), 1);
        assert_eq!(result.agents[0].definition.id.as_str(), "custom-both");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn native_private_directory_symlink_is_included_in_associated_agents() {
        use std::os::unix::fs::symlink;

        let temp = tempfile::tempdir().unwrap();
        let project_root = temp.path();
        let canonical_root = project_root.join(".agents/skills");
        let canonical_skill = canonical_root.join("toolkit");
        let agent_root = project_root.join(".custom/skills");
        std::fs::create_dir_all(&canonical_skill).unwrap();
        std::fs::create_dir_all(&agent_root).unwrap();
        std::fs::write(
            canonical_skill.join("SKILL.md"),
            b"---\nname: toolkit\ndescription: Toolkit\n---\n",
        )
        .unwrap();
        symlink(&canonical_skill, agent_root.join("toolkit")).unwrap();

        let environment = EnvironmentRef::Host;
        let mut context = context(environment.clone());
        context.project.as_mut().unwrap().native_path = project_root.to_string_lossy().into_owned();
        context.skill_root.native_path = canonical_root.to_string_lossy().into_owned();
        context.lock.native_path = project_root
            .join("skills-lock.json")
            .to_string_lossy()
            .into_owned();

        let mut runtime = runtime(environment.clone());
        let resolved = runtime.agents.values_mut().next().unwrap();
        resolved.project.reads_shared = false;
        resolved.project.shared_path = Some(canonical_root.to_string_lossy().into_owned());
        resolved.project.private_path = Some(agent_root.to_string_lossy().into_owned());
        let plan = build_skill_read_plan(&context, &runtime, &[]).unwrap();
        let snapshot = NativeInspector::new(environment)
            .inspect(&plan.read_plan)
            .await
            .unwrap();

        let result = project_skill_snapshot(&plan, snapshot, &runtime).unwrap();

        let skill = result
            .skills
            .iter()
            .find(|skill| skill.name == "toolkit")
            .unwrap();
        assert_eq!(
            skill.associated_agents.as_slice(),
            [AgentId::parse("custom-both").unwrap()].as_slice()
        );
        assert_eq!(
            skill.private_adapted_agents.as_deref(),
            Some([AgentId::parse("custom-both").unwrap()].as_slice())
        );
    }
}

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use serde::Serialize;
use sha2::{Digest, Sha256};
use specta::Type;

use crate::core::agent_availability::{
    availability_for_resolved_scope, resolved_agent_presence_from_paths, AgentAvailabilityKind,
};
use crate::core::agent_definition::{AgentAdapter, AgentId};
use crate::core::skill::{InstalledSkill, InstalledSkillLocation, SkillFrontmatter};
use crate::environment::agent_environment::inspect_eve_project;
use crate::environment::agent_environment::{AgentRuntimeSnapshot, DetectionState, ResolvedAgent};
use crate::environment::context_resolver::ResolvedContext;
use crate::environment::inspection::{
    FilesystemEntryKind, RawFilesystemSnapshot, ReadPlan, ReadPlanBuilder, ReadRootPurpose,
};
use crate::environment::runtime::ContextSnapshotRevision;
use crate::environment::types::{
    same_environment_identity, EnvironmentRef, ResourceLocator, SkillLocation,
};
use crate::environment::wsl::WslWorkspace;
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
    project_lock: Option<crate::core::lossless_lock::LosslessLockDocument>,
    eve_project: bool,
}

impl SkillReadPlan {
    pub(crate) fn set_project_lock(&mut self, bytes: Option<&[u8]>) {
        self.project_lock = if matches!(self.read_plan.context.scope, SkillLocation::Project { .. })
        {
            bytes.and_then(|bytes| {
                crate::core::lossless_lock::LosslessLockDocument::parse(bytes).ok()
            })
        } else {
            None
        };
    }
}

/// `list_skills` 的运行时读取结果。
/// Skill 与 scope Agents 来自同一次 Agent runtime snapshot，避免 Frontend 拼接不同 revision。
#[derive(Debug, Serialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct ListSkillsResult {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path_base: Option<crate::environment::context_resolver::ScopePathBase>,
    pub skills: Vec<InstalledSkill>,
    pub agents: Vec<ResolvedAgent>,
    /// 项目目录是否存在（project scope 时有意义，global 始终为 true）
    pub path_exists: bool,
    pub library_application: crate::application::library_application::LibraryApplicationSummary,
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
    wsl_workspace: Option<&WslWorkspace>,
) -> Result<Vec<SkillInstallTargetInfo>, AppError> {
    let Some(project) = context.project.as_ref() else {
        return Ok(Vec::new());
    };
    if !runtime
        .agents
        .values()
        .any(|agent| agent.definition.adapter == AgentAdapter::Eve)
    {
        return Ok(Vec::new());
    }
    match (&context.context.environment, wsl_workspace) {
        (EnvironmentRef::Native, None) => Ok(crate::core::eve::eve_install_targets_for_project(
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
        (EnvironmentRef::Wsl { .. }, Some(workspace)) => {
            let inspected = inspect_eve_project(workspace, &project.native_path).await?;
            Ok(inspected
                .install_targets(&project.native_path)
                .into_iter()
                .map(|target| SkillInstallTargetInfo {
                    target_id: target.target_id,
                    agent: target.agent,
                    display_name: target.display_name,
                    subagent: target.subagent,
                    path: target.path,
                })
                .collect())
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
    let is_global = matches!(context.context.scope, SkillLocation::Global);
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
        project_lock: None,
        eve_project: context.project.is_some() && !eve_targets.is_empty(),
    })
}

pub(crate) async fn project_direct_skill_snapshot(
    plan: &SkillReadPlan,
    mut snapshot: RawFilesystemSnapshot,
    runtime: &AgentRuntimeSnapshot,
    libraries: &dyn crate::application::skill_libraries::SkillLibraryRepository,
    targets: &dyn crate::environment::planning::TargetFactResolver,
) -> Result<ListSkillsResult, AppError> {
    use crate::application::installed_skill_resolver::SkillDirectoryName;
    use crate::application::library_candidates::ResolvedLibraryCandidateIndex;
    use crate::core::skill::InstalledLibraryVersion;

    let single_files = project_recorded_eve_entries(plan, &mut snapshot, runtime, targets).await?;
    let mut entries = Vec::new();
    for fact in &snapshot.facts {
        let Some(relative) = fact.relative_path.strip_suffix("/SKILL.md") else {
            continue;
        };
        let Some(frontmatter) = parse_skill_frontmatter(&fact.frontmatter_bytes) else {
            continue;
        };
        let root = plan
            .read_plan
            .roots
            .get(fact.root_index as usize)
            .ok_or(AppError::StaleTarget)?;
        entries.push((
            (fact.root_index, fact.relative_path.clone()),
            SkillDirectoryName::try_from(frontmatter.name.as_str())?,
            ResourceLocator {
                environment: snapshot.environment.clone(),
                native_path: join_native_path(
                    &snapshot.environment,
                    &root.locator.native_path,
                    relative,
                ),
            },
        ));
    }
    let names = entries
        .iter()
        .map(|(_, name, _)| name.clone())
        .collect::<BTreeSet<_>>();
    // 读取器不会展开作为根的链接；仍需核对这些根下已知 Skill 的库归属。
    for (root_index, root) in plan.read_plan.roots.iter().enumerate() {
        if !plan.owners.contains_key(&root.locator.native_path) {
            continue;
        }
        for name in &names {
            let path = root.locator.join_child(name.as_ref());
            if !entries.iter().any(|(_, _, existing)| existing == &path) {
                entries.push((
                    (root_index as u32, format!("{}/SKILL.md", name.as_ref())),
                    name.clone(),
                    path,
                ));
            }
        }
    }
    let catalog = libraries.load(&snapshot.environment).await?;
    let known = ResolvedLibraryCandidateIndex::from_catalog(
        libraries,
        targets,
        &snapshot.environment,
        &names,
        &catalog,
    )
    .await?;
    let destinations = entries
        .iter()
        .map(|(_, _, path)| path.clone())
        .collect::<Vec<_>>();
    let facts = if destinations.is_empty() {
        Vec::new()
    } else {
        targets
            .resolve(&plan.read_plan.context, &destinations, None)
            .await?
    };
    if facts.len() != entries.len() {
        return Err(AppError::StaleTarget);
    }
    let mut excluded = BTreeSet::new();
    let mut references = BTreeMap::<SkillDirectoryName, Vec<InstalledLibraryVersion>>::new();
    for ((id, name, _), fact) in entries.iter().zip(&facts) {
        if let Some(owner) = known.owner(name, fact) {
            excluded.insert(id.clone());
            let library = catalog
                .libraries
                .iter()
                .find(|library| &library.id == owner.library_id())
                .ok_or(AppError::StaleTarget)?;
            let reference = InstalledLibraryVersion {
                library_id: library.id.as_str().to_string(),
                library_name: library.name.clone(),
                skill_name: owner.member_name().to_string(),
            };
            let versions = references.entry(name.clone()).or_default();
            if !versions.contains(&reference) {
                versions.push(reference);
            }
        } else if matches!(
            fact.entry_kind,
            crate::environment::planning::TargetEntryKind::Symlink
                | crate::environment::planning::TargetEntryKind::Junction
        ) {
            let direct = entries
                .iter()
                .zip(&facts)
                .filter(|((_, candidate_name, _), target)| {
                    candidate_name == name
                        && target.entry_kind
                            == crate::environment::planning::TargetEntryKind::Directory
                        && known.owner(name, target).is_none()
                })
                .map(|(_, target)| target)
                .collect::<Vec<_>>();
            if !direct.is_empty()
                && !direct.iter().any(|target| {
                    fact.link_target_identity
                        .as_ref()
                        .is_some_and(|identity| identity.matches(&target.destination))
                })
            {
                excluded.insert(id.clone());
            }
        }
    }
    snapshot
        .facts
        .retain(|fact| !excluded.contains(&(fact.root_index, fact.relative_path.clone())));
    let mut result = project_skill_snapshot(plan, snapshot, runtime)?;
    for skill in &mut result.skills {
        skill.library_versions =
            references.remove(&SkillDirectoryName::try_from(skill.name.as_str())?);
    }
    for single in single_files {
        if let Some(skill) = result
            .skills
            .iter_mut()
            .find(|skill| skill.name == single.name)
        {
            skill.maintenance_error = single.maintenance_error;
        } else {
            result.skills.push(single);
        }
    }
    result.skills.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(result)
}

async fn project_recorded_eve_entries(
    plan: &SkillReadPlan,
    snapshot: &mut RawFilesystemSnapshot,
    runtime: &AgentRuntimeSnapshot,
    targets: &dyn crate::environment::planning::TargetFactResolver,
) -> Result<Vec<InstalledSkill>, AppError> {
    use crate::application::installed_skill_resolver::InstalledSkillResolver;
    use crate::application::scope_skill_placements::{ambiguous_installed_source, eve_target_ids};
    use crate::core::local_lock::LocalSkillLockEntry;

    if !plan.eve_project {
        return Ok(Vec::new());
    }
    let mut actual = Vec::new();
    for (index, fact) in snapshot.facts.iter().enumerate() {
        let root = plan
            .read_plan
            .roots
            .get(fact.root_index as usize)
            .ok_or(AppError::StaleTarget)?;
        if !plan
            .owners
            .get(&root.locator.native_path)
            .is_some_and(|owners| {
                owners
                    .iter()
                    .any(|owner| matches!(owner, SkillReadOwner::Eve(_)))
            })
        {
            continue;
        }
        let (relative, single_file) =
            if let Some(relative) = fact.relative_path.strip_suffix("/SKILL.md") {
                (relative, false)
            } else if fact.kind == FilesystemEntryKind::File
                && !fact.relative_path.contains('/')
                && fact.relative_path.ends_with(".md")
            {
                (fact.relative_path.as_str(), true)
            } else {
                continue;
            };
        actual.push((
            index,
            fact.root_index,
            root.locator.join_child(relative),
            single_file,
        ));
    }
    if actual.is_empty() {
        return Ok(Vec::new());
    }
    let records = plan.project_lock.as_ref().and_then(|document| {
        (document
            .root_snapshot("version")
            .value()
            .and_then(serde_json::Value::as_u64)
            .unwrap_or_default()
            >= 1)
            .then(|| document.root_snapshot("skills").value().cloned())
            .flatten()
    });
    let mut expected = Vec::new();
    if let Some(records) = records.as_ref().and_then(serde_json::Value::as_object) {
        for (name, value) in records {
            let Ok(record) = serde_json::from_value::<LocalSkillLockEntry>(value.clone()) else {
                continue;
            };
            if record.source.trim().is_empty() || record.source_type.trim().is_empty() {
                continue;
            }
            let directory = InstalledSkillResolver::install_dir_name(name)?;
            let ids = eve_target_ids(record.subagents.as_deref())?;
            for (root_index, root) in plan.read_plan.roots.iter().enumerate() {
                for owner in plan
                    .owners
                    .get(&root.locator.native_path)
                    .into_iter()
                    .flatten()
                {
                    let SkillReadOwner::Eve(target) = owner else {
                        continue;
                    };
                    if !ids.contains(&target.target_id) {
                        continue;
                    }
                    for single_file in [false, true] {
                        let leaf = if single_file {
                            format!("{directory}.md")
                        } else {
                            directory.clone()
                        };
                        expected.push((
                            root_index as u32,
                            name.clone(),
                            target.clone(),
                            root.locator.join_child(&leaf),
                            single_file,
                        ));
                    }
                }
            }
        }
    }
    let destinations = actual
        .iter()
        .map(|(_, _, path, _)| path.clone())
        .chain(expected.iter().map(|(_, _, _, path, _)| path.clone()))
        .collect::<Vec<_>>();
    let facts = targets
        .resolve(&plan.read_plan.context, &destinations, None)
        .await?;
    if facts.len() != destinations.len() {
        return Err(AppError::StaleTarget);
    }
    let mut single_files = Vec::new();
    for (position, (fact_index, root_index, path, single_file)) in actual.iter().enumerate() {
        let mut matches = expected
            .iter()
            .zip(&facts[actual.len()..])
            .filter(|((_, _, _, _, file), fact)| {
                file == single_file && fact.key == facts[position].key
            })
            .collect::<Vec<_>>();
        if matches
            .iter()
            .map(|((_, name, _, _, _), _)| name)
            .collect::<BTreeSet<_>>()
            .len()
            > 1
        {
            return Err(ambiguous_installed_source(&path.native_path));
        }
        matches.sort_by_key(|((root, _, _, _, _), _)| root != root_index);
        let Some(((root, name, target, _, _), _)) = matches.first() else {
            snapshot.facts[*fact_index].frontmatter_bytes.clear();
            continue;
        };
        let positions = expected
            .iter()
            .zip(&facts[actual.len()..])
            .filter(|((_, other_name, other_target, _, file), _)| {
                other_name == name
                    && other_target.target_id == target.target_id
                    && file == single_file
            })
            .map(|(_, fact)| &fact.key)
            .collect::<BTreeSet<_>>();
        if positions.len() > 1 {
            return Err(ambiguous_installed_source(&path.native_path));
        }
        if *single_file {
            if expected.iter().zip(&facts[actual.len()..]).any(
                |((other_root, other_name, _, _, file), fact)| {
                    other_root == root
                        && other_name == name
                        && !file
                        && fact.entry_kind != crate::environment::planning::TargetEntryKind::Missing
                },
            ) {
                return Err(ambiguous_installed_source(&path.native_path));
            }
            let mut target = target.clone();
            target.path = path.native_path.clone();
            let mut skill = project_candidate(
                name.clone(),
                SkillCandidate {
                    description: String::new(),
                    canonical_path: path.native_path.clone(),
                    canonical_present: false,
                    canonical_is_symlink: false,
                    private_agents: BTreeSet::new(),
                    private_symlink_agents: BTreeSet::new(),
                    eve_targets: vec![target],
                },
                runtime,
                false,
            );
            skill.maintenance_error = Some(AppError::CapabilityUnavailable {
                capability: "eveSingleFile".into(),
                path: Some(path.native_path.clone()),
            });
            single_files.push(skill);
            continue;
        }
        // 仅为读取投影补充已确认的身份；磁盘中的 Eve 文件保持原样。
        let bytes = &mut snapshot.facts[*fact_index].frontmatter_bytes;
        let parsed = std::str::from_utf8(bytes)
            .ok()
            .and_then(|raw| raw.strip_prefix("---"))
            .and_then(|rest| rest.find("---").map(|end| &rest[..end]))
            .and_then(|yaml| serde_yaml::from_str::<serde_yaml::Mapping>(yaml).ok());
        if let Some(mut metadata) = parsed {
            metadata.insert(
                serde_yaml::Value::String("name".into()),
                serde_yaml::Value::String(name.clone()),
            );
            *bytes = format!("---\n{}---\n", serde_yaml::to_string(&metadata)?).into_bytes();
        } else {
            bytes.clear();
        }
    }
    Ok(single_files)
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
    let is_global = matches!(plan.read_plan.context.scope, SkillLocation::Global);
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
        if relative_dir.starts_with(".skill-deck-stage-")
            || relative_dir.starts_with(".skill-deck-backup-")
        {
            continue;
        }
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
        if canonical_owner
            || (!candidate.canonical_present && skill_path < candidate.canonical_path)
        {
            candidate.description = frontmatter.description.clone();
            candidate.canonical_path = skill_path.clone();
        }
        if canonical_owner {
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
        path_base: None,
        skills,
        agents,
        path_exists,
        library_application: crate::application::library_application::LibraryApplicationSummary {
            ordered_libraries: Vec::new(),
            selected_agent_ids: Vec::new(),
            pending: false,
            sync_state:
                crate::application::library_application::LibraryApplicationSyncState::Synced,
        },
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
        EnvironmentRef::Native => PathBuf::from(root)
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
                .is_some_and(|private_path| scope.standard_path.as_ref() == Some(private_path));
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
                    == AgentAvailabilityKind::StandardCompatible
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
            InstalledSkillLocation::Global
        } else {
            InstalledSkillLocation::Project
        },
        agents,
        associated_agents,
        library_versions: None,
        maintenance_error: None,
        comparison_fingerprint: None,
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
        skill_path: None,
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
    use crate::environment::inspection::FilesystemInspector;
    use crate::environment::inspection::{FilesystemEntryKind, RawFilesystemSnapshot, RawPathFact};
    use crate::environment::native::inspection::NativeInspector;
    use crate::environment::types::{
        EnvironmentRef, EnvironmentStatus, RegisteredProject, ResourceLocator, SkillLocation,
        SkillLocationRef,
    };

    fn resolved_scope(standard_root: &str, private_root: Option<&str>) -> ResolvedAgentScope {
        ResolvedAgentScope {
            enabled: true,
            reads_standard: true,
            standard_path: Some(standard_root.to_string()),
            private_path: private_root.map(str::to_string),
            read_paths: Vec::new(),
            standard_presence: None,
            private_presence: None,
            legacy_paths: Vec::new(),
        }
    }

    fn runtime(environment: EnvironmentRef) -> AgentRuntimeSnapshot {
        let id = AgentId::parse("custom-both").unwrap();
        let standard_root = "/work/app/.agents/skills";
        let private_root = "/work/app/.custom/skills";
        let resolved = ResolvedAgent {
            definition: AgentDefinition {
                id: id.clone(),
                display_name: "Custom Both".to_string(),
                source: AgentSource::Custom,
                aliases: Vec::new(),
                global: ScopeDefinition {
                    enabled: false,
                    reads_standard: false,
                    private_path: None,
                },
                project: ScopeDefinition {
                    enabled: true,
                    reads_standard: true,
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
                reads_standard: false,
                standard_path: None,
                private_path: None,
                read_paths: Vec::new(),
                standard_presence: None,
                private_presence: None,
                legacy_paths: Vec::new(),
            },
            project: resolved_scope(standard_root, Some(private_root)),
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
            context: SkillLocationRef {
                environment: environment.clone(),
                scope: SkillLocation::Project {
                    project_id: "project-1".to_string(),
                },
            },
            project: Some(RegisteredProject {
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
        let environment = EnvironmentRef::Native;
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

    #[test]
    fn skill_snapshot_ignores_managed_stage_and_backup_directories() {
        let environment = EnvironmentRef::Wsl {
            distro_name: "Ubuntu".to_string(),
        };
        let context = context(environment.clone());
        let runtime = runtime(environment.clone());
        let plan = build_skill_read_plan(&context, &runtime, &[]).unwrap();
        let canonical_index = root_index(&plan, "/work/app/.agents/skills");
        let mut facts = vec![root_fact(canonical_index)];
        for (directory, name) in [
            ("toolkit", "toolkit"),
            (".skill-deck-stage-operation-000000", "staged"),
            (".skill-deck-backup-operation-000000", "backup"),
        ] {
            facts.push(RawPathFact {
                root_index: canonical_index,
                relative_path: directory.to_string(),
                kind: FilesystemEntryKind::Directory,
                resolved_target: None,
                frontmatter_bytes: Vec::new(),
                truncated: false,
                error_code: None,
            });
            facts.push(RawPathFact {
                root_index: canonical_index,
                relative_path: format!("{directory}/SKILL.md"),
                kind: FilesystemEntryKind::File,
                resolved_target: None,
                frontmatter_bytes: format!("---\nname: {name}\ndescription: Test\n---\n")
                    .into_bytes(),
                truncated: false,
                error_code: None,
            });
        }

        let result = project_skill_snapshot(
            &plan,
            RawFilesystemSnapshot {
                environment,
                facts,
                total_content_bytes: 0,
            },
            &runtime,
        )
        .unwrap();

        assert_eq!(
            result
                .skills
                .iter()
                .map(|skill| skill.name.as_str())
                .collect::<Vec<_>>(),
            vec!["toolkit"]
        );
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

        let environment = EnvironmentRef::Native;
        let mut context = context(environment.clone());
        context.project.as_mut().unwrap().native_path = project_root.to_string_lossy().into_owned();
        context.skill_root.native_path = canonical_root.to_string_lossy().into_owned();
        context.lock.native_path = project_root
            .join("skills-lock.json")
            .to_string_lossy()
            .into_owned();

        let mut runtime = runtime(environment.clone());
        let resolved = runtime.agents.values_mut().next().unwrap();
        resolved.project.reads_standard = false;
        resolved.project.standard_path = Some(canonical_root.to_string_lossy().into_owned());
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

    #[tokio::test]
    async fn native_skill_list_projects_direct_version_beside_library_root() {
        use crate::application::skill_libraries::{LibraryId, LIBRARY_SCHEMA_VERSION};
        use crate::environment::planning::RuntimeTargetFactResolver;
        use crate::environment::wsl::WslRuntime;
        use std::sync::Arc;

        let temp = tempfile::tempdir().unwrap();
        let libraries =
            crate::native_workflow_integration_support::update_library_repository(temp.path());
        let catalog = serde_json::from_value(serde_json::json!({
            "schemaVersion": LIBRARY_SCHEMA_VERSION,
            "libraries": [{"id":"library-one", "name":"Team library", "skills":[{
                "name":"toolkit", "description":"Library description", "sourceRecord":{}, "contentManifestHash":"old"
            }], "retiredSkills":[]}]
        })).unwrap();
        libraries
            .save(&EnvironmentRef::Native, &catalog)
            .await
            .unwrap();
        let root = libraries
            .resolve_collection(&EnvironmentRef::Native, &LibraryId::parse("library-one"))
            .await
            .unwrap();
        let library_root = std::path::PathBuf::from(root.root.native_path);
        std::fs::create_dir_all(library_root.join("toolkit")).unwrap();
        std::fs::write(
            library_root.join("toolkit/SKILL.md"),
            b"---\nname: toolkit\ndescription: Library description\n---\n",
        )
        .unwrap();
        let canonical_root = temp.path().join(".agents/skills");
        std::fs::create_dir_all(&canonical_root).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(library_root.join("toolkit"), canonical_root.join("toolkit"))
            .unwrap();
        #[cfg(windows)]
        junction::create(library_root.join("toolkit"), canonical_root.join("toolkit")).unwrap();
        let private_root = temp.path().join(".custom/skills");
        std::fs::create_dir_all(private_root.join("toolkit")).unwrap();
        std::fs::write(
            private_root.join("toolkit/SKILL.md"),
            b"---\nname: toolkit\ndescription: Direct description\n---\n",
        )
        .unwrap();
        let mut context = context(EnvironmentRef::Native);
        context.project.as_mut().unwrap().native_path = temp.path().to_string_lossy().into_owned();
        context.skill_root.native_path = canonical_root.to_string_lossy().into_owned();
        let mut runtime = runtime(EnvironmentRef::Native);
        let agent = runtime.agents.values_mut().next().unwrap();
        agent.project.standard_path = Some(canonical_root.to_string_lossy().into_owned());
        agent.project.private_path = Some(private_root.to_string_lossy().into_owned());
        let targets = RuntimeTargetFactResolver::new(Arc::new(WslRuntime::default()));
        let plan = build_skill_read_plan(&context, &runtime, &[]).unwrap();
        let snapshot = NativeInspector::new(EnvironmentRef::Native)
            .inspect(&plan.read_plan)
            .await
            .unwrap();
        let result = super::project_direct_skill_snapshot(
            &plan,
            snapshot,
            &runtime,
            libraries.as_ref(),
            &targets,
        )
        .await
        .unwrap();
        assert_eq!(result.skills.len(), 1);
        let skill = &result.skills[0];
        assert_eq!(skill.description, "Direct description");
        assert_eq!(
            skill.canonical_path,
            private_root.join("toolkit").to_string_lossy()
        );
        assert_eq!(skill.default_available_agent_count, Some(0));
        assert_eq!(
            skill.associated_agents,
            vec![AgentId::parse("custom-both").unwrap()]
        );
        let versions = skill.library_versions.as_ref().unwrap();
        assert_eq!(versions.len(), 1);
        assert_eq!(versions[0].library_name, "Team library");
        assert_eq!(versions[0].skill_name, "toolkit");

        #[cfg(unix)]
        std::fs::remove_file(canonical_root.join("toolkit")).unwrap();
        #[cfg(windows)]
        {
            junction::delete(canonical_root.join("toolkit")).unwrap();
            std::fs::remove_dir(canonical_root.join("toolkit")).unwrap();
        }
        std::fs::remove_dir(&canonical_root).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(&library_root, &canonical_root).unwrap();
        #[cfg(windows)]
        junction::create(&library_root, &canonical_root).unwrap();
        let snapshot = NativeInspector::new(EnvironmentRef::Native)
            .inspect(&plan.read_plan)
            .await
            .unwrap();
        let result = super::project_direct_skill_snapshot(
            &plan,
            snapshot,
            &runtime,
            libraries.as_ref(),
            &targets,
        )
        .await
        .unwrap();
        assert_eq!(result.skills[0].description, "Direct description");
        assert_eq!(
            result.skills[0].library_versions.as_ref().unwrap()[0].library_id,
            "library-one"
        );

        std::fs::remove_dir_all(private_root.join("toolkit")).unwrap();
        let snapshot = NativeInspector::new(EnvironmentRef::Native)
            .inspect(&plan.read_plan)
            .await
            .unwrap();
        let result = super::project_direct_skill_snapshot(
            &plan,
            snapshot,
            &runtime,
            libraries.as_ref(),
            &targets,
        )
        .await
        .unwrap();
        assert!(result.skills.is_empty());
    }
}

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};

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
    SkillMetadataSource,
};
use crate::environment::planning::{locator_comparison_key, ResolvedTargetFact, TargetEntryKind};
use crate::environment::runtime::{ContextSnapshotRevision, PhysicalTargetKey};
use crate::environment::types::{
    same_environment_identity, EnvironmentKey, EnvironmentRef, ResourceLocator, SkillLocation,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub read_status: Option<SkillReadStatus>,
    pub library_application: crate::application::library_application::LibraryApplicationSummary,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct SkillReadIssue {
    pub code: String,
    pub path: ResourceLocator,
    pub agent_ids: Vec<AgentId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct SkillReadIssueCount {
    pub code: String,
    pub count: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct SkillReadStatus {
    pub complete: bool,
    pub issues: Vec<SkillReadIssue>,
    pub counts: Vec<SkillReadIssueCount>,
    pub omitted_count: u32,
}

const MAX_PUBLIC_READ_ISSUES: usize = 256;

#[derive(Default)]
struct SkillReadIssueCollector {
    issues: Vec<SkillReadIssue>,
    counts: BTreeMap<String, u32>,
    omitted_count: u32,
    seen: BTreeSet<(String, EnvironmentKey, String)>,
}

impl SkillReadIssueCollector {
    fn record(
        &mut self,
        code: impl Into<String>,
        path: ResourceLocator,
        agent_ids: impl IntoIterator<Item = AgentId>,
    ) {
        let code = code.into();
        if !self.seen.insert((
            code.clone(),
            EnvironmentKey::from_ref(&path.environment),
            path.native_path.clone(),
        )) {
            return;
        }
        *self.counts.entry(code.clone()).or_default() += 1;
        if self.issues.len() < MAX_PUBLIC_READ_ISSUES {
            self.issues.push(SkillReadIssue {
                code,
                path,
                agent_ids: agent_ids.into_iter().collect(),
            });
        } else {
            self.omitted_count += 1;
        }
    }

    fn finish(self) -> SkillReadStatus {
        SkillReadStatus {
            complete: self.counts.is_empty(),
            issues: self.issues,
            counts: self
                .counts
                .into_iter()
                .map(|(code, count)| SkillReadIssueCount { code, count })
                .collect(),
            omitted_count: self.omitted_count,
        }
    }
}

type SkillFactId = (u32, String);
type SkillDocuments = BTreeMap<SkillFactId, Arc<SkillFrontmatter>>;
type SkillDocumentGroup = Vec<(SkillFactId, ResourceLocator)>;

fn skill_fact_id(fact: &crate::environment::inspection::RawPathFact) -> SkillFactId {
    (fact.root_index, fact.relative_path.clone())
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum DocumentIdentity {
    Physical(PhysicalTargetKey),
    LinkTarget(EnvironmentKey, String),
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct SkillMetadataCacheKey {
    registry_revision: String,
    environment_revision: String,
    identity: DocumentIdentity,
    fingerprint: String,
}

#[derive(Clone)]
enum CachedSkillMetadata {
    Parsed(Arc<SkillFrontmatter>),
    InvalidFrontmatter,
}

#[derive(Default)]
struct SkillMetadataCache {
    entries: Mutex<BTreeMap<SkillMetadataCacheKey, CachedSkillMetadata>>,
}

impl SkillMetadataCache {
    const MAX_ENTRIES: usize = 16_384;

    fn get(&self, key: &SkillMetadataCacheKey) -> Option<CachedSkillMetadata> {
        self.entries.lock().ok()?.get(key).cloned()
    }

    fn insert(&self, key: SkillMetadataCacheKey, value: CachedSkillMetadata) {
        let Ok(mut entries) = self.entries.lock() else {
            return;
        };
        if entries.len() >= Self::MAX_ENTRIES && !entries.contains_key(&key) {
            entries.clear();
        }
        entries.insert(key, value);
    }
}

fn skill_metadata_cache() -> &'static SkillMetadataCache {
    static CACHE: OnceLock<SkillMetadataCache> = OnceLock::new();
    CACHE.get_or_init(SkillMetadataCache::default)
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

async fn read_skill_documents(
    plan: &SkillReadPlan,
    snapshot: &mut RawFilesystemSnapshot,
    targets: &dyn crate::environment::planning::TargetFactResolver,
    metadata: &dyn SkillMetadataSource,
) -> Result<
    (
        SkillDocuments,
        BTreeMap<SkillFactId, ResolvedTargetFact>,
        Vec<SkillReadIssue>,
        Vec<SkillDocumentGroup>,
    ),
    AppError,
> {
    struct Candidate {
        fact_id: SkillFactId,
        target: ResourceLocator,
        document: ResourceLocator,
    }

    let mut candidates = Vec::new();
    for fact in &snapshot.facts {
        let root = plan
            .read_plan
            .roots
            .get(fact.root_index as usize)
            .ok_or(AppError::StaleTarget)?;
        if let Some(relative) = fact.relative_path.strip_suffix("/SKILL.md") {
            if relative.starts_with(".skill-deck-stage-")
                || relative.starts_with(".skill-deck-backup-")
            {
                continue;
            }
            candidates.push(Candidate {
                fact_id: skill_fact_id(fact),
                target: root.locator.join_child(relative),
                document: root.locator.join_child(&fact.relative_path),
            });
        } else if fact.kind == FilesystemEntryKind::File
            && !fact.relative_path.contains('/')
            && fact.relative_path.ends_with(".md")
        {
            candidates.push(Candidate {
                fact_id: skill_fact_id(fact),
                target: root.locator.join_child(&fact.relative_path),
                document: root.locator.join_child(&fact.relative_path),
            });
        }
    }
    if candidates.is_empty() {
        return Ok((BTreeMap::new(), BTreeMap::new(), Vec::new(), Vec::new()));
    }

    let target_locators = candidates
        .iter()
        .map(|candidate| candidate.target.clone())
        .collect::<Vec<_>>();
    let resolved = targets
        .resolve(&plan.read_plan.context, &target_locators, None)
        .await?;
    if resolved.len() != candidates.len() {
        return Err(AppError::StaleTarget);
    }
    let direct_by_path = resolved
        .iter()
        .filter(|fact| fact.entry_kind == TargetEntryKind::Directory)
        .filter_map(|fact| {
            locator_comparison_key(&fact.destination).map(|key| (key, fact.key.clone()))
        })
        .collect::<BTreeMap<_, _>>();
    let mut groups = BTreeMap::<DocumentIdentity, Vec<(SkillFactId, ResourceLocator)>>::new();
    let mut resolved_by_fact = BTreeMap::new();
    for (candidate, fact) in candidates.into_iter().zip(resolved) {
        let identity = fact
            .link_target_identity
            .as_ref()
            .map(|identity| identity.comparison_key())
            .map(|key| {
                direct_by_path
                    .get(&key)
                    .cloned()
                    .map(DocumentIdentity::Physical)
                    .unwrap_or_else(|| DocumentIdentity::LinkTarget(key.0, key.1))
            })
            .unwrap_or_else(|| DocumentIdentity::Physical(fact.key.clone()));
        groups
            .entry(identity)
            .or_default()
            .push((candidate.fact_id.clone(), candidate.document));
        resolved_by_fact.insert(candidate.fact_id, fact);
    }

    let fact_positions = snapshot
        .facts
        .iter()
        .enumerate()
        .map(|(index, fact)| (skill_fact_id(fact), index))
        .collect::<BTreeMap<_, _>>();
    struct Group {
        members: Vec<(SkillFactId, ResourceLocator)>,
        cache_key: Option<SkillMetadataCacheKey>,
    }
    #[derive(Clone)]
    struct GroupResult {
        frontmatter: Option<Arc<SkillFrontmatter>>,
        truncated: bool,
        error_code: Option<String>,
        bytes_read: u32,
    }
    let groups = groups
        .into_iter()
        .map(|(identity, members)| {
            let fingerprint = members
                .first()
                .and_then(|(fact_id, _)| fact_positions.get(fact_id))
                .and_then(|index| snapshot.facts[*index].fingerprint.as_ref());
            let cache_key = fingerprint.map(|fingerprint| SkillMetadataCacheKey {
                registry_revision: plan.read_plan.registry_revision.clone(),
                environment_revision: plan.read_plan.environment_revision.clone(),
                identity,
                fingerprint: fingerprint.0.clone(),
            });
            Group { members, cache_key }
        })
        .collect::<Vec<_>>();
    let cache = skill_metadata_cache();
    let mut results = vec![None; groups.len()];
    let mut misses = Vec::new();
    for (index, group) in groups.iter().enumerate() {
        let cached = group.cache_key.as_ref().and_then(|key| cache.get(key));
        results[index] = cached.map(|cached| match cached {
            CachedSkillMetadata::Parsed(frontmatter) => GroupResult {
                frontmatter: Some(frontmatter),
                truncated: false,
                error_code: None,
                bytes_read: 0,
            },
            CachedSkillMetadata::InvalidFrontmatter => GroupResult {
                frontmatter: None,
                truncated: false,
                error_code: Some("invalidFrontmatter".to_string()),
                bytes_read: 0,
            },
        });
        if results[index].is_none() {
            misses.push((index, group.members[0].1.clone()));
        }
    }
    if !misses.is_empty() {
        let representatives = misses
            .iter()
            .map(|(_, locator)| locator.clone())
            .collect::<Vec<_>>();
        let loaded = metadata
            .read(&representatives, plan.read_plan.per_file_limit)
            .await?;
        if loaded.len() != representatives.len()
            || loaded
                .iter()
                .zip(&representatives)
                .any(|(fact, expected)| fact.locator != *expected)
        {
            return Err(AppError::ConfigurationCorrupted {
                message: "Skill metadata response does not match its request".to_string(),
            });
        }
        for ((group_index, _), metadata) in misses.into_iter().zip(loaded) {
            let parsed = (!metadata.truncated && metadata.error_code.is_none())
                .then(|| parse_skill_frontmatter_allow_missing_name(&metadata.bytes))
                .flatten()
                .map(Arc::new);
            let error_code = metadata.error_code.clone().or_else(|| {
                (!metadata.truncated && parsed.is_none()).then(|| "invalidFrontmatter".to_string())
            });
            if let Some(cache_key) = groups[group_index].cache_key.clone() {
                if let Some(frontmatter) = &parsed {
                    cache.insert(cache_key, CachedSkillMetadata::Parsed(frontmatter.clone()));
                } else if !metadata.truncated && metadata.error_code.is_none() {
                    cache.insert(cache_key, CachedSkillMetadata::InvalidFrontmatter);
                }
            }
            results[group_index] = Some(GroupResult {
                frontmatter: parsed,
                truncated: metadata.truncated,
                error_code,
                bytes_read: metadata.bytes.len() as u32,
            });
        }
    }

    let mut documents = BTreeMap::new();
    let mut metadata_issues = Vec::new();
    let mut missing_name_groups = Vec::new();
    snapshot.total_content_bytes = 0;
    for (group, result) in groups.into_iter().zip(results) {
        let result = result.ok_or_else(|| AppError::ConfigurationCorrupted {
            message: "Skill metadata group has no result".to_string(),
        })?;
        snapshot.total_content_bytes = snapshot
            .total_content_bytes
            .saturating_add(result.bytes_read);
        let issue_code = result
            .error_code
            .clone()
            .or_else(|| result.truncated.then(|| "frontmatterTooLarge".to_string()));
        if let Some(code) = issue_code {
            let agent_ids = group
                .members
                .iter()
                .filter_map(|(fact_id, _)| fact_positions.get(fact_id))
                .filter_map(|index| {
                    plan.read_plan
                        .roots
                        .get(snapshot.facts[*index].root_index as usize)
                })
                .flat_map(|root| root.consumer_agent_ids.iter().cloned())
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect();
            metadata_issues.push(SkillReadIssue {
                code,
                path: group.members[0].1.clone(),
                agent_ids,
            });
        }
        if result
            .frontmatter
            .as_ref()
            .is_some_and(|frontmatter| frontmatter.name.is_empty())
        {
            missing_name_groups.push(group.members.clone());
        }
        for (fact_id, _) in group.members {
            if let Some(frontmatter) = &result.frontmatter {
                documents.insert(fact_id, frontmatter.clone());
            }
        }
    }
    Ok((
        documents,
        resolved_by_fact,
        metadata_issues,
        missing_name_groups,
    ))
}

fn reject_unidentified_skill_documents(
    plan: &SkillReadPlan,
    snapshot: &RawFilesystemSnapshot,
    documents: &mut SkillDocuments,
    groups: Vec<SkillDocumentGroup>,
    issues: &mut Vec<SkillReadIssue>,
) -> Result<(), AppError> {
    let positions = snapshot
        .facts
        .iter()
        .enumerate()
        .map(|(index, fact)| (skill_fact_id(fact), index))
        .collect::<BTreeMap<_, _>>();
    for group in groups {
        let invalid = group
            .into_iter()
            .filter(|(fact_id, _)| {
                documents
                    .get(fact_id)
                    .is_some_and(|frontmatter| frontmatter.name.is_empty())
            })
            .collect::<Vec<_>>();
        if invalid.is_empty() {
            continue;
        }
        let agent_ids = invalid
            .iter()
            .filter_map(|(fact_id, _)| positions.get(fact_id))
            .filter_map(|index| {
                plan.read_plan
                    .roots
                    .get(snapshot.facts[*index].root_index as usize)
            })
            .flat_map(|root| root.consumer_agent_ids.iter().cloned())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        for (fact_id, _) in &invalid {
            documents.remove(fact_id);
        }
        issues.push(SkillReadIssue {
            code: "invalidFrontmatter".to_string(),
            path: invalid[0].1.clone(),
            agent_ids,
        });
    }
    Ok(())
}

pub(crate) async fn project_direct_skill_snapshot(
    plan: &SkillReadPlan,
    mut snapshot: RawFilesystemSnapshot,
    runtime: &AgentRuntimeSnapshot,
    libraries: &dyn crate::application::skill_libraries::SkillLibraryRepository,
    targets: &dyn crate::environment::planning::TargetFactResolver,
    metadata: &dyn SkillMetadataSource,
) -> Result<ListSkillsResult, AppError> {
    use crate::application::installed_skill_resolver::SkillDirectoryName;
    use crate::application::library_candidates::ResolvedLibraryCandidateIndex;
    use crate::core::skill::InstalledLibraryVersion;

    let (mut documents, resolved_by_fact, mut metadata_issues, missing_name_groups) =
        read_skill_documents(plan, &mut snapshot, targets, metadata).await?;
    let single_files =
        project_recorded_eve_entries(plan, &mut snapshot, &mut documents, runtime, targets).await?;
    reject_unidentified_skill_documents(
        plan,
        &snapshot,
        &mut documents,
        missing_name_groups,
        &mut metadata_issues,
    )?;
    let mut entries = Vec::new();
    for fact in &snapshot.facts {
        let Some(relative) = fact.relative_path.strip_suffix("/SKILL.md") else {
            continue;
        };
        let fact_id = skill_fact_id(fact);
        let Some(frontmatter) = documents.get(&fact_id) else {
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
            resolved_by_fact
                .get(&fact_id)
                .cloned()
                .ok_or(AppError::StaleTarget)?,
        ));
    }
    let names = entries
        .iter()
        .map(|(_, name, _, _)| name.clone())
        .collect::<BTreeSet<_>>();
    let catalog = libraries.load(&snapshot.environment).await?;
    let known = ResolvedLibraryCandidateIndex::from_catalog(
        libraries,
        targets,
        &snapshot.environment,
        &names,
        &catalog,
    )
    .await?;
    let mut references = BTreeMap::<SkillDirectoryName, Vec<InstalledLibraryVersion>>::new();
    let mut record_reference =
        |name: &SkillDirectoryName,
         owner: &crate::application::library_candidates::LibraryVersionCandidate|
         -> Result<(), AppError> {
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
            Ok(())
        };
    for fact in snapshot.facts.iter().filter(|fact| {
        fact.relative_path.is_empty()
            && matches!(
                fact.kind,
                FilesystemEntryKind::Symlink | FilesystemEntryKind::ReparsePoint
            )
    }) {
        let Some(root) = plan.read_plan.roots.get(fact.root_index as usize) else {
            return Err(AppError::StaleTarget);
        };
        if !plan.owners.contains_key(&root.locator.native_path) {
            continue;
        }
        let Some(identity) = fact.resolved_target.as_deref().and_then(|target| {
            crate::environment::planning::resolve_link_target_identity(&root.locator, target)
        }) else {
            continue;
        };
        for owner in known.members_for_root_link(&identity, &names) {
            let name = SkillDirectoryName::try_from(owner.member_name())?;
            record_reference(&name, owner)?;
        }
    }
    let mut excluded = BTreeSet::new();
    for (id, name, _, fact) in &entries {
        if let Some(owner) = known.owner(name, fact) {
            excluded.insert(id.clone());
            record_reference(name, owner)?;
        } else if matches!(
            fact.entry_kind,
            crate::environment::planning::TargetEntryKind::Symlink
                | crate::environment::planning::TargetEntryKind::Junction
        ) {
            let direct = entries
                .iter()
                .filter(|(_, candidate_name, _, target)| {
                    candidate_name == name
                        && target.entry_kind
                            == crate::environment::planning::TargetEntryKind::Directory
                        && known.owner(name, target).is_none()
                })
                .map(|(_, _, _, target)| target)
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
    let mut result = project_skill_snapshot_with_documents(
        plan,
        snapshot,
        runtime,
        &documents,
        metadata_issues,
    )?;
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
    documents: &mut SkillDocuments,
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
            documents.remove(&skill_fact_id(&snapshot.facts[*fact_index]));
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
        let fact_id = skill_fact_id(&snapshot.facts[*fact_index]);
        // 仅为读取投影补充已确认的身份；磁盘中的 Eve 文件保持原样。
        if let Some(frontmatter) = documents.get(&fact_id) {
            let mut frontmatter = frontmatter.as_ref().clone();
            frontmatter.name = name.clone();
            documents.insert(fact_id, Arc::new(frontmatter));
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
    }
    Ok(single_files)
}

#[cfg(test)]
pub fn project_skill_snapshot(
    plan: &SkillReadPlan,
    mut snapshot: RawFilesystemSnapshot,
    runtime: &AgentRuntimeSnapshot,
) -> Result<ListSkillsResult, AppError> {
    let mut documents = SkillDocuments::new();
    for fact in &mut snapshot.facts {
        if fact.frontmatter_bytes.is_empty() {
            continue;
        }
        if let Some(frontmatter) = parse_skill_frontmatter(&fact.frontmatter_bytes) {
            documents.insert(skill_fact_id(fact), Arc::new(frontmatter));
        } else {
            fact.error_code = Some("invalidFrontmatter".to_string());
        }
    }
    project_skill_snapshot_with_documents(plan, snapshot, runtime, &documents, Vec::new())
}

fn project_skill_snapshot_with_documents(
    plan: &SkillReadPlan,
    snapshot: RawFilesystemSnapshot,
    runtime: &AgentRuntimeSnapshot,
    documents: &SkillDocuments,
    initial_issues: Vec<SkillReadIssue>,
) -> Result<ListSkillsResult, AppError> {
    if !same_environment_identity(&snapshot.environment, &plan.read_plan.context.environment) {
        return Err(AppError::ConfigurationCorrupted {
            message: "Skill read snapshot belongs to another Environment".to_string(),
        });
    }
    let mut read_issues = SkillReadIssueCollector::default();
    for issue in initial_issues {
        read_issues.record(issue.code, issue.path, issue.agent_ids);
    }
    for fact in &snapshot.facts {
        let Some(code) = fact
            .error_code
            .clone()
            .or_else(|| fact.truncated.then(|| "frontmatterTooLarge".to_string()))
        else {
            continue;
        };
        let root = plan
            .read_plan
            .roots
            .get(fact.root_index as usize)
            .ok_or(AppError::StaleTarget)?;
        let path = if fact.relative_path.is_empty() {
            root.locator.clone()
        } else {
            root.locator.join_child(&fact.relative_path)
        };
        read_issues.record(code, path, root.consumer_agent_ids.iter().cloned());
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
        let fact_id = skill_fact_id(fact);
        let Some(frontmatter) = documents.get(&fact_id) else {
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
        read_status: Some(read_issues.finish()),
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

#[cfg(test)]
fn parse_skill_frontmatter(bytes: &[u8]) -> Option<SkillFrontmatter> {
    let frontmatter = parse_skill_frontmatter_allow_missing_name(bytes)?;
    (!frontmatter.name.is_empty()).then_some(frontmatter)
}

fn parse_skill_frontmatter_allow_missing_name(bytes: &[u8]) -> Option<SkillFrontmatter> {
    let content = std::str::from_utf8(bytes).ok()?;
    let rest = content.strip_prefix("---")?;
    let end = rest.find("---")?;
    let frontmatter: SkillFrontmatter = serde_yaml::from_str(rest[..end].trim()).ok()?;
    (!frontmatter.description.is_empty()).then_some(frontmatter)
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
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    use super::{build_skill_read_plan, project_skill_snapshot};
    use crate::core::agent_definition::{
        AgentAdapter, AgentDefinition, AgentId, AgentSource, DetectionSpec, PathSpec,
        ScopeDefinition,
    };
    use crate::environment::agent_environment::{
        AgentRuntimeSnapshot, DetectionState, ResolvedAgent, ResolvedAgentScope,
    };
    use crate::environment::context_resolver::ResolvedContext;
    use crate::environment::inspection::{
        FilesystemEntryKind, FilesystemInspector, MetadataFuture, RawFilesystemSnapshot,
        RawPathFact, RawSkillMetadata, SkillMetadataSource,
    };
    use crate::environment::native::inspection::NativeInspector;
    use crate::environment::planning::{
        ResolvedTargetFact, RuntimeTargetFactResolver, TargetEntryKind, TargetFactFuture,
        TargetFactResolver,
    };
    use crate::environment::runtime::{
        EntryFingerprint, ExecutionBackend, PhysicalParentIdentity, PhysicalTargetKey,
    };
    use crate::environment::types::{
        EnvironmentRef, EnvironmentStatus, RegisteredProject, ResourceLocator, SkillLocation,
        SkillLocationRef, StorageAccess,
    };
    use crate::error::AppError;

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
            fingerprint: None,
            frontmatter_bytes: Vec::new(),
            truncated: false,
            error_code: None,
        }
    }

    #[derive(Clone)]
    struct CountingTargets {
        inner: RuntimeTargetFactResolver,
        batch_sizes: Arc<Mutex<Vec<usize>>>,
    }

    impl TargetFactResolver for CountingTargets {
        fn resolve<'a>(
            &'a self,
            context: &'a SkillLocationRef,
            logical_destinations: &'a [ResourceLocator],
            cancellation: Option<crate::core::mutation::CancellationSignal>,
        ) -> TargetFactFuture<'a, Result<Vec<ResolvedTargetFact>, AppError>> {
            self.batch_sizes
                .lock()
                .unwrap()
                .push(logical_destinations.len());
            self.inner
                .resolve(context, logical_destinations, cancellation)
        }
    }

    #[cfg(unix)]
    struct CountingMetadata {
        inner: NativeInspector,
        batch_sizes: Arc<Mutex<Vec<usize>>>,
    }

    #[cfg(unix)]
    impl SkillMetadataSource for CountingMetadata {
        fn read<'a>(
            &'a self,
            locators: &'a [ResourceLocator],
            per_file_limit: u32,
        ) -> MetadataFuture<'a, Result<Vec<RawSkillMetadata>, AppError>> {
            self.batch_sizes.lock().unwrap().push(locators.len());
            self.inner.read(locators, per_file_limit)
        }
    }

    struct SyntheticScaleTargets;

    impl TargetFactResolver for SyntheticScaleTargets {
        fn resolve<'a>(
            &'a self,
            _context: &'a SkillLocationRef,
            logical_destinations: &'a [ResourceLocator],
            _cancellation: Option<crate::core::mutation::CancellationSignal>,
        ) -> TargetFactFuture<'a, Result<Vec<ResolvedTargetFact>, AppError>> {
            Box::pin(async move {
                logical_destinations
                    .iter()
                    .map(|destination| {
                        let name = destination.native_path.rsplit('/').next().ok_or_else(|| {
                            AppError::Validation {
                                field: Some("scaleTarget".to_string()),
                                message: "missing synthetic Skill name".to_string(),
                            }
                        })?;
                        let index = name
                            .strip_prefix("skill-")
                            .and_then(|value| value.parse::<u64>().ok())
                            .unwrap_or_default();
                        Ok(ResolvedTargetFact {
                            key: PhysicalTargetKey {
                                backend: ExecutionBackend::NativeUnix,
                                physical_parent: PhysicalParentIdentity::Unix {
                                    device: 1,
                                    inode: index + 1,
                                },
                                normalized_final_child_name: name.to_string(),
                            },
                            destination: destination.clone(),
                            storage_access: StorageAccess::Native,
                            fingerprint: EntryFingerprint(format!("scale-{index}")),
                            entry_kind: TargetEntryKind::Directory,
                            link_target: None,
                            link_target_identity: None,
                        })
                    })
                    .collect()
            })
        }
    }

    struct SyntheticScaleMetadata {
        reads: Arc<AtomicUsize>,
    }

    impl SkillMetadataSource for SyntheticScaleMetadata {
        fn read<'a>(
            &'a self,
            locators: &'a [ResourceLocator],
            _per_file_limit: u32,
        ) -> MetadataFuture<'a, Result<Vec<RawSkillMetadata>, AppError>> {
            self.reads.fetch_add(locators.len(), Ordering::SeqCst);
            Box::pin(async move {
                Ok(locators
                    .iter()
                    .map(|locator| {
                        let name = locator
                            .native_path
                            .trim_end_matches("/SKILL.md")
                            .rsplit('/')
                            .next()
                            .unwrap();
                        RawSkillMetadata {
                            locator: locator.clone(),
                            bytes: format!(
                                "---\nname: {name}\ndescription: Synthetic {name}\n---\n"
                            )
                            .into_bytes(),
                            truncated: false,
                            error_code: None,
                        }
                    })
                    .collect())
            })
        }
    }

    fn skill_facts(root_index: u32) -> [RawPathFact; 2] {
        [
            RawPathFact {
                root_index,
                relative_path: "toolkit".to_string(),
                kind: FilesystemEntryKind::Directory,
                resolved_target: None,
                fingerprint: None,
                frontmatter_bytes: Vec::new(),
                truncated: false,
                error_code: None,
            },
            RawPathFact {
                root_index,
                relative_path: "toolkit/SKILL.md".to_string(),
                kind: FilesystemEntryKind::File,
                resolved_target: None,
                fingerprint: None,
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
                fingerprint: None,
                frontmatter_bytes: Vec::new(),
                truncated: false,
                error_code: None,
            });
            facts.push(RawPathFact {
                root_index: canonical_index,
                relative_path: format!("{directory}/SKILL.md"),
                kind: FilesystemEntryKind::File,
                resolved_target: None,
                fingerprint: None,
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

    #[test]
    fn skill_snapshot_keeps_valid_cards_when_another_frontmatter_is_invalid() {
        let environment = EnvironmentRef::Native;
        let context = context(environment.clone());
        let runtime = runtime(environment.clone());
        let plan = build_skill_read_plan(&context, &runtime, &[]).unwrap();
        let canonical_index = root_index(&plan, "/work/app/.agents/skills");
        let mut facts = vec![root_fact(canonical_index)];
        facts.extend(skill_facts(canonical_index));
        facts.push(RawPathFact {
            root_index: canonical_index,
            relative_path: "broken".to_string(),
            kind: FilesystemEntryKind::Directory,
            resolved_target: None,
            fingerprint: None,
            frontmatter_bytes: Vec::new(),
            truncated: false,
            error_code: None,
        });
        facts.push(RawPathFact {
            root_index: canonical_index,
            relative_path: "broken/SKILL.md".to_string(),
            kind: FilesystemEntryKind::File,
            resolved_target: None,
            fingerprint: None,
            frontmatter_bytes: b"---\ndescription: Missing name\n---\n".to_vec(),
            truncated: false,
            error_code: None,
        });

        let result = project_skill_snapshot(
            &plan,
            RawFilesystemSnapshot {
                environment,
                total_content_bytes: facts
                    .iter()
                    .map(|fact| fact.frontmatter_bytes.len() as u32)
                    .sum(),
                facts,
            },
            &runtime,
        )
        .unwrap();

        assert_eq!(result.skills.len(), 1);
        let status = result.read_status.unwrap();
        assert!(!status.complete);
        assert_eq!(status.counts[0].code, "invalidFrontmatter");
        assert_eq!(status.counts[0].count, 1);
        assert_eq!(status.omitted_count, 0);
    }

    #[tokio::test]
    async fn direct_skill_projection_resolves_only_observed_entries() {
        use crate::environment::wsl::WslRuntime;

        let temp = tempfile::tempdir().unwrap();
        let project_root = temp.path();
        let canonical_root = project_root.join(".agents/skills");
        std::fs::create_dir_all(&canonical_root).unwrap();

        let environment = EnvironmentRef::Native;
        let mut context = context(environment.clone());
        context.project.as_mut().unwrap().native_path = project_root.to_string_lossy().into_owned();
        context.skill_root.native_path = canonical_root.to_string_lossy().into_owned();
        context.lock.native_path = project_root
            .join("skills-lock.json")
            .to_string_lossy()
            .into_owned();

        let template = runtime(environment.clone())
            .agents
            .into_values()
            .next()
            .unwrap();
        let mut runtime = runtime(environment.clone());
        runtime.project_path = Some(project_root.to_string_lossy().into_owned());
        runtime.agents.clear();
        for index in 0..20 {
            let id = AgentId::parse(format!("custom-{index}")).unwrap();
            let relative_root = format!(".custom-{index}/skills");
            let private_root = project_root.join(&relative_root);
            let skill_name = format!("skill-{index}");
            std::fs::create_dir_all(private_root.join(&skill_name)).unwrap();
            std::fs::write(
                private_root.join(&skill_name).join("SKILL.md"),
                format!("---\nname: {skill_name}\ndescription: Skill {index}\n---\n"),
            )
            .unwrap();

            let mut agent = template.clone();
            agent.definition.id = id.clone();
            agent.definition.display_name = format!("Custom {index}");
            agent.definition.project.private_path = Some(PathSpec::project(&relative_root));
            agent.project.standard_path = Some(canonical_root.to_string_lossy().into_owned());
            agent.project.private_path = Some(private_root.to_string_lossy().into_owned());
            runtime.agents.insert(id, agent);
        }

        let plan = build_skill_read_plan(&context, &runtime, &[]).unwrap();
        let inspector = NativeInspector::new(environment.clone());
        let snapshot = inspector.inspect(&plan.read_plan).await.unwrap();
        let libraries =
            crate::native_workflow_integration_support::update_library_repository(temp.path());
        let batch_sizes = Arc::new(Mutex::new(Vec::new()));
        let targets = CountingTargets {
            inner: RuntimeTargetFactResolver::new(Arc::new(WslRuntime::default())),
            batch_sizes: batch_sizes.clone(),
        };

        let result = super::project_direct_skill_snapshot(
            &plan,
            snapshot,
            &runtime,
            libraries.as_ref(),
            &targets,
            &inspector,
        )
        .await
        .unwrap();

        assert_eq!(result.skills.len(), 20);
        assert_eq!(*batch_sizes.lock().unwrap(), vec![20]);
    }

    #[tokio::test]
    #[ignore = "large-scale Skill read contract"]
    async fn scale_projection_deduplicates_ten_thousand_placements_to_one_thousand_reads() {
        const AGENTS: usize = 100;
        const SKILLS: usize = 1_000;
        const PLACEMENTS_PER_ROOT: usize = 100;

        let environment = EnvironmentRef::Native;
        let context = context(environment.clone());
        let template = runtime(environment.clone())
            .agents
            .into_values()
            .next()
            .unwrap();
        let mut runtime = runtime(environment.clone());
        runtime.agents.clear();
        for index in 0..AGENTS {
            let id = AgentId::parse(format!("scale-agent-{index:03}")).unwrap();
            let relative_root = if index == 0 {
                ".agents/skills".to_string()
            } else {
                format!(".scale-agent-{index:03}/skills")
            };
            let private_root = format!("/work/app/{relative_root}");
            let mut agent = template.clone();
            agent.definition.id = id.clone();
            agent.definition.display_name = format!("Scale Agent {index:03}");
            agent.definition.project.private_path = Some(PathSpec::project(&relative_root));
            agent.project.standard_path = Some("/work/app/.agents/skills".to_string());
            agent.project.private_path = Some(private_root);
            runtime.agents.insert(id, agent);
        }

        let plan = build_skill_read_plan(&context, &runtime, &[]).unwrap();
        let mut facts = plan
            .read_plan
            .roots
            .iter()
            .enumerate()
            .map(|(index, _)| root_fact(index as u32))
            .collect::<Vec<_>>();
        let skill_roots = plan
            .read_plan
            .roots
            .iter()
            .enumerate()
            .filter(|(_, root)| root.locator.native_path != "/work/app")
            .map(|(index, _)| index as u32)
            .collect::<Vec<_>>();
        assert_eq!(skill_roots.len(), AGENTS);
        for (root_position, root_index) in skill_roots.into_iter().enumerate() {
            for offset in 0..PLACEMENTS_PER_ROOT {
                let skill_index = (root_position * PLACEMENTS_PER_ROOT + offset) % SKILLS;
                let name = format!("skill-{skill_index:04}");
                facts.push(RawPathFact {
                    root_index,
                    relative_path: name.clone(),
                    kind: FilesystemEntryKind::Directory,
                    resolved_target: None,
                    fingerprint: None,
                    frontmatter_bytes: Vec::new(),
                    truncated: false,
                    error_code: None,
                });
                facts.push(RawPathFact {
                    root_index,
                    relative_path: format!("{name}/SKILL.md"),
                    kind: FilesystemEntryKind::File,
                    resolved_target: None,
                    fingerprint: Some(EntryFingerprint(format!("document-{skill_index:04}"))),
                    frontmatter_bytes: Vec::new(),
                    truncated: false,
                    error_code: None,
                });
            }
        }
        let reads = Arc::new(AtomicUsize::new(0));
        let metadata = SyntheticScaleMetadata {
            reads: reads.clone(),
        };
        let temp = tempfile::tempdir().unwrap();
        let libraries =
            crate::native_workflow_integration_support::update_library_repository(temp.path());

        let snapshot = RawFilesystemSnapshot {
            environment,
            facts,
            total_content_bytes: 0,
        };
        let result = super::project_direct_skill_snapshot(
            &plan,
            snapshot.clone(),
            &runtime,
            libraries.as_ref(),
            &SyntheticScaleTargets,
            &metadata,
        )
        .await
        .unwrap();

        assert_eq!(result.skills.len(), SKILLS);
        assert_eq!(reads.load(Ordering::SeqCst), SKILLS);
        assert!(result.read_status.unwrap().complete);

        let refreshed = super::project_direct_skill_snapshot(
            &plan,
            snapshot.clone(),
            &runtime,
            libraries.as_ref(),
            &SyntheticScaleTargets,
            &metadata,
        )
        .await
        .unwrap();
        assert_eq!(refreshed.skills.len(), SKILLS);
        assert_eq!(reads.load(Ordering::SeqCst), SKILLS);

        let mut changed = snapshot;
        for fact in &mut changed.facts {
            if fact.relative_path == "skill-0000/SKILL.md" {
                fact.fingerprint = Some(EntryFingerprint("document-0000-changed".to_string()));
            }
        }
        let changed_result = super::project_direct_skill_snapshot(
            &plan,
            changed,
            &runtime,
            libraries.as_ref(),
            &SyntheticScaleTargets,
            &metadata,
        )
        .await
        .unwrap();
        assert_eq!(changed_result.skills.len(), SKILLS);
        assert_eq!(reads.load(Ordering::SeqCst), SKILLS + 1);
    }

    #[cfg(target_os = "windows")]
    #[tokio::test]
    #[ignore = "requires SKILL_DECK_TEST_WSL_DISTRO and a matching real WSL Worker"]
    async fn real_wsl_skill_read_scale_p95_is_within_five_seconds() {
        use std::time::{Duration, Instant};

        use crate::environment::wsl::WslRuntime;

        const AGENTS: usize = 100;
        const SAMPLES: usize = 20;
        const SKILLS: usize = 1_000;

        let distro_name =
            std::env::var("SKILL_DECK_TEST_WSL_DISTRO").expect("set SKILL_DECK_TEST_WSL_DISTRO");
        let fixture_root = format!("/tmp/skill-deck-skill-read-scale-{}", uuid::Uuid::new_v4());
        let wsl = Arc::new(WslRuntime::for_wsl_test());
        wsl.connect(&distro_name).await.expect("connect WSL Worker");
        wsl.run_test_script(
            &distro_name,
            r#"set -eu
root=$1
mkdir -p "$root/content" "$root/project"
i=0
while [ "$i" -lt 1000 ]; do
  name=$(printf 'skill-%04d' "$i")
  mkdir -p "$root/content/$name"
  printf '%s\n' '---' "name: $name" "description: Synthetic $name" '---' > "$root/content/$name/SKILL.md"
  i=$((i + 1))
done
r=0
while [ "$r" -lt 100 ]; do
  root_name=$(printf 'root-%03d' "$r")
  mkdir -p "$root/roots/$root_name"
  k=0
  while [ "$k" -lt 100 ]; do
    skill=$(((r * 100 + k) % 1000))
    name=$(printf 'skill-%04d' "$skill")
    ln -s "$root/content/$name" "$root/roots/$root_name/$name"
    k=$((k + 1))
  done
  r=$((r + 1))
done
"#,
            vec![fixture_root.clone()],
            Duration::from_secs(60),
        )
        .await
        .expect("create scale fixture");

        let environment = EnvironmentRef::Wsl {
            distro_name: distro_name.clone(),
        };
        let mut resolved_context = context(environment.clone());
        resolved_context.project.as_mut().unwrap().native_path = format!("{fixture_root}/project");
        resolved_context.skill_root.native_path = format!("{fixture_root}/roots/root-000");
        resolved_context.lock.native_path = format!("{fixture_root}/project/skills-lock.json");
        let template = runtime(environment.clone())
            .agents
            .into_values()
            .next()
            .unwrap();
        let mut agent_runtime = runtime(environment.clone());
        agent_runtime.project_path = Some(format!("{fixture_root}/project"));
        agent_runtime.agents.clear();
        for index in 0..AGENTS {
            let id = AgentId::parse(format!("scale-agent-{index:03}")).unwrap();
            let root = format!("{fixture_root}/roots/root-{index:03}");
            let mut agent = template.clone();
            agent.definition.id = id.clone();
            agent.definition.display_name = format!("Scale Agent {index:03}");
            agent.definition.project.private_path =
                Some(PathSpec::project(format!(".scale-agent-{index:03}/skills")));
            agent.project.standard_path = Some(resolved_context.skill_root.native_path.clone());
            agent.project.private_path = Some(root);
            agent_runtime.agents.insert(id, agent);
        }
        let base_plan = build_skill_read_plan(&resolved_context, &agent_runtime, &[]).unwrap();
        assert_eq!(base_plan.read_plan.roots.len(), AGENTS + 1);
        let workspace = wsl.workspace(&distro_name).unwrap();
        let inspector =
            crate::environment::wsl::operations::inspection::WslInspector::new(workspace);
        let targets = RuntimeTargetFactResolver::new(wsl.clone());
        let mut elapsed = Vec::with_capacity(SAMPLES);
        let measured = async {
            for sample in 0..SAMPLES {
                let mut plan = base_plan.clone();
                plan.read_plan.registry_revision = format!("scale-registry-{sample}");
                let started = Instant::now();
                let mut snapshot = inspector.inspect(&plan.read_plan).await?;
                let (documents, _, metadata_issues, _) =
                    super::read_skill_documents(&plan, &mut snapshot, &targets, &inspector).await?;
                let result = super::project_skill_snapshot_with_documents(
                    &plan,
                    snapshot,
                    &agent_runtime,
                    &documents,
                    metadata_issues,
                )?;
                if result.skills.len() != SKILLS {
                    return Err(AppError::ConfigurationCorrupted {
                        message: format!(
                            "scale fixture projected {} Skills instead of {SKILLS}",
                            result.skills.len()
                        ),
                    });
                }
                elapsed.push(started.elapsed());
            }
            Ok::<_, AppError>(())
        }
        .await;

        wsl.run_test_script(
            &distro_name,
            "set -eu\ncase $1 in /tmp/skill-deck-skill-read-scale-*) rm -rf -- \"$1\" ;; *) exit 64 ;; esac\n",
            vec![fixture_root],
            Duration::from_secs(30),
        )
        .await
        .expect("clean scale fixture");
        measured.expect("execute scale reads");

        elapsed.sort_unstable();
        let p95 = elapsed[(SAMPLES * 95).div_ceil(100) - 1];
        eprintln!("real WSL Skill read samples: {elapsed:?}; p95={p95:?}");
        assert!(p95 <= Duration::from_secs(5), "P95 was {p95:?}");
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
        let inspector = NativeInspector::new(environment);
        let snapshot = inspector.inspect(&plan.read_plan).await.unwrap();
        let targets = RuntimeTargetFactResolver::new(Arc::new(
            crate::environment::wsl::WslRuntime::default(),
        ));
        let libraries =
            crate::native_workflow_integration_support::update_library_repository(temp.path());
        let metadata_batches = Arc::new(Mutex::new(Vec::new()));
        let metadata = CountingMetadata {
            inner: NativeInspector::new(EnvironmentRef::Native),
            batch_sizes: metadata_batches.clone(),
        };
        let result = super::project_direct_skill_snapshot(
            &plan,
            snapshot,
            &runtime,
            libraries.as_ref(),
            &targets,
            &metadata,
        )
        .await
        .unwrap();

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
        assert_eq!(*metadata_batches.lock().unwrap(), vec![1]);
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
        let inspector = NativeInspector::new(EnvironmentRef::Native);
        let snapshot = inspector.inspect(&plan.read_plan).await.unwrap();
        let result = super::project_direct_skill_snapshot(
            &plan,
            snapshot,
            &runtime,
            libraries.as_ref(),
            &targets,
            &inspector,
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
        let snapshot = inspector.inspect(&plan.read_plan).await.unwrap();
        let result = super::project_direct_skill_snapshot(
            &plan,
            snapshot,
            &runtime,
            libraries.as_ref(),
            &targets,
            &inspector,
        )
        .await
        .unwrap();
        assert_eq!(result.skills[0].description, "Direct description");
        assert_eq!(
            result.skills[0].library_versions.as_ref().unwrap()[0].library_id,
            "library-one"
        );

        std::fs::remove_dir_all(private_root.join("toolkit")).unwrap();
        let snapshot = inspector.inspect(&plan.read_plan).await.unwrap();
        let result = super::project_direct_skill_snapshot(
            &plan,
            snapshot,
            &runtime,
            libraries.as_ref(),
            &targets,
            &inspector,
        )
        .await
        .unwrap();
        assert!(result.skills.is_empty());
    }
}

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use serde_json::{json, Map, Value};

use crate::application::install_planner::{InstallPlanningFactSource, InstallPlanningFacts};
use crate::application::mutation::plan::{
    group_physical_mutations, stable_digest, ExpectedTargetEntry, MutationPlan,
    PreparedEntryAction, PreparedEntryMutation, PreviewToken,
};
use crate::application::mutation::planning::{
    assemble_plan, issue_preview_token, MutationPlanDraft, MutationUnitDraft,
    PreparedMutationEntries, PreviewTokenDraft,
};
use crate::application::mutation::result::OperationErrorCode;
use crate::application::payload_session::{
    AcquiredPayloadHandle, PayloadSessionManager, PinnedPayloadLease,
};
use crate::application::remove::{ObservedEntryKind, ObservedEntryOwner, ObservedPhysicalEntry};
use crate::application::update::{
    derive_update_capability_from_metadata, CheckUpdateCapability, UpdateFuture, UpdatePlanner,
    UpdateRequest,
};
use crate::core::agent_definition::{AgentAdapter, AgentId};
use crate::core::lossless_lock::LockSchema;
use crate::core::mutation::MutationKind;
use crate::core::update_metadata::recover_source_url;
use crate::environment::agent_environment::{
    AgentRuntimeSnapshot, DetectionState, ResolvedAgentScope,
};
use crate::environment::content_manifest::{
    ContentManifestHash, ContentManifestReader, ContentManifestTarget,
};
use crate::environment::planning::{ResolvedTargetFact, TargetEntryKind, TargetFactResolver};
use crate::environment::runtime::{observed_entry_id, ObservedEntryId, PhysicalTargetKey};
use crate::environment::types::{
    same_environment_identity, EnvironmentRef, ResourceLocator, SkillLocation,
};
use crate::error::AppError;
use crate::models::InstallMode;
use crate::storage::lock_plan::{LockEntryMutation, LockExpectedState, PreparedLockMutation};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LockedUpdateSkill {
    pub name: String,
    pub lock_key: String,
    pub source: String,
    pub source_type: String,
    pub source_url: Option<String>,
    pub ref_name: Option<String>,
    pub skill_path: String,
    pub remote_hash: Option<String>,
    pub computed_hash: Option<String>,
    pub installed_at: Option<String>,
    pub subagents: Option<Vec<String>>,
    pub well_known_digest: Option<String>,
}

#[derive(Debug, Clone)]
pub struct LocalUpdateInspection {
    pub token: PreviewToken,
    pub source_candidates: Vec<LockedUpdateSkill>,
    pub skills: Vec<LocalUpdateSkillInspection>,
}

#[derive(Debug, Clone)]
pub struct LocalUpdateSkillInspection {
    pub skill_name: String,
    pub observed_digest: String,
    pub adapter_targets: Vec<ObservedEntryOwner>,
    pub(crate) clean_copies: Vec<ObservedPhysicalEntry>,
    pub conflicts: Vec<ObservedPhysicalEntry>,
    pub blocking_reasons: Vec<OperationErrorCode>,
}

impl LockedUpdateSkill {
    pub(crate) fn metadata(&self) -> crate::core::NormalizedUpdateMetadata {
        crate::core::NormalizedUpdateMetadata {
            source: self.source.clone(),
            source_type: self.source_type.clone(),
            source_url: self.source_url.clone(),
            ref_name: self.ref_name.clone(),
            skill_path: Some(self.skill_path.clone()),
            remote_hash: self.remote_hash.clone(),
            computed_hash: self.computed_hash.clone(),
            well_known_digest: self.well_known_digest.clone(),
        }
    }

    pub(crate) fn capability(&self) -> CheckUpdateCapability {
        derive_update_capability_from_metadata(&self.metadata())
    }
}

pub struct ConcreteUpdatePlanner<F, T> {
    facts: F,
    targets: T,
    payloads: Arc<PayloadSessionManager>,
    now: Arc<dyn Fn() -> String + Send + Sync>,
}

impl<F, T> ConcreteUpdatePlanner<F, T> {
    pub fn new(
        facts: F,
        targets: T,
        payloads: Arc<PayloadSessionManager>,
        now: impl Fn() -> String + Send + Sync + 'static,
    ) -> Self {
        Self {
            facts,
            targets,
            payloads,
            now: Arc::new(now),
        }
    }
}

impl<F, T> UpdatePlanner for ConcreteUpdatePlanner<F, T>
where
    F: InstallPlanningFactSource,
    T: TargetFactResolver + ContentManifestReader,
{
    fn inspect<'a>(
        &'a self,
        request: &'a UpdateRequest,
    ) -> UpdateFuture<'a, Result<LocalUpdateInspection, AppError>> {
        Box::pin(ConcreteUpdatePlanner::inspect(self, request))
    }

    fn build<'a>(
        &'a self,
        execution: &'a crate::application::update::UpdateExecutionRequest,
        handles: Vec<AcquiredPayloadHandle>,
        payloads: Vec<PinnedPayloadLease>,
    ) -> UpdateFuture<'a, Result<(PreviewToken, MutationPlan), AppError>> {
        Box::pin(async move {
            let request = &execution.request;
            let facts = self.facts.current(&request.context).await?;
            let locked = locked_skills(request, &facts)?;
            self.build_plan(
                request,
                &execution.overwrite_private_entries,
                facts,
                locked,
                handles,
                payloads,
            )
            .await
        })
    }
}

#[derive(Clone)]
enum CandidateKind {
    Canonical,
    Private {
        target_id: String,
        owner: ObservedEntryOwner,
    },
    Adapter {
        owner: ObservedEntryOwner,
    },
}

struct SkillSeed {
    locked_index: usize,
    fact_start: usize,
    fact_count: usize,
    candidates: Vec<CandidateKind>,
}

impl<F, T> ConcreteUpdatePlanner<F, T>
where
    F: InstallPlanningFactSource,
    T: TargetFactResolver,
{
    pub async fn inspect(&self, request: &UpdateRequest) -> Result<LocalUpdateInspection, AppError>
    where
        T: ContentManifestReader,
    {
        let facts = self.facts.current(&request.context).await?;
        let locked = locked_skills(request, &facts)?;
        let (destinations, seeds) = planning_seeds(&facts, request, &locked)?;
        let target_facts = self
            .targets
            .resolve(&request.context, &destinations, None)
            .await?;
        if target_facts.len() != destinations.len() {
            return Err(AppError::StaleTarget);
        }
        let mut manifests = BTreeMap::<PhysicalTargetKey, Option<ContentManifestHash>>::new();
        let mut skills = Vec::with_capacity(seeds.len());
        for seed in &seeds {
            let facts_slice = &target_facts[seed.fact_start..seed.fact_start + seed.fact_count];
            read_manifest_states(&self.targets, facts_slice, &mut manifests).await;
            let observed_digest =
                skill_observed_digest(facts_slice, &facts, &locked[seed.locked_index], &manifests)?;
            skills.push(
                self.inspect_skill_copies(
                    &locked[seed.locked_index],
                    facts_slice,
                    &seed.candidates,
                    &mut manifests,
                    observed_digest,
                )
                .await?,
            );
        }
        let token = inspection_token(request, &facts, &locked, &target_facts, &manifests)?;
        Ok(LocalUpdateInspection {
            token,
            source_candidates: locked,
            skills,
        })
    }

    async fn inspect_skill_copies(
        &self,
        locked: &LockedUpdateSkill,
        facts: &[ResolvedTargetFact],
        candidates: &[CandidateKind],
        manifests: &mut BTreeMap<PhysicalTargetKey, Option<ContentManifestHash>>,
        observed_digest: String,
    ) -> Result<LocalUpdateSkillInspection, AppError>
    where
        T: ContentManifestReader,
    {
        let canonical = facts.first().ok_or(AppError::StaleTarget)?;
        let canonical_hash = if canonical.entry_kind == TargetEntryKind::Directory {
            read_manifest_once(&self.targets, canonical, manifests).await
        } else {
            None
        };
        let mut clean = BTreeMap::<PhysicalTargetKey, ObservedPhysicalEntry>::new();
        let mut conflicts = BTreeMap::<PhysicalTargetKey, ObservedPhysicalEntry>::new();
        let mut adapter_targets = BTreeMap::<String, ObservedEntryOwner>::new();
        for (fact, candidate) in facts.iter().zip(candidates).skip(1) {
            let (target_id, owner) = match candidate {
                CandidateKind::Private { target_id, owner } => (target_id, owner),
                CandidateKind::Adapter { owner }
                    if fact.entry_kind == TargetEntryKind::Directory =>
                {
                    adapter_targets.insert(owner.logical_target_id.clone(), owner.clone());
                    continue;
                }
                CandidateKind::Canonical | CandidateKind::Adapter { .. } => continue,
            };
            if fact.key == canonical.key
                || matches!(
                    fact.entry_kind,
                    TargetEntryKind::Missing | TargetEntryKind::Symlink | TargetEntryKind::Junction
                )
            {
                continue;
            }
            let is_clean = if fact.entry_kind == TargetEntryKind::Directory {
                read_manifest_once(&self.targets, fact, manifests)
                    .await
                    .zip(canonical_hash.as_ref())
                    .is_some_and(|(observed, canonical)| observed == *canonical)
            } else {
                false
            };
            let grouped = if is_clean { &mut clean } else { &mut conflicts };
            insert_private_entry(grouped, fact, target_id, owner)?;
        }
        let clean_copies = clean.into_values().collect::<Vec<_>>();
        let blocking_reasons = (canonical.entry_kind == TargetEntryKind::Directory
            && canonical_hash.is_none())
        .then_some(OperationErrorCode::ConfigurationCorrupted)
        .into_iter()
        .collect();
        Ok(LocalUpdateSkillInspection {
            skill_name: locked.name.clone(),
            observed_digest,
            adapter_targets: adapter_targets.into_values().collect(),
            clean_copies,
            conflicts: conflicts.into_values().collect(),
            blocking_reasons,
        })
    }

    #[allow(clippy::too_many_arguments)]
    async fn build_plan(
        &self,
        request: &UpdateRequest,
        overwrite_private_entries: &[ObservedEntryId],
        facts: InstallPlanningFacts,
        locked: Vec<LockedUpdateSkill>,
        handles: Vec<AcquiredPayloadHandle>,
        payloads: Vec<PinnedPayloadLease>,
    ) -> Result<(PreviewToken, MutationPlan), AppError>
    where
        T: ContentManifestReader,
    {
        validate_payloads(request, &facts, &locked, &handles, &payloads)?;
        let mut payloads = payloads;
        let (destinations, seeds) = planning_seeds(&facts, request, &locked)?;
        let target_facts = self
            .targets
            .resolve(&request.context, &destinations, None)
            .await?;
        if target_facts.len() != destinations.len() {
            return Err(AppError::StaleTarget);
        }
        let mut manifests = BTreeMap::<PhysicalTargetKey, Option<ContentManifestHash>>::new();
        read_manifest_states(&self.targets, &target_facts, &mut manifests).await;
        let token = inspection_token(request, &facts, &locked, &target_facts, &manifests)?;
        let mut eve_payload_indexes = BTreeMap::new();
        for (seed_index, seed) in seeds.iter().enumerate() {
            let facts_slice = &target_facts[seed.fact_start..seed.fact_start + seed.fact_count];
            let needs_eve_payload =
                facts_slice
                    .iter()
                    .zip(&seed.candidates)
                    .any(|(fact, candidate)| {
                        matches!(candidate, CandidateKind::Adapter { .. })
                            && fact.entry_kind == TargetEntryKind::Directory
                    });
            if needs_eve_payload {
                let canonical = &payloads[seed.locked_index];
                let derived =
                    crate::core::eve::derive_eve_skill_payload(&canonical.load_payload().await?)?;
                let lease = self
                    .payloads
                    .pin_derived_payload(canonical, "eve-update", derived)
                    .await?;
                let index = payloads.len();
                payloads.push(lease);
                eve_payload_indexes.insert(seed_index, index);
            }
        }
        let selected = overwrite_private_entries
            .iter()
            .cloned()
            .collect::<BTreeSet<_>>();
        let mut all_selectable = BTreeSet::new();
        let mut units = Vec::with_capacity(seeds.len());
        for (seed_index, seed) in seeds.iter().enumerate() {
            let locked = &locked[seed.locked_index];
            let payload = &payloads[seed.locked_index];
            let facts_slice = &target_facts[seed.fact_start..seed.fact_start + seed.fact_count];
            let observed = private_entries(facts_slice, &seed.candidates)?;
            all_selectable.extend(observed.iter().map(|entry| entry.entry_id.clone()));
            units.push(build_unit(
                request,
                &facts,
                locked,
                payload,
                eve_payload_indexes
                    .get(&seed_index)
                    .map(|index| &payloads[*index]),
                facts_slice,
                &seed.candidates,
                &selected,
                &manifests,
                (self.now)(),
            )?);
        }
        if !selected.is_subset(&all_selectable) {
            return Err(AppError::StaleTarget);
        }
        let plan = assemble_plan(MutationPlanDraft {
            kind: MutationKind::Update,
            payloads: payloads
                .into_iter()
                .map(|lease| (lease.manifest().payload_id().clone(), lease))
                .collect(),
            units,
        });
        Ok((token, plan))
    }
}

fn locked_skills(
    request: &UpdateRequest,
    facts: &InstallPlanningFacts,
) -> Result<Vec<LockedUpdateSkill>, AppError> {
    request
        .skill_names
        .iter()
        .map(|name| {
            let resolved =
                crate::application::installed_skill_resolver::InstalledSkillResolver::resolve(
                    name,
                    &facts.lock_document,
                )?;
            let raw = facts
                .lock_document
                .entry_snapshot(&resolved.lock_key)
                .value()
                .cloned()
                .ok_or_else(|| AppError::InvalidSource {
                    value: format!("Skill '{name}' not found in lock file"),
                })?;
            locked_skill(name, &resolved.lock_key, facts.lock_schema, &raw)
        })
        .collect()
}

fn locked_skill(
    name: &str,
    lock_key: &str,
    schema: LockSchema,
    raw: &Value,
) -> Result<LockedUpdateSkill, AppError> {
    let object = raw
        .as_object()
        .ok_or_else(|| AppError::ConfigurationCorrupted {
            message: format!("Skill '{name}' lock entry must be an object"),
        })?;
    let source = string_field(object, "source").unwrap_or_default();
    let source_type = string_field(object, "sourceType").unwrap_or_default();
    let stored_source_url = if source_type == "well-known" && schema == LockSchema::Global {
        string_field(object, "sourceBaseUrl")
    } else {
        string_field(object, "sourceUrl")
    };
    let source_url = if source_type == "well-known" {
        stored_source_url
    } else {
        recover_source_url(&source, &source_type, stored_source_url.as_deref())
    };
    let skill_path = string_field(object, "skillPath").unwrap_or_default();
    let remote_hash = match schema {
        LockSchema::Global => string_field(object, "skillFolderHash"),
        LockSchema::Project => string_field(object, "remoteHash"),
    };
    let subagents = match (schema, object.get("subagents")) {
        (LockSchema::Global, _) | (LockSchema::Project, None) => None,
        (LockSchema::Project, Some(Value::Array(values))) => Some(
            values
                .iter()
                .map(|value| {
                    value.as_str().map(str::to_string).ok_or_else(|| {
                        AppError::ConfigurationCorrupted {
                            message: format!(
                                "Skill '{name}' Eve placement must contain only strings"
                            ),
                        }
                    })
                })
                .collect::<Result<Vec<_>, _>>()?,
        ),
        (LockSchema::Project, Some(_)) => {
            return Err(AppError::ConfigurationCorrupted {
                message: format!("Skill '{name}' Eve placement must be an array"),
            })
        }
    };
    let skill = LockedUpdateSkill {
        name: name.to_string(),
        lock_key: lock_key.to_string(),
        source,
        source_type,
        source_url,
        ref_name: string_field(object, "ref"),
        skill_path,
        remote_hash,
        computed_hash: (schema == LockSchema::Project)
            .then(|| string_field(object, "computedHash"))
            .flatten(),
        installed_at: string_field(object, "installedAt"),
        subagents,
        well_known_digest: string_field(object, "wellKnownDigest"),
    };
    if !skill.capability().can_run_update {
        return Err(AppError::InvalidSource {
            value: format!("Skill '{name}' cannot be updated from its lock metadata"),
        });
    }
    Ok(skill)
}

fn string_field(object: &Map<String, Value>, field: &str) -> Option<String> {
    object
        .get(field)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn validate_payloads(
    request: &UpdateRequest,
    facts: &InstallPlanningFacts,
    locked: &[LockedUpdateSkill],
    handles: &[AcquiredPayloadHandle],
    payloads: &[PinnedPayloadLease],
) -> Result<(), AppError> {
    let install_dir_names = locked
        .iter()
        .map(|skill| {
            crate::application::installed_skill_resolver::InstalledSkillResolver::install_dir_name(
                &skill.name,
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    if facts.resolved_context.context != request.context
        || !same_environment_identity(
            &facts.agent_runtime.environment,
            &request.context.environment,
        )
        || facts.agent_runtime.registry_revision != facts.revisions.registry
        || facts.agent_runtime.environment_revision != facts.revisions.environment
        || locked.len() != handles.len()
        || handles.len() != payloads.len()
        || payloads.iter().enumerate().any(|(index, payload)| {
            payload.planning_metadata().validate().is_err()
                || payload.planning_metadata().skill_name != locked[index].name
                || payload.planning_metadata().install_dir_name != install_dir_names[index]
                || !same_environment_identity(
                    &handles[index].environment,
                    &request.context.environment,
                )
        })
    {
        return Err(AppError::StaleContext);
    }
    Ok(())
}

fn private_entries(
    facts: &[ResolvedTargetFact],
    candidates: &[CandidateKind],
) -> Result<Vec<ObservedPhysicalEntry>, AppError> {
    let canonical_key = facts.first().map(|fact| &fact.key);
    let mut grouped = BTreeMap::<PhysicalTargetKey, ObservedPhysicalEntry>::new();
    for (fact, candidate) in facts.iter().zip(candidates) {
        let CandidateKind::Private { target_id, owner } = candidate else {
            continue;
        };
        if fact.entry_kind != TargetEntryKind::Directory
            || canonical_key.is_some_and(|canonical| canonical == &fact.key)
        {
            continue;
        }
        insert_private_entry(&mut grouped, fact, target_id, owner)?;
    }
    for entry in grouped.values_mut() {
        entry
            .owners
            .sort_by(|left, right| left.agent_id.cmp(&right.agent_id));
        entry
            .owners
            .dedup_by(|left, right| left.agent_id == right.agent_id);
    }
    Ok(grouped.into_values().collect())
}

fn planning_seeds(
    facts: &InstallPlanningFacts,
    request: &UpdateRequest,
    locked: &[LockedUpdateSkill],
) -> Result<(Vec<ResourceLocator>, Vec<SkillSeed>), AppError> {
    let mut destinations = Vec::new();
    let mut seeds = Vec::with_capacity(locked.len());
    for (index, skill) in locked.iter().enumerate() {
        let start = destinations.len();
        let mut candidates = vec![CandidateKind::Canonical];
        let install_dir_name =
            crate::application::installed_skill_resolver::InstalledSkillResolver::install_dir_name(
                &skill.name,
            )?;
        destinations.push(join_entry(
            &facts.resolved_context.skill_root,
            &install_dir_name,
        ));
        for (agent_id, agent) in &facts.agent_runtime.agents {
            if agent.definition.adapter != AgentAdapter::Standard {
                continue;
            }
            let scope = agent_scope(&facts.agent_runtime, agent_id, &request.context.scope)?;
            if !scope.enabled {
                continue;
            }
            let Some(root) = scope.private_path.as_deref() else {
                continue;
            };
            let target_id = format!("agent:{}:private", agent_id.as_str());
            destinations.push(join_entry(
                &ResourceLocator {
                    environment: request.context.environment.clone(),
                    native_path: root.to_string(),
                },
                &install_dir_name,
            ));
            candidates.push(CandidateKind::Private {
                target_id: target_id.clone(),
                owner: ObservedEntryOwner {
                    agent_id: agent_id.clone(),
                    display_name: agent.definition.display_name.clone(),
                    logical_target_id: target_id,
                },
            });
        }
        for (_target_id, root, owner) in
            eve_adapter_roots(&facts.agent_runtime, skill, &request.context.environment)?
        {
            destinations.push(join_entry(&root, &install_dir_name));
            candidates.push(CandidateKind::Adapter { owner });
        }
        seeds.push(SkillSeed {
            locked_index: index,
            fact_start: start,
            fact_count: candidates.len(),
            candidates,
        });
    }
    Ok((destinations, seeds))
}

fn inspection_token(
    request: &UpdateRequest,
    facts: &InstallPlanningFacts,
    locked: &[LockedUpdateSkill],
    target_facts: &[ResolvedTargetFact],
    manifests: &BTreeMap<PhysicalTargetKey, Option<ContentManifestHash>>,
) -> Result<PreviewToken, AppError> {
    issue_preview_token(PreviewTokenDraft {
        kind: MutationKind::Update,
        request,
        revisions: facts.revisions.clone(),
        observed_state_digest: observed_digest(target_facts, facts, locked, manifests)?,
        planner_contract_version: 1,
    })
}

async fn read_manifest_states<R: ContentManifestReader>(
    reader: &R,
    facts: &[ResolvedTargetFact],
    manifests: &mut BTreeMap<PhysicalTargetKey, Option<ContentManifestHash>>,
) {
    for fact in facts {
        if fact.entry_kind == TargetEntryKind::Directory {
            read_manifest_once(reader, fact, manifests).await;
        }
    }
}

async fn read_manifest_once<R: ContentManifestReader>(
    reader: &R,
    fact: &ResolvedTargetFact,
    manifests: &mut BTreeMap<PhysicalTargetKey, Option<ContentManifestHash>>,
) -> Option<ContentManifestHash> {
    if let Some(cached) = manifests.get(&fact.key) {
        return cached.clone();
    }
    let target = ContentManifestTarget {
        key: fact.key.clone(),
        location: fact.destination.clone(),
    };
    let hash = reader
        .read(&target)
        .await
        .ok()
        .map(|manifest| manifest.hash().clone());
    manifests.insert(fact.key.clone(), hash.clone());
    hash
}

fn insert_private_entry(
    grouped: &mut BTreeMap<PhysicalTargetKey, ObservedPhysicalEntry>,
    fact: &ResolvedTargetFact,
    target_id: &str,
    owner: &ObservedEntryOwner,
) -> Result<(), AppError> {
    let entry_id = observed_entry_id(&fact.key, &fact.fingerprint)?;
    let physical_target_key = stable_digest(&fact.key)?;
    let entry = grouped
        .entry(fact.key.clone())
        .or_insert_with(|| ObservedPhysicalEntry {
            entry_id,
            display_path: fact.destination.clone(),
            kind: observed_kind(fact.entry_kind),
            physical_target_key,
            owners: Vec::new(),
            will_break_if_canonical_removed: false,
        });
    let mut owner = owner.clone();
    owner.logical_target_id = target_id.to_string();
    entry.owners.push(owner);
    entry
        .owners
        .sort_by(|left, right| left.agent_id.cmp(&right.agent_id));
    entry
        .owners
        .dedup_by(|left, right| left.agent_id == right.agent_id);
    Ok(())
}

fn observed_kind(kind: TargetEntryKind) -> ObservedEntryKind {
    match kind {
        TargetEntryKind::Missing => ObservedEntryKind::Missing,
        TargetEntryKind::File | TargetEntryKind::Other => ObservedEntryKind::Other,
        TargetEntryKind::Directory => ObservedEntryKind::Directory,
        TargetEntryKind::Symlink => ObservedEntryKind::Symlink,
        TargetEntryKind::Junction => ObservedEntryKind::Junction,
        TargetEntryKind::BrokenLink => ObservedEntryKind::BrokenLink,
    }
}

#[allow(clippy::too_many_arguments)]
fn build_unit(
    request: &UpdateRequest,
    facts: &InstallPlanningFacts,
    locked: &LockedUpdateSkill,
    payload: &PinnedPayloadLease,
    eve_payload: Option<&PinnedPayloadLease>,
    target_facts: &[ResolvedTargetFact],
    candidates: &[CandidateKind],
    selected: &BTreeSet<ObservedEntryId>,
    manifests: &BTreeMap<PhysicalTargetKey, Option<ContentManifestHash>>,
    now: String,
) -> Result<MutationUnitDraft, AppError> {
    let canonical_fact = &target_facts[0];
    if matches!(
        canonical_fact.entry_kind,
        TargetEntryKind::File | TargetEntryKind::Other | TargetEntryKind::BrokenLink
    ) {
        return Err(AppError::UnsafePath {
            path: canonical_fact.destination.native_path.clone(),
            reason: "canonical Skill entry is not a directory or link".to_string(),
        });
    }
    let canonical_owners = standard_owners(&facts.agent_runtime, &request.context.scope);
    let canonical = PreparedEntryMutation {
        key: canonical_fact.key.clone(),
        destination: canonical_fact.destination.clone(),
        action: PreparedEntryAction::Replace {
            payload_id: payload.manifest().payload_id().clone(),
            requested_mode: InstallMode::Copy,
        },
        owner_agent_ids: canonical_owners,
    };
    let mut private = Vec::new();
    for (fact, candidate) in target_facts.iter().zip(candidates).skip(1) {
        match candidate {
            CandidateKind::Private { owner, .. }
                if fact.entry_kind == TargetEntryKind::Directory
                    && selected.contains(&observed_entry_id(&fact.key, &fact.fingerprint)?) =>
            {
                private.push(PreparedEntryMutation {
                    key: fact.key.clone(),
                    destination: fact.destination.clone(),
                    action: PreparedEntryAction::Replace {
                        payload_id: payload.manifest().payload_id().clone(),
                        requested_mode: InstallMode::Copy,
                    },
                    owner_agent_ids: vec![owner.agent_id.clone()],
                });
            }
            CandidateKind::Adapter { owner, .. }
                if fact.entry_kind == TargetEntryKind::Directory =>
            {
                private.push(PreparedEntryMutation {
                    key: fact.key.clone(),
                    destination: fact.destination.clone(),
                    action: PreparedEntryAction::Replace {
                        payload_id: eve_payload
                            .ok_or(AppError::StalePayload)?
                            .manifest()
                            .payload_id()
                            .clone(),
                        requested_mode: InstallMode::Copy,
                    },
                    owner_agent_ids: vec![owner.agent_id.clone()],
                });
            }
            _ => {}
        }
    }
    let grouped =
        group_physical_mutations(std::iter::once(canonical.clone()).chain(private).collect())?;
    let canonical_entry = grouped
        .iter()
        .find(|entry| entry.key == canonical.key)
        .cloned();
    let required_agent_entries = grouped
        .into_iter()
        .filter(|entry| entry.key != canonical.key)
        .collect();
    Ok(MutationUnitDraft {
        id: format!("update:{}", locked.name),
        skill_name: locked.name.clone(),
        source: None,
        target: request.context.clone(),
        expected_revisions: facts.revisions.clone(),
        entries: PreparedMutationEntries {
            canonical: canonical_entry,
            required_agents: required_agent_entries,
            expected_targets: target_facts
                .iter()
                .map(|fact| ExpectedTargetEntry {
                    key: fact.key.clone(),
                    fingerprint: fact.fingerprint.clone(),
                    expected_content_manifest_hash: manifests.get(&fact.key).cloned().flatten(),
                })
                .collect(),
        },
        lock_mutation: Some(lock_mutation(facts, locked, payload, now)?),
    })
}

fn lock_mutation(
    facts: &InstallPlanningFacts,
    locked: &LockedUpdateSkill,
    payload: &PinnedPayloadLease,
    now: String,
) -> Result<PreparedLockMutation, AppError> {
    let metadata = payload.planning_metadata();
    let replacement = match facts.lock_schema {
        LockSchema::Global => json!({
            "source": metadata.source,
            "sourceType": metadata.source_type,
            "sourceUrl": metadata.well_known.as_ref().map(|value| value.artifact_url.clone()).or_else(|| metadata.source_url.clone()),
            "sourceBaseUrl": metadata.well_known.as_ref().and(metadata.source_url.clone()),
            "wellKnownDigest": metadata.well_known.as_ref().map(|value| value.digest.clone()),
            "ref": metadata.ref_name,
            "skillPath": metadata.skill_path,
            "skillFolderHash": metadata.global_skill_folder_hash(),
            "installedAt": locked.installed_at.clone().unwrap_or_else(|| now.clone()),
            "updatedAt": now,
            "pluginName": metadata.plugin_name,
        }),
        LockSchema::Project => {
            let mut entry = serde_json::Map::new();
            let source = if metadata.source_type == "local" {
                facts
                    .resolved_context
                    .project
                    .as_ref()
                    .map(|project| {
                        crate::core::portable_project_path::serialize_project_source(
                            &project.native_path,
                            &metadata.source,
                        )
                    })
                    .unwrap_or_else(|| metadata.source.clone())
            } else {
                metadata.source.clone()
            };
            entry.insert("source".to_string(), json!(source));
            entry.insert("sourceType".to_string(), json!(metadata.source_type));
            entry.insert("sourceUrl".to_string(), json!(metadata.source_url));
            if let Some(well_known) = &metadata.well_known {
                entry.insert("wellKnownDigest".to_string(), json!(well_known.digest));
            }
            entry.insert("ref".to_string(), json!(metadata.ref_name));
            entry.insert("skillPath".to_string(), json!(metadata.skill_path));
            entry.insert("computedHash".to_string(), json!(metadata.computed_hash));
            entry.insert("pluginName".to_string(), json!(metadata.plugin_name));
            if let Some(subagents) = &locked.subagents {
                entry.insert("subagents".to_string(), json!(subagents));
            }
            if let Some(revision) = &metadata.upstream_revision {
                entry.insert("remoteHash".to_string(), json!(revision));
            }
            Value::Object(entry)
        }
    };
    Ok(PreparedLockMutation {
        target: facts.resolved_context.lock.clone(),
        legacy_target: None,
        schema: facts.lock_schema,
        entry: if locked.lock_key != locked.name {
            LockEntryMutation::MoveAndReplace {
                from: locked.lock_key.clone(),
                to: locked.name.clone(),
                replacement,
            }
        } else {
            LockEntryMutation::Replace {
                key: locked.lock_key.clone(),
                replacement,
            }
        },
        root_replacements: BTreeMap::new(),
        expected: LockExpectedState::capture(
            &facts.lock_document,
            [locked.lock_key.as_str(), locked.name.as_str()],
            std::iter::empty::<&str>(),
        ),
    })
}

fn standard_owners(runtime: &AgentRuntimeSnapshot, scope: &SkillLocation) -> Vec<AgentId> {
    let mut owners = runtime
        .agents
        .iter()
        .filter_map(|(id, agent)| {
            let resolved = match scope {
                SkillLocation::Global => &agent.global,
                SkillLocation::Project { .. } => &agent.project,
            };
            (agent.definition.adapter == AgentAdapter::Standard
                && resolved.enabled
                && resolved.reads_standard)
                .then(|| id.clone())
        })
        .collect::<Vec<_>>();
    owners.sort();
    owners
}

fn agent_scope<'a>(
    runtime: &'a AgentRuntimeSnapshot,
    id: &AgentId,
    scope: &SkillLocation,
) -> Result<&'a ResolvedAgentScope, AppError> {
    let agent = runtime.agents.get(id).ok_or(AppError::StaleRegistry)?;
    Ok(match scope {
        SkillLocation::Global => &agent.global,
        SkillLocation::Project { .. } => &agent.project,
    })
}

fn eve_adapter_roots(
    runtime: &AgentRuntimeSnapshot,
    skill: &LockedUpdateSkill,
    environment: &EnvironmentRef,
) -> Result<Vec<(String, ResourceLocator, ObservedEntryOwner)>, AppError> {
    let has_explicit_placement = skill
        .subagents
        .as_ref()
        .is_some_and(|targets| !targets.is_empty());
    let Some((agent_id, agent)) = runtime.agents.iter().find(|(_, agent)| {
        agent.definition.adapter == AgentAdapter::Eve
            && agent.project.enabled
            && agent.detection == DetectionState::Detected
    }) else {
        if has_explicit_placement {
            return Err(AppError::Validation {
                field: Some("subagents".to_string()),
                message: format!(
                    "Skill '{}' records Eve placement, but Eve is not detected in this Project",
                    skill.name
                ),
            });
        }
        return Ok(Vec::new());
    };
    let project = runtime
        .project_path
        .as_deref()
        .ok_or(AppError::StaleContext)?;
    let target_ids = match &skill.subagents {
        None => vec!["eve:root".to_string()],
        Some(subagents) => subagents
            .iter()
            .map(|subagent| {
                if subagent.is_empty() {
                    "eve:root".to_string()
                } else {
                    format!("eve:{}", crate::core::skill::sanitize_name(subagent))
                }
            })
            .collect(),
    };
    target_ids
        .into_iter()
        .map(|target_id| {
            let relative = match target_id.strip_prefix("eve:") {
                Some("root") => "agent/skills".to_string(),
                Some(subagent) if !subagent.is_empty() => {
                    format!("agent/subagents/{subagent}/skills")
                }
                _ => {
                    return Err(AppError::ConfigurationCorrupted {
                        message: "invalid Eve adapter target in lock metadata".to_string(),
                    })
                }
            };
            Ok((
                target_id.clone(),
                join_entry(
                    &ResourceLocator {
                        environment: environment.clone(),
                        native_path: project.to_string(),
                    },
                    &relative,
                ),
                ObservedEntryOwner {
                    agent_id: agent_id.clone(),
                    display_name: agent.definition.display_name.clone(),
                    logical_target_id: target_id,
                },
            ))
        })
        .collect()
}

fn observed_digest(
    target_facts: &[ResolvedTargetFact],
    facts: &InstallPlanningFacts,
    locked: &[LockedUpdateSkill],
    manifests: &BTreeMap<PhysicalTargetKey, Option<ContentManifestHash>>,
) -> Result<String, AppError> {
    stable_digest(&(
        target_facts
            .iter()
            .map(|fact| (&fact.key, &fact.fingerprint, fact.entry_kind as u8))
            .collect::<Vec<_>>(),
        manifest_digest_entries(target_facts, manifests),
        locked
            .iter()
            .map(|skill| {
                (
                    &skill.name,
                    facts
                        .lock_document
                        .entry_snapshot(&skill.lock_key)
                        .value()
                        .cloned(),
                )
            })
            .collect::<Vec<_>>(),
    ))
}

fn skill_observed_digest(
    target_facts: &[ResolvedTargetFact],
    facts: &InstallPlanningFacts,
    locked: &LockedUpdateSkill,
    manifests: &BTreeMap<PhysicalTargetKey, Option<ContentManifestHash>>,
) -> Result<String, AppError> {
    stable_digest(&(
        target_facts
            .iter()
            .map(|fact| (&fact.key, &fact.fingerprint, fact.entry_kind as u8))
            .collect::<Vec<_>>(),
        manifest_digest_entries(target_facts, manifests),
        facts
            .lock_document
            .entry_snapshot(&locked.lock_key)
            .value()
            .cloned(),
    ))
}

fn manifest_digest_entries<'a>(
    target_facts: &'a [ResolvedTargetFact],
    manifests: &'a BTreeMap<PhysicalTargetKey, Option<ContentManifestHash>>,
) -> Vec<(&'a PhysicalTargetKey, Option<&'a str>)> {
    target_facts
        .iter()
        .filter(|fact| fact.entry_kind == TargetEntryKind::Directory)
        .map(|fact| {
            (
                &fact.key,
                manifests
                    .get(&fact.key)
                    .and_then(Option::as_ref)
                    .map(ContentManifestHash::as_str),
            )
        })
        .collect()
}

fn join_entry(root: &ResourceLocator, child: &str) -> ResourceLocator {
    let separator = if matches!(root.environment, EnvironmentRef::Native) && cfg!(windows) {
        '\\'
    } else {
        '/'
    };
    ResourceLocator {
        environment: root.environment.clone(),
        native_path: format!(
            "{}{}{}",
            root.native_path.trim_end_matches(['/', '\\']),
            separator,
            child
        ),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use tempfile::tempdir;

    use super::*;
    use crate::core::agent_definition::{
        AgentAdapter, AgentDefinition, AgentId, AgentSource, DetectionSpec, PathSpec,
        ScopeDefinition,
    };
    use crate::core::lossless_lock::LockSchema;
    use crate::environment::agent_environment::{
        AgentRuntimeSnapshot, DetectionState, ResolvedAgent, ResolvedAgentScope,
    };
    use crate::environment::types::{EnvironmentRef, EnvironmentStatus};

    #[cfg(unix)]
    mod unix_tests {
        use std::fs;
        use std::os::unix::fs::symlink;
        use std::sync::{Arc, Mutex};

        use super::*;
        use crate::application::install::InstallFuture;
        use crate::application::install_planner::{
            InstallPlanningFactSource, InstallPlanningFacts,
        };
        use crate::application::mutation::plan::RuntimeRevisions;
        use crate::application::payload_session::{
            PayloadPlanningMetadata, PayloadSessionLimits, PayloadSessionManager,
        };
        use crate::application::update::{UpdateExecutionRequest, UpdatePlanner, UpdateRequest};
        use crate::core::agent_definition::PathSpec;
        use crate::core::lossless_lock::LosslessLockDocument;
        use crate::core::skill_payload::build_skill_payload;
        use crate::environment::content_manifest::{
            ContentManifest, ContentManifestReader, ContentManifestTarget,
        };
        use crate::environment::context_resolver::ResolvedContext;
        use crate::environment::planning::{
            RuntimeTargetFactResolver, TargetFactFuture, TargetFactResolver,
        };
        use crate::environment::runtime::ContextSnapshotRevision;
        use crate::environment::types::{ResourceLocator, SkillLocation, SkillLocationRef};
        use crate::environment::wsl::WslRuntime;

        struct Facts(InstallPlanningFacts);

        impl InstallPlanningFactSource for Facts {
            fn current<'a>(
                &'a self,
                _context: &'a SkillLocationRef,
            ) -> InstallFuture<'a, Result<InstallPlanningFacts, crate::error::AppError>>
            {
                Box::pin(async move { Ok(self.0.clone()) })
            }
        }

        struct CountingTargets {
            inner: RuntimeTargetFactResolver,
            manifest_reads: Arc<Mutex<BTreeMap<PhysicalTargetKey, usize>>>,
        }

        impl TargetFactResolver for CountingTargets {
            fn resolve<'a>(
                &'a self,
                context: &'a SkillLocationRef,
                logical_destinations: &'a [ResourceLocator],
                cancellation: Option<crate::core::mutation::CancellationSignal>,
            ) -> TargetFactFuture<'a, Result<Vec<ResolvedTargetFact>, AppError>> {
                self.inner
                    .resolve(context, logical_destinations, cancellation)
            }
        }

        impl ContentManifestReader for CountingTargets {
            fn read<'a>(
                &'a self,
                target: &'a ContentManifestTarget,
            ) -> std::pin::Pin<
                Box<
                    dyn std::future::Future<Output = Result<ContentManifest, AppError>> + Send + 'a,
                >,
            > {
                *self
                    .manifest_reads
                    .lock()
                    .unwrap()
                    .entry(target.key.clone())
                    .or_default() += 1;
                self.inner.read(target)
            }
        }

        #[tokio::test]
        async fn private_copy_requires_observed_selection_while_symlink_follows_canonical() {
            let temp = tempdir().unwrap();
            let physical_root = fs::canonicalize(temp.path()).unwrap();
            let canonical_root = physical_root.join(".agents/skills");
            let copy_root = physical_root.join(".copy/skills");
            let link_root = physical_root.join(".link/skills");
            let alias_root = physical_root.join(".alias/skills");
            let source = temp.path().join("source/demo");
            fs::create_dir_all(canonical_root.join("demo")).unwrap();
            fs::create_dir_all(copy_root.join("demo")).unwrap();
            fs::create_dir_all(&link_root).unwrap();
            fs::create_dir_all(alias_root.parent().unwrap()).unwrap();
            symlink(canonical_root.join("demo"), link_root.join("demo")).unwrap();
            symlink(&copy_root, &alias_root).unwrap();
            fs::create_dir_all(&source).unwrap();
            fs::write(source.join("SKILL.md"), b"---\nname: demo\n---\nnew").unwrap();
            let payload = build_skill_payload(&source).unwrap();
            let manager = Arc::new(PayloadSessionManager::in_memory(
                PayloadSessionLimits {
                    ttl_ms: 60_000,
                    max_sessions: 4,
                    max_bytes: 1_000_000,
                },
                || 1_000,
            ));
            let discovery = manager
                .discover(EnvironmentRef::Native, "source-fingerprint")
                .await
                .unwrap();
            let handle = manager
                .acquire_payload_with_metadata(
                    &discovery,
                    "skills/demo",
                    payload,
                    PayloadPlanningMetadata {
                        skill_name: "demo".to_string(),
                        install_dir_name: "demo".to_string(),
                        source: "owner/repo".to_string(),
                        source_type: "github".to_string(),
                        source_url: Some("https://github.com/owner/repo.git".to_string()),
                        ref_name: Some("main".to_string()),
                        skill_path: "skills/demo".to_string(),
                        plugin_name: None,
                        computed_hash: "new-computed".to_string(),
                        upstream_revision: Some("new-remote".to_string()),
                        well_known: None,
                    },
                )
                .await
                .unwrap();
            let context = SkillLocationRef {
                environment: EnvironmentRef::Native,
                scope: SkillLocation::Project {
                    project_id: "project-1".to_string(),
                },
            };
            let mut facts = InstallPlanningFacts {
            resolved_context: ResolvedContext {
                context: context.clone(),
                project: None,
                home: locator(temp.path()),
                skill_root: locator(&canonical_root),
                lock: locator(&temp.path().join("skills-lock.json")),
            },
            agent_runtime: runtime(&copy_root, &link_root, &alias_root),
            revisions: RuntimeRevisions {
                registry: "registry-1".to_string(),
                environment: "environment-1".to_string(),
                context: ContextSnapshotRevision::parse("context-v1-update").unwrap(),
            },
            lock_schema: LockSchema::Project,
            lock_document: LosslessLockDocument::parse(
                br#"{"version":1,"futureRoot":true,"skills":{"demo":{"source":"owner/repo","sourceType":"github","sourceUrl":"https://github.com/owner/repo.git","ref":"main","skillPath":"skills/demo","computedHash":"old-computed","remoteHash":"old-remote","futureEntry":42}}}"#,
            )
            .unwrap(),
            eve_targets: Vec::new(),
        };
            for index in 0..75 {
                let missing_root = physical_root.join(format!(".missing-{index}/skills"));
                let (id, resolved) = agent(
                    &format!("missing-agent-{index}"),
                    &format!("Missing Agent {index}"),
                    &missing_root,
                );
                facts.agent_runtime.agents.insert(id, resolved);
            }
            let manifest_reads = Arc::new(Mutex::new(BTreeMap::new()));
            let planner = ConcreteUpdatePlanner::new(
                Facts(facts),
                CountingTargets {
                    inner: RuntimeTargetFactResolver::new(Arc::new(WslRuntime::default())),
                    manifest_reads: Arc::clone(&manifest_reads),
                },
                Arc::clone(&manager),
                || "2026-07-18T00:00:00.000Z".to_string(),
            );
            let request = UpdateRequest {
                context,
                skill_names: vec!["demo".to_string()],
            };

            let inspection = planner.inspect(&request).await.unwrap();
            assert_eq!(inspection.source_candidates.len(), 1);
            assert_eq!(inspection.skills[0].clean_copies.len(), 1);
            assert!(inspection.skills[0].conflicts.is_empty());
            assert!(inspection.skills[0].adapter_targets.is_empty());
            assert_eq!(
                manifest_reads
                    .lock()
                    .unwrap()
                    .values()
                    .copied()
                    .sum::<usize>(),
                2
            );
            assert!(manifest_reads
                .lock()
                .unwrap()
                .values()
                .all(|reads| *reads == 1));

            assert_eq!(inspection.skills[0].clean_copies.len(), 1);
            assert_eq!(inspection.skills[0].clean_copies[0].owners.len(), 2);
            let execution = UpdateExecutionRequest {
                request: request.clone(),
                overwrite_private_entries: vec![inspection.skills[0].clean_copies[0]
                    .entry_id
                    .clone()],
            };
            let (token, plan) = planner
                .build(
                    &execution,
                    vec![handle.clone()],
                    vec![manager.pin_verified(&handle).await.unwrap()],
                )
                .await
                .unwrap();

            assert_eq!(inspection.token, token);
            assert_eq!(plan.units.len(), 1);
            assert!(plan.units[0]
                .canonical_entry
                .iter()
                .chain(&plan.units[0].required_agent_entries)
                .all(
                    |mutation| plan.units[0].expected_targets.iter().any(|expected| {
                        expected.key == mutation.key
                            && expected.expected_content_manifest_hash.is_some()
                    })
                ));
            assert!(plan.units[0].canonical_entry.is_some());
            assert_eq!(plan.units[0].required_agent_entries.len(), 1);
            assert_eq!(
                plan.units[0].required_agent_entries[0]
                    .destination
                    .native_path,
                copy_root.join("demo").to_string_lossy()
            );
            assert_eq!(
                plan.units[0]
                    .lock_mutation
                    .as_ref()
                    .unwrap()
                    .replacement()
                    .unwrap()["remoteHash"],
                "new-remote"
            );

            fs::write(copy_root.join("demo/local-change.txt"), b"modified").unwrap();
            let changed = planner.inspect(&request).await.unwrap();
            assert_ne!(changed.token, token);
            assert_eq!(changed.skills[0].clean_copies.len(), 0);
            assert_eq!(changed.skills[0].conflicts.len(), 1);
            assert!(manifest_reads
                .lock()
                .unwrap()
                .values()
                .all(|reads| *reads == 3));
        }

        fn locator(path: &std::path::Path) -> ResourceLocator {
            ResourceLocator {
                environment: EnvironmentRef::Native,
                native_path: path.to_string_lossy().into_owned(),
            }
        }

        fn runtime(
            copy_root: &std::path::Path,
            link_root: &std::path::Path,
            alias_root: &std::path::Path,
        ) -> AgentRuntimeSnapshot {
            AgentRuntimeSnapshot {
                registry_revision: "registry-1".to_string(),
                environment_revision: "environment-1".to_string(),
                environment: EnvironmentRef::Native,
                availability: EnvironmentStatus::Available,
                project_path: None,
                agents: BTreeMap::from([
                    agent("copy-agent", "Copy Agent", copy_root),
                    agent("link-agent", "Link Agent", link_root),
                    agent("alias-agent", "Alias Agent", alias_root),
                ]),
            }
        }

        fn agent(id: &str, display_name: &str, root: &std::path::Path) -> (AgentId, ResolvedAgent) {
            let id = AgentId::parse(id).unwrap();
            let scope = ResolvedAgentScope {
                enabled: true,
                reads_standard: true,
                standard_path: Some("unused".to_string()),
                private_path: Some(root.to_string_lossy().into_owned()),
                read_paths: Vec::new(),
                standard_presence: None,
                private_presence: None,
                legacy_paths: Vec::new(),
            };
            (
                id.clone(),
                ResolvedAgent {
                    definition: AgentDefinition {
                        id,
                        display_name: display_name.to_string(),
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
                            private_path: Some(PathSpec::project(".agent/skills")),
                        },
                        detection: DetectionSpec::AnyPathExists {
                            paths: vec![PathSpec::home(".agent")],
                        },
                        legacy_paths: Vec::new(),
                        adapter: AgentAdapter::Standard,
                    },
                    detection: DetectionState::Detected,
                    detection_reason: None,
                    global: scope.clone(),
                    project: scope,
                },
            )
        }
    }

    #[test]
    fn eve_adapter_roots_follow_the_locked_root_or_subagent_targets() {
        let temp = tempdir().unwrap();
        let project = temp.path().join("project");
        let runtime = eve_runtime(project.to_string_lossy().as_ref());
        let mut skill = LockedUpdateSkill {
            name: "demo".to_string(),
            lock_key: "demo".to_string(),
            source: "owner/repo".to_string(),
            source_type: "github".to_string(),
            source_url: Some("https://github.com/owner/repo".to_string()),
            ref_name: None,
            skill_path: "skills/demo".to_string(),
            remote_hash: Some("old".to_string()),
            computed_hash: None,
            installed_at: None,
            subagents: None,
            well_known_digest: None,
        };

        skill.subagents = Some(vec!["".to_string()]);
        let root = eve_adapter_roots(&runtime, &skill, &EnvironmentRef::Native).unwrap();
        assert_eq!(root.len(), 1);
        assert_eq!(
            root[0].1.native_path,
            project.join("agent/skills").to_string_lossy()
        );

        skill.subagents = Some(vec!["".to_string(), "Research Team".to_string()]);
        let subagents = eve_adapter_roots(&runtime, &skill, &EnvironmentRef::Native).unwrap();
        assert_eq!(subagents.len(), 2);
        assert_eq!(
            subagents[1].1.native_path,
            project
                .join("agent/subagents/research-team/skills")
                .to_string_lossy()
        );
    }

    #[test]
    fn eve_adapter_roots_distinguish_legacy_missing_targets_from_explicit_empty_targets() {
        let temp = tempdir().unwrap();
        let runtime = eve_runtime(temp.path().join("project").to_string_lossy().as_ref());
        let base = json!({
            "source": "owner/repo",
            "sourceType": "github",
            "sourceUrl": "https://github.com/owner/repo",
            "skillPath": "skills/demo",
            "remoteHash": "old",
            "computedHash": "old"
        });

        let legacy = locked_skill("demo", "demo", LockSchema::Project, &base).unwrap();
        assert_eq!(
            eve_adapter_roots(&runtime, &legacy, &EnvironmentRef::Native)
                .unwrap()
                .len(),
            1
        );

        let mut explicit_empty = base;
        explicit_empty
            .as_object_mut()
            .unwrap()
            .insert("subagents".to_string(), json!([]));
        let explicit_empty =
            locked_skill("demo", "demo", LockSchema::Project, &explicit_empty).unwrap();
        assert!(
            eve_adapter_roots(&runtime, &explicit_empty, &EnvironmentRef::Native)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn eve_adapter_roots_require_a_detected_eve_project() {
        let temp = tempdir().unwrap();
        let project = temp.path().join("project");
        let mut runtime = eve_runtime(project.to_string_lossy().as_ref());
        runtime.agents.values_mut().next().unwrap().detection = DetectionState::NotDetected;
        let skill = LockedUpdateSkill {
            name: "demo".to_string(),
            lock_key: "demo".to_string(),
            source: "owner/repo".to_string(),
            source_type: "github".to_string(),
            source_url: Some("https://github.com/owner/repo".to_string()),
            ref_name: None,
            skill_path: "skills/demo".to_string(),
            remote_hash: Some("old".to_string()),
            computed_hash: Some("old".to_string()),
            installed_at: None,
            subagents: None,
            well_known_digest: None,
        };

        assert!(eve_adapter_roots(&runtime, &skill, &EnvironmentRef::Native)
            .unwrap()
            .is_empty());

        let mut explicit = skill;
        explicit.subagents = Some(vec!["research".to_string()]);
        assert!(matches!(
            eve_adapter_roots(&runtime, &explicit, &EnvironmentRef::Native),
            Err(AppError::Validation { .. })
        ));
    }

    #[test]
    fn locked_skill_rejects_malformed_eve_placement() {
        for subagents in [json!("root"), json!(["", 1])] {
            let result = locked_skill(
                "demo",
                "demo",
                LockSchema::Project,
                &json!({
                    "source": "owner/repo",
                    "sourceType": "github",
                    "sourceUrl": "https://github.com/owner/repo",
                    "skillPath": "skills/demo",
                    "remoteHash": "old",
                    "computedHash": "old",
                    "subagents": subagents
                }),
            );

            assert!(matches!(
                result,
                Err(AppError::ConfigurationCorrupted { .. })
            ));
        }
    }

    #[test]
    fn global_lock_ignores_project_only_eve_placement() {
        assert!(locked_skill(
            "demo",
            "demo",
            LockSchema::Global,
            &json!({
                "source": "owner/repo",
                "sourceType": "github",
                "sourceUrl": "https://github.com/owner/repo",
                "skillPath": "skills/demo",
                "skillFolderHash": "old",
                "subagents": "external-extension"
            }),
        )
        .is_ok());
    }

    #[test]
    fn gitlab_lock_uses_computed_hash_for_remote_precheck() {
        let skill = LockedUpdateSkill {
            name: "demo".to_string(),
            lock_key: "demo".to_string(),
            source: "https://gitlab.com/owner/repo".to_string(),
            source_type: "gitlab".to_string(),
            source_url: Some("https://gitlab.com/owner/repo".to_string()),
            ref_name: None,
            skill_path: "skills/demo".to_string(),
            remote_hash: Some("tree".to_string()),
            computed_hash: Some("content-v1".to_string()),
            installed_at: None,
            subagents: None,
            well_known_digest: None,
        };

        let capability = skill.capability();
        assert!(capability.can_run_update);
        assert!(capability.can_check_for_updates);
        assert_eq!(capability.reason, None);
    }

    #[test]
    fn project_generic_git_lock_uses_computed_hash_as_update_baseline() {
        let skill = locked_skill(
            "demo",
            "demo",
            LockSchema::Project,
            &json!({
                "source": "https://example.com/owner/repo.git",
                "sourceType": "git",
                "sourceUrl": "https://example.com/owner/repo.git",
                "ref": "main",
                "skillPath": "skills/demo",
                "computedHash": "content-v1"
            }),
        )
        .unwrap();

        let capability = skill.capability();
        assert!(capability.can_run_update);
        assert!(capability.can_check_for_updates);
        assert_eq!(capability.reason, None);
    }

    fn eve_runtime(project: &str) -> AgentRuntimeSnapshot {
        let id = AgentId::parse("eve").unwrap();
        let disabled = ResolvedAgentScope {
            enabled: false,
            reads_standard: false,
            standard_path: None,
            private_path: None,
            read_paths: Vec::new(),
            standard_presence: None,
            private_presence: None,
            legacy_paths: Vec::new(),
        };
        let project_scope = ResolvedAgentScope {
            enabled: true,
            ..disabled.clone()
        };
        AgentRuntimeSnapshot {
            registry_revision: "registry-1".to_string(),
            environment_revision: "environment-1".to_string(),
            environment: EnvironmentRef::Native,
            availability: EnvironmentStatus::Available,
            project_path: Some(project.to_string()),
            agents: BTreeMap::from([(
                id.clone(),
                ResolvedAgent {
                    definition: AgentDefinition {
                        id,
                        display_name: "Eve".to_string(),
                        source: AgentSource::Builtin,
                        aliases: Vec::new(),
                        global: ScopeDefinition {
                            enabled: false,
                            reads_standard: false,
                            private_path: None,
                        },
                        project: ScopeDefinition {
                            enabled: true,
                            reads_standard: false,
                            private_path: None,
                        },
                        detection: DetectionSpec::AnyPathExists {
                            paths: vec![
                                PathSpec::project("agent"),
                                PathSpec::project("package.json"),
                            ],
                        },
                        legacy_paths: Vec::new(),
                        adapter: AgentAdapter::Eve,
                    },
                    detection: DetectionState::Detected,
                    detection_reason: None,
                    global: disabled,
                    project: project_scope,
                },
            )]),
        }
    }
}

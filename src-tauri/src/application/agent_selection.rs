use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::application::agent_intent::{AdapterTargetId, AgentWriteIntent, PrivateEntryIntent};
use crate::application::mutation::plan::stable_digest;
use crate::core::agent_definition::{AgentAdapter, AgentId};
use crate::environment::agent_environment::{
    AgentRuntimeSnapshot, DetectionState, ResolvedAgentScope,
};
use crate::environment::planning::{ResolvedTargetFact, TargetFactResolver};
use crate::environment::runtime::PhysicalTargetKey;
use crate::environment::types::{
    same_environment_identity, ContextRef, ContextScope, ResourceLocator,
};
use crate::error::{AgentSelectionInvalidReason, AppError};
use crate::models::{InstallMode, InstallTargetInfo};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Type)]
#[serde(transparent)]
pub struct AgentSelectionItemId(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(transparent)]
pub struct AgentSelectionRevision(pub String);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub enum AgentSelectionCategory {
    SeparateInstall,
    AdditionalInstall,
    GroupChild,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub enum AgentSelectionModeConstraint {
    UserSelectable,
    CopyOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub enum AgentSelectionDisabledReason {
    PlacementConflict,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct AgentSelectionAgent {
    pub id: AgentId,
    pub display_name: String,
    pub detection: DetectionState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct AgentSelectionItem {
    pub id: AgentSelectionItemId,
    pub agent_ids: Vec<AgentId>,
    pub category: AgentSelectionCategory,
    pub display_name: String,
    pub path: String,
    pub group_id: Option<String>,
    pub selectable: bool,
    pub mode_constraint: AgentSelectionModeConstraint,
    pub disabled_reason: Option<AgentSelectionDisabledReason>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct AgentSelectionDisplayGroup {
    pub id: String,
    pub agent_id: AgentId,
    pub display_name: String,
    pub item_ids: Vec<AgentSelectionItemId>,
    pub detection: DetectionState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub enum UnavailableAgentSelectionReason {
    DefinitionMissing,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct UnavailableAgentSelection {
    pub agent_id: String,
    pub reason: UnavailableAgentSelectionReason,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct AgentSelectionSnapshot {
    pub agents: Vec<AgentSelectionAgent>,
    pub direct_agent_ids: Vec<AgentId>,
    pub items: Vec<AgentSelectionItem>,
    pub groups: Vec<AgentSelectionDisplayGroup>,
    pub initial_selected_item_ids: Vec<AgentSelectionItemId>,
    pub unavailable_explicit_agents: Vec<UnavailableAgentSelection>,
    pub requested_mode_item_ids: Vec<AgentSelectionItemId>,
    pub revision: AgentSelectionRevision,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct AgentSelectionSubmission {
    pub revision: AgentSelectionRevision,
    pub selected_item_ids: Vec<AgentSelectionItemId>,
    pub requested_mode: InstallMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub enum DefaultSelectionWarning {
    ReadFailed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct InstallAgentSelectionSnapshot {
    pub selection: AgentSelectionSnapshot,
    pub default_selection_warning: Option<DefaultSelectionWarning>,
}

#[derive(Debug, Clone)]
pub(crate) struct ResolvedAgentSelectionItem {
    pub public: AgentSelectionItem,
    pub root: ResourceLocator,
    pub adapter_target_ids: Vec<String>,
    physical_key: PhysicalTargetKey,
    content: AgentSelectionContent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum AgentSelectionContent {
    Canonical,
    EveDerived,
}

#[derive(Debug, Clone)]
pub(crate) struct AgentSelectionCatalog {
    pub snapshot: AgentSelectionSnapshot,
    pub resolved_items: BTreeMap<AgentSelectionItemId, ResolvedAgentSelectionItem>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AgentSelectionResolution {
    Ready(Vec<AgentWriteIntent>),
    Stale(AgentSelectionSnapshot),
}

pub(crate) fn resolve_agent_selection_submission(
    catalog: &AgentSelectionCatalog,
    submission: &AgentSelectionSubmission,
) -> Result<AgentSelectionResolution, AppError> {
    if submission.revision != catalog.snapshot.revision {
        return Ok(AgentSelectionResolution::Stale(catalog.snapshot.clone()));
    }

    let mut selected_ids = std::collections::BTreeSet::new();
    for item_id in &submission.selected_item_ids {
        if !selected_ids.insert(item_id.clone()) {
            return Err(selection_validation(
                AgentSelectionInvalidReason::DuplicateItem,
            ));
        }
        let Some(item) = catalog.resolved_items.get(item_id) else {
            return Ok(AgentSelectionResolution::Stale(catalog.snapshot.clone()));
        };
        if !item.public.selectable {
            return Err(selection_validation(
                AgentSelectionInvalidReason::ItemUnavailable,
            ));
        }
    }
    let mut selected_content_by_target =
        BTreeMap::<PhysicalTargetKey, BTreeSet<AgentSelectionContent>>::new();
    for item_id in &selected_ids {
        let item = catalog
            .resolved_items
            .get(item_id)
            .expect("selected item was validated above");
        selected_content_by_target
            .entry(item.physical_key.clone())
            .or_default()
            .insert(item.content);
    }
    if selected_content_by_target
        .values()
        .any(|contents| contents.len() > 1)
    {
        return Err(selection_validation(
            AgentSelectionInvalidReason::PlacementConflict,
        ));
    }

    let mut intents = BTreeMap::<AgentId, AgentWriteIntent>::new();
    for agent_id in &catalog.snapshot.direct_agent_ids {
        intents.insert(
            agent_id.clone(),
            AgentWriteIntent {
                agent_id: agent_id.clone(),
                private_entry: PrivateEntryIntent::None,
                adapter_targets: Vec::new(),
            },
        );
    }

    for item_id in selected_ids {
        let item = catalog
            .resolved_items
            .get(&item_id)
            .expect("selected item was validated above");
        for agent_id in &item.public.agent_ids {
            let intent = intents
                .entry(agent_id.clone())
                .or_insert_with(|| AgentWriteIntent {
                    agent_id: agent_id.clone(),
                    private_entry: PrivateEntryIntent::None,
                    adapter_targets: Vec::new(),
                });
            match item.public.category {
                AgentSelectionCategory::SeparateInstall => {
                    intent.private_entry = PrivateEntryIntent::Required;
                }
                AgentSelectionCategory::AdditionalInstall => {
                    if intent.private_entry == PrivateEntryIntent::None {
                        intent.private_entry = PrivateEntryIntent::OptionalSelected;
                    }
                }
                AgentSelectionCategory::GroupChild => {
                    intent
                        .adapter_targets
                        .extend(item.adapter_target_ids.iter().cloned().map(AdapterTargetId));
                }
            }
        }
    }

    let mut intents = intents.into_values().collect::<Vec<_>>();
    for intent in &mut intents {
        intent.adapter_targets.sort();
        intent.adapter_targets.dedup();
    }
    Ok(AgentSelectionResolution::Ready(intents))
}

fn selection_validation(reason: AgentSelectionInvalidReason) -> AppError {
    AppError::AgentSelectionInvalid { reason }
}

pub(crate) fn apply_initial_agent_selection(
    catalog: &mut AgentSelectionCatalog,
    requested_agent_ids: &[String],
) {
    let known_agents = catalog
        .snapshot
        .agents
        .iter()
        .map(|agent| agent.id.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    catalog.snapshot.unavailable_explicit_agents = requested_agent_ids
        .iter()
        .filter(|agent_id| !known_agents.contains(agent_id.as_str()))
        .map(|agent_id| UnavailableAgentSelection {
            agent_id: agent_id.clone(),
            reason: UnavailableAgentSelectionReason::DefinitionMissing,
        })
        .collect();

    let requested = requested_agent_ids
        .iter()
        .filter_map(|agent_id| AgentId::parse(agent_id).ok())
        .collect::<std::collections::BTreeSet<_>>();
    catalog.snapshot.initial_selected_item_ids = catalog
        .snapshot
        .items
        .iter()
        .filter(|item| {
            item.selectable
                && item
                    .agent_ids
                    .iter()
                    .any(|agent_id| requested.contains(agent_id))
        })
        .map(|item| item.id.clone())
        .collect();
}

struct CatalogCandidate {
    agent_id: AgentId,
    reads_shared: bool,
    display_name: String,
    path: String,
    destination: ResourceLocator,
}

pub(crate) async fn build_agent_selection_catalog<T: TargetFactResolver>(
    context: &ContextRef,
    runtime: &AgentRuntimeSnapshot,
    eve_targets: &[InstallTargetInfo],
    targets: &T,
) -> Result<AgentSelectionCatalog, AppError> {
    if !same_environment_identity(&context.environment, &runtime.environment) {
        return Err(AppError::StaleEnvironment);
    }

    let mut agents = Vec::new();
    let mut direct_agent_ids = Vec::new();
    let mut candidates = Vec::new();
    for (agent_id, agent) in &runtime.agents {
        let scope = selected_scope(context, &agent.global, &agent.project);
        if !scope.enabled {
            continue;
        }
        agents.push(AgentSelectionAgent {
            id: agent_id.clone(),
            display_name: agent.definition.display_name.clone(),
            detection: agent.detection,
        });
        if scope.reads_shared {
            direct_agent_ids.push(agent_id.clone());
        }
        if agent.definition.adapter != AgentAdapter::Standard {
            continue;
        }
        let Some(path) = scope.private_path.clone() else {
            continue;
        };
        candidates.push(CatalogCandidate {
            agent_id: agent_id.clone(),
            reads_shared: scope.reads_shared,
            display_name: agent.definition.display_name.clone(),
            path: path.clone(),
            destination: ResourceLocator {
                environment: context.environment.clone(),
                native_path: path,
            },
        });
    }

    let destinations = candidates
        .iter()
        .map(|candidate| candidate.destination.clone())
        .collect::<Vec<_>>();
    let facts = if destinations.is_empty() {
        Vec::new()
    } else {
        targets.resolve(context, &destinations, None).await?
    };
    if facts.len() != destinations.len() {
        return Err(AppError::StaleTarget);
    }

    let mut by_physical_key =
        BTreeMap::<PhysicalTargetKey, Vec<(CatalogCandidate, ResolvedTargetFact)>>::new();
    for (candidate, fact) in candidates.into_iter().zip(facts) {
        by_physical_key
            .entry(fact.key.clone())
            .or_default()
            .push((candidate, fact));
    }

    let mut items = Vec::new();
    let mut resolved_items = BTreeMap::new();
    for (physical_key, mut members) in by_physical_key {
        members.sort_by(|left, right| left.0.agent_id.cmp(&right.0.agent_id));
        let agent_ids = members
            .iter()
            .map(|(candidate, _)| candidate.agent_id.clone())
            .collect::<Vec<_>>();
        let category = if members.iter().any(|(candidate, _)| !candidate.reads_shared) {
            AgentSelectionCategory::SeparateInstall
        } else {
            AgentSelectionCategory::AdditionalInstall
        };
        let id = AgentSelectionItemId(stable_digest(&(
            "agent-selection-item-v1",
            context,
            &physical_key,
            &agent_ids,
            category,
            AgentSelectionModeConstraint::UserSelectable,
        ))?);
        let display_name = members[0].0.display_name.clone();
        let public = AgentSelectionItem {
            id: id.clone(),
            agent_ids,
            category,
            display_name,
            path: members[0].0.path.clone(),
            group_id: None,
            selectable: true,
            mode_constraint: AgentSelectionModeConstraint::UserSelectable,
            disabled_reason: None,
        };
        items.push(public.clone());
        resolved_items.insert(
            id,
            ResolvedAgentSelectionItem {
                public,
                root: members[0].0.destination.clone(),
                adapter_target_ids: Vec::new(),
                physical_key,
                content: AgentSelectionContent::Canonical,
            },
        );
    }

    let mut groups = Vec::new();
    if let Some((eve_id, eve)) = runtime.agents.iter().find(|(_, agent)| {
        agent.definition.adapter == AgentAdapter::Eve
            && selected_scope(context, &agent.global, &agent.project).enabled
    }) {
        let available_targets = eve_targets
            .iter()
            .filter(|target| target.agent == *eve_id)
            .collect::<Vec<_>>();
        if !available_targets.is_empty() {
            let destinations = available_targets
                .iter()
                .map(|target| ResourceLocator {
                    environment: context.environment.clone(),
                    native_path: target.path.clone(),
                })
                .collect::<Vec<_>>();
            let facts = targets.resolve(context, &destinations, None).await?;
            if facts.len() != destinations.len() {
                return Err(AppError::StaleTarget);
            }
            let group_id = format!("agent-group:{}", eve_id.as_str());
            let mut item_ids = Vec::new();
            for (target, fact) in available_targets.into_iter().zip(facts) {
                let id = AgentSelectionItemId(stable_digest(&(
                    "agent-selection-item-v1",
                    context,
                    &fact.key,
                    [&target.agent],
                    AgentSelectionCategory::GroupChild,
                    AgentSelectionModeConstraint::CopyOnly,
                    &target.target_id,
                ))?);
                let public = AgentSelectionItem {
                    id: id.clone(),
                    agent_ids: vec![target.agent.clone()],
                    category: AgentSelectionCategory::GroupChild,
                    display_name: target.display_name.clone(),
                    path: target.path.clone(),
                    group_id: Some(group_id.clone()),
                    selectable: true,
                    mode_constraint: AgentSelectionModeConstraint::CopyOnly,
                    disabled_reason: None,
                };
                item_ids.push(id.clone());
                items.push(public.clone());
                resolved_items.insert(
                    id,
                    ResolvedAgentSelectionItem {
                        public,
                        root: fact.destination,
                        adapter_target_ids: vec![target.target_id.clone()],
                        physical_key: fact.key,
                        content: AgentSelectionContent::EveDerived,
                    },
                );
            }
            groups.push(AgentSelectionDisplayGroup {
                id: group_id,
                agent_id: eve_id.clone(),
                display_name: eve.definition.display_name.clone(),
                item_ids,
                detection: eve.detection,
            });
        }
    }
    let content_by_target = resolved_items.values().fold(
        BTreeMap::<PhysicalTargetKey, BTreeSet<AgentSelectionContent>>::new(),
        |mut grouped, item| {
            grouped
                .entry(item.physical_key.clone())
                .or_default()
                .insert(item.content);
            grouped
        },
    );
    let conflicts = content_by_target
        .into_iter()
        .filter_map(|(key, contents)| (contents.len() > 1).then_some(key))
        .collect::<BTreeSet<_>>();
    if !conflicts.is_empty() {
        for resolved in resolved_items.values_mut() {
            if conflicts.contains(&resolved.physical_key) {
                resolved.public.disabled_reason =
                    Some(AgentSelectionDisabledReason::PlacementConflict);
            }
        }
        for item in &mut items {
            if let Some(resolved) = resolved_items.get(&item.id) {
                *item = resolved.public.clone();
            }
        }
    }
    items.sort_by(|left, right| {
        left.category
            .cmp(&right.category)
            .then_with(|| left.display_name.cmp(&right.display_name))
            .then_with(|| left.id.cmp(&right.id))
    });
    let requested_mode_item_ids = items
        .iter()
        .filter(|item| {
            item.selectable && item.mode_constraint == AgentSelectionModeConstraint::UserSelectable
        })
        .map(|item| item.id.clone())
        .collect::<Vec<_>>();
    let semantic_items = items
        .iter()
        .map(|item| {
            (
                &item.id,
                &item.agent_ids,
                item.category,
                item.mode_constraint,
                item.selectable,
            )
        })
        .collect::<Vec<_>>();
    let revision = AgentSelectionRevision(stable_digest(&(
        "agent-selection-revision-v1",
        context,
        &direct_agent_ids,
        semantic_items,
    ))?);

    Ok(AgentSelectionCatalog {
        snapshot: AgentSelectionSnapshot {
            agents,
            direct_agent_ids,
            items,
            groups,
            initial_selected_item_ids: Vec::new(),
            unavailable_explicit_agents: Vec::new(),
            requested_mode_item_ids,
            revision,
        },
        resolved_items,
    })
}

fn selected_scope<'a>(
    context: &ContextRef,
    global: &'a ResolvedAgentScope,
    project: &'a ResolvedAgentScope,
) -> &'a ResolvedAgentScope {
    match context.scope {
        ContextScope::Global => global,
        ContextScope::Project { .. } => project,
    }
}

#[cfg(test)]
pub(crate) async fn test_submission_for_agents<T: TargetFactResolver>(
    context: &ContextRef,
    runtime: &AgentRuntimeSnapshot,
    eve_targets: &[InstallTargetInfo],
    targets: &T,
    agent_ids: &[&str],
    requested_mode: InstallMode,
) -> AgentSelectionSubmission {
    let mut catalog = build_agent_selection_catalog(context, runtime, eve_targets, targets)
        .await
        .expect("test Agent selection catalog");
    apply_initial_agent_selection(
        &mut catalog,
        &agent_ids
            .iter()
            .map(|id| id.to_string())
            .collect::<Vec<_>>(),
    );
    AgentSelectionSubmission {
        revision: catalog.snapshot.revision,
        selected_item_ids: catalog.snapshot.initial_selected_item_ids,
        requested_mode,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::core::agent_definition::{
        AgentAdapter, AgentDefinition, AgentId, AgentSource, DetectionSpec, PathSpec,
        ScopeDefinition,
    };
    use crate::environment::agent_environment::{
        AgentRuntimeSnapshot, DetectionState, DirectoryPresenceState, ResolvedAgent,
        ResolvedAgentScope,
    };
    use crate::environment::planning::{
        ResolvedTargetFact, TargetEntryKind, TargetFactFuture, TargetFactResolver,
    };
    use crate::environment::runtime::{
        EntryFingerprint, ExecutionBackend, PhysicalParentIdentity, PhysicalTargetKey,
    };
    use crate::environment::types::{
        ContextRef, ContextScope, EnvironmentRef, EnvironmentStatus, ResourceLocator,
    };

    use super::{
        build_agent_selection_catalog, resolve_agent_selection_submission, AgentSelectionCategory,
        AgentSelectionDisabledReason, AgentSelectionModeConstraint, AgentSelectionResolution,
        AgentSelectionRevision, AgentSelectionSubmission,
    };

    #[derive(Clone)]
    struct SharedTargetResolver;

    impl TargetFactResolver for SharedTargetResolver {
        fn resolve<'a>(
            &'a self,
            _context: &'a ContextRef,
            logical_destinations: &'a [ResourceLocator],
            _cancellation: Option<crate::core::mutation::CancellationSignal>,
        ) -> TargetFactFuture<'a, Result<Vec<ResolvedTargetFact>, crate::error::AppError>> {
            Box::pin(async move {
                Ok(logical_destinations
                    .iter()
                    .map(|destination| ResolvedTargetFact {
                        key: PhysicalTargetKey {
                            backend: ExecutionBackend::NativeUnix,
                            physical_parent: PhysicalParentIdentity::Unix {
                                device: 7,
                                inode: 11,
                            },
                            normalized_final_child_name: "skills".to_string(),
                        },
                        destination: destination.clone(),
                        fingerprint: EntryFingerprint("missing".to_string()),
                        entry_kind: TargetEntryKind::Missing,
                        link_target: None,
                    })
                    .collect())
            })
        }
    }

    #[derive(Clone)]
    struct DistinctTargetResolver;

    impl TargetFactResolver for DistinctTargetResolver {
        fn resolve<'a>(
            &'a self,
            _context: &'a ContextRef,
            logical_destinations: &'a [ResourceLocator],
            _cancellation: Option<crate::core::mutation::CancellationSignal>,
        ) -> TargetFactFuture<'a, Result<Vec<ResolvedTargetFact>, crate::error::AppError>> {
            Box::pin(async move {
                Ok(logical_destinations
                    .iter()
                    .enumerate()
                    .map(|(index, destination)| ResolvedTargetFact {
                        key: PhysicalTargetKey {
                            backend: ExecutionBackend::NativeUnix,
                            physical_parent: PhysicalParentIdentity::Unix {
                                device: 7,
                                inode: 11 + index as u64,
                            },
                            normalized_final_child_name: "skills".to_string(),
                        },
                        destination: destination.clone(),
                        fingerprint: EntryFingerprint("missing".to_string()),
                        entry_kind: TargetEntryKind::Missing,
                        link_target: None,
                    })
                    .collect())
            })
        }
    }

    fn scope(reads_shared: bool, private_path: &str) -> ScopeDefinition {
        ScopeDefinition {
            enabled: true,
            reads_shared,
            private_path: Some(PathSpec::home(private_path)),
        }
    }

    fn resolved_scope(reads_shared: bool, private_path: &str) -> ResolvedAgentScope {
        ResolvedAgentScope {
            enabled: true,
            reads_shared,
            shared_path: Some("/home/alice/.agents/skills".to_string()),
            private_path: Some(private_path.to_string()),
            read_paths: vec![private_path.to_string()],
            shared_presence: Some(DirectoryPresenceState::Missing),
            private_presence: Some(DirectoryPresenceState::Missing),
            legacy_paths: Vec::new(),
        }
    }

    fn agent(id: &str, reads_shared: bool, private_path: &str) -> (AgentId, ResolvedAgent) {
        let id = AgentId::parse(id).unwrap();
        (
            id.clone(),
            ResolvedAgent {
                definition: AgentDefinition {
                    id,
                    display_name: "Agent".to_string(),
                    source: AgentSource::Builtin,
                    aliases: Vec::new(),
                    global: scope(reads_shared, private_path),
                    project: ScopeDefinition {
                        enabled: false,
                        reads_shared: false,
                        private_path: None,
                    },
                    detection: DetectionSpec::AnyPathExists {
                        paths: vec![PathSpec::home(".agent")],
                    },
                    legacy_paths: Vec::new(),
                    adapter: AgentAdapter::Standard,
                },
                detection: DetectionState::Detected,
                detection_reason: None,
                global: resolved_scope(reads_shared, private_path),
                project: ResolvedAgentScope {
                    enabled: false,
                    reads_shared: false,
                    shared_path: None,
                    private_path: None,
                    read_paths: Vec::new(),
                    shared_presence: None,
                    private_presence: None,
                    legacy_paths: Vec::new(),
                },
            },
        )
    }

    fn project_agent(id: &str, private_path: &str) -> (AgentId, ResolvedAgent) {
        let (id, mut agent) = agent(id, false, "/unused/global/skills");
        agent.definition.global = ScopeDefinition {
            enabled: false,
            reads_shared: false,
            private_path: None,
        };
        agent.global = ResolvedAgentScope {
            enabled: false,
            reads_shared: false,
            shared_path: None,
            private_path: None,
            read_paths: Vec::new(),
            shared_presence: None,
            private_presence: None,
            legacy_paths: Vec::new(),
        };
        agent.definition.project = ScopeDefinition {
            enabled: true,
            reads_shared: false,
            private_path: Some(PathSpec::project(private_path)),
        };
        agent.project = ResolvedAgentScope {
            enabled: true,
            reads_shared: false,
            shared_path: Some("./.agents/skills".to_string()),
            private_path: None,
            read_paths: vec![private_path.to_string()],
            shared_presence: None,
            private_presence: None,
            legacy_paths: Vec::new(),
        };
        (id, agent)
    }

    fn runtime(agents: Vec<(AgentId, ResolvedAgent)>) -> AgentRuntimeSnapshot {
        AgentRuntimeSnapshot {
            registry_revision: "registry-1".to_string(),
            environment_revision: "environment-1".to_string(),
            environment: EnvironmentRef::Host,
            availability: EnvironmentStatus::Available,
            project_path: None,
            agents: agents.into_iter().collect::<BTreeMap<_, _>>(),
        }
    }

    #[tokio::test]
    async fn catalog_classifies_direct_separate_and_additional_agents() {
        let context = ContextRef {
            environment: EnvironmentRef::Host,
            scope: ContextScope::Global,
        };
        let (codex_id, mut codex) = agent("codex", true, "/logical/codex/skills");
        codex.definition.global.private_path = None;
        codex.global.private_path = None;
        let catalog = build_agent_selection_catalog(
            &context,
            &runtime(vec![
                (codex_id, codex),
                agent("claude-code", false, "/logical/claude/skills"),
                agent("cursor", true, "/logical/cursor/skills"),
            ]),
            &[],
            &DistinctTargetResolver,
        )
        .await
        .unwrap();

        assert_eq!(
            catalog.snapshot.direct_agent_ids,
            vec![
                AgentId::parse("codex").unwrap(),
                AgentId::parse("cursor").unwrap(),
            ]
        );
        assert!(catalog.snapshot.items.iter().any(|item| {
            item.agent_ids == vec![AgentId::parse("claude-code").unwrap()]
                && item.category == AgentSelectionCategory::SeparateInstall
        }));
        assert!(catalog.snapshot.items.iter().any(|item| {
            item.agent_ids == vec![AgentId::parse("cursor").unwrap()]
                && item.category == AgentSelectionCategory::AdditionalInstall
        }));
        assert_eq!(catalog.snapshot.requested_mode_item_ids.len(), 2);
    }

    #[tokio::test]
    async fn catalog_merges_shared_standard_placements_into_one_item() {
        let context = ContextRef {
            environment: EnvironmentRef::Host,
            scope: ContextScope::Global,
        };
        let catalog = build_agent_selection_catalog(
            &context,
            &runtime(vec![
                agent("claude-code", false, "/logical/claude/skills"),
                agent("cursor", true, "/logical/cursor/skills"),
            ]),
            &[],
            &SharedTargetResolver,
        )
        .await
        .unwrap();

        assert_eq!(catalog.snapshot.items.len(), 1);
        assert_eq!(
            catalog.snapshot.items[0].category,
            AgentSelectionCategory::SeparateInstall
        );
        assert_eq!(catalog.snapshot.items[0].agent_ids.len(), 2);
        assert_eq!(
            catalog.snapshot.direct_agent_ids,
            vec![AgentId::parse("cursor").unwrap()]
        );
    }

    #[tokio::test]
    async fn catalog_exposes_eve_targets_as_copy_only_group_children() {
        let context = ContextRef {
            environment: EnvironmentRef::Host,
            scope: ContextScope::Project {
                project_id: "project-1".to_string(),
            },
        };
        let (eve_id, mut eve) = project_agent("eve", ".unused/skills");
        eve.definition.adapter = AgentAdapter::Eve;
        eve.definition.display_name = "Eve".to_string();
        eve.definition.project.private_path = None;
        eve.project.private_path = None;
        let mut runtime = runtime(vec![(eve_id.clone(), eve)]);
        runtime.project_path = Some("/workspace/project".to_string());
        let targets = vec![
            crate::models::InstallTargetInfo {
                target_id: "eve:root".to_string(),
                agent: eve_id.clone(),
                display_name: "Eve (root)".to_string(),
                subagent: None,
                path: "/workspace/project/agent/skills".to_string(),
            },
            crate::models::InstallTargetInfo {
                target_id: "eve:research".to_string(),
                agent: eve_id.clone(),
                display_name: "Research".to_string(),
                subagent: Some("research".to_string()),
                path: "/workspace/project/agent/subagents/research/skills".to_string(),
            },
        ];

        let catalog =
            build_agent_selection_catalog(&context, &runtime, &targets, &DistinctTargetResolver)
                .await
                .unwrap();

        assert_eq!(catalog.snapshot.groups.len(), 1);
        assert_eq!(catalog.snapshot.groups[0].display_name, "Eve");
        assert_eq!(catalog.snapshot.items.len(), 2);
        assert!(catalog.snapshot.items.iter().all(|item| {
            item.category == AgentSelectionCategory::GroupChild
                && item.mode_constraint == AgentSelectionModeConstraint::CopyOnly
        }));
        assert!(catalog.snapshot.requested_mode_item_ids.is_empty());
    }

    #[tokio::test]
    async fn catalog_marks_and_rejects_selected_placements_that_require_different_content() {
        let context = ContextRef {
            environment: EnvironmentRef::Host,
            scope: ContextScope::Project {
                project_id: "project-1".to_string(),
            },
        };
        let (eve_id, mut eve) = project_agent("eve", ".unused/skills");
        eve.definition.adapter = AgentAdapter::Eve;
        eve.definition.project.private_path = None;
        eve.project.private_path = None;
        let (standard_id, mut standard) = project_agent("claude-code", ".claude/skills");
        standard.project.private_path = Some("/workspace/project/agent/skills".to_string());
        let mut runtime = runtime(vec![(standard_id, standard), (eve_id.clone(), eve)]);
        runtime.project_path = Some("/workspace/project".to_string());
        let targets = vec![crate::models::InstallTargetInfo {
            target_id: "eve:root".to_string(),
            agent: eve_id,
            display_name: "Eve (root)".to_string(),
            subagent: None,
            path: "/workspace/project/agent/skills".to_string(),
        }];

        let catalog =
            build_agent_selection_catalog(&context, &runtime, &targets, &SharedTargetResolver)
                .await
                .unwrap();

        assert_eq!(catalog.snapshot.items.len(), 2);
        assert!(catalog.snapshot.items.iter().all(|item| {
            item.selectable
                && item.disabled_reason == Some(AgentSelectionDisabledReason::PlacementConflict)
        }));
        let selection = AgentSelectionSubmission {
            revision: catalog.snapshot.revision.clone(),
            selected_item_ids: catalog
                .snapshot
                .items
                .iter()
                .map(|item| item.id.clone())
                .collect(),
            requested_mode: crate::models::InstallMode::Copy,
        };
        assert!(matches!(
            resolve_agent_selection_submission(&catalog, &selection),
            Err(crate::error::AppError::AgentSelectionInvalid {
                reason: crate::error::AgentSelectionInvalidReason::PlacementConflict
            })
        ));
    }

    #[tokio::test]
    async fn submission_resolves_selected_items_and_direct_agents_to_internal_intents() {
        let context = ContextRef {
            environment: EnvironmentRef::Host,
            scope: ContextScope::Global,
        };
        let (codex_id, mut codex) = agent("codex", true, "/logical/codex/skills");
        codex.definition.global.private_path = None;
        codex.global.private_path = None;
        let catalog = build_agent_selection_catalog(
            &context,
            &runtime(vec![
                (codex_id.clone(), codex),
                agent("claude-code", false, "/logical/claude/skills"),
                agent("cursor", true, "/logical/cursor/skills"),
            ]),
            &[],
            &DistinctTargetResolver,
        )
        .await
        .unwrap();
        let selected_item_ids = catalog
            .snapshot
            .items
            .iter()
            .map(|item| item.id.clone())
            .collect();

        let resolved = resolve_agent_selection_submission(
            &catalog,
            &AgentSelectionSubmission {
                revision: catalog.snapshot.revision.clone(),
                selected_item_ids,
                requested_mode: crate::models::InstallMode::Symlink,
            },
        )
        .unwrap();
        let AgentSelectionResolution::Ready(intents) = resolved else {
            panic!("current selection must resolve");
        };

        assert_eq!(intents.len(), 3);
        assert!(intents.iter().any(|intent| {
            intent.agent_id == codex_id
                && intent.private_entry
                    == crate::application::agent_intent::PrivateEntryIntent::None
        }));
        assert!(intents.iter().any(|intent| {
            intent.agent_id.as_str() == "claude-code"
                && intent.private_entry
                    == crate::application::agent_intent::PrivateEntryIntent::Required
        }));
        assert!(intents.iter().any(|intent| {
            intent.agent_id.as_str() == "cursor"
                && intent.private_entry
                    == crate::application::agent_intent::PrivateEntryIntent::OptionalSelected
        }));
    }

    #[tokio::test]
    async fn stale_submission_returns_the_latest_snapshot_without_intents() {
        let context = ContextRef {
            environment: EnvironmentRef::Host,
            scope: ContextScope::Global,
        };
        let catalog = build_agent_selection_catalog(
            &context,
            &runtime(vec![agent("claude-code", false, "/logical/claude/skills")]),
            &[],
            &DistinctTargetResolver,
        )
        .await
        .unwrap();

        let resolved = resolve_agent_selection_submission(
            &catalog,
            &AgentSelectionSubmission {
                revision: AgentSelectionRevision("selection-v1-stale".to_string()),
                selected_item_ids: Vec::new(),
                requested_mode: crate::models::InstallMode::Copy,
            },
        )
        .unwrap();

        assert!(matches!(resolved, AgentSelectionResolution::Stale(_)));
    }

    #[tokio::test]
    async fn revision_tracks_submit_semantics_but_ignores_display_and_detection() {
        let context = ContextRef {
            environment: EnvironmentRef::Host,
            scope: ContextScope::Global,
        };
        let (id, mut direct) = agent("codex", true, "/logical/codex/skills");
        direct.definition.global.private_path = None;
        direct.global.private_path = None;
        let original = build_agent_selection_catalog(
            &context,
            &runtime(vec![(id.clone(), direct.clone())]),
            &[],
            &DistinctTargetResolver,
        )
        .await
        .unwrap();

        direct.definition.display_name = "Codex renamed".to_string();
        direct.detection = DetectionState::NotDetected;
        let display_only = build_agent_selection_catalog(
            &context,
            &runtime(vec![(id.clone(), direct.clone())]),
            &[],
            &DistinctTargetResolver,
        )
        .await
        .unwrap();
        assert_eq!(original.snapshot.revision, display_only.snapshot.revision);

        direct.definition.global.reads_shared = false;
        direct.global.reads_shared = false;
        let changed_semantics = build_agent_selection_catalog(
            &context,
            &runtime(vec![(id, direct)]),
            &[],
            &DistinctTargetResolver,
        )
        .await
        .unwrap();
        assert_ne!(
            original.snapshot.revision,
            changed_semantics.snapshot.revision
        );
    }
}

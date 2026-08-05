use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::application::agent_intent::{AdapterTargetId, AgentWriteIntent};
use crate::application::mutation::plan::stable_digest;
use crate::application::workflow_planner::{
    AgentEntryContent, AgentEntryPlan, LogicalAgentEntryRoot,
};
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
pub struct AgentInstallOptionId(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(transparent)]
pub struct AgentSelectionRevision(pub String);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub enum SkillDirectoryAccess {
    SharedOnly,
    PrivateOnly,
    Both,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub enum AgentSelectionAgentKind {
    Standard,
    Grouped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub enum AgentInstallOptionKind {
    StandardDirectory,
    GroupLocation,
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
    pub kind: AgentSelectionAgentKind,
    pub id: AgentId,
    pub display_name: String,
    pub detection: DetectionState,
    pub directory_access: Option<SkillDirectoryAccess>,
    pub install_option_id: Option<AgentInstallOptionId>,
    pub group_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct AgentInstallOption {
    pub id: AgentInstallOptionId,
    pub kind: AgentInstallOptionKind,
    pub agent_ids: Vec<AgentId>,
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
pub struct AgentSelectionGroup {
    pub id: String,
    pub agent_id: AgentId,
    pub display_name: String,
    pub option_ids: Vec<AgentInstallOptionId>,
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
    pub install_options: Vec<AgentInstallOption>,
    pub groups: Vec<AgentSelectionGroup>,
    pub initial_selected_option_ids: Vec<AgentInstallOptionId>,
    pub unavailable_explicit_agents: Vec<UnavailableAgentSelection>,
    pub user_mode_option_ids: Vec<AgentInstallOptionId>,
    pub revision: AgentSelectionRevision,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct AgentSelectionSubmission {
    pub revision: AgentSelectionRevision,
    pub selected_option_ids: Vec<AgentInstallOptionId>,
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
pub(crate) struct ResolvedAgentInstallOption {
    pub public: AgentInstallOption,
    pub root: ResourceLocator,
    pub adapter_target_ids: Vec<String>,
    physical_key: PhysicalTargetKey,
    content: AgentSelectionContent,
}

impl ResolvedAgentInstallOption {
    pub(crate) fn target_id(&self) -> String {
        self.adapter_target_ids
            .first()
            .cloned()
            .unwrap_or_else(|| format!("agent-install-option:{}", self.public.id.0))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
enum AgentSelectionContent {
    Canonical,
    EveDerived { subagent: Option<String> },
}

#[derive(Debug, Clone)]
pub(crate) struct AgentSelectionCatalog {
    pub snapshot: AgentSelectionSnapshot,
    pub resolved_options: BTreeMap<AgentInstallOptionId, ResolvedAgentInstallOption>,
}

#[derive(Debug, Clone)]
pub(crate) enum AgentSelectionResolution {
    Ready(ResolvedAgentSelection),
    Stale,
}

#[derive(Debug, Clone)]
pub(crate) struct ResolvedAgentSelection {
    pub intents: Vec<AgentWriteIntent>,
    direct_agent_ids: Vec<AgentId>,
    selected_options: Vec<ResolvedAgentInstallOption>,
}

impl ResolvedAgentSelection {
    pub(crate) fn intents(&self) -> &[AgentWriteIntent] {
        &self.intents
    }

    pub fn entry_plan(&self, include_all_direct_agents: bool) -> AgentEntryPlan {
        let selected_agent_ids = self
            .selected_options
            .iter()
            .flat_map(|option| option.public.agent_ids.iter().cloned())
            .collect::<BTreeSet<_>>();
        let mut canonical_owner_agent_ids = self
            .direct_agent_ids
            .iter()
            .filter(|agent_id| include_all_direct_agents || selected_agent_ids.contains(*agent_id))
            .cloned()
            .collect::<Vec<_>>();
        canonical_owner_agent_ids.sort();
        canonical_owner_agent_ids.dedup();
        let required_agent_roots = self
            .selected_options
            .iter()
            .map(|option| LogicalAgentEntryRoot {
                target_id: option.target_id(),
                root: option.root.clone(),
                owner_agent_ids: option.public.agent_ids.clone(),
                content: match &option.content {
                    AgentSelectionContent::Canonical => AgentEntryContent::Canonical,
                    AgentSelectionContent::EveDerived { subagent } => {
                        AgentEntryContent::EveDerived {
                            subagent: subagent.clone(),
                        }
                    }
                },
            })
            .collect();
        AgentEntryPlan {
            canonical_owner_agent_ids,
            required_agent_roots,
        }
    }
}

pub(crate) fn map_agent_intents_to_submission(
    catalog: &AgentSelectionCatalog,
    intents: &[AgentWriteIntent],
    requested_mode: InstallMode,
) -> AgentSelectionSubmission {
    let intents_by_agent = intents
        .iter()
        .map(|intent| (&intent.agent_id, intent))
        .collect::<BTreeMap<_, _>>();
    let access_by_agent = catalog
        .snapshot
        .agents
        .iter()
        .map(|agent| (&agent.id, agent.directory_access))
        .collect::<BTreeMap<_, _>>();
    let selected_option_ids = catalog
        .resolved_options
        .values()
        .filter(|option| option.public.selectable)
        .filter(|option| match option.public.kind {
            AgentInstallOptionKind::StandardDirectory => option.public.agent_ids.iter().any(|id| {
                intents_by_agent.get(id).is_some_and(|intent| {
                    intent.own_directory_selected
                        || access_by_agent.get(id) == Some(&Some(SkillDirectoryAccess::PrivateOnly))
                })
            }),
            AgentInstallOptionKind::GroupLocation => option.public.agent_ids.iter().any(|id| {
                intents_by_agent.get(id).is_some_and(|intent| {
                    option.adapter_target_ids.iter().any(|target_id| {
                        intent
                            .adapter_targets
                            .iter()
                            .any(|requested| requested.0 == *target_id)
                    })
                })
            }),
        })
        .map(|option| option.public.id.clone())
        .collect();
    AgentSelectionSubmission {
        revision: catalog.snapshot.revision.clone(),
        selected_option_ids,
        requested_mode,
    }
}

pub(crate) fn resolve_agent_selection_submission(
    catalog: &AgentSelectionCatalog,
    submission: &AgentSelectionSubmission,
) -> Result<AgentSelectionResolution, AppError> {
    if submission.revision != catalog.snapshot.revision {
        return Ok(AgentSelectionResolution::Stale);
    }

    let mut selected_ids = std::collections::BTreeSet::new();
    for option_id in &submission.selected_option_ids {
        if !selected_ids.insert(option_id.clone()) {
            return Err(selection_validation(
                AgentSelectionInvalidReason::DuplicateOption,
            ));
        }
        let Some(option) = catalog.resolved_options.get(option_id) else {
            return Ok(AgentSelectionResolution::Stale);
        };
        if !option.public.selectable {
            return Err(selection_validation(
                AgentSelectionInvalidReason::OptionUnavailable,
            ));
        }
    }
    let mut selected_content_by_target =
        BTreeMap::<PhysicalTargetKey, BTreeSet<AgentSelectionContent>>::new();
    for option_id in &selected_ids {
        let option = catalog
            .resolved_options
            .get(option_id)
            .expect("selected option was validated above");
        selected_content_by_target
            .entry(option.physical_key.clone())
            .or_default()
            .insert(option.content.clone());
    }
    if selected_content_by_target
        .values()
        .any(|contents| contents.len() > 1)
    {
        return Err(selection_validation(
            AgentSelectionInvalidReason::PlacementConflict,
        ));
    }

    let direct_agents = catalog.snapshot.agents.iter().filter(|agent| {
        matches!(
            agent.directory_access,
            Some(SkillDirectoryAccess::SharedOnly | SkillDirectoryAccess::Both)
        )
    });
    let direct_agent_ids = direct_agents
        .clone()
        .map(|agent| agent.id.clone())
        .collect::<Vec<_>>();
    let mut intents = BTreeMap::<AgentId, AgentWriteIntent>::new();
    for agent in direct_agents {
        intents.insert(
            agent.id.clone(),
            AgentWriteIntent {
                agent_id: agent.id.clone(),
                own_directory_selected: false,
                adapter_targets: Vec::new(),
            },
        );
    }

    let selected_options = selected_ids
        .iter()
        .map(|option_id| {
            catalog
                .resolved_options
                .get(option_id)
                .expect("selected option was validated above")
                .clone()
        })
        .collect::<Vec<_>>();
    for option_id in selected_ids {
        let option = catalog
            .resolved_options
            .get(&option_id)
            .expect("selected option was validated above");
        for agent_id in &option.public.agent_ids {
            let intent = intents
                .entry(agent_id.clone())
                .or_insert_with(|| AgentWriteIntent {
                    agent_id: agent_id.clone(),
                    own_directory_selected: false,
                    adapter_targets: Vec::new(),
                });
            match option.public.kind {
                AgentInstallOptionKind::StandardDirectory => {
                    intent.own_directory_selected = true;
                }
                AgentInstallOptionKind::GroupLocation => {
                    intent.adapter_targets.extend(
                        option
                            .adapter_target_ids
                            .iter()
                            .cloned()
                            .map(AdapterTargetId),
                    );
                }
            }
        }
    }

    let mut intents = intents.into_values().collect::<Vec<_>>();
    for intent in &mut intents {
        intent.adapter_targets.sort();
        intent.adapter_targets.dedup();
    }
    Ok(AgentSelectionResolution::Ready(ResolvedAgentSelection {
        intents,
        direct_agent_ids,
        selected_options,
    }))
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
    let access_by_agent = catalog
        .snapshot
        .agents
        .iter()
        .map(|agent| (&agent.id, agent.directory_access))
        .collect::<BTreeMap<_, _>>();
    catalog.snapshot.initial_selected_option_ids = catalog
        .snapshot
        .install_options
        .iter()
        .filter(|option| {
            option.selectable
                && option.agent_ids.iter().any(|agent_id| {
                    requested.contains(agent_id)
                        && (option.kind == AgentInstallOptionKind::GroupLocation
                            || access_by_agent.get(agent_id)
                                == Some(&Some(SkillDirectoryAccess::PrivateOnly)))
                })
        })
        .map(|option| option.id.clone())
        .collect();
}

struct CatalogCandidate {
    agent_id: AgentId,
    display_name: String,
    path: String,
    destination: ResourceLocator,
    shared_destination: Option<ResourceLocator>,
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
    let mut candidates = Vec::new();
    for (agent_id, agent) in &runtime.agents {
        let scope = selected_scope(context, &agent.global, &agent.project);
        if !scope.enabled {
            continue;
        }
        let kind = match agent.definition.adapter {
            AgentAdapter::Standard => AgentSelectionAgentKind::Standard,
            AgentAdapter::Eve => AgentSelectionAgentKind::Grouped,
        };
        let reads_shared = scope.reads_shared && scope.shared_path.is_some();
        let directory_access = match (
            agent.definition.adapter,
            reads_shared,
            scope.private_path.is_some(),
        ) {
            (AgentAdapter::Standard, true, false) => Some(SkillDirectoryAccess::SharedOnly),
            (AgentAdapter::Standard, false, true) => Some(SkillDirectoryAccess::PrivateOnly),
            (AgentAdapter::Standard, true, true) => Some(SkillDirectoryAccess::Both),
            _ => None,
        };
        agents.push(AgentSelectionAgent {
            kind,
            id: agent_id.clone(),
            display_name: agent.definition.display_name.clone(),
            detection: agent.detection,
            directory_access,
            install_option_id: None,
            group_id: None,
        });
        if agent.definition.adapter != AgentAdapter::Standard {
            continue;
        }
        let Some(path) = scope.private_path.clone() else {
            continue;
        };
        candidates.push(CatalogCandidate {
            agent_id: agent_id.clone(),
            display_name: agent.definition.display_name.clone(),
            path: path.clone(),
            destination: ResourceLocator {
                environment: context.environment.clone(),
                native_path: path,
            },
            shared_destination: scope
                .shared_path
                .clone()
                .map(|native_path| ResourceLocator {
                    environment: context.environment.clone(),
                    native_path,
                }),
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
    let shared_destinations = candidates
        .iter()
        .filter_map(|candidate| candidate.shared_destination.clone())
        .collect::<Vec<_>>();
    let shared_facts = if shared_destinations.is_empty() {
        Vec::new()
    } else {
        targets.resolve(context, &shared_destinations, None).await?
    };
    if shared_facts.len() != shared_destinations.len() {
        return Err(AppError::StaleTarget);
    }

    let mut by_physical_key =
        BTreeMap::<PhysicalTargetKey, Vec<(CatalogCandidate, ResolvedTargetFact)>>::new();
    let mut shared_facts = shared_facts.into_iter();
    for (candidate, fact) in candidates.into_iter().zip(facts) {
        let duplicates_shared = candidate.shared_destination.is_some()
            && shared_facts
                .next()
                .is_some_and(|shared_fact| shared_fact.key == fact.key);
        if duplicates_shared {
            if let Some(agent) = agents
                .iter_mut()
                .find(|agent| agent.id == candidate.agent_id)
            {
                agent.directory_access = Some(SkillDirectoryAccess::SharedOnly);
            }
            continue;
        }
        by_physical_key
            .entry(fact.key.clone())
            .or_default()
            .push((candidate, fact));
    }

    let mut install_options = Vec::new();
    let mut resolved_options = BTreeMap::new();
    for (physical_key, mut members) in by_physical_key {
        members.sort_by(|left, right| left.0.agent_id.cmp(&right.0.agent_id));
        let agent_ids = members
            .iter()
            .map(|(candidate, _)| candidate.agent_id.clone())
            .collect::<Vec<_>>();
        let id = AgentInstallOptionId(stable_digest(&(
            "agent-install-option-v2",
            context,
            &physical_key,
            AgentSelectionContent::Canonical,
            AgentSelectionModeConstraint::UserSelectable,
        ))?);
        for agent_id in &agent_ids {
            if let Some(agent) = agents.iter_mut().find(|agent| &agent.id == agent_id) {
                agent.install_option_id = Some(id.clone());
            }
        }
        let display_name = members[0].0.display_name.clone();
        let public = AgentInstallOption {
            id: id.clone(),
            kind: AgentInstallOptionKind::StandardDirectory,
            agent_ids,
            display_name,
            path: members[0].0.path.clone(),
            group_id: None,
            selectable: true,
            mode_constraint: AgentSelectionModeConstraint::UserSelectable,
            disabled_reason: None,
        };
        install_options.push(public.clone());
        resolved_options.insert(
            id,
            ResolvedAgentInstallOption {
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
            if let Some(agent) = agents.iter_mut().find(|agent| agent.id == *eve_id) {
                agent.group_id = Some(group_id.clone());
            }
            let mut option_ids = Vec::new();
            for (target, fact) in available_targets.into_iter().zip(facts) {
                let content = AgentSelectionContent::EveDerived {
                    subagent: target.subagent.clone(),
                };
                let id = AgentInstallOptionId(stable_digest(&(
                    "agent-install-option-v2",
                    context,
                    &fact.key,
                    &content,
                    AgentSelectionModeConstraint::CopyOnly,
                    &target.target_id,
                ))?);
                let public = AgentInstallOption {
                    id: id.clone(),
                    kind: AgentInstallOptionKind::GroupLocation,
                    agent_ids: vec![target.agent.clone()],
                    display_name: target.display_name.clone(),
                    path: target.path.clone(),
                    group_id: Some(group_id.clone()),
                    selectable: true,
                    mode_constraint: AgentSelectionModeConstraint::CopyOnly,
                    disabled_reason: None,
                };
                option_ids.push(id.clone());
                install_options.push(public.clone());
                resolved_options.insert(
                    id,
                    ResolvedAgentInstallOption {
                        public,
                        root: fact.destination,
                        adapter_target_ids: vec![target.target_id.clone()],
                        physical_key: fact.key,
                        content,
                    },
                );
            }
            groups.push(AgentSelectionGroup {
                id: group_id,
                agent_id: eve_id.clone(),
                display_name: eve.definition.display_name.clone(),
                option_ids,
                detection: eve.detection,
            });
        }
    }
    let content_by_target = resolved_options.values().fold(
        BTreeMap::<PhysicalTargetKey, BTreeSet<AgentSelectionContent>>::new(),
        |mut grouped, item| {
            grouped
                .entry(item.physical_key.clone())
                .or_default()
                .insert(item.content.clone());
            grouped
        },
    );
    let conflicts = content_by_target
        .into_iter()
        .filter_map(|(key, contents)| (contents.len() > 1).then_some(key))
        .collect::<BTreeSet<_>>();
    if !conflicts.is_empty() {
        for resolved in resolved_options.values_mut() {
            if conflicts.contains(&resolved.physical_key) {
                resolved.public.disabled_reason =
                    Some(AgentSelectionDisabledReason::PlacementConflict);
            }
        }
        for option in &mut install_options {
            if let Some(resolved) = resolved_options.get(&option.id) {
                *option = resolved.public.clone();
            }
        }
    }
    install_options.sort_by(|left, right| {
        left.kind
            .cmp(&right.kind)
            .then_with(|| left.display_name.cmp(&right.display_name))
            .then_with(|| left.id.cmp(&right.id))
    });
    let user_mode_option_ids = install_options
        .iter()
        .filter(|option| {
            option.selectable
                && option.mode_constraint == AgentSelectionModeConstraint::UserSelectable
        })
        .map(|option| option.id.clone())
        .collect::<Vec<_>>();
    let semantic_agents = agents
        .iter()
        .map(|agent| {
            (
                &agent.id,
                agent.kind,
                agent.directory_access,
                &agent.install_option_id,
                &agent.group_id,
            )
        })
        .collect::<Vec<_>>();
    let semantic_options = install_options
        .iter()
        .map(|option| {
            (
                &option.id,
                &option.agent_ids,
                option.kind,
                option.mode_constraint,
                option.selectable,
            )
        })
        .collect::<Vec<_>>();
    let revision = AgentSelectionRevision(stable_digest(&(
        "agent-selection-revision-v2",
        context,
        semantic_agents,
        semantic_options,
    ))?);

    Ok(AgentSelectionCatalog {
        snapshot: AgentSelectionSnapshot {
            agents,
            install_options,
            groups,
            initial_selected_option_ids: Vec::new(),
            unavailable_explicit_agents: Vec::new(),
            user_mode_option_ids,
            revision,
        },
        resolved_options,
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
        selected_option_ids: catalog.snapshot.initial_selected_option_ids,
        requested_mode,
    }
}

#[cfg(test)]
pub(crate) async fn test_submission_for_agents_and_own_directories<T: TargetFactResolver>(
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
    let requested = agent_ids.iter().copied().collect::<BTreeSet<_>>();
    apply_initial_agent_selection(
        &mut catalog,
        &agent_ids
            .iter()
            .map(|id| id.to_string())
            .collect::<Vec<_>>(),
    );
    catalog.snapshot.initial_selected_option_ids = catalog
        .snapshot
        .install_options
        .iter()
        .filter(|option| {
            option
                .agent_ids
                .iter()
                .any(|agent_id| requested.contains(agent_id.as_str()))
        })
        .map(|option| option.id.clone())
        .collect();
    AgentSelectionSubmission {
        revision: catalog.snapshot.revision,
        selected_option_ids: catalog.snapshot.initial_selected_option_ids,
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
    use crate::models::InstallMode;

    use super::{
        apply_initial_agent_selection, build_agent_selection_catalog,
        resolve_agent_selection_submission, AgentInstallOptionKind, AgentSelectionDisabledReason,
        AgentSelectionModeConstraint, AgentSelectionResolution, AgentSelectionRevision,
        AgentSelectionSubmission, SkillDirectoryAccess,
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
                                inode: if destination.native_path.ends_with(".agents/skills") {
                                    99
                                } else {
                                    11
                                },
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
                                inode: if destination.native_path.ends_with(".agents/skills") {
                                    99
                                } else {
                                    11 + index as u64
                                },
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

        let access = catalog
            .snapshot
            .agents
            .iter()
            .map(|agent| (agent.id.as_str(), agent.directory_access))
            .collect::<BTreeMap<_, _>>();
        assert_eq!(access["codex"], Some(SkillDirectoryAccess::SharedOnly));
        assert_eq!(
            access["claude-code"],
            Some(SkillDirectoryAccess::PrivateOnly)
        );
        assert_eq!(access["cursor"], Some(SkillDirectoryAccess::Both));
        assert!(catalog.snapshot.install_options.iter().any(|option| {
            option.agent_ids == vec![AgentId::parse("claude-code").unwrap()]
                && option.kind == AgentInstallOptionKind::StandardDirectory
        }));
        assert!(catalog.snapshot.install_options.iter().any(|option| {
            option.agent_ids == vec![AgentId::parse("cursor").unwrap()]
                && option.kind == AgentInstallOptionKind::StandardDirectory
        }));
        assert_eq!(catalog.snapshot.user_mode_option_ids.len(), 2);
    }

    #[tokio::test]
    async fn initial_selection_does_not_select_an_optional_private_directory() {
        let context = ContextRef {
            environment: EnvironmentRef::Host,
            scope: ContextScope::Global,
        };
        let mut catalog = build_agent_selection_catalog(
            &context,
            &runtime(vec![agent("cursor", true, "/logical/cursor/skills")]),
            &[],
            &DistinctTargetResolver,
        )
        .await
        .unwrap();

        apply_initial_agent_selection(&mut catalog, &["cursor".to_string()]);

        assert!(catalog.snapshot.initial_selected_option_ids.is_empty());
    }

    #[tokio::test]
    async fn catalog_normalizes_a_private_only_agent_when_its_directory_is_the_shared_directory() {
        let context = ContextRef {
            environment: EnvironmentRef::Host,
            scope: ContextScope::Global,
        };
        let mut duplicate = agent("cursor", false, "/home/alice/.agents/skills").1;
        duplicate.global.shared_path = Some("/home/alice/.agents/skills".to_string());
        let catalog = build_agent_selection_catalog(
            &context,
            &runtime(vec![(AgentId::parse("cursor").unwrap(), duplicate)]),
            &[],
            &DistinctTargetResolver,
        )
        .await
        .unwrap();

        assert_eq!(
            catalog.snapshot.agents[0].directory_access,
            Some(SkillDirectoryAccess::SharedOnly)
        );
        assert!(catalog.snapshot.install_options.is_empty());

        let resolution = resolve_agent_selection_submission(
            &catalog,
            &AgentSelectionSubmission {
                revision: catalog.snapshot.revision.clone(),
                selected_option_ids: Vec::new(),
                requested_mode: InstallMode::Symlink,
            },
        )
        .unwrap();
        let AgentSelectionResolution::Ready(selection) = resolution else {
            panic!("current selection must resolve");
        };
        assert_eq!(
            selection.entry_plan(true).canonical_owner_agent_ids,
            vec![AgentId::parse("cursor").unwrap()]
        );
    }

    #[tokio::test]
    async fn catalog_merges_shared_standard_placements_into_one_option() {
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

        assert_eq!(catalog.snapshot.install_options.len(), 1);
        assert_eq!(
            catalog.snapshot.install_options[0].kind,
            AgentInstallOptionKind::StandardDirectory
        );
        assert_eq!(catalog.snapshot.install_options[0].agent_ids.len(), 2);
        assert_eq!(
            catalog
                .snapshot
                .agents
                .iter()
                .find(|agent| agent.id.as_str() == "cursor")
                .and_then(|agent| agent.directory_access),
            Some(SkillDirectoryAccess::Both)
        );

        let resolution = resolve_agent_selection_submission(
            &catalog,
            &AgentSelectionSubmission {
                revision: catalog.snapshot.revision.clone(),
                selected_option_ids: vec![catalog.snapshot.install_options[0].id.clone()],
                requested_mode: InstallMode::Symlink,
            },
        )
        .unwrap();
        let AgentSelectionResolution::Ready(selection) = resolution else {
            panic!("current selection must resolve");
        };
        let plan = selection.entry_plan(true);
        assert_eq!(plan.required_agent_roots.len(), 1);
        assert_eq!(plan.required_agent_roots[0].owner_agent_ids.len(), 2);
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
        assert_eq!(catalog.snapshot.install_options.len(), 2);
        assert!(catalog.snapshot.install_options.iter().all(|option| {
            option.kind == AgentInstallOptionKind::GroupLocation
                && option.mode_constraint == AgentSelectionModeConstraint::CopyOnly
        }));
        assert!(catalog.snapshot.user_mode_option_ids.is_empty());
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

        assert_eq!(catalog.snapshot.install_options.len(), 2);
        assert!(catalog.snapshot.install_options.iter().all(|option| {
            option.selectable
                && option.disabled_reason == Some(AgentSelectionDisabledReason::PlacementConflict)
        }));
        let selection = AgentSelectionSubmission {
            revision: catalog.snapshot.revision.clone(),
            selected_option_ids: catalog
                .snapshot
                .install_options
                .iter()
                .map(|option| option.id.clone())
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
    async fn submission_resolves_selected_options_and_direct_agents_to_internal_intents() {
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
        let selected_option_ids = catalog
            .snapshot
            .install_options
            .iter()
            .map(|option| option.id.clone())
            .collect();

        let resolved = resolve_agent_selection_submission(
            &catalog,
            &AgentSelectionSubmission {
                revision: catalog.snapshot.revision.clone(),
                selected_option_ids,
                requested_mode: crate::models::InstallMode::Symlink,
            },
        )
        .unwrap();
        let AgentSelectionResolution::Ready(selection) = resolved else {
            panic!("current selection must resolve");
        };
        let intents = selection.intents;

        assert_eq!(intents.len(), 3);
        assert!(intents
            .iter()
            .any(|intent| { intent.agent_id == codex_id && !intent.own_directory_selected }));
        assert!(intents.iter().any(|intent| {
            intent.agent_id.as_str() == "claude-code" && intent.own_directory_selected
        }));
        assert!(intents.iter().any(|intent| {
            intent.agent_id.as_str() == "cursor" && intent.own_directory_selected
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
                selected_option_ids: Vec::new(),
                requested_mode: crate::models::InstallMode::Copy,
            },
        )
        .unwrap();

        assert!(matches!(resolved, AgentSelectionResolution::Stale));
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

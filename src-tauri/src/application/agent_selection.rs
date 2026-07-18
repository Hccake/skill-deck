use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::application::mutation::plan::stable_digest;
use crate::core::agent_definition::{AgentId, PathSpec};
use crate::environment::agent_environment::{AgentRuntimeSnapshot, ResolvedAgentScope};
use crate::environment::planning::TargetFactResolver;
use crate::environment::types::{ContextRef, ResourceLocator};
use crate::error::AppError;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct AgentSelectionGroups {
    pub global: Vec<AgentSelectionGroup>,
    pub project: Vec<AgentSelectionGroup>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct AgentSelectionGroup {
    pub group_id: String,
    pub agent_ids: Vec<AgentId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
enum SelectionScope {
    Global,
    Project,
}

struct SelectionCandidate {
    scope: SelectionScope,
    agent_id: AgentId,
    logical_key: String,
    destination: Option<ResourceLocator>,
}

pub async fn resolve_agent_selection_groups<T: TargetFactResolver>(
    context: &ContextRef,
    runtime: &AgentRuntimeSnapshot,
    targets: &T,
) -> Result<AgentSelectionGroups, AppError> {
    let mut candidates = Vec::new();
    for (agent_id, agent) in &runtime.agents {
        collect_candidate(
            &mut candidates,
            context,
            SelectionScope::Global,
            agent_id,
            &agent.definition.global.private_path,
            &agent.global,
        )?;
        collect_candidate(
            &mut candidates,
            context,
            SelectionScope::Project,
            agent_id,
            &agent.definition.project.private_path,
            &agent.project,
        )?;
    }

    let resolved_indexes = candidates
        .iter()
        .enumerate()
        .filter_map(|(index, candidate)| {
            candidate.destination.clone().map(|target| (index, target))
        })
        .collect::<Vec<_>>();
    let destinations = resolved_indexes
        .iter()
        .map(|(_, destination)| destination.clone())
        .collect::<Vec<_>>();

    let physical_keys = if destinations.is_empty() {
        None
    } else {
        targets
            .resolve(context, &destinations, None)
            .await
            .ok()
            .filter(|facts| facts.len() == destinations.len())
    };

    if let Some(facts) = physical_keys {
        for ((index, _), fact) in resolved_indexes.into_iter().zip(facts) {
            candidates[index].logical_key = stable_digest(&(
                "agent-selection-physical-v1",
                candidates[index].scope,
                fact.key,
            ))?;
        }
    }

    let mut global = BTreeMap::<String, Vec<AgentId>>::new();
    let mut project = BTreeMap::<String, Vec<AgentId>>::new();
    for candidate in candidates {
        let groups = match candidate.scope {
            SelectionScope::Global => &mut global,
            SelectionScope::Project => &mut project,
        };
        groups
            .entry(candidate.logical_key)
            .or_default()
            .push(candidate.agent_id);
    }

    Ok(AgentSelectionGroups {
        global: into_groups(global),
        project: into_groups(project),
    })
}

fn collect_candidate(
    candidates: &mut Vec<SelectionCandidate>,
    context: &ContextRef,
    scope: SelectionScope,
    agent_id: &AgentId,
    path_spec: &Option<PathSpec>,
    resolved_scope: &ResolvedAgentScope,
) -> Result<(), AppError> {
    if !resolved_scope.enabled {
        return Ok(());
    }
    let Some(path_spec) = path_spec else {
        return Ok(());
    };
    let logical_key = stable_digest(&(
        "agent-selection-logical-v1",
        &context.environment,
        scope,
        path_spec,
    ))?;
    candidates.push(SelectionCandidate {
        scope,
        agent_id: agent_id.clone(),
        logical_key,
        destination: resolved_scope
            .private_path
            .as_ref()
            .map(|native_path| ResourceLocator {
                environment: context.environment.clone(),
                native_path: native_path.clone(),
            }),
    });
    Ok(())
}

fn into_groups(groups: BTreeMap<String, Vec<AgentId>>) -> Vec<AgentSelectionGroup> {
    groups
        .into_iter()
        .map(|(group_id, agent_ids)| AgentSelectionGroup {
            group_id,
            agent_ids,
        })
        .collect()
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

    use super::resolve_agent_selection_groups;

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
    async fn physical_target_identity_groups_required_and_optional_agents_together() {
        let context = ContextRef {
            environment: EnvironmentRef::Host,
            scope: ContextScope::Global,
        };
        let groups = resolve_agent_selection_groups(
            &context,
            &runtime(vec![
                agent("claude-code", false, "/logical/claude/skills"),
                agent("cursor", true, "/logical/cursor/skills"),
            ]),
            &SharedTargetResolver,
        )
        .await
        .unwrap();

        assert_eq!(groups.global.len(), 1);
        assert_eq!(
            groups.global[0].agent_ids,
            vec![
                AgentId::parse("claude-code").unwrap(),
                AgentId::parse("cursor").unwrap(),
            ]
        );
        assert!(!groups.global[0].group_id.contains("/logical/"));
    }

    #[tokio::test]
    async fn distinct_physical_target_identities_remain_separate() {
        let context = ContextRef {
            environment: EnvironmentRef::Host,
            scope: ContextScope::Global,
        };
        let groups = resolve_agent_selection_groups(
            &context,
            &runtime(vec![
                agent("claude-code", false, "/logical/claude/skills"),
                agent("cursor", false, "/logical/cursor/skills"),
            ]),
            &DistinctTargetResolver,
        )
        .await
        .unwrap();

        assert_eq!(groups.global.len(), 2);
        assert!(groups.global.iter().all(|group| group.agent_ids.len() == 1));
    }

    #[tokio::test]
    async fn project_templates_group_without_a_concrete_project_path() {
        let context = ContextRef {
            environment: EnvironmentRef::Host,
            scope: ContextScope::Global,
        };
        let groups = resolve_agent_selection_groups(
            &context,
            &runtime(vec![
                project_agent("claude-code", ".shared/skills"),
                project_agent("cursor", ".shared/skills"),
            ]),
            &SharedTargetResolver,
        )
        .await
        .unwrap();

        assert_eq!(groups.project.len(), 1);
        assert_eq!(groups.project[0].agent_ids.len(), 2);
    }
}

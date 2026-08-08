use std::collections::BTreeMap;

use crate::application::agent_intent::{validate_agent_intents, AgentWriteIntent};
use crate::core::agent_definition::{AgentAdapter, AgentId};
use crate::core::skill::sanitize_name;
use crate::environment::agent_environment::{
    AgentRuntimeSnapshot, DetectionState, ResolvedAgent, ResolvedAgentScope,
};
use crate::environment::types::{
    same_environment_identity, EnvironmentRef, ResourceLocator, SkillLocation, SkillLocationRef,
};
use crate::error::AppError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogicalAgentEntryRoot {
    pub target_id: String,
    pub root: ResourceLocator,
    pub owner_agent_ids: Vec<AgentId>,
    pub content: AgentEntryContent,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentEntryContent {
    Canonical,
    EveDerived { subagent: Option<String> },
}

impl AgentEntryContent {
    pub fn uses_eve_payload(&self) -> bool {
        matches!(self, Self::EveDerived { .. })
    }

    pub fn eve_subagent(&self) -> Option<Option<&str>> {
        match self {
            Self::Canonical => None,
            Self::EveDerived { subagent } => Some(subagent.as_deref()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentEntryPlan {
    pub canonical_owner_agent_ids: Vec<AgentId>,
    pub required_agent_roots: Vec<LogicalAgentEntryRoot>,
}

#[cfg_attr(not(test), allow(dead_code))]
pub fn resolve_agent_entry_plan(
    context: &SkillLocationRef,
    runtime: &AgentRuntimeSnapshot,
    intents: &[AgentWriteIntent],
) -> Result<AgentEntryPlan, AppError> {
    validate_agent_intents(intents)?;
    if !same_environment_identity(&runtime.environment, &context.environment) {
        return Err(AppError::StaleEnvironment);
    }

    let mut canonical_owner_agent_ids = Vec::new();
    let mut required = BTreeMap::<String, LogicalAgentEntryRoot>::new();
    for intent in intents {
        let agent = runtime
            .agents
            .get(&intent.agent_id)
            .ok_or(AppError::StaleRegistry)?;
        let scope = resolved_scope(agent, &context.scope);
        if !scope.enabled {
            return Err(validation("Agent does not support the selected scope"));
        }

        match agent.definition.adapter {
            AgentAdapter::Standard => resolve_standard(
                context,
                agent,
                scope,
                intent,
                &mut canonical_owner_agent_ids,
                &mut required,
            )?,
            AgentAdapter::Eve => resolve_eve(context, runtime, intent, &mut required)?,
        }
    }

    canonical_owner_agent_ids.sort();
    canonical_owner_agent_ids.dedup();
    for entry in required.values_mut() {
        entry.owner_agent_ids.sort();
        entry.owner_agent_ids.dedup();
    }
    Ok(AgentEntryPlan {
        canonical_owner_agent_ids,
        required_agent_roots: required.into_values().collect(),
    })
}

fn resolve_standard(
    context: &SkillLocationRef,
    agent: &ResolvedAgent,
    scope: &ResolvedAgentScope,
    intent: &AgentWriteIntent,
    canonical_owner_agent_ids: &mut Vec<AgentId>,
    required: &mut BTreeMap<String, LogicalAgentEntryRoot>,
) -> Result<(), AppError> {
    if !intent.adapter_targets.is_empty() {
        return Err(validation("Standard Agent does not accept adapter targets"));
    }
    if scope.reads_standard {
        if scope.standard_path.is_none() {
            return Err(validation("standard Skill directory is unavailable"));
        }
        canonical_owner_agent_ids.push(intent.agent_id.clone());
    }

    let needs_private = match (
        scope.reads_standard,
        scope.private_path.as_ref(),
        intent.own_directory_selected,
    ) {
        (_, None, false) | (true, Some(_), false) => false,
        (_, Some(_), true) => true,
        _ => {
            return Err(validation(
                "own directory selection does not match Agent scope",
            ))
        }
    };
    if needs_private {
        let root = scope
            .private_path
            .as_ref()
            .expect("validated private scope has a path");
        insert_required_root(
            required,
            context,
            format!("agent:{}:private", agent.definition.id.as_str()),
            root,
            intent.agent_id.clone(),
            AgentEntryContent::Canonical,
        );
    }
    Ok(())
}

fn resolve_eve(
    context: &SkillLocationRef,
    runtime: &AgentRuntimeSnapshot,
    intent: &AgentWriteIntent,
    required: &mut BTreeMap<String, LogicalAgentEntryRoot>,
) -> Result<(), AppError> {
    if !matches!(context.scope, SkillLocation::Project { .. })
        || intent.own_directory_selected
        || intent.adapter_targets.is_empty()
    {
        return Err(validation(
            "Eve requires explicit project adapter targets without a private entry intent",
        ));
    }
    if runtime
        .agents
        .get(&intent.agent_id)
        .is_none_or(|agent| agent.detection != DetectionState::Detected)
    {
        return Err(validation("Eve is not detected in the selected Project"));
    }
    let project = runtime
        .project_path
        .as_deref()
        .ok_or_else(|| validation("Eve requires Project Context"))?;
    for target in &intent.adapter_targets {
        let (target_id, root, subagent) = eve_target(project, &target.0, &context.environment)?;
        insert_required_root(
            required,
            context,
            target_id,
            &root,
            intent.agent_id.clone(),
            AgentEntryContent::EveDerived { subagent },
        );
    }
    Ok(())
}

fn eve_target(
    project: &str,
    target_id: &str,
    environment: &EnvironmentRef,
) -> Result<(String, String, Option<String>), AppError> {
    if target_id == "eve:root" {
        return Ok((
            target_id.to_string(),
            join_target_path(environment, project, "agent/skills"),
            None,
        ));
    }
    let subagent = target_id
        .strip_prefix("eve:")
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| validation("invalid Eve adapter target"))?;
    let normalized = sanitize_name(subagent);
    if normalized.is_empty() {
        return Err(validation("invalid Eve adapter target"));
    }
    Ok((
        format!("eve:{normalized}"),
        join_target_path(
            environment,
            project,
            &format!("agent/subagents/{normalized}/skills"),
        ),
        Some(normalized),
    ))
}

fn insert_required_root(
    required: &mut BTreeMap<String, LogicalAgentEntryRoot>,
    context: &SkillLocationRef,
    target_id: String,
    root: &str,
    owner: AgentId,
    content: AgentEntryContent,
) {
    let key = logical_root_key(&context.environment, root);
    required
        .entry(key)
        .and_modify(|entry| entry.owner_agent_ids.push(owner.clone()))
        .or_insert_with(|| LogicalAgentEntryRoot {
            target_id,
            root: ResourceLocator {
                environment: context.environment.clone(),
                native_path: root.to_string(),
            },
            owner_agent_ids: vec![owner],
            content,
        });
}

fn resolved_scope<'a>(agent: &'a ResolvedAgent, scope: &SkillLocation) -> &'a ResolvedAgentScope {
    match scope {
        SkillLocation::Global => &agent.global,
        SkillLocation::Project { .. } => &agent.project,
    }
}

fn logical_root_key(environment: &EnvironmentRef, root: &str) -> String {
    let trimmed = root.trim_end_matches(['/', '\\']);
    match environment {
        EnvironmentRef::Native if cfg!(windows) => trimmed.replace('/', "\\").to_lowercase(),
        EnvironmentRef::Native => trimmed.to_string(),
        EnvironmentRef::Wsl { distro_name } => {
            format!("{}:{trimmed}", distro_name.to_lowercase())
        }
    }
}

fn join_target_path(environment: &EnvironmentRef, root: &str, relative: &str) -> String {
    match environment {
        EnvironmentRef::Native if cfg!(windows) => format!(
            "{}\\{}",
            root.trim_end_matches(['/', '\\']),
            relative.replace('/', "\\")
        ),
        _ => format!(
            "{}/{}",
            root.trim_end_matches(['/', '\\']),
            relative.trim_start_matches('/')
        ),
    }
}

fn validation(message: &str) -> AppError {
    AppError::Validation {
        field: Some("agentIntents".to_string()),
        message: message.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::application::agent_intent::{AdapterTargetId, AgentWriteIntent};
    use crate::core::agent_definition::{
        AgentAdapter, AgentDefinition, AgentId, AgentSource, DetectionSpec, PathSpec,
        ScopeDefinition,
    };
    use crate::environment::agent_environment::{
        AgentRuntimeSnapshot, DetectionState, ResolvedAgent, ResolvedAgentScope,
    };
    use crate::environment::types::{
        EnvironmentRef, EnvironmentStatus, SkillLocation, SkillLocationRef,
    };

    fn agent(
        id: &str,
        source: AgentSource,
        adapter: AgentAdapter,
        reads_standard: bool,
        private_path: Option<&str>,
    ) -> (AgentId, ResolvedAgent) {
        let id = AgentId::parse(id).unwrap();
        let scope = ResolvedAgentScope {
            enabled: true,
            reads_standard,
            standard_path: reads_standard.then(|| "/home/alice/.agents/skills".to_string()),
            private_path: private_path.map(str::to_string),
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
                    display_name: "Agent".to_string(),
                    source,
                    aliases: Vec::new(),
                    global: ScopeDefinition {
                        enabled: true,
                        reads_standard,
                        private_path: private_path.map(|_| PathSpec::home(".agent/skills")),
                    },
                    project: ScopeDefinition {
                        enabled: true,
                        reads_standard,
                        private_path: private_path.map(|_| PathSpec::project(".agent/skills")),
                    },
                    detection: DetectionSpec::AnyPathExists {
                        paths: vec![PathSpec::home(".agent")],
                    },
                    legacy_paths: Vec::new(),
                    adapter,
                },
                detection: DetectionState::Detected,
                detection_reason: None,
                global: scope.clone(),
                project: scope,
            },
        )
    }

    fn runtime(agents: Vec<(AgentId, ResolvedAgent)>) -> AgentRuntimeSnapshot {
        AgentRuntimeSnapshot {
            registry_revision: "registry-1".to_string(),
            environment_revision: "environment-1".to_string(),
            environment: EnvironmentRef::Native,
            availability: EnvironmentStatus::Available,
            project_path: Some("/work/app".to_string()),
            agents: agents.into_iter().collect::<BTreeMap<_, _>>(),
        }
    }

    fn context(scope: SkillLocation) -> SkillLocationRef {
        SkillLocationRef {
            environment: EnvironmentRef::Native,
            scope,
        }
    }

    fn intent(id: &str, own_directory_selected: bool) -> AgentWriteIntent {
        AgentWriteIntent {
            agent_id: AgentId::parse(id).unwrap(),
            own_directory_selected,
            adapter_targets: Vec::new(),
        }
    }

    #[test]
    fn standard_shared_requires_only_the_canonical_entry() {
        let runtime = runtime(vec![agent(
            "shared",
            AgentSource::Builtin,
            AgentAdapter::Standard,
            true,
            None,
        )]);

        let plan = resolve_agent_entry_plan(
            &context(SkillLocation::Global),
            &runtime,
            &[intent("shared", false)],
        )
        .unwrap();

        assert_eq!(
            plan.canonical_owner_agent_ids,
            vec![AgentId::parse("shared").unwrap()]
        );
        assert!(plan.required_agent_roots.is_empty());
    }

    #[test]
    fn standard_private_requires_canonical_and_private_entries() {
        let runtime = runtime(vec![agent(
            "private",
            AgentSource::Custom,
            AgentAdapter::Standard,
            false,
            Some("/home/alice/.private/skills"),
        )]);

        let plan = resolve_agent_entry_plan(
            &context(SkillLocation::Global),
            &runtime,
            &[intent("private", true)],
        )
        .unwrap();

        assert!(plan.canonical_owner_agent_ids.is_empty());
        assert_eq!(plan.required_agent_roots.len(), 1);
        assert_eq!(
            plan.required_agent_roots[0].root.native_path,
            "/home/alice/.private/skills"
        );
    }

    #[test]
    fn standard_both_uses_canonical_until_private_is_selected() {
        let runtime = runtime(vec![agent(
            "both",
            AgentSource::Custom,
            AgentAdapter::Standard,
            true,
            Some("/home/alice/.both/skills"),
        )]);

        let canonical_only = resolve_agent_entry_plan(
            &context(SkillLocation::Global),
            &runtime,
            &[intent("both", false)],
        )
        .unwrap();
        let with_private = resolve_agent_entry_plan(
            &context(SkillLocation::Global),
            &runtime,
            &[intent("both", true)],
        )
        .unwrap();

        assert!(canonical_only.required_agent_roots.is_empty());
        assert_eq!(with_private.required_agent_roots.len(), 1);
        assert_eq!(
            with_private.required_agent_roots[0].target_id,
            "agent:both:private"
        );
    }

    #[test]
    fn builtin_and_custom_standard_definitions_produce_the_same_plan() {
        let builtin = runtime(vec![agent(
            "standard",
            AgentSource::Builtin,
            AgentAdapter::Standard,
            true,
            Some("/home/alice/.standard/skills"),
        )]);
        let custom = runtime(vec![agent(
            "standard",
            AgentSource::Custom,
            AgentAdapter::Standard,
            true,
            Some("/home/alice/.standard/skills"),
        )]);
        let intents = [intent("standard", true)];

        assert_eq!(
            resolve_agent_entry_plan(&context(SkillLocation::Global), &builtin, &intents).unwrap(),
            resolve_agent_entry_plan(&context(SkillLocation::Global), &custom, &intents).unwrap(),
        );
    }

    #[test]
    fn eve_uses_only_explicit_adapter_targets() {
        let runtime = runtime(vec![agent(
            "eve",
            AgentSource::Builtin,
            AgentAdapter::Eve,
            false,
            Some("/work/app/agent/skills"),
        )]);
        let intent = AgentWriteIntent {
            agent_id: AgentId::parse("eve").unwrap(),
            own_directory_selected: false,
            adapter_targets: vec![
                AdapterTargetId("eve:root".to_string()),
                AdapterTargetId("eve:Research Team".to_string()),
            ],
        };

        let plan = resolve_agent_entry_plan(
            &context(SkillLocation::Project {
                project_id: "project-1".to_string(),
            }),
            &runtime,
            &[intent],
        )
        .unwrap();

        assert!(plan.canonical_owner_agent_ids.is_empty());
        assert_eq!(plan.required_agent_roots.len(), 2);
        assert_eq!(
            plan.required_agent_roots[0].content,
            AgentEntryContent::EveDerived { subagent: None }
        );
        assert_eq!(
            plan.required_agent_roots[1].content,
            AgentEntryContent::EveDerived {
                subagent: Some("research-team".to_string())
            }
        );
        assert_eq!(
            plan.required_agent_roots[0].root.native_path,
            std::path::Path::new("/work/app")
                .join("agent")
                .join("skills")
                .to_string_lossy()
        );
        assert_eq!(
            plan.required_agent_roots[1].root.native_path,
            std::path::Path::new("/work/app")
                .join("agent")
                .join("subagents")
                .join("research-team")
                .join("skills")
                .to_string_lossy()
        );
    }

    #[test]
    fn eve_requires_a_detected_project() {
        for detection in [DetectionState::NotDetected, DetectionState::Indeterminate] {
            let mut runtime = runtime(vec![agent(
                "eve",
                AgentSource::Builtin,
                AgentAdapter::Eve,
                false,
                Some("/work/app/agent/skills"),
            )]);
            runtime
                .agents
                .get_mut(&AgentId::parse("eve").unwrap())
                .unwrap()
                .detection = detection;
            let intent = AgentWriteIntent {
                agent_id: AgentId::parse("eve").unwrap(),
                own_directory_selected: false,
                adapter_targets: vec![AdapterTargetId("eve:root".to_string())],
            };

            assert!(matches!(
                resolve_agent_entry_plan(
                    &context(SkillLocation::Project {
                        project_id: "project-1".to_string(),
                    }),
                    &runtime,
                    &[intent],
                ),
                Err(AppError::Validation { .. })
            ));
        }
    }
}

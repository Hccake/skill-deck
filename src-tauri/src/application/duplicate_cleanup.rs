use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;
use specta::Type;

use crate::application::mutation::result::{
    MutationUnitResult, MutationUnitStatus, OperationErrorCode,
};
use crate::application::remove::{
    ObservedEntryKind, ObservedPhysicalEntry, RemoveIntent, RemoveRequest,
};
use crate::application::remove_runtime::RuntimeRemoveService;
use crate::application::runtime_admission::RuntimeAdmissionCoordinator;
use crate::core::agent_definition::AgentId;
use crate::core::mutation::{MutationKind, MutationPhase};
use crate::environment::types::{ContextRef, ResourceLocator};
use crate::error::AppError;

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct DuplicateCleanupResult {
    pub agent: AgentId,
    pub success: bool,
    pub skipped: bool,
    pub path: Option<ResourceLocator>,
    pub error: Option<OperationErrorCode>,
}

#[derive(Default)]
pub struct DuplicateCleanupService;

impl DuplicateCleanupService {
    pub async fn execute(
        &self,
        context: ContextRef,
        skill_name: String,
        agents: Vec<AgentId>,
        remove: &RuntimeRemoveService,
        controller: &RuntimeAdmissionCoordinator,
    ) -> Result<Vec<DuplicateCleanupResult>, AppError> {
        validate_agents(&agents)?;
        let preview = remove.preview(&context, &skill_name).await?;
        let selected = select_duplicate_entries(&agents, &preview.physical_entries);
        let selected_ids = selected
            .values()
            .map(|entry| entry.entry_id.clone())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        if selected_ids.is_empty() {
            return Ok(cleanup_results(&agents, &selected, None));
        }
        let guard = controller.begin_mutation(MutationKind::DuplicateCleanup, context.clone())?;
        guard.transition(MutationPhase::Preparing, None, false);
        let response = remove
            .execute(
                &RemoveRequest {
                    token: preview.token,
                    context,
                    skill_name,
                    intent: RemoveIntent::AgentEntries(selected_ids),
                },
                guard.cancellation(),
            )
            .await?;
        Ok(cleanup_results(&agents, &selected, response.units.first()))
    }
}

fn validate_agents(agents: &[AgentId]) -> Result<(), AppError> {
    let mut unique = BTreeSet::new();
    if agents.is_empty() || agents.iter().any(|agent| !unique.insert(agent)) {
        return Err(AppError::Validation {
            field: Some("agents".to_string()),
            message: "Agent selection must be non-empty and unique".to_string(),
        });
    }
    Ok(())
}

fn select_duplicate_entries<'a>(
    agents: &[AgentId],
    entries: &'a [ObservedPhysicalEntry],
) -> BTreeMap<AgentId, &'a ObservedPhysicalEntry> {
    agents
        .iter()
        .filter_map(|agent| {
            entries
                .iter()
                .find(|entry| {
                    entry.kind == ObservedEntryKind::Directory
                        && entry.owners.iter().any(|owner| {
                            &owner.agent_id == agent
                                && owner.logical_target_id.starts_with("agent:")
                                && owner.logical_target_id.ends_with(":private")
                        })
                })
                .map(|entry| (agent.clone(), entry))
        })
        .collect()
}

fn cleanup_results(
    agents: &[AgentId],
    selected: &BTreeMap<AgentId, &ObservedPhysicalEntry>,
    unit: Option<&MutationUnitResult>,
) -> Vec<DuplicateCleanupResult> {
    agents
        .iter()
        .map(|agent| {
            let entry = selected.get(agent);
            let skipped = entry.is_none();
            let success = !skipped
                && unit.is_some_and(|result| result.status == MutationUnitStatus::Succeeded);
            DuplicateCleanupResult {
                agent: agent.clone(),
                success,
                skipped,
                path: entry.map(|entry| entry.display_path.clone()),
                error: (!skipped && !success)
                    .then(|| {
                        unit.and_then(|result| result.error.as_ref())
                            .map(|error| error.code)
                    })
                    .flatten(),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::remove::ObservedEntryOwner;
    use crate::environment::runtime::ObservedEntryId;
    use crate::environment::types::EnvironmentRef;

    fn entry(kind: ObservedEntryKind, owners: &[(&str, &str)]) -> ObservedPhysicalEntry {
        ObservedPhysicalEntry {
            entry_id: ObservedEntryId::parse("entry-v1-private").unwrap(),
            display_path: ResourceLocator {
                environment: EnvironmentRef::Host,
                native_path: "/work/.claude/skills/demo".to_string(),
            },
            kind,
            physical_target_key: "target-v1-private".to_string(),
            owners: owners
                .iter()
                .map(|(agent, target)| ObservedEntryOwner {
                    agent_id: AgentId::parse(*agent).unwrap(),
                    display_name: (*agent).to_string(),
                    logical_target_id: (*target).to_string(),
                })
                .collect(),
            will_break_if_canonical_removed: false,
        }
    }

    #[test]
    fn selects_only_private_entity_directories_for_open_agent_ids() {
        let custom = AgentId::parse("custom-agent").unwrap();
        let entries = vec![
            entry(
                ObservedEntryKind::Directory,
                &[("custom-agent", "agent:custom-agent:private")],
            ),
            entry(
                ObservedEntryKind::Symlink,
                &[("custom-agent", "agent:custom-agent:private")],
            ),
            entry(
                ObservedEntryKind::Directory,
                &[("custom-agent", "eve:researcher")],
            ),
        ];

        let selected = select_duplicate_entries(std::slice::from_ref(&custom), &entries);

        assert_eq!(selected.len(), 1);
        assert_eq!(selected[&custom].kind, ObservedEntryKind::Directory);
    }
}

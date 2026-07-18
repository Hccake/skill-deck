use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::application::agent_intent::{validate_agent_intents, AgentWriteIntent};
use crate::application::agent_selection::{resolve_agent_selection_groups, AgentSelectionGroups};
use crate::application::install::InstallPlanExecutor;
use crate::application::install_planner::InstallPlanningFactSource;
use crate::application::mutation::plan::{
    group_physical_mutations, preview_token, stable_digest, ExecutionUnit, ExpectedTargetEntry,
    MutationPlan, PreparedEntryAction, PreparedEntryMutation, PreviewFingerprint, PreviewToken,
};
use crate::application::mutation::result::MutationUnitResult;
use crate::application::payload_session::{
    AcquiredPayloadHandle, PayloadSessionManager, PinnedPayloadLease,
};
use crate::application::remove::ObservedPhysicalEntry;
use crate::application::skill_entries::{
    join_entry, InstalledSkillPayloadAcquirer, ObservedSkillSnapshot, SkillEntryObserver,
};
use crate::application::workflow_planner::{resolve_agent_entry_plan, AgentEntryPlan};
use crate::core::mutation::{CancellationSignal, MutationKind};
use crate::core::skill_payload::PayloadId;
use crate::environment::agent_environment::ResolvedAgent;
use crate::environment::planning::{ResolvedTargetFact, TargetFactResolver};
use crate::environment::runtime::ObservedEntryId;
use crate::environment::types::{same_environment_identity, ContextRef};
use crate::error::AppError;
use crate::models::InstallMode;
use crate::storage::lock_plan::{LockExpectedState, PreparedLockMutation};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct ManageAgentsPreviewRequest {
    pub context: ContextRef,
    pub skill_name: String,
    pub add: Vec<AgentWriteIntent>,
    pub remove_entry_ids: Vec<ObservedEntryId>,
    pub requested_mode: InstallMode,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct ManageAgentsRequest {
    pub token: PreviewToken,
    pub context: ContextRef,
    pub skill_name: String,
    pub add: Vec<AgentWriteIntent>,
    pub remove_entry_ids: Vec<ObservedEntryId>,
    pub requested_mode: InstallMode,
    pub confirm_entity_directories: bool,
    pub canonical_payload: Option<AcquiredPayloadHandle>,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct ManageAgentsPreview {
    pub token: PreviewToken,
    pub context: ContextRef,
    pub skill_name: String,
    pub available_agents: Vec<ResolvedAgent>,
    pub selection_groups: AgentSelectionGroups,
    pub observed_entries: Vec<ObservedPhysicalEntry>,
    pub canonical_payload: Option<AcquiredPayloadHandle>,
    pub add_targets: Vec<crate::environment::types::ResourceLocator>,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct ManageAgentsResponse {
    pub units: Vec<MutationUnitResult>,
}

pub struct ManageAgentsService<F, T, E> {
    observer: SkillEntryObserver<F, T>,
    targets: T,
    payloads: Arc<PayloadSessionManager>,
    acquirer: InstalledSkillPayloadAcquirer,
    executor: E,
}

impl<F, T, E> ManageAgentsService<F, T, E>
where
    F: InstallPlanningFactSource,
    T: TargetFactResolver + Clone,
    E: InstallPlanExecutor,
{
    pub fn new(
        observer: SkillEntryObserver<F, T>,
        targets: T,
        payloads: Arc<PayloadSessionManager>,
        acquirer: InstalledSkillPayloadAcquirer,
        executor: E,
    ) -> Self {
        Self {
            observer,
            targets,
            payloads,
            acquirer,
            executor,
        }
    }

    pub async fn preview(
        &self,
        request: &ManageAgentsPreviewRequest,
    ) -> Result<ManageAgentsPreview, AppError> {
        validate_manage_selection(&request.add, &request.remove_entry_ids)?;
        let snapshot = self
            .observer
            .observe(&request.context, &request.skill_name)
            .await?;
        let selection_groups = resolve_agent_selection_groups(
            &request.context,
            &snapshot.facts.agent_runtime,
            &self.targets,
        )
        .await?;
        let additions = self.resolve_additions(request, &snapshot).await?;
        let canonical_payload = if request.add.is_empty() {
            None
        } else {
            Some(
                self.acquirer
                    .acquire(&request.context, &request.skill_name, &snapshot.canonical)
                    .await?,
            )
        };
        manage_preview(
            request,
            &snapshot,
            &additions,
            selection_groups,
            canonical_payload,
        )
    }

    pub async fn execute(
        &self,
        request: &ManageAgentsRequest,
        cancellation: CancellationSignal,
    ) -> Result<ManageAgentsResponse, AppError> {
        let preview_request = ManageAgentsPreviewRequest {
            context: request.context.clone(),
            skill_name: request.skill_name.clone(),
            add: request.add.clone(),
            remove_entry_ids: request.remove_entry_ids.clone(),
            requested_mode: request.requested_mode.clone(),
        };
        let snapshot = self
            .observer
            .observe(&request.context, &request.skill_name)
            .await?;
        validate_manage_execution(
            &request.add,
            &request.remove_entry_ids,
            request.confirm_entity_directories,
            &snapshot
                .entries
                .iter()
                .map(|entry| entry.public.clone())
                .collect::<Vec<_>>(),
        )?;
        let additions = self.resolve_additions(&preview_request, &snapshot).await?;
        let canonical_lease = match (&request.canonical_payload, request.add.is_empty()) {
            (None, true) => None,
            (Some(handle), false)
                if same_environment_identity(&handle.environment, &request.context.environment) =>
            {
                Some(self.payloads.pin_verified(handle).await?)
            }
            _ => return Err(AppError::StalePayload),
        };
        let actual_preview = manage_preview(
            &preview_request,
            &snapshot,
            &additions,
            AgentSelectionGroups::default(),
            request.canonical_payload.clone(),
        )?;
        validate_token(&request.token, &actual_preview.token)?;
        let plan = build_manage_plan(
            request,
            snapshot,
            additions,
            canonical_lease,
            &self.payloads,
        )
        .await?;
        Ok(ManageAgentsResponse {
            units: self.executor.execute(plan, cancellation).await,
        })
    }

    async fn resolve_additions(
        &self,
        request: &ManageAgentsPreviewRequest,
        snapshot: &ObservedSkillSnapshot,
    ) -> Result<ResolvedAdditions, AppError> {
        let plan = resolve_agent_entry_plan(
            &request.context,
            &snapshot.facts.agent_runtime,
            &request.add,
        )?;
        let destinations = plan
            .required_agent_roots
            .iter()
            .map(|target| join_entry(&target.root, &request.skill_name))
            .collect::<Vec<_>>();
        let facts = if destinations.is_empty() {
            Vec::new()
        } else {
            self.targets
                .resolve(&request.context, &destinations, None)
                .await?
        };
        if facts.len() != destinations.len() {
            return Err(AppError::StaleTarget);
        }
        Ok(ResolvedAdditions { plan, facts })
    }
}

struct ResolvedAdditions {
    plan: AgentEntryPlan,
    facts: Vec<ResolvedTargetFact>,
}

fn manage_preview(
    request: &ManageAgentsPreviewRequest,
    snapshot: &ObservedSkillSnapshot,
    additions: &ResolvedAdditions,
    selection_groups: AgentSelectionGroups,
    canonical_payload: Option<AcquiredPayloadHandle>,
) -> Result<ManageAgentsPreview, AppError> {
    let observed_state_digest = stable_digest(&(
        &snapshot.canonical.key,
        &snapshot.canonical.fingerprint,
        snapshot
            .entries
            .iter()
            .map(|entry| (&entry.public.entry_id, &entry.fact.fingerprint))
            .collect::<Vec<_>>(),
        additions
            .facts
            .iter()
            .map(|fact| (&fact.key, &fact.fingerprint))
            .collect::<Vec<_>>(),
        canonical_payload
            .as_ref()
            .map(|handle| (&handle.payload_id, &handle.manifest_hash)),
        snapshot
            .facts
            .lock_document
            .entry_snapshot(&request.skill_name)
            .value()
            .cloned(),
    ))?;
    let token = preview_token(&PreviewFingerprint {
        kind: MutationKind::ManageAgents,
        request_digest: stable_digest(request)?,
        revisions: snapshot.facts.revisions.clone(),
        observed_state_digest,
        planner_contract_version: 1,
    })?;
    Ok(ManageAgentsPreview {
        token,
        context: request.context.clone(),
        skill_name: request.skill_name.clone(),
        available_agents: snapshot
            .facts
            .agent_runtime
            .agents
            .values()
            .cloned()
            .collect(),
        selection_groups,
        observed_entries: snapshot
            .entries
            .iter()
            .map(|entry| entry.public.clone())
            .collect(),
        canonical_payload,
        add_targets: additions
            .facts
            .iter()
            .map(|fact| fact.destination.clone())
            .collect(),
    })
}

async fn build_manage_plan(
    request: &ManageAgentsRequest,
    snapshot: ObservedSkillSnapshot,
    additions: ResolvedAdditions,
    canonical_lease: Option<PinnedPayloadLease>,
    payload_manager: &PayloadSessionManager,
) -> Result<MutationPlan, AppError> {
    let selected = request.remove_entry_ids.iter().collect::<BTreeSet<_>>();
    let mut mutations = snapshot
        .entries
        .iter()
        .filter(|entry| selected.contains(&entry.public.entry_id))
        .map(|entry| PreparedEntryMutation {
            key: entry.fact.key.clone(),
            destination: entry.fact.destination.clone(),
            action: PreparedEntryAction::Remove,
            owner_agent_ids: entry
                .public
                .owners
                .iter()
                .map(|owner| owner.agent_id.clone())
                .collect(),
        })
        .collect::<Vec<_>>();
    let mut payloads = Vec::new();
    let mut canonical_payload_id = None;
    let mut eve_payload_id = None;
    if let Some(canonical) = canonical_lease {
        canonical_payload_id = Some(canonical.manifest().payload_id().clone());
        if additions
            .plan
            .required_agent_roots
            .iter()
            .any(|target| target.target_id.starts_with("eve:"))
        {
            let derived =
                crate::core::eve::derive_eve_skill_payload(&canonical.load_payload().await?)?;
            let eve = payload_manager
                .pin_derived_payload(&canonical, "eve-manage", derived)
                .await?;
            eve_payload_id = Some(eve.manifest().payload_id().clone());
            payloads.push(eve);
        }
        payloads.push(canonical);
    }
    for (target, fact) in additions
        .plan
        .required_agent_roots
        .iter()
        .zip(&additions.facts)
    {
        let eve = target.target_id.starts_with("eve:");
        mutations.push(PreparedEntryMutation {
            key: fact.key.clone(),
            destination: fact.destination.clone(),
            action: PreparedEntryAction::Replace {
                payload_id: if eve {
                    eve_payload_id.clone().ok_or(AppError::StalePayload)?
                } else {
                    canonical_payload_id.clone().ok_or(AppError::StalePayload)?
                },
                requested_mode: if eve {
                    InstallMode::Copy
                } else {
                    request.requested_mode.clone()
                },
            },
            owner_agent_ids: target.owner_agent_ids.clone(),
        });
    }
    let required_agent_entries = group_physical_mutations(mutations)?;
    if required_agent_entries.is_empty() {
        return Err(AppError::Validation {
            field: Some("selection".to_string()),
            message: "selection does not change a physical Agent entry".to_string(),
        });
    }
    let canonical_entry =
        (!additions.plan.required_agent_roots.is_empty()).then(|| PreparedEntryMutation {
            key: snapshot.canonical.key.clone(),
            destination: snapshot.canonical.destination.clone(),
            action: PreparedEntryAction::Keep,
            owner_agent_ids: additions.plan.canonical_owner_agent_ids.clone(),
        });
    let lock_mutation = manage_lock_mutation(request, &snapshot, &additions)?;
    let expected_targets = std::iter::once(&snapshot.canonical)
        .chain(snapshot.entries.iter().map(|entry| &entry.fact))
        .chain(additions.facts.iter())
        .map(|fact| ExpectedTargetEntry {
            key: fact.key.clone(),
            fingerprint: fact.fingerprint.clone(),
            expected_content_manifest_hash: None,
        })
        .collect();
    Ok(MutationPlan {
        operation_id: Uuid::new_v4().simple().to_string(),
        payloads: payloads
            .into_iter()
            .map(|lease| (lease.manifest().payload_id().clone(), lease))
            .collect::<BTreeMap<PayloadId, PinnedPayloadLease>>(),
        units: vec![ExecutionUnit {
            id: format!("manage-agents:{}", request.skill_name),
            skill_name: request.skill_name.clone(),
            source: None,
            target: request.context.clone(),
            expected_revisions: snapshot.facts.revisions,
            canonical_entry,
            required_agent_entries,
            lock_mutation,
            expected_targets,
        }],
    })
}

fn manage_lock_mutation(
    request: &ManageAgentsRequest,
    snapshot: &ObservedSkillSnapshot,
    additions: &ResolvedAdditions,
) -> Result<Option<PreparedLockMutation>, AppError> {
    let selected = request.remove_entry_ids.iter().collect::<BTreeSet<_>>();
    let removes_eve = snapshot.entries.iter().any(|entry| {
        selected.contains(&entry.public.entry_id)
            && entry
                .public
                .owners
                .iter()
                .any(|owner| owner.logical_target_id.starts_with("eve:"))
    });
    let adds_eve = additions
        .plan
        .required_agent_roots
        .iter()
        .any(|target| target.target_id.starts_with("eve:"));
    if !removes_eve && !adds_eve {
        return Ok(None);
    }
    if !matches!(
        request.context.scope,
        crate::environment::types::ContextScope::Project { .. }
    ) {
        return Err(AppError::Validation {
            field: Some("eveTargets".to_string()),
            message: "Eve targets require Project Context".to_string(),
        });
    }
    let raw = snapshot
        .facts
        .lock_document
        .entry_snapshot(&request.skill_name)
        .value()
        .cloned()
        .ok_or_else(|| AppError::InvalidSource {
            value: format!(
                "Skill '{}' is missing from the project lock",
                request.skill_name
            ),
        })?;
    let mut target_ids = snapshot
        .entries
        .iter()
        .filter(|entry| !selected.contains(&entry.public.entry_id))
        .flat_map(|entry| entry.public.owners.iter())
        .filter(|owner| owner.logical_target_id.starts_with("eve:"))
        .map(|owner| owner.logical_target_id.clone())
        .collect::<BTreeSet<_>>();
    target_ids.extend(
        additions
            .plan
            .required_agent_roots
            .iter()
            .filter(|target| target.target_id.starts_with("eve:"))
            .map(|target| target.target_id.clone()),
    );
    let subagents = target_ids
        .iter()
        .filter_map(|target| target.strip_prefix("eve:"))
        .filter(|target| *target != "root")
        .map(|target| serde_json::Value::String(target.to_string()))
        .collect::<Vec<_>>();
    let mut replacement = raw;
    replacement
        .as_object_mut()
        .ok_or_else(|| AppError::ConfigurationCorrupted {
            message: "project lock entry must be an object".to_string(),
        })?
        .insert("subagents".to_string(), serde_json::Value::Array(subagents));
    Ok(Some(PreparedLockMutation {
        target: snapshot.facts.resolved_context.lock.clone(),
        legacy_target: None,
        schema: snapshot.facts.lock_schema,
        skill_name: request.skill_name.clone(),
        replacement: Some(replacement),
        root_replacements: BTreeMap::new(),
        expected: LockExpectedState::capture(
            &snapshot.facts.lock_document,
            [&request.skill_name],
            std::iter::empty::<&str>(),
        ),
    }))
}

fn validate_token(expected: &PreviewToken, actual: &PreviewToken) -> Result<(), AppError> {
    if expected.registry_revision != actual.registry_revision {
        return Err(AppError::StaleRegistry);
    }
    if expected.environment_revision != actual.environment_revision {
        return Err(AppError::StaleEnvironment);
    }
    if expected.context_revision != actual.context_revision
        || expected.generation != actual.generation
    {
        return Err(AppError::StaleContext);
    }
    Ok(())
}

pub fn validate_manage_selection(
    add: &[AgentWriteIntent],
    remove_entry_ids: &[ObservedEntryId],
) -> Result<(), AppError> {
    validate_agent_intents(add)?;
    let mut ids = BTreeSet::new();
    if remove_entry_ids.iter().any(|id| !ids.insert(id)) {
        return Err(AppError::Validation {
            field: Some("removeEntryIds".to_string()),
            message: "duplicate observed entry selection".to_string(),
        });
    }
    Ok(())
}

pub fn validate_manage_execution(
    add: &[AgentWriteIntent],
    remove_entry_ids: &[ObservedEntryId],
    confirm_entity_directories: bool,
    observed_entries: &[ObservedPhysicalEntry],
) -> Result<(), AppError> {
    validate_manage_selection(add, remove_entry_ids)?;
    if add.is_empty() && remove_entry_ids.is_empty() {
        return Err(AppError::Validation {
            field: Some("selection".to_string()),
            message: "nothing is selected".to_string(),
        });
    }
    let available = observed_entries
        .iter()
        .map(|entry| (&entry.entry_id, entry))
        .collect::<std::collections::BTreeMap<_, _>>();
    let mut removes_directory = false;
    for id in remove_entry_ids {
        let entry = available.get(id).ok_or(AppError::StaleTarget)?;
        removes_directory |= entry.kind == crate::application::remove::ObservedEntryKind::Directory;
    }
    if removes_directory && !confirm_entity_directories {
        return Err(AppError::Validation {
            field: Some("confirmEntityDirectories".to_string()),
            message: "selected entity directories require confirmation".to_string(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::agent_intent::{AgentWriteIntent, PrivateEntryIntent};
    use crate::core::agent_definition::AgentId;
    use crate::environment::runtime::ObservedEntryId;

    #[test]
    fn manage_preview_selection_accepts_an_empty_baseline() {
        assert!(validate_manage_selection(&[], &[]).is_ok());
    }

    #[test]
    fn manage_execution_rejects_an_empty_selection() {
        assert!(validate_manage_execution(&[], &[], false, &[]).is_err());
    }

    #[test]
    fn manage_request_rejects_duplicate_additions_and_removals() {
        let intent = AgentWriteIntent {
            agent_id: AgentId::parse("custom-agent").unwrap(),
            private_entry: PrivateEntryIntent::Required,
            adapter_targets: Vec::new(),
        };
        let id = ObservedEntryId::parse("entry-v1-demo").unwrap();
        assert!(
            validate_manage_selection(&[intent.clone(), intent], std::slice::from_ref(&id))
                .is_err()
        );
        assert!(validate_manage_selection(&[], &[id.clone(), id]).is_err());
    }

    #[test]
    fn removing_an_entity_directory_requires_confirmation() {
        let id = ObservedEntryId::parse("entry-v1-copy").unwrap();
        let observed = vec![crate::application::remove::ObservedPhysicalEntry {
            entry_id: id.clone(),
            display_path: crate::environment::types::ResourceLocator {
                environment: crate::environment::types::EnvironmentRef::Host,
                native_path: "/agent/skills/demo".to_string(),
            },
            kind: crate::application::remove::ObservedEntryKind::Directory,
            physical_target_key: "target-v1-copy".to_string(),
            owners: Vec::new(),
            will_break_if_canonical_removed: false,
        }];

        assert!(validate_manage_execution(&[], &[id], false, &observed).is_err());
    }
}

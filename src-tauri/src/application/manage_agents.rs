use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::application::agent_intent::{validate_agent_intents, AgentWriteIntent};
use crate::application::agent_selection::{
    build_agent_selection_catalog, resolve_agent_selection_submission, AgentInstallOptionId,
    AgentSelectionDisabledReason, AgentSelectionModeConstraint, AgentSelectionResolution,
    AgentSelectionRevision, AgentSelectionSnapshot, AgentSelectionSubmission,
};
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
    join_entry, link_points_to, InstalledSkillPayloadAcquirer, ObservedSkillSnapshot,
    SkillEntryObserver,
};
use crate::application::workflow_planner::AgentEntryPlan;
use crate::core::mutation::{CancellationSignal, MutationKind};
use crate::core::skill_payload::PayloadId;
use crate::environment::planning::{ResolvedTargetFact, TargetEntryKind, TargetFactResolver};
use crate::environment::runtime::ObservedEntryId;
use crate::environment::types::{same_environment_identity, ContextRef};
use crate::error::{AgentSelectionInvalidReason, AppError};
use crate::models::InstallMode;
use crate::storage::lock_plan::{LockExpectedState, PreparedLockMutation};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct ManageAgentsPreviewRequest {
    pub context: ContextRef,
    pub skill_name: String,
    pub agent_selection: AgentSelectionSubmission,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct ManageAgentsRequest {
    pub token: PreviewToken,
    pub context: ContextRef,
    pub skill_name: String,
    pub agent_selection: AgentSelectionSubmission,
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
    pub canonical_payload: Option<AcquiredPayloadHandle>,
    pub confirmation: Option<ManageAgentsConfirmation>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct ManageAgentsConfirmation {
    pub removes_entity_directories: bool,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(tag = "status", rename_all = "camelCase")]
#[specta(tag = "status", rename_all = "camelCase")]
#[allow(
    clippy::large_enum_variant,
    reason = "IPC 结果直接携带最新选择快照，调用侧无需额外读取"
)]
pub enum ManageAgentsPreviewOutcome {
    Ready {
        preview: ManageAgentsPreview,
    },
    SelectionStale {
        snapshot: ManageAgentSelectionSnapshot,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub enum ManageCurrentEntry {
    None,
    Link,
    Copy,
    BrokenLink,
    Unrecognized,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub enum ManageAllowedResults {
    Selected,
    Both,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub enum ManageSelectedEffect {
    Retain,
    Add,
    Repair,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub enum ManageUnselectedEffect {
    KeepAbsent,
    Remove,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub enum ManageSelectionDisabledReason {
    UnrecognizedEntry,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct ManageInstallOptionState {
    pub option_id: AgentInstallOptionId,
    pub current_entry: ManageCurrentEntry,
    pub initial_selected: bool,
    pub allowed_results: ManageAllowedResults,
    pub selected_effect: Option<ManageSelectedEffect>,
    pub unselected_effect: Option<ManageUnselectedEffect>,
    pub disabled_reason: Option<ManageSelectionDisabledReason>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct ManageAgentSelectionSnapshot {
    pub selection: AgentSelectionSnapshot,
    pub option_states: Vec<ManageInstallOptionState>,
}

#[derive(Serialize)]
struct ResolvedManageSelection {
    context: ContextRef,
    skill_name: String,
    add: Vec<AgentWriteIntent>,
    #[serde(skip)]
    add_plan: AgentEntryPlan,
    remove_entry_ids: Vec<ObservedEntryId>,
    requested_mode: InstallMode,
}

struct ResolvedManageExecution {
    selection: ResolvedManageSelection,
}

pub(crate) struct LoadedManageSelection {
    pub(crate) skill_name: String,
    pub(crate) public: ManageAgentSelectionSnapshot,
    pub(crate) catalog: crate::application::agent_selection::AgentSelectionCatalog,
    pub(crate) observed: ObservedSkillSnapshot,
    pub(crate) observed_entry_ids: BTreeMap<AgentInstallOptionId, ObservedEntryId>,
}

fn manage_option_state(
    option_id: &AgentInstallOptionId,
    fact: &ResolvedTargetFact,
    canonical: &ResolvedTargetFact,
    placement_conflict: bool,
) -> ManageInstallOptionState {
    let recognized_link = fact
        .link_target
        .as_deref()
        .is_some_and(|target| link_points_to(fact, target, canonical));
    let mut state = match fact.entry_kind {
        TargetEntryKind::Missing => ManageInstallOptionState {
            option_id: option_id.clone(),
            current_entry: ManageCurrentEntry::None,
            initial_selected: false,
            allowed_results: ManageAllowedResults::Both,
            selected_effect: Some(ManageSelectedEffect::Add),
            unselected_effect: Some(ManageUnselectedEffect::KeepAbsent),
            disabled_reason: None,
        },
        TargetEntryKind::Directory => existing_option_state(option_id, ManageCurrentEntry::Copy),
        TargetEntryKind::Symlink | TargetEntryKind::Junction if recognized_link => {
            existing_option_state(option_id, ManageCurrentEntry::Link)
        }
        TargetEntryKind::BrokenLink if recognized_link => ManageInstallOptionState {
            option_id: option_id.clone(),
            current_entry: ManageCurrentEntry::BrokenLink,
            initial_selected: true,
            allowed_results: ManageAllowedResults::Both,
            selected_effect: Some(ManageSelectedEffect::Repair),
            unselected_effect: Some(ManageUnselectedEffect::Remove),
            disabled_reason: None,
        },
        TargetEntryKind::File
        | TargetEntryKind::Other
        | TargetEntryKind::Symlink
        | TargetEntryKind::Junction
        | TargetEntryKind::BrokenLink => ManageInstallOptionState {
            option_id: option_id.clone(),
            current_entry: ManageCurrentEntry::Unrecognized,
            initial_selected: false,
            allowed_results: ManageAllowedResults::None,
            selected_effect: None,
            unselected_effect: None,
            disabled_reason: Some(ManageSelectionDisabledReason::UnrecognizedEntry),
        },
    };
    if placement_conflict && fact.entry_kind != TargetEntryKind::Missing {
        state.initial_selected = true;
        state.allowed_results = ManageAllowedResults::Selected;
        state.selected_effect = Some(ManageSelectedEffect::Retain);
        state.unselected_effect = None;
    }
    state
}

fn existing_option_state(
    option_id: &AgentInstallOptionId,
    current_entry: ManageCurrentEntry,
) -> ManageInstallOptionState {
    ManageInstallOptionState {
        option_id: option_id.clone(),
        current_entry,
        initial_selected: true,
        allowed_results: ManageAllowedResults::Both,
        selected_effect: Some(ManageSelectedEffect::Retain),
        unselected_effect: Some(ManageUnselectedEffect::Remove),
        disabled_reason: None,
    }
}

fn requested_manage_option_ids(
    states: &[ManageInstallOptionState],
    submission: &AgentSelectionSubmission,
) -> Result<(BTreeSet<AgentInstallOptionId>, Vec<AgentInstallOptionId>), AppError> {
    let selected = submission
        .selected_option_ids
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    if selected.len() != submission.selected_option_ids.len() {
        return Err(selection_validation(
            AgentSelectionInvalidReason::DuplicateOption,
        ));
    }
    let states_by_id = states
        .iter()
        .map(|state| (&state.option_id, state))
        .collect::<BTreeMap<_, _>>();
    for option_id in &selected {
        if !states_by_id.contains_key(option_id) {
            return Err(selection_validation(
                AgentSelectionInvalidReason::OptionMissing,
            ));
        }
    }
    if states.iter().any(|state| {
        let is_selected = selected.contains(&state.option_id);
        matches!(
            (state.allowed_results, is_selected),
            (ManageAllowedResults::Selected, false) | (ManageAllowedResults::None, true)
        )
    }) {
        return Err(selection_validation(
            AgentSelectionInvalidReason::ResultNotAllowed,
        ));
    }
    let actionable = states
        .iter()
        .filter(|state| selected.contains(&state.option_id))
        .filter(|state| {
            matches!(
                state.selected_effect,
                Some(ManageSelectedEffect::Add | ManageSelectedEffect::Repair)
            )
        })
        .map(|state| state.option_id.clone())
        .collect();
    Ok((selected, actionable))
}

fn selection_validation(reason: AgentSelectionInvalidReason) -> AppError {
    AppError::AgentSelectionInvalid { reason }
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
    ) -> Result<ManageAgentsPreviewOutcome, AppError> {
        let loaded = self
            .load_selection(&request.context, &request.skill_name)
            .await?;
        if request.agent_selection.revision != loaded.public.selection.revision {
            return Ok(ManageAgentsPreviewOutcome::SelectionStale {
                snapshot: loaded.public,
            });
        }
        let selection = match self.resolve_requested_selection(&loaded, &request.agent_selection)? {
            Some(selection) => selection,
            None => {
                return Ok(ManageAgentsPreviewOutcome::SelectionStale {
                    snapshot: loaded.public,
                });
            }
        };
        let additions = self.resolve_additions(&selection).await?;
        let canonical_payload = if selection.add_plan.required_agent_roots.is_empty() {
            None
        } else {
            Some(
                self.acquirer
                    .acquire(
                        &request.context,
                        &request.skill_name,
                        &loaded.observed.canonical,
                    )
                    .await?,
            )
        };
        Ok(ManageAgentsPreviewOutcome::Ready {
            preview: manage_preview(&selection, &loaded.observed, &additions, canonical_payload)?,
        })
    }

    pub async fn selection(
        &self,
        context: &ContextRef,
        skill_name: &str,
    ) -> Result<ManageAgentSelectionSnapshot, AppError> {
        Ok(self.load_selection(context, skill_name).await?.public)
    }

    async fn load_selection(
        &self,
        context: &ContextRef,
        skill_name: &str,
    ) -> Result<LoadedManageSelection, AppError> {
        load_observed_agent_selection(&self.observer, &self.targets, context, skill_name).await
    }

    fn resolve_requested_selection(
        &self,
        loaded: &LoadedManageSelection,
        submission: &AgentSelectionSubmission,
    ) -> Result<Option<ResolvedManageSelection>, AppError> {
        let (selected, actionable_option_ids) =
            requested_manage_option_ids(&loaded.public.option_states, submission)?;
        let actionable_submission = AgentSelectionSubmission {
            revision: submission.revision.clone(),
            selected_option_ids: actionable_option_ids,
            requested_mode: submission.requested_mode.clone(),
        };
        let resolved =
            match resolve_agent_selection_submission(&loaded.catalog, &actionable_submission)? {
                AgentSelectionResolution::Ready(selection) => selection,
                AgentSelectionResolution::Stale => return Ok(None),
            };
        let add_plan = resolved.entry_plan(false);
        let add = resolved
            .intents
            .into_iter()
            .filter(|intent| intent.own_directory_selected || !intent.adapter_targets.is_empty())
            .collect::<Vec<_>>();
        let remove_entry_ids = loaded
            .public
            .option_states
            .iter()
            .filter(|state| {
                (state.initial_selected && !selected.contains(&state.option_id))
                    || (state.selected_effect == Some(ManageSelectedEffect::Repair)
                        && selected.contains(&state.option_id))
            })
            .filter_map(|state| loaded.observed_entry_ids.get(&state.option_id).cloned())
            .collect::<Vec<_>>();

        Ok(Some(ResolvedManageSelection {
            context: loaded.observed.facts.resolved_context.context.clone(),
            skill_name: loaded.skill_name.clone(),
            add,
            add_plan,
            remove_entry_ids,
            requested_mode: submission.requested_mode.clone(),
        }))
    }

    pub async fn execute(
        &self,
        request: &ManageAgentsRequest,
        cancellation: CancellationSignal,
    ) -> Result<ManageAgentsResponse, AppError> {
        let loaded = self
            .load_selection(&request.context, &request.skill_name)
            .await?;
        if request.agent_selection.revision != loaded.public.selection.revision {
            return Err(AppError::StaleTarget);
        }
        let selection = self
            .resolve_requested_selection(&loaded, &request.agent_selection)?
            .ok_or(AppError::StaleTarget)?;
        let snapshot = loaded.observed;
        validate_manage_execution(
            &selection.add,
            &selection.remove_entry_ids,
            request.confirm_entity_directories,
            &snapshot
                .entries
                .iter()
                .map(|entry| entry.public.clone())
                .collect::<Vec<_>>(),
        )?;
        let additions = self.resolve_additions(&selection).await?;
        let canonical_lease = match (
            &request.canonical_payload,
            selection.add_plan.required_agent_roots.is_empty(),
        ) {
            (None, true) => None,
            (Some(handle), false)
                if same_environment_identity(&handle.environment, &request.context.environment) =>
            {
                Some(self.payloads.pin_verified(handle).await?)
            }
            _ => return Err(AppError::StalePayload),
        };
        let actual_preview = manage_preview(
            &selection,
            &snapshot,
            &additions,
            request.canonical_payload.clone(),
        )?;
        validate_token(&request.token, &actual_preview.token)?;
        let execution = ResolvedManageExecution { selection };
        let plan = build_manage_plan(
            &execution,
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
        request: &ResolvedManageSelection,
    ) -> Result<ResolvedAdditions, AppError> {
        let plan = request.add_plan.clone();
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

pub(crate) async fn load_observed_agent_selection<F, T>(
    observer: &SkillEntryObserver<F, T>,
    targets: &T,
    context: &ContextRef,
    skill_name: &str,
) -> Result<LoadedManageSelection, AppError>
where
    F: InstallPlanningFactSource,
    T: TargetFactResolver + Clone,
{
    let observed = observer.observe(context, skill_name).await?;
    build_observed_agent_selection(targets, context, skill_name, observed).await
}

pub(crate) async fn load_observed_agent_selection_for_copy<F, T>(
    observer: &SkillEntryObserver<F, T>,
    targets: &T,
    context: &ContextRef,
    skill_name: &str,
) -> Result<LoadedManageSelection, AppError>
where
    F: InstallPlanningFactSource,
    T: TargetFactResolver + Clone,
{
    let observed = observer
        .observe_for_copy_source(context, skill_name)
        .await?;
    build_observed_agent_selection(targets, context, skill_name, observed).await
}

async fn build_observed_agent_selection<T>(
    targets: &T,
    context: &ContextRef,
    skill_name: &str,
    observed: ObservedSkillSnapshot,
) -> Result<LoadedManageSelection, AppError>
where
    T: TargetFactResolver + Clone,
{
    let mut catalog = build_agent_selection_catalog(
        context,
        &observed.facts.agent_runtime,
        &observed.facts.eve_targets,
        targets,
    )
    .await?;
    let option_ids = catalog
        .snapshot
        .install_options
        .iter()
        .map(|option| option.id.clone())
        .collect::<Vec<_>>();
    let destinations = option_ids
        .iter()
        .map(|option_id| {
            let option = catalog
                .resolved_options
                .get(option_id)
                .expect("catalog option has an internal target");
            join_entry(&option.root, skill_name)
        })
        .collect::<Vec<_>>();
    let option_facts = if destinations.is_empty() {
        Vec::new()
    } else {
        targets.resolve(context, &destinations, None).await?
    };
    if option_facts.len() != option_ids.len() {
        return Err(AppError::StaleTarget);
    }

    let observed_by_key = observed
        .entries
        .iter()
        .map(|entry| (&entry.fact.key, &entry.public.entry_id))
        .collect::<BTreeMap<_, _>>();
    let mut states = Vec::with_capacity(option_ids.len());
    let mut observed_entry_ids = BTreeMap::new();
    for (option_id, fact) in option_ids.into_iter().zip(option_facts) {
        let placement_conflict = catalog
            .resolved_options
            .get(&option_id)
            .is_some_and(|option| {
                option.public.disabled_reason
                    == Some(AgentSelectionDisabledReason::PlacementConflict)
            });
        let state = manage_option_state(&option_id, &fact, &observed.canonical, placement_conflict);
        if state.initial_selected {
            if let Some(entry_id) = observed_by_key.get(&fact.key) {
                observed_entry_ids.insert(option_id.clone(), (*entry_id).clone());
            }
        }
        states.push(state);
    }
    catalog.snapshot.initial_selected_option_ids = states
        .iter()
        .filter(|state| state.initial_selected)
        .map(|state| state.option_id.clone())
        .collect();
    catalog.snapshot.revision = AgentSelectionRevision(stable_digest(&(
        "manage-agent-selection-revision-v1",
        &catalog.snapshot.revision,
        states
            .iter()
            .map(|state| {
                (
                    &state.option_id,
                    state.current_entry,
                    state.initial_selected,
                    state.allowed_results,
                    state.selected_effect,
                    state.unselected_effect,
                    state.disabled_reason,
                )
            })
            .collect::<Vec<_>>(),
    ))?);
    catalog.snapshot.user_mode_option_ids = states
        .iter()
        .filter(|state| {
            matches!(
                state.selected_effect,
                Some(ManageSelectedEffect::Add | ManageSelectedEffect::Repair)
            )
        })
        .filter_map(|state| {
            catalog
                .resolved_options
                .get(&state.option_id)
                .filter(|option| {
                    option.public.selectable
                        && option.public.mode_constraint
                            == AgentSelectionModeConstraint::UserSelectable
                })
                .map(|_| state.option_id.clone())
        })
        .collect();

    Ok(LoadedManageSelection {
        skill_name: skill_name.to_string(),
        public: ManageAgentSelectionSnapshot {
            selection: catalog.snapshot.clone(),
            option_states: states,
        },
        catalog,
        observed,
        observed_entry_ids,
    })
}

struct ResolvedAdditions {
    plan: AgentEntryPlan,
    facts: Vec<ResolvedTargetFact>,
}

fn manage_preview(
    request: &ResolvedManageSelection,
    snapshot: &ObservedSkillSnapshot,
    additions: &ResolvedAdditions,
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
        canonical_payload,
        confirmation: snapshot
            .entries
            .iter()
            .filter(|entry| request.remove_entry_ids.contains(&entry.public.entry_id))
            .any(|entry| {
                entry.public.kind == crate::application::remove::ObservedEntryKind::Directory
            })
            .then_some(ManageAgentsConfirmation {
                removes_entity_directories: true,
            }),
    })
}

async fn build_manage_plan(
    request: &ResolvedManageExecution,
    snapshot: ObservedSkillSnapshot,
    additions: ResolvedAdditions,
    canonical_lease: Option<PinnedPayloadLease>,
    payload_manager: &PayloadSessionManager,
) -> Result<MutationPlan, AppError> {
    let selected = request
        .selection
        .remove_entry_ids
        .iter()
        .collect::<BTreeSet<_>>();
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
            .any(|target| target.content.uses_eve_payload())
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
        let eve = target.content.uses_eve_payload();
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
                    request.selection.requested_mode.clone()
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
    let lock_mutation = manage_lock_mutation(&request.selection, &snapshot, &additions)?;
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
            id: format!("manage-agents:{}", request.selection.skill_name),
            skill_name: request.selection.skill_name.clone(),
            source: None,
            target: request.selection.context.clone(),
            expected_revisions: snapshot.facts.revisions,
            canonical_entry,
            required_agent_entries,
            lock_mutation,
            expected_targets,
        }],
    })
}

fn manage_lock_mutation(
    request: &ResolvedManageSelection,
    snapshot: &ObservedSkillSnapshot,
    additions: &ResolvedAdditions,
) -> Result<Option<PreparedLockMutation>, AppError> {
    let selected = request.remove_entry_ids.iter().collect::<BTreeSet<_>>();
    let removes_eve = snapshot.entries.iter().any(|entry| {
        selected.contains(&entry.public.entry_id)
            && entry.public.owners.iter().any(|owner| {
                crate::core::eve::parse_eve_target_id(&owner.logical_target_id).is_some()
            })
    });
    let adds_eve = additions
        .plan
        .required_agent_roots
        .iter()
        .any(|target| target.content.uses_eve_payload());
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
    let mut subagents = snapshot
        .entries
        .iter()
        .filter(|entry| !selected.contains(&entry.public.entry_id))
        .flat_map(|entry| entry.public.owners.iter())
        .filter_map(|owner| crate::core::eve::parse_eve_target_id(&owner.logical_target_id))
        .map(|target| match target {
            crate::core::eve::EveTargetRef::Root => String::new(),
            crate::core::eve::EveTargetRef::Subagent(subagent) => subagent.to_string(),
        })
        .collect::<BTreeSet<_>>();
    subagents.extend(
        additions
            .plan
            .required_agent_roots
            .iter()
            .filter_map(|target| {
                target
                    .content
                    .eve_subagent()
                    .map(crate::core::eve::lock_subagent_value)
            }),
    );
    let Some(replacement) = eve_lock_replacement(
        snapshot
            .facts
            .lock_document
            .entry_snapshot(&request.skill_name)
            .value()
            .cloned(),
        &subagents,
    )?
    else {
        return Ok(None);
    };
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

fn eve_lock_replacement(
    raw: Option<serde_json::Value>,
    subagents: &BTreeSet<String>,
) -> Result<Option<serde_json::Value>, AppError> {
    let Some(mut replacement) = raw else {
        return Ok(None);
    };
    {
        let replacement =
            replacement
                .as_object_mut()
                .ok_or_else(|| AppError::ConfigurationCorrupted {
                    message: "project lock entry must be an object".to_string(),
                })?;
        if subagents.iter().any(|target| !target.is_empty()) {
            replacement.insert("subagents".to_string(), serde_json::json!(subagents));
        } else {
            replacement.remove("subagents");
        }
    }
    Ok(Some(replacement))
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
    use crate::application::agent_intent::AgentWriteIntent;
    use crate::core::agent_definition::AgentId;
    use crate::environment::runtime::ObservedEntryId;

    #[test]
    fn eve_management_without_a_lock_entry_skips_lock_mutation() {
        assert_eq!(
            eve_lock_replacement(None, &BTreeSet::from(["builder".to_string()])).unwrap(),
            None
        );
    }

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
            own_directory_selected: true,
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

    #[test]
    fn manage_selection_rejects_a_selected_unrecognized_entry() {
        let option_id = AgentInstallOptionId("option-unrecognized".to_string());
        let state = ManageInstallOptionState {
            option_id: option_id.clone(),
            current_entry: ManageCurrentEntry::Unrecognized,
            initial_selected: false,
            allowed_results: ManageAllowedResults::None,
            selected_effect: None,
            unselected_effect: None,
            disabled_reason: Some(ManageSelectionDisabledReason::UnrecognizedEntry),
        };
        let submission = AgentSelectionSubmission {
            revision: AgentSelectionRevision("revision".to_string()),
            selected_option_ids: vec![option_id],
            requested_mode: InstallMode::Copy,
        };

        assert!(requested_manage_option_ids(&[state], &submission).is_err());
    }

    #[test]
    fn manage_selection_rejects_unknown_option_ids() {
        let submission = AgentSelectionSubmission {
            revision: AgentSelectionRevision("revision".to_string()),
            selected_option_ids: vec![AgentInstallOptionId("missing".to_string())],
            requested_mode: InstallMode::Copy,
        };

        assert!(matches!(
            requested_manage_option_ids(&[], &submission),
            Err(AppError::AgentSelectionInvalid {
                reason: AgentSelectionInvalidReason::OptionMissing
            })
        ));
    }

    #[test]
    fn manage_selection_returns_only_options_that_need_an_add_or_repair() {
        let retained = AgentInstallOptionId("retained".to_string());
        let added = AgentInstallOptionId("added".to_string());
        let states = vec![
            ManageInstallOptionState {
                option_id: retained.clone(),
                current_entry: ManageCurrentEntry::Link,
                initial_selected: true,
                allowed_results: ManageAllowedResults::Both,
                selected_effect: Some(ManageSelectedEffect::Retain),
                unselected_effect: Some(ManageUnselectedEffect::Remove),
                disabled_reason: None,
            },
            ManageInstallOptionState {
                option_id: added.clone(),
                current_entry: ManageCurrentEntry::None,
                initial_selected: false,
                allowed_results: ManageAllowedResults::Both,
                selected_effect: Some(ManageSelectedEffect::Add),
                unselected_effect: Some(ManageUnselectedEffect::KeepAbsent),
                disabled_reason: None,
            },
        ];
        let submission = AgentSelectionSubmission {
            revision: AgentSelectionRevision("revision".to_string()),
            selected_option_ids: vec![retained.clone(), added.clone()],
            requested_mode: InstallMode::Symlink,
        };

        let (selected, actionable) = requested_manage_option_ids(&states, &submission).unwrap();

        assert_eq!(selected, BTreeSet::from([retained, added.clone()]));
        assert_eq!(actionable, vec![added]);
    }
}

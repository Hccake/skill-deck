use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::application::agent_intent::{validate_agent_intents, AgentWriteIntent};
use crate::application::agent_selection::{
    resolve_agent_selection_submission_for_snapshot, AgentInstallOptionId,
    AgentSelectionDisabledReason, AgentSelectionModeConstraint, AgentSelectionResolution,
    AgentSelectionRevision, AgentSelectionSnapshot, AgentSelectionSubmission,
    ResolvedAgentInstallOption,
};
use crate::application::installed_skill_payload::InstalledSkillPayloadAcquirer;
use crate::application::installed_skill_resolver::SkillDirectoryName;
use crate::application::library_agent_placements::LibraryAgentPlacementMap;
use crate::application::library_candidates::{LibraryCandidateSnapshot, LibraryCandidateSource};
use crate::application::mutation::executor::MutationPlanExecutor;
use crate::application::mutation::plan::{
    stable_digest, MutationPlan, PreparedEntryAction, PreviewToken,
};
use crate::application::mutation::planning::{
    assemble_plan, issue_preview_token, validate_exact_preview, MutationPlanDraft,
    MutationUnitDraft, PreviewTokenDraft,
};
use crate::application::mutation::result::MutationUnitResult;
use crate::application::payload_session::{
    AcquiredPayloadHandle, PayloadSessionManager, PinnedPayloadLease,
};
use crate::application::planning_facts::{ScopePlanningSnapshot, ScopePlanningSnapshotSource};
use crate::application::scope_skill_placements::{
    ResolvedScopeSkillPlacements, ScopeSkillPlacementResolver,
};
use crate::application::scope_skill_planning::{
    DirectContentIdentity, DirectPlacementChange, DirectSkillChangeRequest, LibraryElectionState,
    PreparedDirectVersion, ScopeSkillPlanner,
};
use crate::application::scope_skill_planning::{DirectoryPlacementRef, ObservedVersion};
use crate::application::skill_entry_projection::ObservedPhysicalEntry;
use crate::core::mutation::{CancellationSignal, MutationKind};
use crate::core::skill_payload::PayloadId;
use crate::environment::planning::{ResolvedTargetFact, TargetEntryKind, TargetFactResolver};
use crate::environment::runtime::ObservedEntryId;
use crate::environment::types::{same_environment_identity, SkillLocationRef};
use crate::error::{AgentSelectionInvalidReason, AppError};
use crate::models::InstallMode;
use crate::storage::lock_plan::{LockEntryMutation, LockExpectedState, PreparedLockMutation};

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct ManageAgentsPreviewRequest {
    pub context: SkillLocationRef,
    pub skill_name: String,
    pub agent_selection: AgentSelectionSubmission,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct ManageAgentsRequest {
    pub token: PreviewToken,
    pub context: SkillLocationRef,
    pub skill_name: String,
    pub agent_selection: AgentSelectionSubmission,
    pub confirm_entity_directories: bool,
    pub original_payload: Option<AcquiredPayloadHandle>,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct ManageAgentsPreview {
    pub token: PreviewToken,
    pub context: SkillLocationRef,
    pub skill_name: String,
    pub original_payload: Option<AcquiredPayloadHandle>,
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
pub enum ManageCurrentVersion {
    None,
    Direct,
    Library,
    External,
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
    RestoreLibrary,
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
    pub current_version: ManageCurrentVersion,
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
    context: SkillLocationRef,
    skill_name: String,
    add: Vec<AgentWriteIntent>,
    #[serde(skip)]
    add_options: Vec<ResolvedAgentInstallOption>,
    remove_entry_ids: Vec<ObservedEntryId>,
    requested_mode: InstallMode,
}

struct ResolvedManageExecution {
    selection: ResolvedManageSelection,
    catalog: crate::application::agent_selection::AgentSelectionCatalog,
}

pub(crate) struct LoadedManageSelection {
    pub(crate) skill_name: String,
    pub(crate) public: ManageAgentSelectionSnapshot,
    pub(crate) catalog: crate::application::agent_selection::AgentSelectionCatalog,
    observed: ManageObservedState,
    pub(crate) observed_entry_ids: BTreeMap<AgentInstallOptionId, ObservedEntryId>,
    pub(crate) library_candidates: LibraryCandidateSnapshot,
}

struct ManageObservedState {
    facts: ScopePlanningSnapshot,
    plan: crate::application::scope_skill_planning::ScopeSkillPlan,
    entries: Vec<crate::application::skill_entry_projection::ObservedPlannedEntry>,
    placements: ResolvedScopeSkillPlacements,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ManageObservedVersion {
    Missing,
    Direct,
    Library,
    BrokenDirect,
    BrokenLibrary,
    BrokenUnknown,
    External,
}

fn manage_option_state(
    option_id: &AgentInstallOptionId,
    fact: &ResolvedTargetFact,
    observed: ManageObservedVersion,
    library_available: bool,
    placement_conflict: bool,
) -> ManageInstallOptionState {
    let mut state = match observed {
        ManageObservedVersion::Missing => ManageInstallOptionState {
            option_id: option_id.clone(),
            current_entry: ManageCurrentEntry::None,
            current_version: ManageCurrentVersion::None,
            initial_selected: false,
            allowed_results: ManageAllowedResults::Both,
            selected_effect: Some(ManageSelectedEffect::Add),
            unselected_effect: Some(ManageUnselectedEffect::KeepAbsent),
            disabled_reason: None,
        },
        ManageObservedVersion::Direct => existing_option_state(
            option_id,
            if fact.entry_kind == TargetEntryKind::Directory {
                ManageCurrentEntry::Copy
            } else {
                ManageCurrentEntry::Link
            },
            library_available,
        ),
        ManageObservedVersion::BrokenDirect => ManageInstallOptionState {
            option_id: option_id.clone(),
            current_entry: ManageCurrentEntry::BrokenLink,
            current_version: ManageCurrentVersion::Direct,
            initial_selected: true,
            allowed_results: ManageAllowedResults::Both,
            selected_effect: Some(ManageSelectedEffect::Repair),
            unselected_effect: Some(if library_available {
                ManageUnselectedEffect::RestoreLibrary
            } else {
                ManageUnselectedEffect::Remove
            }),
            disabled_reason: None,
        },
        ManageObservedVersion::Library => library_option_state(option_id, ManageCurrentEntry::Link),
        ManageObservedVersion::BrokenLibrary => {
            library_option_state(option_id, ManageCurrentEntry::BrokenLink)
        }
        ManageObservedVersion::BrokenUnknown => ManageInstallOptionState {
            option_id: option_id.clone(),
            current_entry: ManageCurrentEntry::BrokenLink,
            current_version: ManageCurrentVersion::External,
            initial_selected: false,
            allowed_results: ManageAllowedResults::Both,
            selected_effect: Some(ManageSelectedEffect::Repair),
            unselected_effect: Some(if library_available {
                ManageUnselectedEffect::RestoreLibrary
            } else {
                ManageUnselectedEffect::KeepAbsent
            }),
            disabled_reason: None,
        },
        ManageObservedVersion::External => ManageInstallOptionState {
            option_id: option_id.clone(),
            current_entry: ManageCurrentEntry::Unrecognized,
            current_version: ManageCurrentVersion::External,
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
    library_available: bool,
) -> ManageInstallOptionState {
    ManageInstallOptionState {
        option_id: option_id.clone(),
        current_entry,
        current_version: ManageCurrentVersion::Direct,
        initial_selected: true,
        allowed_results: ManageAllowedResults::Both,
        selected_effect: Some(ManageSelectedEffect::Retain),
        unselected_effect: Some(if library_available {
            ManageUnselectedEffect::RestoreLibrary
        } else {
            ManageUnselectedEffect::Remove
        }),
        disabled_reason: None,
    }
}

fn library_option_state(
    option_id: &AgentInstallOptionId,
    current_entry: ManageCurrentEntry,
) -> ManageInstallOptionState {
    ManageInstallOptionState {
        option_id: option_id.clone(),
        current_entry,
        current_version: ManageCurrentVersion::Library,
        initial_selected: false,
        allowed_results: ManageAllowedResults::Both,
        selected_effect: Some(ManageSelectedEffect::Add),
        unselected_effect: Some(ManageUnselectedEffect::KeepAbsent),
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
    facts: F,
    observer: ScopeSkillPlacementResolver<T>,
    targets: T,
    payloads: Arc<PayloadSessionManager>,
    acquirer: InstalledSkillPayloadAcquirer,
    executor: E,
    library_candidates: Arc<dyn LibraryCandidateSource>,
}

impl<F, T, E> ManageAgentsService<F, T, E>
where
    F: ScopePlanningSnapshotSource,
    T: TargetFactResolver + Clone,
    E: MutationPlanExecutor,
{
    pub fn new(
        facts: F,
        observer: ScopeSkillPlacementResolver<T>,
        targets: T,
        payloads: Arc<PayloadSessionManager>,
        acquirer: InstalledSkillPayloadAcquirer,
        executor: E,
        library_candidates: Arc<dyn LibraryCandidateSource>,
    ) -> Self {
        Self {
            facts,
            observer,
            targets,
            payloads,
            acquirer,
            executor,
            library_candidates,
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
        let original_payload = if selection.add_options.is_empty() {
            None
        } else {
            Some(
                self.acquirer
                    .acquire(
                        &request.context,
                        &request.skill_name,
                        loaded
                            .observed
                            .plan
                            .standard_fact()
                            .map_err(|error| error.into_app_error())?,
                    )
                    .await?,
            )
        };
        Ok(ManageAgentsPreviewOutcome::Ready {
            preview: manage_preview(
                &selection,
                &loaded.observed,
                &additions,
                original_payload,
                &loaded.library_candidates,
            )?,
        })
    }

    pub async fn selection(
        &self,
        context: &SkillLocationRef,
        skill_name: &str,
    ) -> Result<ManageAgentSelectionSnapshot, AppError> {
        Ok(self.load_selection(context, skill_name).await?.public)
    }

    async fn load_selection(
        &self,
        context: &SkillLocationRef,
        skill_name: &str,
    ) -> Result<LoadedManageSelection, AppError> {
        let facts = self.facts.snapshot(context).await?;
        let catalog = crate::application::agent_selection::build_agent_selection_catalog(
            context,
            &facts.agent_runtime,
            &facts.eve_targets,
            &facts.resolved_context.skill_root,
            &self.targets,
        )
        .await?;
        let observed = self
            .observer
            .observe(context, skill_name, &facts, &catalog)
            .await?;
        let library_candidates = self.library_candidates(context, skill_name).await?;
        build_observed_agent_selection(skill_name, catalog, facts, observed, &library_candidates)
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
        let resolved = match resolve_agent_selection_submission_for_snapshot(
            &loaded.catalog,
            &loaded.public.selection,
            &actionable_submission,
        )? {
            AgentSelectionResolution::Ready(selection) => selection,
            AgentSelectionResolution::Stale => return Ok(None),
        };
        let add_options = resolved.selected_options().to_vec();
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
            add_options,
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
        let catalog = loaded.catalog;
        let observed = loaded.observed;
        let library_candidates = loaded.library_candidates;
        validate_manage_execution(
            &selection.add,
            &selection.remove_entry_ids,
            request.confirm_entity_directories,
            &observed
                .entries
                .iter()
                .map(|entry| entry.public.clone())
                .collect::<Vec<_>>(),
        )?;
        let additions = self.resolve_additions(&selection).await?;
        let canonical_lease = match (&request.original_payload, selection.add_options.is_empty()) {
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
            &observed,
            &additions,
            request.original_payload.clone(),
            &library_candidates,
        )?;
        validate_exact_preview(&request.token, &actual_preview.token)?;
        let execution = ResolvedManageExecution { selection, catalog };
        let plan = build_manage_plan(
            &execution,
            observed,
            additions,
            canonical_lease,
            &self.payloads,
            &library_candidates,
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
        let options = request.add_options.clone();
        let install_dir_name =
            crate::application::installed_skill_resolver::InstalledSkillResolver::install_dir_name(
                &request.skill_name,
            )?;
        let destinations = options
            .iter()
            .map(|option| option.placement.root.join_child(&install_dir_name))
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
        Ok(ResolvedAdditions { options, facts })
    }

    async fn library_candidates(
        &self,
        context: &SkillLocationRef,
        skill_name: &str,
    ) -> Result<LibraryCandidateSnapshot, AppError> {
        let skill = SkillDirectoryName::try_from(skill_name)?;
        self.library_candidates
            .load_candidates(context, &skill)
            .await
    }
}

fn manage_observed_version(
    fact: &ResolvedTargetFact,
    observed: &ObservedVersion,
) -> ManageObservedVersion {
    match (fact.entry_kind, observed) {
        (TargetEntryKind::Missing, _) => ManageObservedVersion::Missing,
        (TargetEntryKind::Directory, _) => ManageObservedVersion::Direct,
        (TargetEntryKind::Symlink | TargetEntryKind::Junction, ObservedVersion::Library(_)) => {
            ManageObservedVersion::Library
        }
        (TargetEntryKind::Symlink | TargetEntryKind::Junction, _) => ManageObservedVersion::Direct,
        (TargetEntryKind::BrokenLink, ObservedVersion::Library(_)) => {
            ManageObservedVersion::BrokenLibrary
        }
        (TargetEntryKind::BrokenLink, ObservedVersion::Direct) => {
            ManageObservedVersion::BrokenDirect
        }
        (TargetEntryKind::BrokenLink, ObservedVersion::Unknown) => {
            ManageObservedVersion::BrokenUnknown
        }
        (TargetEntryKind::File | TargetEntryKind::Other, _) => ManageObservedVersion::External,
    }
}

fn build_observed_agent_selection(
    skill_name: &str,
    catalog: crate::application::agent_selection::AgentSelectionCatalog,
    facts: ScopePlanningSnapshot,
    observed: ResolvedScopeSkillPlacements,
    library_candidates: &LibraryCandidateSnapshot,
) -> Result<LoadedManageSelection, AppError> {
    let scope_plan = ScopeSkillPlanner::plan_direct_change(DirectSkillChangeRequest {
        skill: SkillDirectoryName::try_from(skill_name)?,
        catalog: &catalog,
        placements: observed.placements.clone(),
        libraries: LibraryElectionState {
            candidates: library_candidates.candidates(),
            selected_agent_ids: library_candidates.selected_agent_ids(),
        },
        direct_changes: BTreeMap::new(),
    })
    .map_err(|error| error.into_app_error())?;
    let library_target_ids = LibraryAgentPlacementMap::from_catalog(&catalog)
        .placements_for(library_candidates.selected_agent_ids())
        .map_err(|error| match error {
            crate::application::library_agent_placements::LibraryAgentPlacementError::UnknownAgent(
                agent,
            ) => AppError::InvalidAgent {
                agent: agent.as_str().to_string(),
            },
            crate::application::library_agent_placements::LibraryAgentPlacementError::PartialSelection(
                _,
            ) => AppError::AgentSelectionInvalid {
                reason: AgentSelectionInvalidReason::OptionUnavailable,
            },
        })?;
    let option_ids = catalog
        .snapshot()
        .install_options
        .iter()
        .map(|option| option.id.clone())
        .collect::<Vec<_>>();
    let option_facts = option_ids
        .iter()
        .map(|option_id| {
            observed
                .placements
                .facts()
                .get(
                    &crate::application::agent_selection::DirectoryPlacementId::Option(
                        option_id.clone(),
                    ),
                )
                .cloned()
                .ok_or(AppError::StaleTarget)
        })
        .collect::<Result<Vec<_>, _>>()?;

    let entries = scope_plan
        .project_observed_entries()
        .map_err(|error| error.into_app_error())?;
    let observed_by_key = entries
        .iter()
        .map(|entry| (&entry.fact.key, &entry.public.entry_id))
        .collect::<BTreeMap<_, _>>();
    let mut states = Vec::with_capacity(option_ids.len());
    let mut observed_entry_ids = BTreeMap::new();
    for (option_id, fact) in option_ids.into_iter().zip(option_facts) {
        let placement_conflict = catalog.option(&option_id).is_some_and(|option| {
            option.public.disabled_reason == Some(AgentSelectionDisabledReason::PlacementConflict)
        });
        let placement_id =
            crate::application::agent_selection::DirectoryPlacementId::Option(option_id.clone());
        let library_target = !library_candidates.candidates().ordered().is_empty()
            && library_target_ids.contains(&placement_id);
        let directory = scope_plan
            .directories()
            .iter()
            .find(|directory| {
                directory
                    .placements()
                    .contains(&DirectoryPlacementRef::Catalog(placement_id.clone()))
            })
            .ok_or(AppError::StaleTarget)?;
        let observed_source = manage_observed_version(&fact, directory.observed());
        let state = manage_option_state(
            &option_id,
            &fact,
            observed_source,
            library_target,
            placement_conflict,
        );
        if state.initial_selected {
            if let Some(entry_id) = observed_by_key.get(&fact.key) {
                observed_entry_ids.insert(option_id.clone(), (*entry_id).clone());
            }
        }
        states.push(state);
    }
    let initial_selected_option_ids = states
        .iter()
        .filter(|state| state.initial_selected)
        .map(|state| state.option_id.clone())
        .collect();
    let revision = AgentSelectionRevision(stable_digest(&(
        "manage-agent-selection-revision-v1",
        &catalog.snapshot().revision,
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
    let user_mode_option_ids = states
        .iter()
        .filter(|state| {
            matches!(
                state.selected_effect,
                Some(ManageSelectedEffect::Add | ManageSelectedEffect::Repair)
            )
        })
        .filter_map(|state| {
            catalog
                .option(&state.option_id)
                .filter(|option| {
                    option.public.selectable
                        && option.public.mode_constraint
                            == AgentSelectionModeConstraint::UserSelectable
                })
                .map(|_| state.option_id.clone())
        })
        .collect();
    let mut selection = catalog.snapshot().clone();
    selection.initial_selected_option_ids = initial_selected_option_ids;
    selection.revision = revision;
    selection.user_mode_option_ids = user_mode_option_ids;

    Ok(LoadedManageSelection {
        skill_name: skill_name.to_string(),
        public: ManageAgentSelectionSnapshot {
            selection,
            option_states: states,
        },
        catalog,
        observed: ManageObservedState {
            facts,
            plan: scope_plan,
            entries,
            placements: observed,
        },
        observed_entry_ids,
        library_candidates: library_candidates.clone(),
    })
}

struct ResolvedAdditions {
    options: Vec<ResolvedAgentInstallOption>,
    facts: Vec<ResolvedTargetFact>,
}

fn manage_preview(
    request: &ResolvedManageSelection,
    observed: &ManageObservedState,
    additions: &ResolvedAdditions,
    original_payload: Option<AcquiredPayloadHandle>,
    library_candidates: &LibraryCandidateSnapshot,
) -> Result<ManageAgentsPreview, AppError> {
    let facts = &observed.facts;
    let plan = &observed.plan;
    let observed_entries = &observed.entries;
    let snapshot = &observed.placements;
    let observed_state_digest = stable_digest(&(
        &plan
            .standard_fact()
            .map_err(|error| error.into_app_error())?
            .key,
        &plan
            .standard_fact()
            .map_err(|error| error.into_app_error())?
            .fingerprint,
        observed_entries
            .iter()
            .map(|entry| (&entry.public.entry_id, &entry.fact.fingerprint))
            .collect::<Vec<_>>(),
        additions
            .facts
            .iter()
            .map(|fact| (&fact.key, &fact.fingerprint))
            .collect::<Vec<_>>(),
        original_payload
            .as_ref()
            .map(|handle| (&handle.payload_id, &handle.manifest_hash)),
        facts
            .lock_document
            .entry_snapshot(&snapshot.resolved.lock_key)
            .value()
            .cloned(),
        library_candidates.evidence_digest(),
        library_candidates.selected_agent_ids(),
        library_candidates
            .candidates()
            .recognized()
            .iter()
            .map(|candidate| {
                (
                    candidate.library_id(),
                    candidate.member_name(),
                    candidate.locator(),
                )
            })
            .collect::<Vec<_>>(),
    ))?;
    let token = issue_preview_token(PreviewTokenDraft {
        kind: MutationKind::ManageAgents,
        request,
        revisions: facts.revisions.clone(),
        observed_state_digest,
        planner_contract_version: 3,
    })?;
    Ok(ManageAgentsPreview {
        token,
        context: request.context.clone(),
        skill_name: request.skill_name.clone(),
        original_payload,
        confirmation: observed_entries
            .iter()
            .filter(|entry| request.remove_entry_ids.contains(&entry.public.entry_id))
            .any(|entry| {
                entry.public.kind
                    == crate::application::skill_entry_projection::ObservedEntryKind::Directory
            })
            .then_some(ManageAgentsConfirmation {
                removes_entity_directories: true,
            }),
    })
}

async fn build_manage_plan(
    request: &ResolvedManageExecution,
    observed: ManageObservedState,
    additions: ResolvedAdditions,
    canonical_lease: Option<PinnedPayloadLease>,
    payload_manager: &PayloadSessionManager,
    library_candidates: &LibraryCandidateSnapshot,
) -> Result<MutationPlan, AppError> {
    let ManageObservedState {
        facts,
        entries: observed_entries,
        placements: snapshot,
        ..
    } = observed;
    let selected_removals = request
        .selection
        .remove_entry_ids
        .iter()
        .collect::<BTreeSet<_>>();
    let mut payloads = Vec::new();
    let mut original_payload_id = None;
    let mut eve_payload_id = None;
    if let Some(canonical) = canonical_lease {
        original_payload_id = Some(canonical.manifest().payload_id().clone());
        if additions
            .options
            .iter()
            .any(|option| option.placement.content.uses_eve_payload())
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
    let mut prepared_by_key = BTreeMap::new();
    for (option, fact) in additions.options.iter().zip(&additions.facts) {
        let eve = option.placement.content.uses_eve_payload();
        let payload_id = if eve {
            eve_payload_id.clone().ok_or(AppError::StalePayload)?
        } else {
            original_payload_id.clone().ok_or(AppError::StalePayload)?
        };
        prepared_by_key.insert(
            fact.key.clone(),
            PreparedDirectVersion::new(
                DirectContentIdentity::Payload(payload_id.clone()),
                PreparedEntryAction::Replace {
                    payload_id,
                    requested_mode: if eve {
                        InstallMode::Copy
                    } else {
                        request.selection.requested_mode.clone()
                    },
                },
            ),
        );
    }
    let snapshot_entries = observed_entries
        .iter()
        .map(|entry| (entry.fact.key.clone(), entry))
        .collect::<BTreeMap<_, _>>();
    let mut direct_changes = BTreeMap::new();
    for (placement_id, fact) in snapshot.placements.facts() {
        let selected_for_removal = snapshot_entries
            .get(&fact.key)
            .is_some_and(|entry| selected_removals.contains(&entry.public.entry_id));
        let change = prepared_by_key
            .get(&fact.key)
            .cloned()
            .map(DirectPlacementChange::Set)
            .unwrap_or_else(|| {
                if selected_for_removal {
                    DirectPlacementChange::Clear
                } else {
                    DirectPlacementChange::Preserve
                }
            });
        direct_changes.insert(placement_id.clone(), change);
    }
    let scope_plan = ScopeSkillPlanner::plan_direct_change(DirectSkillChangeRequest {
        skill: SkillDirectoryName::try_from(request.selection.skill_name.as_str())?,
        catalog: &request.catalog,
        placements: snapshot.placements.clone(),
        libraries: LibraryElectionState {
            candidates: library_candidates.candidates(),
            selected_agent_ids: library_candidates.selected_agent_ids(),
        },
        direct_changes,
    })
    .map_err(|error| error.into_app_error())?;
    let entries = scope_plan.compile_entries();
    let lock_mutation = manage_lock_mutation(
        &request.selection,
        &facts,
        &observed_entries,
        &snapshot.resolved,
        &additions,
    )?;
    Ok(assemble_plan(MutationPlanDraft {
        kind: MutationKind::ManageAgents,
        payloads: payloads
            .into_iter()
            .map(|lease| (lease.manifest().payload_id().clone(), lease))
            .collect::<BTreeMap<PayloadId, PinnedPayloadLease>>(),
        units: vec![MutationUnitDraft {
            id: format!("manage-agents:{}", request.selection.skill_name),
            skill_name: request.selection.skill_name.clone(),
            source: None,
            target: request.selection.context.clone(),
            expected_revisions: facts.revisions,
            entries,
            lock_mutation,
        }],
    }))
}

fn manage_lock_mutation(
    request: &ResolvedManageSelection,
    facts: &ScopePlanningSnapshot,
    observed_entries: &[crate::application::skill_entry_projection::ObservedPlannedEntry],
    resolved: &crate::application::installed_skill_resolver::ResolvedInstalledSkill,
    additions: &ResolvedAdditions,
) -> Result<Option<PreparedLockMutation>, AppError> {
    let selected = request.remove_entry_ids.iter().collect::<BTreeSet<_>>();
    let removes_eve = observed_entries.iter().any(|entry| {
        selected.contains(&entry.public.entry_id)
            && entry.public.readers.iter().any(|reader| {
                crate::core::eve::parse_eve_target_id(&reader.logical_target_id).is_some()
            })
    });
    let adds_eve = additions
        .options
        .iter()
        .any(|option| option.placement.content.uses_eve_payload());
    if !removes_eve && !adds_eve {
        return Ok(None);
    }
    if !matches!(
        request.context.scope,
        crate::environment::types::SkillLocation::Project { .. }
    ) {
        return Err(AppError::Validation {
            field: Some("eveTargets".to_string()),
            message: "Eve targets require Project Context".to_string(),
        });
    }
    let mut subagents = observed_entries
        .iter()
        .filter(|entry| !selected.contains(&entry.public.entry_id))
        .flat_map(|entry| entry.public.readers.iter())
        .filter_map(|reader| crate::core::eve::parse_eve_target_id(&reader.logical_target_id))
        .map(|target| match target {
            crate::core::eve::EveTargetRef::Root => String::new(),
            crate::core::eve::EveTargetRef::Subagent(subagent) => subagent.to_string(),
        })
        .collect::<BTreeSet<_>>();
    subagents.extend(additions.options.iter().filter_map(|option| {
        option
            .placement
            .content
            .eve_subagent()
            .map(crate::core::eve::lock_subagent_value)
    }));
    let Some(replacement) = eve_lock_replacement(
        facts
            .lock_document
            .entry_snapshot(&resolved.lock_key)
            .value()
            .cloned(),
        &subagents,
    )?
    else {
        return Ok(None);
    };
    Ok(Some(PreparedLockMutation {
        target: facts.resolved_context.lock.clone(),
        legacy_target: None,
        schema: facts.lock_schema,
        entry: if resolved.requires_lock_key_migration() {
            LockEntryMutation::MoveAndReplace {
                from: resolved.lock_key.clone(),
                to: resolved.skill_name.clone(),
                replacement,
            }
        } else {
            LockEntryMutation::Replace {
                key: resolved.lock_key.clone(),
                replacement,
            }
        },
        root_replacements: BTreeMap::new(),
        expected: LockExpectedState::capture(
            &facts.lock_document,
            [resolved.lock_key.as_str(), resolved.skill_name.as_str()],
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
        removes_directory |=
            entry.kind == crate::application::skill_entry_projection::ObservedEntryKind::Directory;
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
    use crate::environment::planning::{ResolvedTargetFact, TargetEntryKind};
    use crate::environment::runtime::{
        EntryFingerprint, ExecutionBackend, ObservedEntryId, PhysicalParentIdentity,
        PhysicalTargetKey,
    };
    use crate::environment::types::{EnvironmentRef, ResourceLocator, StorageAccess};

    #[test]
    fn library_link_is_available_without_being_a_direct_agent_association() {
        let option_id = AgentInstallOptionId("private-agent".to_string());
        let library_link = manage_target_fact(
            "private-agent",
            "/agents/private/skills/demo",
            TargetEntryKind::Symlink,
            Some("/libraries/lib-one/skills/demo"),
        );

        let candidates = library_candidates(
            &["/libraries/lib-one/skills/demo"],
            &["/libraries/lib-one/skills/demo"],
        );
        let observed = manage_observed_version(
            &library_link,
            &ObservedVersion::Library(candidates.candidates().recognized()[0].clone()),
        );
        let state = manage_option_state(&option_id, &library_link, observed, true, false);
        let serialized = serde_json::to_value(state).unwrap();

        assert_eq!(serialized["currentEntry"], "link");
        assert_eq!(serialized["currentVersion"], "library");
        assert_eq!(serialized["initialSelected"], false);
        assert_eq!(serialized["allowedResults"], "both");
        assert_eq!(serialized["selectedEffect"], "add");
        assert_eq!(serialized["unselectedEffect"], "keepAbsent");
        assert_eq!(serialized["disabledReason"], serde_json::Value::Null);
    }

    #[test]
    fn valid_non_library_link_is_preserved_as_a_historical_direct_association() {
        let option_id = AgentInstallOptionId("private-agent".to_string());
        let historical_link = manage_target_fact(
            "private-agent",
            "/agents/private/skills/demo",
            TargetEntryKind::Symlink,
            Some("/legacy/direct/demo"),
        );

        let observed = manage_observed_version(&historical_link, &ObservedVersion::Direct);
        let state = manage_option_state(&option_id, &historical_link, observed, true, false);

        assert_eq!(state.current_entry, ManageCurrentEntry::Link);
        assert_eq!(state.current_version, ManageCurrentVersion::Direct);
        assert!(state.initial_selected);
        assert_eq!(state.allowed_results, ManageAllowedResults::Both);
        assert_eq!(state.selected_effect, Some(ManageSelectedEffect::Retain));
        assert_eq!(
            state.unselected_effect,
            Some(ManageUnselectedEffect::RestoreLibrary)
        );
        assert_eq!(state.disabled_reason, None);
    }

    #[test]
    fn broken_unknown_link_can_be_repaired_with_an_explicit_direct_target() {
        let option_id = AgentInstallOptionId("private-agent".to_string());
        let fact = manage_target_fact(
            "private-agent",
            "/agents/private/skills/demo",
            TargetEntryKind::BrokenLink,
            Some("/missing/unknown"),
        );

        let state = manage_option_state(
            &option_id,
            &fact,
            ManageObservedVersion::BrokenUnknown,
            true,
            false,
        );

        assert!(!state.initial_selected);
        assert_eq!(state.allowed_results, ManageAllowedResults::Both);
        assert_eq!(state.selected_effect, Some(ManageSelectedEffect::Repair));
        assert_eq!(
            state.unselected_effect,
            Some(ManageUnselectedEffect::RestoreLibrary)
        );
        assert_eq!(state.disabled_reason, None);
    }

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
        let observed = vec![
            crate::application::skill_entry_projection::ObservedPhysicalEntry {
                entry_id: id.clone(),
                display_path: crate::environment::types::ResourceLocator {
                    environment: crate::environment::types::EnvironmentRef::Native,
                    native_path: "/agent/skills/demo".to_string(),
                },
                kind: crate::application::skill_entry_projection::ObservedEntryKind::Directory,
                physical_target_key: "target-v1-copy".to_string(),
                readers: Vec::new(),
                will_break_if_standard_removed: false,
            },
        ];

        assert!(validate_manage_execution(&[], &[id], false, &observed).is_err());
    }

    #[test]
    fn manage_selection_rejects_a_selected_unrecognized_entry() {
        let option_id = AgentInstallOptionId("option-unrecognized".to_string());
        let state = ManageInstallOptionState {
            option_id: option_id.clone(),
            current_entry: ManageCurrentEntry::Unrecognized,
            current_version: ManageCurrentVersion::External,
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
                current_version: ManageCurrentVersion::Direct,
                initial_selected: true,
                allowed_results: ManageAllowedResults::Both,
                selected_effect: Some(ManageSelectedEffect::Retain),
                unselected_effect: Some(ManageUnselectedEffect::Remove),
                disabled_reason: None,
            },
            ManageInstallOptionState {
                option_id: added.clone(),
                current_entry: ManageCurrentEntry::None,
                current_version: ManageCurrentVersion::None,
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

    fn library_candidates(known: &[&str], ordered: &[&str]) -> LibraryCandidateSnapshot {
        let recognized = known
            .iter()
            .enumerate()
            .map(|(index, path)| {
                crate::application::library_candidates::LibraryVersionCandidate::new(
                    crate::application::skill_libraries::LibraryId::parse(format!("lib-{index}")),
                    "demo",
                    ResourceLocator {
                        environment: EnvironmentRef::Native,
                        native_path: (*path).to_string(),
                    },
                )
            })
            .collect::<Vec<_>>();
        let ordered = ordered
            .iter()
            .map(|path| {
                recognized
                    .iter()
                    .find(|candidate| candidate.locator().native_path == *path)
                    .cloned()
                    .unwrap()
            })
            .collect();
        LibraryCandidateSnapshot::new(
            "library-evidence-1",
            Vec::new(),
            crate::application::library_candidates::LibraryCandidateSet::new(recognized, ordered)
                .unwrap(),
        )
        .unwrap()
    }

    fn manage_target_fact(
        name: &str,
        destination: &str,
        entry_kind: TargetEntryKind,
        link_target: Option<&str>,
    ) -> ResolvedTargetFact {
        let destination = ResourceLocator {
            environment: EnvironmentRef::Native,
            native_path: destination.to_string(),
        };
        ResolvedTargetFact {
            key: PhysicalTargetKey {
                backend: ExecutionBackend::NativeUnix,
                physical_parent: PhysicalParentIdentity::Unix {
                    device: 7,
                    inode: if name == "canonical" { 11 } else { 12 },
                },
                normalized_final_child_name: name.to_string(),
            },
            link_target_identity: link_target.and_then(|raw| {
                crate::environment::planning::resolve_link_target_identity(&destination, raw)
            }),
            destination,
            storage_access: StorageAccess::Native,
            fingerprint: EntryFingerprint(format!("entry-v1-{name}")),
            entry_kind,
            link_target: link_target.map(str::to_string),
        }
    }
}

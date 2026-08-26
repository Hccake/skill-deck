use std::collections::{BTreeMap, BTreeSet};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use specta::Type;

use crate::application::agent_intent::{AgentTargetFallbackPreview, AgentWriteIntent};
use crate::application::agent_selection::{
    build_agent_selection_catalog, map_agent_intents_to_submission,
    resolve_agent_selection_submission, resolve_agent_selection_submission_for_snapshot,
    AgentInstallOptionId, AgentSelectionModeConstraint, AgentSelectionResolution,
    AgentSelectionSnapshot, AgentSelectionSubmission, DirectoryPlacementId, SkillDirectoryAccess,
};
use crate::application::installed_skill_payload::InstalledSkillPayloadAcquirer;
use crate::application::installed_skill_resolver::SkillDirectoryName;
#[cfg(test)]
use crate::application::library_candidates::EmptyLibraryCandidateSource;
#[cfg(test)]
#[cfg(all(test, unix))]
use crate::application::library_candidates::{LibraryCandidateSet, LibraryVersionCandidate};
use crate::application::library_candidates::{LibraryCandidateSnapshot, LibraryCandidateSource};
use crate::application::mutation::executor::MutationPlanExecutor;
use crate::application::mutation::plan::{
    stable_digest, MutationPlan, PreparedEntryAction, PreviewToken, RuntimeRevisions,
};
use crate::application::mutation::planning::{
    assemble_plan, issue_preview_token, validate_exact_preview, MutationPlanDraft,
    MutationUnitDraft, PreviewTokenDraft,
};
use crate::application::mutation::result::{
    ErrorReport, MutationUnitResult, MutationUnitStatus, OperationErrorCode,
};
use crate::application::payload_session::{
    AcquiredPayloadHandle, CopySourceSnapshot, PayloadSessionManager, PinnedPayloadLease,
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
use crate::core::agent_definition::AgentId;
use crate::core::mutation::{CancellationSignal, MutationKind};
use crate::core::skill_payload::{validate_manifest_for_target, PayloadId, TargetPathProfile};
use crate::environment::agent_environment::DetectionState;
use crate::environment::planning::{ResolvedTargetFact, TargetEntryKind, TargetFactResolver};
use crate::environment::runtime::{
    ContextSnapshotRevision, ExecutionBackend, PhysicalIdentityComparison,
};
use crate::environment::types::{
    same_environment_identity, EnvironmentRef, ResourceLocator, SkillLocationRef, StorageAccess,
};
use crate::error::AppError;
use crate::models::InstallMode;
use crate::storage::lock_plan::{LockEntryMutation, LockExpectedState, PreparedLockMutation};

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct CopyRequest {
    pub skill_name: String,
    pub source: SkillLocationRef,
    pub target_environment: EnvironmentRef,
    pub target_project_ids: Vec<String>,
    pub agent_selection: AgentSelectionSubmission,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct CopyAgentSelectionSnapshot {
    pub selection: AgentSelectionSnapshot,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct CopyExecutionRequest {
    pub request: CopyRequest,
    pub token: PreviewToken,
    pub payload: AcquiredPayloadHandle,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct CopyPreview {
    pub token: PreviewToken,
    pub payload: AcquiredPayloadHandle,
    pub source: SkillLocationRef,
    pub target_environment: EnvironmentRef,
    pub targets: Vec<CopyTargetPreview>,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(tag = "status", rename_all = "camelCase")]
#[specta(tag = "status", rename_all = "camelCase")]
#[allow(
    clippy::large_enum_variant,
    reason = "IPC 结果保持直接 DTO，避免只为内存布局增加 Box 和调用侧解包"
)]
pub enum CopyPreviewOutcome {
    Ready {
        preview: CopyPreview,
    },
    SelectionStale {
        snapshot: CopyAgentSelectionSnapshot,
    },
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct CopyTargetPreview {
    pub project_id: String,
    pub display_name: String,
    pub storage_access: StorageAccess,
    pub physical_identity: PhysicalIdentityComparison,
    pub agent_targets: Vec<AgentTargetPreview>,
    pub fallback_forecasts: Vec<AgentTargetFallbackPreview>,
    pub blocking_reasons: Vec<OperationErrorCode>,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct AgentTargetPreview {
    pub agent_id: AgentId,
    pub target_id: String,
    pub display_path: ResourceLocator,
    pub own_directory_selected: bool,
    pub availability: DetectionState,
    pub blocking_reason: Option<OperationErrorCode>,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct CopyResponse {
    pub units: Vec<MutationUnitResult>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectComparison {
    pub physical_identity: PhysicalIdentityComparison,
    pub target_storage_access: StorageAccess,
}

pub type CopyFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

pub trait CopyProjectComparator: Send + Sync {
    fn capture_source<'a>(
        &'a self,
        source: &'a ScopePlanningSnapshot,
    ) -> CopyFuture<'a, Result<ResolvedTargetFact, AppError>>;

    fn compare<'a>(
        &'a self,
        source: &'a ResolvedTargetFact,
        target: &'a ScopePlanningSnapshot,
    ) -> CopyFuture<'a, Result<ProjectComparison, AppError>>;
}

pub struct CopyService<F, T, E, C> {
    facts: F,
    targets: T,
    observer: ScopeSkillPlacementResolver<T>,
    payloads: Arc<PayloadSessionManager>,
    acquirer: InstalledSkillPayloadAcquirer,
    executor: E,
    comparator: C,
    library_candidates: Arc<dyn LibraryCandidateSource>,
}

impl<F, T, E, C> CopyService<F, T, E, C>
where
    F: ScopePlanningSnapshotSource + Clone,
    T: TargetFactResolver + Clone,
    E: MutationPlanExecutor,
    C: CopyProjectComparator,
{
    pub fn new(
        facts: F,
        targets: T,
        payloads: Arc<PayloadSessionManager>,
        acquirer: InstalledSkillPayloadAcquirer,
        executor: E,
        comparator: C,
        library_candidates: Arc<dyn LibraryCandidateSource>,
    ) -> Self {
        Self {
            observer: ScopeSkillPlacementResolver::new(targets.clone()),
            facts,
            targets,
            payloads,
            acquirer,
            executor,
            comparator,
            library_candidates,
        }
    }

    pub async fn selection(
        &self,
        source: &SkillLocationRef,
        skill_name: &str,
    ) -> Result<CopyAgentSelectionSnapshot, AppError> {
        let loaded = self.load_copy_source(source, skill_name).await?;
        Ok(CopyAgentSelectionSnapshot {
            selection: copy_selection_snapshot(loaded.selection.clone()),
        })
    }

    pub async fn preview(&self, request: &CopyRequest) -> Result<CopyPreviewOutcome, AppError> {
        validate_copy_request(request)?;
        let loaded = self
            .load_copy_source(&request.source, &request.skill_name)
            .await?;
        let source = &loaded.observed;
        let source_selection = match resolve_agent_selection_submission_for_snapshot(
            &loaded.catalog,
            &loaded.selection,
            &request.agent_selection,
        )? {
            AgentSelectionResolution::Ready(selection) => selection,
            AgentSelectionResolution::Stale => {
                return Ok(CopyPreviewOutcome::SelectionStale {
                    snapshot: CopyAgentSelectionSnapshot {
                        selection: copy_selection_snapshot(loaded.selection.clone()),
                    },
                });
            }
        };
        let source_lock_entry = loaded
            .facts
            .lock_document
            .entry_snapshot(&source.resolved.lock_key)
            .value()
            .cloned();
        let handle = self
            .acquirer
            .acquire(
                &request.source,
                &request.skill_name,
                loaded
                    .plan
                    .standard_fact()
                    .map_err(|error| error.into_app_error())?,
            )
            .await?;
        let payload = self.payloads.pin_verified(&handle).await?;
        let source_snapshot = CopySourceSnapshot {
            source_context: request.source.clone(),
            skill_name: request.skill_name.clone(),
            revisions: loaded.facts.revisions.clone(),
            lock_entry: source_lock_entry,
            project_identity: self.comparator.capture_source(&loaded.facts).await?,
            standard_identity: loaded
                .plan
                .standard_fact()
                .map_err(|error| error.into_app_error())?
                .clone(),
            agent_intents: source_selection.intents().to_vec(),
        };
        self.payloads
            .bind_copy_source_snapshot(&handle, source_snapshot.clone())?;
        let built = self
            .build(
                request,
                source_selection.intents(),
                &source_snapshot,
                payload,
                false,
            )
            .await?;
        Ok(CopyPreviewOutcome::Ready {
            preview: CopyPreview {
                token: built.token,
                payload: handle,
                source: request.source.clone(),
                target_environment: request.target_environment.clone(),
                targets: built.previews,
            },
        })
    }

    pub async fn execute(
        &self,
        execution: &CopyExecutionRequest,
        cancellation: CancellationSignal,
    ) -> Result<CopyResponse, AppError> {
        validate_copy_request(&execution.request)?;
        let payload = self.payloads.pin_verified(&execution.payload).await?;
        let source_snapshot = self.payloads.copy_source_snapshot(&execution.payload)?;
        validate_copy_source_snapshot(&source_snapshot, &execution.request)?;
        self.validate_current_source(
            &source_snapshot,
            &execution.request,
            &execution.payload.manifest_hash,
        )
        .await?;
        let built = self
            .build(
                &execution.request,
                &source_snapshot.agent_intents,
                &source_snapshot,
                payload,
                true,
            )
            .await?;
        validate_exact_preview(&execution.token, &built.token)?;
        let plan = built.plan.expect("execute build has a plan");
        let mut units = if plan.units.is_empty() {
            Vec::new()
        } else {
            self.executor.execute(plan, cancellation).await
        };
        units.extend(
            built
                .planning_failures
                .iter()
                .map(|failure| failure.result(&execution.request)),
        );
        units.sort_by_key(|unit| {
            let project_id = project_id(&unit.target).unwrap_or_default();
            execution
                .request
                .target_project_ids
                .iter()
                .position(|candidate| candidate == project_id)
                .unwrap_or(usize::MAX)
        });
        Ok(CopyResponse { units })
    }

    async fn validate_current_source(
        &self,
        snapshot: &CopySourceSnapshot,
        request: &CopyRequest,
        expected_manifest_hash: &str,
    ) -> Result<(), AppError> {
        let loaded = self
            .load_copy_source(&request.source, &request.skill_name)
            .await?;
        let selection = match resolve_agent_selection_submission_for_snapshot(
            &loaded.catalog,
            &loaded.selection,
            &request.agent_selection,
        )? {
            AgentSelectionResolution::Ready(selection) => selection,
            AgentSelectionResolution::Stale => return Err(AppError::StaleContext),
        };
        let current = CopySourceSnapshot {
            source_context: request.source.clone(),
            skill_name: request.skill_name.clone(),
            revisions: loaded.facts.revisions.clone(),
            lock_entry: loaded
                .facts
                .lock_document
                .entry_snapshot(&loaded.observed.resolved.lock_key)
                .value()
                .cloned(),
            project_identity: self.comparator.capture_source(&loaded.facts).await?,
            standard_identity: loaded
                .plan
                .standard_fact()
                .map_err(|error| error.into_app_error())?
                .clone(),
            agent_intents: selection.intents().to_vec(),
        };

        if snapshot.revisions.registry != current.revisions.registry {
            return Err(AppError::StaleRegistry);
        }
        if snapshot.revisions.environment != current.revisions.environment {
            return Err(AppError::StaleEnvironment);
        }
        if snapshot.revisions.context != current.revisions.context
            || snapshot.lock_entry != current.lock_entry
            || snapshot.project_identity != current.project_identity
            || snapshot.standard_identity != current.standard_identity
            || snapshot.agent_intents != current.agent_intents
        {
            return Err(AppError::StaleContext);
        }
        let current_manifest_hash = self
            .acquirer
            .current_manifest_hash(
                &request.source,
                &request.skill_name,
                &current.standard_identity,
            )
            .await?;
        if current_manifest_hash != expected_manifest_hash {
            return Err(AppError::StalePayload);
        }
        Ok(())
    }

    async fn current_library_candidates(
        &self,
        context: &SkillLocationRef,
        skill_name: &str,
    ) -> Result<LibraryCandidateSnapshot, AppError> {
        let skill = SkillDirectoryName::try_from(skill_name)?;
        self.library_candidates
            .load_candidates(context, &skill)
            .await
    }

    async fn load_copy_source(
        &self,
        context: &SkillLocationRef,
        skill_name: &str,
    ) -> Result<LoadedCopySource, AppError> {
        let facts = self.facts.copy_source_snapshot(context).await?;
        let catalog = build_agent_selection_catalog(
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
        let library_candidates = self.current_library_candidates(context, skill_name).await?;
        let (option_states, plan) =
            copy_observed_option_states(&catalog, &observed, &library_candidates)?;
        let selection = copy_observed_selection(&catalog, &option_states, &library_candidates)?;
        Ok(LoadedCopySource {
            facts,
            catalog,
            selection,
            plan,
            observed,
        })
    }

    async fn build(
        &self,
        request: &CopyRequest,
        source_agent_intents: &[AgentWriteIntent],
        source: &CopySourceSnapshot,
        original_payload: PinnedPayloadLease,
        include_plan: bool,
    ) -> Result<BuiltCopy, AppError> {
        let source_lock_entry = source.lock_entry.as_ref();
        let mut targets = Vec::with_capacity(request.target_project_ids.len());
        let mut planning_failures = Vec::new();
        let canonical = original_payload.load_payload().await?;
        let mut needs_eve = false;
        for project_id in &request.target_project_ids {
            let context = project_context(&request.target_environment, project_id);
            let target = async {
                let library_candidates = self
                    .current_library_candidates(&context, &request.skill_name)
                    .await?;
                let facts = self.facts.snapshot(&context).await?;
                let catalog = build_agent_selection_catalog(
                    &context,
                    &facts.agent_runtime,
                    &facts.eve_targets,
                    &facts.resolved_context.skill_root,
                    &self.targets,
                )
                .await?;
                let observed = self
                    .observer
                    .observe(&context, &request.skill_name, &facts, &catalog)
                    .await?;
                let (option_states, _) =
                    copy_observed_option_states(&catalog, &observed, &library_candidates)?;
                let target_catalog = catalog;
                let mut target_submission = map_agent_intents_to_submission(
                    &target_catalog,
                    source_agent_intents,
                    request.agent_selection.requested_mode.clone(),
                );
                target_submission.selected_option_ids.extend(
                    option_states
                        .iter()
                        .filter(|state| state.initial_selected)
                        .map(|state| state.option_id.clone()),
                );
                target_submission.selected_option_ids.sort();
                target_submission.selected_option_ids.dedup();
                let selected_option_ids = target_submission.selected_option_ids.clone();
                let entry_modes = copy_entry_modes(
                    &target_catalog,
                    &option_states,
                    &target_submission.selected_option_ids,
                    &request.agent_selection.requested_mode,
                );
                match resolve_agent_selection_submission(&target_catalog, &target_submission)? {
                    AgentSelectionResolution::Ready(_) => {}
                    AgentSelectionResolution::Stale => return Err(AppError::StaleTarget),
                }
                let mut target_facts = vec![observed
                    .placements
                    .facts()
                    .get(&DirectoryPlacementId::Standard)
                    .cloned()
                    .ok_or(AppError::StaleTarget)?];
                for option_id in &selected_option_ids {
                    target_facts.push(
                        observed
                            .placements
                            .facts()
                            .get(&DirectoryPlacementId::Option(option_id.clone()))
                            .cloned()
                            .ok_or(AppError::StaleTarget)?,
                    );
                }
                let comparison = self
                    .comparator
                    .compare(&source.project_identity, &facts)
                    .await?;
                Ok::<BuiltCopyTarget, AppError>(BuiltCopyTarget {
                    context: context.clone(),
                    facts,
                    entry_modes,
                    target_facts,
                    comparison,
                    library_candidates,
                    catalog: target_catalog,
                    observed,
                    selected_option_ids,
                })
            }
            .await;
            match target {
                Ok(target)
                    if target.comparison.physical_identity == PhysicalIdentityComparison::Same =>
                {
                    planning_failures.push(CopyPlanningFailure {
                        context: target.context,
                        error: AppError::SelfCopy,
                    });
                }
                Ok(target) => {
                    needs_eve |= target
                        .selected_option_ids
                        .iter()
                        .filter_map(|id| target.catalog.option(id))
                        .any(|option| option.placement.content.uses_eve_payload());
                    targets.push(target);
                }
                Err(error) => planning_failures.push(CopyPlanningFailure { context, error }),
            }
        }
        let eve_payload = if needs_eve {
            let derived = crate::core::eve::derive_eve_skill_payload(&canonical)?;
            Some(
                self.payloads
                    .pin_derived_payload(&original_payload, "eve-copy", derived)
                    .await?,
            )
        } else {
            None
        };
        let mut validated_targets = Vec::with_capacity(targets.len());
        for target in targets {
            if let Err(error) =
                validate_copy_payload_targets(&canonical, eve_payload.as_ref(), &target).await
            {
                planning_failures.push(CopyPlanningFailure {
                    context: target.context,
                    error,
                });
            } else {
                validated_targets.push(target);
            }
        }
        let computed_hash = original_payload.planning_metadata().computed_hash.clone();
        let mut targets = Vec::with_capacity(validated_targets.len());
        let mut units = Vec::with_capacity(validated_targets.len());
        for target in validated_targets {
            match build_copy_unit(
                request,
                &target,
                &original_payload,
                eve_payload.as_ref(),
                source_lock_entry,
                &computed_hash,
            ) {
                Ok(unit) => {
                    targets.push(target);
                    units.push(unit);
                }
                Err(error) => planning_failures.push(CopyPlanningFailure {
                    context: target.context,
                    error,
                }),
            }
        }
        let revisions = aggregate_copy_revisions(&source.revisions, &targets)?;
        let observed_state_digest = stable_digest(&(
            &request.skill_name,
            &original_payload.manifest().payload_id(),
            &original_payload.manifest().payload_root_hash,
            source_lock_entry,
            (
                &source.project_identity.key,
                &source.project_identity.fingerprint,
            ),
            targets
                .iter()
                .map(|target| {
                    (
                        &target.context,
                        target
                            .target_facts
                            .iter()
                            .map(|fact| (&fact.key, &fact.fingerprint))
                            .collect::<Vec<_>>(),
                        target.comparison.physical_identity,
                        target.library_candidates.evidence_digest(),
                        target.library_candidates.selected_agent_ids(),
                    )
                })
                .collect::<Vec<_>>(),
            planning_failures
                .iter()
                .map(|failure| (&failure.context, failure.error_report().code))
                .collect::<Vec<_>>(),
        ))?;
        let token = issue_preview_token(PreviewTokenDraft {
            kind: MutationKind::Copy,
            request,
            revisions,
            observed_state_digest,
            planner_contract_version: 3,
        })?;
        let mut previews = targets
            .iter()
            .map(copy_target_preview)
            .collect::<Result<Vec<_>, _>>()?;
        previews.extend(planning_failures.iter().map(CopyPlanningFailure::preview));
        previews.sort_by_key(|preview| {
            request
                .target_project_ids
                .iter()
                .position(|project_id| project_id == &preview.project_id)
                .unwrap_or(usize::MAX)
        });
        let plan = if include_plan {
            let mut payloads = vec![original_payload];
            if let Some(eve) = eve_payload {
                payloads.push(eve);
            }
            Some(assemble_plan(MutationPlanDraft {
                kind: MutationKind::Copy,
                payloads: payloads
                    .into_iter()
                    .map(|payload| (payload.manifest().payload_id().clone(), payload))
                    .collect::<BTreeMap<PayloadId, PinnedPayloadLease>>(),
                units,
            }))
        } else {
            None
        };
        Ok(BuiltCopy {
            token,
            previews,
            plan,
            planning_failures,
        })
    }
}

struct LoadedCopySource {
    facts: ScopePlanningSnapshot,
    catalog: crate::application::agent_selection::AgentSelectionCatalog,
    selection: AgentSelectionSnapshot,
    plan: crate::application::scope_skill_planning::ScopeSkillPlan,
    observed: ResolvedScopeSkillPlacements,
}

struct BuiltCopy {
    token: PreviewToken,
    previews: Vec<CopyTargetPreview>,
    plan: Option<MutationPlan>,
    planning_failures: Vec<CopyPlanningFailure>,
}

struct CopyPlanningFailure {
    context: SkillLocationRef,
    error: AppError,
}

impl CopyPlanningFailure {
    fn error_report(&self) -> ErrorReport {
        ErrorReport::from_app_error(self.error.clone(), Some(self.context.clone()))
    }

    fn preview(&self) -> CopyTargetPreview {
        let project_id = project_id(&self.context).unwrap_or_default().to_string();
        CopyTargetPreview {
            project_id: project_id.clone(),
            display_name: project_id,
            storage_access: StorageAccess::Unknown,
            physical_identity: PhysicalIdentityComparison::Unknown,
            agent_targets: Vec::new(),
            fallback_forecasts: Vec::new(),
            blocking_reasons: vec![self.error_report().code],
        }
    }

    fn result(&self, request: &CopyRequest) -> MutationUnitResult {
        let unit_id = format!(
            "copy:{}:{}",
            request.skill_name,
            project_id(&self.context).unwrap_or_default()
        );
        let mut error = self.error_report();
        error.unit_id = Some(unit_id.clone());
        let retryable = error.retryable;
        MutationUnitResult {
            unit_id,
            skill_name: request.skill_name.clone(),
            source: Some(request.source.clone()),
            target: self.context.clone(),
            status: MutationUnitStatus::Failed,
            retryable,
            lock_committed: false,
            actual_mode: None,
            fallback_reason: None,
            agent_targets: Vec::new(),
            warnings: Vec::new(),
            error: Some(error),
            recovery: None,
        }
    }
}

struct BuiltCopyTarget {
    context: SkillLocationRef,
    facts: ScopePlanningSnapshot,
    entry_modes: BTreeMap<String, InstallMode>,
    target_facts: Vec<ResolvedTargetFact>,
    comparison: ProjectComparison,
    library_candidates: LibraryCandidateSnapshot,
    catalog: crate::application::agent_selection::AgentSelectionCatalog,
    observed: ResolvedScopeSkillPlacements,
    selected_option_ids: Vec<AgentInstallOptionId>,
}

fn copy_entry_modes(
    catalog: &crate::application::agent_selection::AgentSelectionCatalog,
    states: &[CopyObservedOptionState],
    selected_option_ids: &[AgentInstallOptionId],
    requested_mode: &InstallMode,
) -> BTreeMap<String, InstallMode> {
    let states_by_id = states
        .iter()
        .map(|state| (&state.option_id, state))
        .collect::<BTreeMap<_, _>>();
    selected_option_ids
        .iter()
        .filter_map(|option_id| {
            let option = catalog.option(option_id)?;
            let mode = if option.public.mode_constraint
                == crate::application::agent_selection::AgentSelectionModeConstraint::CopyOnly
            {
                InstallMode::Copy
            } else {
                match states_by_id.get(option_id).map(|state| state.current_entry) {
                    Some(CopyCurrentEntry::Link) => InstallMode::Symlink,
                    Some(CopyCurrentEntry::Copy) => InstallMode::Copy,
                    _ => requested_mode.clone(),
                }
            };
            Some((option.target_id(), mode))
        })
        .collect()
}

#[derive(Debug, Clone, Copy, Serialize)]
enum CopyCurrentEntry {
    Link,
    Copy,
}

struct CopyObservedOptionState {
    option_id: AgentInstallOptionId,
    current_entry: CopyCurrentEntry,
    initial_selected: bool,
}

fn copy_observed_option_states(
    catalog: &crate::application::agent_selection::AgentSelectionCatalog,
    observed: &ResolvedScopeSkillPlacements,
    library_candidates: &LibraryCandidateSnapshot,
) -> Result<
    (
        Vec<CopyObservedOptionState>,
        crate::application::scope_skill_planning::ScopeSkillPlan,
    ),
    AppError,
> {
    let plan = ScopeSkillPlanner::plan_direct_change(DirectSkillChangeRequest {
        skill: SkillDirectoryName::try_from(observed.resolved.skill_name.as_str())?,
        catalog,
        placements: observed.placements.clone(),
        libraries: LibraryElectionState {
            candidates: library_candidates.candidates(),
            selected_agent_ids: library_candidates.selected_agent_ids(),
        },
        direct_changes: BTreeMap::new(),
    })
    .map_err(|error| error.into_app_error())?;
    let states = catalog
        .options()
        .filter_map(|option| {
            let placement_id = DirectoryPlacementId::Option(option.public.id.clone());
            let directory = plan.directories().iter().find(|directory| {
                directory
                    .placements()
                    .contains(&DirectoryPlacementRef::Catalog(placement_id.clone()))
            })?;
            let fact = directory.fact();
            let initial_selected = matches!(directory.observed(), ObservedVersion::Direct);
            let current_entry = match fact.entry_kind {
                TargetEntryKind::Directory => CopyCurrentEntry::Copy,
                TargetEntryKind::Symlink
                | TargetEntryKind::Junction
                | TargetEntryKind::BrokenLink => CopyCurrentEntry::Link,
                TargetEntryKind::Missing | TargetEntryKind::File | TargetEntryKind::Other => {
                    return None;
                }
            };
            Some(CopyObservedOptionState {
                option_id: option.public.id.clone(),
                current_entry,
                initial_selected,
            })
        })
        .collect::<Vec<_>>();
    Ok((states, plan))
}

fn copy_observed_selection(
    catalog: &crate::application::agent_selection::AgentSelectionCatalog,
    states: &[CopyObservedOptionState],
    library_candidates: &LibraryCandidateSnapshot,
) -> Result<AgentSelectionSnapshot, AppError> {
    let initial_selected_option_ids = states
        .iter()
        .filter(|state| state.initial_selected)
        .map(|state| state.option_id.clone())
        .collect();
    let revision = crate::application::agent_selection::AgentSelectionRevision(stable_digest(&(
        "copy-agent-selection-revision-v1",
        &catalog.snapshot().revision,
        library_candidates.evidence_digest(),
        library_candidates.selected_agent_ids(),
        states
            .iter()
            .map(|state| {
                (
                    &state.option_id,
                    state.current_entry,
                    state.initial_selected,
                )
            })
            .collect::<Vec<_>>(),
    ))?);
    let mut snapshot = catalog.snapshot().clone();
    snapshot.initial_selected_option_ids = initial_selected_option_ids;
    snapshot.revision = revision;
    Ok(snapshot)
}

fn standard_reader_agent_ids(
    catalog: &crate::application::agent_selection::AgentSelectionCatalog,
) -> Vec<AgentId> {
    catalog
        .snapshot()
        .agents
        .iter()
        .filter(|agent| {
            matches!(
                agent.directory_access,
                Some(SkillDirectoryAccess::StandardOnly | SkillDirectoryAccess::Both)
            )
        })
        .map(|agent| agent.id.clone())
        .collect()
}

fn copy_selection_snapshot(mut selection: AgentSelectionSnapshot) -> AgentSelectionSnapshot {
    selection.user_mode_option_ids = selection
        .install_options
        .iter()
        .filter(|option| {
            option.selectable
                && option.mode_constraint == AgentSelectionModeConstraint::UserSelectable
        })
        .map(|option| option.id.clone())
        .collect();
    selection
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NormalizedCopyMetadata {
    pub source: String,
    pub source_type: String,
    pub source_url: Option<String>,
    pub ref_name: Option<String>,
    pub skill_path: Option<String>,
    pub upstream_revision: Option<String>,
    pub plugin_name: Option<String>,
    pub source_base_url: Option<String>,
    pub well_known_digest: Option<String>,
}

pub fn validate_copy_request(request: &CopyRequest) -> Result<(), AppError> {
    if request.skill_name.trim().is_empty()
        || request.target_project_ids.is_empty()
        || !matches!(
            request.source.scope,
            crate::environment::types::SkillLocation::Project { .. }
        )
    {
        return Err(validation("Skill and target projects are required"));
    }
    let mut projects = BTreeSet::new();
    if request
        .target_project_ids
        .iter()
        .any(|project| project.trim().is_empty() || !projects.insert(project))
    {
        return Err(validation("invalid or duplicate target project"));
    }
    Ok(())
}

fn project_context(environment: &EnvironmentRef, project_id: &str) -> SkillLocationRef {
    SkillLocationRef {
        environment: environment.clone(),
        scope: crate::environment::types::SkillLocation::Project {
            project_id: project_id.to_string(),
        },
    }
}

fn aggregate_copy_revisions(
    source: &RuntimeRevisions,
    targets: &[BuiltCopyTarget],
) -> Result<RuntimeRevisions, AppError> {
    let all = std::iter::once(source)
        .chain(targets.iter().map(|target| &target.facts.revisions))
        .collect::<Vec<_>>();
    let registry = stable_digest(
        &all.iter()
            .map(|revision| &revision.registry)
            .collect::<Vec<_>>(),
    )?;
    let environment = stable_digest(
        &all.iter()
            .map(|revision| &revision.environment)
            .collect::<Vec<_>>(),
    )?;
    let context_digest = stable_digest(
        &all.iter()
            .map(|revision| revision.context.as_str())
            .collect::<Vec<_>>(),
    )?;
    Ok(RuntimeRevisions {
        registry,
        environment,
        context: ContextSnapshotRevision::parse(format!(
            "context-copy-{}",
            context_digest.trim_start_matches("digest-v1-")
        ))?,
    })
}

fn validate_copy_source_snapshot(
    snapshot: &CopySourceSnapshot,
    request: &CopyRequest,
) -> Result<(), AppError> {
    if snapshot.skill_name != request.skill_name
        || snapshot.source_context.scope != request.source.scope
        || !same_environment_identity(
            &snapshot.source_context.environment,
            &request.source.environment,
        )
    {
        return Err(AppError::StalePayload);
    }
    Ok(())
}

async fn validate_copy_payload_targets(
    canonical: &crate::core::skill_payload::SkillPayload,
    eve_payload: Option<&PinnedPayloadLease>,
    target: &BuiltCopyTarget,
) -> Result<(), AppError> {
    if target.target_facts.len() != 1 + target.selected_option_ids.len() {
        return Err(AppError::StaleTarget);
    }
    let eve = match eve_payload {
        Some(payload) => Some(payload.load_payload().await?),
        None => None,
    };
    for (index, fact) in target.target_facts.iter().enumerate() {
        let eve_target = index > 0
            && target
                .catalog
                .option(&target.selected_option_ids[index - 1])
                .is_some_and(|option| option.placement.content.uses_eve_payload());
        let payload = if eve_target {
            eve.as_ref().ok_or(AppError::StalePayload)?
        } else {
            canonical
        };
        let profile = match fact.key.backend {
            ExecutionBackend::NativeWindows => TargetPathProfile::native_windows(),
            ExecutionBackend::NativeUnix | ExecutionBackend::WslPosix { .. } => {
                TargetPathProfile::native_unix()
            }
        };
        validate_manifest_for_target(payload, &fact.destination, &profile)?;
    }
    Ok(())
}

fn copy_target_preview(target: &BuiltCopyTarget) -> Result<CopyTargetPreview, AppError> {
    let project = target
        .facts
        .resolved_context
        .project
        .as_ref()
        .ok_or(AppError::StaleContext)?;
    let mut agent_targets = Vec::new();
    for agent_id in standard_reader_agent_ids(&target.catalog) {
        let agent = target
            .facts
            .agent_runtime
            .agents
            .get(&agent_id)
            .ok_or(AppError::StaleRegistry)?;
        agent_targets.push(AgentTargetPreview {
            agent_id,
            target_id: "canonical".to_string(),
            display_path: target.target_facts[0].destination.clone(),
            own_directory_selected: false,
            availability: agent.detection,
            blocking_reason: None,
        });
    }
    for (option_id, fact) in target
        .selected_option_ids
        .iter()
        .zip(&target.target_facts[1..])
    {
        let option = target
            .catalog
            .option(option_id)
            .ok_or(AppError::StaleTarget)?;
        for agent_id in &option.public.agent_ids {
            let agent = target
                .facts
                .agent_runtime
                .agents
                .get(agent_id)
                .ok_or(AppError::StaleRegistry)?;
            agent_targets.push(AgentTargetPreview {
                agent_id: agent_id.clone(),
                target_id: option.target_id(),
                display_path: fact.destination.clone(),
                own_directory_selected: true,
                availability: agent.detection,
                blocking_reason: None,
            });
        }
    }
    let blocking_reasons = (target.comparison.physical_identity
        == PhysicalIdentityComparison::Same)
        .then_some(OperationErrorCode::SelfCopy)
        .into_iter()
        .collect();
    Ok(CopyTargetPreview {
        project_id: project.id.clone(),
        display_name: project
            .display_name
            .clone()
            .unwrap_or_else(|| project.native_path.clone()),
        storage_access: target.comparison.target_storage_access,
        physical_identity: target.comparison.physical_identity,
        agent_targets,
        fallback_forecasts: Vec::new(),
        blocking_reasons,
    })
}

fn build_copy_unit(
    request: &CopyRequest,
    target: &BuiltCopyTarget,
    original_payload: &PinnedPayloadLease,
    eve_payload: Option<&PinnedPayloadLease>,
    source_lock_entry: Option<&Value>,
    computed_hash: &str,
) -> Result<MutationUnitDraft, AppError> {
    let canonical_id = original_payload.manifest().payload_id().clone();
    let selected = target
        .selected_option_ids
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    let mut direct_changes = BTreeMap::new();
    for placement_id in target.observed.placements.facts().keys() {
        let prepared = match placement_id {
            DirectoryPlacementId::Standard => Some((canonical_id.clone(), InstallMode::Copy)),
            DirectoryPlacementId::Option(option_id) if selected.contains(option_id) => {
                let option = target
                    .catalog
                    .option(option_id)
                    .ok_or(AppError::StaleTarget)?;
                let eve = option.placement.content.uses_eve_payload();
                Some((
                    if eve {
                        eve_payload
                            .ok_or(AppError::StalePayload)?
                            .manifest()
                            .payload_id()
                            .clone()
                    } else {
                        canonical_id.clone()
                    },
                    if eve {
                        InstallMode::Copy
                    } else {
                        target
                            .entry_modes
                            .get(&option.target_id())
                            .cloned()
                            .unwrap_or_else(|| request.agent_selection.requested_mode.clone())
                    },
                ))
            }
            DirectoryPlacementId::Option(_) => None,
        };
        direct_changes.insert(
            placement_id.clone(),
            prepared.map_or(
                DirectPlacementChange::Preserve,
                |(payload_id, requested_mode)| {
                    DirectPlacementChange::Set(PreparedDirectVersion::new(
                        DirectContentIdentity::Payload(payload_id.clone()),
                        PreparedEntryAction::Replace {
                            payload_id,
                            requested_mode,
                        },
                    ))
                },
            ),
        );
    }
    let entries = ScopeSkillPlanner::plan_direct_change(DirectSkillChangeRequest {
        skill: SkillDirectoryName::try_from(request.skill_name.as_str())?,
        catalog: &target.catalog,
        placements: target.observed.placements.clone(),
        libraries: LibraryElectionState {
            candidates: target.library_candidates.candidates(),
            selected_agent_ids: target.library_candidates.selected_agent_ids(),
        },
        direct_changes,
    })
    .map(|plan| plan.compile_entries())
    .map_err(|error| error.into_app_error())?;
    let eve_target_ids = target
        .selected_option_ids
        .iter()
        .filter_map(|id| target.catalog.option(id))
        .filter(|option| option.placement.content.uses_eve_payload())
        .flat_map(|option| option.adapter_target_ids.iter().cloned())
        .collect::<Vec<_>>();
    let resolved = crate::application::installed_skill_resolver::InstalledSkillResolver::resolve(
        &request.skill_name,
        &target.facts.lock_document,
    )?;
    let replacement = project_lock_replacement_at(
        source_lock_entry,
        computed_hash,
        &eve_target_ids,
        target
            .facts
            .resolved_context
            .project
            .as_ref()
            .map(|project| project.native_path.as_str()),
    );
    let lock_mutation = PreparedLockMutation {
        target: target.facts.resolved_context.lock.clone(),
        legacy_target: None,
        schema: target.facts.lock_schema,
        entry: match replacement {
            Some(replacement) if resolved.requires_lock_key_migration() => {
                LockEntryMutation::MoveAndReplace {
                    from: resolved.lock_key.clone(),
                    to: resolved.skill_name.clone(),
                    replacement,
                }
            }
            Some(replacement) => LockEntryMutation::Replace {
                key: resolved.lock_key.clone(),
                replacement,
            },
            None => LockEntryMutation::Remove {
                key: resolved.lock_key.clone(),
            },
        },
        root_replacements: BTreeMap::new(),
        expected: LockExpectedState::capture(
            &target.facts.lock_document,
            [resolved.lock_key.as_str(), resolved.skill_name.as_str()],
            std::iter::empty::<&str>(),
        ),
    };
    Ok(MutationUnitDraft {
        id: format!(
            "copy:{}:{}",
            request.skill_name,
            project_id(&target.context)?
        ),
        skill_name: request.skill_name.clone(),
        source: Some(request.source.clone()),
        target: target.context.clone(),
        expected_revisions: target.facts.revisions.clone(),
        entries,
        lock_mutation: Some(lock_mutation),
    })
}

fn project_id(context: &SkillLocationRef) -> Result<&str, AppError> {
    match &context.scope {
        crate::environment::types::SkillLocation::Project { project_id } => Ok(project_id),
        crate::environment::types::SkillLocation::Global => Err(AppError::StaleContext),
    }
}

fn validation(message: &str) -> AppError {
    AppError::Validation {
        field: Some("request".to_string()),
        message: message.to_string(),
    }
}

pub(crate) fn compare_resolved_projects(
    left: &crate::environment::planning::ResolvedTargetFact,
    right: &crate::environment::planning::ResolvedTargetFact,
) -> Result<PhysicalIdentityComparison, AppError> {
    if left.key.backend != right.key.backend
        || !crate::environment::types::same_environment_identity(
            &left.destination.environment,
            &right.destination.environment,
        )
    {
        return Ok(PhysicalIdentityComparison::Unknown);
    }
    if left.key == right.key {
        return Ok(PhysicalIdentityComparison::Same);
    }
    let case_sensitive = !matches!(left.key.backend, ExecutionBackend::NativeWindows);
    if crate::environment::runtime::physical_paths_overlap(
        &left.destination,
        &right.destination,
        case_sensitive,
    )? {
        Ok(PhysicalIdentityComparison::Same)
    } else {
        Ok(PhysicalIdentityComparison::Different)
    }
}

fn normalize_copy_metadata(source: Option<&Value>) -> Option<NormalizedCopyMetadata> {
    let source = source?;
    if !source.is_object() {
        return None;
    }
    let text = |field: &str| {
        source
            .get(field)
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    };
    let source_value = text("source")?;
    let source_type = text("sourceType")?;
    Some(NormalizedCopyMetadata {
        source: source_value,
        source_type,
        source_url: text("sourceUrl"),
        ref_name: text("ref"),
        skill_path: text("skillPath"),
        upstream_revision: text("remoteHash").or_else(|| {
            matches!(
                text("sourceType").as_deref(),
                Some("github" | "gitlab" | "git")
            )
            .then(|| text("skillFolderHash"))
            .flatten()
        }),
        plugin_name: text("pluginName"),
        source_base_url: text("sourceBaseUrl"),
        well_known_digest: text("wellKnownDigest"),
    })
}

#[cfg(test)]
fn project_lock_replacement(
    source: Option<&Value>,
    computed_hash: &str,
    eve_target_ids: &[String],
) -> Option<Value> {
    project_lock_replacement_at(source, computed_hash, eve_target_ids, None)
}

fn project_lock_replacement_at(
    source: Option<&Value>,
    computed_hash: &str,
    eve_target_ids: &[String],
    project_root: Option<&str>,
) -> Option<Value> {
    let metadata = normalize_copy_metadata(source)?;
    let mut entry = Map::new();
    let is_well_known = metadata.source_type == "well-known";
    let source = if metadata.source_type == "local" {
        project_root
            .map(|root| {
                crate::core::portable_project_path::serialize_project_source(root, &metadata.source)
            })
            .unwrap_or(metadata.source)
    } else {
        metadata.source
    };
    entry.insert("source".to_string(), Value::String(source));
    entry.insert(
        "sourceType".to_string(),
        Value::String(metadata.source_type),
    );
    entry.insert(
        "sourceUrl".to_string(),
        serde_json::json!(if is_well_known {
            metadata.source_base_url.clone()
        } else {
            metadata.source_url.clone()
        }),
    );
    entry.insert("ref".to_string(), serde_json::json!(metadata.ref_name));
    entry.insert(
        "skillPath".to_string(),
        serde_json::json!(metadata.skill_path),
    );
    entry.insert(
        "computedHash".to_string(),
        Value::String(computed_hash.to_string()),
    );
    entry.insert(
        "pluginName".to_string(),
        serde_json::json!(metadata.plugin_name),
    );
    if let Some(remote_hash) = metadata.upstream_revision {
        entry.insert("remoteHash".to_string(), Value::String(remote_hash));
    }
    if let Some(digest) = metadata.well_known_digest {
        entry.insert("wellKnownDigest".to_string(), Value::String(digest));
    }
    let subagents = eve_target_ids
        .iter()
        .filter_map(|target| crate::core::eve::parse_eve_target_id(target))
        .map(|target| match target {
            crate::core::eve::EveTargetRef::Root => String::new(),
            crate::core::eve::EveTargetRef::Subagent(subagent) => subagent.to_string(),
        })
        .collect::<BTreeSet<_>>();
    if subagents.iter().any(|target| !target.is_empty()) {
        entry.insert("subagents".to_string(), serde_json::json!(subagents));
    }
    Some(Value::Object(entry))
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::fs;
    use std::sync::Mutex;

    use tempfile::tempdir;

    use super::*;
    use crate::application::install::InstallFuture;
    use crate::application::payload_session::PayloadSessionLimits;
    use crate::core::agent_definition::{
        AgentAdapter, AgentDefinition, AgentSource, DetectionSpec, PathSpec, ScopeDefinition,
    };
    use crate::core::lossless_lock::{LockSchema, LosslessLockDocument};
    use crate::environment::agent_environment::{
        AgentRuntimeSnapshot, DirectoryPresenceState, ResolvedAgent, ResolvedAgentScope,
    };
    use crate::environment::context_resolver::ResolvedContext;
    use crate::environment::native::acquire::NativePayloadSessionStorage;
    use crate::environment::planning::{ResolvedTargetFact, TargetEntryKind};
    use crate::environment::runtime::{
        EntryFingerprint, ExecutionBackend, PhysicalParentIdentity, PhysicalTargetKey,
    };
    use crate::environment::types::{
        EnvironmentRef, EnvironmentStatus, RegisteredProject, SkillLocation, SkillLocationRef,
    };
    use crate::environment::wsl::WslRuntime;
    use crate::models::InstallMode;

    #[derive(Clone)]
    struct DistinctAgentTargets;

    impl TargetFactResolver for DistinctAgentTargets {
        fn resolve<'a>(
            &'a self,
            _context: &'a SkillLocationRef,
            logical_destinations: &'a [ResourceLocator],
            _cancellation: Option<CancellationSignal>,
        ) -> crate::environment::planning::TargetFactFuture<
            'a,
            Result<Vec<ResolvedTargetFact>, AppError>,
        > {
            Box::pin(async move {
                Ok(logical_destinations
                    .iter()
                    .enumerate()
                    .map(|(index, destination)| ResolvedTargetFact {
                        key: PhysicalTargetKey {
                            backend: ExecutionBackend::NativeUnix,
                            physical_parent: PhysicalParentIdentity::Unix {
                                device: 1,
                                inode: 100 + index as u64,
                            },
                            normalized_final_child_name: format!("skills-{index}"),
                        },
                        destination: destination.clone(),
                        storage_access: crate::environment::types::StorageAccess::Native,
                        fingerprint: EntryFingerprint("missing".to_string()),
                        entry_kind: TargetEntryKind::Missing,
                        link_target: None,
                        link_target_identity: None,
                    })
                    .collect())
            })
        }
    }

    fn private_agent(id: &str, path: &str) -> (AgentId, ResolvedAgent) {
        let id = AgentId::parse(id).unwrap();
        let disabled_scope = ScopeDefinition {
            enabled: false,
            reads_standard: false,
            private_path: None,
        };
        let disabled_resolved_scope = ResolvedAgentScope {
            enabled: false,
            reads_standard: false,
            standard_path: None,
            private_path: None,
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
                    source: AgentSource::Builtin,
                    aliases: Vec::new(),
                    global: ScopeDefinition {
                        enabled: true,
                        reads_standard: false,
                        private_path: Some(PathSpec::home(path)),
                    },
                    project: disabled_scope,
                    detection: DetectionSpec::AnyPathExists {
                        paths: vec![PathSpec::home(".agent")],
                    },
                    legacy_paths: Vec::new(),
                    adapter: AgentAdapter::Standard,
                },
                detection: DetectionState::Detected,
                detection_reason: None,
                global: ResolvedAgentScope {
                    enabled: true,
                    reads_standard: false,
                    standard_path: None,
                    private_path: Some(path.to_string()),
                    read_paths: vec![path.to_string()],
                    standard_presence: Some(DirectoryPresenceState::Missing),
                    private_presence: Some(DirectoryPresenceState::Present),
                    legacy_paths: Vec::new(),
                },
                project: disabled_resolved_scope,
            },
        )
    }

    fn project_private_agent(id: &str, path: &str) -> (AgentId, ResolvedAgent) {
        let (id, mut agent) = private_agent(id, "/unused/global/skills");
        agent.definition.global = ScopeDefinition {
            enabled: false,
            reads_standard: false,
            private_path: None,
        };
        agent.global = ResolvedAgentScope {
            enabled: false,
            reads_standard: false,
            standard_path: None,
            private_path: None,
            read_paths: Vec::new(),
            standard_presence: None,
            private_presence: None,
            legacy_paths: Vec::new(),
        };
        agent.definition.project = ScopeDefinition {
            enabled: true,
            reads_standard: false,
            private_path: Some(PathSpec::project(path)),
        };
        agent.project = ResolvedAgentScope {
            enabled: true,
            reads_standard: false,
            standard_path: None,
            private_path: Some(path.to_string()),
            read_paths: vec![path.to_string()],
            standard_presence: None,
            private_presence: Some(DirectoryPresenceState::Present),
            legacy_paths: Vec::new(),
        };
        (id, agent)
    }

    fn test_selection(mode: InstallMode) -> AgentSelectionSubmission {
        AgentSelectionSubmission {
            revision: crate::application::agent_selection::AgentSelectionRevision(
                "test-selection".to_string(),
            ),
            selected_option_ids: Vec::new(),
            requested_mode: mode,
        }
    }

    #[test]
    fn copy_request_has_one_target_environment_and_unique_projects() {
        let request = CopyRequest {
            skill_name: "demo".to_string(),
            source: SkillLocationRef {
                environment: EnvironmentRef::Native,
                scope: SkillLocation::Global,
            },
            target_environment: EnvironmentRef::Wsl {
                distro_name: "Ubuntu".to_string(),
            },
            target_project_ids: vec!["project-1".to_string(), "project-1".to_string()],
            agent_selection: test_selection(InstallMode::Copy),
        };
        assert!(validate_copy_request(&request).is_err());
        let value = serde_json::to_value(request).unwrap();
        assert!(value.get("targetEnvironments").is_none());
        assert_eq!(value["targetEnvironment"]["kind"], "wsl");
    }

    #[test]
    fn copy_source_must_be_a_project_context() {
        let request = CopyRequest {
            skill_name: "demo".to_string(),
            source: SkillLocationRef {
                environment: EnvironmentRef::Native,
                scope: SkillLocation::Global,
            },
            target_environment: EnvironmentRef::Native,
            target_project_ids: vec!["project-1".to_string()],
            agent_selection: test_selection(InstallMode::Copy),
        };

        assert!(validate_copy_request(&request).is_err());
    }

    fn project_fact(path: &str, inode: u64, child: &str) -> ResolvedTargetFact {
        ResolvedTargetFact {
            key: PhysicalTargetKey {
                backend: ExecutionBackend::NativeUnix,
                physical_parent: PhysicalParentIdentity::Unix { device: 1, inode },
                normalized_final_child_name: child.to_string(),
            },
            destination: ResourceLocator {
                environment: EnvironmentRef::Native,
                native_path: path.to_string(),
            },
            storage_access: crate::environment::types::StorageAccess::Native,
            fingerprint: EntryFingerprint("entry-v1-project".to_string()),
            entry_kind: TargetEntryKind::Directory,
            link_target: None,
            link_target_identity: None,
        }
    }

    #[test]
    fn physical_project_comparison_blocks_equal_and_nested_but_allows_similar_paths() {
        let source = project_fact("/work/app", 1, "app");
        let same = project_fact("/work/app", 1, "app");
        let nested = project_fact("/work/app/examples/demo", 1, "app/examples/demo");
        let similar = project_fact("/work/application", 1, "application");

        assert_eq!(
            compare_resolved_projects(&source, &same).unwrap(),
            PhysicalIdentityComparison::Same
        );
        assert_eq!(
            compare_resolved_projects(&source, &nested).unwrap(),
            PhysicalIdentityComparison::Same
        );
        assert_eq!(
            compare_resolved_projects(&source, &similar).unwrap(),
            PhysicalIdentityComparison::Different
        );
    }

    #[test]
    fn target_lock_projects_source_metadata_without_inheriting_adapter_state() {
        let source = serde_json::json!({
            "source": "owner/repo",
            "sourceType": "github",
            "sourceUrl": "https://github.com/owner/repo.git",
            "ref": "main",
            "skillPath": "skills/demo",
            "computedHash": "source-local",
            "remoteHash": "upstream-version",
            "pluginName": "toolkit",
            "subagents": ["researcher"],
            "futureField": { "keepAtSourceOnly": true }
        });

        let target = project_lock_replacement(Some(&source), "target-computed", &[]).unwrap();

        assert_eq!(target["computedHash"], "target-computed");
        assert_eq!(target["remoteHash"], "upstream-version");
        assert_eq!(target["sourceUrl"], source["sourceUrl"]);
        assert_eq!(target["skillPath"], source["skillPath"]);
        assert_eq!(target["pluginName"], source["pluginName"]);
        assert!(target.get("subagents").is_none());
        assert!(target.get("futureField").is_none());
    }

    #[test]
    fn target_project_lock_maps_global_well_known_index_and_digest() {
        let source = serde_json::json!({
            "source": "skills.example.com",
            "sourceType": "well-known",
            "sourceUrl": "https://cdn.example.com/demo/SKILL.md",
            "sourceBaseUrl": "https://skills.example.com/catalog/index.json",
            "wellKnownDigest": "sha256:v2",
            "skillFolderHash": ""
        });

        let target = project_lock_replacement(Some(&source), "target-computed", &[]).unwrap();

        assert_eq!(target["sourceUrl"], source["sourceBaseUrl"]);
        assert_eq!(target["wellKnownDigest"], source["wellKnownDigest"]);
        assert_ne!(target["sourceUrl"], source["sourceUrl"]);
    }

    #[test]
    fn target_project_lock_relativizes_a_local_source() {
        let source = serde_json::json!({
            "source": "/work/shared/skills",
            "sourceType": "local",
            "skillFolderHash": "local-hash"
        });

        let target =
            project_lock_replacement_at(Some(&source), "target-computed", &[], Some("/work/app"))
                .unwrap();

        assert_eq!(target["source"], "../shared/skills");
    }

    #[test]
    fn target_lock_records_selected_eve_targets() {
        let source = serde_json::json!({
            "source": "owner/repo",
            "sourceType": "github"
        });

        let target = project_lock_replacement(
            Some(&source),
            "target-computed",
            &[
                "eve:root".to_string(),
                "eve:research".to_string(),
                "eve:review".to_string(),
            ],
        )
        .unwrap();

        assert_eq!(
            target["subagents"],
            serde_json::json!(["", "research", "review"])
        );
    }

    #[tokio::test]
    async fn copy_preserves_existing_entry_modes_and_uses_requested_mode_for_new_entries() {
        let context = SkillLocationRef {
            environment: EnvironmentRef::Native,
            scope: SkillLocation::Global,
        };
        let runtime = AgentRuntimeSnapshot {
            registry_revision: "registry-1".to_string(),
            environment_revision: "environment-1".to_string(),
            environment: EnvironmentRef::Native,
            availability: EnvironmentStatus::Available,
            project_path: None,
            agents: [
                private_agent("linked", "/agents/linked/skills"),
                private_agent("copied", "/agents/copied/skills"),
                private_agent("new", "/agents/new/skills"),
            ]
            .into_iter()
            .collect(),
        };
        let catalog = build_agent_selection_catalog(
            &context,
            &runtime,
            &[],
            &ResourceLocator {
                environment: EnvironmentRef::Native,
                native_path: "/skills".to_string(),
            },
            &DistinctAgentTargets,
        )
        .await
        .unwrap();
        let option_id_for = |agent_id: &str| {
            catalog
                .snapshot()
                .install_options
                .iter()
                .find(|option| option.agent_ids.iter().any(|id| id.as_str() == agent_id))
                .unwrap()
                .id
                .clone()
        };
        let linked = option_id_for("linked");
        let copied = option_id_for("copied");
        let new = option_id_for("new");
        let states = vec![
            CopyObservedOptionState {
                option_id: linked.clone(),
                current_entry: CopyCurrentEntry::Link,
                initial_selected: true,
            },
            CopyObservedOptionState {
                option_id: copied.clone(),
                current_entry: CopyCurrentEntry::Copy,
                initial_selected: true,
            },
        ];

        let modes = copy_entry_modes(
            &catalog,
            &states,
            &[linked.clone(), copied.clone(), new.clone()],
            &InstallMode::Copy,
        );

        assert_eq!(
            modes[&catalog.option(&linked).unwrap().target_id()],
            InstallMode::Symlink
        );
        assert_eq!(
            modes[&catalog.option(&copied).unwrap().target_id()],
            InstallMode::Copy
        );
        assert_eq!(
            modes[&catalog.option(&new).unwrap().target_id()],
            InstallMode::Copy
        );
    }

    #[test]
    fn copying_a_global_local_source_does_not_invent_project_remote_hash() {
        let source = serde_json::json!({
            "source": "/home/alice/skills",
            "sourceType": "local",
            "skillPath": "skills/demo",
            "skillFolderHash": "local-content-sha256"
        });

        let target = project_lock_replacement(Some(&source), "target-computed", &[]).unwrap();

        assert_eq!(target["computedHash"], "target-computed");
        assert!(target.get("remoteHash").is_none());
    }

    #[test]
    fn copying_without_source_lock_metadata_removes_target_lock_entry() {
        assert!(project_lock_replacement(None, "target-computed", &[]).is_none());
    }

    #[test]
    fn invalid_source_lock_metadata_is_treated_as_missing() {
        let source = serde_json::json!({"source": 42, "sourceType": "github"});

        assert!(project_lock_replacement(Some(&source), "target-computed", &[]).is_none());
    }

    #[derive(Clone)]
    struct Facts(Arc<Mutex<HashMap<SkillLocationRef, ScopePlanningSnapshot>>>);

    impl ScopePlanningSnapshotSource for Facts {
        fn snapshot<'a>(
            &'a self,
            context: &'a SkillLocationRef,
        ) -> InstallFuture<'a, Result<ScopePlanningSnapshot, AppError>> {
            Box::pin(async move {
                self.0
                    .lock()
                    .unwrap()
                    .get(context)
                    .cloned()
                    .ok_or(AppError::StaleContext)
            })
        }
    }

    #[cfg(unix)]
    #[derive(Clone)]
    struct CountingFacts {
        inner: Facts,
        calls: Arc<Mutex<Vec<SkillLocationRef>>>,
    }

    #[cfg(unix)]
    impl ScopePlanningSnapshotSource for CountingFacts {
        fn snapshot<'a>(
            &'a self,
            context: &'a SkillLocationRef,
        ) -> InstallFuture<'a, Result<ScopePlanningSnapshot, AppError>> {
            self.calls.lock().unwrap().push(context.clone());
            self.inner.snapshot(context)
        }
    }

    #[cfg(unix)]
    struct FixedLibraryCandidates(HashMap<SkillLocationRef, LibraryCandidateSnapshot>);

    #[cfg(unix)]
    impl LibraryCandidateSource for FixedLibraryCandidates {
        fn load_candidates<'a>(
            &'a self,
            context: &'a SkillLocationRef,
            _skill_name: &'a SkillDirectoryName,
        ) -> crate::application::library_candidates::LibraryCandidateFuture<
            'a,
            Result<LibraryCandidateSnapshot, AppError>,
        > {
            Box::pin(async move { self.0.get(context).cloned().ok_or(AppError::StaleContext) })
        }
    }

    #[derive(Clone)]
    struct FailingTargetFacts {
        facts: Facts,
        failed_context: SkillLocationRef,
    }

    impl ScopePlanningSnapshotSource for FailingTargetFacts {
        fn snapshot<'a>(
            &'a self,
            context: &'a SkillLocationRef,
        ) -> InstallFuture<'a, Result<ScopePlanningSnapshot, AppError>> {
            if context == &self.failed_context {
                return Box::pin(async {
                    Err(AppError::ConfigurationCorrupted {
                        message: "target project lock is corrupted".to_string(),
                    })
                });
            }
            self.facts.snapshot(context)
        }
    }

    struct DifferentProjects;

    impl CopyProjectComparator for DifferentProjects {
        fn capture_source<'a>(
            &'a self,
            source: &'a ScopePlanningSnapshot,
        ) -> CopyFuture<'a, Result<ResolvedTargetFact, AppError>> {
            Box::pin(async move {
                let project = source
                    .resolved_context
                    .project
                    .as_ref()
                    .ok_or(AppError::StaleContext)?;
                Ok(project_fact(&project.native_path, 1, "source"))
            })
        }

        fn compare<'a>(
            &'a self,
            _source: &'a ResolvedTargetFact,
            _target: &'a ScopePlanningSnapshot,
        ) -> CopyFuture<'a, Result<ProjectComparison, AppError>> {
            Box::pin(async {
                Ok(ProjectComparison {
                    physical_identity: PhysicalIdentityComparison::Different,
                    target_storage_access: StorageAccess::Native,
                })
            })
        }
    }

    #[derive(Debug)]
    struct CapturedCopyPlan {
        unit_count: usize,
        payload_paths: Vec<String>,
        has_lock_mutations: Vec<bool>,
        lock_replacements: Vec<Option<Value>>,
        expected_lock_entries: Vec<Option<Value>>,
        remote_hashes: Vec<Option<String>>,
        #[cfg(unix)]
        agent_actions: Vec<Vec<PreparedEntryAction>>,
    }

    struct CapturingExecutor(Arc<Mutex<Option<CapturedCopyPlan>>>);

    impl MutationPlanExecutor for CapturingExecutor {
        fn execute<'a>(
            &'a self,
            plan: MutationPlan,
            _cancellation: CancellationSignal,
        ) -> InstallFuture<'a, Vec<MutationUnitResult>> {
            Box::pin(async move {
                let payload = plan
                    .payloads
                    .values()
                    .next()
                    .expect("copy payload")
                    .load_payload()
                    .await
                    .expect("load payload");
                let payload_paths = payload
                    .manifest()
                    .entries
                    .iter()
                    .map(|entry| entry.relative_path.clone())
                    .collect();
                let lock_replacements = plan
                    .units
                    .iter()
                    .map(|unit| {
                        unit.lock_mutation
                            .as_ref()
                            .and_then(|lock| lock.replacement().cloned())
                    })
                    .collect::<Vec<_>>();
                let has_lock_mutations = plan
                    .units
                    .iter()
                    .map(|unit| unit.lock_mutation.is_some())
                    .collect();
                let expected_lock_entries = plan
                    .units
                    .iter()
                    .map(|unit| {
                        unit.lock_mutation.as_ref().and_then(|lock| {
                            lock.expected
                                .entry_snapshots
                                .get(&unit.skill_name)
                                .and_then(|entry| entry.value().cloned())
                        })
                    })
                    .collect();
                let remote_hashes = lock_replacements
                    .iter()
                    .map(|replacement| {
                        replacement
                            .as_ref()
                            .and_then(|entry| entry.get("remoteHash"))
                            .and_then(Value::as_str)
                            .map(str::to_string)
                    })
                    .collect();
                #[cfg(unix)]
                let agent_actions = plan
                    .units
                    .iter()
                    .map(|unit| {
                        unit.additional_entries
                            .iter()
                            .map(|entry| entry.action.clone())
                            .collect()
                    })
                    .collect();
                *self.0.lock().unwrap() = Some(CapturedCopyPlan {
                    unit_count: plan.units.len(),
                    payload_paths,
                    has_lock_mutations,
                    lock_replacements,
                    expected_lock_entries,
                    remote_hashes,
                    #[cfg(unix)]
                    agent_actions,
                });
                Vec::new()
            })
        }
    }

    fn context(environment: EnvironmentRef, project_id: &str) -> SkillLocationRef {
        SkillLocationRef {
            environment,
            scope: SkillLocation::Project {
                project_id: project_id.to_string(),
            },
        }
    }

    fn planning_facts(
        context: SkillLocationRef,
        root: &std::path::Path,
        source_lock: bool,
    ) -> ScopePlanningSnapshot {
        let project_id = project_id(&context).unwrap().to_string();
        let lock_document = if source_lock {
            LosslessLockDocument::parse(
                br#"{"version":1,"skills":{"demo":{"source":"owner/repo","sourceType":"github","sourceUrl":"https://github.com/owner/repo.git","ref":"main","skillPath":"skills/demo","computedHash":"old-local","remoteHash":"remote-v1","pluginName":"toolkit"}}}"#,
            )
            .unwrap()
        } else {
            LosslessLockDocument::empty(LockSchema::Project)
        };
        let project = RegisteredProject {
            id: project_id.clone(),
            native_path: root.to_string_lossy().into_owned(),
            display_name: Some(project_id.clone()),
            order: None,
            suppress_cross_storage_warning: false,
        };
        ScopePlanningSnapshot {
            resolved_context: ResolvedContext {
                context: context.clone(),
                project: Some(project),
                home: ResourceLocator {
                    environment: context.environment.clone(),
                    native_path: root.to_string_lossy().into_owned(),
                },
                skill_root: ResourceLocator {
                    environment: context.environment.clone(),
                    native_path: root.join(".agents/skills").to_string_lossy().into_owned(),
                },
                lock: ResourceLocator {
                    environment: context.environment.clone(),
                    native_path: root.join("skills-lock.json").to_string_lossy().into_owned(),
                },
            },
            agent_runtime: AgentRuntimeSnapshot {
                registry_revision: "registry-1".to_string(),
                environment_revision: "environment-1".to_string(),
                environment: context.environment.clone(),
                availability: EnvironmentStatus::Available,
                project_path: Some(root.to_string_lossy().into_owned()),
                agents: BTreeMap::new(),
            },
            revisions: RuntimeRevisions {
                registry: "registry-1".to_string(),
                environment: "environment-1".to_string(),
                context: ContextSnapshotRevision::parse(format!("context-{project_id}")).unwrap(),
            },
            lock_schema: LockSchema::Project,
            lock_document,
            eve_targets: Vec::new(),
        }
    }

    async fn preview_with_source_lock(
        source_lock: LosslessLockDocument,
        target_project_ids: Vec<String>,
        stale_selection: bool,
    ) -> Result<CopyPreviewOutcome, AppError> {
        let temp = tempdir().unwrap();
        let source_root = temp.path().join("source");
        let target_root = temp.path().join("target");
        let source_skill = source_root.join(".agents/skills/demo");
        fs::create_dir_all(&source_skill).unwrap();
        fs::write(source_skill.join("SKILL.md"), b"---\nname: demo\n---\nbody").unwrap();
        fs::create_dir_all(&target_root).unwrap();

        let source = context(EnvironmentRef::Native, "source");
        let target = context(EnvironmentRef::Native, "target");
        let mut source_facts = planning_facts(source.clone(), &source_root, false);
        source_facts.lock_document = source_lock;
        let facts = Arc::new(Mutex::new(HashMap::from([
            (source.clone(), source_facts),
            (target.clone(), planning_facts(target, &target_root, false)),
        ])));
        let environments = Arc::new(WslRuntime::default());
        let storage =
            Arc::new(NativePayloadSessionStorage::new(temp.path().join("payloads")).unwrap());
        let payloads = Arc::new(PayloadSessionManager::new(
            storage,
            PayloadSessionLimits {
                ttl_ms: 60_000,
                max_sessions: 8,
                max_bytes: 1024 * 1024,
            },
            || 1_000,
        ));
        let service = CopyService::new(
            Facts(facts),
            crate::environment::planning::RuntimeTargetFactResolver::new(environments.clone()),
            payloads.clone(),
            InstalledSkillPayloadAcquirer::new(payloads, environments),
            CapturingExecutor(Arc::new(Mutex::new(None))),
            DifferentProjects,
            Arc::new(EmptyLibraryCandidateSource),
        );
        let mut agent_selection = service.selection(&source, "demo").await?.selection;
        if stale_selection {
            agent_selection.revision = crate::application::agent_selection::AgentSelectionRevision(
                "stale-selection".to_string(),
            );
        }
        let request = CopyRequest {
            skill_name: "demo".to_string(),
            source,
            target_environment: EnvironmentRef::Native,
            target_project_ids,
            agent_selection: AgentSelectionSubmission {
                revision: agent_selection.revision,
                selected_option_ids: agent_selection.initial_selected_option_ids,
                requested_mode: InstallMode::Copy,
            },
        };

        service.preview(&request).await
    }

    #[tokio::test]
    async fn copy_without_an_applied_library_rejects_an_external_file_target() {
        let temp = tempdir().unwrap();
        let source_root = temp.path().join("source");
        let target_root = temp.path().join("target");
        let source_skill = source_root.join(".agents/skills/demo");
        fs::create_dir_all(&source_skill).unwrap();
        fs::write(source_skill.join("SKILL.md"), b"---\nname: demo\n---\nbody").unwrap();
        let target_skill_root = target_root.join(".agents/skills");
        fs::create_dir_all(&target_skill_root).unwrap();
        let external_target = target_skill_root.join("demo");
        fs::write(&external_target, b"external content").unwrap();

        let source = context(EnvironmentRef::Native, "source");
        let target = context(EnvironmentRef::Native, "target");
        let facts = Arc::new(Mutex::new(HashMap::from([
            (
                source.clone(),
                planning_facts(source.clone(), &source_root, true),
            ),
            (target.clone(), planning_facts(target, &target_root, false)),
        ])));
        let environments = Arc::new(WslRuntime::default());
        let storage =
            Arc::new(NativePayloadSessionStorage::new(temp.path().join("payloads")).unwrap());
        let payloads = Arc::new(PayloadSessionManager::new(
            storage,
            PayloadSessionLimits {
                ttl_ms: 60_000,
                max_sessions: 8,
                max_bytes: 1024 * 1024,
            },
            || 1_000,
        ));
        let service = CopyService::new(
            Facts(facts),
            crate::environment::planning::RuntimeTargetFactResolver::new(environments.clone()),
            payloads.clone(),
            InstalledSkillPayloadAcquirer::new(payloads, environments),
            CapturingExecutor(Arc::new(Mutex::new(None))),
            DifferentProjects,
            Arc::new(EmptyLibraryCandidateSource),
        );
        let agent_selection = service.selection(&source, "demo").await.unwrap().selection;
        let request = CopyRequest {
            skill_name: "demo".to_string(),
            source,
            target_environment: EnvironmentRef::Native,
            target_project_ids: vec!["target".to_string()],
            agent_selection: AgentSelectionSubmission {
                revision: agent_selection.revision,
                selected_option_ids: agent_selection.initial_selected_option_ids,
                requested_mode: InstallMode::Copy,
            },
        };
        let CopyPreviewOutcome::Ready { preview } = service.preview(&request).await.unwrap() else {
            panic!("fresh Agent selection should produce a copy preview");
        };

        let response = service
            .execute(
                &CopyExecutionRequest {
                    request,
                    token: preview.token,
                    payload: preview.payload,
                },
                CancellationSignal::default(),
            )
            .await
            .unwrap();

        let [unit] = response.units.as_slice() else {
            panic!("expected the external target to produce one failed copy unit");
        };
        let error = unit.error.as_ref().expect("copy failure should be located");
        assert_eq!(error.code, OperationErrorCode::SkillPlacementTargetConflict);
        assert_eq!(
            error.parameters.get("skillName").map(String::as_str),
            Some("demo")
        );
        assert_eq!(
            error.parameters.get("agentIds").map(String::as_str),
            Some("")
        );
        let reported_target = error
            .parameters
            .get("targetPath")
            .expect("typed conflict includes the normalized target path");
        assert_eq!(
            std::path::PathBuf::from(reported_target),
            fs::canonicalize(&external_target).expect("canonical external target")
        );
        assert_eq!(
            error.parameters.get("targetKind").map(String::as_str),
            Some("file")
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn copy_selection_does_not_inherit_a_non_winning_library_link_as_direct() {
        use std::os::unix::fs::symlink;

        let temp = tempdir().unwrap();
        let source_root = temp.path().join("source");
        let target_root = temp.path().join("target");
        let canonical_path = source_root.join(".agents/skills/demo");
        let agent_root = source_root.join(".custom/skills");
        let agent_path = agent_root.join("demo");
        let target_canonical_path = target_root.join(".agents/skills/demo");
        let target_agent_root = target_root.join(".custom/skills");
        let target_agent_path = target_agent_root.join("demo");
        let first_library = temp.path().join("libraries/first/skills/demo");
        let second_library = temp.path().join("libraries/second/skills/demo");
        fs::create_dir_all(&canonical_path).unwrap();
        fs::write(
            canonical_path.join("SKILL.md"),
            b"---\nname: demo\n---\nbody",
        )
        .unwrap();
        fs::create_dir_all(&agent_root).unwrap();
        fs::create_dir_all(&target_canonical_path).unwrap();
        fs::create_dir_all(&target_agent_root).unwrap();
        fs::create_dir_all(&first_library).unwrap();
        fs::create_dir_all(&second_library).unwrap();
        symlink(&second_library, &agent_path).unwrap();
        symlink(&second_library, &target_agent_path).unwrap();

        let source = context(EnvironmentRef::Native, "source");
        let target = context(EnvironmentRef::Native, "target");
        let mut source_facts = planning_facts(source.clone(), &source_root, true);
        let mut target_facts = planning_facts(target.clone(), &target_root, false);
        let (agent_id, agent) = project_private_agent("custom", &agent_root.to_string_lossy());
        let (_, target_agent) =
            project_private_agent("custom", &target_agent_root.to_string_lossy());
        source_facts
            .agent_runtime
            .agents
            .insert(agent_id.clone(), agent);
        target_facts
            .agent_runtime
            .agents
            .insert(agent_id.clone(), target_agent);
        let facts = Arc::new(Mutex::new(HashMap::from([
            (source.clone(), source_facts),
            (target.clone(), target_facts),
        ])));
        let environments = Arc::new(WslRuntime::default());
        let targets =
            crate::environment::planning::RuntimeTargetFactResolver::new(environments.clone());
        let first_locator = ResourceLocator {
            environment: EnvironmentRef::Native,
            native_path: first_library.to_string_lossy().into_owned(),
        };
        let second_locator = ResourceLocator {
            environment: EnvironmentRef::Native,
            native_path: second_library.to_string_lossy().into_owned(),
        };
        let candidates = vec![
            LibraryVersionCandidate::new(
                crate::application::skill_libraries::LibraryId::parse("first"),
                "demo",
                first_locator.clone(),
            ),
            LibraryVersionCandidate::new(
                crate::application::skill_libraries::LibraryId::parse("second"),
                "demo",
                second_locator,
            ),
        ];
        let candidate_set = LibraryCandidateSet::new(candidates.clone(), candidates).unwrap();
        let source_candidates = LibraryCandidateSnapshot::new(
            "library-evidence-1",
            vec![agent_id.clone()],
            candidate_set.clone(),
        )
        .unwrap();
        let target_candidates = LibraryCandidateSnapshot::new(
            "library-evidence-1",
            vec![agent_id.clone()],
            candidate_set,
        )
        .unwrap();
        let payloads = Arc::new(PayloadSessionManager::in_memory(
            PayloadSessionLimits {
                ttl_ms: 60_000,
                max_sessions: 8,
                max_bytes: 1024 * 1024,
            },
            || 1_000,
        ));
        let captured = Arc::new(Mutex::new(None));
        let calls = Arc::new(Mutex::new(Vec::new()));
        let service = CopyService::new(
            CountingFacts {
                inner: Facts(facts),
                calls: calls.clone(),
            },
            targets,
            payloads.clone(),
            InstalledSkillPayloadAcquirer::new(payloads, environments),
            CapturingExecutor(captured.clone()),
            DifferentProjects,
            Arc::new(FixedLibraryCandidates(HashMap::from([
                (source.clone(), source_candidates),
                (target.clone(), target_candidates),
            ]))),
        );

        let selection = service.selection(&source, "demo").await.unwrap().selection;
        let option = selection
            .install_options
            .iter()
            .find(|option| option.agent_ids.contains(&agent_id))
            .unwrap();

        assert!(!selection.initial_selected_option_ids.contains(&option.id));
        calls.lock().unwrap().clear();

        let request = CopyRequest {
            skill_name: "demo".to_string(),
            source,
            target_environment: EnvironmentRef::Native,
            target_project_ids: vec!["target".to_string()],
            agent_selection: AgentSelectionSubmission {
                revision: selection.revision,
                selected_option_ids: selection.initial_selected_option_ids,
                requested_mode: InstallMode::Copy,
            },
        };
        let CopyPreviewOutcome::Ready { preview } = service.preview(&request).await.unwrap() else {
            panic!("fresh source selection should produce a copy preview");
        };
        assert_eq!(
            calls
                .lock()
                .unwrap()
                .iter()
                .filter(|context| *context == &target)
                .count(),
            1
        );
        service
            .execute(
                &CopyExecutionRequest {
                    request,
                    token: preview.token,
                    payload: preview.payload,
                },
                CancellationSignal::default(),
            )
            .await
            .unwrap();
        let captured = captured.lock().unwrap();
        let plan = captured.as_ref().unwrap();

        assert!(matches!(
            plan.agent_actions.as_slice(),
            [actions]
                if matches!(actions.as_slice(), [PreparedEntryAction::Link { target }]
                    if target == &first_locator)
        ));
    }

    #[tokio::test]
    async fn copy_preview_allows_copy_when_lock_entry_is_missing() {
        let outcome = preview_with_source_lock(
            LosslessLockDocument::empty(LockSchema::Project),
            vec!["target".to_string()],
            false,
        )
        .await
        .unwrap();

        assert!(matches!(outcome, CopyPreviewOutcome::Ready { .. }));
    }

    #[tokio::test]
    async fn copy_preview_allows_copy_when_lock_entry_is_invalid() {
        let outcome = preview_with_source_lock(
            LosslessLockDocument::parse(
                br#"{"version":1,"skills":{"demo":{"source":42,"sourceType":"github"}}}"#,
            )
            .unwrap(),
            vec!["target".to_string()],
            false,
        )
        .await
        .unwrap();

        assert!(matches!(outcome, CopyPreviewOutcome::Ready { .. }));
    }

    #[tokio::test]
    async fn copy_preview_keeps_non_source_failures_as_app_errors() {
        let error = preview_with_source_lock(
            LosslessLockDocument::parse(
                br#"{"version":1,"skills":{"demo":{"source":"owner/repo","sourceType":"github"}}}"#,
            )
            .unwrap(),
            Vec::new(),
            false,
        )
        .await
        .unwrap_err();

        assert!(matches!(error, AppError::Validation { .. }));
    }

    #[tokio::test]
    async fn copy_preview_allows_source_metadata_without_a_remote_hash() {
        let outcome = preview_with_source_lock(
            LosslessLockDocument::parse(
                br#"{"version":1,"skills":{"demo":{"source":"owner/repo","sourceType":"github","skillPath":"skills/demo"}}}"#,
            )
            .unwrap(),
            vec!["target".to_string()],
            false,
        )
        .await
        .unwrap();

        assert!(matches!(outcome, CopyPreviewOutcome::Ready { .. }));
    }

    #[tokio::test]
    async fn copy_preview_returns_a_fresh_snapshot_when_agent_selection_is_stale() {
        let outcome = preview_with_source_lock(
            LosslessLockDocument::parse(
                br#"{"version":1,"skills":{"demo":{"source":"owner/repo","sourceType":"github"}}}"#,
            )
            .unwrap(),
            vec!["target".to_string()],
            true,
        )
        .await
        .unwrap();

        match outcome {
            CopyPreviewOutcome::SelectionStale { snapshot } => {
                assert_ne!(snapshot.selection.revision.0, "stale-selection");
            }
            other => panic!("expected stale selection outcome, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn copy_preview_and_execution_scope_a_target_lock_failure_to_that_project() {
        let temp = tempdir().unwrap();
        let source_root = temp.path().join("source");
        let healthy_root = temp.path().join("healthy");
        let broken_root = temp.path().join("broken");
        let source_skill = source_root.join(".agents/skills/demo");
        fs::create_dir_all(&source_skill).unwrap();
        fs::write(source_skill.join("SKILL.md"), b"---\nname: demo\n---\nbody").unwrap();
        fs::create_dir_all(&healthy_root).unwrap();
        fs::create_dir_all(&broken_root).unwrap();

        let source = context(EnvironmentRef::Native, "source");
        let healthy = context(EnvironmentRef::Native, "healthy");
        let broken = context(EnvironmentRef::Native, "broken");
        let base_facts = Facts(Arc::new(Mutex::new(HashMap::from([
            (
                source.clone(),
                planning_facts(source.clone(), &source_root, true),
            ),
            (
                healthy.clone(),
                planning_facts(healthy.clone(), &healthy_root, false),
            ),
            (
                broken.clone(),
                planning_facts(broken.clone(), &broken_root, false),
            ),
        ]))));
        let facts = FailingTargetFacts {
            facts: base_facts,
            failed_context: broken,
        };
        let environments = Arc::new(WslRuntime::default());
        let storage =
            Arc::new(NativePayloadSessionStorage::new(temp.path().join("payloads")).unwrap());
        let payloads = Arc::new(PayloadSessionManager::new(
            storage,
            PayloadSessionLimits {
                ttl_ms: 60_000,
                max_sessions: 8,
                max_bytes: 1024 * 1024,
            },
            || 1_000,
        ));
        let captured = Arc::new(Mutex::new(None));
        let service = CopyService::new(
            facts,
            crate::environment::planning::RuntimeTargetFactResolver::new(environments.clone()),
            payloads.clone(),
            InstalledSkillPayloadAcquirer::new(payloads, environments),
            CapturingExecutor(captured.clone()),
            DifferentProjects,
            Arc::new(EmptyLibraryCandidateSource),
        );
        let selection = service.selection(&source, "demo").await.unwrap().selection;
        let request = CopyRequest {
            skill_name: "demo".to_string(),
            source,
            target_environment: EnvironmentRef::Native,
            target_project_ids: vec!["healthy".to_string(), "broken".to_string()],
            agent_selection: AgentSelectionSubmission {
                revision: selection.revision,
                selected_option_ids: selection.initial_selected_option_ids,
                requested_mode: InstallMode::Copy,
            },
        };

        let CopyPreviewOutcome::Ready { preview } = service.preview(&request).await.unwrap() else {
            panic!("fresh Agent selection should produce a copy preview");
        };

        assert_eq!(preview.targets.len(), 2);
        assert!(preview.targets[0].blocking_reasons.is_empty());
        assert_eq!(
            preview.targets[1].blocking_reasons,
            vec![OperationErrorCode::ConfigurationCorrupted]
        );

        let mut blocked_request = request;
        blocked_request.target_project_ids = vec!["broken".to_string()];
        let CopyPreviewOutcome::Ready {
            preview: blocked_preview,
        } = service.preview(&blocked_request).await.unwrap()
        else {
            panic!("fresh Agent selection should produce a blocked copy preview");
        };
        let response = service
            .execute(
                &CopyExecutionRequest {
                    request: blocked_request,
                    token: blocked_preview.token,
                    payload: blocked_preview.payload,
                },
                CancellationSignal::default(),
            )
            .await
            .unwrap();

        assert!(captured.lock().unwrap().is_none());
        assert_eq!(response.units.len(), 1);
        assert_eq!(
            response.units[0].error.as_ref().unwrap().code,
            OperationErrorCode::ConfigurationCorrupted
        );
    }

    #[tokio::test]
    async fn copy_service_builds_two_atomic_units_from_one_complete_source_payload() {
        let temp = tempdir().unwrap();
        let source_root = temp.path().join("source");
        let first_root = temp.path().join("first");
        let second_root = temp.path().join("second");
        let source_skill = source_root.join(".agents/skills/demo");
        fs::create_dir_all(source_skill.join("scripts")).unwrap();
        fs::write(source_skill.join("SKILL.md"), b"---\nname: demo\n---\nbody").unwrap();
        fs::write(source_skill.join("scripts/run.sh"), b"#!/bin/sh\n").unwrap();
        fs::create_dir_all(&first_root).unwrap();
        fs::create_dir_all(&second_root).unwrap();

        let source = context(EnvironmentRef::Native, "source");
        let first = context(EnvironmentRef::Native, "first");
        let second = context(EnvironmentRef::Native, "second");
        let facts = Arc::new(Mutex::new(HashMap::from([
            (
                source.clone(),
                planning_facts(source.clone(), &source_root, true),
            ),
            (
                first.clone(),
                planning_facts(first.clone(), &first_root, false),
            ),
            (second.clone(), planning_facts(second, &second_root, false)),
        ])));
        let environments = Arc::new(WslRuntime::default());
        let storage =
            Arc::new(NativePayloadSessionStorage::new(temp.path().join("payloads")).unwrap());
        let payloads = Arc::new(PayloadSessionManager::new(
            storage,
            PayloadSessionLimits {
                ttl_ms: 60_000,
                max_sessions: 8,
                max_bytes: 1024 * 1024,
            },
            || 1_000,
        ));
        let captured = Arc::new(Mutex::new(None));
        let service = CopyService::new(
            Facts(facts.clone()),
            crate::environment::planning::RuntimeTargetFactResolver::new(environments.clone()),
            payloads.clone(),
            InstalledSkillPayloadAcquirer::new(payloads, environments),
            CapturingExecutor(captured.clone()),
            DifferentProjects,
            Arc::new(EmptyLibraryCandidateSource),
        );
        let source_selection = service.selection(&source, "demo").await.unwrap().selection;
        let request = CopyRequest {
            skill_name: "demo".to_string(),
            source,
            target_environment: EnvironmentRef::Native,
            target_project_ids: vec!["first".to_string(), "second".to_string()],
            agent_selection: AgentSelectionSubmission {
                revision: source_selection.revision,
                selected_option_ids: source_selection.initial_selected_option_ids,
                requested_mode: InstallMode::Symlink,
            },
        };
        let preview = match service.preview(&request).await.unwrap() {
            CopyPreviewOutcome::Ready { preview } => preview,
            CopyPreviewOutcome::SelectionStale { .. } => {
                panic!("fresh copy Agent selection unexpectedly became stale")
            }
        };
        assert_eq!(preview.targets.len(), 2);

        facts
            .lock()
            .unwrap()
            .get_mut(&first)
            .unwrap()
            .revisions
            .environment = "environment-2".to_string();
        let stale_error = service
            .execute(
                &CopyExecutionRequest {
                    request: request.clone(),
                    token: preview.token.clone(),
                    payload: preview.payload.clone(),
                },
                CancellationSignal::default(),
            )
            .await
            .unwrap_err();
        assert!(matches!(stale_error, AppError::StaleEnvironment));
        facts
            .lock()
            .unwrap()
            .get_mut(&first)
            .unwrap()
            .revisions
            .environment = "environment-1".to_string();

        fs::write(
            source_skill.join("SKILL.md"),
            b"---\nname: demo\n---\nchanged",
        )
        .unwrap();
        let stale_payload_error = service
            .execute(
                &CopyExecutionRequest {
                    request: request.clone(),
                    token: preview.token.clone(),
                    payload: preview.payload.clone(),
                },
                CancellationSignal::default(),
            )
            .await
            .unwrap_err();
        assert!(matches!(stale_payload_error, AppError::StalePayload));
        fs::write(source_skill.join("SKILL.md"), b"---\nname: demo\n---\nbody").unwrap();

        let original_source_lock = facts
            .lock()
            .unwrap()
            .get(&request.source)
            .unwrap()
            .lock_document
            .clone();
        facts
            .lock()
            .unwrap()
            .get_mut(&request.source)
            .unwrap()
            .lock_document = LosslessLockDocument::empty(LockSchema::Project);
        let stale_lock_error = service
            .execute(
                &CopyExecutionRequest {
                    request: request.clone(),
                    token: preview.token.clone(),
                    payload: preview.payload.clone(),
                },
                CancellationSignal::default(),
            )
            .await
            .unwrap_err();
        assert!(matches!(stale_lock_error, AppError::StaleContext));
        facts
            .lock()
            .unwrap()
            .get_mut(&request.source)
            .unwrap()
            .lock_document = original_source_lock;

        facts
            .lock()
            .unwrap()
            .get_mut(&request.source)
            .unwrap()
            .revisions
            .context = ContextSnapshotRevision::parse("context-source-2").unwrap();
        let stale_source_error = service
            .execute(
                &CopyExecutionRequest {
                    request: request.clone(),
                    token: preview.token.clone(),
                    payload: preview.payload.clone(),
                },
                CancellationSignal::default(),
            )
            .await
            .unwrap_err();
        assert!(matches!(stale_source_error, AppError::StaleContext));
        facts
            .lock()
            .unwrap()
            .get_mut(&request.source)
            .unwrap()
            .revisions
            .context = ContextSnapshotRevision::parse("context-source").unwrap();

        let added_agent_path = source_root.join(".added/skills");
        let (added_agent_id, added_agent) =
            project_private_agent("added", &added_agent_path.to_string_lossy());
        facts
            .lock()
            .unwrap()
            .get_mut(&request.source)
            .unwrap()
            .agent_runtime
            .agents
            .insert(added_agent_id.clone(), added_agent);
        let stale_selection_error = service
            .execute(
                &CopyExecutionRequest {
                    request: request.clone(),
                    token: preview.token.clone(),
                    payload: preview.payload.clone(),
                },
                CancellationSignal::default(),
            )
            .await
            .unwrap_err();
        assert!(matches!(stale_selection_error, AppError::StaleContext));
        facts
            .lock()
            .unwrap()
            .get_mut(&request.source)
            .unwrap()
            .agent_runtime
            .agents
            .remove(&added_agent_id);

        service
            .execute(
                &CopyExecutionRequest {
                    request,
                    token: preview.token,
                    payload: preview.payload,
                },
                CancellationSignal::default(),
            )
            .await
            .unwrap();

        let captured = captured.lock().unwrap();
        let captured = captured.as_ref().unwrap();
        assert_eq!(captured.unit_count, 2);
        assert!(captured
            .payload_paths
            .iter()
            .any(|path| path == "scripts/run.sh"));
        assert_eq!(
            captured.remote_hashes,
            vec![Some("remote-v1".to_string()), Some("remote-v1".to_string())]
        );
    }

    #[tokio::test]
    async fn copy_merges_an_option_that_overlaps_the_standard_directory() {
        let temp = tempdir().unwrap();
        let source_root = temp.path().join("source");
        let healthy_root = temp.path().join("healthy");
        let conflicted_root = temp.path().join("conflicted");
        fs::create_dir_all(source_root.join(".agents/skills/demo")).unwrap();
        fs::write(
            source_root.join(".agents/skills/demo/SKILL.md"),
            b"---\nname: demo\n---\nbody",
        )
        .unwrap();
        fs::create_dir_all(&healthy_root).unwrap();
        fs::create_dir_all(&conflicted_root).unwrap();

        let source = context(EnvironmentRef::Native, "source");
        let healthy = context(EnvironmentRef::Native, "healthy");
        let conflicted = context(EnvironmentRef::Native, "conflicted");
        let mut source_facts = planning_facts(source.clone(), &source_root, true);
        let mut healthy_facts = planning_facts(healthy.clone(), &healthy_root, false);
        let mut conflicted_facts = planning_facts(conflicted.clone(), &conflicted_root, false);
        let source_agent_path = source_root.join(".source-agent/skills");
        let healthy_agent_path = healthy_root.join(".healthy-agent/skills");
        let conflicted_agent_path = conflicted_root.join(".agents/skills");
        let (agent_id, source_agent) =
            project_private_agent("custom", &source_agent_path.to_string_lossy());
        let (_, healthy_agent) =
            project_private_agent("custom", &healthy_agent_path.to_string_lossy());
        let (_, conflicted_agent) =
            project_private_agent("custom", &conflicted_agent_path.to_string_lossy());
        source_facts
            .agent_runtime
            .agents
            .insert(agent_id.clone(), source_agent);
        healthy_facts
            .agent_runtime
            .agents
            .insert(agent_id.clone(), healthy_agent);
        conflicted_facts
            .agent_runtime
            .agents
            .insert(agent_id.clone(), conflicted_agent);
        let facts = Arc::new(Mutex::new(HashMap::from([
            (source.clone(), source_facts),
            (healthy, healthy_facts),
            (conflicted, conflicted_facts),
        ])));
        let environments = Arc::new(WslRuntime::default());
        let storage =
            Arc::new(NativePayloadSessionStorage::new(temp.path().join("payloads")).unwrap());
        let payloads = Arc::new(PayloadSessionManager::new(
            storage,
            PayloadSessionLimits {
                ttl_ms: 60_000,
                max_sessions: 8,
                max_bytes: 1024 * 1024,
            },
            || 1_000,
        ));
        let captured = Arc::new(Mutex::new(None));
        let service = CopyService::new(
            Facts(facts),
            crate::environment::planning::RuntimeTargetFactResolver::new(environments.clone()),
            payloads.clone(),
            InstalledSkillPayloadAcquirer::new(payloads, environments),
            CapturingExecutor(captured.clone()),
            DifferentProjects,
            Arc::new(EmptyLibraryCandidateSource),
        );
        let selection = service.selection(&source, "demo").await.unwrap().selection;
        let option_id = selection
            .install_options
            .iter()
            .find(|option| option.agent_ids.contains(&agent_id))
            .unwrap()
            .id
            .clone();
        let request = CopyRequest {
            skill_name: "demo".to_string(),
            source,
            target_environment: EnvironmentRef::Native,
            target_project_ids: vec!["healthy".to_string(), "conflicted".to_string()],
            agent_selection: AgentSelectionSubmission {
                revision: selection.revision,
                selected_option_ids: vec![option_id],
                requested_mode: InstallMode::Symlink,
            },
        };
        let CopyPreviewOutcome::Ready { preview } = service.preview(&request).await.unwrap() else {
            panic!("fresh Agent selection should produce a copy preview");
        };

        assert!(preview.targets[0].blocking_reasons.is_empty());
        assert!(preview.targets[1].blocking_reasons.is_empty());
        let response = service
            .execute(
                &CopyExecutionRequest {
                    request,
                    token: preview.token,
                    payload: preview.payload,
                },
                CancellationSignal::default(),
            )
            .await
            .unwrap();

        assert_eq!(captured.lock().unwrap().as_ref().unwrap().unit_count, 2);
        assert!(response.units.is_empty());
    }

    #[tokio::test]
    async fn copy_without_source_metadata_plans_atomic_target_lock_removal() {
        let temp = tempdir().unwrap();
        let source_root = temp.path().join("source");
        let target_root = temp.path().join("target");
        let source_skill = source_root.join(".agents/skills/demo");
        fs::create_dir_all(&source_skill).unwrap();
        fs::write(source_skill.join("SKILL.md"), b"---\nname: demo\n---\nbody").unwrap();
        fs::create_dir_all(&target_root).unwrap();

        let source = context(EnvironmentRef::Native, "source");
        let target = context(EnvironmentRef::Native, "target");
        let facts = Arc::new(Mutex::new(HashMap::from([
            (
                source.clone(),
                planning_facts(source.clone(), &source_root, false),
            ),
            (target.clone(), planning_facts(target, &target_root, true)),
        ])));
        let environments = Arc::new(WslRuntime::default());
        let storage =
            Arc::new(NativePayloadSessionStorage::new(temp.path().join("payloads")).unwrap());
        let payloads = Arc::new(PayloadSessionManager::new(
            storage,
            PayloadSessionLimits {
                ttl_ms: 60_000,
                max_sessions: 8,
                max_bytes: 1024 * 1024,
            },
            || 1_000,
        ));
        let captured = Arc::new(Mutex::new(None));
        let service = CopyService::new(
            Facts(facts),
            crate::environment::planning::RuntimeTargetFactResolver::new(environments.clone()),
            payloads.clone(),
            InstalledSkillPayloadAcquirer::new(payloads, environments),
            CapturingExecutor(captured.clone()),
            DifferentProjects,
            Arc::new(EmptyLibraryCandidateSource),
        );
        let selection = service.selection(&source, "demo").await.unwrap().selection;
        let request = CopyRequest {
            skill_name: "demo".to_string(),
            source,
            target_environment: EnvironmentRef::Native,
            target_project_ids: vec!["target".to_string()],
            agent_selection: AgentSelectionSubmission {
                revision: selection.revision,
                selected_option_ids: selection.initial_selected_option_ids,
                requested_mode: InstallMode::Copy,
            },
        };
        let CopyPreviewOutcome::Ready { preview } = service.preview(&request).await.unwrap() else {
            panic!("fresh Agent selection should produce a copy preview");
        };

        service
            .execute(
                &CopyExecutionRequest {
                    request,
                    token: preview.token,
                    payload: preview.payload,
                },
                CancellationSignal::default(),
            )
            .await
            .unwrap();

        let captured = captured.lock().unwrap();
        let captured = captured.as_ref().unwrap();
        assert_eq!(captured.has_lock_mutations, vec![true]);
        assert_eq!(captured.lock_replacements, vec![None]);
        assert_eq!(
            captured.expected_lock_entries,
            vec![Some(serde_json::json!({
                "source": "owner/repo",
                "sourceType": "github",
                "sourceUrl": "https://github.com/owner/repo.git",
                "ref": "main",
                "skillPath": "skills/demo",
                "computedHash": "old-local",
                "remoteHash": "remote-v1",
                "pluginName": "toolkit"
            }))]
        );
    }
}

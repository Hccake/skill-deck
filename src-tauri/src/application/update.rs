use std::collections::BTreeSet;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::application::agent_intent::AgentTargetFallbackPreview;
use crate::application::install::InstallPlanExecutor;
use crate::application::mutation::plan::{MutationPlan, PreviewToken};
use crate::application::mutation::result::{
    ErrorReport, MutationUnitResult, MutationUnitStatus, OperationErrorCode,
};
use crate::application::payload_session::{
    AcquiredPayloadHandle, PayloadSessionManager, PinnedPayloadLease,
};
use crate::application::remove::ObservedEntryOwner;
use crate::application::resources::SkillIdentity;
#[cfg(test)]
use crate::application::source_evidence::RemoteSnapshotId;
use crate::application::source_evidence::{
    EvidenceAttempt, EvidenceFreshness, RemoteEvidenceKey, SourceSnapshotFacts,
};
use crate::application::source_snapshot_reuse::PayloadAcquisitionKey;
use crate::application::update_planner::{LocalUpdateInspection, LockedUpdateSkill};
use crate::core::agent_definition::AgentId;
use crate::core::mutation::CancellationSignal;
use crate::core::source_identity::{AcquisitionDescriptor, NormalizedRef, SourceIdentity};
use crate::environment::runtime::ObservedEntryId;
use crate::environment::types::ContextRef;
use crate::error::AppError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub enum UpdateCapabilityReasonCode {
    MissingRemoteHash,
    MissingSource,
    UnsupportedSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub enum SkillUpdateCheckStatus {
    UpdateAvailable,
    UpToDate,
    CannotCheck,
    DeletedUpstream,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub enum UpdateCheckReasonCode {
    MissingRemoteHash,
    MissingSource,
    UnsupportedSource,
    UpstreamUnavailable,
    DeletedUpstream,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct CheckUpdateCapability {
    pub can_run_update: bool,
    pub can_check_for_updates: bool,
    pub reason: Option<UpdateCapabilityReasonCode>,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct SkillUpdateInfo {
    pub name: String,
    pub source: String,
    pub has_update: bool,
    pub status: SkillUpdateCheckStatus,
    pub capability: CheckUpdateCapability,
    pub reason: Option<UpdateCheckReasonCode>,
    pub git_ref: Option<String>,
    pub source_url: Option<String>,
    pub skill_path: Option<String>,
    pub freshness: EvidenceFreshness,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub enum UpdateCheckMode {
    Automatic,
    Force,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "kind", content = "skills", rename_all = "camelCase")]
#[specta(tag = "kind", content = "skills", rename_all = "camelCase")]
pub enum UpdateCheckSelection {
    All,
    Skills(Vec<SkillIdentity>),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct UpdateCheckRequest {
    pub context: ContextRef,
    pub mode: UpdateCheckMode,
    pub selection: UpdateCheckSelection,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct SourceUpdateCheckInfo {
    pub source: String,
    pub requested_ref: Option<String>,
    pub resolved_ref: Option<String>,
    pub ref_revision: Option<String>,
    pub checked_at_epoch_ms: Option<u64>,
    pub expires_at_epoch_ms: Option<u64>,
    pub freshness: EvidenceFreshness,
    pub last_attempt: Option<EvidenceAttempt>,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct UpdateCheckResponse {
    pub sources: Vec<SourceUpdateCheckInfo>,
    pub skills: Vec<SkillUpdateInfo>,
}

pub fn derive_update_capability(
    has_reinstall_source: bool,
    has_remote_hash: bool,
) -> CheckUpdateCapability {
    match (has_reinstall_source, has_remote_hash) {
        (true, true) => CheckUpdateCapability {
            can_run_update: true,
            can_check_for_updates: true,
            reason: None,
        },
        (true, false) => CheckUpdateCapability {
            can_run_update: true,
            can_check_for_updates: false,
            reason: Some(UpdateCapabilityReasonCode::MissingRemoteHash),
        },
        (false, _) => CheckUpdateCapability {
            can_run_update: false,
            can_check_for_updates: false,
            reason: Some(UpdateCapabilityReasonCode::MissingSource),
        },
    }
}

pub fn derive_update_capability_from_metadata(
    metadata: &crate::core::NormalizedUpdateMetadata,
) -> CheckUpdateCapability {
    let core = crate::core::derive_update_capability(metadata);
    let mut capability = derive_update_capability(
        core.can_run_update,
        metadata.comparison_baseline().is_some(),
    );
    capability.can_check_for_updates = core.can_check_for_updates;
    capability.reason = match core.reason.as_deref() {
        None => None,
        Some("missing-remote-hash") => Some(UpdateCapabilityReasonCode::MissingRemoteHash),
        Some(_)
            if metadata.source.is_empty()
                || metadata.skill_path.as_deref().unwrap_or("").is_empty() =>
        {
            Some(UpdateCapabilityReasonCode::MissingSource)
        }
        Some(_) => Some(UpdateCapabilityReasonCode::UnsupportedSource),
    };
    capability
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct UpdateRequest {
    pub context: ContextRef,
    pub skill_names: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct UpdateExecutionRequest {
    pub request: UpdateRequest,
    pub overwrite_private_entries: Vec<ObservedEntryId>,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct UpdatePreview {
    pub token: PreviewToken,
    pub skills: Vec<UpdateSkillPreview>,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct UpdateSkillPreview {
    pub skill_name: String,
    pub source_display: String,
    pub ref_display: String,
    pub placement_agent_ids: Vec<AgentId>,
    pub capability: CheckUpdateCapability,
    pub clean_copy_count: usize,
    pub overwrite_private_entries: Vec<UpdateConflictCopyPreview>,
    pub blocking_reasons: Vec<OperationErrorCode>,
    pub fallback_forecasts: Vec<AgentTargetFallbackPreview>,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct UpdateConflictCopyPreview {
    pub entry_id: ObservedEntryId,
    pub owners: Vec<ObservedEntryOwner>,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct UpdateResponse {
    pub sources: Vec<UpdateSourceResult>,
    pub skills: Vec<UpdateSkillResult>,
    pub outcome: UpdateOutcome,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub enum UpdateSourceStatus {
    Acquired,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct UpdateSourceResult {
    pub id: String,
    pub source: String,
    pub status: UpdateSourceStatus,
    pub error: Option<ErrorReport>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub enum UpdateWarningCode {
    PreservedConflictingCopy,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
#[specta(tag = "kind", rename_all = "camelCase")]
#[expect(
    clippy::large_enum_variant,
    reason = "boxing ErrorReport would complicate the stable generated IPC contract"
)]
pub enum UpdateCoverage {
    Updated,
    PreservedConflicts,
    NotUpdated { error: ErrorReport },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub enum UpdateOutcome {
    Succeeded,
    Partial,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct UpdateSkillResult {
    pub skill_identity: SkillIdentity,
    pub source_result_id: String,
    pub mutation: Option<MutationUnitResult>,
    pub coverage: UpdateCoverage,
    pub warnings: Vec<UpdateWarningCode>,
    pub retryable: bool,
}

pub type UpdateFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

pub trait UpdatePlanner: Send + Sync {
    fn inspect<'a>(
        &'a self,
        request: &'a UpdateRequest,
    ) -> UpdateFuture<'a, Result<LocalUpdateInspection, AppError>>;

    fn build<'a>(
        &'a self,
        execution: &'a UpdateExecutionRequest,
        handles: Vec<AcquiredPayloadHandle>,
        payloads: Vec<PinnedPayloadLease>,
    ) -> UpdateFuture<'a, Result<(PreviewToken, MutationPlan), AppError>>;
}

pub struct UpdateAcquisitionGroup {
    pub source_result_id: String,
    pub source: String,
    pub context: ContextRef,
    pub key: PayloadAcquisitionKey,
    pub evidence_key: RemoteEvidenceKey,
    pub descriptor: Arc<AcquisitionDescriptor>,
    pub skills: Vec<LockedUpdateSkill>,
}

pub struct AcquiredUpdateSource {
    pub facts: SourceSnapshotFacts,
    pub payloads: Vec<(String, AcquiredPayloadHandle)>,
}

pub struct UpdateSourceAcquisition {
    pub source_result_id: String,
    pub source: String,
    pub skill_names: Vec<String>,
    pub result: Result<AcquiredUpdateSource, AppError>,
}

pub trait UpdatePayloadAcquirer: Send + Sync {
    fn acquire<'a>(
        &'a self,
        groups: &'a [UpdateAcquisitionGroup],
        cancellation: CancellationSignal,
    ) -> UpdateFuture<'a, Result<Vec<UpdateSourceAcquisition>, AppError>>;
}

pub struct UpdateService<P, A, E> {
    payloads: Arc<PayloadSessionManager>,
    planner: P,
    acquirer: A,
    executor: E,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateExecutionStage {
    Validating,
    Updating,
}

impl<P, A, E> UpdateService<P, A, E>
where
    P: UpdatePlanner,
    A: UpdatePayloadAcquirer,
    E: InstallPlanExecutor,
{
    pub fn new(payloads: Arc<PayloadSessionManager>, planner: P, acquirer: A, executor: E) -> Self {
        Self {
            payloads,
            planner,
            acquirer,
            executor,
        }
    }

    pub async fn preview(&self, request: &UpdateRequest) -> Result<UpdatePreview, AppError> {
        validate_update_request(request)?;
        preview_from_inspection(self.planner.inspect(request).await?)
    }

    #[cfg(any(test, feature = "wsl-integration-tests"))]
    #[allow(dead_code, reason = "used by the Windows-only WSL acceptance harness")]
    pub async fn execute(
        &self,
        execution: &UpdateExecutionRequest,
        expected_token: PreviewToken,
        cancellation: CancellationSignal,
    ) -> Result<UpdateResponse, AppError> {
        self.execute_with_stage_observer(execution, expected_token, cancellation, |_| {})
            .await
    }

    pub async fn execute_with_stage_observer<F>(
        &self,
        execution: &UpdateExecutionRequest,
        expected_token: PreviewToken,
        cancellation: CancellationSignal,
        mut observe_stage: F,
    ) -> Result<UpdateResponse, AppError>
    where
        F: FnMut(UpdateExecutionStage),
    {
        validate_update_request(&execution.request)?;
        validate_conflict_decisions(execution)?;
        let initial = self.planner.inspect(&execution.request).await?;
        validate_preview_token(&expected_token, &initial.token)?;
        let groups = acquisition_groups(
            &execution.request.context,
            initial.source_candidates.clone(),
        )?;
        let acquisitions = self.acquirer.acquire(&groups, cancellation.clone()).await?;
        observe_stage(UpdateExecutionStage::Validating);
        let latest = self.planner.inspect(&execution.request).await?;
        validate_preview_authority(&initial.token, &latest.token)?;

        let mut sources = Vec::with_capacity(acquisitions.len());
        let mut skills = Vec::with_capacity(execution.request.skill_names.len());
        let mut handles = Vec::new();
        let mut payloads = Vec::new();
        let mut executable_names = Vec::new();
        let selected = execution
            .overwrite_private_entries
            .iter()
            .cloned()
            .collect::<BTreeSet<_>>();
        let latest_by_name = latest
            .skills
            .iter()
            .map(|skill| (skill.skill_name.as_str(), skill))
            .collect::<std::collections::BTreeMap<_, _>>();
        let initial_by_name = initial
            .skills
            .iter()
            .map(|skill| (skill.skill_name.as_str(), skill))
            .collect::<std::collections::BTreeMap<_, _>>();
        let initial_locked_by_name = initial
            .source_candidates
            .iter()
            .map(|skill| (skill.name.as_str(), skill))
            .collect::<std::collections::BTreeMap<_, _>>();
        let latest_locked_by_name = latest
            .source_candidates
            .iter()
            .map(|skill| (skill.name.as_str(), skill))
            .collect::<std::collections::BTreeMap<_, _>>();
        let drifted_names = execution
            .request
            .skill_names
            .iter()
            .filter(|name| {
                initial_by_name
                    .get(name.as_str())
                    .zip(latest_by_name.get(name.as_str()))
                    .is_none_or(|(initial, latest)| {
                        initial.observed_digest != latest.observed_digest
                    })
                    || initial_locked_by_name.get(name.as_str())
                        != latest_locked_by_name.get(name.as_str())
            })
            .cloned()
            .collect::<BTreeSet<_>>();
        for acquisition in acquisitions {
            match acquisition.result {
                Ok(acquired) => {
                    let _facts = acquired.facts;
                    sources.push(UpdateSourceResult {
                        id: acquisition.source_result_id.clone(),
                        source: acquisition.source,
                        status: UpdateSourceStatus::Acquired,
                        error: None,
                    });
                    for (skill_name, handle) in acquired.payloads {
                        if drifted_names.contains(&skill_name) {
                            skills.push(not_updated_skill(
                                &execution.request.context,
                                skill_name,
                                acquisition.source_result_id.clone(),
                                ErrorReport::from_app_error(
                                    AppError::StaleTarget,
                                    Some(execution.request.context.clone()),
                                ),
                            ));
                            continue;
                        }
                        let lease = self.payloads.pin_verified(&handle).await?;
                        executable_names.push(skill_name);
                        handles.push(handle);
                        payloads.push(lease);
                    }
                }
                Err(error) => {
                    let report =
                        ErrorReport::from_app_error(error, Some(execution.request.context.clone()));
                    sources.push(UpdateSourceResult {
                        id: acquisition.source_result_id.clone(),
                        source: acquisition.source,
                        status: UpdateSourceStatus::Failed,
                        error: Some(report.clone()),
                    });
                    for skill_name in acquisition.skill_names {
                        skills.push(UpdateSkillResult {
                            skill_identity: SkillIdentity {
                                context: execution.request.context.clone(),
                                skill_name,
                            },
                            source_result_id: acquisition.source_result_id.clone(),
                            mutation: None,
                            coverage: UpdateCoverage::NotUpdated {
                                error: report.clone(),
                            },
                            warnings: Vec::new(),
                            retryable: report.retryable,
                        });
                    }
                }
            }
        }

        let source_by_skill = groups
            .iter()
            .flat_map(|group| {
                group
                    .skills
                    .iter()
                    .map(|skill| (skill.name.clone(), group.source_result_id.clone()))
            })
            .collect::<std::collections::BTreeMap<_, _>>();
        if !executable_names.is_empty() {
            let successful_request = UpdateRequest {
                context: execution.request.context.clone(),
                skill_names: executable_names.clone(),
            };
            let mut overwrite_private_entries = execution.overwrite_private_entries.clone();
            for skill_name in &executable_names {
                if let Some(inspection) = latest_by_name.get(skill_name.as_str()) {
                    overwrite_private_entries.extend(
                        inspection
                            .clean_copies
                            .iter()
                            .map(|entry| entry.entry_id.clone()),
                    );
                }
            }
            overwrite_private_entries.sort();
            overwrite_private_entries.dedup();
            let successful_execution = UpdateExecutionRequest {
                request: successful_request,
                overwrite_private_entries,
            };
            let (actual_token, plan) = self
                .planner
                .build(&successful_execution, handles, payloads)
                .await?;
            validate_preview_authority(&latest.token, &actual_token)?;
            observe_stage(UpdateExecutionStage::Updating);
            let mutations = self.executor.execute(plan, cancellation).await;
            let mut mutation_by_skill = mutations
                .into_iter()
                .map(|result| (result.skill_name.clone(), result))
                .collect::<std::collections::BTreeMap<_, _>>();
            for skill_name in executable_names {
                let inspection = latest_by_name.get(skill_name.as_str());
                let preserved = inspection.is_some_and(|inspection| {
                    inspection
                        .conflicts
                        .iter()
                        .any(|entry| !selected.contains(&entry.entry_id))
                });
                let mutation = mutation_by_skill.remove(&skill_name);
                let (coverage, warnings, retryable) =
                    update_coverage(mutation.as_ref(), preserved, &execution.request.context);
                skills.push(UpdateSkillResult {
                    skill_identity: SkillIdentity {
                        context: execution.request.context.clone(),
                        skill_name: skill_name.clone(),
                    },
                    source_result_id: source_by_skill
                        .get(&skill_name)
                        .cloned()
                        .unwrap_or_default(),
                    mutation,
                    coverage,
                    warnings,
                    retryable,
                });
            }
        }
        skills.sort_by(|left, right| {
            left.skill_identity
                .skill_name
                .cmp(&right.skill_identity.skill_name)
        });
        let outcome = update_outcome(&skills);
        Ok(UpdateResponse {
            sources,
            skills,
            outcome,
        })
    }
}

fn not_updated_skill(
    context: &ContextRef,
    skill_name: String,
    source_result_id: String,
    report: ErrorReport,
) -> UpdateSkillResult {
    UpdateSkillResult {
        skill_identity: SkillIdentity {
            context: context.clone(),
            skill_name,
        },
        source_result_id,
        mutation: None,
        coverage: UpdateCoverage::NotUpdated {
            error: report.clone(),
        },
        warnings: Vec::new(),
        retryable: report.retryable,
    }
}

fn update_coverage(
    mutation: Option<&MutationUnitResult>,
    preserved: bool,
    context: &ContextRef,
) -> (UpdateCoverage, Vec<UpdateWarningCode>, bool) {
    let Some(mutation) = mutation else {
        let report = ErrorReport::from_app_error(
            AppError::ExecutionFailed {
                message: "update coordinator did not return a mutation result".to_string(),
            },
            Some(context.clone()),
        );
        return (
            UpdateCoverage::NotUpdated { error: report },
            Vec::new(),
            false,
        );
    };
    if mutation.status == MutationUnitStatus::Succeeded {
        return if preserved {
            (
                UpdateCoverage::PreservedConflicts,
                vec![UpdateWarningCode::PreservedConflictingCopy],
                mutation.retryable,
            )
        } else {
            (UpdateCoverage::Updated, Vec::new(), mutation.retryable)
        };
    }
    let report = mutation.error.clone().unwrap_or_else(|| {
        ErrorReport::from_app_error(
            AppError::ExecutionFailed {
                message: format!("update mutation ended as {:?}", mutation.status),
            },
            Some(context.clone()),
        )
    });
    (
        UpdateCoverage::NotUpdated { error: report },
        Vec::new(),
        mutation.retryable,
    )
}

pub fn validate_update_request(request: &UpdateRequest) -> Result<(), AppError> {
    let mut names = BTreeSet::new();
    if request.skill_names.is_empty()
        || request
            .skill_names
            .iter()
            .any(|name| name.trim().is_empty() || !names.insert(name))
    {
        return Err(validation("invalid or duplicate Skill selection"));
    }
    Ok(())
}

fn preview_from_inspection(inspection: LocalUpdateInspection) -> Result<UpdatePreview, AppError> {
    let display_by_name = inspection
        .source_candidates
        .iter()
        .map(|skill| {
            let identity = SourceIdentity::from_metadata(&skill.metadata())?;
            let ref_display = match identity.normalized_ref() {
                NormalizedRef::Default => "HEAD".to_string(),
                NormalizedRef::Named(value) => value.clone(),
            };
            Ok((
                skill.name.clone(),
                (
                    skill.capability(),
                    identity.sanitized_display().to_string(),
                    ref_display,
                ),
            ))
        })
        .collect::<Result<std::collections::BTreeMap<_, _>, AppError>>()?;
    let skills = inspection
        .skills
        .into_iter()
        .map(|skill| {
            let (capability, source_display, ref_display) = display_by_name
                .get(&skill.skill_name)
                .cloned()
                .ok_or(AppError::StaleContext)?;
            Ok(UpdateSkillPreview {
                capability,
                source_display,
                ref_display,
                placement_agent_ids: skill.placement_agent_ids,
                skill_name: skill.skill_name,
                clean_copy_count: skill.clean_copies.len(),
                overwrite_private_entries: skill
                    .conflicts
                    .into_iter()
                    .map(|entry| UpdateConflictCopyPreview {
                        entry_id: entry.entry_id,
                        owners: entry.owners,
                    })
                    .collect(),
                blocking_reasons: skill.blocking_reasons,
                fallback_forecasts: Vec::new(),
            })
        })
        .collect::<Result<Vec<_>, AppError>>()?;
    Ok(UpdatePreview {
        token: inspection.token,
        skills,
    })
}

fn acquisition_groups(
    context: &ContextRef,
    skills: Vec<LockedUpdateSkill>,
) -> Result<Vec<UpdateAcquisitionGroup>, AppError> {
    let mut groups = Vec::<UpdateAcquisitionGroup>::new();
    for skill in skills {
        let identity = crate::core::SourceIdentity::from_metadata(&skill.metadata())?;
        let key = PayloadAcquisitionKey::from_identity(&identity, &context.environment);
        if let Some(group) = groups.iter_mut().find(|group| {
            group.key == key
                && group
                    .descriptor
                    .acquisition_equivalent(identity.acquisition())
        }) {
            group.skills.push(skill);
            continue;
        }
        groups.push(UpdateAcquisitionGroup {
            source_result_id: format!("source-{}", groups.len() + 1),
            source: identity.sanitized_display().to_string(),
            context: context.clone(),
            key,
            evidence_key: RemoteEvidenceKey::from_identity(&identity),
            descriptor: Arc::new(identity.acquisition().clone()),
            skills: vec![skill],
        });
    }
    Ok(groups)
}

fn validate_conflict_decisions(execution: &UpdateExecutionRequest) -> Result<(), AppError> {
    let mut entries = BTreeSet::new();
    if execution
        .overwrite_private_entries
        .iter()
        .any(|entry| !entries.insert(entry))
    {
        return Err(AppError::StalePayload);
    }
    Ok(())
}

fn update_outcome(skills: &[UpdateSkillResult]) -> UpdateOutcome {
    let succeeded = skills.iter().filter(|skill| {
        skill
            .mutation
            .as_ref()
            .is_some_and(|mutation| mutation.status == MutationUnitStatus::Succeeded)
    });
    let succeeded_count = succeeded.count();
    let cancelled_count = skills
        .iter()
        .filter(|skill| {
            skill
                .mutation
                .as_ref()
                .is_some_and(|mutation| mutation.status == MutationUnitStatus::Cancelled)
        })
        .count();
    let cancelled_before_mutation = skills.iter().any(|skill| {
        matches!(
            skill.coverage,
            UpdateCoverage::NotUpdated {
                error: ErrorReport {
                    code: OperationErrorCode::MutationCancelled,
                    ..
                }
            }
        )
    });
    if succeeded_count == skills.len() {
        UpdateOutcome::Succeeded
    } else if succeeded_count > 0 {
        UpdateOutcome::Partial
    } else if cancelled_count > 0 || cancelled_before_mutation {
        UpdateOutcome::Cancelled
    } else {
        UpdateOutcome::Failed
    }
}

fn validate_preview_token(expected: &PreviewToken, actual: &PreviewToken) -> Result<(), AppError> {
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

fn validate_preview_authority(
    expected: &PreviewToken,
    actual: &PreviewToken,
) -> Result<(), AppError> {
    if expected.registry_revision != actual.registry_revision {
        return Err(AppError::StaleRegistry);
    }
    if expected.environment_revision != actual.environment_revision {
        return Err(AppError::StaleEnvironment);
    }
    if expected.context_revision != actual.context_revision {
        return Err(AppError::StaleContext);
    }
    Ok(())
}

fn validation(message: &str) -> AppError {
    AppError::Validation {
        field: Some("request".to_string()),
        message: message.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    use super::*;
    use crate::application::install::InstallPlanExecutor;
    use crate::application::mutation::plan::MutationPlan;
    use crate::application::payload_session::{PayloadSessionLimits, PayloadSessionManager};
    use crate::application::source_evidence::SourceSnapshotFacts;
    use crate::core::mutation::CancellationSignal;
    use crate::core::skill_payload::build_skill_payload;
    use crate::core::source_identity::NormalizedRef;
    use crate::environment::runtime::ContextSnapshotRevision;
    use crate::environment::types::{ContextScope, EnvironmentRef};
    use crate::error::AppError;
    use tempfile::tempdir;

    #[test]
    fn missing_remote_hash_can_reinstall_but_cannot_check() {
        let capability = derive_update_capability(true, false);
        assert!(capability.can_run_update);
        assert!(!capability.can_check_for_updates);
        assert_eq!(
            capability.reason,
            Some(UpdateCapabilityReasonCode::MissingRemoteHash)
        );
    }

    #[test]
    fn missing_source_metadata_is_not_reported_as_an_unsupported_provider() {
        let capability =
            derive_update_capability_from_metadata(&crate::core::NormalizedUpdateMetadata {
                source: String::new(),
                source_type: "github".to_string(),
                source_url: None,
                ref_name: None,
                skill_path: Some("skills/demo".to_string()),
                remote_hash: None,
                computed_hash: None,
            });

        assert_eq!(
            capability.reason,
            Some(UpdateCapabilityReasonCode::MissingSource)
        );
    }

    #[test]
    fn update_preview_counts_clean_copies_without_exposing_their_entries() {
        fn observed_entry(
            id: &str,
            path: &str,
        ) -> crate::application::remove::ObservedPhysicalEntry {
            crate::application::remove::ObservedPhysicalEntry {
                entry_id: crate::environment::runtime::ObservedEntryId::parse(id).unwrap(),
                display_path: crate::environment::types::ResourceLocator {
                    environment: EnvironmentRef::Host,
                    native_path: path.to_string(),
                },
                kind: crate::application::remove::ObservedEntryKind::Directory,
                physical_target_key: format!("credential-secret-target-{id}"),
                owners: vec![crate::application::remove::ObservedEntryOwner {
                    agent_id: crate::core::agent_definition::AgentId::parse("codex").unwrap(),
                    display_name: "Codex".to_string(),
                    logical_target_id: "codex-private".to_string(),
                }],
                will_break_if_canonical_removed: false,
            }
        }

        let token = PreviewToken {
            generation: "preview-v1-clean-copies".to_string(),
            registry_revision: "registry-v1".to_string(),
            environment_revision: "environment-v1".to_string(),
            context_revision: ContextSnapshotRevision::parse("context-v1").unwrap(),
        };
        let mut inspection = inspection(token, "old");
        inspection.skills[0].clean_copies = vec![
            observed_entry("entry-v1-clean-one", "/agents/clean-one"),
            observed_entry("entry-v1-clean-two", "/agents/clean-two"),
        ];
        inspection.skills[0].conflicts =
            vec![observed_entry("entry-v1-conflict", "/agents/conflict")];

        let preview = serde_json::to_value(preview_from_inspection(inspection).unwrap()).unwrap();
        let skill = &preview["skills"][0];

        assert_eq!(skill["cleanCopyCount"], serde_json::json!(2));
        assert!(skill.get("cleanCopies").is_none());
        assert!(!skill.to_string().contains("entry-v1-clean"));
        assert!(!skill.to_string().contains("/agents/clean"));
        assert_eq!(
            skill["overwritePrivateEntries"].as_array().unwrap().len(),
            1
        );
        assert_eq!(
            skill["overwritePrivateEntries"][0]["entryId"],
            serde_json::json!("entry-v1-conflict")
        );
        let serialized = skill["overwritePrivateEntries"][0].to_string();
        assert!(!serialized.contains("nativePath"));
        assert!(!serialized.contains("displayPath"));
        assert!(!serialized.contains("physicalTargetKey"));
        assert!(!serialized.contains("/agents/conflict"));
        assert!(!serialized.contains("credential-secret"));
        assert!(serialized.contains("Codex"));
    }

    #[test]
    fn update_preview_exposes_only_backend_sanitized_display_facts() {
        let token = PreviewToken {
            generation: "preview-v1-display".to_string(),
            registry_revision: "registry-v1".to_string(),
            environment_revision: "environment-v1".to_string(),
            context_revision: ContextSnapshotRevision::parse("context-v1").unwrap(),
        };
        let mut inspection = inspection(token, "old");
        inspection.source_candidates.truncate(1);
        inspection.skills.truncate(1);
        inspection.source_candidates[0].source = "owner/repo".to_string();
        inspection.source_candidates[0].source_url =
            Some("https://secret-token@github.com/owner/repo.git".to_string());
        inspection.source_candidates[0].ref_name = Some("release".to_string());
        inspection.skills[0].placement_agent_ids =
            vec![crate::core::agent_definition::AgentId::parse("codex").unwrap()];
        inspection.skills[0].conflicts = vec![crate::application::remove::ObservedPhysicalEntry {
            entry_id: crate::environment::runtime::ObservedEntryId::parse("entry-v1-private")
                .unwrap(),
            display_path: crate::environment::types::ResourceLocator {
                environment: EnvironmentRef::Host,
                native_path: "/agents/private".to_string(),
            },
            kind: crate::application::remove::ObservedEntryKind::Directory,
            physical_target_key: "credential-secret-target".to_string(),
            owners: vec![crate::application::remove::ObservedEntryOwner {
                agent_id: crate::core::agent_definition::AgentId::parse("codex").unwrap(),
                display_name: "Codex".to_string(),
                logical_target_id: "codex-private".to_string(),
            }],
            will_break_if_canonical_removed: false,
        }];

        let preview = serde_json::to_value(preview_from_inspection(inspection).unwrap()).unwrap();
        let skill = &preview["skills"][0];

        assert_eq!(
            skill["sourceDisplay"],
            serde_json::json!("github.com/owner/repo")
        );
        assert_eq!(skill["refDisplay"], serde_json::json!("release"));
        assert_eq!(skill["placementAgentIds"], serde_json::json!(["codex"]));
        let serialized = preview.to_string();
        assert!(!serialized.contains("secret-token"));
        assert!(!serialized.contains("sourceUrl"));
        assert!(!serialized.contains("/agents/private"));
        assert!(!serialized.contains("credential-secret"));
    }

    #[test]
    fn update_check_request_serializes_mode_and_typed_selection() {
        let context = ContextRef {
            environment: EnvironmentRef::Host,
            scope: ContextScope::Global,
        };
        let request = UpdateCheckRequest {
            context: context.clone(),
            mode: UpdateCheckMode::Force,
            selection: UpdateCheckSelection::Skills(vec![
                crate::application::resources::SkillIdentity {
                    context,
                    skill_name: "demo".to_string(),
                },
            ]),
        };

        assert_eq!(
            serde_json::to_value(request).unwrap(),
            serde_json::json!({
                "context": {
                    "environment": { "kind": "host" },
                    "scope": { "scope": "global" }
                },
                "mode": "force",
                "selection": { "kind": "skills", "skills": [{
                    "context": {
                        "environment": { "kind": "host" },
                        "scope": { "scope": "global" }
                    },
                    "skillName": "demo"
                }] }
            })
        );
    }

    struct Planner {
        token: PreviewToken,
        rebuilds: Arc<AtomicUsize>,
    }

    fn locked_update_skill(name: &str, source_url: &str) -> LockedUpdateSkill {
        LockedUpdateSkill {
            name: name.to_string(),
            source: "owner/repo".to_string(),
            source_type: "github".to_string(),
            source_url: Some(source_url.to_string()),
            ref_name: Some("main".to_string()),
            skill_path: format!("skills/{name}"),
            remote_hash: Some("old".to_string()),
            computed_hash: None,
            installed_at: None,
            subagents: Vec::new(),
        }
    }

    impl UpdatePlanner for Planner {
        fn inspect<'a>(
            &'a self,
            _request: &'a UpdateRequest,
        ) -> UpdateFuture<'a, Result<LocalUpdateInspection, AppError>> {
            Box::pin(async move {
                Ok(LocalUpdateInspection {
                    token: self.token.clone(),
                    source_candidates: vec![LockedUpdateSkill {
                        name: "demo".to_string(),
                        source: "owner/repo".to_string(),
                        source_type: "github".to_string(),
                        source_url: Some("https://github.com/owner/repo.git".to_string()),
                        ref_name: Some("main".to_string()),
                        skill_path: "skills/demo".to_string(),
                        remote_hash: Some("old".to_string()),
                        computed_hash: None,
                        installed_at: None,
                        subagents: Vec::new(),
                    }],
                    skills: vec![
                        crate::application::update_planner::LocalUpdateSkillInspection {
                            skill_name: "demo".to_string(),
                            observed_digest: "demo-observed".to_string(),
                            placement_agent_ids: Vec::new(),
                            clean_copies: Vec::new(),
                            conflicts: Vec::new(),
                            blocking_reasons: Vec::new(),
                        },
                    ],
                })
            })
        }

        fn build<'a>(
            &'a self,
            _execution: &'a UpdateExecutionRequest,
            _handles: Vec<AcquiredPayloadHandle>,
            payloads: Vec<PinnedPayloadLease>,
        ) -> UpdateFuture<'a, Result<(PreviewToken, MutationPlan), AppError>> {
            self.rebuilds.fetch_add(1, Ordering::SeqCst);
            Box::pin(async move {
                Ok((
                    self.token.clone(),
                    MutationPlan {
                        operation_id: "update-1".to_string(),
                        payloads: payloads
                            .into_iter()
                            .map(|lease| (lease.manifest().payload_id().clone(), lease))
                            .collect(),
                        units: Vec::new(),
                    },
                ))
            })
        }
    }

    struct Acquirer(Arc<AtomicUsize>);

    impl UpdatePayloadAcquirer for Acquirer {
        fn acquire<'a>(
            &'a self,
            _groups: &'a [UpdateAcquisitionGroup],
            _cancellation: CancellationSignal,
        ) -> UpdateFuture<'a, Result<Vec<UpdateSourceAcquisition>, AppError>> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Box::pin(async { Ok(Vec::new()) })
        }
    }

    struct FailingAcquirer;

    impl UpdatePayloadAcquirer for FailingAcquirer {
        fn acquire<'a>(
            &'a self,
            groups: &'a [UpdateAcquisitionGroup],
            _cancellation: CancellationSignal,
        ) -> UpdateFuture<'a, Result<Vec<UpdateSourceAcquisition>, AppError>> {
            let group = &groups[0];
            Box::pin(async move {
                Ok(vec![UpdateSourceAcquisition {
                    source_result_id: group.source_result_id.clone(),
                    source: group.source.clone(),
                    skill_names: group
                        .skills
                        .iter()
                        .map(|skill| skill.name.clone())
                        .collect(),
                    result: Err(AppError::GitCloneFailed {
                        message: "source unavailable".to_string(),
                    }),
                }])
            })
        }
    }

    struct Executor(Arc<AtomicUsize>);

    impl InstallPlanExecutor for Executor {
        fn execute<'a>(
            &'a self,
            plan: MutationPlan,
            _cancellation: CancellationSignal,
        ) -> UpdateFuture<'a, Vec<MutationUnitResult>> {
            assert_eq!(plan.payloads.len(), 1);
            self.0.fetch_add(1, Ordering::SeqCst);
            Box::pin(async { Vec::new() })
        }
    }

    struct SequencedPlanner {
        inspections: Mutex<Vec<LocalUpdateInspection>>,
        builds: Arc<AtomicUsize>,
    }

    impl UpdatePlanner for SequencedPlanner {
        fn inspect<'a>(
            &'a self,
            _request: &'a UpdateRequest,
        ) -> UpdateFuture<'a, Result<LocalUpdateInspection, AppError>> {
            Box::pin(async move { Ok(self.inspections.lock().unwrap().remove(0)) })
        }

        fn build<'a>(
            &'a self,
            _execution: &'a UpdateExecutionRequest,
            _handles: Vec<AcquiredPayloadHandle>,
            payloads: Vec<PinnedPayloadLease>,
        ) -> UpdateFuture<'a, Result<(PreviewToken, MutationPlan), AppError>> {
            self.builds.fetch_add(1, Ordering::SeqCst);
            Box::pin(async move {
                Ok((
                    PreviewToken {
                        generation: "rebuilt".to_string(),
                        registry_revision: "registry-1".to_string(),
                        environment_revision: "environment-1".to_string(),
                        context_revision: ContextSnapshotRevision::parse("context-v1-demo")
                            .unwrap(),
                    },
                    MutationPlan {
                        operation_id: "update-1".to_string(),
                        payloads: payloads
                            .into_iter()
                            .map(|lease| (lease.manifest().payload_id().clone(), lease))
                            .collect(),
                        units: Vec::new(),
                    },
                ))
            })
        }
    }

    struct FixedAcquirer {
        acquisitions: Mutex<Option<Vec<UpdateSourceAcquisition>>>,
        calls: Arc<AtomicUsize>,
        expected_group_count: usize,
    }

    impl UpdatePayloadAcquirer for FixedAcquirer {
        fn acquire<'a>(
            &'a self,
            groups: &'a [UpdateAcquisitionGroup],
            _cancellation: CancellationSignal,
        ) -> UpdateFuture<'a, Result<Vec<UpdateSourceAcquisition>, AppError>> {
            assert_eq!(groups.len(), self.expected_group_count);
            self.calls.fetch_add(1, Ordering::SeqCst);
            let acquisitions = self.acquisitions.lock().unwrap().take().unwrap();
            Box::pin(async move { Ok(acquisitions) })
        }
    }

    struct ResultExecutor {
        calls: Arc<AtomicUsize>,
        results: Vec<MutationUnitResult>,
        expected_payload_count: usize,
    }

    impl InstallPlanExecutor for ResultExecutor {
        fn execute<'a>(
            &'a self,
            plan: MutationPlan,
            _cancellation: CancellationSignal,
        ) -> UpdateFuture<'a, Vec<MutationUnitResult>> {
            assert_eq!(plan.payloads.len(), self.expected_payload_count);
            self.calls.fetch_add(1, Ordering::SeqCst);
            Box::pin(async { self.results.clone() })
        }
    }

    fn mutation_result(name: &str, status: MutationUnitStatus) -> MutationUnitResult {
        MutationUnitResult {
            unit_id: name.to_string(),
            skill_name: name.to_string(),
            source: None,
            target: ContextRef {
                environment: EnvironmentRef::Host,
                scope: ContextScope::Global,
            },
            status,
            retryable: status != MutationUnitStatus::Succeeded,
            lock_committed: status == MutationUnitStatus::Succeeded,
            actual_mode: None,
            fallback_reason: None,
            agent_targets: Vec::new(),
            warnings: Vec::new(),
            error: None,
            recovery: None,
        }
    }

    fn inspection(token: PreviewToken, alpha_hash: &str) -> LocalUpdateInspection {
        LocalUpdateInspection {
            token,
            source_candidates: vec![
                LockedUpdateSkill {
                    remote_hash: Some(alpha_hash.to_string()),
                    ..locked_update_skill("alpha", "https://github.com/owner/repo.git")
                },
                locked_update_skill("beta", "https://github.com/owner/repo.git"),
            ],
            skills: vec![
                crate::application::update_planner::LocalUpdateSkillInspection {
                    skill_name: "alpha".to_string(),
                    observed_digest: "alpha-observed".to_string(),
                    placement_agent_ids: Vec::new(),
                    clean_copies: Vec::new(),
                    conflicts: Vec::new(),
                    blocking_reasons: Vec::new(),
                },
                crate::application::update_planner::LocalUpdateSkillInspection {
                    skill_name: "beta".to_string(),
                    observed_digest: "beta-observed".to_string(),
                    placement_agent_ids: Vec::new(),
                    clean_copies: Vec::new(),
                    conflicts: Vec::new(),
                    blocking_reasons: Vec::new(),
                },
            ],
        }
    }

    #[tokio::test]
    async fn local_lock_drift_after_group_acquisition_does_not_block_stable_skill() {
        let manager = Arc::new(PayloadSessionManager::in_memory(
            PayloadSessionLimits {
                ttl_ms: 60_000,
                max_sessions: 4,
                max_bytes: 1_000_000,
            },
            || 1_000,
        ));
        let source = tempdir().unwrap();
        for name in ["alpha", "beta"] {
            let root = source.path().join(name);
            fs::create_dir_all(&root).unwrap();
            fs::write(root.join("SKILL.md"), format!("---\nname: {name}\n---\n")).unwrap();
        }
        let discovery = manager
            .discover(EnvironmentRef::Host, "source")
            .await
            .unwrap();
        let mut payloads = Vec::new();
        for name in ["alpha", "beta"] {
            payloads.push((
                name.to_string(),
                manager
                    .acquire_payload(
                        &discovery,
                        format!("skills/{name}"),
                        build_skill_payload(&source.path().join(name)).unwrap(),
                    )
                    .await
                    .unwrap(),
            ));
        }
        let token = PreviewToken {
            generation: "preview-initial".to_string(),
            registry_revision: "registry-1".to_string(),
            environment_revision: "environment-1".to_string(),
            context_revision: ContextSnapshotRevision::parse("context-v1-demo").unwrap(),
        };
        let facts = SourceSnapshotFacts {
            discovery_session: discovery,
            snapshot_id: RemoteSnapshotId::new(
                NormalizedRef::Named("main".to_string()),
                "main",
                "revision-1",
            ),
            complete_skill_path_catalog: Default::default(),
        };
        let calls = Arc::new(AtomicUsize::new(0));
        let builds = Arc::new(AtomicUsize::new(0));
        let executed = Arc::new(AtomicUsize::new(0));
        let service = UpdateService::new(
            Arc::clone(&manager),
            SequencedPlanner {
                inspections: Mutex::new(vec![
                    inspection(token.clone(), "old"),
                    inspection(
                        PreviewToken {
                            generation: "preview-latest".to_string(),
                            ..token.clone()
                        },
                        "changed",
                    ),
                ]),
                builds: Arc::clone(&builds),
            },
            FixedAcquirer {
                calls: Arc::clone(&calls),
                expected_group_count: 1,
                acquisitions: Mutex::new(Some(vec![UpdateSourceAcquisition {
                    source_result_id: "source-1".to_string(),
                    source: "owner/repo".to_string(),
                    skill_names: vec!["alpha".to_string(), "beta".to_string()],
                    result: Ok(AcquiredUpdateSource { facts, payloads }),
                }])),
            },
            ResultExecutor {
                calls: Arc::clone(&executed),
                expected_payload_count: 1,
                results: vec![mutation_result("beta", MutationUnitStatus::Succeeded)],
            },
        );

        let mut stages = Vec::new();
        let response = service
            .execute_with_stage_observer(
                &UpdateExecutionRequest {
                    request: UpdateRequest {
                        context: ContextRef {
                            environment: EnvironmentRef::Host,
                            scope: ContextScope::Global,
                        },
                        skill_names: vec!["alpha".to_string(), "beta".to_string()],
                    },
                    overwrite_private_entries: Vec::new(),
                },
                token,
                CancellationSignal::default(),
                |stage| stages.push(stage),
            )
            .await
            .unwrap();

        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(builds.load(Ordering::SeqCst), 1);
        assert_eq!(executed.load(Ordering::SeqCst), 1);
        assert_eq!(
            stages,
            vec![
                UpdateExecutionStage::Validating,
                UpdateExecutionStage::Updating
            ]
        );
        assert!(matches!(
            response
                .skills
                .iter()
                .find(|skill| skill.skill_identity.skill_name == "alpha")
                .unwrap()
                .coverage,
            UpdateCoverage::NotUpdated { .. }
        ));
        assert_eq!(
            response
                .skills
                .iter()
                .find(|skill| skill.skill_identity.skill_name == "beta")
                .unwrap()
                .coverage,
            UpdateCoverage::Updated
        );
        assert_eq!(response.outcome, UpdateOutcome::Partial);
    }

    #[tokio::test]
    async fn mixed_source_failure_keeps_the_other_source_executable_in_one_coordinator_run() {
        let manager = Arc::new(PayloadSessionManager::in_memory(
            PayloadSessionLimits {
                ttl_ms: 60_000,
                max_sessions: 4,
                max_bytes: 1_000_000,
            },
            || 1_000,
        ));
        let source = tempdir().unwrap();
        let alpha = source.path().join("alpha");
        fs::create_dir_all(&alpha).unwrap();
        fs::write(alpha.join("SKILL.md"), "---\nname: alpha\n---\n").unwrap();
        let discovery = manager
            .discover(EnvironmentRef::Host, "source")
            .await
            .unwrap();
        let alpha_handle = manager
            .acquire_payload(
                &discovery,
                "skills/alpha".to_string(),
                build_skill_payload(&alpha).unwrap(),
            )
            .await
            .unwrap();
        let facts = SourceSnapshotFacts {
            discovery_session: discovery,
            snapshot_id: RemoteSnapshotId::new(
                NormalizedRef::Named("main".to_string()),
                "main",
                "revision-1",
            ),
            complete_skill_path_catalog: Default::default(),
        };
        let token = PreviewToken {
            generation: "preview-initial".to_string(),
            registry_revision: "registry-1".to_string(),
            environment_revision: "environment-1".to_string(),
            context_revision: ContextSnapshotRevision::parse("context-v1-demo").unwrap(),
        };
        let inspection = LocalUpdateInspection {
            token: token.clone(),
            source_candidates: vec![
                locked_update_skill("alpha", "https://github.com/owner/alpha.git"),
                locked_update_skill("beta", "https://github.com/owner/beta.git"),
            ],
            skills: vec![
                crate::application::update_planner::LocalUpdateSkillInspection {
                    skill_name: "alpha".to_string(),
                    observed_digest: "alpha-observed".to_string(),
                    placement_agent_ids: Vec::new(),
                    clean_copies: Vec::new(),
                    conflicts: Vec::new(),
                    blocking_reasons: Vec::new(),
                },
                crate::application::update_planner::LocalUpdateSkillInspection {
                    skill_name: "beta".to_string(),
                    observed_digest: "beta-observed".to_string(),
                    placement_agent_ids: Vec::new(),
                    clean_copies: Vec::new(),
                    conflicts: Vec::new(),
                    blocking_reasons: Vec::new(),
                },
            ],
        };
        let acquire_calls = Arc::new(AtomicUsize::new(0));
        let builds = Arc::new(AtomicUsize::new(0));
        let executor_calls = Arc::new(AtomicUsize::new(0));
        let service = UpdateService::new(
            Arc::clone(&manager),
            SequencedPlanner {
                inspections: Mutex::new(vec![inspection.clone(), inspection]),
                builds: Arc::clone(&builds),
            },
            FixedAcquirer {
                calls: Arc::clone(&acquire_calls),
                expected_group_count: 2,
                acquisitions: Mutex::new(Some(vec![
                    UpdateSourceAcquisition {
                        source_result_id: "source-1".to_string(),
                        source: "owner/alpha".to_string(),
                        skill_names: vec!["alpha".to_string()],
                        result: Ok(AcquiredUpdateSource {
                            facts,
                            payloads: vec![("alpha".to_string(), alpha_handle)],
                        }),
                    },
                    UpdateSourceAcquisition {
                        source_result_id: "source-2".to_string(),
                        source: "owner/beta".to_string(),
                        skill_names: vec!["beta".to_string()],
                        result: Err(AppError::GitCloneFailed {
                            message: "unavailable".to_string(),
                        }),
                    },
                ])),
            },
            ResultExecutor {
                calls: Arc::clone(&executor_calls),
                expected_payload_count: 1,
                results: vec![mutation_result("alpha", MutationUnitStatus::Succeeded)],
            },
        );

        let response = service
            .execute(
                &UpdateExecutionRequest {
                    request: UpdateRequest {
                        context: ContextRef {
                            environment: EnvironmentRef::Host,
                            scope: ContextScope::Global,
                        },
                        skill_names: vec!["alpha".to_string(), "beta".to_string()],
                    },
                    overwrite_private_entries: Vec::new(),
                },
                token,
                CancellationSignal::default(),
            )
            .await
            .unwrap();

        assert_eq!(acquire_calls.load(Ordering::SeqCst), 1);
        assert_eq!(builds.load(Ordering::SeqCst), 1);
        assert_eq!(executor_calls.load(Ordering::SeqCst), 1);
        assert_eq!(response.outcome, UpdateOutcome::Partial);
        assert_eq!(
            response
                .sources
                .iter()
                .map(|source| source.status)
                .collect::<Vec<_>>(),
            vec![UpdateSourceStatus::Acquired, UpdateSourceStatus::Failed]
        );
        assert_eq!(
            response
                .skills
                .iter()
                .find(|skill| skill.skill_identity.skill_name == "alpha")
                .unwrap()
                .source_result_id,
            "source-1"
        );
        assert_eq!(
            response
                .skills
                .iter()
                .find(|skill| skill.skill_identity.skill_name == "beta")
                .unwrap()
                .source_result_id,
            "source-2"
        );
        assert!(matches!(
            response
                .skills
                .iter()
                .find(|skill| skill.skill_identity.skill_name == "beta")
                .unwrap()
                .coverage,
            UpdateCoverage::NotUpdated { .. }
        ));
    }

    #[test]
    fn successful_conflict_preservation_and_failed_mutation_have_distinct_coverage() {
        let context = ContextRef {
            environment: EnvironmentRef::Host,
            scope: ContextScope::Global,
        };
        let succeeded = mutation_result("demo", MutationUnitStatus::Succeeded);
        let failed = mutation_result("demo", MutationUnitStatus::Failed);

        assert_eq!(
            update_coverage(Some(&succeeded), true, &context).0,
            UpdateCoverage::PreservedConflicts,
        );
        assert!(matches!(
            update_coverage(Some(&failed), false, &context).0,
            UpdateCoverage::NotUpdated { .. }
        ));
    }

    #[test]
    fn earlier_success_and_later_cancellation_remain_partial() {
        let context = ContextRef {
            environment: EnvironmentRef::Host,
            scope: ContextScope::Global,
        };
        let cancellation =
            ErrorReport::from_app_error(AppError::MutationCancelled, Some(context.clone()));
        let skills = vec![
            UpdateSkillResult {
                skill_identity: SkillIdentity {
                    context: context.clone(),
                    skill_name: "alpha".to_string(),
                },
                source_result_id: "source-1".to_string(),
                mutation: Some(mutation_result("alpha", MutationUnitStatus::Succeeded)),
                coverage: UpdateCoverage::Updated,
                warnings: Vec::new(),
                retryable: false,
            },
            not_updated_skill(
                &context,
                "beta".to_string(),
                "source-2".to_string(),
                cancellation,
            ),
        ];

        assert_eq!(update_outcome(&skills), UpdateOutcome::Partial);
    }

    #[tokio::test]
    async fn stale_preview_token_is_rejected_before_acquisition() {
        let manager = Arc::new(PayloadSessionManager::in_memory(
            PayloadSessionLimits {
                ttl_ms: 60_000,
                max_sessions: 4,
                max_bytes: 1_000_000,
            },
            || 1_000,
        ));
        let token = PreviewToken {
            generation: "preview-v1-demo".to_string(),
            registry_revision: "registry-1".to_string(),
            environment_revision: "environment-1".to_string(),
            context_revision: ContextSnapshotRevision::parse("context-v1-demo").unwrap(),
        };
        let request = UpdateRequest {
            context: ContextRef {
                environment: EnvironmentRef::Host,
                scope: ContextScope::Global,
            },
            skill_names: vec!["demo".to_string()],
        };
        let rebuilds = Arc::new(AtomicUsize::new(0));
        let acquisitions = Arc::new(AtomicUsize::new(0));
        let executions = Arc::new(AtomicUsize::new(0));
        let service = UpdateService::new(
            Arc::clone(&manager),
            Planner {
                token: token.clone(),
                rebuilds: Arc::clone(&rebuilds),
            },
            Acquirer(Arc::clone(&acquisitions)),
            Executor(Arc::clone(&executions)),
        );
        let preview = service.preview(&request).await.unwrap();
        let serialized_preview = serde_json::to_value(&preview).unwrap();
        assert!(serialized_preview["skills"][0].get("payload").is_none());
        let execution = UpdateExecutionRequest {
            request,
            overwrite_private_entries: Vec::new(),
        };
        let mut changed_token = token;
        changed_token.generation = "preview-v1-changed".to_string();

        assert!(matches!(
            service
                .execute(&execution, changed_token, CancellationSignal::default())
                .await,
            Err(AppError::StaleContext)
        ));
        assert_eq!(acquisitions.load(Ordering::SeqCst), 0);
        assert_eq!(rebuilds.load(Ordering::SeqCst), 0);
        assert_eq!(executions.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn source_failure_is_referenced_without_a_fake_mutation_result() {
        let manager = Arc::new(PayloadSessionManager::in_memory(
            PayloadSessionLimits {
                ttl_ms: 60_000,
                max_sessions: 4,
                max_bytes: 1_000_000,
            },
            || 1_000,
        ));
        let token = PreviewToken {
            generation: "preview-v1-demo".to_string(),
            registry_revision: "registry-1".to_string(),
            environment_revision: "environment-1".to_string(),
            context_revision: ContextSnapshotRevision::parse("context-v1-demo").unwrap(),
        };
        let request = UpdateRequest {
            context: ContextRef {
                environment: EnvironmentRef::Host,
                scope: ContextScope::Global,
            },
            skill_names: vec!["demo".to_string()],
        };
        let service = UpdateService::new(
            manager,
            Planner {
                token: token.clone(),
                rebuilds: Arc::new(AtomicUsize::new(0)),
            },
            FailingAcquirer,
            Executor(Arc::new(AtomicUsize::new(0))),
        );

        let response = service
            .execute(
                &UpdateExecutionRequest {
                    request,
                    overwrite_private_entries: Vec::new(),
                },
                token,
                CancellationSignal::default(),
            )
            .await
            .unwrap();

        assert_eq!(response.sources.len(), 1);
        assert_eq!(response.sources[0].status, UpdateSourceStatus::Failed);
        assert_eq!(response.skills[0].source_result_id, response.sources[0].id);
        assert!(response.skills[0].mutation.is_none());
        assert!(matches!(
            response.skills[0].coverage,
            UpdateCoverage::NotUpdated { .. }
        ));
        assert_eq!(response.outcome, UpdateOutcome::Failed);
    }

    #[test]
    fn cancelled_source_acquisition_sets_cancelled_outcome_without_a_mutation_unit() {
        let context = ContextRef {
            environment: EnvironmentRef::Host,
            scope: ContextScope::Global,
        };
        let cancelled =
            ErrorReport::from_app_error(AppError::MutationCancelled, Some(context.clone()));
        let skills = vec![UpdateSkillResult {
            skill_identity: SkillIdentity {
                context,
                skill_name: "demo".to_string(),
            },
            source_result_id: "source-1".to_string(),
            mutation: None,
            coverage: UpdateCoverage::NotUpdated { error: cancelled },
            warnings: Vec::new(),
            retryable: true,
        }];

        assert_eq!(update_outcome(&skills), UpdateOutcome::Cancelled);
    }

    #[test]
    fn acquisition_groups_share_only_equivalent_source_descriptors() {
        let context = ContextRef {
            environment: EnvironmentRef::Host,
            scope: ContextScope::Global,
        };
        let shared = acquisition_groups(
            &context,
            vec![
                locked_update_skill("alpha", "https://github.com/owner/repo.git"),
                locked_update_skill("beta", "https://github.com/owner/repo.git"),
            ],
        )
        .unwrap();
        assert_eq!(shared.len(), 1);
        assert_eq!(shared[0].skills.len(), 2);

        let distinct = acquisition_groups(
            &context,
            vec![
                locked_update_skill("alpha", "https://github.com/owner/repo"),
                locked_update_skill("beta", "https://github.com/owner/repo.git"),
            ],
        )
        .unwrap();
        assert_eq!(distinct.len(), 2);
    }
}

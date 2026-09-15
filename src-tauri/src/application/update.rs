use std::collections::BTreeSet;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::application::agent_intent::AgentTargetFallbackPreview;
use crate::application::mutation::coordinator::MutationUnitObserver;
use crate::application::mutation::executor::MutationPlanExecutor;
use crate::application::mutation::plan::{MutationPlan, PreviewToken};
use crate::application::mutation::planning::validate_exact_preview;
use crate::application::mutation::result::{
    ErrorReport, MutationUnitResult, MutationUnitStatus, OperationErrorCode,
};
#[cfg(test)]
use crate::application::payload_session::AcquiredPayloadHandle;
use crate::application::payload_session::PayloadSessionManager;
use crate::application::resources::SkillIdentity;
use crate::application::skill_changes::ValidatedSkillPayload;
use crate::application::skill_entry_projection::ObservedEntryReader;
use crate::application::skill_source::{
    AcquiredSavedSkillSource, SavedSkillSource, SavedSkillSourceAcquisition, SavedSkillSourceGroup,
    SkillSourceModule,
};
use crate::application::source_evidence::{EvidenceAttempt, EvidenceFreshness};
use crate::application::update_planner::LocalUpdateInspection;
#[cfg(test)]
use crate::application::update_planner::LockedUpdateSkill;
use crate::core::mutation::CancellationSignal;
use crate::core::source_identity::{NormalizedRef, SourceIdentity};
use crate::environment::runtime::ObservedEntryId;
use crate::environment::types::SkillLocationRef;
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comparison_fingerprint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<AppError>,
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
    Skills(Vec<SkillIdentity>),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct UpdateCheckRequest {
    pub context: SkillLocationRef,
    pub mode: UpdateCheckMode,
    pub selection: UpdateCheckSelection,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct SourceUpdateCheckInfo {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<crate::core::source_identity::SourceProvider>,
    pub source: String,
    pub requested_ref: Option<String>,
    pub resolved_ref: Option<String>,
    pub ref_revision: Option<String>,
    pub checked_at_epoch_ms: Option<u64>,
    pub expires_at_epoch_ms: Option<u64>,
    pub freshness: EvidenceFreshness,
    pub last_attempt: Option<EvidenceAttempt>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<AppError>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub enum UpdateCheckOutcome {
    Completed,
    Partial,
    NotCompleted,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct UpdateCheckResponse {
    pub outcome: UpdateCheckOutcome,
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
        Some("missing-source") => Some(UpdateCapabilityReasonCode::MissingSource),
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
    pub context: SkillLocationRef,
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
pub struct UpdatePreparationIssue {
    pub skill_name: String,
    pub error: AppError,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct PreparedUpdatePreview {
    pub sources: Vec<UpdateSourcePreview>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path_base: Option<crate::environment::context_resolver::ScopePathBase>,
    pub skills: Vec<UpdateSkillPreview>,
    pub blocked: Vec<UpdatePreparationIssue>,
    pub redirected_download_hosts: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct UpdateSourcePreview {
    pub source_key: String,
    pub source_display: String,
    pub ref_display: String,
    pub skill_names: Vec<String>,
    pub error: Option<AppError>,
}

pub struct PreparedUpdate {
    pub request: UpdateRequest,
    pub preview: PreparedUpdatePreview,
    items: Vec<PreparedUpdateItem>,
    sources: Vec<UpdateSourceResult>,
    source_by_skill: std::collections::BTreeMap<String, String>,
}

struct PreparedUpdateItem {
    name: String,
    source_result_id: String,
    handle: crate::application::payload_session::AcquiredPayloadHandle,
    plan: MutationPlan,
}

impl PreparedUpdate {
    pub fn expires_at_epoch_ms(&self) -> Option<u64> {
        self.items
            .iter()
            .map(|item| item.handle.expires_at_epoch_ms)
            .min()
    }

    pub fn expire_payloads(&mut self, now: u64) {
        self.items.retain(|item| {
            if item.handle.expires_at_epoch_ms > now {
                return true;
            }
            // 保留确认时允许的覆盖选择；过期只撤销该项的执行内容。
            self.preview.blocked.push(UpdatePreparationIssue {
                skill_name: item.name.clone(),
                error: AppError::StalePayload,
            });
            false
        });
    }
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct UpdateSkillPreview {
    pub linked_targets: Vec<UpdateLinkedTargetPreview>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_key: Option<String>,
    pub skill_name: String,
    pub source_display: String,
    pub ref_display: String,
    pub adapter_targets: Vec<ObservedEntryReader>,
    pub capability: CheckUpdateCapability,
    pub clean_copy_count: usize,
    pub targets: Vec<UpdateTargetPreview>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preserved_targets: Option<Vec<UpdateTargetPreview>>,
    pub overwrite_private_entries: Vec<UpdateConflictCopyPreview>,
    pub blocking_reasons: Vec<OperationErrorCode>,
    pub fallback_forecasts: Vec<AgentTargetFallbackPreview>,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct UpdateConflictCopyPreview {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_standard: Option<bool>,
    pub entry_id: ObservedEntryId,
    pub readers: Vec<ObservedEntryReader>,
    pub display_path: crate::environment::types::ResourceLocator,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct UpdateTargetPreview {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selectable_entry_id: Option<ObservedEntryId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_standard: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<crate::application::skill_entry_projection::ObservedEntryKind>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub link_target: Option<crate::environment::types::ResourceLocator>,
    pub display_path: crate::environment::types::ResourceLocator,
    pub readers: Vec<ObservedEntryReader>,
    pub restoring: bool,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct UpdateLinkedTargetPreview {
    pub display_path: crate::environment::types::ResourceLocator,
    pub readers: Vec<ObservedEntryReader>,
    pub is_standard: bool,
    pub target_path: crate::environment::types::ResourceLocator,
    pub target_copy_entry_id: Option<ObservedEntryId>,
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
    SkippedCopy,
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
    UpdatedWithSkippedCopies,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skipped_copy_paths: Option<Vec<crate::environment::types::ResourceLocator>>,
    pub skill_identity: SkillIdentity,
    pub source_result_id: String,
    pub mutation: Option<MutationUnitResult>,
    pub coverage: UpdateCoverage,
    pub warnings: Vec<UpdateWarningCode>,
    pub retryable: bool,
}

pub type UpdateFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

pub trait UpdatePlanner: Send + Sync {
    fn inspect_items<'a>(
        &'a self,
        request: &'a UpdateRequest,
    ) -> UpdateFuture<'a, Vec<(String, Result<LocalUpdateInspection, AppError>)>>;

    fn build<'a>(
        &'a self,
        execution: &'a UpdateExecutionRequest,
        payloads: Vec<ValidatedSkillPayload>,
    ) -> UpdateFuture<'a, Result<(PreviewToken, MutationPlan), AppError>>;
}

pub type UpdateAcquisitionGroup = SavedSkillSourceGroup;
pub type AcquiredUpdateSource = AcquiredSavedSkillSource;
pub type UpdateSourceAcquisition = SavedSkillSourceAcquisition;

pub struct UpdateService<P, A, E> {
    payloads: Arc<PayloadSessionManager>,
    planner: P,
    skill_source: A,
    executor: E,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateExecutionStage {
    Validating,
    Updating,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateExecutionProgress {
    pub stage: UpdateExecutionStage,
    pub subject: Option<String>,
    pub current: Option<u32>,
    pub total: Option<u32>,
}

impl<P, A, E> UpdateService<P, A, E>
where
    P: UpdatePlanner,
    A: SkillSourceModule,
    E: MutationPlanExecutor,
{
    pub fn new(
        payloads: Arc<PayloadSessionManager>,
        planner: P,
        skill_source: A,
        executor: E,
    ) -> Self {
        Self {
            payloads,
            planner,
            skill_source,
            executor,
        }
    }

    pub async fn prepare(
        &self,
        request: &UpdateRequest,
        cancellation: CancellationSignal,
    ) -> Result<PreparedUpdate, AppError> {
        validate_update_request(request)?;
        let inspections = self
            .planner
            .inspect_items(request)
            .await
            .into_iter()
            .map(|(name, result)| {
                let result = result.and_then(|inspection| {
                    for skill in &inspection.source_candidates {
                        SourceIdentity::from_metadata(&skill.metadata())?;
                    }
                    Ok(inspection)
                });
                (name, result)
            })
            .collect::<Vec<_>>();
        let saved = inspections
            .iter()
            .filter_map(|(_, item)| item.as_ref().ok())
            .flat_map(|item| item.source_candidates.iter())
            .map(|skill| SavedSkillSource {
                name: skill.name.clone(),
                metadata: skill.metadata(),
            })
            .collect::<Vec<_>>();
        let groups = crate::application::skill_source::group_saved_skills(
            &request.context.environment,
            saved,
        )?;
        let executable_names = inspections
            .iter()
            .filter_map(|(_, item)| item.as_ref().ok())
            .flat_map(|item| &item.skills)
            .filter(|skill| skill.blocking_reasons.is_empty())
            .map(|skill| skill.skill_name.as_str())
            .collect::<BTreeSet<_>>();
        let acquisition_groups = groups
            .iter()
            .cloned()
            .filter_map(|mut group| {
                group
                    .skills
                    .retain(|skill| executable_names.contains(skill.name.as_str()));
                (!group.skills.is_empty()).then_some(group)
            })
            .collect::<Vec<_>>();
        let acquisitions = if acquisition_groups.is_empty() {
            Vec::new()
        } else {
            match self
                .skill_source
                .acquire_saved_groups(&acquisition_groups, cancellation.clone())
                .await
            {
                Ok(sources) => sources,
                Err(AppError::MutationCancelled) => acquisition_groups
                    .iter()
                    .map(|group| SavedSkillSourceAcquisition {
                        source_result_id: group.source_result_id.clone(),
                        source: group.source.clone(),
                        skill_names: group
                            .skills
                            .iter()
                            .map(|skill| skill.name.clone())
                            .collect(),
                        result: Err(AppError::MutationCancelled),
                    })
                    .collect(),
                Err(error) => return Err(error),
            }
        };
        let source_previews = groups
            .iter()
            .map(|group| {
                let identity = SourceIdentity::from_metadata(&group.skills[0].metadata)?;
                let acquired = acquisitions.iter().find(|source| {
                    group
                        .skills
                        .iter()
                        .any(|skill| source.skill_names.contains(&skill.name))
                });
                Ok(UpdateSourcePreview {
                    source_key: acquired.map_or_else(
                        || group.source_result_id.clone(),
                        |source| source.source_result_id.clone(),
                    ),
                    source_display: group.source.clone(),
                    ref_display: match identity.normalized_ref() {
                        NormalizedRef::Named(value) => value.clone(),
                        NormalizedRef::Default => String::new(),
                    },
                    skill_names: group
                        .skills
                        .iter()
                        .map(|skill| skill.name.clone())
                        .collect(),
                    error: acquired
                        .and_then(|source| source.result.as_ref().err())
                        .cloned(),
                })
            })
            .collect::<Result<Vec<_>, AppError>>()?;
        let mut prepared = PreparedUpdate {
            request: request.clone(),
            preview: PreparedUpdatePreview {
                sources: source_previews,
                path_base: inspections.iter().find_map(|(_, item)| {
                    item.as_ref()
                        .ok()
                        .and_then(|inspection| inspection.path_base.clone())
                }),
                skills: Vec::new(),
                blocked: Vec::new(),
                redirected_download_hosts: Vec::new(),
            },
            items: Vec::new(),
            sources: Vec::new(),
            source_by_skill: groups
                .iter()
                .flat_map(|source| {
                    source.skills.iter().map(|skill| {
                        (
                            skill.name.clone(),
                            acquisitions
                                .iter()
                                .find(|acquired| acquired.skill_names.contains(&skill.name))
                                .map_or_else(
                                    || source.source_result_id.clone(),
                                    |acquired| acquired.source_result_id.clone(),
                                ),
                        )
                    })
                })
                .collect(),
        };
        let mut hosts = BTreeSet::new();
        for source in &acquisitions {
            if let Ok(content) = &source.result {
                hosts.extend(content.redirected_download_hosts.iter().cloned());
            }
            prepared.sources.push(UpdateSourceResult {
                id: source.source_result_id.clone(),
                source: source.source.clone(),
                status: if source.result.is_ok() {
                    UpdateSourceStatus::Acquired
                } else {
                    UpdateSourceStatus::Failed
                },
                error: source
                    .result
                    .as_ref()
                    .err()
                    .cloned()
                    .map(|error| ErrorReport::from_app_error(error, Some(request.context.clone()))),
            });
        }
        prepared.preview.redirected_download_hosts = hosts.into_iter().collect();
        for (name, inspection) in inspections {
            if cancellation.is_cancelled() {
                return Err(AppError::MutationCancelled);
            }
            let outcome = async {
                let inspection = inspection?;
                let mut preview = previews_from_inspection(inspection.clone())?
                    .into_iter()
                    .next()
                    .ok_or(AppError::StaleTarget)?;
                preview.source_key = prepared.source_by_skill.get(&name).cloned();
                if preview.blocking_reasons == [OperationErrorCode::NoUpdateTargets] {
                    return Ok((preview, None));
                }
                if !preview.blocking_reasons.is_empty() {
                    return Err(AppError::StaleTarget);
                }
                let source = acquisitions
                    .iter()
                    .find(|source| source.skill_names.contains(&name))
                    .ok_or(AppError::StalePayload)?;
                let content = source.result.as_ref().map_err(Clone::clone)?;
                let payload = content
                    .validate_member(self.payloads.as_ref(), &request.context.environment, &name)
                    .await?;
                let handle = payload.handle().clone();
                let execution = UpdateExecutionRequest {
                    request: UpdateRequest {
                        context: request.context.clone(),
                        skill_names: vec![name.clone()],
                    },
                    overwrite_private_entries: inspection
                        .skills
                        .iter()
                        .flat_map(|skill| skill.clean_copies.iter().chain(&skill.conflicts))
                        .map(|entry| entry.entry_id.clone())
                        .collect(),
                };
                let (token, plan) = self.planner.build(&execution, vec![payload]).await?;
                validate_exact_preview(&inspection.token, &token)?;
                Ok::<_, AppError>((
                    preview,
                    Some(PreparedUpdateItem {
                        name: name.clone(),
                        source_result_id: source.source_result_id.clone(),
                        handle: handle.clone(),
                        plan,
                    }),
                ))
            }
            .await;
            match outcome {
                Ok((preview, item)) => {
                    prepared.preview.skills.push(preview);
                    if let Some(item) = item {
                        prepared.items.push(item);
                    }
                }
                Err(error) => prepared.preview.blocked.push(UpdatePreparationIssue {
                    skill_name: name,
                    error,
                }),
            }
        }
        // 共享的只读观察可以复用；同一物理位置涉及写入时，仍阻断相互竞争的 Skill。
        let mut owners = std::collections::BTreeMap::<_, (BTreeSet<String>, bool)>::new();
        let mut collisions = BTreeSet::new();
        for item in &prepared.items {
            for entry in item
                .plan
                .units
                .iter()
                .flat_map(|unit| unit.primary_entry.iter().chain(&unit.additional_entries))
            {
                let (names, has_write) = owners.entry(entry.key.clone()).or_default();
                let writes = !matches!(
                    entry.action,
                    crate::application::mutation::plan::PreparedEntryAction::Keep
                );
                if (*has_write || writes) && names.iter().any(|name| name != &item.name) {
                    collisions.extend(names.iter().cloned());
                    collisions.insert(item.name.clone());
                }
                names.insert(item.name.clone());
                *has_write |= writes;
            }
        }
        let mut valid = Vec::new();
        for item in prepared.items {
            let error = if collisions.contains(&item.name) {
                Some(AppError::StaleTarget)
            } else {
                self.payloads.pin_verified(&item.handle).await.err()
            };
            if let Some(error) = error {
                prepared
                    .preview
                    .skills
                    .retain(|skill| skill.skill_name != item.name);
                prepared.preview.blocked.push(UpdatePreparationIssue {
                    skill_name: item.name,
                    error,
                });
            } else {
                valid.push(item);
            }
        }
        prepared.items = valid;
        if cancellation.is_cancelled() {
            return Err(AppError::MutationCancelled);
        }
        Ok(prepared)
    }

    pub async fn execute_prepared<F>(
        &self,
        prepared: PreparedUpdate,
        overwrite: &[ObservedEntryId],
        cancellation: CancellationSignal,
        observe: F,
    ) -> Result<UpdateResponse, AppError>
    where
        F: Fn(UpdateExecutionProgress) + Send + Sync,
    {
        use crate::application::mutation::plan::PreparedEntryAction;
        let selected = overwrite.iter().cloned().collect::<BTreeSet<_>>();
        let selectable = prepared
            .preview
            .skills
            .iter()
            .flat_map(|skill| {
                skill
                    .targets
                    .iter()
                    .filter_map(|target| target.selectable_entry_id.clone())
                    .chain(
                        skill
                            .overwrite_private_entries
                            .iter()
                            .map(|entry| entry.entry_id.clone()),
                    )
            })
            .collect::<BTreeSet<_>>();
        if selected.len() != overwrite.len() || !selected.is_subset(&selectable) {
            return Err(AppError::StaleTarget);
        }
        let mut results = prepared
            .preview
            .blocked
            .iter()
            .map(|issue| {
                not_updated_skill(
                    &prepared.request.context,
                    issue.skill_name.clone(),
                    prepared
                        .source_by_skill
                        .get(&issue.skill_name)
                        .cloned()
                        .unwrap_or_default(),
                    ErrorReport::from_app_error(
                        issue.error.clone(),
                        Some(prepared.request.context.clone()),
                    ),
                )
            })
            .collect::<Vec<_>>();
        let total = u32::try_from(prepared.items.len()).unwrap_or(u32::MAX);
        results.extend(
            prepared
                .preview
                .skills
                .iter()
                .filter(|skill| skill.blocking_reasons == [OperationErrorCode::NoUpdateTargets])
                .map(|skill| {
                    not_updated_skill(
                        &prepared.request.context,
                        skill.skill_name.clone(),
                        prepared
                            .source_by_skill
                            .get(&skill.skill_name)
                            .cloned()
                            .unwrap_or_default(),
                        ErrorReport::new(OperationErrorCode::NoUpdateTargets),
                    )
                }),
        );
        for (index, mut item) in prepared.items.into_iter().enumerate() {
            let skipped_paths = prepared
                .preview
                .skills
                .iter()
                .filter(|skill| skill.skill_name == item.name)
                .flat_map(|skill| {
                    skill
                        .targets
                        .iter()
                        .filter_map(|target| {
                            target
                                .selectable_entry_id
                                .as_ref()
                                .map(|id| (id, &target.display_path))
                        })
                        .chain(
                            skill
                                .overwrite_private_entries
                                .iter()
                                .map(|entry| (&entry.entry_id, &entry.display_path)),
                        )
                })
                .filter(|(id, _)| !selected.contains(id))
                .map(|(_, path)| path.clone())
                .collect::<Vec<_>>();
            let result = async {
                if cancellation.is_cancelled() {
                    return Err(AppError::MutationCancelled);
                }
                observe(UpdateExecutionProgress {
                    stage: UpdateExecutionStage::Validating,
                    subject: Some(item.name.clone()),
                    current: Some(index as u32),
                    total: Some(total),
                });
                self.payloads.pin_verified(&item.handle).await?;
                let mut preserved = false;
                for unit in &mut item.plan.units {
                    for entry in unit
                        .primary_entry
                        .iter_mut()
                        .chain(&mut unit.additional_entries)
                    {
                        let expected = unit
                            .expected_targets
                            .iter()
                            .find(|expected| expected.key == entry.key)
                            .ok_or(AppError::StaleTarget)?;
                        let id = crate::environment::runtime::observed_entry_id(
                            &expected.key,
                            &expected.fingerprint,
                        )?;
                        if selectable.contains(&id) && !selected.contains(&id) {
                            entry.action = PreparedEntryAction::Keep;
                            preserved = true;
                        }
                    }
                }
                let has_write = item
                    .plan
                    .units
                    .iter()
                    .flat_map(|unit| unit.primary_entry.iter().chain(&unit.additional_entries))
                    .any(|entry| !matches!(entry.action, PreparedEntryAction::Keep));
                if !has_write {
                    let mut result = not_updated_skill(
                        &prepared.request.context,
                        item.name.clone(),
                        item.source_result_id.clone(),
                        ErrorReport::new(OperationErrorCode::NoUpdateTargets),
                    );
                    result.skipped_copy_paths =
                        (!skipped_paths.is_empty()).then_some(skipped_paths);
                    return Ok(result);
                }
                observe(UpdateExecutionProgress {
                    stage: UpdateExecutionStage::Updating,
                    subject: Some(item.name.clone()),
                    current: Some(index as u32),
                    total: Some(total),
                });
                let observer: MutationUnitObserver<'_> = Arc::new(|progress| {
                    observe(UpdateExecutionProgress {
                        stage: UpdateExecutionStage::Updating,
                        subject: Some(progress.skill_name),
                        current: Some((index as u32).saturating_add(progress.current)),
                        total: Some(total),
                    })
                });
                let mutation = self
                    .executor
                    .execute_with_observer(item.plan, cancellation.clone(), observer)
                    .await
                    .into_iter()
                    .find(|result| result.skill_name == item.name);
                let (coverage, warnings, retryable) =
                    update_coverage(mutation.as_ref(), preserved, &prepared.request.context);
                Ok::<_, AppError>(UpdateSkillResult {
                    skipped_copy_paths: (!skipped_paths.is_empty()).then_some(skipped_paths),
                    skill_identity: SkillIdentity {
                        context: prepared.request.context.clone(),
                        skill_name: item.name.clone(),
                    },
                    source_result_id: item.source_result_id.clone(),
                    mutation,
                    coverage,
                    warnings,
                    retryable,
                })
            }
            .await;
            results.push(result.unwrap_or_else(|error| {
                not_updated_skill(
                    &prepared.request.context,
                    item.name,
                    item.source_result_id,
                    ErrorReport::from_app_error(error, Some(prepared.request.context.clone())),
                )
            }));
        }
        let outcome = update_outcome(&results);
        Ok(UpdateResponse {
            sources: prepared.sources,
            skills: results,
            outcome,
        })
    }
}

fn not_updated_skill(
    context: &SkillLocationRef,
    skill_name: String,
    source_result_id: String,
    report: ErrorReport,
) -> UpdateSkillResult {
    UpdateSkillResult {
        skipped_copy_paths: None,
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
    context: &SkillLocationRef,
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
                UpdateCoverage::UpdatedWithSkippedCopies,
                vec![UpdateWarningCode::SkippedCopy],
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

fn previews_from_inspection(
    inspection: LocalUpdateInspection,
) -> Result<Vec<UpdateSkillPreview>, AppError> {
    let display_by_name = inspection
        .source_candidates
        .iter()
        .map(|skill| {
            let identity = SourceIdentity::from_metadata(&skill.metadata())?;
            let ref_display = match identity.normalized_ref() {
                _ if identity.remote().provider()
                    == &crate::core::source_identity::SourceProvider::WellKnown =>
                {
                    String::new()
                }
                NormalizedRef::Default => String::new(),
                NormalizedRef::Named(value) => value.clone(),
            };
            Ok((
                skill.name.clone(),
                (
                    skill.capability(),
                    identity.sanitized_display().to_string(),
                    ref_display,
                    identity.key(),
                ),
            ))
        })
        .collect::<Result<std::collections::BTreeMap<_, _>, AppError>>()?;
    inspection
        .skills
        .into_iter()
        .map(|skill| {
            let (capability, source_display, ref_display, source_key) = display_by_name
                .get(&skill.skill_name)
                .cloned()
                .ok_or(AppError::StaleContext)?;
            let location = |id: &ObservedEntryId| skill.locations.iter().find(|location| &location.entry_id == id);
            let selectable_ids = skill.automatic_entries.iter().filter(|entry| {
                entry.kind == crate::application::skill_entry_projection::ObservedEntryKind::Directory
                    && !skill.locations.iter().any(|location| location.entry_id == entry.entry_id && location.is_standard)
            }).chain(&skill.conflicts).map(|entry| entry.entry_id.clone()).collect::<BTreeSet<_>>();
            let target_preview = |entry: crate::application::skill_entry_projection::ObservedPhysicalEntry| {
                let display = location(&entry.entry_id);
                UpdateTargetPreview {
                    selectable_entry_id: selectable_ids.contains(&entry.entry_id).then(|| entry.entry_id.clone()),
                    is_standard: Some(display.is_some_and(|location| location.is_standard)),
                    kind: Some(display.map_or(entry.kind, |location| location.kind)),
                    link_target: display.and_then(|location| location.link_target.clone()),
                    display_path: display.map_or(entry.display_path, |location| location.path.clone()),
                    readers: display.map_or(entry.readers, |location| location.readers.clone()),
                    restoring: entry.kind == crate::application::skill_entry_projection::ObservedEntryKind::Missing,
                }
            };
            let linked_targets = skill.locations.iter().filter_map(|display| {
                let target = display.linked_entry.as_ref()?;
                Some(UpdateLinkedTargetPreview {
                    display_path: display.path.clone(), readers: display.readers.clone(),
                    is_standard: display.is_standard,
                    target_path: location(target).map(|target| target.path.clone())
                        .or_else(|| display.link_target.clone())?,
                    target_copy_entry_id: selectable_ids.contains(target).then(|| target.clone()),
                })
            }).collect();
            Ok(UpdateSkillPreview {
                linked_targets,
                source_key: Some(source_key),
                capability,
                source_display,
                ref_display,
                adapter_targets: skill.adapter_targets,
                skill_name: skill.skill_name,
                clean_copy_count: skill.clean_copies.len(),
                targets: skill.automatic_entries.into_iter().map(target_preview).collect(),
                preserved_targets: (!skill.preserved_entries.is_empty()).then(|| skill.preserved_entries.into_iter().map(target_preview).collect()),
                overwrite_private_entries: skill
                    .conflicts
                    .into_iter()
                    .map(|entry| UpdateConflictCopyPreview {
                        is_standard: Some(location(&entry.entry_id).is_some_and(|location| location.is_standard)),
                        display_path: location(&entry.entry_id).map_or(entry.display_path, |location| location.path.clone()),
                        readers: location(&entry.entry_id).map_or(entry.readers, |location| location.readers.clone()),
                        entry_id: entry.entry_id,
                    })
                    .collect(),
                blocking_reasons: skill.blocking_reasons,
                fallback_forecasts: Vec::new(),
            })
        })
        .collect()
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

fn validation(message: &str) -> AppError {
    AppError::Validation {
        field: Some("request".to_string()),
        message: message.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    use super::*;
    use crate::application::mutation::executor::MutationPlanExecutor;
    use crate::application::mutation::plan::MutationPlan;
    use crate::application::payload_session::DiscoverySessionHandle;
    use crate::application::payload_session::PayloadPlanningMetadata;
    use crate::application::payload_session::{PayloadSessionLimits, PayloadSessionManager};
    use crate::core::mutation::CancellationSignal;
    use crate::core::skill_payload::build_skill_payload;
    use crate::environment::runtime::ContextSnapshotRevision;
    use crate::environment::types::{EnvironmentRef, SkillLocation};
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
                well_known_digest: None,
            });

        assert_eq!(
            capability.reason,
            Some(UpdateCapabilityReasonCode::MissingSource)
        );
    }

    #[test]
    fn update_preview_exposes_locations_without_internal_target_identity() {
        fn observed_entry(
            id: &str,
            path: &str,
        ) -> crate::application::skill_entry_projection::ObservedPhysicalEntry {
            crate::application::skill_entry_projection::ObservedPhysicalEntry {
                entry_id: crate::environment::runtime::ObservedEntryId::parse(id).unwrap(),
                display_path: crate::environment::types::ResourceLocator {
                    environment: EnvironmentRef::Native,
                    native_path: path.to_string(),
                },
                kind: crate::application::skill_entry_projection::ObservedEntryKind::Directory,
                physical_target_key: format!("credential-secret-target-{id}"),
                readers: vec![
                    crate::application::skill_entry_projection::ObservedEntryReader {
                        agent_id: crate::core::agent_definition::AgentId::parse("codex").unwrap(),
                        display_name: "Codex".to_string(),
                        logical_target_id: "codex-private".to_string(),
                    },
                ],
                will_break_if_standard_removed: false,
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
        inspection.skills[0].automatic_entries = inspection.skills[0].clean_copies.clone();
        inspection.skills[0].conflicts =
            vec![observed_entry("entry-v1-conflict", "/agents/conflict")];

        let preview = serde_json::to_value(previews_from_inspection(inspection).unwrap()).unwrap();
        let skill = &preview[0];

        assert_eq!(skill["cleanCopyCount"], serde_json::json!(2));
        assert!(skill.get("cleanCopies").is_none());
        assert_eq!(
            skill["targets"][0]["selectableEntryId"],
            "entry-v1-clean-one"
        );
        assert!(!skill.to_string().contains("credential-secret"));
        assert_eq!(skill["targets"].as_array().unwrap().len(), 2);
        assert_eq!(
            skill["targets"][0]["displayPath"]["nativePath"],
            "/agents/clean-one"
        );
        assert_eq!(
            skill["overwritePrivateEntries"].as_array().unwrap().len(),
            1
        );
        assert_eq!(
            skill["overwritePrivateEntries"][0]["entryId"],
            serde_json::json!("entry-v1-conflict")
        );
        let serialized = skill["overwritePrivateEntries"][0].to_string();
        assert_eq!(
            skill["overwritePrivateEntries"][0]["displayPath"]["nativePath"],
            "/agents/conflict"
        );
        assert!(!serialized.contains("physicalTargetKey"));
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
        inspection.skills[0].adapter_targets = vec![
            crate::application::skill_entry_projection::ObservedEntryReader {
                agent_id: crate::core::agent_definition::AgentId::parse("eve").unwrap(),
                display_name: "Eve".to_string(),
                logical_target_id: "eve:root".to_string(),
            },
        ];
        inspection.skills[0].conflicts = vec![
            crate::application::skill_entry_projection::ObservedPhysicalEntry {
                entry_id: crate::environment::runtime::ObservedEntryId::parse("entry-v1-private")
                    .unwrap(),
                display_path: crate::environment::types::ResourceLocator {
                    environment: EnvironmentRef::Native,
                    native_path: "/agents/private".to_string(),
                },
                kind: crate::application::skill_entry_projection::ObservedEntryKind::Directory,
                physical_target_key: "credential-secret-target".to_string(),
                readers: vec![
                    crate::application::skill_entry_projection::ObservedEntryReader {
                        agent_id: crate::core::agent_definition::AgentId::parse("codex").unwrap(),
                        display_name: "Codex".to_string(),
                        logical_target_id: "codex-private".to_string(),
                    },
                ],
                will_break_if_standard_removed: false,
            },
        ];

        let preview = serde_json::to_value(previews_from_inspection(inspection).unwrap()).unwrap();
        let skill = &preview[0];

        assert_eq!(
            skill["sourceDisplay"],
            serde_json::json!("github.com/owner/repo")
        );
        assert_eq!(skill["refDisplay"], serde_json::json!("release"));
        assert!(skill.get("placementAgentIds").is_none());
        assert_eq!(skill["adapterTargets"].as_array().unwrap().len(), 1);
        assert_eq!(skill["adapterTargets"][0]["displayName"], "Eve");
        assert_eq!(skill["adapterTargets"][0]["logicalTargetId"], "eve:root");
        let serialized = preview.to_string();
        assert!(!serialized.contains("secret-token"));
        assert!(!serialized.contains("sourceUrl"));
        assert_eq!(
            skill["overwritePrivateEntries"][0]["displayPath"]["nativePath"],
            "/agents/private"
        );
        assert!(!serialized.contains("credential-secret"));
    }

    #[test]
    fn update_check_request_serializes_mode_and_typed_selection() {
        let context = SkillLocationRef {
            environment: EnvironmentRef::Native,
            scope: SkillLocation::Global,
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
                    "environment": { "kind": "native" },
                    "scope": { "scope": "global" }
                },
                "mode": "force",
                "selection": { "kind": "skills", "skills": [{
                    "context": {
                        "environment": { "kind": "native" },
                        "scope": { "scope": "global" }
                    },
                    "skillName": "demo"
                }] }
            })
        );
    }

    fn update_test_plan(
        request: &UpdateRequest,
        payloads: Vec<ValidatedSkillPayload>,
        token: &PreviewToken,
        keep_entries: bool,
    ) -> MutationPlan {
        use crate::application::mutation::plan::{
            ExpectedTargetEntry, PreparedEntryAction, PreparedEntryMutation, RuntimeRevisions,
        };
        use crate::application::mutation::planning::{
            assemble_plan, MutationPlanDraft, MutationUnitDraft, PreparedMutationEntries,
        };
        use crate::environment::runtime::{
            EntryFingerprint, ExecutionBackend, PhysicalParentIdentity, PhysicalTargetKey,
        };
        let units = payloads
            .iter()
            .zip(&request.skill_names)
            .map(|(payload, name)| {
                let key = PhysicalTargetKey {
                    backend: if cfg!(windows) {
                        ExecutionBackend::NativeWindows
                    } else {
                        ExecutionBackend::NativeUnix
                    },
                    physical_parent: if cfg!(windows) {
                        PhysicalParentIdentity::Windows {
                            volume_serial: 1,
                            file_id: 2,
                        }
                    } else {
                        PhysicalParentIdentity::Unix {
                            device: 1,
                            inode: 2,
                        }
                    },
                    normalized_final_child_name: name.clone(),
                };
                let fingerprint = EntryFingerprint(format!("entry-{name}"));
                MutationUnitDraft {
                    id: format!("update:{name}"),
                    skill_name: name.clone(),
                    source: None,
                    target: request.context.clone(),
                    expected_revisions: RuntimeRevisions {
                        registry: token.registry_revision.clone(),
                        environment: token.environment_revision.clone(),
                        context: token.context_revision.clone(),
                    },
                    entries: PreparedMutationEntries {
                        primary: Some(PreparedEntryMutation {
                            key: key.clone(),
                            destination: crate::environment::types::ResourceLocator {
                                environment: request.context.environment.clone(),
                                native_path: std::env::temp_dir()
                                    .join("skill-deck-update-test")
                                    .join(name)
                                    .to_string_lossy()
                                    .into_owned(),
                            },
                            action: if keep_entries {
                                PreparedEntryAction::Keep
                            } else {
                                PreparedEntryAction::Replace {
                                    payload_id: payload.manifest().payload_id().clone(),
                                    requested_mode: crate::models::InstallMode::Copy,
                                }
                            },
                            reader_agent_ids: Vec::new(),
                        }),
                        additional: Vec::new(),
                        expected_targets: vec![ExpectedTargetEntry {
                            key,
                            fingerprint,
                            expected_content_manifest_hash: None,
                        }],
                    },
                    lock_mutation: None,
                }
            })
            .collect();
        assemble_plan(MutationPlanDraft {
            kind: crate::core::mutation::MutationKind::Update,
            payloads: payloads
                .into_iter()
                .map(|payload| {
                    (
                        payload.manifest().payload_id().clone(),
                        payload.into_lease(),
                    )
                })
                .collect(),
            units,
        })
    }

    struct Planner {
        token: PreviewToken,
        rebuilds: Arc<AtomicUsize>,
    }

    fn update_test_manager() -> Arc<PayloadSessionManager> {
        Arc::new(PayloadSessionManager::in_memory(
            PayloadSessionLimits {
                ttl_ms: 60_000,
                max_sessions: 4,
                max_bytes: 1_000_000,
            },
            || 1_000,
        ))
    }

    fn update_test_token() -> PreviewToken {
        PreviewToken {
            generation: "preview-initial".to_string(),
            registry_revision: "registry-1".to_string(),
            environment_revision: "environment-1".to_string(),
            context_revision: ContextSnapshotRevision::parse("context-v1-demo").unwrap(),
        }
    }

    fn update_payload_metadata(skill_name: &str) -> PayloadPlanningMetadata {
        PayloadPlanningMetadata {
            skill_name: skill_name.to_string(),
            install_dir_name: skill_name.to_string(),
            source: "owner/repo".to_string(),
            source_type: "github".to_string(),
            source_url: Some("https://github.com/owner/repo.git".to_string()),
            ref_name: Some("main".to_string()),
            skill_path: format!("skills/{skill_name}"),
            plugin_name: None,
            computed_hash: format!("computed-{skill_name}"),
            upstream_revision: Some(format!("tree-{skill_name}")),
            well_known: None,
        }
    }

    async fn acquired_update_source(
        manager: &PayloadSessionManager,
        skill_name: &str,
    ) -> (DiscoverySessionHandle, AcquiredPayloadHandle) {
        let source = tempdir().unwrap();
        let skill = source.path().join(skill_name);
        fs::create_dir_all(&skill).unwrap();
        fs::write(
            skill.join("SKILL.md"),
            format!("---\nname: {skill_name}\ndescription: {skill_name}\n---\n"),
        )
        .unwrap();
        let discovery = manager
            .discover(EnvironmentRef::Native, format!("source-{skill_name}"))
            .await
            .unwrap();
        let handle = manager
            .acquire_payload_with_metadata(
                &discovery,
                format!("skills/{skill_name}"),
                build_skill_payload(&skill).unwrap(),
                update_payload_metadata(skill_name),
            )
            .await
            .unwrap();
        (discovery, handle)
    }

    fn locked_update_skill(name: &str, source_url: &str) -> LockedUpdateSkill {
        LockedUpdateSkill {
            name: name.to_string(),
            lock_key: name.to_string(),
            source: "owner/repo".to_string(),
            source_type: "github".to_string(),
            source_url: Some(source_url.to_string()),
            ref_name: Some("main".to_string()),
            skill_path: format!("skills/{name}"),
            remote_hash: Some("old".to_string()),
            computed_hash: None,
            installed_at: None,
            subagents: None,
            well_known_digest: None,
        }
    }

    impl UpdatePlanner for Planner {
        fn inspect_items<'a>(
            &'a self,
            request: &'a UpdateRequest,
        ) -> UpdateFuture<'a, Vec<(String, Result<LocalUpdateInspection, AppError>)>> {
            Box::pin(async move {
                request
                    .skill_names
                    .iter()
                    .map(|name| {
                        (
                            name.clone(),
                            Ok(LocalUpdateInspection {
                                path_base: None,
                                token: self.token.clone(),
                                source_candidates: vec![locked_update_skill(
                                    name,
                                    "https://github.com/owner/repo.git",
                                )],
                                skills: vec![
                                    crate::application::update_planner::LocalUpdateSkillInspection {
                                        locations: Vec::new(),
                                        skill_name: name.clone(),
                                        adapter_targets: Vec::new(),
                                        automatic_entries: Vec::new(),
                    preserved_entries: Vec::new(),
                                        clean_copies: Vec::new(),
                                        conflicts: Vec::new(),
                                        blocking_reasons: Vec::new(),
                                    },
                                ],
                            }),
                        )
                    })
                    .collect()
            })
        }

        fn build<'a>(
            &'a self,
            execution: &'a UpdateExecutionRequest,
            payloads: Vec<ValidatedSkillPayload>,
        ) -> UpdateFuture<'a, Result<(PreviewToken, MutationPlan), AppError>> {
            self.rebuilds.fetch_add(1, Ordering::SeqCst);
            Box::pin(async move {
                Ok((
                    self.token.clone(),
                    update_test_plan(&execution.request, payloads, &self.token, false),
                ))
            })
        }
    }

    #[test]
    fn cancellation_and_window_replacement_cannot_publish_late_results() {
        use crate::application::update_preparation::{PreparedUpdates, UpdatePreparations};

        let prepared = || {
            PreparedUpdates::Direct(PreparedUpdate {
                request: UpdateRequest {
                    context: SkillLocationRef {
                        environment: EnvironmentRef::Native,
                        scope: SkillLocation::Global,
                    },
                    skill_names: vec!["demo".into()],
                },
                preview: PreparedUpdatePreview {
                    sources: Vec::new(),
                    path_base: None,
                    skills: Vec::new(),
                    blocked: Vec::new(),
                    redirected_download_hosts: Vec::new(),
                },
                items: Vec::new(),
                sources: Vec::new(),
                source_by_skill: Default::default(),
            })
        };
        let registry = UpdatePreparations::default();
        let id = uuid::Uuid::new_v4().to_string();
        let cancelled = registry.begin(id.clone(), "main").unwrap();
        registry.cancel(&id, "main").unwrap();
        assert!(cancelled.cancellation.is_cancelled());

        let replacement = registry.begin(id.clone(), "main").unwrap();
        assert_eq!(
            cancelled.publish(prepared()),
            Err(AppError::MutationCancelled)
        );
        assert!(!replacement.cancellation.is_cancelled());
        assert_eq!(
            registry.cancel(&id, "other-window"),
            Err(AppError::StaleContext)
        );
        replacement.publish(prepared()).unwrap();
        assert!(matches!(
            registry.take(&id, "other-window"),
            Err(AppError::StalePayload)
        ));
        assert!(matches!(
            registry.take(&id, "main"),
            Ok(PreparedUpdates::Direct(_))
        ));
        assert!(matches!(
            registry.take(&id, "main"),
            Err(AppError::StalePayload)
        ));

        let old_window_operation = registry
            .begin(uuid::Uuid::new_v4().to_string(), "main")
            .unwrap();
        let current_window_operation = registry
            .begin(uuid::Uuid::new_v4().to_string(), "main")
            .unwrap();
        assert!(old_window_operation.cancellation.is_cancelled());
        assert_eq!(
            old_window_operation.publish(prepared()),
            Err(AppError::MutationCancelled)
        );
        assert!(!current_window_operation.cancellation.is_cancelled());
        registry.close_window("main");
        assert!(current_window_operation.cancellation.is_cancelled());
        assert_eq!(
            current_window_operation.publish(prepared()),
            Err(AppError::MutationCancelled)
        );
    }

    #[tokio::test]
    async fn expiry_releases_only_the_expired_member_and_executes_other_content() {
        let now = Arc::new(AtomicU64::new(1_000));
        let clock = now.clone();
        let manager = Arc::new(PayloadSessionManager::in_memory(
            PayloadSessionLimits {
                ttl_ms: 100,
                max_sessions: 4,
                max_bytes: 1_000_000,
            },
            move || clock.load(Ordering::SeqCst),
        ));
        let (alpha_source, alpha) = acquired_update_source(&manager, "alpha").await;
        now.store(1_050, Ordering::SeqCst);
        let (beta_source, beta) = acquired_update_source(&manager, "beta").await;
        let planner = Planner {
            token: update_test_token(),
            rebuilds: Arc::new(AtomicUsize::new(0)),
        };
        let context = SkillLocationRef {
            environment: EnvironmentRef::Native,
            scope: SkillLocation::Global,
        };
        let mut items = Vec::new();
        for (name, source, handle) in [
            ("alpha", alpha_source, alpha),
            ("beta", beta_source, beta.clone()),
        ] {
            let payload = ValidatedSkillPayload::validate(
                handle.clone(),
                &source,
                &context.environment,
                name,
                manager.pin_verified(&handle).await.unwrap(),
            )
            .await
            .unwrap();
            let execution = UpdateExecutionRequest {
                request: UpdateRequest {
                    context: context.clone(),
                    skill_names: vec![name.into()],
                },
                overwrite_private_entries: Vec::new(),
            };
            let (_, plan) = planner.build(&execution, vec![payload]).await.unwrap();
            items.push(PreparedUpdateItem {
                name: name.into(),
                source_result_id: "source-1".into(),
                handle,
                plan,
            });
        }
        let selected = ObservedEntryId::parse("entry-v1-alpha-private").unwrap();
        let mut skills = previews_from_inspection(inspection(update_test_token(), "old")).unwrap();
        skills.truncate(1);
        skills[0].skill_name = "alpha".into();
        skills[0].overwrite_private_entries = vec![UpdateConflictCopyPreview {
            is_standard: Some(false),
            entry_id: selected.clone(),
            readers: Vec::new(),
            display_path: crate::environment::types::ResourceLocator {
                environment: context.environment.clone(),
                native_path: "/agents/private".to_string(),
            },
        }];
        let mut prepared = PreparedUpdate {
            request: UpdateRequest {
                context,
                skill_names: vec!["alpha".into(), "beta".into()],
            },
            preview: PreparedUpdatePreview {
                sources: Vec::new(),
                path_base: None,
                skills,
                blocked: Vec::new(),
                redirected_download_hosts: Vec::new(),
            },
            items,
            sources: Vec::new(),
            source_by_skill: Default::default(),
        };
        now.store(1_101, Ordering::SeqCst);
        prepared.expire_payloads(1_101);
        assert_eq!(prepared.items.len(), 1);
        assert_eq!(prepared.preview.blocked[0].skill_name, "alpha");
        assert_eq!(manager.cleanup().await.unwrap(), 1);
        assert!(manager.pin_verified(&beta).await.is_ok());
        let executions = Arc::new(AtomicUsize::new(0));
        let service = UpdateService::new(
            manager,
            planner,
            FailingAcquirer,
            ResultExecutor {
                calls: executions.clone(),
                expected_payload_count: 1,
                results: vec![mutation_result("beta", MutationUnitStatus::Succeeded)],
            },
        );
        let result = service
            .execute_prepared(prepared, &[selected], CancellationSignal::default(), |_| {})
            .await
            .unwrap();
        assert_eq!(result.outcome, UpdateOutcome::Partial);
        assert_eq!(executions.load(Ordering::SeqCst), 1);
        assert!(result
            .skills
            .iter()
            .find(|skill| skill.skill_identity.skill_name == "beta")
            .unwrap()
            .mutation
            .as_ref()
            .is_some_and(|mutation| mutation.status == MutationUnitStatus::Succeeded));
    }

    #[tokio::test]
    async fn prepared_update_executes_without_reacquiring_or_replanning() {
        let manager = update_test_manager();
        let (discovery_session, handle) = acquired_update_source(&manager, "demo").await;
        let calls = Arc::new(AtomicUsize::new(0));
        let builds = Arc::new(AtomicUsize::new(0));
        let executed = Arc::new(AtomicUsize::new(0));
        let service = UpdateService::new(
            manager,
            Planner {
                token: update_test_token(),
                rebuilds: builds.clone(),
            },
            FixedAcquirer {
                calls: calls.clone(),
                expected_group_count: 1,
                acquisitions: Mutex::new(Some(vec![UpdateSourceAcquisition {
                    source_result_id: "source-1".into(),
                    source: "owner/repo".into(),
                    skill_names: vec!["demo".into()],
                    result: Ok(AcquiredUpdateSource {
                        discovery_session,
                        payloads: vec![("demo".into(), handle)],
                        skill_errors: Vec::new(),
                        redirected_download_hosts: Vec::new(),
                        _leases: Vec::new(),
                    }),
                }])),
            },
            ResultExecutor {
                calls: executed.clone(),
                expected_payload_count: 1,
                results: vec![mutation_result("demo", MutationUnitStatus::Succeeded)],
            },
        );
        let request = UpdateRequest {
            context: SkillLocationRef {
                environment: EnvironmentRef::Native,
                scope: SkillLocation::Global,
            },
            skill_names: vec!["demo".into()],
        };
        let prepared = service
            .prepare(&request, CancellationSignal::default())
            .await
            .unwrap();
        assert_eq!(prepared.preview.skills.len(), 1);
        assert!(prepared.preview.blocked.is_empty());
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(builds.load(Ordering::SeqCst), 1);
        assert_eq!(executed.load(Ordering::SeqCst), 0);
        let response = service
            .execute_prepared(prepared, &[], CancellationSignal::default(), |_| {})
            .await
            .unwrap();
        assert_eq!(response.outcome, UpdateOutcome::Succeeded);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(builds.load(Ordering::SeqCst), 1);
        assert_eq!(executed.load(Ordering::SeqCst), 1);
    }

    struct FailingAcquirer;

    impl SkillSourceModule for FailingAcquirer {
        fn acquire_saved_groups<'a>(
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

    struct CancellingAcquirer;

    impl SkillSourceModule for CancellingAcquirer {
        fn acquire_saved_groups<'a>(
            &'a self,
            _groups: &'a [UpdateAcquisitionGroup],
            _cancellation: CancellationSignal,
        ) -> UpdateFuture<'a, Result<Vec<UpdateSourceAcquisition>, AppError>> {
            Box::pin(async { Err(AppError::MutationCancelled) })
        }
    }

    struct Executor(Arc<AtomicUsize>);

    impl MutationPlanExecutor for Executor {
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

    struct PreparedPlanner {
        keep_entries: bool,
        fail_inspection: Option<String>,
        invalid_source: Option<String>,
        builds: Arc<AtomicUsize>,
        build_token: PreviewToken,
    }

    impl UpdatePlanner for PreparedPlanner {
        fn inspect_items<'a>(
            &'a self,
            request: &'a UpdateRequest,
        ) -> UpdateFuture<'a, Vec<(String, Result<LocalUpdateInspection, AppError>)>> {
            Box::pin(async move {
                request
                    .skill_names
                    .iter()
                    .map(|name| {
                        let result = if self.fail_inspection.as_ref() == Some(name) {
                            Err(AppError::Io {
                                message: "associated target unavailable".into(),
                            })
                        } else {
                            let mut value = inspection(update_test_token(), "old");
                            value.source_candidates.retain(|skill| &skill.name == name);
                            for skill in &mut value.source_candidates {
                                skill.source_url =
                                    Some(format!("https://github.com/{}/repo.git", skill.name));
                                if self.invalid_source.as_ref() == Some(&skill.name) {
                                    skill.source_type = "git".into();
                                    skill.source_url = Some("http://".into());
                                }
                            }
                            value.skills.retain(|skill| &skill.skill_name == name);
                            Ok(value)
                        };
                        (name.clone(), result)
                    })
                    .collect()
            })
        }

        fn build<'a>(
            &'a self,
            execution: &'a UpdateExecutionRequest,
            payloads: Vec<ValidatedSkillPayload>,
        ) -> UpdateFuture<'a, Result<(PreviewToken, MutationPlan), AppError>> {
            self.builds.fetch_add(1, Ordering::SeqCst);
            Box::pin(async move {
                Ok((
                    self.build_token.clone(),
                    update_test_plan(
                        &execution.request,
                        payloads,
                        &self.build_token,
                        self.keep_entries,
                    ),
                ))
            })
        }
    }

    struct FixedAcquirer {
        acquisitions: Mutex<Option<Vec<UpdateSourceAcquisition>>>,
        calls: Arc<AtomicUsize>,
        expected_group_count: usize,
    }

    impl SkillSourceModule for FixedAcquirer {
        fn acquire_saved_groups<'a>(
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

    impl MutationPlanExecutor for ResultExecutor {
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
            target: SkillLocationRef {
                environment: EnvironmentRef::Native,
                scope: SkillLocation::Global,
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
            path_base: None,
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
                    locations: Vec::new(),
                    adapter_targets: Vec::new(),
                    automatic_entries: Vec::new(),
                    preserved_entries: Vec::new(),
                    clean_copies: Vec::new(),
                    conflicts: Vec::new(),
                    blocking_reasons: Vec::new(),
                },
                crate::application::update_planner::LocalUpdateSkillInspection {
                    skill_name: "beta".to_string(),
                    locations: Vec::new(),
                    adapter_targets: Vec::new(),
                    automatic_entries: Vec::new(),
                    preserved_entries: Vec::new(),
                    clean_copies: Vec::new(),
                    conflicts: Vec::new(),
                    blocking_reasons: Vec::new(),
                },
            ],
        }
    }

    async fn beta_acquisition(manager: &PayloadSessionManager) -> UpdateSourceAcquisition {
        let (discovery_session, handle) = acquired_update_source(manager, "beta").await;
        UpdateSourceAcquisition {
            source_result_id: "beta-source".into(),
            source: "beta/repo".into(),
            skill_names: vec!["beta".into()],
            result: Ok(AcquiredUpdateSource {
                discovery_session,
                payloads: vec![("beta".into(), handle)],
                skill_errors: Vec::new(),
                redirected_download_hosts: Vec::new(),
                _leases: Vec::new(),
            }),
        }
    }

    #[tokio::test]
    async fn preparation_blocks_a_bad_association_but_keeps_an_independent_skill_ready() {
        let manager = update_test_manager();
        let acquisition = beta_acquisition(&manager).await;
        let executed = Arc::new(AtomicUsize::new(0));
        let service = UpdateService::new(
            manager,
            PreparedPlanner {
                keep_entries: false,
                fail_inspection: Some("alpha".into()),
                invalid_source: None,
                builds: Arc::new(AtomicUsize::new(0)),
                build_token: update_test_token(),
            },
            FixedAcquirer {
                acquisitions: Mutex::new(Some(vec![acquisition])),
                calls: Arc::new(AtomicUsize::new(0)),
                expected_group_count: 1,
            },
            ResultExecutor {
                calls: executed.clone(),
                results: vec![mutation_result("beta", MutationUnitStatus::Succeeded)],
                expected_payload_count: 1,
            },
        );
        let request = UpdateRequest {
            context: mutation_result("beta", MutationUnitStatus::Succeeded).target,
            skill_names: vec!["alpha".into(), "beta".into()],
        };
        let prepared = service
            .prepare(&request, CancellationSignal::default())
            .await
            .unwrap();
        assert_eq!(prepared.preview.skills[0].skill_name, "beta");
        assert_eq!(prepared.preview.blocked[0].skill_name, "alpha");
        assert_eq!(executed.load(Ordering::SeqCst), 0);
        let response = service
            .execute_prepared(prepared, &[], CancellationSignal::default(), |_| {})
            .await
            .unwrap();
        assert_eq!(response.outcome, UpdateOutcome::Partial);
        assert_eq!(executed.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn prepared_update_with_only_kept_entries_does_not_reach_the_executor() {
        let manager = update_test_manager();
        let acquisition = beta_acquisition(&manager).await;
        let executed = Arc::new(AtomicUsize::new(0));
        let service = UpdateService::new(
            manager,
            PreparedPlanner {
                keep_entries: true,
                fail_inspection: None,
                invalid_source: None,
                builds: Arc::new(AtomicUsize::new(0)),
                build_token: update_test_token(),
            },
            FixedAcquirer {
                acquisitions: Mutex::new(Some(vec![acquisition])),
                calls: Arc::new(AtomicUsize::new(0)),
                expected_group_count: 1,
            },
            ResultExecutor {
                calls: executed.clone(),
                results: vec![mutation_result("beta", MutationUnitStatus::Succeeded)],
                expected_payload_count: 1,
            },
        );
        let prepared = service
            .prepare(
                &UpdateRequest {
                    context: mutation_result("beta", MutationUnitStatus::Succeeded).target,
                    skill_names: vec!["beta".into()],
                },
                CancellationSignal::default(),
            )
            .await
            .unwrap();
        let response = service
            .execute_prepared(prepared, &[], CancellationSignal::default(), |_| {})
            .await
            .unwrap();
        assert_eq!(executed.load(Ordering::SeqCst), 0);
        assert!(response.skills[0].mutation.is_none());
        assert!(matches!(
            response.skills[0].coverage,
            UpdateCoverage::NotUpdated { .. }
        ));
    }

    #[tokio::test]
    async fn changed_environment_during_plan_build_blocks_confirmation() {
        let manager = update_test_manager();
        let acquisition = beta_acquisition(&manager).await;
        let executed = Arc::new(AtomicUsize::new(0));
        let service = UpdateService::new(
            manager,
            PreparedPlanner {
                keep_entries: false,
                fail_inspection: None,
                invalid_source: None,
                builds: Arc::new(AtomicUsize::new(0)),
                build_token: PreviewToken {
                    environment_revision: "changed".into(),
                    ..update_test_token()
                },
            },
            FixedAcquirer {
                acquisitions: Mutex::new(Some(vec![acquisition])),
                calls: Arc::new(AtomicUsize::new(0)),
                expected_group_count: 1,
            },
            ResultExecutor {
                calls: executed.clone(),
                results: Vec::new(),
                expected_payload_count: 1,
            },
        );
        let request = UpdateRequest {
            context: mutation_result("beta", MutationUnitStatus::Succeeded).target,
            skill_names: vec!["beta".into()],
        };
        let prepared = service
            .prepare(&request, CancellationSignal::default())
            .await
            .unwrap();
        assert!(prepared.preview.skills.is_empty());
        assert_eq!(
            prepared.preview.blocked[0].error,
            AppError::StaleEnvironment
        );
        assert_eq!(executed.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn mixed_source_failure_keeps_an_independent_source_executable() {
        let manager = update_test_manager();
        let beta = beta_acquisition(&manager).await;
        let alpha = UpdateSourceAcquisition {
            source_result_id: "alpha-source".into(),
            source: "alpha/repo".into(),
            skill_names: vec!["alpha".into()],
            result: Err(AppError::GitCloneFailed {
                message: "unavailable".into(),
            }),
        };
        let executed = Arc::new(AtomicUsize::new(0));
        let service = UpdateService::new(
            manager,
            PreparedPlanner {
                keep_entries: false,
                fail_inspection: None,
                invalid_source: None,
                builds: Arc::new(AtomicUsize::new(0)),
                build_token: update_test_token(),
            },
            FixedAcquirer {
                acquisitions: Mutex::new(Some(vec![alpha, beta])),
                calls: Arc::new(AtomicUsize::new(0)),
                expected_group_count: 2,
            },
            ResultExecutor {
                calls: executed.clone(),
                results: vec![mutation_result("beta", MutationUnitStatus::Succeeded)],
                expected_payload_count: 1,
            },
        );
        let request = UpdateRequest {
            context: mutation_result("beta", MutationUnitStatus::Succeeded).target,
            skill_names: vec!["alpha".into(), "beta".into()],
        };
        let prepared = service
            .prepare(&request, CancellationSignal::default())
            .await
            .unwrap();
        let response = service
            .execute_prepared(prepared, &[], CancellationSignal::default(), |_| {})
            .await
            .unwrap();
        assert_eq!(response.outcome, UpdateOutcome::Partial);
        assert_eq!(
            response
                .skills
                .iter()
                .find(|skill| skill.skill_identity.skill_name == "alpha")
                .unwrap()
                .source_result_id,
            "alpha-source"
        );
        assert_eq!(executed.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn successful_conflict_preservation_and_failed_mutation_have_distinct_coverage() {
        let context = SkillLocationRef {
            environment: EnvironmentRef::Native,
            scope: SkillLocation::Global,
        };
        let succeeded = mutation_result("demo", MutationUnitStatus::Succeeded);
        let failed = mutation_result("demo", MutationUnitStatus::Failed);

        assert_eq!(
            update_coverage(Some(&succeeded), true, &context).0,
            UpdateCoverage::UpdatedWithSkippedCopies,
        );
        assert!(matches!(
            update_coverage(Some(&failed), false, &context).0,
            UpdateCoverage::NotUpdated { .. }
        ));

        let recovery = MutationUnitResult::recovery_required(
            "demo",
            "demo",
            context.clone(),
            ErrorReport::recovery_required(
                crate::error::RecoveryResourceId::parse("recovery-update").unwrap(),
                "check the controlled recovery resource",
            ),
        );
        let (coverage, warnings, retryable) = update_coverage(Some(&recovery), false, &context);
        assert!(warnings.is_empty());
        assert!(!retryable);
        let UpdateCoverage::NotUpdated { error } = coverage else {
            panic!("RecoveryRequired must remain a not-updated coverage with its error")
        };
        assert_eq!(error.code, OperationErrorCode::RecoveryRequired);
        assert_eq!(
            error.recovery_resource_id,
            Some(crate::error::RecoveryResourceId::parse("recovery-update").unwrap())
        );
        let result = UpdateSkillResult {
            skipped_copy_paths: None,
            skill_identity: SkillIdentity {
                context: context.clone(),
                skill_name: "demo".to_string(),
            },
            source_result_id: "source-1".to_string(),
            mutation: Some(recovery),
            coverage: UpdateCoverage::NotUpdated { error },
            warnings: Vec::new(),
            retryable,
        };
        assert_eq!(update_outcome(&[result]), UpdateOutcome::Failed);
    }

    #[tokio::test]
    async fn malformed_source_is_blocked_before_grouping_other_members() {
        let manager = update_test_manager();
        let beta = beta_acquisition(&manager).await;
        let service = UpdateService::new(
            manager,
            PreparedPlanner {
                keep_entries: false,
                fail_inspection: None,
                invalid_source: Some("alpha".into()),
                builds: Arc::new(AtomicUsize::new(0)),
                build_token: update_test_token(),
            },
            FixedAcquirer {
                acquisitions: Mutex::new(Some(vec![beta])),
                calls: Arc::new(AtomicUsize::new(0)),
                expected_group_count: 1,
            },
            ResultExecutor {
                calls: Arc::new(AtomicUsize::new(0)),
                results: Vec::new(),
                expected_payload_count: 1,
            },
        );
        let request = UpdateRequest {
            context: mutation_result("beta", MutationUnitStatus::Succeeded).target,
            skill_names: vec!["alpha".into(), "beta".into()],
        };
        let prepared = service
            .prepare(&request, CancellationSignal::default())
            .await
            .unwrap();
        assert_eq!(prepared.preview.skills[0].skill_name, "beta");
        assert_eq!(prepared.preview.blocked[0].skill_name, "alpha");
        assert!(matches!(
            prepared.preview.blocked[0].error,
            AppError::InvalidSource { .. }
        ));
    }

    #[test]
    fn earlier_success_and_later_cancellation_remain_partial() {
        let context = SkillLocationRef {
            environment: EnvironmentRef::Native,
            scope: SkillLocation::Global,
        };
        let cancellation =
            ErrorReport::from_app_error(AppError::MutationCancelled, Some(context.clone()));
        let skills = vec![
            UpdateSkillResult {
                skipped_copy_paths: None,
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
            context: SkillLocationRef {
                environment: EnvironmentRef::Native,
                scope: SkillLocation::Global,
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

        let prepared = service
            .prepare(&request, CancellationSignal::default())
            .await
            .unwrap();
        let response = service
            .execute_prepared(prepared, &[], CancellationSignal::default(), |_| {})
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

    #[tokio::test]
    async fn cancellation_during_acquisition_returns_a_structured_cancelled_response() {
        let manager = Arc::new(PayloadSessionManager::in_memory(
            PayloadSessionLimits {
                ttl_ms: 60_000,
                max_sessions: 4,
                max_bytes: 1_000_000,
            },
            || 1_000,
        ));
        let token = PreviewToken {
            generation: "preview-v1-cancel".to_string(),
            registry_revision: "registry-1".to_string(),
            environment_revision: "environment-1".to_string(),
            context_revision: ContextSnapshotRevision::parse("context-v1-cancel").unwrap(),
        };
        let context = SkillLocationRef {
            environment: EnvironmentRef::Native,
            scope: SkillLocation::Global,
        };
        let service = UpdateService::new(
            manager,
            Planner {
                token: token.clone(),
                rebuilds: Arc::new(AtomicUsize::new(0)),
            },
            CancellingAcquirer,
            Executor(Arc::new(AtomicUsize::new(0))),
        );

        let prepared = service
            .prepare(
                &UpdateRequest {
                    context,
                    skill_names: vec!["demo".to_string()],
                },
                CancellationSignal::default(),
            )
            .await
            .unwrap();
        let response = service
            .execute_prepared(prepared, &[], CancellationSignal::default(), |_| {})
            .await
            .unwrap();

        assert_eq!(response.outcome, UpdateOutcome::Cancelled);
        assert_eq!(response.sources.len(), 1);
        assert_eq!(response.skills.len(), 1);
        assert!(response.skills[0].mutation.is_none());
        assert!(matches!(
            response.skills[0].coverage,
            UpdateCoverage::NotUpdated { ref error }
                if error.code == OperationErrorCode::MutationCancelled
        ));
    }

    #[test]
    fn cancelled_source_acquisition_sets_cancelled_outcome_without_a_mutation_unit() {
        let context = SkillLocationRef {
            environment: EnvironmentRef::Native,
            scope: SkillLocation::Global,
        };
        let cancelled =
            ErrorReport::from_app_error(AppError::MutationCancelled, Some(context.clone()));
        let skills = vec![UpdateSkillResult {
            skipped_copy_paths: None,
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
}

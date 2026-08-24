use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use specta::Type;

#[cfg(test)]
use crate::application::collection_records::SkillSelection;
use crate::application::mutation::plan::stable_digest;
use crate::application::mutation::result::ErrorReport;
use crate::application::payload_session::{
    AcquiredPayloadHandle, DiscoverySessionHandle, PayloadSessionManager,
};
use crate::application::skill_changes::compare_update_subjects;
use crate::application::skill_libraries::{
    SkillLibraryDetail, SkillLibraryModule, UpdateLibrarySkillsRequest,
};
use crate::application::skill_source::{
    validate_saved_payloads, SavedPayloadCandidate, SavedSkillSource, SkillSourceModule,
};
use crate::application::update::{UpdateOutcome, UpdateSourceResult, UpdateSourceStatus};
use crate::application::update_subjects::{
    LibraryUpdateSubjectSnapshots, UpdateSubject, UpdateSubjectSnapshot,
};
use crate::core::mutation::CancellationSignal;
use crate::environment::content_manifest::ContentManifestReader;
use crate::environment::planning::TargetFactResolver;
use crate::error::AppError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub enum LibraryUpdateSkillStatus {
    Succeeded,
    Failed,
    NameChanged,
    DeletedUpstream,
    Cancelled,
    NotRun,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub enum LibraryCommitStatus {
    Succeeded,
    Failed,
    NotRun,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct LibraryUpdateSkillResult {
    pub skill_name: String,
    pub status: LibraryUpdateSkillStatus,
    pub source_result_id: String,
    pub content_commit: LibraryCommitStatus,
    pub catalog_commit: LibraryCommitStatus,
    pub error: Option<ErrorReport>,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct LibraryUpdateResponse {
    pub sources: Vec<UpdateSourceResult>,
    pub results: Vec<LibraryUpdateSkillResult>,
    pub outcome: UpdateOutcome,
    pub library: SkillLibraryDetail,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct LibraryUpdatePreviewToken {
    pub generation: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct LibraryUpdatePreview {
    pub token: LibraryUpdatePreviewToken,
    pub skill_names: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct LibraryUpdateRiskConfirmation {
    pub redirected_download_hosts: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct LibraryUpdatePreparedPayload {
    pub skill_name: String,
    pub payload: AcquiredPayloadHandle,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct LibraryUpdatePreparedSkillError {
    pub skill_name: String,
    pub error: ErrorReport,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(tag = "status", rename_all = "camelCase")]
#[specta(tag = "status", rename_all = "camelCase")]
pub enum LibraryUpdatePreparedSourceResult {
    Acquired {
        #[serde(rename = "discoverySession")]
        #[specta(rename = "discoverySession")]
        discovery_session: DiscoverySessionHandle,
        payloads: Vec<LibraryUpdatePreparedPayload>,
        #[serde(rename = "skillErrors")]
        #[specta(rename = "skillErrors")]
        skill_errors: Vec<LibraryUpdatePreparedSkillError>,
        #[serde(rename = "redirectedDownloadHost")]
        #[specta(rename = "redirectedDownloadHost")]
        redirected_download_host: Option<String>,
    },
    Failed {
        error: ErrorReport,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct LibraryUpdatePreparedSource {
    pub source_result_id: String,
    pub source: String,
    pub skill_names: Vec<String>,
    pub result: LibraryUpdatePreparedSourceResult,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct LibraryUpdateContinuation {
    pub sources: Vec<LibraryUpdatePreparedSource>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct ExecuteLibraryUpdateRequest {
    pub request: UpdateLibrarySkillsRequest,
    pub expected_token: LibraryUpdatePreviewToken,
    pub continuation: Option<LibraryUpdateContinuation>,
    pub risk_confirmation: Option<LibraryUpdateRiskConfirmation>,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(tag = "status", rename_all = "camelCase")]
#[specta(tag = "status", rename_all = "camelCase")]
pub enum LibraryUpdateExecutionOutcome {
    Completed {
        response: LibraryUpdateResponse,
    },
    ConfirmationRequired {
        token: LibraryUpdatePreviewToken,
        #[serde(rename = "redirectedDownloadHosts")]
        #[specta(rename = "redirectedDownloadHosts")]
        redirected_download_hosts: Vec<String>,
        continuation: LibraryUpdateContinuation,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LibraryUpdateExecutionStage {
    Acquiring,
    Validating,
    Committing,
}

pub struct LibraryUpdateService<P, S, T> {
    payloads: Arc<PayloadSessionManager>,
    subjects: P,
    skill_source: S,
    targets: T,
    libraries: Arc<SkillLibraryModule>,
}

impl<P, S, T> LibraryUpdateService<P, S, T>
where
    P: LibraryUpdateSubjectSnapshots,
    S: SkillSourceModule,
    T: TargetFactResolver + ContentManifestReader,
{
    pub fn new(
        payloads: Arc<PayloadSessionManager>,
        subjects: P,
        skill_source: S,
        targets: T,
        libraries: Arc<SkillLibraryModule>,
    ) -> Self {
        Self {
            payloads,
            subjects,
            skill_source,
            targets,
            libraries,
        }
    }

    pub async fn preview(
        &self,
        request: &UpdateLibrarySkillsRequest,
    ) -> Result<LibraryUpdatePreview, AppError> {
        let skill_names = validate_request(request)?;
        let snapshot = self
            .subjects
            .snapshot_library(&request.environment, &request.library_id, skill_names)
            .await?;
        let token = preview_token(request, &snapshot, None)?;
        Ok(LibraryUpdatePreview {
            token,
            skill_names: request.skill_names.clone(),
        })
    }

    #[cfg(test)]
    pub async fn execute(
        &self,
        execution: &ExecuteLibraryUpdateRequest,
        cancellation: CancellationSignal,
    ) -> Result<LibraryUpdateExecutionOutcome, AppError> {
        self.execute_with_stage_observer(execution, cancellation, |_| {})
            .await
    }

    pub async fn execute_with_stage_observer<F>(
        &self,
        execution: &ExecuteLibraryUpdateRequest,
        cancellation: CancellationSignal,
        observe_stage: F,
    ) -> Result<LibraryUpdateExecutionOutcome, AppError>
    where
        F: Fn(LibraryUpdateExecutionStage),
    {
        let request = &execution.request;
        let skill_names = validate_request(request)?;
        let selection = skill_names.clone();
        let initial = self
            .subjects
            .snapshot_library(&request.environment, &request.library_id, selection.clone())
            .await?;
        if preview_token(request, &initial, execution.continuation.as_ref())?
            != execution.expected_token
        {
            return Err(AppError::StaleContext);
        }
        let mut results = BTreeMap::<String, LibraryUpdateSkillResult>::new();
        let initial_by_name = subjects_by_name(&initial);
        let saved = skill_names
            .iter()
            .filter_map(|name| match initial_by_name.get(name.as_str()) {
                Some(subject) => match subject.projection.metadata() {
                    Some(metadata) => Some(SavedSkillSource {
                        name: name.clone(),
                        metadata: metadata.clone(),
                    }),
                    None => {
                        results.insert(
                            name.clone(),
                            failed(
                                name,
                                "",
                                AppError::InvalidSource {
                                    value: name.clone(),
                                },
                            ),
                        );
                        None
                    }
                },
                None => {
                    results.insert(
                        name.clone(),
                        failed(name, "", AppError::PathNotFound { path: name.clone() }),
                    );
                    None
                }
            })
            .collect();
        observe_stage(LibraryUpdateExecutionStage::Acquiring);
        let continuation = match &execution.continuation {
            Some(continuation) => continuation.clone(),
            None => match self
                .skill_source
                .acquire_saved_skills(&request.environment, saved, cancellation.clone())
                .await
            {
                Ok(acquisitions) => continuation_from_acquisitions(acquisitions),
                Err(AppError::MutationCancelled) => {
                    mark_cancelled(&request.skill_names, &mut results);
                    return self
                        .response(request, &[], results)
                        .await
                        .map(|response| LibraryUpdateExecutionOutcome::Completed { response });
                }
                Err(error) => return Err(error),
            },
        };
        let redirected_download_hosts = continuation_redirect_hosts(&continuation);
        let confirmed_hosts = execution
            .risk_confirmation
            .as_ref()
            .map(|confirmation| normalized_hosts(&confirmation.redirected_download_hosts))
            .unwrap_or_default();
        if !redirected_download_hosts.is_empty() && redirected_download_hosts != confirmed_hosts {
            return Ok(LibraryUpdateExecutionOutcome::ConfirmationRequired {
                token: preview_token(request, &initial, Some(&continuation))?,
                redirected_download_hosts,
                continuation,
            });
        }

        let mut candidates = Vec::new();
        observe_stage(LibraryUpdateExecutionStage::Validating);
        let mut source_by_skill = BTreeMap::new();
        let mut cancelled_acquisitions = BTreeSet::new();
        for source in &continuation.sources {
            for skill_name in &source.skill_names {
                source_by_skill.insert(skill_name.clone(), source.source_result_id.clone());
            }
            match &source.result {
                LibraryUpdatePreparedSourceResult::Acquired {
                    discovery_session,
                    payloads,
                    skill_errors,
                    ..
                } => {
                    for skill_error in skill_errors {
                        results.insert(
                            skill_error.skill_name.clone(),
                            failed_report(
                                &skill_error.skill_name,
                                &source.source_result_id,
                                skill_error.error.clone(),
                            ),
                        );
                    }
                    for prepared in payloads {
                        candidates.push(SavedPayloadCandidate {
                            source_result_id: source.source_result_id.clone(),
                            discovery_session: discovery_session.clone(),
                            skill_name: prepared.skill_name.clone(),
                            handle: prepared.payload.clone(),
                        });
                    }
                }
                LibraryUpdatePreparedSourceResult::Failed { error } => {
                    for skill_name in &source.skill_names {
                        let cancelled_result = error.code
                            == crate::application::mutation::result::OperationErrorCode::MutationCancelled;
                        if cancelled_result {
                            cancelled_acquisitions.insert(skill_name.clone());
                            continue;
                        }
                        results.insert(
                            skill_name.clone(),
                            failed_report(skill_name, &source.source_result_id, error.clone()),
                        );
                    }
                }
            }
        }
        let mut first_cancelled = true;
        for skill_name in &request.skill_names {
            if cancelled_acquisitions.contains(skill_name) {
                let result = if first_cancelled {
                    first_cancelled = false;
                    cancelled(
                        skill_name,
                        source_by_skill
                            .get(skill_name)
                            .map(String::as_str)
                            .unwrap_or(""),
                    )
                } else {
                    not_run(
                        skill_name,
                        source_by_skill
                            .get(skill_name)
                            .map(String::as_str)
                            .unwrap_or(""),
                    )
                };
                results.insert(skill_name.clone(), result);
            }
        }
        let validation =
            validate_saved_payloads(self.payloads.as_ref(), &request.environment, candidates).await;
        for failed_payload in validation.failed {
            results.insert(
                failed_payload.skill_name.clone(),
                failed(
                    &failed_payload.skill_name,
                    &failed_payload.source_result_id,
                    failed_payload.error,
                ),
            );
        }
        let validated = validation
            .validated
            .into_iter()
            .map(|validated| validated.payload)
            .collect();

        let latest = self
            .subjects
            .snapshot_library(&request.environment, &request.library_id, selection)
            .await?;
        let prepared = compare_update_subjects(&initial, &latest, validated)?;
        for skill_name in prepared.stale_skill_names {
            results.insert(
                skill_name.clone(),
                failed(
                    &skill_name,
                    source_by_skill
                        .get(&skill_name)
                        .map(String::as_str)
                        .unwrap_or(""),
                    AppError::StaleTarget,
                ),
            );
        }
        let mut ready = prepared
            .ready
            .into_iter()
            .map(|prepared| (prepared.payload.name().to_string(), prepared))
            .collect::<BTreeMap<_, _>>();
        observe_stage(LibraryUpdateExecutionStage::Committing);
        let mut stop_after_cancel = false;
        for skill_name in &request.skill_names {
            if results.contains_key(skill_name) {
                continue;
            }
            if stop_after_cancel {
                results.insert(
                    skill_name.clone(),
                    not_run(
                        skill_name,
                        source_by_skill
                            .get(skill_name)
                            .map(String::as_str)
                            .unwrap_or(""),
                    ),
                );
                continue;
            }
            if cancellation.is_cancelled() {
                results.insert(
                    skill_name.clone(),
                    cancelled(
                        skill_name,
                        source_by_skill
                            .get(skill_name)
                            .map(String::as_str)
                            .unwrap_or(""),
                    ),
                );
                stop_after_cancel = true;
                continue;
            }
            let Some(prepared) = ready.remove(skill_name) else {
                results.insert(
                    skill_name.clone(),
                    failed(
                        skill_name,
                        source_by_skill
                            .get(skill_name)
                            .map(String::as_str)
                            .unwrap_or(""),
                        AppError::StalePayload,
                    ),
                );
                continue;
            };
            let current = match self
                .subjects
                .snapshot_library(
                    &request.environment,
                    &request.library_id,
                    BTreeSet::from([skill_name.clone()]),
                )
                .await
            {
                Ok(current) => current,
                Err(error) => {
                    results.insert(
                        skill_name.clone(),
                        failed(
                            skill_name,
                            source_by_skill
                                .get(skill_name)
                                .map(String::as_str)
                                .unwrap_or(""),
                            error,
                        ),
                    );
                    continue;
                }
            };
            let mut current_prepared =
                match compare_update_subjects(&latest, &current, vec![prepared.payload]) {
                    Ok(prepared) => prepared,
                    Err(error) => {
                        results.insert(
                            skill_name.clone(),
                            failed(
                                skill_name,
                                source_by_skill
                                    .get(skill_name)
                                    .map(String::as_str)
                                    .unwrap_or(""),
                                error,
                            ),
                        );
                        continue;
                    }
                };
            if !current_prepared.stale_skill_names.is_empty() {
                results.insert(
                    skill_name.clone(),
                    failed(
                        skill_name,
                        source_by_skill
                            .get(skill_name)
                            .map(String::as_str)
                            .unwrap_or(""),
                        AppError::StaleTarget,
                    ),
                );
                continue;
            }
            let Some(prepared) = current_prepared.ready.pop() else {
                results.insert(
                    skill_name.clone(),
                    failed(
                        skill_name,
                        source_by_skill
                            .get(skill_name)
                            .map(String::as_str)
                            .unwrap_or(""),
                        AppError::StalePayload,
                    ),
                );
                continue;
            };
            match self
                .libraries
                .commit_validated_update(
                    &self.targets,
                    &request.environment,
                    &request.library_id,
                    prepared,
                )
                .await
            {
                Ok(()) => {
                    results.insert(
                        skill_name.clone(),
                        succeeded(
                            skill_name,
                            source_by_skill
                                .get(skill_name)
                                .map(String::as_str)
                                .unwrap_or(""),
                        ),
                    );
                }
                Err(AppError::MutationCancelled) => {
                    results.insert(
                        skill_name.clone(),
                        cancelled(
                            skill_name,
                            source_by_skill
                                .get(skill_name)
                                .map(String::as_str)
                                .unwrap_or(""),
                        ),
                    );
                    stop_after_cancel = true;
                }
                Err(error) => {
                    results.insert(
                        skill_name.clone(),
                        failed_commit(
                            skill_name,
                            source_by_skill
                                .get(skill_name)
                                .map(String::as_str)
                                .unwrap_or(""),
                            error,
                        ),
                    );
                }
            }
        }
        self.response(request, &continuation.sources, results)
            .await
            .map(|response| LibraryUpdateExecutionOutcome::Completed { response })
    }

    async fn response(
        &self,
        request: &UpdateLibrarySkillsRequest,
        sources: &[LibraryUpdatePreparedSource],
        mut results: BTreeMap<String, LibraryUpdateSkillResult>,
    ) -> Result<LibraryUpdateResponse, AppError> {
        let ordered: Vec<LibraryUpdateSkillResult> = request
            .skill_names
            .iter()
            .map(|name| {
                results
                    .remove(name)
                    .unwrap_or_else(|| failed(name, "", AppError::StalePayload))
            })
            .collect();
        let source_results = sources
            .iter()
            .map(|source| UpdateSourceResult {
                id: source.source_result_id.clone(),
                source: source.source.clone(),
                status: match &source.result {
                    LibraryUpdatePreparedSourceResult::Acquired { .. } => {
                        UpdateSourceStatus::Acquired
                    }
                    LibraryUpdatePreparedSourceResult::Failed { .. } => UpdateSourceStatus::Failed,
                },
                error: match &source.result {
                    LibraryUpdatePreparedSourceResult::Acquired { .. } => None,
                    LibraryUpdatePreparedSourceResult::Failed { error } => Some(error.clone()),
                },
            })
            .collect();
        let outcome = library_update_outcome(&ordered);
        Ok(LibraryUpdateResponse {
            sources: source_results,
            results: ordered,
            outcome,
            library: self
                .libraries
                .detail(request.environment.clone(), request.library_id.clone())
                .await?,
        })
    }
}

fn library_update_outcome(results: &[LibraryUpdateSkillResult]) -> UpdateOutcome {
    let succeeded = results
        .iter()
        .filter(|result| result.status == LibraryUpdateSkillStatus::Succeeded)
        .count();
    if succeeded == results.len() {
        UpdateOutcome::Succeeded
    } else if succeeded > 0 {
        UpdateOutcome::Partial
    } else if results
        .iter()
        .any(|result| result.status == LibraryUpdateSkillStatus::Cancelled)
    {
        UpdateOutcome::Cancelled
    } else {
        UpdateOutcome::Failed
    }
}

fn preview_token(
    request: &UpdateLibrarySkillsRequest,
    snapshot: &UpdateSubjectSnapshot,
    continuation: Option<&LibraryUpdateContinuation>,
) -> Result<LibraryUpdatePreviewToken, AppError> {
    let revisions = snapshot
        .subjects
        .iter()
        .map(|subject| {
            (
                subject.skill_name.as_str(),
                &subject.source_record_revision,
                &subject.target_revision,
                &subject.content_revision,
            )
        })
        .collect::<Vec<_>>();
    Ok(LibraryUpdatePreviewToken {
        generation: stable_digest(&(
            "library-update-preview-v1",
            &request.environment,
            &request.library_id,
            &request.skill_names,
            &snapshot.resolution_revision,
            revisions,
            continuation,
        ))?,
    })
}

fn continuation_from_acquisitions(
    acquisitions: Vec<crate::application::skill_source::SavedSkillSourceAcquisition>,
) -> LibraryUpdateContinuation {
    LibraryUpdateContinuation {
        sources: acquisitions
            .into_iter()
            .map(|acquisition| LibraryUpdatePreparedSource {
                source_result_id: acquisition.source_result_id,
                source: acquisition.source,
                skill_names: acquisition.skill_names,
                result: match acquisition.result {
                    Ok(acquired) => LibraryUpdatePreparedSourceResult::Acquired {
                        discovery_session: acquired.facts.discovery_session,
                        payloads: acquired
                            .payloads
                            .into_iter()
                            .map(|(skill_name, payload)| LibraryUpdatePreparedPayload {
                                skill_name,
                                payload,
                            })
                            .collect(),
                        skill_errors: acquired
                            .skill_errors
                            .into_iter()
                            .map(|(skill_name, error)| LibraryUpdatePreparedSkillError {
                                skill_name,
                                error: ErrorReport::from_app_error(error, None),
                            })
                            .collect(),
                        redirected_download_host: acquired.redirected_download_host,
                    },
                    Err(error) => LibraryUpdatePreparedSourceResult::Failed {
                        error: ErrorReport::from_app_error(error, None),
                    },
                },
            })
            .collect(),
    }
}

fn continuation_redirect_hosts(continuation: &LibraryUpdateContinuation) -> Vec<String> {
    normalized_hosts(
        &continuation
            .sources
            .iter()
            .filter_map(|source| match &source.result {
                LibraryUpdatePreparedSourceResult::Acquired {
                    redirected_download_host,
                    ..
                } => redirected_download_host.clone(),
                LibraryUpdatePreparedSourceResult::Failed { .. } => None,
            })
            .collect::<Vec<_>>(),
    )
}

fn normalized_hosts(hosts: &[String]) -> Vec<String> {
    hosts
        .iter()
        .filter(|host| !host.trim().is_empty())
        .cloned()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn validate_request(request: &UpdateLibrarySkillsRequest) -> Result<BTreeSet<String>, AppError> {
    if request.skill_names.is_empty()
        || request
            .skill_names
            .iter()
            .any(|name| name.trim().is_empty())
    {
        return Err(AppError::Validation {
            field: Some("skillNames".to_string()),
            message: "at least one Skill name is required".to_string(),
        });
    }
    let names = request.skill_names.iter().cloned().collect::<BTreeSet<_>>();
    if names.len() != request.skill_names.len() {
        return Err(AppError::Validation {
            field: Some("skillNames".to_string()),
            message: "a Skill can only be updated once per request".to_string(),
        });
    }
    Ok(names)
}

fn subjects_by_name(snapshot: &UpdateSubjectSnapshot) -> BTreeMap<&str, &UpdateSubject> {
    snapshot
        .subjects
        .iter()
        .map(|subject| (subject.skill_name.as_str(), subject))
        .collect()
}

fn succeeded(skill_name: &str, source_result_id: &str) -> LibraryUpdateSkillResult {
    LibraryUpdateSkillResult {
        skill_name: skill_name.to_string(),
        status: LibraryUpdateSkillStatus::Succeeded,
        source_result_id: source_result_id.to_string(),
        content_commit: LibraryCommitStatus::Succeeded,
        catalog_commit: LibraryCommitStatus::Succeeded,
        error: None,
    }
}

fn failed(skill_name: &str, source_result_id: &str, error: AppError) -> LibraryUpdateSkillResult {
    failed_report(
        skill_name,
        source_result_id,
        ErrorReport::from_app_error(error, None),
    )
}

fn failed_report(
    skill_name: &str,
    source_result_id: &str,
    error: ErrorReport,
) -> LibraryUpdateSkillResult {
    let status = match error.code {
        crate::application::mutation::result::OperationErrorCode::UpstreamSkillNameChanged => {
            LibraryUpdateSkillStatus::NameChanged
        }
        crate::application::mutation::result::OperationErrorCode::UpstreamSkillDeleted => {
            LibraryUpdateSkillStatus::DeletedUpstream
        }
        _ => LibraryUpdateSkillStatus::Failed,
    };
    LibraryUpdateSkillResult {
        skill_name: skill_name.to_string(),
        status,
        source_result_id: source_result_id.to_string(),
        content_commit: LibraryCommitStatus::NotRun,
        catalog_commit: LibraryCommitStatus::NotRun,
        error: Some(error),
    }
}

fn failed_commit(
    skill_name: &str,
    source_result_id: &str,
    error: AppError,
) -> LibraryUpdateSkillResult {
    LibraryUpdateSkillResult {
        skill_name: skill_name.to_string(),
        status: LibraryUpdateSkillStatus::Failed,
        source_result_id: source_result_id.to_string(),
        content_commit: LibraryCommitStatus::Failed,
        catalog_commit: LibraryCommitStatus::Failed,
        error: Some(ErrorReport::from_app_error(error, None)),
    }
}

fn cancelled(skill_name: &str, source_result_id: &str) -> LibraryUpdateSkillResult {
    LibraryUpdateSkillResult {
        skill_name: skill_name.to_string(),
        status: LibraryUpdateSkillStatus::Cancelled,
        source_result_id: source_result_id.to_string(),
        content_commit: LibraryCommitStatus::NotRun,
        catalog_commit: LibraryCommitStatus::NotRun,
        error: Some(ErrorReport::from_app_error(
            AppError::MutationCancelled,
            None,
        )),
    }
}

fn not_run(skill_name: &str, source_result_id: &str) -> LibraryUpdateSkillResult {
    LibraryUpdateSkillResult {
        skill_name: skill_name.to_string(),
        status: LibraryUpdateSkillStatus::NotRun,
        source_result_id: source_result_id.to_string(),
        content_commit: LibraryCommitStatus::NotRun,
        catalog_commit: LibraryCommitStatus::NotRun,
        error: Some(ErrorReport::from_app_error(
            AppError::MutationCancelled,
            None,
        )),
    }
}

fn mark_cancelled(
    skill_names: &[String],
    results: &mut BTreeMap<String, LibraryUpdateSkillResult>,
) {
    let mut first = true;
    for skill_name in skill_names {
        if results.contains_key(skill_name) {
            continue;
        }
        results.insert(
            skill_name.clone(),
            if first {
                first = false;
                cancelled(skill_name, "")
            } else {
                not_run(skill_name, "")
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use std::future::Future;
    use std::pin::Pin;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use crate::application::collection_records::{DocumentRevision, SourceRecordRevision};
    use crate::application::payload_session::{
        AcquiredPayloadHandle, DiscoverySessionHandle, InMemoryPayloadSessionStorage,
        PayloadPlanningMetadata, PayloadSessionLimits,
    };
    use crate::application::skill_libraries::{
        LibraryCatalog, LibraryId, LibrarySkillRecord, LibrarySkillSourceRecord,
        SkillLibraryRecord, SkillLibraryRepository, LIBRARY_SCHEMA_VERSION,
    };
    use crate::application::skill_paths::{
        ContentRevision, RootResolutionRevision, TargetRevision,
    };
    use crate::application::skill_source::{
        AcquiredSavedSkillSource, SavedSkillSourceAcquisition, SavedSkillSourceGroup,
        SkillSourceFuture,
    };
    use crate::application::source_evidence::{RemoteSnapshotId, SourceSnapshotFacts};
    use crate::application::update_subjects::LibraryUpdateSubjectProvider;
    use crate::core::projects::{ProjectMigrationRegistry, ProjectMigrationState};
    use crate::core::skill_payload::build_skill_payload;
    use crate::core::source_identity::NormalizedRef;
    use crate::core::NormalizedUpdateMetadata;
    use crate::environment::planning::RuntimeTargetFactResolver;
    use crate::environment::types::EnvironmentRef;
    use crate::environment::wsl::WslRuntime;
    use crate::runtime::skill_libraries::RuntimeSkillLibraryRepository;

    use super::*;

    struct FixedSubjects {
        environment: EnvironmentRef,
        library_id: LibraryId,
        names: Vec<String>,
    }

    impl FixedSubjects {
        fn snapshot(
            &self,
            selection: SkillSelection,
            changed_target: Option<&str>,
        ) -> UpdateSubjectSnapshot {
            let selected = selection;
            UpdateSubjectSnapshot {
                environment: self.environment.clone(),
                resolution_revision: RootResolutionRevision::for_test("collection-1"),
                document_revision: DocumentRevision::for_test("catalog-1"),
                subjects: self
                    .names
                    .iter()
                    .filter(|name| selected.contains(*name))
                    .map(|name| {
                        let target_revision = if changed_target == Some(name.as_str()) {
                            "changed-target".to_string()
                        } else {
                            format!("target-{name}")
                        };
                        UpdateSubject {
                            skill_name: name.clone(),
                            source_record_revision: SourceRecordRevision::for_test(&format!(
                                "source-{name}"
                            )),
                            target_revision: TargetRevision::for_test(&target_revision),
                            content_revision: ContentRevision::missing_for_test(),
                            projection:
                                crate::application::collection_records::RecordProjection::Available(
                                    metadata(name, "old"),
                                ),
                        }
                    })
                    .collect(),
            }
        }
    }

    impl LibraryUpdateSubjectSnapshots for FixedSubjects {
        fn snapshot_library<'a>(
            &'a self,
            environment: &'a EnvironmentRef,
            library_id: &'a LibraryId,
            selection: SkillSelection,
        ) -> Pin<Box<dyn Future<Output = Result<UpdateSubjectSnapshot, AppError>> + Send + 'a>>
        {
            Box::pin(async move {
                assert_eq!(environment, &self.environment);
                assert_eq!(library_id, &self.library_id);
                Ok(self.snapshot(selection, None))
            })
        }
    }

    struct DriftingSubjects {
        inner: LibraryUpdateSubjectProvider<RuntimeTargetFactResolver>,
        calls: AtomicUsize,
    }

    struct PreviewDriftSubjects {
        fixed: FixedSubjects,
        calls: AtomicUsize,
    }

    struct UnrelatedRecordDriftSubjects {
        fixed: FixedSubjects,
        calls: AtomicUsize,
    }

    struct CommitRaceSubjects {
        inner: LibraryUpdateSubjectProvider<RuntimeTargetFactResolver>,
        repository: Arc<RuntimeSkillLibraryRepository>,
        calls: AtomicUsize,
    }

    impl LibraryUpdateSubjectSnapshots for CommitRaceSubjects {
        fn snapshot_library<'a>(
            &'a self,
            environment: &'a EnvironmentRef,
            library_id: &'a LibraryId,
            selection: SkillSelection,
        ) -> Pin<Box<dyn Future<Output = Result<UpdateSubjectSnapshot, AppError>> + Send + 'a>>
        {
            Box::pin(async move {
                let snapshot = self
                    .inner
                    .snapshot_library(environment, library_id, selection)
                    .await?;
                let call = self.calls.fetch_add(1, Ordering::SeqCst);
                if call == 3 {
                    let mut catalog = self.repository.load(&EnvironmentRef::Native).await?;
                    set_source_revision(
                        &mut catalog.libraries[0].skills[0].source_record,
                        "external",
                    );
                    self.repository
                        .save(&EnvironmentRef::Native, &catalog)
                        .await?;
                }
                Ok(snapshot)
            })
        }
    }

    impl LibraryUpdateSubjectSnapshots for PreviewDriftSubjects {
        fn snapshot_library<'a>(
            &'a self,
            environment: &'a EnvironmentRef,
            library_id: &'a LibraryId,
            selection: SkillSelection,
        ) -> Pin<Box<dyn Future<Output = Result<UpdateSubjectSnapshot, AppError>> + Send + 'a>>
        {
            Box::pin(async move {
                assert_eq!(environment, &self.fixed.environment);
                assert_eq!(library_id, &self.fixed.library_id);
                let call = self.calls.fetch_add(1, Ordering::SeqCst);
                let mut snapshot = self.fixed.snapshot(selection, None);
                if call > 0 {
                    snapshot.subjects[0].source_record_revision =
                        SourceRecordRevision::for_test("source-changed");
                }
                Ok(snapshot)
            })
        }
    }

    impl LibraryUpdateSubjectSnapshots for UnrelatedRecordDriftSubjects {
        fn snapshot_library<'a>(
            &'a self,
            environment: &'a EnvironmentRef,
            library_id: &'a LibraryId,
            selection: SkillSelection,
        ) -> Pin<Box<dyn Future<Output = Result<UpdateSubjectSnapshot, AppError>> + Send + 'a>>
        {
            Box::pin(async move {
                assert_eq!(environment, &self.fixed.environment);
                assert_eq!(library_id, &self.fixed.library_id);
                let call = self.calls.fetch_add(1, Ordering::SeqCst);
                let mut snapshot = self.fixed.snapshot(selection, None);
                if call > 0 {
                    snapshot.document_revision =
                        DocumentRevision::for_test("catalog-with-unrelated-change");
                }
                Ok(snapshot)
            })
        }
    }

    impl LibraryUpdateSubjectSnapshots for DriftingSubjects {
        fn snapshot_library<'a>(
            &'a self,
            environment: &'a EnvironmentRef,
            library_id: &'a LibraryId,
            selection: SkillSelection,
        ) -> Pin<Box<dyn Future<Output = Result<UpdateSubjectSnapshot, AppError>> + Send + 'a>>
        {
            Box::pin(async move {
                let call = self.calls.fetch_add(1, Ordering::SeqCst);
                let mut snapshot = self
                    .inner
                    .snapshot_library(environment, library_id, selection)
                    .await?;
                if call == 2 {
                    let alpha = snapshot
                        .subjects
                        .iter_mut()
                        .find(|subject| subject.skill_name == "alpha")
                        .expect("alpha subject");
                    alpha.target_revision = TargetRevision::for_test("changed-target");
                }
                Ok(snapshot)
            })
        }
    }

    struct CancellingSource;

    impl SkillSourceModule for CancellingSource {
        fn acquire_saved_groups<'a>(
            &'a self,
            _groups: &'a [SavedSkillSourceGroup],
            _cancellation: CancellationSignal,
        ) -> SkillSourceFuture<'a, Result<Vec<SavedSkillSourceAcquisition>, AppError>> {
            Box::pin(async { Err(AppError::MutationCancelled) })
        }
    }

    struct FixedSource {
        discovery: DiscoverySessionHandle,
        payloads: Vec<(String, AcquiredPayloadHandle)>,
        skill_errors: Vec<(String, AppError)>,
        redirected_download_host: Option<String>,
        calls: Arc<AtomicUsize>,
    }

    impl SkillSourceModule for FixedSource {
        fn acquire_saved_groups<'a>(
            &'a self,
            groups: &'a [SavedSkillSourceGroup],
            _cancellation: CancellationSignal,
        ) -> SkillSourceFuture<'a, Result<Vec<SavedSkillSourceAcquisition>, AppError>> {
            Box::pin(async move {
                self.calls.fetch_add(1, Ordering::SeqCst);
                assert_eq!(groups.len(), 1);
                Ok(vec![SavedSkillSourceAcquisition {
                    source_result_id: groups[0].source_result_id.clone(),
                    source: groups[0].source.clone(),
                    skill_names: groups[0]
                        .skills
                        .iter()
                        .map(|skill| skill.name.clone())
                        .collect(),
                    result: Ok(AcquiredSavedSkillSource {
                        facts: SourceSnapshotFacts {
                            discovery_session: self.discovery.clone(),
                            snapshot_id: RemoteSnapshotId::new(
                                NormalizedRef::Named("main".to_string()),
                                "main",
                                "new",
                            ),
                            complete_skill_path_catalog: groups[0]
                                .skills
                                .iter()
                                .map(|skill| skill.skill_path().to_string())
                                .collect(),
                        },
                        payloads: self.payloads.clone(),
                        skill_errors: self.skill_errors.clone(),
                        redirected_download_host: self.redirected_download_host.clone(),
                    }),
                }])
            })
        }
    }

    struct Fixture {
        _temp: tempfile::TempDir,
        repository: Arc<RuntimeSkillLibraryRepository>,
        library_id: LibraryId,
        manager: Arc<PayloadSessionManager>,
        subjects: FixedSubjects,
        source: FixedSource,
    }

    async fn fixture(names: &[&str], redirected_download_host: Option<&str>) -> Fixture {
        let temp = tempfile::tempdir().unwrap();
        let repository = Arc::new(RuntimeSkillLibraryRepository::new(
            temp.path().join("library-storage"),
            Arc::new(WslRuntime::default()),
            Arc::new(ProjectMigrationRegistry::new(
                ProjectMigrationState::NotNeeded,
            )),
        ));
        let library_id = LibraryId::parse("library-1");
        repository
            .save(&EnvironmentRef::Native, &catalog(&library_id, names))
            .await
            .unwrap();
        let manager = Arc::new(payload_manager());
        let discovery = manager
            .discover(EnvironmentRef::Native, "https://example.com/repo.git")
            .await
            .unwrap();
        let mut payloads = Vec::new();
        for name in names {
            payloads.push(acquired_payload(&manager, &discovery, temp.path(), name).await);
        }
        Fixture {
            subjects: FixedSubjects {
                environment: EnvironmentRef::Native,
                library_id: library_id.clone(),
                names: names.iter().map(|name| (*name).to_string()).collect(),
            },
            source: FixedSource {
                discovery,
                payloads,
                skill_errors: Vec::new(),
                redirected_download_host: redirected_download_host.map(str::to_string),
                calls: Arc::new(AtomicUsize::new(0)),
            },
            _temp: temp,
            repository,
            library_id,
            manager,
        }
    }

    async fn execute_completed<P, S, T>(
        service: &LibraryUpdateService<P, S, T>,
        request: UpdateLibrarySkillsRequest,
        cancellation: CancellationSignal,
    ) -> Result<LibraryUpdateResponse, AppError>
    where
        P: LibraryUpdateSubjectSnapshots,
        S: SkillSourceModule,
        T: TargetFactResolver + ContentManifestReader,
    {
        let preview = service.preview(&request).await?;
        match service
            .execute(
                &ExecuteLibraryUpdateRequest {
                    request,
                    expected_token: preview.token,
                    continuation: None,
                    risk_confirmation: None,
                },
                cancellation,
            )
            .await?
        {
            LibraryUpdateExecutionOutcome::Completed { response } => Ok(response),
            LibraryUpdateExecutionOutcome::ConfirmationRequired { .. } => {
                Err(AppError::StaleContext)
            }
        }
    }

    #[tokio::test]
    async fn preview_token_rejects_a_changed_source_record_before_acquisition() {
        let Fixture {
            _temp,
            repository,
            library_id,
            manager,
            subjects,
            ..
        } = fixture(&["alpha"], None).await;
        let service = LibraryUpdateService::new(
            manager,
            PreviewDriftSubjects {
                fixed: subjects,
                calls: AtomicUsize::new(0),
            },
            CancellingSource,
            targets(),
            Arc::new(SkillLibraryModule::new(repository)),
        );
        let request = UpdateLibrarySkillsRequest {
            environment: EnvironmentRef::Native,
            library_id,
            skill_names: vec!["alpha".to_string()],
        };

        let preview = service.preview(&request).await.unwrap();
        let error = service
            .execute(
                &ExecuteLibraryUpdateRequest {
                    request,
                    expected_token: preview.token,
                    continuation: None,
                    risk_confirmation: None,
                },
                CancellationSignal::default(),
            )
            .await
            .unwrap_err();

        assert_eq!(error, AppError::StaleContext);
    }

    #[tokio::test]
    async fn preview_token_ignores_an_unrelated_catalog_record_change() {
        let Fixture {
            _temp,
            repository,
            library_id,
            manager,
            subjects,
            ..
        } = fixture(&["alpha"], None).await;
        let service = LibraryUpdateService::new(
            manager,
            UnrelatedRecordDriftSubjects {
                fixed: subjects,
                calls: AtomicUsize::new(0),
            },
            CancellingSource,
            targets(),
            Arc::new(SkillLibraryModule::new(repository)),
        );
        let request = UpdateLibrarySkillsRequest {
            environment: EnvironmentRef::Native,
            library_id,
            skill_names: vec!["alpha".to_string()],
        };

        let preview = service.preview(&request).await.unwrap();
        let outcome = service
            .execute(
                &ExecuteLibraryUpdateRequest {
                    request,
                    expected_token: preview.token,
                    continuation: None,
                    risk_confirmation: None,
                },
                CancellationSignal::default(),
            )
            .await
            .unwrap();

        assert!(matches!(
            outcome,
            LibraryUpdateExecutionOutcome::Completed { response }
                if response.outcome == UpdateOutcome::Cancelled
        ));
    }

    #[tokio::test]
    async fn commit_rejects_a_source_record_changed_after_the_final_observation() {
        let Fixture {
            _temp,
            repository,
            library_id,
            manager,
            source,
            ..
        } = fixture(&["alpha"], None).await;
        let service = LibraryUpdateService::new(
            manager,
            CommitRaceSubjects {
                inner: LibraryUpdateSubjectProvider::new(repository.clone(), targets()),
                repository: repository.clone(),
                calls: AtomicUsize::new(0),
            },
            source,
            targets(),
            Arc::new(SkillLibraryModule::new(repository.clone())),
        );

        let response = execute_completed(
            &service,
            UpdateLibrarySkillsRequest {
                environment: EnvironmentRef::Native,
                library_id,
                skill_names: vec!["alpha".to_string()],
            },
            CancellationSignal::default(),
        )
        .await
        .unwrap();

        assert_eq!(response.results[0].status, LibraryUpdateSkillStatus::Failed);
        let saved = repository.load(&EnvironmentRef::Native).await.unwrap();
        assert_eq!(
            source_revision(&saved.libraries[0].skills[0].source_record).as_deref(),
            Some("external")
        );
    }

    #[tokio::test]
    async fn cancellation_marks_the_first_requested_skill_and_leaves_the_rest_not_run() {
        let Fixture {
            _temp,
            repository,
            library_id,
            manager,
            subjects,
            ..
        } = fixture(&["alpha", "beta"], None).await;
        let service = LibraryUpdateService::new(
            manager,
            subjects,
            CancellingSource,
            targets(),
            Arc::new(SkillLibraryModule::new(repository)),
        );

        let response = execute_completed(
            &service,
            UpdateLibrarySkillsRequest {
                environment: EnvironmentRef::Native,
                library_id,
                skill_names: vec!["beta".to_string(), "alpha".to_string()],
            },
            CancellationSignal::default(),
        )
        .await
        .unwrap();

        assert_eq!(response.results[0].skill_name, "beta");
        assert_eq!(
            response.results[0].status,
            LibraryUpdateSkillStatus::Cancelled
        );
        assert_eq!(response.results[1].skill_name, "alpha");
        assert_eq!(response.results[1].status, LibraryUpdateSkillStatus::NotRun);
    }

    #[tokio::test]
    async fn one_batch_updates_every_selected_skill_through_the_library_transaction() {
        let Fixture {
            _temp,
            repository,
            library_id,
            manager,
            subjects: _,
            source,
        } = fixture(&["alpha", "beta"], None).await;
        let service = LibraryUpdateService::new(
            manager,
            LibraryUpdateSubjectProvider::new(repository.clone(), targets()),
            source,
            targets(),
            Arc::new(SkillLibraryModule::new(repository.clone())),
        );

        let response = execute_completed(
            &service,
            UpdateLibrarySkillsRequest {
                environment: EnvironmentRef::Native,
                library_id: library_id.clone(),
                skill_names: vec!["alpha".to_string(), "beta".to_string()],
            },
            CancellationSignal::default(),
        )
        .await
        .unwrap();

        assert_eq!(
            response
                .results
                .iter()
                .map(|result| result.status)
                .collect::<Vec<_>>(),
            vec![
                LibraryUpdateSkillStatus::Succeeded,
                LibraryUpdateSkillStatus::Succeeded,
            ],
            "{:#?}",
            response.results,
        );
        assert_eq!(response.sources.len(), 1);
        assert_eq!(
            response.sources[0].status,
            crate::application::update::UpdateSourceStatus::Acquired
        );
        assert_eq!(
            response.outcome,
            crate::application::update::UpdateOutcome::Succeeded
        );
        assert_eq!(
            response.results[0].content_commit,
            LibraryCommitStatus::Succeeded
        );
        assert_eq!(
            response.results[0].catalog_commit,
            LibraryCommitStatus::Succeeded
        );
        assert_eq!(
            response
                .library
                .skills
                .iter()
                .map(|skill| skill.description.as_str())
                .collect::<Vec<_>>(),
            vec!["alpha updated", "beta updated"]
        );
        let saved = repository.load(&EnvironmentRef::Native).await.unwrap();
        assert!(saved.libraries[0]
            .skills
            .iter()
            .all(|skill| source_revision(&skill.source_record).as_deref() == Some("new")));
    }

    #[tokio::test]
    async fn redirected_source_requires_confirmation_before_any_library_write() {
        let Fixture {
            _temp,
            repository,
            library_id,
            manager,
            subjects: _,
            source,
        } = fixture(&["alpha"], Some("cdn.example.com")).await;
        let acquisition_calls = source.calls.clone();
        let service = LibraryUpdateService::new(
            manager,
            LibraryUpdateSubjectProvider::new(repository.clone(), targets()),
            source,
            targets(),
            Arc::new(SkillLibraryModule::new(repository.clone())),
        );
        let request = UpdateLibrarySkillsRequest {
            environment: EnvironmentRef::Native,
            library_id,
            skill_names: vec!["alpha".to_string()],
        };
        let preview = service.preview(&request).await.unwrap();
        let first = service
            .execute(
                &ExecuteLibraryUpdateRequest {
                    request: request.clone(),
                    expected_token: preview.token,
                    continuation: None,
                    risk_confirmation: None,
                },
                CancellationSignal::default(),
            )
            .await
            .unwrap();
        let LibraryUpdateExecutionOutcome::ConfirmationRequired {
            token,
            redirected_download_hosts,
            continuation,
        } = first
        else {
            panic!("redirected update must require confirmation");
        };
        assert_eq!(redirected_download_hosts, vec!["cdn.example.com"]);
        assert_eq!(acquisition_calls.load(Ordering::SeqCst), 1);
        let unchanged = repository.load(&EnvironmentRef::Native).await.unwrap();
        assert_eq!(unchanged.libraries[0].skills[0].description, "alpha old");

        let completed = service
            .execute(
                &ExecuteLibraryUpdateRequest {
                    request,
                    expected_token: token,
                    continuation: Some(continuation),
                    risk_confirmation: Some(LibraryUpdateRiskConfirmation {
                        redirected_download_hosts: vec!["cdn.example.com".to_string()],
                    }),
                },
                CancellationSignal::default(),
            )
            .await
            .unwrap();
        let LibraryUpdateExecutionOutcome::Completed { response } = completed else {
            panic!("confirmed update must complete");
        };
        assert_eq!(acquisition_calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            response.results[0].status,
            LibraryUpdateSkillStatus::Succeeded
        );
        assert_eq!(response.library.skills[0].description, "alpha updated");
    }

    #[tokio::test]
    async fn upstream_name_change_keeps_the_installed_library_skill() {
        let Fixture {
            _temp,
            repository,
            library_id,
            manager,
            subjects: _,
            mut source,
        } = fixture(&["alpha"], None).await;
        source.skill_errors.push((
            "alpha".to_string(),
            AppError::UpstreamSkillNameChanged {
                expected_name: "alpha".to_string(),
                actual_name: "renamed-alpha".to_string(),
            },
        ));
        let service = LibraryUpdateService::new(
            manager,
            LibraryUpdateSubjectProvider::new(repository.clone(), targets()),
            source,
            targets(),
            Arc::new(SkillLibraryModule::new(repository.clone())),
        );

        let response = execute_completed(
            &service,
            UpdateLibrarySkillsRequest {
                environment: EnvironmentRef::Native,
                library_id,
                skill_names: vec!["alpha".to_string()],
            },
            CancellationSignal::default(),
        )
        .await
        .unwrap();

        assert_eq!(
            response.results[0].status,
            LibraryUpdateSkillStatus::NameChanged
        );
        assert_eq!(response.library.skills[0].description, "alpha old");
    }

    #[tokio::test]
    async fn one_drifted_skill_does_not_stop_the_other_library_update() {
        let Fixture {
            _temp,
            repository,
            library_id,
            manager,
            subjects: _,
            source,
        } = fixture(&["alpha", "beta"], None).await;
        let service = LibraryUpdateService::new(
            manager,
            DriftingSubjects {
                inner: LibraryUpdateSubjectProvider::new(repository.clone(), targets()),
                calls: AtomicUsize::new(0),
            },
            source,
            targets(),
            Arc::new(SkillLibraryModule::new(repository.clone())),
        );

        let response = execute_completed(
            &service,
            UpdateLibrarySkillsRequest {
                environment: EnvironmentRef::Native,
                library_id,
                skill_names: vec!["alpha".to_string(), "beta".to_string()],
            },
            CancellationSignal::default(),
        )
        .await
        .unwrap();

        assert_eq!(response.results[0].status, LibraryUpdateSkillStatus::Failed);
        assert_eq!(
            response.results[0].error.as_ref().map(|error| error.code),
            Some(crate::application::mutation::result::OperationErrorCode::StaleTarget)
        );
        assert_eq!(
            response.results[1].status,
            LibraryUpdateSkillStatus::Succeeded
        );
        assert_eq!(response.library.skills[0].description, "alpha old");
        assert_eq!(response.library.skills[1].description, "beta updated");
    }

    fn payload_manager() -> PayloadSessionManager {
        PayloadSessionManager::new(
            Arc::new(InMemoryPayloadSessionStorage::default()),
            PayloadSessionLimits {
                ttl_ms: 60_000,
                max_sessions: 4,
                max_bytes: 1_000_000,
            },
            || 1_000,
        )
    }

    fn targets() -> RuntimeTargetFactResolver {
        RuntimeTargetFactResolver::new(Arc::new(WslRuntime::default()))
    }

    fn catalog(library_id: &LibraryId, names: &[&str]) -> LibraryCatalog {
        LibraryCatalog {
            schema_version: LIBRARY_SCHEMA_VERSION,
            libraries: vec![SkillLibraryRecord {
                id: library_id.clone(),
                name: "Backend".to_string(),
                skills: names
                    .iter()
                    .map(|name| LibrarySkillRecord {
                        name: (*name).to_string(),
                        description: format!("{name} old"),
                        source_record: serde_json::to_value(LibrarySkillSourceRecord {
                            source_type: "git".to_string(),
                            source: "https://example.com/repo.git".to_string(),
                            reacquisition_url: Some("https://example.com/repo.git".to_string()),
                            ref_name: Some("main".to_string()),
                            skill_path: Some(format!("skills/{name}")),
                            installed_revision: Some("old".to_string()),
                            computed_hash: Some("old".to_string()),
                            artifact_url: None,
                            plugin_name: None,
                            well_known: None,
                            extra: serde_json::Map::new(),
                        })
                        .unwrap(),
                        content_manifest_hash: format!("manifest-{name}"),
                        updated_at: None,
                        extra: serde_json::Map::new(),
                    })
                    .collect(),
                extra: serde_json::Map::new(),
            }],
            extra: serde_json::Map::new(),
        }
    }

    fn source_revision(source: &serde_json::Value) -> Option<String> {
        serde_json::from_value::<LibrarySkillSourceRecord>(source.clone())
            .unwrap()
            .installed_revision
    }

    fn set_source_revision(source: &mut serde_json::Value, revision: &str) {
        let mut record =
            serde_json::from_value::<LibrarySkillSourceRecord>(source.clone()).unwrap();
        record.installed_revision = Some(revision.to_string());
        *source = serde_json::to_value(record).unwrap();
    }

    fn metadata(name: &str, revision: &str) -> NormalizedUpdateMetadata {
        NormalizedUpdateMetadata {
            source: "https://example.com/repo.git".to_string(),
            source_type: "git".to_string(),
            source_url: Some("https://example.com/repo.git".to_string()),
            ref_name: Some("main".to_string()),
            skill_path: Some(format!("skills/{name}")),
            remote_hash: Some(revision.to_string()),
            computed_hash: Some(revision.to_string()),
            well_known_digest: None,
        }
    }

    async fn acquired_payload(
        manager: &PayloadSessionManager,
        discovery: &DiscoverySessionHandle,
        root: &std::path::Path,
        name: &str,
    ) -> (String, AcquiredPayloadHandle) {
        let source = root.join(format!("updated-{name}"));
        std::fs::create_dir_all(&source).unwrap();
        std::fs::write(
            source.join("SKILL.md"),
            format!("---\nname: {name}\ndescription: {name} updated\n---\nNew body\n"),
        )
        .unwrap();
        let handle = manager
            .acquire_payload_with_metadata(
                discovery,
                &format!("skills/{name}"),
                build_skill_payload(&source).unwrap(),
                PayloadPlanningMetadata {
                    skill_name: name.to_string(),
                    install_dir_name: name.to_string(),
                    source: "https://example.com/repo.git".to_string(),
                    source_type: "git".to_string(),
                    source_url: Some("https://example.com/repo.git".to_string()),
                    ref_name: Some("main".to_string()),
                    skill_path: format!("skills/{name}"),
                    plugin_name: None,
                    computed_hash: "new".to_string(),
                    upstream_revision: Some("new".to_string()),
                    well_known: None,
                },
            )
            .await
            .unwrap();
        (name.to_string(), handle)
    }
}

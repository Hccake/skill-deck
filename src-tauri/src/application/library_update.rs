use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use specta::Type;

#[cfg(test)]
use crate::application::collection_records::SkillSelection;
use crate::application::library_membership::LibraryMembershipOutcome;
use crate::application::mutation::result::ErrorReport;
use crate::application::payload_session::PayloadSessionManager;
use crate::application::skill_changes::{compare_update_subjects, ReadyUpdatePayload};
use crate::application::skill_libraries::{
    SkillLibraryDetail, SkillLibraryModule, UpdateLibrarySkillsRequest,
};
use crate::application::skill_source::{SavedSkillSource, SkillSourceModule};
use crate::application::update::{UpdateOutcome, UpdateSourceResult, UpdateSourceStatus};
use crate::application::update_subjects::LibraryUpdateSubjectSnapshots;
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
    pub library: Option<SkillLibraryDetail>,
    pub membership: LibraryMembershipOutcome,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct PreparedLibraryUpdatePreview {
    pub skill_names: Vec<String>,
    pub blocked: Vec<crate::application::update::UpdatePreparationIssue>,
    pub redirected_download_hosts: Vec<String>,
}

pub struct PreparedLibraryUpdate {
    pub request: UpdateLibrarySkillsRequest,
    pub preview: PreparedLibraryUpdatePreview,
    items: Vec<(String, ReadyUpdatePayload)>,
    sources: Vec<UpdateSourceResult>,
    source_by_skill: BTreeMap<String, String>,
}

impl PreparedLibraryUpdate {
    pub fn expires_at_epoch_ms(&self) -> Option<u64> {
        self.items
            .iter()
            .map(|(_, item)| item.payload.handle().expires_at_epoch_ms)
            .min()
    }

    pub fn expire_payloads(&mut self, now: u64) {
        self.items.retain(|(_, item)| {
            if item.payload.handle().expires_at_epoch_ms > now {
                return true;
            }
            let name = item.payload.name();
            self.preview.skill_names.retain(|skill| skill != name);
            self.preview
                .blocked
                .push(crate::application::update::UpdatePreparationIssue {
                    skill_name: name.to_string(),
                    error: AppError::StalePayload,
                });
            false
        });
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LibraryUpdateExecutionStage {
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

    pub async fn prepare(
        &self,
        request: &UpdateLibrarySkillsRequest,
        cancellation: CancellationSignal,
    ) -> Result<PreparedLibraryUpdate, AppError> {
        validate_request(request)?;
        let mut inspections = Vec::new();
        let mut saved = Vec::new();
        for name in &request.skill_names {
            let snapshot = self
                .subjects
                .snapshot_library(
                    &request.environment,
                    &request.library_id,
                    BTreeSet::from([name.clone()]),
                )
                .await;
            let snapshot = snapshot.and_then(|snapshot| {
                let metadata = snapshot
                    .subjects
                    .iter()
                    .find(|subject| &subject.skill_name == name)
                    .and_then(|subject| subject.projection.metadata())
                    .ok_or(AppError::StaleTarget)?;
                if !crate::application::update::derive_update_capability_from_metadata(metadata)
                    .can_run_update
                {
                    return Err(AppError::InvalidSource {
                        value: name.clone(),
                    });
                }
                crate::core::source_identity::SourceIdentity::from_metadata(metadata)?;
                saved.push(SavedSkillSource {
                    name: name.clone(),
                    metadata: metadata.clone(),
                });
                Ok(snapshot)
            });
            inspections.push((name.clone(), snapshot));
        }
        let acquisitions = if saved.is_empty() {
            Vec::new()
        } else {
            self.skill_source
                .acquire_saved_skills(&request.environment, saved, cancellation.clone())
                .await?
        };
        let mut prepared = PreparedLibraryUpdate {
            request: request.clone(),
            preview: PreparedLibraryUpdatePreview {
                skill_names: Vec::new(),
                blocked: Vec::new(),
                redirected_download_hosts: Vec::new(),
            },
            items: Vec::new(),
            sources: Vec::new(),
            source_by_skill: acquisitions
                .iter()
                .flat_map(|source| {
                    source
                        .skill_names
                        .iter()
                        .map(|name| (name.clone(), source.source_result_id.clone()))
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
                    .map(|error| ErrorReport::from_app_error(error, None)),
            });
        }
        prepared.preview.redirected_download_hosts = hosts.into_iter().collect();
        for (name, initial) in inspections {
            if cancellation.is_cancelled() {
                return Err(AppError::MutationCancelled);
            }
            let outcome = async {
                let initial = initial?;
                let source = acquisitions
                    .iter()
                    .find(|source| source.skill_names.contains(&name))
                    .ok_or(AppError::StalePayload)?;
                let content = source.result.as_ref().map_err(Clone::clone)?;
                let payload = content
                    .validate_member(self.payloads.as_ref(), &request.environment, &name)
                    .await?;
                let latest = self
                    .subjects
                    .snapshot_library(
                        &request.environment,
                        &request.library_id,
                        BTreeSet::from([name.clone()]),
                    )
                    .await?;
                let mut comparison = compare_update_subjects(&initial, &latest, vec![payload])?;
                if !comparison.stale_skill_names.is_empty() {
                    return Err(AppError::StaleTarget);
                }
                let ready = comparison.ready.pop().ok_or(AppError::StaleTarget)?;
                Ok::<_, AppError>((source.source_result_id.clone(), ready))
            }
            .await;
            match outcome {
                Ok(item) => prepared.items.push(item),
                Err(error) => prepared.preview.blocked.push(
                    crate::application::update::UpdatePreparationIssue {
                        skill_name: name,
                        error,
                    },
                ),
            }
        }
        let mut valid = Vec::new();
        for item in prepared.items {
            match self.payloads.pin_verified(item.1.payload.handle()).await {
                Ok(_) => {
                    prepared
                        .preview
                        .skill_names
                        .push(item.1.payload.name().to_string());
                    valid.push(item);
                }
                Err(error) => prepared.preview.blocked.push(
                    crate::application::update::UpdatePreparationIssue {
                        skill_name: item.1.payload.name().to_string(),
                        error,
                    },
                ),
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
        prepared: PreparedLibraryUpdate,
        cancellation: CancellationSignal,
        observe: F,
    ) -> Result<LibraryUpdateResponse, AppError>
    where
        F: Fn(LibraryUpdateExecutionStage),
    {
        let mut results = prepared
            .preview
            .blocked
            .iter()
            .map(|issue| {
                (
                    issue.skill_name.clone(),
                    failed(
                        &issue.skill_name,
                        prepared
                            .source_by_skill
                            .get(&issue.skill_name)
                            .map(String::as_str)
                            .unwrap_or(""),
                        issue.error.clone(),
                    ),
                )
            })
            .collect::<BTreeMap<_, _>>();
        let mut stopped = false;
        for (source, item) in prepared.items {
            let name = item.payload.name().to_string();
            if stopped {
                results.insert(name.clone(), not_run(&name, &source));
                continue;
            }
            let result = async {
                if cancellation.is_cancelled() {
                    return Err(AppError::MutationCancelled);
                }
                observe(LibraryUpdateExecutionStage::Validating);
                self.payloads.pin_verified(item.payload.handle()).await?;
                observe(LibraryUpdateExecutionStage::Committing);
                self.libraries
                    .commit_validated_update(
                        &self.targets,
                        &prepared.request.environment,
                        &prepared.request.library_id,
                        item,
                    )
                    .await
            }
            .await;
            results.insert(
                name.clone(),
                match result {
                    Ok(()) => succeeded(&name, &source),
                    Err(AppError::MutationCancelled) => {
                        stopped = true;
                        cancelled(&name, &source)
                    }
                    Err(error) => failed_commit(&name, &source, error),
                },
            );
        }
        self.response_with_sources(&prepared.request, prepared.sources, results)
            .await
    }

    async fn response_with_sources(
        &self,
        request: &UpdateLibrarySkillsRequest,
        source_results: Vec<UpdateSourceResult>,
        mut results: BTreeMap<String, LibraryUpdateSkillResult>,
    ) -> Result<LibraryUpdateResponse, AppError> {
        let ordered = request
            .skill_names
            .iter()
            .map(|name| {
                results
                    .remove(name)
                    .unwrap_or_else(|| failed(name, "", AppError::StalePayload))
            })
            .collect::<Vec<_>>();
        let outcome = library_update_outcome(&ordered);
        let snapshot = self
            .libraries
            .detail(request.environment.clone(), request.library_id.clone())
            .await;
        let (library, snapshot_error) = match snapshot {
            Ok(library) => (Some(library), None),
            Err(error) => (None, Some(error)),
        };
        Ok(LibraryUpdateResponse {
            sources: source_results,
            results: ordered,
            outcome,
            library,
            membership: LibraryMembershipOutcome {
                snapshot_error,
                ..LibraryMembershipOutcome::default()
            },
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
        LibraryCatalog, LibraryFuture, LibraryId, LibrarySkillRecord, LibrarySkillSourceRecord,
        LibraryUsage, LibraryUsageProvider, LibraryUsageSnapshot, SkillLibraryRecord,
        SkillLibraryRepository, LIBRARY_SCHEMA_VERSION,
    };
    use crate::application::skill_paths::{
        ContentRevision, RootResolutionRevision, TargetRevision,
    };
    use crate::application::skill_source::{
        AcquiredSavedSkillSource, SavedSkillSourceAcquisition, SavedSkillSourceGroup,
        SkillSourceFuture,
    };
    use crate::application::update_subjects::{
        LibraryUpdateSubjectProvider, UpdateSubject, UpdateSubjectSnapshot,
    };
    use crate::core::projects::{ProjectMigrationRegistry, ProjectMigrationState};
    use crate::core::skill_payload::build_skill_payload;
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

    struct FailingSnapshotUsages;

    impl LibraryUsageProvider for FailingSnapshotUsages {
        fn usages<'a>(
            &'a self,
            _environment: &'a EnvironmentRef,
            _library_id: &'a LibraryId,
        ) -> LibraryFuture<'a, Result<Vec<LibraryUsage>, AppError>> {
            Box::pin(async {
                Err(AppError::Io {
                    message: "snapshot unavailable".to_string(),
                })
            })
        }

        fn usage_projection<'a>(
            &'a self,
            _environment: &'a EnvironmentRef,
        ) -> LibraryFuture<'a, Result<LibraryUsageSnapshot, AppError>> {
            Box::pin(async {
                Ok(LibraryUsageSnapshot {
                    projections: Vec::new(),
                    inventory_complete: true,
                    problem_count: 0,
                })
            })
        }
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
                        redirected_download_hosts: self
                            .redirected_download_host
                            .iter()
                            .cloned()
                            .collect(),
                        _leases: Vec::new(),
                        discovery_session: self.discovery.clone(),
                        payloads: self.payloads.clone(),
                        skill_errors: self.skill_errors.clone(),
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
        let prepared = service.prepare(&request, cancellation.clone()).await?;
        service
            .execute_prepared(prepared, cancellation, |_| {})
            .await
    }

    #[tokio::test]
    async fn preparation_rejects_a_changed_source_record() {
        let Fixture {
            _temp,
            repository,
            library_id,
            manager,
            subjects,
            source,
        } = fixture(&["alpha"], None).await;
        let service = LibraryUpdateService::new(
            manager,
            PreviewDriftSubjects {
                fixed: subjects,
                calls: AtomicUsize::new(0),
            },
            source,
            targets(),
            Arc::new(SkillLibraryModule::new(repository)),
        );
        let prepared = service
            .prepare(
                &UpdateLibrarySkillsRequest {
                    environment: EnvironmentRef::Native,
                    library_id,
                    skill_names: vec!["alpha".into()],
                },
                CancellationSignal::default(),
            )
            .await
            .unwrap();
        assert!(prepared.preview.skill_names.is_empty());
        assert_eq!(prepared.preview.blocked[0].error, AppError::StaleTarget);
    }

    #[tokio::test]
    async fn preparation_ignores_an_unrelated_catalog_record_change() {
        let Fixture {
            _temp,
            repository,
            library_id,
            manager,
            subjects,
            source,
        } = fixture(&["alpha"], None).await;
        let service = LibraryUpdateService::new(
            manager,
            UnrelatedRecordDriftSubjects {
                fixed: subjects,
                calls: AtomicUsize::new(0),
            },
            source,
            targets(),
            Arc::new(SkillLibraryModule::new(repository)),
        );
        let prepared = service
            .prepare(
                &UpdateLibrarySkillsRequest {
                    environment: EnvironmentRef::Native,
                    library_id,
                    skill_names: vec!["alpha".into()],
                },
                CancellationSignal::default(),
            )
            .await
            .unwrap();
        assert_eq!(prepared.preview.skill_names, vec!["alpha"]);
        assert!(prepared.preview.blocked.is_empty());
    }

    #[tokio::test]
    async fn commit_rejects_a_source_record_changed_after_preparation() {
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
            LibraryUpdateSubjectProvider::new(repository.clone(), targets()),
            source,
            targets(),
            Arc::new(SkillLibraryModule::new(repository.clone())),
        );
        let prepared = service
            .prepare(
                &UpdateLibrarySkillsRequest {
                    environment: EnvironmentRef::Native,
                    library_id,
                    skill_names: vec!["alpha".into()],
                },
                CancellationSignal::default(),
            )
            .await
            .unwrap();
        let mut changed = repository.load(&EnvironmentRef::Native).await.unwrap();
        set_source_revision(
            &mut changed.libraries[0].skills[0].source_record,
            "external",
        );
        repository
            .save(&EnvironmentRef::Native, &changed)
            .await
            .unwrap();
        let response = service
            .execute_prepared(prepared, CancellationSignal::default(), |_| {})
            .await
            .unwrap();
        assert_eq!(response.results[0].status, LibraryUpdateSkillStatus::Failed);
        let saved = repository.load(&EnvironmentRef::Native).await.unwrap();
        assert_eq!(
            source_revision(&saved.libraries[0].skills[0].source_record).as_deref(),
            Some("external")
        );
        assert_eq!(saved.libraries[0].skills[0].description, "alpha old");
    }

    #[tokio::test]
    async fn cancellation_marks_the_first_requested_skill_and_leaves_the_rest_not_run() {
        let Fixture {
            _temp,
            repository,
            library_id,
            manager,
            source,
            ..
        } = fixture(&["alpha", "beta"], None).await;
        let service = LibraryUpdateService::new(
            manager,
            LibraryUpdateSubjectProvider::new(repository.clone(), targets()),
            source,
            targets(),
            Arc::new(SkillLibraryModule::new(repository)),
        );
        let prepared = service
            .prepare(
                &UpdateLibrarySkillsRequest {
                    environment: EnvironmentRef::Native,
                    library_id,
                    skill_names: vec!["beta".into(), "alpha".into()],
                },
                CancellationSignal::default(),
            )
            .await
            .unwrap();
        let cancellation = CancellationSignal::default();
        cancellation.cancel();
        let response = service
            .execute_prepared(prepared, cancellation, |_| {})
            .await
            .unwrap();
        assert_eq!(response.results[0].skill_name, "beta");
        assert_eq!(
            response.results[0].status,
            LibraryUpdateSkillStatus::Cancelled
        );
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
        let acquisition_calls = source.calls.clone();
        let service = LibraryUpdateService::new(
            manager,
            LibraryUpdateSubjectProvider::new(repository.clone(), targets()),
            source,
            targets(),
            Arc::new(SkillLibraryModule::new(repository.clone())),
        );

        let prepared = service
            .prepare(
                &UpdateLibrarySkillsRequest {
                    environment: EnvironmentRef::Native,
                    library_id: library_id.clone(),
                    skill_names: vec!["alpha".to_string(), "beta".to_string()],
                },
                CancellationSignal::default(),
            )
            .await
            .unwrap();
        assert_eq!(prepared.preview.skill_names, vec!["alpha", "beta"]);
        assert!(prepared.preview.blocked.is_empty());
        assert_eq!(acquisition_calls.load(Ordering::SeqCst), 1);
        let before = repository.load(&EnvironmentRef::Native).await.unwrap();
        assert!(before.libraries[0]
            .skills
            .iter()
            .all(|skill| source_revision(&skill.source_record).as_deref() != Some("new")));
        let response = service
            .execute_prepared(prepared, CancellationSignal::default(), |_| {})
            .await
            .unwrap();
        assert_eq!(acquisition_calls.load(Ordering::SeqCst), 1);

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
                .as_ref()
                .unwrap()
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
    async fn committed_update_survives_a_failed_detail_snapshot() {
        let Fixture {
            _temp,
            repository,
            library_id,
            manager,
            subjects: _,
            source,
        } = fixture(&["alpha"], None).await;
        let service = LibraryUpdateService::new(
            manager,
            LibraryUpdateSubjectProvider::new(repository.clone(), targets()),
            source,
            targets(),
            Arc::new(SkillLibraryModule::with_usages(
                repository.clone(),
                Arc::new(FailingSnapshotUsages),
            )),
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
            LibraryUpdateSkillStatus::Succeeded
        );
        assert!(response.library.is_none());
        assert!(matches!(
            response.membership.snapshot_error,
            Some(AppError::Io { .. })
        ));
        let saved = repository.load(&EnvironmentRef::Native).await.unwrap();
        assert_eq!(saved.libraries[0].skills[0].description, "alpha updated");
    }

    #[tokio::test]
    async fn redirected_source_is_prepared_before_confirmation_without_a_library_write() {
        let Fixture {
            _temp,
            repository,
            library_id,
            manager,
            source,
            ..
        } = fixture(&["alpha"], Some("cdn.example.com")).await;
        let calls = source.calls.clone();
        let service = LibraryUpdateService::new(
            manager,
            LibraryUpdateSubjectProvider::new(repository.clone(), targets()),
            source,
            targets(),
            Arc::new(SkillLibraryModule::new(repository.clone())),
        );
        let prepared = service
            .prepare(
                &UpdateLibrarySkillsRequest {
                    environment: EnvironmentRef::Native,
                    library_id,
                    skill_names: vec!["alpha".into()],
                },
                CancellationSignal::default(),
            )
            .await
            .unwrap();
        assert_eq!(
            prepared.preview.redirected_download_hosts,
            vec!["cdn.example.com"]
        );
        assert_eq!(
            repository
                .load(&EnvironmentRef::Native)
                .await
                .unwrap()
                .libraries[0]
                .skills[0]
                .description,
            "alpha old"
        );
        let response = service
            .execute_prepared(prepared, CancellationSignal::default(), |_| {})
            .await
            .unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            response.results[0].status,
            LibraryUpdateSkillStatus::Succeeded
        );
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
        assert_eq!(
            response.library.as_ref().unwrap().skills[0].description,
            "alpha old"
        );
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
        assert_eq!(
            response.library.as_ref().unwrap().skills[0].description,
            "alpha old"
        );
        assert_eq!(
            response.library.as_ref().unwrap().skills[1].description,
            "beta updated"
        );
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
                retired_skills: Vec::new(),
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

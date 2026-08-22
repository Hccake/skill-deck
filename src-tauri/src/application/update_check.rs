use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;

use crate::application::collection_records::{RecordProjection, SourceRecordRevision};
use crate::application::source_evidence::{
    EvidenceCheckMode, EvidenceCheckRequest, EvidenceCheckResult, EvidenceFreshness,
    ProviderThrottleKey, RemoteEvidenceKey, SkillRevision, SourceEvidenceCoordinator,
};
use crate::application::update::{
    derive_update_capability_from_metadata, CheckUpdateCapability, SkillUpdateCheckStatus,
    SkillUpdateInfo, SourceUpdateCheckInfo, UpdateCapabilityReasonCode, UpdateCheckMode,
    UpdateCheckOutcome, UpdateCheckReasonCode, UpdateCheckRequest, UpdateCheckResponse,
    UpdateCheckSelection,
};
use crate::application::update_subjects::{
    BoundInstalledUpdateSubjectSource, BoundLibraryUpdateSubjectSource,
    InstalledUpdateSubjectSnapshots, LibraryUpdateSubjectSnapshots, UpdateSubjectSnapshot,
    UpdateSubjectSource,
};
use crate::core::mutation::CancellationSignal;
use crate::core::skill_paths::normalize_skill_folder_path;
use crate::core::{NormalizedRef, NormalizedUpdateMetadata, SourceIdentity};
use crate::error::AppError;

#[derive(Debug, Clone)]
struct UpdateCheckSkill {
    name: String,
    metadata: NormalizedUpdateMetadata,
}

struct PreparedUpdateChecks {
    groups: HashMap<RemoteEvidenceKey, UpdateCheckGroup>,
    immediate_results: Vec<SkillUpdateInfo>,
}

#[derive(Clone)]
struct UpdateCheckGroup {
    identity: Arc<SourceIdentity>,
    skills: Vec<UpdateCheckSkill>,
}

pub struct UpdateCheckService<P> {
    subjects: P,
    evidence: UpdateEvidenceModule,
}

pub struct UpdateEvidenceModule {
    evidence: SourceEvidenceCoordinator,
}

pub struct LibraryUpdateCheckService<P> {
    subjects: P,
    evidence: UpdateEvidenceModule,
}

impl<P> LibraryUpdateCheckService<P>
where
    P: LibraryUpdateSubjectSnapshots,
{
    pub fn new(subjects: P, evidence: SourceEvidenceCoordinator) -> Self {
        Self {
            subjects,
            evidence: UpdateEvidenceModule::new(evidence),
        }
    }

    pub async fn check(
        &self,
        environment: crate::environment::types::EnvironmentRef,
        library_id: crate::application::skill_libraries::LibraryId,
        mode: UpdateCheckMode,
        names: BTreeSet<String>,
    ) -> Result<UpdateCheckResponse, AppError> {
        let source = BoundLibraryUpdateSubjectSource::new(&self.subjects, environment, library_id);
        self.evidence.check(&source, mode, names).await
    }
}

impl<P> UpdateCheckService<P>
where
    P: InstalledUpdateSubjectSnapshots,
{
    pub fn new(subjects: P, evidence: SourceEvidenceCoordinator) -> Self {
        Self {
            subjects,
            evidence: UpdateEvidenceModule::new(evidence),
        }
    }

    pub async fn check(
        &self,
        request: &UpdateCheckRequest,
    ) -> Result<UpdateCheckResponse, AppError> {
        let selected = selected_names(request)?;
        let source =
            BoundInstalledUpdateSubjectSource::new(&self.subjects, request.context.clone());
        self.evidence.check(&source, request.mode, selected).await
    }
}

impl UpdateEvidenceModule {
    pub fn new(evidence: SourceEvidenceCoordinator) -> Self {
        Self { evidence }
    }

    pub async fn check<S>(
        &self,
        source: &S,
        mode: UpdateCheckMode,
        names: BTreeSet<String>,
    ) -> Result<UpdateCheckResponse, AppError>
    where
        S: UpdateSubjectSource,
    {
        let initial = source.snapshot(names.clone()).await?;
        let initial_revisions = subject_revisions(&initial);
        let prepared = prepare_update_checks(projection_entries(initial));
        let mut checked = HashMap::new();
        let mut sources = Vec::new();
        let mut detections = tokio::task::JoinSet::new();
        for (key, group) in &prepared.groups {
            let key = key.clone();
            let group = group.clone();
            let evidence = self.evidence.clone();
            let environment = source.environment().clone();
            detections.spawn(async move {
                let result = evidence
                    .check(
                        EvidenceCheckRequest {
                            environment,
                            key: key.clone(),
                            throttle_key: ProviderThrottleKey::from_identity(
                                group.identity.as_ref(),
                            ),
                            mode: match mode {
                                UpdateCheckMode::Automatic => EvidenceCheckMode::Automatic,
                                UpdateCheckMode::Force => EvidenceCheckMode::Force,
                            },
                            requested_skill_paths: group
                                .skills
                                .iter()
                                .filter_map(evidence_path)
                                .collect(),
                            acquisition: Arc::new(group.identity.acquisition().clone()),
                            acquisition_transport_identity: group
                                .identity
                                .acquisition_transport()
                                .clone(),
                        },
                        CancellationSignal::default(),
                    )
                    .await?;
                Ok::<_, AppError>((key, group, result))
            });
        }
        while let Some(joined) = detections.join_next().await {
            let (key, group, result) = joined.map_err(|error| AppError::ExecutionFailed {
                message: format!("source evidence task failed: {error}"),
            })??;
            sources.push(source_info(&group, &result));
            checked.insert(key, result);
        }

        let latest = source.snapshot(names).await?;
        let latest_revisions = subject_revisions(&latest);
        let latest = prepare_update_checks(projection_entries(latest));
        let mut skills = latest.immediate_results;
        for (key, group) in latest.groups {
            skills.extend(group.skills.into_iter().map(|skill| {
                let unchanged =
                    initial_revisions.get(&skill.name) == latest_revisions.get(&skill.name);
                match unchanged.then(|| checked.get(&key)).flatten() {
                    Some(result) => info_from_evidence(skill, result),
                    None => info(
                        skill.name,
                        skill.metadata,
                        false,
                        SkillUpdateCheckStatus::CannotCheck,
                        Some(UpdateCheckReasonCode::UpstreamUnavailable),
                        EvidenceFreshness::Unavailable,
                    ),
                }
            }));
        }
        skills.sort_by(|left, right| left.name.cmp(&right.name));
        sources.sort_by(|left, right| left.source.cmp(&right.source));
        let outcome = update_check_outcome(&sources, &skills);
        Ok(UpdateCheckResponse {
            outcome,
            sources,
            skills,
        })
    }
}

fn update_check_outcome(
    sources: &[SourceUpdateCheckInfo],
    skills: &[SkillUpdateInfo],
) -> UpdateCheckOutcome {
    let source_completed = |source: &&SourceUpdateCheckInfo| {
        matches!(
            source.freshness,
            EvidenceFreshness::Fresh | EvidenceFreshness::Cached
        ) && source
            .last_attempt
            .as_ref()
            .is_none_or(|attempt| attempt.failure.is_none())
    };
    let completed = sources.iter().filter(source_completed).count()
        + skills
            .iter()
            .filter(|skill| skill.status != SkillUpdateCheckStatus::CannotCheck)
            .count();
    let incomplete = sources.len() + skills.len() - completed;

    match (completed, incomplete) {
        (_, 0) => UpdateCheckOutcome::Completed,
        (0, _) => UpdateCheckOutcome::NotCompleted,
        _ => UpdateCheckOutcome::Partial,
    }
}

fn selected_names(request: &UpdateCheckRequest) -> Result<BTreeSet<String>, AppError> {
    match &request.selection {
        UpdateCheckSelection::Skills(identities) => {
            let mut names = BTreeSet::new();
            if identities.is_empty()
                || identities.iter().any(|identity| {
                    identity.context != request.context
                        || identity.skill_name.trim().is_empty()
                        || !names.insert(identity.skill_name.clone())
                })
            {
                return Err(AppError::Validation {
                    field: Some("selection".to_string()),
                    message: "selected Skills must be unique and belong to the request Context"
                        .to_string(),
                });
            }
            Ok(names)
        }
    }
}

fn projection_entries(snapshot: UpdateSubjectSnapshot) -> Vec<(String, RecordProjection)> {
    snapshot
        .subjects
        .into_iter()
        .map(|subject| (subject.skill_name, subject.projection))
        .collect()
}

fn subject_revisions(snapshot: &UpdateSubjectSnapshot) -> HashMap<String, SourceRecordRevision> {
    snapshot
        .subjects
        .iter()
        .map(|subject| {
            (
                subject.skill_name.clone(),
                subject.source_record_revision.clone(),
            )
        })
        .collect()
}

fn prepare_update_checks(entries: Vec<(String, RecordProjection)>) -> PreparedUpdateChecks {
    let mut groups = HashMap::new();
    let mut immediate_results = Vec::new();
    for (name, projection) in entries {
        let metadata = match projection {
            RecordProjection::Available(metadata) => metadata,
            RecordProjection::Missing => {
                immediate_results.push(unavailable_info(
                    name,
                    UpdateCapabilityReasonCode::MissingSource,
                    UpdateCheckReasonCode::MissingSource,
                ));
                continue;
            }
            RecordProjection::Uninterpretable => {
                immediate_results.push(unavailable_info(
                    name,
                    UpdateCapabilityReasonCode::UnsupportedSource,
                    UpdateCheckReasonCode::UnsupportedSource,
                ));
                continue;
            }
        };
        let capability = capability(&metadata);
        if !capability.can_check_for_updates {
            immediate_results.push(info(
                name,
                metadata,
                false,
                SkillUpdateCheckStatus::CannotCheck,
                capability_reason(capability.reason),
                EvidenceFreshness::Unavailable,
            ));
            continue;
        }
        let identity = match SourceIdentity::from_metadata(&metadata) {
            Ok(identity) => Arc::new(identity),
            Err(_) => {
                immediate_results.push(info(
                    name,
                    metadata,
                    false,
                    SkillUpdateCheckStatus::CannotCheck,
                    Some(UpdateCheckReasonCode::UnsupportedSource),
                    EvidenceFreshness::Unavailable,
                ));
                continue;
            }
        };
        let key = RemoteEvidenceKey::from_identity(identity.as_ref());
        groups
            .entry(key)
            .or_insert_with(|| UpdateCheckGroup {
                identity: identity.clone(),
                skills: Vec::new(),
            })
            .skills
            .push(UpdateCheckSkill { name, metadata });
    }
    PreparedUpdateChecks {
        groups,
        immediate_results,
    }
}

fn unavailable_info(
    name: String,
    capability_reason: UpdateCapabilityReasonCode,
    reason: UpdateCheckReasonCode,
) -> SkillUpdateInfo {
    SkillUpdateInfo {
        name,
        source: String::new(),
        has_update: false,
        status: SkillUpdateCheckStatus::CannotCheck,
        capability: CheckUpdateCapability {
            can_run_update: false,
            can_check_for_updates: false,
            reason: Some(capability_reason),
        },
        reason: Some(reason),
        git_ref: None,
        source_url: None,
        skill_path: None,
        freshness: EvidenceFreshness::Unavailable,
    }
}

fn info_from_evidence(skill: UpdateCheckSkill, result: &EvidenceCheckResult) -> SkillUpdateInfo {
    let Some(evidence) = result
        .evidence
        .as_ref()
        .filter(|_| result.evidence_is_fresh)
    else {
        return info(
            skill.name,
            skill.metadata,
            false,
            SkillUpdateCheckStatus::CannotCheck,
            Some(UpdateCheckReasonCode::UpstreamUnavailable),
            result.freshness,
        );
    };
    let path = evidence_path(&skill).unwrap_or_default();
    if !evidence.complete_skill_path_catalog.contains(&path) {
        return info(
            skill.name,
            skill.metadata,
            false,
            SkillUpdateCheckStatus::DeletedUpstream,
            Some(UpdateCheckReasonCode::DeletedUpstream),
            result.freshness,
        );
    }
    let revision = evidence.skill_revisions.get(&path).and_then(|revision| {
        match (skill.metadata.source_type.as_str(), revision) {
            ("github", SkillRevision::GitTreeOid(value))
            | ("git" | "gitlab", SkillRevision::CliContentHash(value))
            | ("well-known", SkillRevision::WellKnownDigest(value)) => Some(value.as_str()),
            _ => None,
        }
    });
    let Some(revision) = revision else {
        return info(
            skill.name,
            skill.metadata,
            false,
            SkillUpdateCheckStatus::CannotCheck,
            Some(UpdateCheckReasonCode::UpstreamUnavailable),
            result.freshness,
        );
    };
    let has_update = skill.metadata.comparison_baseline() != Some(revision);
    info(
        skill.name,
        skill.metadata,
        has_update,
        if has_update {
            SkillUpdateCheckStatus::UpdateAvailable
        } else {
            SkillUpdateCheckStatus::UpToDate
        },
        None,
        result.freshness,
    )
}

fn evidence_path(skill: &UpdateCheckSkill) -> Option<String> {
    if skill.metadata.source_type == "well-known" {
        Some(skill.name.clone())
    } else {
        skill
            .metadata
            .skill_path
            .as_deref()
            .map(normalize_skill_folder_path)
    }
}

fn source_info(group: &UpdateCheckGroup, result: &EvidenceCheckResult) -> SourceUpdateCheckInfo {
    let evidence = result.evidence.as_ref();
    SourceUpdateCheckInfo {
        source: group.identity.sanitized_display().to_string(),
        requested_ref: (!matches!(
            group.identity.remote().provider(),
            crate::core::SourceProvider::WellKnown
        ))
        .then(|| match group.identity.normalized_ref() {
            NormalizedRef::Default => "HEAD".to_string(),
            NormalizedRef::Named(value) => value.clone(),
        }),
        resolved_ref: evidence.map(|entry| entry.snapshot_id.resolved_ref.clone()),
        ref_revision: evidence.map(|entry| entry.snapshot_id.commit_revision.clone()),
        checked_at_epoch_ms: evidence.map(|entry| entry.checked_at_epoch_ms),
        expires_at_epoch_ms: evidence.map(|entry| entry.expires_at_epoch_ms),
        freshness: result.freshness,
        last_attempt: result.last_attempt.clone(),
    }
}

fn capability(metadata: &NormalizedUpdateMetadata) -> CheckUpdateCapability {
    derive_update_capability_from_metadata(metadata)
}

fn capability_reason(reason: Option<UpdateCapabilityReasonCode>) -> Option<UpdateCheckReasonCode> {
    reason.map(|reason| match reason {
        UpdateCapabilityReasonCode::MissingRemoteHash => UpdateCheckReasonCode::MissingRemoteHash,
        UpdateCapabilityReasonCode::MissingSource => UpdateCheckReasonCode::MissingSource,
        UpdateCapabilityReasonCode::UnsupportedSource => UpdateCheckReasonCode::UnsupportedSource,
    })
}

fn info(
    name: String,
    metadata: NormalizedUpdateMetadata,
    has_update: bool,
    status: SkillUpdateCheckStatus,
    reason: Option<UpdateCheckReasonCode>,
    freshness: EvidenceFreshness,
) -> SkillUpdateInfo {
    SkillUpdateInfo {
        name,
        source: metadata.source.clone(),
        has_update,
        status,
        capability: capability(&metadata),
        reason,
        git_ref: metadata.ref_name,
        source_url: metadata.source_url,
        skill_path: metadata.skill_path,
        freshness,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet, VecDeque};
    use std::fs;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use super::*;
    use crate::application::install::InstallFuture;
    use crate::application::mutation::plan::RuntimeRevisions;
    use crate::application::planning_facts::{ScopePlanningSnapshot, ScopePlanningSnapshotSource};
    use crate::application::source_evidence::{
        EvidenceDetectionFailure, EvidenceDetectionOutcome, EvidenceDetectionRequest,
        EvidenceFuture, RemoteEvidenceObservation, SourceEvidenceDetector,
    };
    use crate::application::update_subjects::InstalledUpdateSubjectProvider;
    use crate::core::lossless_lock::{LockSchema, LosslessLockDocument};
    use crate::core::NormalizedUpdateMetadata;
    use crate::environment::agent_environment::AgentRuntimeSnapshot;
    use crate::environment::context_resolver::ResolvedContext;
    use crate::environment::planning::RuntimeTargetFactResolver;
    use crate::environment::runtime::ContextSnapshotRevision;
    use crate::environment::types::{
        EnvironmentRef, EnvironmentStatus, ResourceLocator, SkillLocation, SkillLocationRef,
    };
    use crate::environment::wsl::WslRuntime;

    struct Facts {
        values: Mutex<VecDeque<ScopePlanningSnapshot>>,
        _root: tempfile::TempDir,
    }

    impl Facts {
        fn new(mut values: Vec<ScopePlanningSnapshot>) -> Self {
            let root = tempfile::tempdir().unwrap();
            let skill_root = root.path().join(".agents/skills");
            fs::create_dir_all(&skill_root).unwrap();
            for facts in &mut values {
                facts.resolved_context.home.native_path =
                    root.path().to_string_lossy().into_owned();
                facts.resolved_context.skill_root.native_path =
                    skill_root.to_string_lossy().into_owned();
                facts.resolved_context.lock.native_path = root
                    .path()
                    .join("skills-lock.json")
                    .to_string_lossy()
                    .into_owned();
            }
            Self {
                values: Mutex::new(values.into()),
                _root: root,
            }
        }
    }

    impl ScopePlanningSnapshotSource for Facts {
        fn snapshot<'a>(
            &'a self,
            _context: &'a SkillLocationRef,
        ) -> InstallFuture<'a, Result<ScopePlanningSnapshot, AppError>> {
            Box::pin(async move {
                let mut values = self.values.lock().unwrap();
                Ok(if values.len() > 1 {
                    values.pop_front().unwrap()
                } else {
                    values.front().unwrap().clone()
                })
            })
        }
    }

    struct RecordingDetector {
        requested: Mutex<Vec<BTreeSet<String>>>,
        outcome: EvidenceDetectionOutcome,
    }

    impl SourceEvidenceDetector for RecordingDetector {
        fn detect<'a>(
            &'a self,
            request: EvidenceDetectionRequest,
            _previous: Option<crate::application::source_evidence::RemoteEvidenceEntry>,
            _cancellation: CancellationSignal,
        ) -> EvidenceFuture<'a> {
            Box::pin(async move {
                self.requested
                    .lock()
                    .unwrap()
                    .push(request.requested_skill_paths);
                Ok(self.outcome.clone())
            })
        }
    }

    struct ConcurrentDetector {
        active: AtomicUsize,
        peak: AtomicUsize,
    }

    impl ConcurrentDetector {
        fn new() -> Self {
            Self {
                active: AtomicUsize::new(0),
                peak: AtomicUsize::new(0),
            }
        }

        fn peak(&self) -> usize {
            self.peak.load(Ordering::SeqCst)
        }
    }

    impl SourceEvidenceDetector for ConcurrentDetector {
        fn detect<'a>(
            &'a self,
            request: EvidenceDetectionRequest,
            _previous: Option<crate::application::source_evidence::RemoteEvidenceEntry>,
            _cancellation: CancellationSignal,
        ) -> EvidenceFuture<'a> {
            Box::pin(async move {
                let active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
                self.peak.fetch_max(active, Ordering::SeqCst);
                tokio::time::sleep(Duration::from_millis(25)).await;
                self.active.fetch_sub(1, Ordering::SeqCst);
                let paths = request.requested_skill_paths;
                Ok(observation(
                    &paths.iter().map(String::as_str).collect::<Vec<_>>(),
                ))
            })
        }
    }

    fn context() -> SkillLocationRef {
        SkillLocationRef {
            environment: EnvironmentRef::Native,
            scope: SkillLocation::Global,
        }
    }

    fn selection(names: &[&str]) -> UpdateCheckSelection {
        UpdateCheckSelection::Skills(
            names
                .iter()
                .map(|name| crate::application::resources::SkillIdentity {
                    context: context(),
                    skill_name: (*name).to_string(),
                })
                .collect(),
        )
    }

    fn facts(lock: &str) -> ScopePlanningSnapshot {
        let context = context();
        ScopePlanningSnapshot {
            resolved_context: ResolvedContext {
                context: context.clone(),
                project: None,
                home: ResourceLocator {
                    environment: EnvironmentRef::Native,
                    native_path: "/tmp".to_string(),
                },
                skill_root: ResourceLocator {
                    environment: EnvironmentRef::Native,
                    native_path: "/tmp/.agents/skills".to_string(),
                },
                lock: ResourceLocator {
                    environment: EnvironmentRef::Native,
                    native_path: "/tmp/skills-lock.json".to_string(),
                },
            },
            agent_runtime: AgentRuntimeSnapshot {
                registry_revision: "registry".to_string(),
                environment_revision: "environment".to_string(),
                environment: EnvironmentRef::Native,
                availability: EnvironmentStatus::Available,
                project_path: None,
                agents: BTreeMap::new(),
            },
            revisions: RuntimeRevisions {
                registry: "registry".to_string(),
                environment: "environment".to_string(),
                context: ContextSnapshotRevision::parse("context").unwrap(),
            },
            lock_schema: LockSchema::Global,
            lock_document: LosslessLockDocument::parse(lock.as_bytes()).unwrap(),
            eve_targets: Vec::new(),
        }
    }

    fn observation(paths: &[&str]) -> EvidenceDetectionOutcome {
        let catalog = paths
            .iter()
            .map(|path| (*path).to_string())
            .collect::<BTreeSet<_>>();
        EvidenceDetectionOutcome::Modified(RemoteEvidenceObservation {
            snapshot_id: crate::application::source_evidence::RemoteSnapshotId::new(
                NormalizedRef::Named("main".to_string()),
                "main",
                "revision-1",
            ),
            provider_validation: None,
            complete_skill_path_catalog: catalog,
            skill_revisions: paths
                .iter()
                .map(|path| {
                    (
                        (*path).to_string(),
                        SkillRevision::GitTreeOid(format!("tree-{path}")),
                    )
                })
                .collect(),
            snapshot_facts: None,
        })
    }

    fn source_result(
        freshness: EvidenceFreshness,
        last_attempt: Option<crate::application::source_evidence::EvidenceAttempt>,
    ) -> SourceUpdateCheckInfo {
        SourceUpdateCheckInfo {
            source: "owner/repo".to_string(),
            requested_ref: Some("main".to_string()),
            resolved_ref: Some("refs/heads/main".to_string()),
            ref_revision: Some("revision-1".to_string()),
            checked_at_epoch_ms: Some(1_000),
            expires_at_epoch_ms: Some(3_601_000),
            freshness,
            last_attempt,
        }
    }

    type TestSubjectProvider = InstalledUpdateSubjectProvider<Facts, RuntimeTargetFactResolver>;

    fn service(
        values: Vec<ScopePlanningSnapshot>,
        detector: Arc<RecordingDetector>,
    ) -> UpdateCheckService<TestSubjectProvider> {
        UpdateCheckService::new(
            InstalledUpdateSubjectProvider::new(
                Facts::new(values),
                RuntimeTargetFactResolver::new(Arc::new(WslRuntime::default())),
            ),
            SourceEvidenceCoordinator::with_clock(detector, || 1_000),
        )
    }

    #[test]
    fn preparation_groups_checkable_entries_and_keeps_missing_remote_hash_runnable() {
        let prepared = prepare_update_checks(vec![
            (
                "toolkit".to_string(),
                RecordProjection::Available(NormalizedUpdateMetadata {
                    source: "owner/repo".to_string(),
                    source_type: "github".to_string(),
                    source_url: Some("https://github.com/owner/repo".to_string()),
                    ref_name: Some("main".to_string()),
                    skill_path: Some("skills/toolkit".to_string()),
                    remote_hash: Some("old-hash".to_string()),
                    computed_hash: None,
                    well_known_digest: None,
                }),
            ),
            (
                "legacy".to_string(),
                RecordProjection::Available(NormalizedUpdateMetadata {
                    source: "owner/repo".to_string(),
                    source_type: "github".to_string(),
                    source_url: Some("https://github.com/owner/repo".to_string()),
                    ref_name: Some("main".to_string()),
                    skill_path: Some("skills/legacy".to_string()),
                    remote_hash: None,
                    computed_hash: None,
                    well_known_digest: None,
                }),
            ),
        ]);

        assert_eq!(prepared.groups.len(), 1);
        assert_eq!(prepared.groups.values().next().unwrap().skills.len(), 1);
        assert_eq!(prepared.immediate_results.len(), 1);
        assert!(prepared.immediate_results[0].capability.can_run_update);
        assert!(
            !prepared.immediate_results[0]
                .capability
                .can_check_for_updates
        );
    }

    #[tokio::test]
    async fn all_selection_unions_same_source_paths_into_one_detection() {
        let lock = r#"{"skills":{"alpha":{"source":"owner/repo","sourceType":"github","sourceUrl":"https://github.com/owner/repo","ref":"main","skillPath":"skills/alpha/SKILL.md","skillFolderHash":"tree-skills/alpha"},"beta":{"source":"owner/repo","sourceType":"github","sourceUrl":"https://github.com/owner/repo","ref":"main","skillPath":"skills/beta","skillFolderHash":"tree-skills/beta"}}}"#;
        let detector = Arc::new(RecordingDetector {
            requested: Mutex::new(Vec::new()),
            outcome: observation(&["skills/alpha", "skills/beta"]),
        });
        let response = service(vec![facts(lock), facts(lock)], detector.clone())
            .check(&UpdateCheckRequest {
                context: context(),
                mode: UpdateCheckMode::Force,
                selection: selection(&["alpha", "beta"]),
            })
            .await
            .unwrap();

        assert_eq!(
            detector.requested.lock().unwrap().as_slice(),
            &[BTreeSet::from([
                "skills/alpha".to_string(),
                "skills/beta".to_string(),
            ])]
        );
        assert_eq!(response.skills.len(), 2);
        assert_eq!(
            response.outcome,
            crate::application::update::UpdateCheckOutcome::Completed
        );
    }

    #[tokio::test]
    async fn successful_and_uncheckable_skills_produce_partial_outcome() {
        let lock = r#"{"skills":{"alpha":{"source":"owner/repo","sourceType":"github","sourceUrl":"https://github.com/owner/repo","ref":"main","skillPath":"skills/alpha","skillFolderHash":"tree-skills/alpha"},"legacy":{"source":"owner/repo","sourceType":"github","sourceUrl":"https://github.com/owner/repo","ref":"main","skillPath":"skills/legacy"}}}"#;
        let detector = Arc::new(RecordingDetector {
            requested: Mutex::new(Vec::new()),
            outcome: observation(&["skills/alpha"]),
        });

        let response = service(vec![facts(lock), facts(lock)], detector)
            .check(&UpdateCheckRequest {
                context: context(),
                mode: UpdateCheckMode::Force,
                selection: selection(&["alpha", "legacy"]),
            })
            .await
            .unwrap();

        assert_eq!(
            response.outcome,
            crate::application::update::UpdateCheckOutcome::Partial
        );
    }

    #[tokio::test]
    async fn selected_missing_and_uninterpretable_records_remain_in_the_report() {
        let lock = r#"{"skills":{"available":{"source":"owner/repo","sourceType":"github","sourceUrl":"https://github.com/owner/repo","ref":"main","skillPath":"skills/available","skillFolderHash":"tree-available"},"broken":{"source":42}}}"#;
        let detector = Arc::new(RecordingDetector {
            requested: Mutex::new(Vec::new()),
            outcome: observation(&["skills/available"]),
        });
        let identities = ["available", "broken", "missing"]
            .into_iter()
            .map(|skill_name| crate::application::resources::SkillIdentity {
                context: context(),
                skill_name: skill_name.to_string(),
            })
            .collect();

        let response = service(vec![facts(lock), facts(lock)], detector.clone())
            .check(&UpdateCheckRequest {
                context: context(),
                mode: UpdateCheckMode::Force,
                selection: UpdateCheckSelection::Skills(identities),
            })
            .await
            .unwrap();

        assert_eq!(
            detector.requested.lock().unwrap().as_slice(),
            &[BTreeSet::from(["skills/available".to_string()])]
        );
        assert_eq!(response.skills.len(), 3);
        assert_eq!(response.skills[0].name, "available");
        assert_eq!(response.skills[1].name, "broken");
        assert_eq!(
            response.skills[1].reason,
            Some(UpdateCheckReasonCode::UnsupportedSource)
        );
        assert_eq!(response.skills[2].name, "missing");
        assert_eq!(
            response.skills[2].reason,
            Some(UpdateCheckReasonCode::MissingSource)
        );
    }

    #[tokio::test]
    async fn failed_sources_produce_not_completed_outcome() {
        let lock = r#"{"skills":{"alpha":{"source":"owner/repo","sourceType":"github","sourceUrl":"https://github.com/owner/repo","ref":"main","skillPath":"skills/alpha","skillFolderHash":"tree-skills/alpha"}}}"#;
        let detector = Arc::new(RecordingDetector {
            requested: Mutex::new(Vec::new()),
            outcome: EvidenceDetectionOutcome::Failed(EvidenceDetectionFailure::network("offline")),
        });

        let response = service(vec![facts(lock), facts(lock)], detector)
            .check(&UpdateCheckRequest {
                context: context(),
                mode: UpdateCheckMode::Force,
                selection: selection(&["alpha"]),
            })
            .await
            .unwrap();

        assert_eq!(
            response.outcome,
            crate::application::update::UpdateCheckOutcome::NotCompleted
        );
    }

    #[test]
    fn cached_source_without_current_environment_attempt_is_completed() {
        let sources = [source_result(EvidenceFreshness::Cached, None)];

        assert_eq!(
            update_check_outcome(&sources, &[]),
            crate::application::update::UpdateCheckOutcome::Completed
        );
    }

    #[test]
    fn cached_source_with_failed_refresh_is_not_completed() {
        let sources = [source_result(
            EvidenceFreshness::Cached,
            Some(crate::application::source_evidence::EvidenceAttempt {
                checked_at_epoch_ms: 2_000,
                failure: Some(EvidenceDetectionFailure::network("offline")),
            }),
        )];

        assert_eq!(
            update_check_outcome(&sources, &[]),
            crate::application::update::UpdateCheckOutcome::NotCompleted
        );
    }

    #[tokio::test]
    async fn skill_selection_checks_only_the_selected_source_group() {
        let lock = r#"{"skills":{"alpha":{"source":"owner/alpha","sourceType":"github","sourceUrl":"https://github.com/owner/alpha","ref":"main","skillPath":"skills/alpha","skillFolderHash":"tree-skills/alpha"},"beta":{"source":"owner/beta","sourceType":"github","sourceUrl":"https://github.com/owner/beta","ref":"main","skillPath":"skills/beta","skillFolderHash":"tree-skills/beta"}}}"#;
        let detector = Arc::new(RecordingDetector {
            requested: Mutex::new(Vec::new()),
            outcome: observation(&["skills/alpha"]),
        });
        let response = service(vec![facts(lock), facts(lock)], detector.clone())
            .check(&UpdateCheckRequest {
                context: context(),
                mode: UpdateCheckMode::Force,
                selection: UpdateCheckSelection::Skills(vec![
                    crate::application::resources::SkillIdentity {
                        context: context(),
                        skill_name: "alpha".to_string(),
                    },
                ]),
            })
            .await
            .unwrap();

        assert_eq!(
            detector.requested.lock().unwrap().as_slice(),
            &[BTreeSet::from(["skills/alpha".to_string()])]
        );
        assert_eq!(response.skills.len(), 1);
        assert_eq!(response.skills[0].name, "alpha");
    }

    #[test]
    fn complete_catalog_deletion_sparse_unknown_and_stale_evidence_stay_distinct() {
        let skill = || UpdateCheckSkill {
            name: "alpha".to_string(),
            metadata: NormalizedUpdateMetadata {
                source: "owner/repo".to_string(),
                source_type: "github".to_string(),
                source_url: Some("https://github.com/owner/repo".to_string()),
                ref_name: Some("main".to_string()),
                skill_path: Some("skills/alpha".to_string()),
                remote_hash: Some("tree-old".to_string()),
                computed_hash: None,
                well_known_digest: None,
            },
        };
        let result = |catalog: BTreeSet<String>,
                      revisions: BTreeMap<String, SkillRevision>,
                      fresh: bool,
                      freshness: EvidenceFreshness| EvidenceCheckResult {
            evidence: Some(crate::application::source_evidence::RemoteEvidenceEntry {
                checked_at_epoch_ms: 1_000,
                expires_at_epoch_ms: if fresh { 901_000 } else { 999 },
                snapshot_id: crate::application::source_evidence::RemoteSnapshotId::new(
                    NormalizedRef::Named("main".to_string()),
                    "main",
                    "revision-1",
                ),
                provider_validation: None,
                complete_skill_path_catalog: catalog,
                skill_revisions: revisions,
            }),
            evidence_is_fresh: fresh,
            freshness,
            last_attempt: None,
        };

        let deleted = info_from_evidence(
            skill(),
            &result(
                BTreeSet::new(),
                BTreeMap::new(),
                true,
                EvidenceFreshness::Fresh,
            ),
        );
        assert_eq!(deleted.status, SkillUpdateCheckStatus::DeletedUpstream);

        let sparse = info_from_evidence(
            skill(),
            &result(
                BTreeSet::from(["skills/alpha".to_string()]),
                BTreeMap::new(),
                true,
                EvidenceFreshness::Fresh,
            ),
        );
        assert_eq!(sparse.status, SkillUpdateCheckStatus::CannotCheck);
        assert_eq!(
            sparse.reason,
            Some(UpdateCheckReasonCode::UpstreamUnavailable)
        );

        let stale = info_from_evidence(
            skill(),
            &result(
                BTreeSet::from(["skills/alpha".to_string()]),
                BTreeMap::from([(
                    "skills/alpha".to_string(),
                    SkillRevision::GitTreeOid("tree-new".to_string()),
                )]),
                false,
                EvidenceFreshness::Stale,
            ),
        );
        assert_eq!(stale.status, SkillUpdateCheckStatus::CannotCheck);
        assert_eq!(stale.freshness, EvidenceFreshness::Stale);
    }

    #[test]
    fn well_known_digest_reports_current_changed_and_deleted_upstream() {
        let skill = || UpdateCheckSkill {
            name: "demo".to_string(),
            metadata: NormalizedUpdateMetadata {
                source: "skills.example.com".to_string(),
                source_type: "well-known".to_string(),
                source_url: Some("https://skills.example.com/catalog/index.json".to_string()),
                ref_name: None,
                skill_path: None,
                remote_hash: None,
                computed_hash: Some("local-content".to_string()),
                well_known_digest: Some("sha256:old".to_string()),
            },
        };
        let evidence = |catalog: BTreeSet<String>, digest: Option<&str>| EvidenceCheckResult {
            evidence: Some(crate::application::source_evidence::RemoteEvidenceEntry {
                checked_at_epoch_ms: 1_000,
                expires_at_epoch_ms: 901_000,
                snapshot_id: crate::application::source_evidence::RemoteSnapshotId::new(
                    NormalizedRef::Default,
                    "https://skills.example.com/catalog/index.json",
                    "catalog-revision",
                ),
                provider_validation: None,
                complete_skill_path_catalog: catalog,
                skill_revisions: digest
                    .map(|digest| {
                        BTreeMap::from([(
                            "demo".to_string(),
                            SkillRevision::WellKnownDigest(digest.to_string()),
                        )])
                    })
                    .unwrap_or_default(),
            }),
            evidence_is_fresh: true,
            freshness: EvidenceFreshness::Fresh,
            last_attempt: None,
        };

        let current = info_from_evidence(
            skill(),
            &evidence(BTreeSet::from(["demo".to_string()]), Some("sha256:old")),
        );
        assert_eq!(current.status, SkillUpdateCheckStatus::UpToDate);

        let changed = info_from_evidence(
            skill(),
            &evidence(BTreeSet::from(["demo".to_string()]), Some("sha256:new")),
        );
        assert_eq!(changed.status, SkillUpdateCheckStatus::UpdateAvailable);

        let deleted = info_from_evidence(skill(), &evidence(BTreeSet::new(), None));
        assert_eq!(deleted.status, SkillUpdateCheckStatus::DeletedUpstream);
        assert_eq!(deleted.reason, Some(UpdateCheckReasonCode::DeletedUpstream));

        let group = UpdateCheckGroup {
            identity: Arc::new(SourceIdentity::from_metadata(&skill().metadata).unwrap()),
            skills: vec![skill()],
        };
        let source = source_info(
            &group,
            &evidence(BTreeSet::from(["demo".to_string()]), Some("sha256:old")),
        );
        assert_eq!(source.requested_ref, None);
    }

    #[tokio::test]
    async fn lock_reread_never_applies_old_source_evidence_to_a_changed_lock() {
        let initial = r#"{"skills":{"alpha":{"source":"owner/repo","sourceType":"github","sourceUrl":"https://github.com/owner/repo","ref":"main","skillPath":"skills/alpha","skillFolderHash":"tree-skills/alpha"}}}"#;
        let changed = r#"{"skills":{"alpha":{"source":"other/repo","sourceType":"github","sourceUrl":"https://github.com/other/repo","ref":"main","skillPath":"skills/alpha","skillFolderHash":"tree-skills/alpha"}}}"#;
        let detector = Arc::new(RecordingDetector {
            requested: Mutex::new(Vec::new()),
            outcome: observation(&["skills/alpha"]),
        });
        let response = service(vec![facts(initial), facts(changed)], detector)
            .check(&UpdateCheckRequest {
                context: context(),
                mode: UpdateCheckMode::Force,
                selection: selection(&["alpha"]),
            })
            .await
            .unwrap();

        assert_eq!(response.skills.len(), 1);
        assert_eq!(response.skills[0].source, "other/repo");
        assert_eq!(
            response.skills[0].status,
            SkillUpdateCheckStatus::CannotCheck
        );
        assert_eq!(
            response.skills[0].reason,
            Some(UpdateCheckReasonCode::UpstreamUnavailable)
        );
    }

    #[tokio::test]
    async fn lock_reread_rejects_evidence_when_only_the_owned_record_fact_changes() {
        let initial = r#"{"skills":{"alpha":{"source":"owner/repo","sourceType":"github","sourceUrl":"https://github.com/owner/repo","ref":"main","skillPath":"skills/alpha","skillFolderHash":"tree-old","futureEntry":1}}}"#;
        let changed = r#"{"skills":{"alpha":{"source":"owner/repo","sourceType":"github","sourceUrl":"https://github.com/owner/repo","ref":"main","skillPath":"skills/alpha","skillFolderHash":"tree-old","futureEntry":2}}}"#;
        let detector = Arc::new(RecordingDetector {
            requested: Mutex::new(Vec::new()),
            outcome: observation(&["skills/alpha"]),
        });
        let response = service(vec![facts(initial), facts(changed)], detector)
            .check(&UpdateCheckRequest {
                context: context(),
                mode: UpdateCheckMode::Force,
                selection: selection(&["alpha"]),
            })
            .await
            .unwrap();

        assert_eq!(response.skills.len(), 1);
        assert_eq!(
            response.skills[0].status,
            SkillUpdateCheckStatus::CannotCheck,
        );
        assert_eq!(
            response.skills[0].reason,
            Some(UpdateCheckReasonCode::UpstreamUnavailable),
        );
    }

    #[tokio::test]
    async fn independent_source_checks_use_the_global_concurrency_limit() {
        let skills = (0..5)
            .map(|index| {
                format!(
                    r#""skill-{index}":{{"source":"owner/repo-{index}","sourceType":"github","sourceUrl":"https://github.com/owner/repo-{index}","ref":"main","skillPath":"skills/demo","skillFolderHash":"tree-skills/demo"}}"#
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        let lock = format!(r#"{{"skills":{{{skills}}}}}"#);
        let detector = Arc::new(ConcurrentDetector::new());
        let service = UpdateCheckService::new(
            InstalledUpdateSubjectProvider::new(
                Facts::new(vec![facts(&lock), facts(&lock)]),
                RuntimeTargetFactResolver::new(Arc::new(WslRuntime::default())),
            ),
            SourceEvidenceCoordinator::with_clock(detector.clone(), || 1_000),
        );

        let response = service
            .check(&UpdateCheckRequest {
                context: context(),
                mode: UpdateCheckMode::Force,
                selection: UpdateCheckSelection::Skills(
                    (0..5)
                        .map(|index| crate::application::resources::SkillIdentity {
                            context: context(),
                            skill_name: format!("skill-{index}"),
                        })
                        .collect(),
                ),
            })
            .await
            .unwrap();

        assert_eq!(response.sources.len(), 5);
        assert_eq!(detector.peak(), 4);
    }
}

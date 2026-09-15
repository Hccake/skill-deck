use std::collections::BTreeMap;
use std::sync::Arc;

use crate::application::agent_registry_source::AgentRegistrySnapshotSource;
use crate::application::git_transport::GitSourceTransport;
use crate::application::github_access::GithubTreeAccess;
use crate::application::mutation::coordinator::RuntimeRevisionSource;
use crate::application::payload_session::{
    DiscoverySessionHandle, DiscoverySourceLocation, PayloadPlanningMetadata, PayloadSessionManager,
};
use crate::application::skill_libraries::SkillLibraryRepository;
use crate::application::skill_source::SkillSourceModule;
use crate::application::source_acquisition::{
    retain_discovered_source, AcquireSelectedPayloadsRequest, GitSourceDiscovery,
    InternalSkillVisibility, ManagedDownloadedDirectory, RetainedSourceOptions,
    SelectedPayloadAcquisitionService, SourceDiscoveryPolicy,
};
use crate::application::source_evidence::{
    RemoteSnapshotId, SkillRevision, SourceEvidenceCoordinator,
};
use crate::application::source_evidence_provider::RuntimeSourceEvidenceDetector;
use crate::application::source_snapshot_reuse::SourceSnapshotReuseIndex;
use crate::application::update::{
    AcquiredUpdateSource, UpdateAcquisitionGroup, UpdateFuture, UpdateService,
    UpdateSourceAcquisition,
};
use crate::application::update_check::UpdateCheckService;
use crate::application::update_planner::ConcreteUpdatePlanner;
use crate::application::update_records::{
    InstalledUpdateRecordProvider, LibraryUpdateRecordProvider,
};
use crate::application::update_subjects::LibraryUpdateSubjectProvider;
#[cfg(test)]
use crate::application::wellknown_access::UnavailableWellKnownAccess;
use crate::application::wellknown_access::WellKnownAccess;
#[cfg(test)]
use crate::application::wsl_source_access::UnavailableWslSourceAccess;
use crate::application::wsl_source_access::WslSourceAccess;
use crate::core::compute_local_ref_revision;
use crate::core::skill_paths::normalize_skill_folder_path;
use crate::core::source_identity::{NormalizedRef, SourceProvider};
use crate::environment::planning::RuntimeTargetFactResolver;
use crate::environment::types::EnvironmentRef;
use crate::environment::wsl::WslRuntime;
use crate::error::AppError;
use crate::runtime::plan_runner::{RuntimeExecutionDependencies, RuntimePlanExecutor};
use crate::runtime::planning_facts::RuntimePlanningFactSource;

#[derive(Clone)]
pub struct RuntimeSkillSourceModule {
    payloads: Arc<PayloadSessionManager>,
    snapshots: Arc<SourceSnapshotReuseIndex>,
    evidence: SourceEvidenceCoordinator,
    git_transport: Arc<dyn GitSourceTransport>,
    wsl_source: Arc<dyn WslSourceAccess>,
    wellknown: Arc<dyn WellKnownAccess>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RetainedSnapshotAction {
    Reuse,
    Reacquire,
    Cancelled,
}

fn retained_snapshot_action(
    _environment: &EnvironmentRef,
    retained_revision: Option<&str>,
    probe: Result<&str, &AppError>,
) -> RetainedSnapshotAction {
    if retained_revision.is_none() {
        return RetainedSnapshotAction::Reacquire;
    }
    match probe {
        Ok(actual_revision) if Some(actual_revision) == retained_revision => {
            RetainedSnapshotAction::Reuse
        }
        Err(AppError::MutationCancelled) => RetainedSnapshotAction::Cancelled,
        Ok(_) | Err(_) => RetainedSnapshotAction::Reacquire,
    }
}

impl RuntimeSkillSourceModule {
    pub fn new(
        payloads: Arc<PayloadSessionManager>,
        snapshots: Arc<SourceSnapshotReuseIndex>,
        evidence: SourceEvidenceCoordinator,
        git_transport: Arc<dyn GitSourceTransport>,
        wsl_source: Arc<dyn WslSourceAccess>,
        wellknown: Arc<dyn WellKnownAccess>,
    ) -> Self {
        Self {
            payloads,
            snapshots,
            evidence,
            git_transport,
            wsl_source,
            wellknown,
        }
    }

    #[cfg(test)]
    pub(crate) fn with_git_transport(
        payloads: Arc<PayloadSessionManager>,
        snapshots: Arc<SourceSnapshotReuseIndex>,
        evidence: SourceEvidenceCoordinator,
        git_transport: Arc<dyn GitSourceTransport>,
    ) -> Self {
        Self {
            payloads,
            snapshots,
            evidence,
            git_transport,
            wsl_source: Arc::new(UnavailableWslSourceAccess),
            wellknown: Arc::new(UnavailableWellKnownAccess),
        }
    }

    async fn acquire_group(
        &self,
        group: &UpdateAcquisitionGroup,
        cancellation: crate::core::mutation::CancellationSignal,
    ) -> Result<AcquiredUpdateSource, AppError> {
        if cancellation.is_cancelled() {
            return Err(AppError::MutationCancelled);
        }
        let provider = group.evidence_key.remote.provider();
        let reusable = if provider != &SourceProvider::WellKnown {
            self.snapshots.candidate(&group.key, self.payloads.as_ref())
        } else {
            None
        };
        let discovery_session = match reusable {
            Some((retained_revision, discovery)) => {
                let probe_source = group.descriptor.source().to_string();
                let probe_ref = group.descriptor.git_ref().map(ToString::to_string);
                let probe_cancellation = cancellation.clone();
                let git_transport = Arc::clone(&self.git_transport);
                let probed = match &group.environment {
                    EnvironmentRef::Native => tokio::task::spawn_blocking(move || {
                        git_transport.probe_ref_revision(
                            &probe_source,
                            probe_ref.as_deref(),
                            probe_cancellation,
                        )
                    })
                    .await
                    .map_err(|_| AppError::StaleEnvironment)?,
                    EnvironmentRef::Wsl { distro_name } => {
                        self.wsl_source
                            .probe_ref(
                                distro_name,
                                &probe_source,
                                probe_ref.as_deref(),
                                probe_cancellation,
                            )
                            .await
                    }
                };
                let action = retained_snapshot_action(
                    &group.environment,
                    Some(&retained_revision),
                    probed.as_ref().map(String::as_str),
                );
                match action {
                    RetainedSnapshotAction::Reuse => discovery,
                    RetainedSnapshotAction::Cancelled => return Err(AppError::MutationCancelled),
                    RetainedSnapshotAction::Reacquire => {
                        self.snapshots.invalidate(&group.key);
                        self.discover_group(group, cancellation.clone()).await?
                    }
                }
            }
            None => self.discover_group(group, cancellation.clone()).await?,
        };
        let result = async {
            if cancellation.is_cancelled() {
                return Err(AppError::MutationCancelled);
            }
            let selected = SelectedPayloadAcquisitionService::new(self.payloads.clone());
            let mut skill_errors = Vec::new();
            if provider != &SourceProvider::WellKnown {
                for skill in &group.skills {
                    if let Err(error) = selected
                        .ensure_saved_path(
                            &discovery_session,
                            skill.skill_path(),
                            cancellation.clone(),
                        )
                        .await
                    {
                        if error == AppError::MutationCancelled {
                            return Err(error);
                        }
                        let error = if matches!(error, AppError::PathNotFound { .. }) {
                            AppError::UpstreamSkillDeleted {
                                skill_name: skill.name.clone(),
                            }
                        } else {
                            error
                        };
                        skill_errors.push((skill.name.clone(), error));
                    }
                }
            }
            let retained = self.payloads.source_snapshot(&discovery_session)?;
            let mut selected_paths = Vec::with_capacity(group.skills.len());
            let mut selected_skills = Vec::with_capacity(group.skills.len());
            for locked in &group.skills {
                if skill_errors.iter().any(|(name, _)| name == &locked.name) {
                    continue;
                }
                if let Some(reason) = retained.member_failure(&locked.name) {
                    skill_errors.push((
                        locked.name.clone(),
                        AppError::WellKnownSourceFailed { reason },
                    ));
                    continue;
                }
                let available = retained.skills().find(|available| {
                    retained_skill_matches(
                        provider,
                        &locked.name,
                        locked.skill_path(),
                        &available.skill_name,
                        &available.relative_path,
                    )
                });
                if let Some(available) = available {
                    selected_paths.push(available.relative_path.clone());
                    selected_skills.push(locked);
                    continue;
                }
                if let Some(renamed) = retained.skills().find(|available| {
                    normalize_skill_folder_path(&available.relative_path)
                        == normalize_skill_folder_path(locked.skill_path())
                }) {
                    skill_errors.push((
                        locked.name.clone(),
                        AppError::UpstreamSkillNameChanged {
                            expected_name: locked.name.clone(),
                            actual_name: renamed.skill_name.clone(),
                        },
                    ));
                } else {
                    skill_errors.push((
                        locked.name.clone(),
                        AppError::UpstreamSkillDeleted {
                            skill_name: locked.name.clone(),
                        },
                    ));
                }
            }
            let mut handles = Vec::new();
            let mut acquired_skills = Vec::new();
            let mut leases = Vec::new();
            let mut skill_revisions = BTreeMap::new();
            for (locked, path) in selected_skills.into_iter().zip(selected_paths) {
                let acquired = async {
                    let handle = SelectedPayloadAcquisitionService::new(self.payloads.clone())
                        .acquire(AcquireSelectedPayloadsRequest {
                            discovery_session: discovery_session.clone(),
                            skill_paths: vec![path],
                        })
                        .await?
                        .into_iter()
                        .next()
                        .ok_or(AppError::StalePayload)?;
                    let lease = self.payloads.pin_verified(&handle).await?;
                    let revision = acquisition_skill_revision(provider, lease.planning_metadata())?;
                    Ok::<_, AppError>((handle, lease, revision))
                }
                .await;
                match acquired {
                    Ok((handle, lease, revision)) => {
                        skill_revisions.insert(
                            snapshot_skill_key(provider, &locked.name, locked.skill_path()),
                            revision,
                        );
                        acquired_skills.push(locked.name.clone());
                        handles.push(handle);
                        leases.push(Arc::new(lease));
                    }
                    Err(AppError::MutationCancelled) => return Err(AppError::MutationCancelled),
                    Err(error) => skill_errors.push((locked.name.clone(), error)),
                }
            }
            if cancellation.is_cancelled() {
                return Err(AppError::MutationCancelled);
            }
            let ref_revision = if provider == &SourceProvider::WellKnown {
                crate::application::mutation::plan::stable_digest(&skill_revisions)?
            } else {
                source_ref_revision(self.payloads.as_ref(), &discovery_session).await?
            };
            let snapshot_id = RemoteSnapshotId::new(
                group.key.normalized_ref.clone(),
                resolved_ref(&group.key.normalized_ref),
                ref_revision,
            );
            if let Err(error) = self.evidence.record_acquisition(
                group.evidence_key.clone(),
                group.key.environment.clone(),
                snapshot_id.clone(),
                skill_revisions,
            ) {
                log::warn!("Could not persist acquired source evidence: {error}");
            }
            let redirected_download_hosts = retained.download_hosts();
            drop(retained);
            if provider == &SourceProvider::WellKnown {
                if let Err(error) = self
                    .payloads
                    .release_source_snapshot(&discovery_session)
                    .await
                {
                    log::warn!(
                        "Prepared content retained; source cleanup will be retried: {error}"
                    );
                }
            } else {
                self.snapshots.remember(
                    group.key.clone(),
                    snapshot_id.commit_revision,
                    discovery_session.clone(),
                );
            }
            Ok(AcquiredUpdateSource {
                discovery_session: discovery_session.clone(),
                redirected_download_hosts,
                _leases: leases,
                payloads: acquired_skills.into_iter().zip(handles).collect(),
                skill_errors,
            })
        }
        .await;
        if let Err(error) = self.payloads.make_source_optional(&discovery_session).await {
            log::warn!("Source retention cleanup will be retried: {error}");
        }
        result
    }

    async fn discover_group(
        &self,
        group: &UpdateAcquisitionGroup,
        cancellation: crate::core::mutation::CancellationSignal,
    ) -> Result<DiscoverySessionHandle, AppError> {
        let source = group.descriptor.source().to_string();
        let parsed = group
            .descriptor
            .parsed_source(group.evidence_key.remote.provider());
        if group.evidence_key.remote.provider() == &SourceProvider::WellKnown {
            let selected_names = group
                .skills
                .iter()
                .map(|skill| skill.name.clone())
                .collect::<Vec<_>>();
            return match &group.environment {
                EnvironmentRef::Native => {
                    let fetched = self
                        .wellknown
                        .fetch_selected(&source, &selected_names, &cancellation)
                        .await
                        .map_err(
                            crate::application::wellknown_access::WellKnownFetchError::into_error,
                        )?;
                    let root = fetched.repo_path.clone();
                    let owner = ManagedDownloadedDirectory::new(root.clone());
                    retain_discovered_source(
                        self.payloads.clone(),
                        group.environment.clone(),
                        parsed,
                        source,
                        DiscoverySourceLocation::Native {
                            root: root.clone(),
                            ref_revision: None,
                        },
                        root,
                        owner,
                        RetainedSourceOptions {
                            trust_metadata: Some(fetched.trust_metadata),
                            redirected_download_host: fetched.redirected_download_host,
                            redirected_download_hosts: fetched.redirected_download_hosts,
                            member_failures: fetched.member_failures,
                            full_depth: true,
                            internal_skill_visibility: InternalSkillVisibility::All,
                            ..Default::default()
                        },
                    )
                    .await
                    .map(|discovery| discovery.discovery_session)
                }
                EnvironmentRef::Wsl { distro_name } => self
                    .wsl_source
                    .discover(
                        distro_name,
                        parsed,
                        source,
                        SourceDiscoveryPolicy {
                            allow_empty_catalog: true,
                            selected_skill_names: Some(selected_names),
                            full_depth: true,
                            internal_skill_visibility: InternalSkillVisibility::All,
                        },
                        cancellation,
                    )
                    .await
                    .map(|discovery| discovery.discovery_session),
            };
        }
        GitSourceDiscovery::new(
            self.payloads.clone(),
            Arc::clone(&self.git_transport),
            Arc::clone(&self.wsl_source),
        )
        .discover(
            group.environment.clone(),
            parsed,
            source,
            SourceDiscoveryPolicy {
                allow_empty_catalog: true,
                selected_skill_names: None,
                full_depth: true,
                internal_skill_visibility: InternalSkillVisibility::All,
            },
            |_| {},
            cancellation,
        )
        .await
        .map(|discovery| discovery.discovery_session)
    }
}

fn snapshot_skill_key(provider: &SourceProvider, skill_name: &str, skill_path: &str) -> String {
    match provider {
        SourceProvider::WellKnown => skill_name.to_string(),
        _ => normalize_skill_folder_path(skill_path),
    }
}

fn retained_skill_matches(
    provider: &SourceProvider,
    locked_name: &str,
    locked_path: &str,
    available_name: &str,
    available_path: &str,
) -> bool {
    if available_name != locked_name {
        return false;
    }
    provider == &SourceProvider::WellKnown
        || normalize_skill_folder_path(available_path) == normalize_skill_folder_path(locked_path)
}

fn acquisition_skill_revision(
    provider: &SourceProvider,
    metadata: &PayloadPlanningMetadata,
) -> Result<SkillRevision, AppError> {
    match provider {
        SourceProvider::Github => metadata
            .upstream_revision
            .as_ref()
            .filter(|revision| !revision.is_empty())
            .cloned()
            .map(SkillRevision::GitTreeOid)
            .ok_or(AppError::StalePayload),
        SourceProvider::Gitlab | SourceProvider::Git => Ok(SkillRevision::CliContentHash(
            metadata.computed_hash.clone(),
        )),
        SourceProvider::WellKnown => metadata
            .well_known
            .as_ref()
            .map(|value| SkillRevision::WellKnownDigest(value.digest.clone()))
            .ok_or(AppError::StalePayload),
    }
}

impl SkillSourceModule for RuntimeSkillSourceModule {
    fn acquire_saved_groups<'a>(
        &'a self,
        groups: &'a [UpdateAcquisitionGroup],
        cancellation: crate::core::mutation::CancellationSignal,
    ) -> UpdateFuture<'a, Result<Vec<UpdateSourceAcquisition>, AppError>> {
        Box::pin(async move {
            use futures_util::StreamExt;
            let jobs = groups
                .iter()
                .map(|group| {
                    let cancellation = cancellation.clone();
                    let source = self.clone();
                    let owned_group = group.clone();
                    Box::pin(async move {
                        let result = tokio::spawn(async move {
                            source.acquire_group(&owned_group, cancellation).await
                        })
                        .await
                        .unwrap_or_else(|error| {
                            Err(AppError::ExecutionFailed {
                                message: error.to_string(),
                            })
                        });
                        UpdateSourceAcquisition {
                            source_result_id: group.source_result_id.clone(),
                            source: group.source.clone(),
                            skill_names: group
                                .skills
                                .iter()
                                .map(|skill| skill.name.clone())
                                .collect(),
                            result,
                        }
                    }) as UpdateFuture<'_, UpdateSourceAcquisition>
                })
                .collect::<Vec<_>>();
            let acquisitions = futures_util::stream::iter(jobs).buffered(4).collect().await;
            Ok(acquisitions)
        })
    }
}

async fn source_ref_revision(
    payloads: &PayloadSessionManager,
    discovery: &DiscoverySessionHandle,
) -> Result<String, AppError> {
    let retained = payloads.source_snapshot(discovery)?;
    match retained.location() {
        DiscoverySourceLocation::Native { root, ref_revision } => ref_revision
            .clone()
            .or_else(|| compute_local_ref_revision(root))
            .ok_or_else(|| AppError::GitCloneFailed {
                message: "acquired source has no resolvable HEAD revision".to_string(),
            }),
        DiscoverySourceLocation::WslNative { ref_revision, .. } => {
            ref_revision
                .clone()
                .ok_or_else(|| AppError::GitCloneFailed {
                    message: "acquired WSL source has no captured HEAD revision".to_string(),
                })
        }
    }
}

fn resolved_ref(normalized_ref: &NormalizedRef) -> String {
    match normalized_ref {
        NormalizedRef::Default => "HEAD".to_string(),
        NormalizedRef::Named(value) => value.clone(),
    }
}

pub type RuntimeUpdateService = UpdateService<
    ConcreteUpdatePlanner<RuntimePlanningFactSource, RuntimeTargetFactResolver>,
    RuntimeSkillSourceModule,
    RuntimePlanExecutor,
>;

pub type RuntimeUpdateCheckService = UpdateCheckService<
    InstalledUpdateRecordProvider<RuntimePlanningFactSource, RuntimeTargetFactResolver>,
>;

pub type RuntimeLibraryUpdateCheckService =
    crate::application::update_check::LibraryUpdateCheckService<LibraryUpdateRecordProvider>;

pub type RuntimeLibraryUpdateService = crate::application::library_update::LibraryUpdateService<
    LibraryUpdateSubjectProvider<RuntimeTargetFactResolver>,
    RuntimeSkillSourceModule,
    RuntimeTargetFactResolver,
>;

pub fn build_runtime_source_evidence_coordinator(
    payloads: Arc<PayloadSessionManager>,
    snapshots: Arc<SourceSnapshotReuseIndex>,
    github: Arc<dyn GithubTreeAccess>,
    git_transport: Arc<dyn GitSourceTransport>,
    wsl_source: Arc<dyn WslSourceAccess>,
    wellknown: Arc<dyn crate::application::wellknown_access::WellKnownAccess>,
) -> Result<SourceEvidenceCoordinator, AppError> {
    let detector = Arc::new(RuntimeSourceEvidenceDetector::new(
        payloads,
        snapshots.clone(),
        github,
        git_transport,
        wsl_source,
        wellknown,
    ));
    let home = dirs::home_dir().ok_or_else(|| AppError::Path {
        message: "无法确定用户主目录，不能初始化更新检查状态".to_string(),
    })?;
    SourceEvidenceCoordinator::with_state_path(
        detector,
        home.join(".skill-deck/state/update-check.json"),
    )
}

pub fn build_runtime_update_check_service(
    environments: Arc<WslRuntime>,
    registry: Arc<dyn AgentRegistrySnapshotSource>,
    evidence: SourceEvidenceCoordinator,
    libraries: Arc<dyn SkillLibraryRepository>,
) -> RuntimeUpdateCheckService {
    let facts = RuntimePlanningFactSource::for_current_user(registry, environments.clone());
    UpdateCheckService::new(
        InstalledUpdateRecordProvider::new(
            facts,
            RuntimeTargetFactResolver::new(environments),
            libraries,
        ),
        evidence,
    )
}

pub fn build_runtime_library_update_check_service(
    repository: Arc<dyn SkillLibraryRepository>,
    evidence: SourceEvidenceCoordinator,
) -> RuntimeLibraryUpdateCheckService {
    crate::application::update_check::LibraryUpdateCheckService::new(
        LibraryUpdateRecordProvider::new(repository),
        evidence,
    )
}

pub fn build_runtime_library_update_service(
    payloads: Arc<PayloadSessionManager>,
    repository: Arc<dyn SkillLibraryRepository>,
    targets: RuntimeTargetFactResolver,
    skill_source: RuntimeSkillSourceModule,
    libraries: Arc<crate::application::skill_libraries::SkillLibraryModule>,
) -> RuntimeLibraryUpdateService {
    crate::application::library_update::LibraryUpdateService::new(
        payloads,
        LibraryUpdateSubjectProvider::new(repository, targets.clone()),
        skill_source,
        targets.clone(),
        libraries,
    )
}

pub fn build_runtime_update_service(
    payloads: Arc<PayloadSessionManager>,
    environments: Arc<WslRuntime>,
    registry: Arc<dyn AgentRegistrySnapshotSource>,
    execution: RuntimeExecutionDependencies,
    skill_source: RuntimeSkillSourceModule,
    libraries: Arc<dyn SkillLibraryRepository>,
) -> RuntimeUpdateService {
    let facts = RuntimePlanningFactSource::for_current_user(registry, environments.clone());
    let planner = ConcreteUpdatePlanner::new(
        facts.clone(),
        RuntimeTargetFactResolver::new(environments.clone()),
        payloads.clone(),
        libraries,
        || {
            chrono::Utc::now()
                .format("%Y-%m-%dT%H:%M:%S%.3fZ")
                .to_string()
        },
    );
    let revisions: Arc<dyn RuntimeRevisionSource> = Arc::new(facts);
    let executor = execution.executor(environments, revisions);
    UpdateService::new(payloads, planner, skill_source, executor)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::payload_session::PayloadPlanningMetadata;
    use crate::core::source_identity::SourceProvider;

    fn planning_metadata(upstream_revision: Option<&str>) -> PayloadPlanningMetadata {
        PayloadPlanningMetadata {
            skill_name: "demo".to_string(),
            install_dir_name: "demo".to_string(),
            source: "acme/tools".to_string(),
            source_type: "github".to_string(),
            source_url: Some("https://github.com/acme/tools.git".to_string()),
            ref_name: Some("main".to_string()),
            skill_path: "skills/demo".to_string(),
            plugin_name: None,
            computed_hash: "cli-hash".to_string(),
            upstream_revision: upstream_revision.map(str::to_string),
            well_known: None,
        }
    }

    #[test]
    fn github_runtime_acquisition_uses_upstream_tree_revision() {
        let revision = acquisition_skill_revision(
            &SourceProvider::Github,
            &planning_metadata(Some("tree-oid")),
        )
        .unwrap();

        assert_eq!(revision, SkillRevision::GitTreeOid("tree-oid".to_string()));
        assert!(
            acquisition_skill_revision(&SourceProvider::Github, &planning_metadata(None)).is_err()
        );
    }

    #[test]
    fn clone_runtime_acquisition_uses_cli_content_hash() {
        for provider in [SourceProvider::Gitlab, SourceProvider::Git] {
            assert_eq!(
                acquisition_skill_revision(&provider, &planning_metadata(Some("tree-oid")))
                    .unwrap(),
                SkillRevision::CliContentHash("cli-hash".to_string())
            );
        }
    }

    #[test]
    fn retained_native_snapshot_reuse_requires_an_unchanged_probe() {
        assert_eq!(
            retained_snapshot_action(
                &EnvironmentRef::Native,
                Some("revision-1"),
                Ok("revision-1")
            ),
            RetainedSnapshotAction::Reuse
        );
        assert_eq!(
            retained_snapshot_action(
                &EnvironmentRef::Native,
                Some("revision-1"),
                Ok("revision-2")
            ),
            RetainedSnapshotAction::Reacquire
        );
    }

    #[test]
    fn failed_probe_reacquires_the_source() {
        let probe_error = AppError::GitCloneFailed {
            message: "probe unavailable".to_string(),
        };
        assert_eq!(
            retained_snapshot_action(
                &EnvironmentRef::Native,
                Some("revision-1"),
                Err(&probe_error),
            ),
            RetainedSnapshotAction::Reacquire
        );
    }

    #[test]
    fn cancelled_probe_preserves_cancellation_instead_of_reacquiring() {
        assert_eq!(
            retained_snapshot_action(
                &EnvironmentRef::Native,
                Some("revision-1"),
                Err(&AppError::MutationCancelled),
            ),
            RetainedSnapshotAction::Cancelled
        );
    }

    #[test]
    fn well_known_snapshot_uses_skill_identity_instead_of_git_path() {
        assert_eq!(
            snapshot_skill_key(&SourceProvider::WellKnown, "ce:review", "ce-review"),
            "ce:review"
        );
        assert!(retained_skill_matches(
            &SourceProvider::WellKnown,
            "ce:review",
            "",
            "ce:review",
            "ce-review",
        ));
        assert!(!retained_skill_matches(
            &SourceProvider::WellKnown,
            "ce:review",
            "",
            "ce-review",
            "ce-review",
        ));
    }
}

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use crate::application::git_transport::{GitSourceTransport, ProcessGitTransport};
use crate::application::mutation::coordinator::RuntimeRevisionSource;
use crate::application::payload_session::{
    DiscoverySessionHandle, DiscoverySourceLocation, PayloadPlanningMetadata, PayloadSessionManager,
};
use crate::application::plan_runner::{RuntimeExecutionDependencies, RuntimePlanExecutor};
use crate::application::runtime_facts::{AgentRegistrySnapshotSource, RuntimePlanningFactSource};
use crate::application::source_acquisition::{
    AcquireSelectedPayloadsRequest, SelectedPayloadAcquisitionService, SourceDiscoveryService,
};
use crate::application::source_evidence::{
    RemoteSnapshotId, SkillRevision, SourceEvidenceCoordinator, SourceSnapshotFacts,
};
use crate::application::source_evidence_provider::RuntimeSourceEvidenceDetector;
use crate::application::source_snapshot_reuse::SourceSnapshotReuseIndex;
use crate::application::update::{
    AcquiredUpdateSource, UpdateAcquisitionGroup, UpdateFuture, UpdatePayloadAcquirer,
    UpdateService, UpdateSourceAcquisition,
};
use crate::application::update_check::UpdateCheckService;
use crate::application::update_planner::ConcreteUpdatePlanner;
use crate::core::compute_local_ref_revision;
use crate::core::skill_paths::normalize_skill_folder_path;
use crate::core::source_identity::{NormalizedRef, SourceProvider};
use crate::environment::planning::RuntimeTargetFactResolver;
use crate::environment::types::EnvironmentRef;
use crate::environment::wsl::EnvironmentRegistry;
use crate::error::AppError;

pub struct RuntimeUpdatePayloadAcquirer {
    payloads: Arc<PayloadSessionManager>,
    environments: Arc<EnvironmentRegistry>,
    snapshots: Arc<SourceSnapshotReuseIndex>,
    evidence: SourceEvidenceCoordinator,
    git_transport: Arc<dyn GitSourceTransport>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RetainedSnapshotAction {
    Reuse,
    Reacquire,
    Cancelled,
}

fn retained_snapshot_action(
    environment: &EnvironmentRef,
    retained_revision: Option<&str>,
    probe: Result<&str, &AppError>,
) -> RetainedSnapshotAction {
    if !matches!(environment, EnvironmentRef::Host) || retained_revision.is_none() {
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

fn snapshot_reuse_eligible(environment: &EnvironmentRef) -> bool {
    matches!(environment, EnvironmentRef::Host)
}

impl RuntimeUpdatePayloadAcquirer {
    pub fn new(
        payloads: Arc<PayloadSessionManager>,
        environments: Arc<EnvironmentRegistry>,
        snapshots: Arc<SourceSnapshotReuseIndex>,
        evidence: SourceEvidenceCoordinator,
    ) -> Self {
        Self {
            payloads,
            environments,
            snapshots,
            evidence,
            git_transport: Arc::new(ProcessGitTransport),
        }
    }

    #[cfg(any(test, all(target_os = "windows", feature = "wsl-integration-tests")))]
    pub(crate) fn with_git_transport(
        payloads: Arc<PayloadSessionManager>,
        environments: Arc<EnvironmentRegistry>,
        snapshots: Arc<SourceSnapshotReuseIndex>,
        evidence: SourceEvidenceCoordinator,
        git_transport: Arc<dyn GitSourceTransport>,
    ) -> Self {
        Self {
            payloads,
            environments,
            snapshots,
            evidence,
            git_transport,
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
        let reusable = if snapshot_reuse_eligible(&group.context.environment) {
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
                let probed = tokio::task::spawn_blocking(move || {
                    git_transport.probe_ref_revision(
                        &probe_source,
                        probe_ref.as_deref(),
                        probe_cancellation,
                    )
                })
                .await;
                let action = match probed {
                    Ok(result) => retained_snapshot_action(
                        &group.context.environment,
                        Some(&retained_revision),
                        result.as_ref().map(String::as_str),
                    ),
                    Err(_) => RetainedSnapshotAction::Reacquire,
                };
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
        if cancellation.is_cancelled() {
            return Err(AppError::MutationCancelled);
        }
        let retained = self.payloads.source_snapshot(&discovery_session)?;
        let catalog = retained
            .skills()
            .map(|skill| normalize_skill_folder_path(&skill.relative_path))
            .collect::<BTreeSet<_>>();
        let mut selected_paths = Vec::with_capacity(group.skills.len());
        for locked in &group.skills {
            let expected_path = normalize_skill_folder_path(&locked.skill_path);
            let available = retained
                .skills()
                .find(|available| {
                    available.skill_name == locked.name
                        && normalize_skill_folder_path(&available.relative_path) == expected_path
                })
                .ok_or_else(|| AppError::InvalidSource {
                    value: format!(
                        "Skill '{}' was not found at locked path '{}'",
                        locked.name, locked.skill_path
                    ),
                })?;
            selected_paths.push(available.relative_path.clone());
        }
        let handles = SelectedPayloadAcquisitionService::new(self.payloads.clone())
            .acquire(AcquireSelectedPayloadsRequest {
                discovery_session: discovery_session.clone(),
                skill_paths: selected_paths,
            })
            .await?;
        if cancellation.is_cancelled() {
            return Err(AppError::MutationCancelled);
        }
        if handles.len() != group.skills.len() {
            return Err(AppError::StalePayload);
        }
        let ref_revision = source_ref_revision(self.payloads.as_ref(), &discovery_session).await?;
        let facts = SourceSnapshotFacts {
            discovery_session,
            snapshot_id: RemoteSnapshotId::new(
                group.key.normalized_ref.clone(),
                resolved_ref(&group.key.normalized_ref),
                ref_revision,
            ),
            complete_skill_path_catalog: catalog,
        };
        let mut skill_revisions = BTreeMap::new();
        for (locked, handle) in group.skills.iter().zip(&handles) {
            let lease = self.payloads.pin_verified(handle).await?;
            skill_revisions.insert(
                normalize_skill_folder_path(&locked.skill_path),
                acquisition_skill_revision(
                    group.evidence_key.remote.provider(),
                    lease.planning_metadata(),
                )?,
            );
        }
        self.evidence.record_acquisition(
            group.evidence_key.clone(),
            group.key.clone(),
            facts.clone(),
            skill_revisions,
        )?;
        Ok(AcquiredUpdateSource {
            facts,
            payloads: group
                .skills
                .iter()
                .map(|skill| skill.name.clone())
                .zip(handles)
                .collect(),
        })
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
        SourceDiscoveryService::with_git_transport(
            self.payloads.clone(),
            self.environments.as_ref(),
            Arc::clone(&self.git_transport),
        )
        .discover_parsed_with_cancellation(
            group.context.clone(),
            parsed,
            source,
            |_| {},
            cancellation,
        )
        .await
        .map(|discovery| discovery.discovery_session)
    }
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
    }
}

impl UpdatePayloadAcquirer for RuntimeUpdatePayloadAcquirer {
    fn acquire<'a>(
        &'a self,
        groups: &'a [UpdateAcquisitionGroup],
        cancellation: crate::core::mutation::CancellationSignal,
    ) -> UpdateFuture<'a, Result<Vec<UpdateSourceAcquisition>, AppError>> {
        Box::pin(async move {
            let mut acquisitions = Vec::with_capacity(groups.len());
            for group in groups {
                let result = self.acquire_group(group, cancellation.clone()).await;
                acquisitions.push(UpdateSourceAcquisition {
                    source_result_id: group.source_result_id.clone(),
                    source: group.source.clone(),
                    skill_names: group
                        .skills
                        .iter()
                        .map(|skill| skill.name.clone())
                        .collect(),
                    result,
                });
                if cancellation.is_cancelled() {
                    for pending in &groups[acquisitions.len()..] {
                        acquisitions.push(UpdateSourceAcquisition {
                            source_result_id: pending.source_result_id.clone(),
                            source: pending.source.clone(),
                            skill_names: pending
                                .skills
                                .iter()
                                .map(|skill| skill.name.clone())
                                .collect(),
                            result: Err(AppError::MutationCancelled),
                        });
                    }
                    break;
                }
            }
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
        DiscoverySourceLocation::Native { root } => {
            compute_local_ref_revision(root).ok_or_else(|| AppError::GitCloneFailed {
                message: "acquired source has no resolvable HEAD revision".to_string(),
            })
        }
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
    RuntimeUpdatePayloadAcquirer,
    RuntimePlanExecutor,
>;

pub type RuntimeUpdateCheckService = UpdateCheckService<RuntimePlanningFactSource>;

pub fn build_runtime_source_evidence_coordinator(
    payloads: Arc<PayloadSessionManager>,
    environments: Arc<EnvironmentRegistry>,
    snapshots: Arc<SourceSnapshotReuseIndex>,
) -> SourceEvidenceCoordinator {
    let detector = Arc::new(RuntimeSourceEvidenceDetector::new(
        payloads,
        environments,
        snapshots.clone(),
    ));
    SourceEvidenceCoordinator::with_snapshot_reuse(detector, snapshots)
}

pub fn build_runtime_update_check_service(
    environments: Arc<EnvironmentRegistry>,
    registry: Arc<dyn AgentRegistrySnapshotSource>,
    evidence: SourceEvidenceCoordinator,
) -> RuntimeUpdateCheckService {
    UpdateCheckService::new(
        RuntimePlanningFactSource::for_current_user(registry, environments),
        evidence,
    )
}

pub fn build_runtime_update_service(
    payloads: Arc<PayloadSessionManager>,
    environments: Arc<EnvironmentRegistry>,
    registry: Arc<dyn AgentRegistrySnapshotSource>,
    execution: RuntimeExecutionDependencies,
    snapshots: Arc<SourceSnapshotReuseIndex>,
    evidence: SourceEvidenceCoordinator,
) -> RuntimeUpdateService {
    let facts = RuntimePlanningFactSource::for_current_user(registry, environments.clone());
    let planner = ConcreteUpdatePlanner::new(
        facts.clone(),
        RuntimeTargetFactResolver::new(environments.clone()),
        payloads.clone(),
        || {
            chrono::Utc::now()
                .format("%Y-%m-%dT%H:%M:%S%.3fZ")
                .to_string()
        },
    );
    let acquirer = RuntimeUpdatePayloadAcquirer::new(
        payloads.clone(),
        environments.clone(),
        snapshots,
        evidence,
    );
    let revisions: Arc<dyn RuntimeRevisionSource> = Arc::new(facts);
    let executor = execution.executor(environments, revisions);
    UpdateService::new(payloads, planner, acquirer, executor)
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
    fn retained_host_snapshot_reuse_requires_an_unchanged_probe() {
        assert_eq!(
            retained_snapshot_action(&EnvironmentRef::Host, Some("revision-1"), Ok("revision-1")),
            RetainedSnapshotAction::Reuse
        );
        assert_eq!(
            retained_snapshot_action(&EnvironmentRef::Host, Some("revision-1"), Ok("revision-2")),
            RetainedSnapshotAction::Reacquire
        );
    }

    #[test]
    fn failed_probe_or_wsl_snapshot_forces_environment_local_reacquisition() {
        let probe_error = AppError::GitCloneFailed {
            message: "probe unavailable".to_string(),
        };
        assert_eq!(
            retained_snapshot_action(&EnvironmentRef::Host, Some("revision-1"), Err(&probe_error),),
            RetainedSnapshotAction::Reacquire
        );
        assert!(snapshot_reuse_eligible(&EnvironmentRef::Host));
        assert!(!snapshot_reuse_eligible(&EnvironmentRef::Wsl {
            distro_name: "Ubuntu".to_string(),
        }));
        assert_eq!(
            retained_snapshot_action(
                &EnvironmentRef::Wsl {
                    distro_name: "Ubuntu".to_string(),
                },
                Some("revision-1"),
                Ok("revision-1"),
            ),
            RetainedSnapshotAction::Reacquire
        );
    }

    #[test]
    fn cancelled_probe_preserves_cancellation_instead_of_reacquiring() {
        assert_eq!(
            retained_snapshot_action(
                &EnvironmentRef::Host,
                Some("revision-1"),
                Err(&AppError::MutationCancelled),
            ),
            RetainedSnapshotAction::Cancelled
        );
    }
}

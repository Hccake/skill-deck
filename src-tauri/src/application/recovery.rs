use std::sync::Arc;

use serde::Serialize;
use specta::Type;

use crate::application::mutation::result::ErrorReport;
use crate::environment::recovery::{
    RecoveryResourcePath, RecoveryResourcePathKind, RecoverySubject,
};
use crate::environment::types::{display_locator, parent_locator, EnvironmentRef, ResourceLocator};
use crate::error::{AppError, RecoveryResourceId};
use crate::storage::recovery_repository::{
    RecoveryAssessmentState, RecoveryConsistencyChecker, RecoveryRepository,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub enum RecoveryResourceState {
    NeedsAttention,
    ConsistentCanCleanup,
    EnvironmentUnavailable,
    Invalid,
    Missing,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct RecoveryResourceStatus {
    pub resource_id: RecoveryResourceId,
    pub state: RecoveryResourceState,
    pub revision: String,
    pub environment: Option<EnvironmentRef>,
    pub created_at_epoch_ms: u64,
    pub subject: Option<RecoverySubject>,
    pub paths: Vec<RecoveryResourcePath>,
    pub diagnostic: Option<ErrorReport>,
}

pub struct RecoveryService<C> {
    repository: Arc<RecoveryRepository<C>>,
}

impl<C> RecoveryService<C>
where
    C: RecoveryConsistencyChecker,
{
    pub fn new(repository: Arc<RecoveryRepository<C>>) -> Self {
        Self { repository }
    }

    pub async fn status(
        &self,
        id: &RecoveryResourceId,
    ) -> Result<RecoveryResourceStatus, AppError> {
        if let Some(invalid) = self.repository.resolve_invalid(id) {
            return Ok(status_from_invalid(invalid));
        }
        let assessment = match self.repository.assess(id).await {
            Ok(assessment) => assessment,
            Err(AppError::PathNotFound { .. }) => {
                return Ok(RecoveryResourceStatus {
                    resource_id: id.clone(),
                    state: RecoveryResourceState::Missing,
                    revision: String::new(),
                    environment: None,
                    created_at_epoch_ms: 0,
                    subject: None,
                    paths: Vec::new(),
                    diagnostic: None,
                })
            }
            Err(error) => return Err(error),
        };
        Ok(status_from_assessment(assessment))
    }

    pub async fn list(&self) -> Result<Vec<RecoveryResourceStatus>, AppError> {
        let mut resources = self
            .repository
            .assess_all()
            .await?
            .into_iter()
            .map(status_from_assessment)
            .collect::<Vec<_>>();
        resources.extend(
            self.repository
                .invalid_records()?
                .into_iter()
                .map(status_from_invalid),
        );
        resources.sort_by(|left, right| {
            left.created_at_epoch_ms
                .cmp(&right.created_at_epoch_ms)
                .then_with(|| left.resource_id.as_str().cmp(right.resource_id.as_str()))
        });
        Ok(resources)
    }

    pub async fn confirm_resolved(
        &self,
        id: &RecoveryResourceId,
        expected_revision: &str,
    ) -> Result<(), AppError> {
        if self.repository.resolve_invalid(id).is_some() {
            return Err(AppError::StaleTarget);
        }
        self.repository
            .confirm_consistent(id, expected_revision)
            .await
    }

    pub(crate) fn open_target(&self, id: &RecoveryResourceId) -> Result<ResourceLocator, AppError> {
        if let Some(recovery) = self.repository.resolve(id) {
            self.repository
                .validate_managed_root(&recovery.marker_ref.managed_root)?;
            let destination = &recovery
                .marker
                .entries
                .first()
                .ok_or_else(|| AppError::ConfigurationCorrupted {
                    message: "recovery marker has no target entry".to_string(),
                })?
                .destination;
            return parent_locator(destination).ok_or_else(|| AppError::UnsafePath {
                path: destination.native_path.clone(),
                reason: "recovery target has no parent directory".to_string(),
            });
        }
        let target = self
            .repository
            .resolve_invalid(id)
            .map(|recovery| recovery.managed_root)
            .ok_or_else(|| AppError::PathNotFound {
                path: id.as_str().to_string(),
            })?;
        self.repository.validate_managed_root(&target)?;
        Ok(target)
    }

    #[cfg(test)]
    fn repository(&self) -> &RecoveryRepository<C> {
        self.repository.as_ref()
    }
}

fn status_from_assessment(
    assessment: crate::storage::recovery_repository::RecoveryAssessment,
) -> RecoveryResourceStatus {
    let state = match assessment.state {
        RecoveryAssessmentState::NeedsAttention => RecoveryResourceState::NeedsAttention,
        RecoveryAssessmentState::ConsistentCanCleanup => {
            RecoveryResourceState::ConsistentCanCleanup
        }
        RecoveryAssessmentState::EnvironmentUnavailable => {
            RecoveryResourceState::EnvironmentUnavailable
        }
    };
    let mut paths = Vec::new();
    for entry in &assessment.recovery.marker.entries {
        paths.push(RecoveryResourcePath {
            kind: RecoveryResourcePathKind::Current,
            location: display_locator(&entry.destination),
        });
        if let Some(backup) = &entry.backup {
            paths.push(RecoveryResourcePath {
                kind: RecoveryResourcePathKind::Backup,
                location: display_locator(backup),
            });
        }
    }
    let mut unique_paths = Vec::with_capacity(paths.len());
    for path in paths {
        if !unique_paths.contains(&path) {
            unique_paths.push(path);
        }
    }
    RecoveryResourceStatus {
        resource_id: assessment.recovery.marker.resource_id.clone(),
        state,
        revision: assessment.revision.as_str().to_string(),
        environment: Some(assessment.recovery.marker.environment.clone()),
        created_at_epoch_ms: assessment.recovery.marker.created_at_epoch_ms,
        subject: assessment.recovery.marker.subject.clone(),
        paths: unique_paths,
        diagnostic: None,
    }
}

fn status_from_invalid(
    recovery: crate::storage::recovery_repository::InvalidRecoveryRecord,
) -> RecoveryResourceStatus {
    let mut diagnostic = ErrorReport::from_app_error(recovery.diagnostic, None);
    diagnostic.environment = Some(recovery.environment.clone());
    diagnostic.display_paths = vec![recovery.managed_root.clone()];
    RecoveryResourceStatus {
        resource_id: recovery.resource_id,
        state: RecoveryResourceState::Invalid,
        revision: String::new(),
        environment: Some(recovery.environment),
        created_at_epoch_ms: 0,
        subject: None,
        paths: vec![RecoveryResourcePath {
            kind: RecoveryResourcePathKind::Record,
            location: display_locator(&recovery.managed_root),
        }],
        diagnostic: Some(diagnostic),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use std::fs;
    use std::sync::{Arc, Mutex};

    use tempfile::tempdir;

    use super::*;
    use crate::core::mutation::MutationKind;
    use crate::environment::native::recovery::NativeRecoveryMarkerStore;
    use crate::environment::recovery::{
        RecoveryEntryPhase, RecoveryFuture, RecoveryMarker, RecoveryMarkerEntry,
        RecoveryMarkerKind, RecoveryMarkerStore, RecoveryResourcePathKind, RecoverySubject,
        RECOVERY_MARKER_SCHEMA_VERSION,
    };
    use crate::environment::types::{
        EnvironmentRef, ResourceLocator, SkillLocation, SkillLocationRef,
    };
    use crate::error::{AppError, RecoveryResourceId};
    use crate::storage::recovery_repository::{
        RecoveryConsistency, RecoveryConsistencyChecker, RecoveryRepository,
    };

    struct Checker(Mutex<RecoveryConsistency>);

    impl RecoveryConsistencyChecker for Checker {
        fn check<'a>(
            &'a self,
            _marker: &'a RecoveryMarker,
        ) -> RecoveryFuture<'a, Result<RecoveryConsistency, AppError>> {
            Box::pin(async move { Ok(*self.0.lock().unwrap()) })
        }
    }

    #[tokio::test]
    async fn status_and_confirm_use_opaque_repository_identity() {
        let temp = tempdir().unwrap();
        let store: Arc<dyn RecoveryMarkerStore> =
            Arc::new(NativeRecoveryMarkerStore::new(temp.path()).expect("store"));
        let checker = Arc::new(Checker(Mutex::new(RecoveryConsistency::Inconsistent)));
        let repository = Arc::new(RecoveryRepository::new(vec![store], Arc::clone(&checker)));
        let marker = marker("recovery-1");
        repository
            .record_in_progress(marker.clone())
            .await
            .expect("record");
        let service = RecoveryService::new(repository);

        let attention = service.status(&marker.resource_id).await.expect("status");
        assert_eq!(attention.state, RecoveryResourceState::NeedsAttention);
        assert!(!attention.revision.is_empty());

        service
            .repository()
            .record_required(&marker.resource_id)
            .await
            .expect("required");
        *checker.0.lock().unwrap() = RecoveryConsistency::Consistent;
        let consistent = service.status(&marker.resource_id).await.unwrap();
        assert_eq!(
            consistent.state,
            RecoveryResourceState::ConsistentCanCleanup
        );
        service
            .confirm_resolved(&marker.resource_id, &consistent.revision)
            .await
            .expect("confirm");
        assert!(service.repository().resolve(&marker.resource_id).is_none());
        assert_eq!(
            service.status(&marker.resource_id).await.unwrap().state,
            RecoveryResourceState::Missing
        );

        let missing = RecoveryResourceId::parse("missing").unwrap();
        assert_eq!(
            service.status(&missing).await.unwrap().state,
            RecoveryResourceState::Missing
        );
        assert!(service.open_target(&missing).is_err());
        service
            .repository()
            .reindex_environment(&EnvironmentRef::Native, &HashSet::new())
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn restart_lists_reindexed_recovery_without_prior_frontend_id() {
        let temp = tempdir().unwrap();
        let store: Arc<dyn RecoveryMarkerStore> =
            Arc::new(NativeRecoveryMarkerStore::new(temp.path()).expect("store"));
        let checker = Arc::new(Checker(Mutex::new(RecoveryConsistency::Inconsistent)));
        let repository = Arc::new(RecoveryRepository::new(vec![store], Arc::clone(&checker)));
        let value = marker("recovery-list");
        repository
            .record_in_progress(value.clone())
            .await
            .expect("record");
        drop(repository);

        let reopened_store: Arc<dyn RecoveryMarkerStore> =
            Arc::new(NativeRecoveryMarkerStore::new(temp.path()).expect("reopen store"));
        let reopened = Arc::new(RecoveryRepository::new(
            vec![reopened_store],
            Arc::clone(&checker),
        ));
        reopened
            .reindex_environment(&EnvironmentRef::Native, &HashSet::new())
            .await
            .expect("reindex");

        let statuses = RecoveryService::new(reopened).list().await.expect("list");

        assert_eq!(statuses.len(), 1);
        assert_eq!(statuses[0].resource_id, value.resource_id);
        assert_eq!(statuses[0].created_at_epoch_ms, value.created_at_epoch_ms);
        assert_eq!(statuses[0].state, RecoveryResourceState::NeedsAttention);
    }

    #[tokio::test]
    async fn status_identifies_the_operation_and_labels_safe_display_paths() {
        let temp = tempdir().unwrap();
        let store: Arc<dyn RecoveryMarkerStore> =
            Arc::new(NativeRecoveryMarkerStore::new(temp.path()).expect("store"));
        let checker = Arc::new(Checker(Mutex::new(RecoveryConsistency::Inconsistent)));
        let repository = Arc::new(RecoveryRepository::new(vec![store], checker));
        let mut value = marker("recovery-update");
        value.subject = Some(RecoverySubject {
            operation_kind: MutationKind::Update,
            skill_name: "skill-deck".to_string(),
            context: SkillLocationRef {
                environment: EnvironmentRef::Native,
                scope: SkillLocation::Global,
            },
        });
        value.entries[0].destination = locator(r"\\?\C:\Users\cheng\.agents\skills\skill-deck");
        value.entries[0].backup = Some(locator(
            r"\\?\C:\Users\cheng\.agents\skills\.skill-deck-backup-update",
        ));
        repository
            .record_in_progress(value.clone())
            .await
            .expect("record");

        let service = RecoveryService::new(repository);
        let status = service.status(&value.resource_id).await.expect("status");

        assert_eq!(status.subject, value.subject);
        assert_eq!(status.paths.len(), 2);
        assert_eq!(status.paths[0].kind, RecoveryResourcePathKind::Current);
        assert_eq!(
            status.paths[0].location.native_path,
            r"C:\Users\cheng\.agents\skills\skill-deck"
        );
        assert_eq!(status.paths[1].kind, RecoveryResourcePathKind::Backup);
        assert_eq!(
            status.paths[1].location.native_path,
            r"C:\Users\cheng\.agents\skills\.skill-deck-backup-update"
        );
        assert_eq!(
            service
                .open_target(&value.resource_id)
                .expect("open processing directory")
                .native_path,
            r"\\?\C:\Users\cheng\.agents\skills"
        );
    }

    #[tokio::test]
    async fn invalid_marker_is_visible_openable_stable_and_never_confirmable() {
        let temp = tempdir().unwrap();
        let physical_root = fs::canonicalize(temp.path()).unwrap();
        let invalid_root = physical_root.join("operation-corrupt");
        fs::create_dir(&invalid_root).unwrap();
        fs::write(invalid_root.join("recovery.json"), b"not-json").unwrap();
        let store: Arc<dyn RecoveryMarkerStore> =
            Arc::new(NativeRecoveryMarkerStore::new(&physical_root).expect("store"));
        let checker = Arc::new(Checker(Mutex::new(RecoveryConsistency::Inconsistent)));
        let repository = Arc::new(RecoveryRepository::new(vec![store], checker));
        repository
            .reindex_environment(&EnvironmentRef::Native, &HashSet::new())
            .await
            .expect("reindex invalid marker");
        let service = RecoveryService::new(repository);

        let first = service.list().await.expect("list invalid marker");
        assert_eq!(first.len(), 1);
        assert_eq!(first[0].state, RecoveryResourceState::Invalid);
        assert!(first[0].resource_id.as_str().starts_with("invalid-"));
        assert!(first[0].diagnostic.is_some());
        assert_eq!(
            service
                .open_target(&first[0].resource_id)
                .expect("open managed root")
                .native_path,
            invalid_root.to_string_lossy()
        );
        assert!(matches!(
            service
                .confirm_resolved(&first[0].resource_id, "ignored")
                .await,
            Err(AppError::StaleTarget)
        ));

        #[cfg(unix)]
        {
            let redirected = physical_root.join("outside");
            fs::create_dir(&redirected).unwrap();
            fs::remove_dir_all(&invalid_root).unwrap();
            std::os::unix::fs::symlink(&redirected, &invalid_root).unwrap();
            assert!(matches!(
                service.open_target(&first[0].resource_id),
                Err(AppError::UnsafePath { .. })
            ));
            fs::remove_file(&invalid_root).unwrap();
            fs::create_dir(&invalid_root).unwrap();
            fs::write(invalid_root.join("recovery.json"), b"not-json").unwrap();
        }

        service
            .repository()
            .reindex_environment(&EnvironmentRef::Native, &HashSet::new())
            .await
            .expect("stable reindex");
        assert_eq!(
            service.list().await.unwrap()[0].resource_id,
            first[0].resource_id
        );

        fs::remove_dir_all(&invalid_root).unwrap();
        service
            .repository()
            .reindex_environment(&EnvironmentRef::Native, &HashSet::new())
            .await
            .expect("remove stale invalid index");
        assert!(service.list().await.unwrap().is_empty());
    }

    fn marker(id: &str) -> RecoveryMarker {
        RecoveryMarker {
            schema_version: RECOVERY_MARKER_SCHEMA_VERSION,
            resource_id: RecoveryResourceId::parse(id).unwrap(),
            kind: RecoveryMarkerKind::InProgress,
            environment: EnvironmentRef::Native,
            operation_id: "operation-1".to_string(),
            unit_id: "unit-1".to_string(),
            subject: Some(RecoverySubject {
                operation_kind: MutationKind::Update,
                skill_name: "demo".to_string(),
                context: SkillLocationRef {
                    environment: EnvironmentRef::Native,
                    scope: SkillLocation::Global,
                },
            }),
            created_at_epoch_ms: 1_000,
            entries: vec![RecoveryMarkerEntry {
                physical_target_digest: "target-1".to_string(),
                destination: locator("/work/.agents/skills/demo"),
                backup: Some(locator("/work/.agents/skills/.skill-deck-backup-demo")),
                expected_state: crate::environment::recovery::RecoveryExpectedEntryState::Present,
                original_fingerprint: "entry-v1-original".to_string(),
                phase: RecoveryEntryPhase::RestoreFailed,
            }],
        }
    }

    fn locator(path: &str) -> ResourceLocator {
        ResourceLocator {
            environment: EnvironmentRef::Native,
            native_path: path.to_string(),
        }
    }
}

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, MutexGuard};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::environment::recovery::{
    RecoveryFuture, RecoveryMarker, RecoveryMarkerKind, RecoveryMarkerLoad, RecoveryMarkerRef,
    RecoveryMarkerStore,
};
use crate::environment::types::{
    same_environment_identity, EnvironmentKey, EnvironmentRef, ResourceLocator,
};
use crate::error::{AppError, RecoveryResourceId};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RecoveryConsistency {
    Consistent,
    Inconsistent,
    EnvironmentUnavailable,
}

pub trait RecoveryConsistencyChecker: Send + Sync {
    fn check<'a>(
        &'a self,
        marker: &'a RecoveryMarker,
    ) -> RecoveryFuture<'a, Result<RecoveryConsistency, AppError>>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoveryAssessmentState {
    NeedsAttention,
    ConsistentCanCleanup,
    EnvironmentUnavailable,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoveryStatusRevision(String);

impl RecoveryStatusRevision {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone)]
pub struct IndexedRecovery {
    pub marker: RecoveryMarker,
    pub marker_ref: RecoveryMarkerRef,
}

#[derive(Debug, Clone)]
pub struct InvalidRecoveryRecord {
    pub resource_id: RecoveryResourceId,
    pub environment: EnvironmentRef,
    pub managed_root: ResourceLocator,
    pub diagnostic: AppError,
}

#[derive(Default)]
struct RecoveryIndex {
    valid: HashMap<RecoveryResourceId, IndexedRecovery>,
    invalid: HashMap<RecoveryResourceId, InvalidRecoveryRecord>,
}

#[derive(Debug, Clone)]
pub struct RecoveryAssessment {
    pub state: RecoveryAssessmentState,
    pub revision: RecoveryStatusRevision,
    pub recovery: IndexedRecovery,
}

pub struct RecoveryRepository<C> {
    stores: Mutex<Vec<Arc<dyn RecoveryMarkerStore>>>,
    checker: Arc<C>,
    index: Mutex<RecoveryIndex>,
}

impl<C> RecoveryRepository<C>
where
    C: RecoveryConsistencyChecker,
{
    pub fn new(stores: Vec<Arc<dyn RecoveryMarkerStore>>, checker: Arc<C>) -> Self {
        Self {
            stores: Mutex::new(stores),
            checker,
            index: Mutex::new(RecoveryIndex::default()),
        }
    }

    pub fn register_store(&self, store: Arc<dyn RecoveryMarkerStore>) -> Result<(), AppError> {
        let environment = store.environment();
        let mut stores = lock(&self.stores)?;
        if let Some(existing) = stores
            .iter_mut()
            .find(|existing| same_environment_identity(&existing.environment(), &environment))
        {
            *existing = store;
        } else {
            stores.push(store);
        }
        Ok(())
    }

    pub async fn record_in_progress(
        &self,
        marker: RecoveryMarker,
    ) -> Result<RecoveryResourceId, AppError> {
        if marker.kind != RecoveryMarkerKind::InProgress {
            return Err(AppError::Validation {
                field: Some("kind".to_string()),
                message: "new recovery marker must be InProgress".to_string(),
            });
        }
        let store = self.store_for(&marker.environment)?;
        let marker_ref = store.create(&marker).await?;
        let id = marker.resource_id.clone();
        lock(&self.index)?
            .valid
            .insert(id.clone(), IndexedRecovery { marker, marker_ref });
        Ok(id)
    }

    #[cfg(test)]
    pub async fn record_required(&self, id: &RecoveryResourceId) -> Result<(), AppError> {
        self.update_kind(id, RecoveryMarkerKind::RecoveryRequired)
            .await
    }

    #[cfg(test)]
    pub async fn mark_cleanup_only(&self, id: &RecoveryResourceId) -> Result<(), AppError> {
        self.update_kind(id, RecoveryMarkerKind::CleanupOnly).await
    }

    pub fn resolve(&self, id: &RecoveryResourceId) -> Option<IndexedRecovery> {
        self.index.lock().ok()?.valid.get(id).cloned()
    }

    pub fn resolve_invalid(&self, id: &RecoveryResourceId) -> Option<InvalidRecoveryRecord> {
        self.index.lock().ok()?.invalid.get(id).cloned()
    }

    pub fn validate_managed_root(&self, root: &ResourceLocator) -> Result<(), AppError> {
        self.store_for(&root.environment)?
            .validate_managed_root(root)
    }

    pub fn invalid_records(&self) -> Result<Vec<InvalidRecoveryRecord>, AppError> {
        let mut records = lock(&self.index)?
            .invalid
            .values()
            .cloned()
            .collect::<Vec<_>>();
        records.sort_by(|left, right| {
            left.managed_root
                .native_path
                .cmp(&right.managed_root.native_path)
                .then_with(|| left.resource_id.as_str().cmp(right.resource_id.as_str()))
        });
        Ok(records)
    }

    pub async fn reindex_environment(
        &self,
        environment: &EnvironmentRef,
        active_operation_ids: &HashSet<String>,
    ) -> Result<(), AppError> {
        let store = self.store_for(environment)?;
        let loads = store.enumerate().await?;
        let mut valid = HashMap::new();
        let mut invalid = HashMap::new();
        for load in loads {
            match load {
                RecoveryMarkerLoad::Valid {
                    mut marker,
                    marker_ref,
                } => {
                    if marker.kind == RecoveryMarkerKind::InProgress
                        && !active_operation_ids.contains(&marker.operation_id)
                    {
                        let kind = match self.checker.check(&marker).await? {
                            RecoveryConsistency::Consistent => RecoveryMarkerKind::CleanupOnly,
                            RecoveryConsistency::Inconsistent => {
                                RecoveryMarkerKind::RecoveryRequired
                            }
                            RecoveryConsistency::EnvironmentUnavailable => {
                                RecoveryMarkerKind::InProgress
                            }
                        };
                        if kind != marker.kind {
                            marker.kind = kind;
                            store.update(&marker_ref, &marker).await?;
                        }
                    }
                    let resource_id = marker.resource_id.clone();
                    if valid
                        .insert(resource_id.clone(), IndexedRecovery { marker, marker_ref })
                        .is_some()
                    {
                        return Err(AppError::ConfigurationCorrupted {
                            message: format!(
                                "duplicate recovery resource ID {}",
                                resource_id.as_str()
                            ),
                        });
                    }
                }
                RecoveryMarkerLoad::Invalid {
                    mut managed_root,
                    error,
                } => {
                    managed_root.environment = environment.clone();
                    let resource_id = invalid_recovery_id(environment, &managed_root.native_path)?;
                    invalid.insert(
                        resource_id.clone(),
                        InvalidRecoveryRecord {
                            resource_id,
                            environment: environment.clone(),
                            managed_root,
                            diagnostic: error,
                        },
                    );
                }
            }
        }
        let mut index = lock(&self.index)?;
        let environment_key = EnvironmentKey::from_ref(environment);
        if let Some(id) = valid.keys().find(|id| {
            index.valid.get(*id).is_some_and(|existing| {
                EnvironmentKey::from_ref(&existing.marker.environment) != environment_key
            })
        }) {
            return Err(AppError::ConfigurationCorrupted {
                message: format!("duplicate recovery resource ID {}", id.as_str()),
            });
        }
        index.valid.retain(|_, recovery| {
            EnvironmentKey::from_ref(&recovery.marker.environment) != environment_key
        });
        index.invalid.retain(|_, recovery| {
            EnvironmentKey::from_ref(&recovery.environment) != environment_key
        });
        index.valid.extend(valid);
        index.invalid.extend(invalid);
        Ok(())
    }

    #[cfg(test)]
    pub fn protected_resources(&self) -> Vec<ResourceLocator> {
        let Ok(index) = self.index.lock() else {
            return Vec::new();
        };
        let mut resources = Vec::new();
        for recovery in index.valid.values() {
            resources.push(recovery.marker_ref.managed_root.clone());
            resources.extend(
                recovery
                    .marker
                    .entries
                    .iter()
                    .filter_map(|entry| entry.backup.clone()),
            );
        }
        resources.sort_by(|left, right| {
            format!("{:?}:{}", left.environment, left.native_path)
                .cmp(&format!("{:?}:{}", right.environment, right.native_path))
        });
        resources.dedup();
        resources
    }

    pub async fn assess(&self, id: &RecoveryResourceId) -> Result<RecoveryAssessment, AppError> {
        let recovery = self.resolve(id).ok_or_else(|| AppError::PathNotFound {
            path: id.as_str().to_string(),
        })?;
        self.assess_indexed(recovery).await
    }

    pub async fn assess_all(&self) -> Result<Vec<RecoveryAssessment>, AppError> {
        let mut recoveries = lock(&self.index)?
            .valid
            .values()
            .cloned()
            .collect::<Vec<_>>();
        recoveries.sort_by(|left, right| {
            left.marker
                .created_at_epoch_ms
                .cmp(&right.marker.created_at_epoch_ms)
                .then_with(|| {
                    left.marker
                        .resource_id
                        .as_str()
                        .cmp(right.marker.resource_id.as_str())
                })
        });
        let mut assessments = Vec::with_capacity(recoveries.len());
        for recovery in recoveries {
            assessments.push(self.assess_indexed(recovery).await?);
        }
        Ok(assessments)
    }

    async fn assess_indexed(
        &self,
        recovery: IndexedRecovery,
    ) -> Result<RecoveryAssessment, AppError> {
        let consistency = self.checker.check(&recovery.marker).await?;
        let state = match consistency {
            RecoveryConsistency::EnvironmentUnavailable => {
                RecoveryAssessmentState::EnvironmentUnavailable
            }
            RecoveryConsistency::Consistent
                if matches!(
                    recovery.marker.kind,
                    RecoveryMarkerKind::CleanupOnly | RecoveryMarkerKind::RecoveryRequired
                ) =>
            {
                RecoveryAssessmentState::ConsistentCanCleanup
            }
            RecoveryConsistency::Consistent | RecoveryConsistency::Inconsistent => {
                RecoveryAssessmentState::NeedsAttention
            }
        };
        let revision = status_revision(&recovery.marker, consistency)?;
        Ok(RecoveryAssessment {
            state,
            revision,
            recovery,
        })
    }

    pub async fn confirm_consistent(
        &self,
        id: &RecoveryResourceId,
        expected_revision: &str,
    ) -> Result<(), AppError> {
        let assessment = self.assess(id).await?;
        if assessment.revision.as_str() != expected_revision
            || assessment.state != RecoveryAssessmentState::ConsistentCanCleanup
        {
            return Err(AppError::StaleTarget);
        }
        if assessment.recovery.marker.kind != RecoveryMarkerKind::CleanupOnly {
            self.update_kind(id, RecoveryMarkerKind::CleanupOnly)
                .await?;
        }
        let recovery = self.resolve(id).ok_or(AppError::StaleTarget)?;
        self.cleanup_marker(&recovery.marker_ref, &recovery.marker)
            .await
    }

    #[cfg(test)]
    pub fn cleanup_candidates(&self, now_epoch_ms: u64, ttl_ms: u64) -> Vec<RecoveryResourceId> {
        let Ok(index) = self.index.lock() else {
            return Vec::new();
        };
        let mut ids = index
            .valid
            .iter()
            .filter(|(_, recovery)| {
                recovery.marker.kind == RecoveryMarkerKind::CleanupOnly
                    && recovery.marker.created_at_epoch_ms.saturating_add(ttl_ms) < now_epoch_ms
            })
            .map(|(id, _)| id.clone())
            .collect::<Vec<_>>();
        ids.sort_by(|left, right| left.as_str().cmp(right.as_str()));
        ids
    }

    async fn update_kind(
        &self,
        id: &RecoveryResourceId,
        kind: RecoveryMarkerKind,
    ) -> Result<(), AppError> {
        let mut recovery = self.resolve(id).ok_or_else(|| AppError::PathNotFound {
            path: id.as_str().to_string(),
        })?;
        recovery.marker.kind = kind;
        let store = self.store_for(&recovery.marker.environment)?;
        store.update(&recovery.marker_ref, &recovery.marker).await?;
        lock(&self.index)?.valid.insert(id.clone(), recovery);
        Ok(())
    }

    async fn update_marker(
        &self,
        marker_ref: &RecoveryMarkerRef,
        marker: &RecoveryMarker,
    ) -> Result<(), AppError> {
        if marker_ref.resource_id != marker.resource_id
            || !same_environment_identity(&marker_ref.environment, &marker.environment)
        {
            return Err(AppError::StaleTarget);
        }
        let store = self.store_for(&marker.environment)?;
        store.update(marker_ref, marker).await?;
        lock(&self.index)?.valid.insert(
            marker.resource_id.clone(),
            IndexedRecovery {
                marker: marker.clone(),
                marker_ref: marker_ref.clone(),
            },
        );
        Ok(())
    }

    async fn remove_marker(&self, marker_ref: &RecoveryMarkerRef) -> Result<(), AppError> {
        let store = self.store_for(&marker_ref.environment)?;
        store.remove(marker_ref).await?;
        lock(&self.index)?.valid.remove(&marker_ref.resource_id);
        Ok(())
    }

    async fn cleanup_marker(
        &self,
        marker_ref: &RecoveryMarkerRef,
        marker: &RecoveryMarker,
    ) -> Result<(), AppError> {
        let indexed = self
            .resolve(&marker_ref.resource_id)
            .ok_or(AppError::StaleTarget)?;
        if indexed.marker_ref != *marker_ref || indexed.marker != *marker {
            return Err(AppError::StaleTarget);
        }
        let store = self.store_for(&marker_ref.environment)?;
        store.cleanup(marker_ref, marker).await?;
        lock(&self.index)?.valid.remove(&marker_ref.resource_id);
        Ok(())
    }

    fn store_for(
        &self,
        environment: &EnvironmentRef,
    ) -> Result<Arc<dyn RecoveryMarkerStore>, AppError> {
        lock(&self.stores)?
            .iter()
            .find(|store| same_environment_identity(&store.environment(), environment))
            .cloned()
            .ok_or_else(|| AppError::EnvironmentUnavailable {
                environment: environment.clone(),
                message: "recovery marker store is unavailable".to_string(),
            })
    }
}

pub struct RepositoryRecoveryMarkerStore<C> {
    environment: EnvironmentRef,
    repository: Arc<RecoveryRepository<C>>,
}

impl<C> RepositoryRecoveryMarkerStore<C>
where
    C: RecoveryConsistencyChecker,
{
    pub fn new(environment: EnvironmentRef, repository: Arc<RecoveryRepository<C>>) -> Self {
        Self {
            environment,
            repository,
        }
    }
}

impl<C> RecoveryMarkerStore for RepositoryRecoveryMarkerStore<C>
where
    C: RecoveryConsistencyChecker + 'static,
{
    fn environment(&self) -> EnvironmentRef {
        self.environment.clone()
    }

    fn create<'a>(
        &'a self,
        marker: &'a RecoveryMarker,
    ) -> RecoveryFuture<'a, Result<RecoveryMarkerRef, AppError>> {
        Box::pin(async move {
            if !same_environment_identity(&marker.environment, &self.environment) {
                return Err(AppError::StaleEnvironment);
            }
            self.repository.record_in_progress(marker.clone()).await?;
            self.repository
                .resolve(&marker.resource_id)
                .map(|recovery| recovery.marker_ref)
                .ok_or(AppError::StaleTarget)
        })
    }

    fn update<'a>(
        &'a self,
        marker_ref: &'a RecoveryMarkerRef,
        marker: &'a RecoveryMarker,
    ) -> RecoveryFuture<'a, Result<(), AppError>> {
        Box::pin(async move { self.repository.update_marker(marker_ref, marker).await })
    }

    fn enumerate<'a>(&'a self) -> RecoveryFuture<'a, Result<Vec<RecoveryMarkerLoad>, AppError>> {
        Box::pin(async move {
            self.repository
                .store_for(&self.environment)?
                .enumerate()
                .await
        })
    }

    fn remove<'a>(
        &'a self,
        marker_ref: &'a RecoveryMarkerRef,
    ) -> RecoveryFuture<'a, Result<(), AppError>> {
        Box::pin(async move { self.repository.remove_marker(marker_ref).await })
    }

    fn cleanup<'a>(
        &'a self,
        marker_ref: &'a RecoveryMarkerRef,
        marker: &'a RecoveryMarker,
    ) -> RecoveryFuture<'a, Result<(), AppError>> {
        Box::pin(async move { self.repository.cleanup_marker(marker_ref, marker).await })
    }
}

fn status_revision(
    marker: &RecoveryMarker,
    consistency: RecoveryConsistency,
) -> Result<RecoveryStatusRevision, AppError> {
    let bytes = serde_json::to_vec(&(marker, consistency))?;
    Ok(RecoveryStatusRevision(format!(
        "recovery-status-v1-{:x}",
        Sha256::digest(bytes)
    )))
}

fn invalid_recovery_id(
    environment: &EnvironmentRef,
    managed_root: &str,
) -> Result<RecoveryResourceId, AppError> {
    let encoded = serde_json::to_vec(&(EnvironmentKey::from_ref(environment), managed_root))?;
    RecoveryResourceId::parse(format!("invalid-{:x}", Sha256::digest(encoded))).map_err(|_| {
        AppError::ConfigurationCorrupted {
            message: "failed to derive invalid recovery resource ID".to_string(),
        }
    })
}

fn lock<T>(mutex: &Mutex<T>) -> Result<MutexGuard<'_, T>, AppError> {
    mutex.lock().map_err(|_| AppError::Io {
        message: "recovery repository state is unavailable".to_string(),
    })
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::fs;
    use std::path::Path;
    use std::sync::{Arc, Mutex};

    use tempfile::tempdir;

    use super::*;
    use crate::environment::native::recovery::NativeRecoveryMarkerStore;
    use crate::environment::recovery::{
        RecoveryEntryPhase, RecoveryFuture, RecoveryMarkerEntry, RECOVERY_MARKER_SCHEMA_VERSION,
    };
    use crate::environment::types::{EnvironmentRef, ResourceLocator};
    use crate::error::{AppError, RecoveryResourceId};

    #[derive(Default)]
    struct FakeChecker {
        results: Mutex<HashMap<RecoveryResourceId, RecoveryConsistency>>,
    }

    impl FakeChecker {
        fn set(&self, id: &RecoveryResourceId, result: RecoveryConsistency) {
            self.results.lock().unwrap().insert(id.clone(), result);
        }
    }

    impl RecoveryConsistencyChecker for FakeChecker {
        fn check<'a>(
            &'a self,
            marker: &'a RecoveryMarker,
        ) -> RecoveryFuture<'a, Result<RecoveryConsistency, AppError>> {
            Box::pin(async move {
                Ok(self
                    .results
                    .lock()
                    .unwrap()
                    .get(&marker.resource_id)
                    .copied()
                    .unwrap_or(RecoveryConsistency::Inconsistent))
            })
        }
    }

    struct FailingCleanupStore {
        inner: NativeRecoveryMarkerStore,
    }

    impl RecoveryMarkerStore for FailingCleanupStore {
        fn environment(&self) -> EnvironmentRef {
            self.inner.environment()
        }

        fn create<'a>(
            &'a self,
            marker: &'a RecoveryMarker,
        ) -> RecoveryFuture<'a, Result<RecoveryMarkerRef, AppError>> {
            self.inner.create(marker)
        }

        fn update<'a>(
            &'a self,
            marker_ref: &'a RecoveryMarkerRef,
            marker: &'a RecoveryMarker,
        ) -> RecoveryFuture<'a, Result<(), AppError>> {
            self.inner.update(marker_ref, marker)
        }

        fn enumerate<'a>(
            &'a self,
        ) -> RecoveryFuture<'a, Result<Vec<RecoveryMarkerLoad>, AppError>> {
            self.inner.enumerate()
        }

        fn remove<'a>(
            &'a self,
            marker_ref: &'a RecoveryMarkerRef,
        ) -> RecoveryFuture<'a, Result<(), AppError>> {
            self.inner.remove(marker_ref)
        }

        fn cleanup<'a>(
            &'a self,
            _marker_ref: &'a RecoveryMarkerRef,
            _marker: &'a RecoveryMarker,
        ) -> RecoveryFuture<'a, Result<(), AppError>> {
            Box::pin(async {
                Err(AppError::Io {
                    message: "injected cleanup failure".to_string(),
                })
            })
        }
    }

    fn locator(path: &str) -> ResourceLocator {
        ResourceLocator {
            environment: EnvironmentRef::Host,
            native_path: path.to_string(),
        }
    }

    fn marker(id: &str, kind: RecoveryMarkerKind, created: u64) -> RecoveryMarker {
        RecoveryMarker {
            schema_version: RECOVERY_MARKER_SCHEMA_VERSION,
            resource_id: RecoveryResourceId::parse(id).expect("resource ID"),
            kind,
            environment: EnvironmentRef::Host,
            operation_id: format!("operation-{id}"),
            unit_id: "unit-1".to_string(),
            created_at_epoch_ms: created,
            entries: vec![RecoveryMarkerEntry {
                physical_target_digest: "target".to_string(),
                destination: locator("/work/skills/demo"),
                backup: Some(locator("/work/skills/.skill-deck-backup-demo")),
                expected_state: crate::environment::recovery::RecoveryExpectedEntryState::Present,
                original_fingerprint: "entry-v1-original".to_string(),
                phase: RecoveryEntryPhase::BackedUp,
            }],
        }
    }

    fn repository(root: &Path, checker: Arc<FakeChecker>) -> RecoveryRepository<FakeChecker> {
        let store = Arc::new(NativeRecoveryMarkerStore::new(root).expect("store"));
        RecoveryRepository::new(vec![store], checker)
    }

    #[tokio::test]
    async fn restart_reindexes_the_same_required_id_and_protects_its_resources() {
        let temp = tempdir().expect("temp");
        let checker = Arc::new(FakeChecker::default());
        let first = repository(temp.path(), checker.clone());
        let value = marker("recovery-1", RecoveryMarkerKind::InProgress, 1_000);
        let id = first.record_in_progress(value).await.expect("record");
        first.record_required(&id).await.expect("required");
        drop(first);

        let restarted = repository(temp.path(), checker);
        restarted
            .reindex_environment(&EnvironmentRef::Host, &Default::default())
            .await
            .expect("reindex");
        let resolved = restarted.resolve(&id).expect("resolved");
        assert_eq!(resolved.marker.kind, RecoveryMarkerKind::RecoveryRequired);
        let protected = restarted.protected_resources();
        assert!(protected
            .iter()
            .any(|resource| resource.native_path.contains(".skill-deck-backup-demo")));
        assert!(protected
            .iter()
            .any(|resource| resource.native_path.contains("operation-recovery-1")));
    }

    #[tokio::test]
    async fn orphan_in_progress_is_reclassified_from_backend_consistency() {
        let temp = tempdir().expect("temp");
        let checker = Arc::new(FakeChecker::default());
        let first = repository(temp.path(), checker.clone());
        let consistent = first
            .record_in_progress(marker("consistent", RecoveryMarkerKind::InProgress, 1_000))
            .await
            .unwrap();
        let inconsistent = first
            .record_in_progress(marker(
                "inconsistent",
                RecoveryMarkerKind::InProgress,
                1_000,
            ))
            .await
            .unwrap();
        checker.set(&consistent, RecoveryConsistency::Consistent);
        checker.set(&inconsistent, RecoveryConsistency::Inconsistent);
        drop(first);

        let restarted = repository(temp.path(), checker);
        restarted
            .reindex_environment(&EnvironmentRef::Host, &Default::default())
            .await
            .expect("reindex");
        assert_eq!(
            restarted.resolve(&consistent).unwrap().marker.kind,
            RecoveryMarkerKind::CleanupOnly
        );
        assert_eq!(
            restarted.resolve(&inconsistent).unwrap().marker.kind,
            RecoveryMarkerKind::RecoveryRequired
        );
    }

    #[tokio::test]
    async fn confirmation_requires_current_consistent_status_revision() {
        let temp = tempdir().expect("temp");
        let checker = Arc::new(FakeChecker::default());
        let repository = repository(temp.path(), checker.clone());
        let target_parent = temp.path().join("target");
        fs::create_dir_all(&target_parent).expect("target parent");
        let backup = target_parent.join(".skill-deck-backup-demo");
        fs::create_dir(&backup).expect("backup");
        fs::write(backup.join("SKILL.md"), b"backup").expect("backup file");
        let mut recovery_marker = marker("recovery-2", RecoveryMarkerKind::InProgress, 1_000);
        recovery_marker.entries[0].destination =
            locator(&target_parent.join("demo").to_string_lossy());
        recovery_marker.entries[0].backup = Some(locator(&backup.to_string_lossy()));
        let id = repository
            .record_in_progress(recovery_marker)
            .await
            .unwrap();
        let managed_root = repository
            .resolve(&id)
            .expect("indexed recovery")
            .marker_ref
            .managed_root
            .native_path;
        repository.record_required(&id).await.unwrap();
        checker.set(&id, RecoveryConsistency::Consistent);
        let assessment = repository.assess(&id).await.expect("assessment");
        assert_eq!(
            assessment.state,
            RecoveryAssessmentState::ConsistentCanCleanup
        );

        assert!(matches!(
            repository.confirm_consistent(&id, "stale-revision").await,
            Err(AppError::StaleTarget)
        ));
        repository
            .confirm_consistent(&id, assessment.revision.as_str())
            .await
            .expect("confirm");
        assert!(repository.resolve(&id).is_none());
        assert!(!backup.exists());
        assert!(!Path::new(&managed_root).exists());
    }

    #[tokio::test]
    async fn failed_cleanup_retains_cleanup_only_marker_and_backup_for_retry() {
        let temp = tempdir().expect("temp");
        let checker = Arc::new(FakeChecker::default());
        let store: Arc<dyn RecoveryMarkerStore> = Arc::new(FailingCleanupStore {
            inner: NativeRecoveryMarkerStore::new(temp.path().join("recovery")).expect("store"),
        });
        let repository = RecoveryRepository::new(vec![store], checker.clone());
        let target_parent = temp.path().join("target");
        fs::create_dir_all(&target_parent).expect("target parent");
        let backup = target_parent.join(".skill-deck-backup-demo");
        fs::create_dir(&backup).expect("backup");
        let mut recovery_marker = marker("cleanup-failure", RecoveryMarkerKind::InProgress, 1_000);
        recovery_marker.entries[0].destination =
            locator(&target_parent.join("demo").to_string_lossy());
        recovery_marker.entries[0].backup = Some(locator(&backup.to_string_lossy()));
        let id = repository
            .record_in_progress(recovery_marker)
            .await
            .expect("record");
        repository.record_required(&id).await.expect("required");
        checker.set(&id, RecoveryConsistency::Consistent);
        let assessment = repository.assess(&id).await.expect("assessment");

        assert!(matches!(
            repository
                .confirm_consistent(&id, assessment.revision.as_str())
                .await,
            Err(AppError::Io { .. })
        ));
        let retained = repository.resolve(&id).expect("retained marker");
        assert_eq!(retained.marker.kind, RecoveryMarkerKind::CleanupOnly);
        assert!(backup.exists());
        assert!(Path::new(&retained.marker_ref.managed_root.native_path).exists());
    }

    #[tokio::test]
    async fn ttl_cleanup_selects_only_old_cleanup_only_markers() {
        let temp = tempdir().expect("temp");
        let checker = Arc::new(FakeChecker::default());
        let repository = repository(temp.path(), checker);
        let cleanup = repository
            .record_in_progress(marker("cleanup", RecoveryMarkerKind::CleanupOnly, 1_000))
            .await
            .unwrap_err();
        assert!(matches!(cleanup, AppError::Validation { .. }));

        let cleanup_id = repository
            .record_in_progress(marker("cleanup", RecoveryMarkerKind::InProgress, 1_000))
            .await
            .unwrap();
        repository.mark_cleanup_only(&cleanup_id).await.unwrap();
        let required_id = repository
            .record_in_progress(marker("required", RecoveryMarkerKind::InProgress, 1_000))
            .await
            .unwrap();
        repository.record_required(&required_id).await.unwrap();

        assert_eq!(
            repository.cleanup_candidates(100_000, 10_000),
            vec![cleanup_id]
        );
        assert!(Path::new(
            &repository
                .resolve(&required_id)
                .unwrap()
                .marker_ref
                .managed_root
                .native_path
        )
        .exists());
        assert!(fs::read_dir(temp.path()).unwrap().count() >= 2);
    }
}

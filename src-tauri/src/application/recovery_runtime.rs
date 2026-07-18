use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;

use crate::application::recovery::RecoveryService;
use crate::environment::native::recovery::NativeRecoveryMarkerStore;
use crate::environment::native::tree::inspect_entry_no_follow;
use crate::environment::recovery::{
    RecoveryEntryPhase, RecoveryExpectedEntryState, RecoveryFuture, RecoveryMarker,
    RecoveryMarkerEntry, RecoveryMarkerStore,
};
use crate::environment::types::EnvironmentRef;
use crate::environment::wsl::operations::entry::{inspect_entries, PosixEntryKind};
use crate::environment::wsl::operations::recovery::WslRecoveryMarkerStore;
use crate::environment::wsl::{EnvironmentRegistry, WslSession};
use crate::error::AppError;
use crate::storage::recovery_repository::{
    RecoveryConsistency, RecoveryConsistencyChecker, RecoveryRepository,
    RepositoryRecoveryMarkerStore,
};

#[derive(Clone)]
pub struct RuntimeRecoveryConsistencyChecker {
    environments: Arc<EnvironmentRegistry>,
}

impl RuntimeRecoveryConsistencyChecker {
    pub fn new(environments: Arc<EnvironmentRegistry>) -> Self {
        Self { environments }
    }

    async fn check_runtime(
        &self,
        marker: &RecoveryMarker,
    ) -> Result<RecoveryConsistency, AppError> {
        match &marker.environment {
            EnvironmentRef::Host => {
                let marker = marker.clone();
                tokio::task::spawn_blocking(move || check_host_marker(&marker))
                    .await
                    .map_err(|error| AppError::ExecutionFailed {
                        message: format!("host recovery consistency task failed: {error}"),
                    })?
            }
            EnvironmentRef::Wsl { distro_name } => {
                let marker = marker.clone();
                match self
                    .environments
                    .with_session(distro_name, move |session| {
                        let marker = marker.clone();
                        async move { check_wsl_marker(&session, &marker).await }
                    })
                    .await
                {
                    Ok(consistency) => Ok(consistency),
                    Err(error) if environment_unavailable(&error) => {
                        Ok(RecoveryConsistency::EnvironmentUnavailable)
                    }
                    Err(error) => Err(error),
                }
            }
        }
    }
}

impl RecoveryConsistencyChecker for RuntimeRecoveryConsistencyChecker {
    fn check<'a>(
        &'a self,
        marker: &'a RecoveryMarker,
    ) -> RecoveryFuture<'a, Result<RecoveryConsistency, AppError>> {
        Box::pin(async move { self.check_runtime(marker).await })
    }
}

fn check_host_marker(marker: &RecoveryMarker) -> Result<RecoveryConsistency, AppError> {
    for entry in &marker.entries {
        let destination = inspect_entry_no_follow(Path::new(&entry.destination.native_path))?;
        let backup_exists = entry
            .backup
            .as_ref()
            .map(|backup| {
                inspect_entry_no_follow(Path::new(&backup.native_path))
                    .map(|state| state.fingerprint.0 != "entry-v1-missing")
            })
            .transpose()?
            .unwrap_or(false);
        if !entry_is_consistent(entry, &destination.fingerprint.0, backup_exists) {
            return Ok(RecoveryConsistency::Inconsistent);
        }
    }
    Ok(RecoveryConsistency::Consistent)
}

async fn check_wsl_marker(
    session: &WslSession,
    marker: &RecoveryMarker,
) -> Result<RecoveryConsistency, AppError> {
    for entry in &marker.entries {
        let mut paths = vec![entry.destination.native_path.clone()];
        if let Some(backup) = &entry.backup {
            paths.push(backup.native_path.clone());
        }
        let states = inspect_entries(session, &paths, None).await?;
        let destination = &states[0].fingerprint.0;
        let backup_exists = states
            .get(1)
            .is_some_and(|state| state.kind != PosixEntryKind::Missing);
        if !entry_is_consistent(entry, destination, backup_exists) {
            return Ok(RecoveryConsistency::Inconsistent);
        }
    }
    Ok(RecoveryConsistency::Consistent)
}

fn entry_is_consistent(
    entry: &RecoveryMarkerEntry,
    destination_fingerprint: &str,
    backup_exists: bool,
) -> bool {
    if entry.expected_state == RecoveryExpectedEntryState::Unknown
        || entry.original_fingerprint.is_empty()
    {
        return false;
    }
    let destination_missing = destination_fingerprint == "entry-v1-missing";
    let original_matches = destination_fingerprint == entry.original_fingerprint;
    match entry.phase {
        RecoveryEntryPhase::Staged | RecoveryEntryPhase::BackedUp => {
            original_matches && !backup_exists
        }
        RecoveryEntryPhase::Swapped
        | RecoveryEntryPhase::Verified
        | RecoveryEntryPhase::LockCommitted => match entry.expected_state {
            RecoveryExpectedEntryState::Present => !destination_missing,
            RecoveryExpectedEntryState::Missing => destination_missing,
            RecoveryExpectedEntryState::Unknown => false,
        },
        RecoveryEntryPhase::RestoreFailed => original_matches,
    }
}

fn environment_unavailable(error: &AppError) -> bool {
    matches!(
        error,
        AppError::EnvironmentUnavailable { .. }
            | AppError::EnvironmentDiscoveryFailed { .. }
            | AppError::WslCommandFailed { .. }
            | AppError::WslCommandTimedOut
    )
}

pub type RuntimeRecoveryRepository = RecoveryRepository<RuntimeRecoveryConsistencyChecker>;
pub type RuntimeRecoveryService = RecoveryService<RuntimeRecoveryConsistencyChecker>;

pub struct RuntimeRecoveryGraph {
    repository: Arc<RuntimeRecoveryRepository>,
    native_store: Arc<dyn RecoveryMarkerStore>,
}

impl RuntimeRecoveryGraph {
    pub fn new(
        environments: Arc<EnvironmentRegistry>,
        recovery_root: std::path::PathBuf,
    ) -> Result<Self, AppError> {
        let native_underlying = Arc::new(NativeRecoveryMarkerStore::new(recovery_root)?);
        let checker = Arc::new(RuntimeRecoveryConsistencyChecker::new(environments));
        let repository_store: Arc<dyn RecoveryMarkerStore> = native_underlying.clone();
        let repository = Arc::new(RecoveryRepository::new(vec![repository_store], checker));
        let native_store: Arc<dyn RecoveryMarkerStore> = Arc::new(
            RepositoryRecoveryMarkerStore::new(EnvironmentRef::Host, Arc::clone(&repository)),
        );
        Ok(Self {
            repository,
            native_store,
        })
    }

    pub fn native_store(&self) -> Arc<dyn RecoveryMarkerStore> {
        Arc::clone(&self.native_store)
    }

    pub fn wsl_store(&self, session: WslSession) -> Result<Arc<dyn RecoveryMarkerStore>, AppError> {
        let environment = EnvironmentRef::Wsl {
            distro_name: session.distro_name.clone(),
        };
        let underlying: Arc<dyn RecoveryMarkerStore> =
            Arc::new(WslRecoveryMarkerStore::new(session));
        self.repository.register_store(underlying)?;
        Ok(Arc::new(RepositoryRecoveryMarkerStore::new(
            environment,
            Arc::clone(&self.repository),
        )))
    }

    pub async fn reindex_wsl(&self, session: WslSession) -> Result<(), AppError> {
        let environment = EnvironmentRef::Wsl {
            distro_name: session.distro_name.clone(),
        };
        let underlying: Arc<dyn RecoveryMarkerStore> =
            Arc::new(WslRecoveryMarkerStore::new(session));
        self.repository.register_store(underlying)?;
        self.repository
            .reindex_environment(&environment, &HashSet::new())
            .await
    }

    pub fn service(&self) -> RuntimeRecoveryService {
        RecoveryService::new(Arc::clone(&self.repository))
    }

    pub async fn reindex_host(&self) -> Result<(), AppError> {
        self.repository
            .reindex_environment(&EnvironmentRef::Host, &HashSet::new())
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::environment::types::ResourceLocator;
    use crate::error::RecoveryResourceId;

    fn entry(
        phase: RecoveryEntryPhase,
        expected_state: RecoveryExpectedEntryState,
        original: &str,
    ) -> RecoveryMarkerEntry {
        RecoveryMarkerEntry {
            physical_target_digest: "target-1".to_string(),
            destination: ResourceLocator {
                environment: EnvironmentRef::Host,
                native_path: "/work/demo".to_string(),
            },
            backup: Some(ResourceLocator {
                environment: EnvironmentRef::Host,
                native_path: "/work/.skill-deck-backup-demo".to_string(),
            }),
            expected_state,
            original_fingerprint: original.to_string(),
            phase,
        }
    }

    #[test]
    fn checker_distinguishes_successful_remove_from_restore_failure() {
        let removed = entry(
            RecoveryEntryPhase::LockCommitted,
            RecoveryExpectedEntryState::Missing,
            "entry-v1-original",
        );
        assert!(entry_is_consistent(&removed, "entry-v1-missing", true));

        let restore_failed = entry(
            RecoveryEntryPhase::RestoreFailed,
            RecoveryExpectedEntryState::Present,
            "entry-v1-original",
        );
        assert!(!entry_is_consistent(
            &restore_failed,
            "entry-v1-different",
            true
        ));
    }

    #[test]
    fn legacy_unknown_marker_never_becomes_automatic_cleanup() {
        let legacy = entry(
            RecoveryEntryPhase::LockCommitted,
            RecoveryExpectedEntryState::Unknown,
            "",
        );
        assert!(!entry_is_consistent(&legacy, "entry-v1-present", false));
        assert!(RecoveryResourceId::parse("recovery-legacy").is_ok());
    }
}

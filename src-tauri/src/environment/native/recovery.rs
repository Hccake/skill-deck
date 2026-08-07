use std::fs;
use std::path::{Path, PathBuf};

use crate::environment::native::atomic_file::NativeAtomicDocumentIo;
use crate::environment::native::tree::{inspect_entry_no_follow, NativeEntryKind};
use crate::environment::recovery::{
    validate_recovery_marker, RecoveryFuture, RecoveryMarker, RecoveryMarkerKind,
    RecoveryMarkerLoad, RecoveryMarkerRef, RecoveryMarkerStore,
};
use crate::environment::types::{EnvironmentRef, ResourceLocator};
use crate::error::{AppError, RecoveryResourceId};
use crate::storage::atomic_document::AtomicDocumentIo;

const MARKER_FILE: &str = "recovery.json";

#[derive(Clone)]
pub struct NativeRecoveryMarkerStore {
    root: PathBuf,
    io: NativeAtomicDocumentIo,
}

impl NativeRecoveryMarkerStore {
    pub fn new(root: impl AsRef<Path>) -> Result<Self, AppError> {
        fs::create_dir_all(root.as_ref())?;
        Ok(Self {
            root: fs::canonicalize(root.as_ref())?,
            io: NativeAtomicDocumentIo,
        })
    }

    fn managed_root(&self, id: &RecoveryResourceId) -> PathBuf {
        self.root.join(format!("operation-{}", id.as_str()))
    }

    fn marker_locator(&self, root: &Path) -> ResourceLocator {
        ResourceLocator {
            environment: EnvironmentRef::Host,
            native_path: root.join(MARKER_FILE).to_string_lossy().into_owned(),
        }
    }

    fn marker_ref(&self, marker: &RecoveryMarker, root: &Path) -> RecoveryMarkerRef {
        RecoveryMarkerRef {
            resource_id: marker.resource_id.clone(),
            environment: EnvironmentRef::Host,
            managed_root: ResourceLocator {
                environment: EnvironmentRef::Host,
                native_path: root.to_string_lossy().into_owned(),
            },
        }
    }

    fn verify_ref(&self, marker_ref: &RecoveryMarkerRef) -> Result<PathBuf, AppError> {
        if marker_ref.environment != EnvironmentRef::Host
            || marker_ref.managed_root.environment != EnvironmentRef::Host
        {
            return Err(AppError::StorageUnsupported {
                path: marker_ref.managed_root.native_path.clone(),
            });
        }
        let expected = self.managed_root(&marker_ref.resource_id);
        if Path::new(&marker_ref.managed_root.native_path) != expected {
            return Err(AppError::UnsafePath {
                path: marker_ref.managed_root.native_path.clone(),
                reason: "recovery root is outside the managed namespace".to_string(),
            });
        }
        Ok(expected)
    }

    fn parse_load(&self, root: PathBuf) -> RecoveryMarkerLoad {
        let managed_root = ResourceLocator {
            environment: EnvironmentRef::Host,
            native_path: root.to_string_lossy().into_owned(),
        };
        let result = (|| {
            let metadata = fs::symlink_metadata(&root)?;
            if !metadata.is_dir() || metadata.file_type().is_symlink() {
                return Err(AppError::UnsafePath {
                    path: root.to_string_lossy().into_owned(),
                    reason: "recovery root is not a managed directory".to_string(),
                });
            }
            let marker: RecoveryMarker =
                serde_json::from_slice(&fs::read(root.join(MARKER_FILE))?)?;
            validate_recovery_marker(&marker)?;
            if marker.environment != EnvironmentRef::Host
                || self.managed_root(&marker.resource_id) != root
            {
                return Err(AppError::ConfigurationCorrupted {
                    message: "recovery marker does not match its managed root".to_string(),
                });
            }
            Ok(marker)
        })();
        match result {
            Ok(marker) => RecoveryMarkerLoad::Valid {
                marker_ref: self.marker_ref(&marker, &root),
                marker,
            },
            Err(error) => RecoveryMarkerLoad::Invalid {
                managed_root,
                error,
            },
        }
    }

    fn remove_backup(path: &Path) -> Result<(), AppError> {
        match fs::symlink_metadata(path) {
            Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {
                fs::remove_dir_all(path)?;
            }
            Ok(_) => fs::remove_file(path)?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        Ok(())
    }
}

impl RecoveryMarkerStore for NativeRecoveryMarkerStore {
    fn environment(&self) -> EnvironmentRef {
        EnvironmentRef::Host
    }

    fn validate_managed_root(&self, root: &ResourceLocator) -> Result<(), AppError> {
        if root.environment != EnvironmentRef::Host {
            return Err(AppError::StorageUnsupported {
                path: root.native_path.clone(),
            });
        }
        let path = Path::new(&root.native_path);
        let parent = path.parent().ok_or_else(|| AppError::UnsafePath {
            path: root.native_path.clone(),
            reason: "recovery root has no managed parent".to_string(),
        })?;
        if fs::canonicalize(parent)? != self.root
            || !path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("operation-"))
            || inspect_entry_no_follow(path)?.kind != NativeEntryKind::Directory
        {
            return Err(AppError::UnsafePath {
                path: root.native_path.clone(),
                reason: "recovery root is outside the managed namespace".to_string(),
            });
        }
        Ok(())
    }

    fn create<'a>(
        &'a self,
        marker: &'a RecoveryMarker,
    ) -> RecoveryFuture<'a, Result<RecoveryMarkerRef, AppError>> {
        Box::pin(async move {
            validate_recovery_marker(marker)?;
            if marker.environment != EnvironmentRef::Host {
                return Err(AppError::StorageUnsupported {
                    path: format!("{:?}", marker.environment),
                });
            }
            let root = self.managed_root(&marker.resource_id);
            fs::create_dir(&root)?;
            let result = self
                .io
                .write_atomic(
                    &self.marker_locator(&root),
                    serde_json::to_vec_pretty(marker)?,
                )
                .await;
            if result.is_err() {
                let _ = fs::remove_dir_all(&root);
            }
            result?;
            Ok(self.marker_ref(marker, &root))
        })
    }

    fn update<'a>(
        &'a self,
        marker_ref: &'a RecoveryMarkerRef,
        marker: &'a RecoveryMarker,
    ) -> RecoveryFuture<'a, Result<(), AppError>> {
        Box::pin(async move {
            validate_recovery_marker(marker)?;
            let root = self.verify_ref(marker_ref)?;
            if marker.resource_id != marker_ref.resource_id
                || marker.environment != marker_ref.environment
            {
                return Err(AppError::StaleTarget);
            }
            self.io
                .write_atomic(
                    &self.marker_locator(&root),
                    serde_json::to_vec_pretty(marker)?,
                )
                .await
        })
    }

    fn enumerate<'a>(&'a self) -> RecoveryFuture<'a, Result<Vec<RecoveryMarkerLoad>, AppError>> {
        let store = self.clone();
        Box::pin(async move {
            tokio::task::spawn_blocking(move || {
                let mut roots = fs::read_dir(&store.root)?
                    .filter_map(Result::ok)
                    .filter(|entry| {
                        entry
                            .file_name()
                            .to_string_lossy()
                            .starts_with("operation-")
                    })
                    .map(|entry| entry.path())
                    .collect::<Vec<_>>();
                roots.sort();
                Ok(roots
                    .into_iter()
                    .map(|root| store.parse_load(root))
                    .collect())
            })
            .await
            .map_err(|error| AppError::ExecutionFailed {
                message: format!("native recovery enumeration task failed: {error}"),
            })?
        })
    }

    fn remove<'a>(
        &'a self,
        marker_ref: &'a RecoveryMarkerRef,
    ) -> RecoveryFuture<'a, Result<(), AppError>> {
        Box::pin(async move {
            let root = self.verify_ref(marker_ref)?;
            let load = self.parse_load(root.clone());
            match load {
                RecoveryMarkerLoad::Valid { marker, .. }
                    if marker.resource_id == marker_ref.resource_id
                        && marker.kind == RecoveryMarkerKind::CleanupOnly =>
                {
                    fs::remove_dir_all(root)?;
                    Ok(())
                }
                RecoveryMarkerLoad::Valid { .. } => Err(AppError::StaleTarget),
                RecoveryMarkerLoad::Invalid { error, .. } => Err(error),
            }
        })
    }

    fn cleanup<'a>(
        &'a self,
        marker_ref: &'a RecoveryMarkerRef,
        marker: &'a RecoveryMarker,
    ) -> RecoveryFuture<'a, Result<(), AppError>> {
        Box::pin(async move {
            validate_recovery_marker(marker)?;
            let root = self.verify_ref(marker_ref)?;
            match self.parse_load(root.clone()) {
                RecoveryMarkerLoad::Valid { marker: stored, .. }
                    if stored == *marker
                        && stored.resource_id == marker_ref.resource_id
                        && stored.kind == RecoveryMarkerKind::CleanupOnly => {}
                RecoveryMarkerLoad::Valid { .. } => return Err(AppError::StaleTarget),
                RecoveryMarkerLoad::Invalid { error, .. } => return Err(error),
            }
            for backup in marker
                .entries
                .iter()
                .filter_map(|entry| entry.backup.as_ref())
            {
                Self::remove_backup(Path::new(&backup.native_path))?;
            }
            fs::remove_dir_all(root)?;
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::*;
    use crate::environment::recovery::{
        RecoveryEntryPhase, RecoveryMarkerEntry, RecoveryMarkerStore,
        RECOVERY_MARKER_SCHEMA_VERSION,
    };

    fn marker(id: &str, kind: RecoveryMarkerKind) -> RecoveryMarker {
        RecoveryMarker {
            schema_version: RECOVERY_MARKER_SCHEMA_VERSION,
            resource_id: RecoveryResourceId::parse(id).expect("resource ID"),
            kind,
            environment: EnvironmentRef::Host,
            operation_id: "operation-1".to_string(),
            unit_id: "unit-1".to_string(),
            subject: Some(crate::environment::recovery::RecoverySubject {
                operation_kind: crate::core::mutation::MutationKind::Install,
                skill_name: "demo".to_string(),
                context: crate::environment::types::ContextRef {
                    environment: EnvironmentRef::Host,
                    scope: crate::environment::types::ContextScope::Global,
                },
            }),
            created_at_epoch_ms: 1_000,
            entries: vec![RecoveryMarkerEntry {
                physical_target_digest: "target-digest".to_string(),
                destination: locator("/work/.agents/skills/demo"),
                backup: Some(locator("/work/.agents/skills/.skill-deck-backup-demo")),
                expected_state: crate::environment::recovery::RecoveryExpectedEntryState::Present,
                original_fingerprint: "entry-v1-original".to_string(),
                phase: RecoveryEntryPhase::BackedUp,
            }],
        }
    }

    fn locator(path: &str) -> ResourceLocator {
        ResourceLocator {
            environment: EnvironmentRef::Host,
            native_path: path.to_string(),
        }
    }

    #[tokio::test]
    async fn marker_survives_store_recreation_and_updates_kind() {
        let temp = tempdir().expect("temp");
        let store = NativeRecoveryMarkerStore::new(temp.path()).expect("store");
        let mut value = marker("recovery-1", RecoveryMarkerKind::InProgress);
        let marker_ref = store.create(&value).await.expect("create");
        value.kind = RecoveryMarkerKind::RecoveryRequired;
        store.update(&marker_ref, &value).await.expect("update");
        drop(store);

        let reopened = NativeRecoveryMarkerStore::new(temp.path()).expect("reopen");
        let loads = reopened.enumerate().await.expect("enumerate");
        assert!(matches!(
            loads.as_slice(),
            [RecoveryMarkerLoad::Valid { marker, marker_ref: loaded_ref }]
                if marker == &value && loaded_ref.resource_id == value.resource_id
        ));
    }

    #[tokio::test]
    async fn corrupt_and_future_markers_are_quarantined_without_deletion() {
        let temp = tempdir().expect("temp");
        let store = NativeRecoveryMarkerStore::new(temp.path()).expect("store");
        let corrupt_root = temp.path().join("operation-corrupt");
        fs::create_dir(&corrupt_root).expect("root");
        fs::write(corrupt_root.join(MARKER_FILE), b"not-json").expect("corrupt marker");
        let future_root = temp.path().join("operation-future");
        fs::create_dir(&future_root).expect("root");
        let mut future = marker("future", RecoveryMarkerKind::InProgress);
        future.schema_version = 99;
        fs::write(
            future_root.join(MARKER_FILE),
            serde_json::to_vec(&future).unwrap(),
        )
        .expect("future marker");

        let loads = store.enumerate().await.expect("enumerate");
        assert_eq!(
            loads
                .iter()
                .filter(|load| matches!(load, RecoveryMarkerLoad::Invalid { .. }))
                .count(),
            2
        );
        assert!(corrupt_root.is_dir());
        assert!(future_root.is_dir());
    }

    #[tokio::test]
    async fn backup_outside_destination_parent_is_rejected_before_marker_write() {
        let temp = tempdir().expect("temp");
        let store = NativeRecoveryMarkerStore::new(temp.path()).expect("store");
        let mut value = marker("recovery-2", RecoveryMarkerKind::InProgress);
        value.entries[0].backup = Some(locator("/other/.skill-deck-backup-demo"));

        assert!(store.create(&value).await.is_err());
        assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 0);
    }

    #[tokio::test]
    async fn remove_requires_the_indexed_managed_root_and_valid_marker() {
        let temp = tempdir().expect("temp");
        let store = NativeRecoveryMarkerStore::new(temp.path()).expect("store");
        let value = marker("recovery-3", RecoveryMarkerKind::CleanupOnly);
        let marker_ref = store.create(&value).await.expect("create");
        let forged = RecoveryMarkerRef {
            managed_root: locator("/tmp/not-managed"),
            ..marker_ref.clone()
        };
        assert!(store.remove(&forged).await.is_err());
        assert!(Path::new(&marker_ref.managed_root.native_path).is_dir());

        store.remove(&marker_ref).await.expect("remove");
        assert!(!Path::new(&marker_ref.managed_root.native_path).exists());
    }
}

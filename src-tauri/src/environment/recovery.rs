use std::future::Future;
use std::pin::Pin;

use serde::{Deserialize, Serialize};

use crate::environment::types::{same_environment_identity, EnvironmentRef, ResourceLocator};
use crate::error::{AppError, RecoveryResourceId};

pub const RECOVERY_MARKER_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum RecoveryMarkerKind {
    InProgress,
    CleanupOnly,
    RecoveryRequired,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum RecoveryEntryPhase {
    Staged,
    BackedUp,
    Swapped,
    Verified,
    LockCommitted,
    RestoreFailed,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum RecoveryExpectedEntryState {
    Present,
    Missing,
    #[default]
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryMarkerEntry {
    pub physical_target_digest: String,
    pub destination: ResourceLocator,
    pub backup: Option<ResourceLocator>,
    #[serde(default)]
    pub expected_state: RecoveryExpectedEntryState,
    #[serde(default)]
    pub original_fingerprint: String,
    pub phase: RecoveryEntryPhase,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryMarker {
    pub schema_version: u32,
    pub resource_id: RecoveryResourceId,
    pub kind: RecoveryMarkerKind,
    pub environment: EnvironmentRef,
    pub operation_id: String,
    pub unit_id: String,
    pub created_at_epoch_ms: u64,
    pub entries: Vec<RecoveryMarkerEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoveryMarkerRef {
    pub resource_id: RecoveryResourceId,
    pub environment: EnvironmentRef,
    pub managed_root: ResourceLocator,
}

#[derive(Debug)]
pub enum RecoveryMarkerLoad {
    Valid {
        marker: RecoveryMarker,
        marker_ref: RecoveryMarkerRef,
    },
    Invalid {
        managed_root: ResourceLocator,
        error: AppError,
    },
}

pub type RecoveryFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

pub trait RecoveryMarkerStore: Send + Sync {
    fn environment(&self) -> EnvironmentRef;

    fn validate_managed_root(&self, _root: &ResourceLocator) -> Result<(), AppError> {
        Ok(())
    }

    fn create<'a>(
        &'a self,
        marker: &'a RecoveryMarker,
    ) -> RecoveryFuture<'a, Result<RecoveryMarkerRef, AppError>>;

    fn update<'a>(
        &'a self,
        marker_ref: &'a RecoveryMarkerRef,
        marker: &'a RecoveryMarker,
    ) -> RecoveryFuture<'a, Result<(), AppError>>;

    fn enumerate<'a>(&'a self) -> RecoveryFuture<'a, Result<Vec<RecoveryMarkerLoad>, AppError>>;

    fn remove<'a>(
        &'a self,
        marker_ref: &'a RecoveryMarkerRef,
    ) -> RecoveryFuture<'a, Result<(), AppError>>;

    fn cleanup<'a>(
        &'a self,
        marker_ref: &'a RecoveryMarkerRef,
        marker: &'a RecoveryMarker,
    ) -> RecoveryFuture<'a, Result<(), AppError>>;
}

pub fn validate_recovery_marker(marker: &RecoveryMarker) -> Result<(), AppError> {
    if marker.schema_version != RECOVERY_MARKER_SCHEMA_VERSION {
        return Err(AppError::ConfigurationCorrupted {
            message: "unsupported recovery marker schema".to_string(),
        });
    }
    if marker.operation_id.is_empty() || marker.unit_id.is_empty() || marker.entries.is_empty() {
        return Err(AppError::ConfigurationCorrupted {
            message: "recovery marker is incomplete".to_string(),
        });
    }
    for entry in &marker.entries {
        if entry.physical_target_digest.is_empty()
            || !same_environment_identity(&entry.destination.environment, &marker.environment)
            || (entry.expected_state != RecoveryExpectedEntryState::Unknown
                && entry.original_fingerprint.is_empty())
        {
            return Err(AppError::ConfigurationCorrupted {
                message: "recovery entry identity does not match its Environment".to_string(),
            });
        }
        if let Some(backup) = &entry.backup {
            if !same_environment_identity(&backup.environment, &entry.destination.environment)
                || parent_path(&backup.native_path) != parent_path(&entry.destination.native_path)
                || !final_name(&backup.native_path)
                    .is_some_and(|name| name.starts_with(".skill-deck-backup-"))
            {
                return Err(AppError::ConfigurationCorrupted {
                    message: "recovery backup is outside its physical target parent".to_string(),
                });
            }
        }
    }
    Ok(())
}

fn parent_path(path: &str) -> Option<String> {
    let normalized = path.replace('\\', "/");
    normalized
        .rsplit_once('/')
        .map(|(parent, _)| parent.to_string())
}

fn final_name(path: &str) -> Option<&str> {
    path.rsplit(['/', '\\'])
        .find(|component| !component.is_empty())
}

use std::future::Future;
use std::pin::Pin;

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::core::mutation::MutationKind;
use crate::environment::types::{
    same_environment_identity, EnvironmentRef, ResourceLocator, SkillLocationRef,
};
use crate::error::{AppError, RecoveryResourceId};

pub const RECOVERY_MARKER_SCHEMA_VERSION: u32 = 2;
const LEGACY_RECOVERY_MARKER_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct RecoverySubject {
    pub operation_kind: MutationKind,
    pub skill_name: String,
    pub context: SkillLocationRef,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub enum RecoveryResourcePathKind {
    Current,
    Backup,
    Record,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct RecoveryResourcePath {
    pub kind: RecoveryResourcePathKind,
    pub location: ResourceLocator,
}

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
    #[serde(default)]
    pub subject: Option<RecoverySubject>,
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
    if !matches!(
        marker.schema_version,
        LEGACY_RECOVERY_MARKER_SCHEMA_VERSION | RECOVERY_MARKER_SCHEMA_VERSION
    ) {
        return Err(AppError::ConfigurationCorrupted {
            message: "unsupported recovery marker schema".to_string(),
        });
    }
    if marker.operation_id.is_empty() || marker.unit_id.is_empty() || marker.entries.is_empty() {
        return Err(AppError::ConfigurationCorrupted {
            message: "recovery marker is incomplete".to_string(),
        });
    }
    if marker.schema_version == RECOVERY_MARKER_SCHEMA_VERSION {
        let Some(subject) = &marker.subject else {
            return Err(AppError::ConfigurationCorrupted {
                message: "current recovery marker is missing its subject".to_string(),
            });
        };
        if subject.skill_name.is_empty()
            || !same_environment_identity(&subject.context.environment, &marker.environment)
        {
            return Err(AppError::ConfigurationCorrupted {
                message: "recovery subject does not match its Environment".to_string(),
            });
        }
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

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn legacy_marker_without_subject_remains_readable() {
        let value = json!({
            "schemaVersion": 1,
            "resourceId": "legacy-recovery",
            "kind": "recoveryRequired",
            "environment": { "kind": "host" },
            "operationId": "operation-1",
            "unitId": "update:demo",
            "createdAtEpochMs": 1,
            "entries": [{
                "physicalTargetDigest": "target-1",
                "destination": {
                    "environment": { "kind": "host" },
                    "nativePath": "/work/.agents/skills/demo"
                },
                "backup": {
                    "environment": { "kind": "host" },
                    "nativePath": "/work/.agents/skills/.skill-deck-backup-demo"
                },
                "expectedState": "present",
                "originalFingerprint": "entry-v1-original",
                "phase": "restoreFailed"
            }]
        });

        let marker: RecoveryMarker = serde_json::from_value(value).expect("legacy marker");

        assert!(marker.subject.is_none());
        validate_recovery_marker(&marker).expect("legacy marker remains valid");
    }
}

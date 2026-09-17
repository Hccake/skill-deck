use sha2::{Digest, Sha256};

use crate::environment::recovery::{
    validate_recovery_marker, RecoveryFuture, RecoveryMarker, RecoveryMarkerKind,
    RecoveryMarkerLoad, RecoveryMarkerRef, RecoveryMarkerStore,
};
use crate::environment::types::{EnvironmentRef, ResourceLocator};
use crate::environment::wsl::WslWorkspace;
use crate::error::AppError;

#[derive(Clone)]
pub struct WslRecoveryMarkerStore {
    workspace: WslWorkspace,
}

impl WslRecoveryMarkerStore {
    pub fn new(workspace: WslWorkspace) -> Self {
        Self { workspace }
    }

    fn environment_ref(&self) -> EnvironmentRef {
        EnvironmentRef::Wsl {
            distro_name: self.workspace.distro_name().to_string(),
        }
    }

    fn marker_ref(&self, marker: &RecoveryMarker, managed_root: String) -> RecoveryMarkerRef {
        RecoveryMarkerRef {
            resource_id: marker.resource_id.clone(),
            environment: self.environment_ref(),
            managed_root: ResourceLocator {
                environment: self.environment_ref(),
                native_path: managed_root,
            },
        }
    }

    async fn list(&self) -> Result<environment_protocol::MutationRecoveryList, AppError> {
        self.workspace
            .request_worker_payload(environment_protocol::Message::ListMutationRecovery)
            .await
    }

    fn validate_ref(&self, marker_ref: &RecoveryMarkerRef) -> Result<(), AppError> {
        if marker_ref.environment != self.environment_ref()
            || marker_ref.managed_root.environment != self.environment_ref()
            || marker_ref.managed_root.native_path
                != format!(
                    "/tmp/skill-deck-operation-{}",
                    marker_ref.resource_id.as_str()
                )
        {
            return Err(AppError::StaleTarget);
        }
        Ok(())
    }
}

impl RecoveryMarkerStore for WslRecoveryMarkerStore {
    fn environment(&self) -> EnvironmentRef {
        self.environment_ref()
    }

    fn validate_managed_root(&self, root: &ResourceLocator) -> Result<(), AppError> {
        if root.environment != self.environment_ref()
            || !root.native_path.starts_with("/tmp/skill-deck-operation-")
            || root.native_path.contains("/../")
            || root.native_path.ends_with("/..")
        {
            return Err(AppError::UnsafePath {
                path: root.native_path.clone(),
                reason: "WSL recovery root is outside the managed namespace".to_string(),
            });
        }
        Ok(())
    }

    fn create<'a>(
        &'a self,
        _marker: &'a RecoveryMarker,
    ) -> RecoveryFuture<'a, Result<RecoveryMarkerRef, AppError>> {
        Box::pin(async {
            Err(AppError::CapabilityUnavailable {
                capability: "hostCreatedWslMutationRecovery".to_string(),
                path: None,
            })
        })
    }

    fn update<'a>(
        &'a self,
        _marker_ref: &'a RecoveryMarkerRef,
        _marker: &'a RecoveryMarker,
    ) -> RecoveryFuture<'a, Result<(), AppError>> {
        Box::pin(async {
            Err(AppError::CapabilityUnavailable {
                capability: "hostUpdatedWslMutationRecovery".to_string(),
                path: None,
            })
        })
    }

    fn enumerate<'a>(&'a self) -> RecoveryFuture<'a, Result<Vec<RecoveryMarkerLoad>, AppError>> {
        Box::pin(async move {
            let response = self.list().await?;
            Ok(response
                .records
                .into_iter()
                .map(|record| {
                    let managed_root = ResourceLocator {
                        environment: self.environment_ref(),
                        native_path: record.managed_root.clone(),
                    };
                    match record.state {
                        environment_protocol::MutationRecoveryState::Present => {
                            let parsed =
                                serde_json::from_slice::<RecoveryMarker>(&record.marker_bytes)
                                    .map_err(AppError::from)
                                    .and_then(|marker| {
                                        validate_recovery_marker(&marker)?;
                                        if marker.resource_id.as_str() != record.resource_id
                                            || marker.environment != self.environment_ref()
                                        {
                                            return Err(AppError::StaleTarget);
                                        }
                                        Ok(marker)
                                    });
                            match parsed {
                                Ok(marker) => RecoveryMarkerLoad::Valid {
                                    marker_ref: self.marker_ref(&marker, record.managed_root),
                                    marker,
                                },
                                Err(error) => RecoveryMarkerLoad::Invalid {
                                    managed_root,
                                    error,
                                },
                            }
                        }
                        environment_protocol::MutationRecoveryState::Unreadable => {
                            RecoveryMarkerLoad::Invalid {
                                managed_root,
                                error: AppError::ConfigurationCorrupted {
                                    message: "WSL recovery marker is unreadable".to_string(),
                                },
                            }
                        }
                        environment_protocol::MutationRecoveryState::Unsafe => {
                            RecoveryMarkerLoad::Invalid {
                                managed_root,
                                error: AppError::UnsafePath {
                                    path: record.managed_root,
                                    reason: "WSL recovery root is unsafe".to_string(),
                                },
                            }
                        }
                    }
                })
                .collect())
        })
    }

    fn remove<'a>(
        &'a self,
        marker_ref: &'a RecoveryMarkerRef,
    ) -> RecoveryFuture<'a, Result<(), AppError>> {
        Box::pin(async move {
            self.validate_ref(marker_ref)?;
            let response = self.list().await?;
            let record = response
                .records
                .into_iter()
                .find(|record| record.resource_id == marker_ref.resource_id.as_str())
                .ok_or(AppError::StaleTarget)?;
            let marker: RecoveryMarker = serde_json::from_slice(&record.marker_bytes)?;
            validate_recovery_marker(&marker)?;
            if marker.kind != RecoveryMarkerKind::CleanupOnly {
                return Err(AppError::StaleTarget);
            }
            let cleanup = environment_protocol::MutationCleanupToken {
                resource_id: record.resource_id,
                marker_sha256: format!("sha256:{:x}", Sha256::digest(&record.marker_bytes)),
            };
            let (_, response) = self
                .workspace
                .request_worker_control_once(
                    environment_protocol::Message::AcknowledgeMutationUnit {
                        cleanup: cleanup.clone(),
                    },
                    None,
                    std::time::Duration::from_secs(30),
                )
                .await?;
            match response {
                environment_protocol::Message::MutationAcknowledged { resource_id }
                    if resource_id == cleanup.resource_id =>
                {
                    Ok(())
                }
                _ => Err(AppError::StaleTarget),
            }
        })
    }

    fn cleanup<'a>(
        &'a self,
        marker_ref: &'a RecoveryMarkerRef,
        marker: &'a RecoveryMarker,
    ) -> RecoveryFuture<'a, Result<(), AppError>> {
        Box::pin(async move {
            self.validate_ref(marker_ref)?;
            validate_recovery_marker(marker)?;
            if marker.resource_id != marker_ref.resource_id
                || marker.kind != RecoveryMarkerKind::CleanupOnly
            {
                return Err(AppError::StaleTarget);
            }
            let response = self.list().await?;
            let record = response
                .records
                .into_iter()
                .find(|record| record.resource_id == marker.resource_id.as_str())
                .ok_or(AppError::StaleTarget)?;
            let stored: RecoveryMarker = serde_json::from_slice(&record.marker_bytes)?;
            if stored != *marker {
                return Err(AppError::StaleTarget);
            }
            let backups = marker
                .entries
                .iter()
                .filter_map(|entry| entry.backup.as_ref())
                .map(|backup| backup.native_path.clone())
                .collect();
            let (_, response) = self
                .workspace
                .request_worker_control_once(
                    environment_protocol::Message::CleanupMutationRecovery {
                        resource_id: marker.resource_id.as_str().to_string(),
                        expected_marker_json: record.marker_bytes,
                        backups,
                    },
                    None,
                    std::time::Duration::from_secs(30),
                )
                .await?;
            match response {
                environment_protocol::Message::MutationRecoveryCleaned { resource_id }
                    if resource_id == marker.resource_id.as_str() =>
                {
                    Ok(())
                }
                _ => Err(AppError::StaleTarget),
            }
        })
    }
}

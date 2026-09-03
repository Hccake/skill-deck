use std::time::Duration;

use sha2::{Digest, Sha256};

use crate::environment::types::EnvironmentRef;
use crate::environment::wsl::WslWorkspace;
use crate::error::AppError;

const LIBRARY_DEADLINE_MILLIS: u64 = 60_000;

pub struct LibraryCatalogSnapshot {
    pub generation: u64,
    pub bytes: Option<Vec<u8>>,
    pub revision: Option<String>,
}

impl WslWorkspace {
    pub(crate) async fn read_library_catalog(&self) -> Result<Option<Vec<u8>>, AppError> {
        let response: environment_protocol::LibraryCatalogResponse = self
            .request_worker_payload(environment_protocol::Message::ReadLibraryCatalog {
                deadline_millis: LIBRARY_DEADLINE_MILLIS,
            })
            .await?;
        validate_catalog_response(&response)?;
        Ok(response.present.then_some(response.bytes))
    }

    pub(crate) async fn read_library_catalog_once(
        &self,
    ) -> Result<LibraryCatalogSnapshot, AppError> {
        let (generation, response): (u64, environment_protocol::LibraryCatalogResponse) = self
            .request_worker_payload_once(
                environment_protocol::Message::ReadLibraryCatalog {
                    deadline_millis: LIBRARY_DEADLINE_MILLIS,
                },
                environment_protocol::MAX_RESPONSE_TRANSFER_BYTES,
                None,
                Duration::from_millis(LIBRARY_DEADLINE_MILLIS),
            )
            .await?;
        validate_catalog_response(&response)?;
        Ok(LibraryCatalogSnapshot {
            generation,
            bytes: response.present.then_some(response.bytes),
            revision: response.revision,
        })
    }

    pub(crate) async fn execute_library_operation(
        &self,
        generation: u64,
        request: environment_protocol::LibraryOperationRequest,
    ) -> Result<String, AppError> {
        if request.deadline_millis != LIBRARY_DEADLINE_MILLIS {
            return Err(AppError::Validation {
                field: Some("libraryOperation".to_string()),
                message: "invalid WSL Library operation deadline".to_string(),
            });
        }
        let expected_revision = format!("sha256:{:x}", Sha256::digest(&request.catalog_bytes));
        let payload = environment_protocol::encode_payload(&request).map_err(|error| {
            AppError::ConfigurationCorrupted {
                message: format!("failed to encode WSL Library operation: {error}"),
            }
        })?;
        if payload.is_empty() || payload.len() > environment_protocol::MAX_MUTATION_TRANSFER_BYTES {
            return Err(AppError::CapabilityUnavailable {
                capability: "wslLibraryOperationSize".to_string(),
                path: None,
            });
        }
        let digest = format!("sha256:{:x}", Sha256::digest(&payload));
        let response = self
            .request_worker_control_for_generation(
                generation,
                environment_protocol::Message::PrepareLibraryOperation {
                    request: environment_protocol::LibraryOperationPreparation {
                        total_bytes: payload.len() as u64,
                        sha256: digest,
                        deadline_millis: LIBRARY_DEADLINE_MILLIS,
                    },
                },
                None,
                Duration::from_millis(LIBRARY_DEADLINE_MILLIS),
            )
            .await?;
        let transfer_id = match response {
            environment_protocol::Message::TransferReady { transfer_id } => transfer_id,
            message => return Err(response_error(self.distro_name(), message, "TransferReady")),
        };
        let response = self
            .send_worker_transfer_for_generation(
                generation,
                transfer_id,
                &payload,
                environment_protocol::MAX_MUTATION_TRANSFER_BYTES,
                Duration::from_millis(LIBRARY_DEADLINE_MILLIS),
            )
            .await?;
        match response {
            environment_protocol::Message::LibraryOperationCompleted { catalog_revision }
                if catalog_revision == expected_revision =>
            {
                Ok(catalog_revision)
            }
            message => Err(response_error(
                self.distro_name(),
                message,
                "LibraryOperationCompleted",
            )),
        }
    }
}

fn validate_catalog_response(
    response: &environment_protocol::LibraryCatalogResponse,
) -> Result<(), AppError> {
    if response.present == response.bytes.is_empty()
        || response.present != response.revision.is_some()
    {
        Err(protocol_error("LibraryCatalogResponse"))
    } else {
        Ok(())
    }
}

fn response_error(
    distro_name: &str,
    message: environment_protocol::Message,
    expected: &str,
) -> AppError {
    match message {
        environment_protocol::Message::Error { code, .. } if code == "staleTarget" => {
            AppError::StaleTarget
        }
        environment_protocol::Message::Error { code, .. } if code == "stalePayload" => {
            AppError::StalePayload
        }
        environment_protocol::Message::Error { code, .. } if code == "deadlineExceeded" => {
            AppError::WslCommandTimedOut
        }
        environment_protocol::Message::Error { code, .. }
            if code == "libraryRecoveryIncomplete" =>
        {
            AppError::LibraryRecoveryIncomplete {
                environment: EnvironmentRef::Wsl {
                    distro_name: distro_name.to_string(),
                },
                message: "WSL Skill Library recovery is incomplete".to_string(),
            }
        }
        environment_protocol::Message::Error { code, phase, .. } => AppError::ExecutionFailed {
            message: format!("WSL Library operation failed during {phase}: {code}"),
        },
        _ => protocol_error(expected),
    }
}

fn protocol_error(expected: &str) -> AppError {
    AppError::ConfigurationCorrupted {
        message: format!("WSL Worker returned an invalid {expected}"),
    }
}

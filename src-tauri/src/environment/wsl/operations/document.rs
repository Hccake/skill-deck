use crate::environment::wsl::WslWorkspace;
use crate::error::AppError;
use crate::storage::atomic_document::{DocumentWriteFailure, PublicationState};
use sha2::{Digest, Sha256};
use tokio::time::Duration;

const DOCUMENT_READ_DEADLINE_MILLIS: u64 = 20_000;
const DOCUMENT_WRITE_DEADLINE_MILLIS: u64 = 30_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentQuery {
    pub path: String,
    pub limit: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DocumentState {
    Missing,
    NotFile,
    Unreadable,
    Bytes(Vec<u8>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentFact {
    pub path: String,
    pub state: DocumentState,
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OptionalDocumentSnapshot {
    pub bytes: Option<Vec<u8>>,
    pub revision: Option<String>,
    pub generation: u64,
}

impl WslWorkspace {
    pub(crate) async fn read_documents(
        &self,
        queries: Vec<DocumentQuery>,
        aggregate_limit: u32,
    ) -> Result<Vec<DocumentFact>, AppError> {
        validate_document_queries(&queries, aggregate_limit)?;
        let response: environment_protocol::DocumentReadResponse = self
            .request_worker_payload(document_read_message(&queries, aggregate_limit))
            .await?;
        document_facts(queries, response)
    }

    async fn read_documents_once(
        &self,
        queries: Vec<DocumentQuery>,
        aggregate_limit: u32,
    ) -> Result<(u64, Vec<DocumentFact>), AppError> {
        validate_document_queries(&queries, aggregate_limit)?;
        let (generation, response): (u64, environment_protocol::DocumentReadResponse) = self
            .request_worker_payload_once(
                document_read_message(&queries, aggregate_limit),
                environment_protocol::MAX_RESPONSE_TRANSFER_BYTES,
                None,
                Duration::from_millis(DOCUMENT_READ_DEADLINE_MILLIS),
            )
            .await?;
        Ok((generation, document_facts(queries, response)?))
    }

    pub(crate) async fn read_optional_document(
        &self,
        path: String,
        limit: u32,
    ) -> Result<Option<Vec<u8>>, AppError> {
        let mut facts = self
            .read_documents(
                vec![DocumentQuery {
                    path: path.clone(),
                    limit,
                }],
                limit,
            )
            .await?;
        Ok(optional_snapshot(facts.pop().expect("one document query returns one fact"), 0)?.bytes)
    }

    pub(crate) async fn read_optional_document_snapshot_once(
        &self,
        path: String,
        limit: u32,
    ) -> Result<OptionalDocumentSnapshot, AppError> {
        let (generation, mut facts) = self
            .read_documents_once(
                vec![DocumentQuery {
                    path: path.clone(),
                    limit,
                }],
                limit,
            )
            .await?;
        optional_snapshot(
            facts.pop().expect("one document query returns one fact"),
            generation,
        )
    }

    pub(crate) async fn write_document_atomic(
        &self,
        generation: u64,
        path: String,
        expected_revision: Option<String>,
        bytes: Vec<u8>,
    ) -> Result<String, AppError> {
        self.commit_document_atomic(generation, path, expected_revision, bytes)
            .await
            .map_err(DocumentWriteFailure::into_error)
    }

    pub(crate) async fn commit_document_atomic(
        &self,
        generation: u64,
        path: String,
        expected_revision: Option<String>,
        bytes: Vec<u8>,
    ) -> Result<String, DocumentWriteFailure> {
        if !path.starts_with('/') || bytes.len() > environment_protocol::MAX_DOCUMENT_BYTES as usize
        {
            return Err(DocumentWriteFailure::not_published(AppError::Validation {
                field: Some("documentWrite".to_string()),
                message: "WSL document write requires an absolute path and bounded content"
                    .to_string(),
            }));
        }
        let deadline =
            tokio::time::Instant::now() + Duration::from_millis(DOCUMENT_WRITE_DEADLINE_MILLIS);
        let revision = document_revision(&bytes);
        let response = self
            .request_worker_control_for_generation(
                generation,
                environment_protocol::Message::PrepareDocumentWrite {
                    request: environment_protocol::DocumentWritePreparation {
                        path,
                        expected_revision,
                        total_bytes: bytes.len() as u64,
                        sha256: revision.clone(),
                        deadline_millis: DOCUMENT_WRITE_DEADLINE_MILLIS,
                    },
                },
                None,
                remaining_document_time(deadline)?,
            )
            .await
            .map_err(DocumentWriteFailure::not_published)?;
        let transfer_id = match response {
            environment_protocol::Message::TransferReady { transfer_id } => transfer_id,
            message => {
                return Err(DocumentWriteFailure::not_published(
                    document_write_response_error(message, "TransferReady"),
                ));
            }
        };
        let response = self
            .send_worker_transfer_for_generation(
                generation,
                transfer_id,
                &bytes,
                environment_protocol::MAX_DOCUMENT_BYTES as usize,
                remaining_document_time(deadline)?,
            )
            .await
            .map_err(DocumentWriteFailure::unknown)?;
        match response {
            environment_protocol::Message::DocumentWritten {
                revision: actual_revision,
            } if actual_revision == revision => Ok(revision),
            message => Err(document_write_response_failure(message, "DocumentWritten")),
        }
    }

    pub(crate) async fn remove_document_if_revision(
        &self,
        generation: u64,
        path: String,
        expected_revision: Option<String>,
    ) -> Result<(), DocumentWriteFailure> {
        if !path.starts_with('/') {
            return Err(DocumentWriteFailure::not_published(AppError::Validation {
                field: Some("documentRemove".to_string()),
                message: "WSL document remove requires an absolute path".to_string(),
            }));
        }
        let response = self
            .request_worker_control_for_generation(
                generation,
                environment_protocol::Message::RemoveDocument {
                    request: environment_protocol::DocumentRemoveRequest {
                        path,
                        expected_revision,
                        deadline_millis: DOCUMENT_WRITE_DEADLINE_MILLIS,
                    },
                },
                None,
                Duration::from_millis(DOCUMENT_WRITE_DEADLINE_MILLIS),
            )
            .await
            .map_err(DocumentWriteFailure::unknown)?;
        match response {
            environment_protocol::Message::DocumentRemoved => Ok(()),
            message => Err(document_write_response_failure(message, "DocumentRemoved")),
        }
    }
}

fn validate_document_queries(
    queries: &[DocumentQuery],
    aggregate_limit: u32,
) -> Result<(), AppError> {
    if queries.is_empty()
        || aggregate_limit == 0
        || queries
            .iter()
            .any(|query| !query.path.starts_with('/') || query.limit == 0)
    {
        return Err(AppError::Validation {
            field: Some("documentRead".to_string()),
            message: "WSL document read requires absolute paths and positive limits".to_string(),
        });
    }
    Ok(())
}

fn document_read_message(
    queries: &[DocumentQuery],
    aggregate_limit: u32,
) -> environment_protocol::Message {
    environment_protocol::Message::ReadDocuments {
        request: environment_protocol::DocumentReadRequest {
            queries: queries
                .iter()
                .map(|query| environment_protocol::DocumentReadQuery {
                    path: query.path.clone(),
                    limit: query.limit,
                })
                .collect(),
            aggregate_limit,
            deadline_millis: DOCUMENT_READ_DEADLINE_MILLIS,
        },
    }
}

fn document_facts(
    queries: Vec<DocumentQuery>,
    response: environment_protocol::DocumentReadResponse,
) -> Result<Vec<DocumentFact>, AppError> {
    let expected_paths = queries
        .into_iter()
        .map(|query| query.path)
        .collect::<Vec<_>>();
    if response.facts.len() != expected_paths.len()
        || response
            .facts
            .iter()
            .zip(&expected_paths)
            .any(|(fact, path)| fact.path != *path)
    {
        return Err(AppError::ConfigurationCorrupted {
            message: "invalid WSL Worker document response".to_string(),
        });
    }
    Ok(response
        .facts
        .into_iter()
        .map(|fact| DocumentFact {
            path: fact.path,
            state: match fact.state {
                environment_protocol::DocumentReadState::Missing => DocumentState::Missing,
                environment_protocol::DocumentReadState::NotFile => DocumentState::NotFile,
                environment_protocol::DocumentReadState::Unreadable => DocumentState::Unreadable,
                environment_protocol::DocumentReadState::Bytes(bytes) => {
                    DocumentState::Bytes(bytes)
                }
            },
            truncated: fact.truncated,
        })
        .collect())
}

fn optional_snapshot(
    fact: DocumentFact,
    generation: u64,
) -> Result<OptionalDocumentSnapshot, AppError> {
    let path = fact.path;
    match fact.state {
        DocumentState::Missing => Ok(OptionalDocumentSnapshot {
            bytes: None,
            revision: None,
            generation,
        }),
        DocumentState::Bytes(bytes) if !fact.truncated => {
            let revision = Some(document_revision(&bytes));
            Ok(OptionalDocumentSnapshot {
                bytes: Some(bytes),
                revision,
                generation,
            })
        }
        DocumentState::Bytes(_) => Err(AppError::ExecutionFailed {
            message: format!("document exceeds its read limit: {path}"),
        }),
        DocumentState::NotFile | DocumentState::Unreadable => Err(AppError::Path {
            message: format!("document is not readable: {path}"),
        }),
    }
}

fn document_revision(bytes: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(bytes))
}

fn remaining_document_time(
    deadline: tokio::time::Instant,
) -> Result<Duration, DocumentWriteFailure> {
    deadline
        .checked_duration_since(tokio::time::Instant::now())
        .filter(|remaining| !remaining.is_zero())
        .ok_or_else(|| DocumentWriteFailure::not_published(AppError::WslCommandTimedOut))
}

fn document_write_response_error(
    message: environment_protocol::Message,
    expected: &str,
) -> AppError {
    document_write_response_failure(message, expected).into_error()
}

fn document_write_response_failure(
    message: environment_protocol::Message,
    expected: &str,
) -> DocumentWriteFailure {
    match message {
        environment_protocol::Message::Error { code, phase, .. } if code == "documentConflict" => {
            DocumentWriteFailure::not_published_at(
                document_write_phase(&phase),
                AppError::StaleTarget,
            )
        }
        environment_protocol::Message::Error { code, phase, .. } if code == "deadlineExceeded" => {
            DocumentWriteFailure::not_published_at(
                document_write_phase(&phase),
                AppError::WslCommandTimedOut,
            )
        }
        environment_protocol::Message::Error { code, phase, .. }
            if matches!(
                code.as_str(),
                "invalidRequest" | "invalidTarget" | "invalidTransfer" | "cancelled"
            ) =>
        {
            DocumentWriteFailure::not_published_at(
                document_write_phase(&phase),
                AppError::ExecutionFailed {
                    message: format!("WSL Worker document write failed during {phase}: {code}"),
                },
            )
        }
        environment_protocol::Message::Error { code, phase, .. }
            if code == "documentPublishedUnconfirmed" =>
        {
            DocumentWriteFailure {
                error: AppError::ExecutionFailed {
                    message: format!("WSL Worker document write failed during {phase}: {code}"),
                },
                phase: document_write_phase(&phase),
                publication: PublicationState::PublishedUnconfirmed,
            }
        }
        environment_protocol::Message::Error { code, phase, .. }
            if code == "documentOutcomeUnknown" =>
        {
            DocumentWriteFailure::unknown_at(
                document_write_phase(&phase),
                AppError::ExecutionFailed {
                    message: format!("WSL Worker document write failed during {phase}: {code}"),
                },
            )
        }
        environment_protocol::Message::Error { code, phase, .. } => {
            DocumentWriteFailure::unknown_at(
                document_write_phase(&phase),
                AppError::ExecutionFailed {
                    message: format!("WSL Worker document write failed during {phase}: {code}"),
                },
            )
        }
        _ => DocumentWriteFailure::unknown(AppError::ConfigurationCorrupted {
            message: format!("WSL Worker returned an invalid {expected} response"),
        }),
    }
}

fn document_write_phase(phase: &str) -> crate::storage::atomic_document::WritePhase {
    use crate::storage::atomic_document::WritePhase;

    match phase {
        "writing" => WritePhase::Writing,
        "beforePublish" => WritePhase::BeforePublish,
        "publishing" => WritePhase::Publishing,
        "confirming" => WritePhase::Confirming,
        _ => WritePhase::Preparing,
    }
}

#[cfg(test)]
mod tests {
    use super::document_write_response_failure;
    use crate::error::AppError;
    use crate::storage::atomic_document::{PublicationState, WritePhase};

    #[test]
    fn document_conflict_maps_to_stale_target() {
        let failure = document_write_response_failure(
            environment_protocol::Message::Error {
                code: "documentConflict".to_string(),
                phase: "documentWrite".to_string(),
                parameters: Vec::new(),
            },
            "DocumentWritten",
        );

        assert_eq!(failure.error, AppError::StaleTarget);
        assert_eq!(failure.publication, PublicationState::NotPublished);
    }

    #[test]
    fn worker_publication_codes_retain_their_commit_state() {
        for (code, publication) in [
            (
                "documentPublishedUnconfirmed",
                PublicationState::PublishedUnconfirmed,
            ),
            ("documentOutcomeUnknown", PublicationState::OutcomeUnknown),
        ] {
            let failure = document_write_response_failure(
                environment_protocol::Message::Error {
                    code: code.to_string(),
                    phase: "confirming".to_string(),
                    parameters: Vec::new(),
                },
                "DocumentWritten",
            );

            assert_eq!(failure.publication, publication);
            assert_eq!(failure.phase, WritePhase::Confirming);
        }
    }
}

use std::future::Future;
use std::pin::Pin;

pub use environment_engine::atomic_document::{PublicationState, WritePhase};

use crate::environment::types::ResourceLocator;
use crate::error::AppError;

pub type IoFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Full bytes and the runtime session in which they were observed. A generation
/// is never a persisted document version and must not be written into JSON.
#[derive(Debug, Clone)]
pub struct DocumentSnapshot {
    pub bytes: Option<Vec<u8>>,
    pub generation: Option<u64>,
}

pub type DocumentCommitReceipt = DocumentSnapshot;

#[derive(Debug)]
pub struct DocumentWriteFailure {
    pub error: AppError,
    pub phase: WritePhase,
    pub publication: PublicationState,
}

impl DocumentWriteFailure {
    pub fn not_published(error: AppError) -> Self {
        Self::not_published_at(WritePhase::Preparing, error)
    }

    pub fn not_published_at(phase: WritePhase, error: AppError) -> Self {
        Self {
            error,
            phase,
            publication: PublicationState::NotPublished,
        }
    }

    pub fn unknown(error: AppError) -> Self {
        Self::unknown_at(WritePhase::Publishing, error)
    }

    pub fn unknown_at(phase: WritePhase, error: AppError) -> Self {
        Self {
            error,
            phase,
            publication: PublicationState::OutcomeUnknown,
        }
    }

    pub fn into_error(self) -> AppError {
        self.error
    }

    pub(crate) fn from_engine(
        error: environment_engine::atomic_document::AtomicWriteError,
    ) -> Self {
        let publication = error.publication;
        let phase = error.phase;
        let error = if error.is_conflict() {
            AppError::StaleTarget
        } else {
            AppError::Io {
                message: error.to_string(),
            }
        };
        Self {
            error,
            phase,
            publication,
        }
    }
}

pub trait AtomicDocumentIo: Send + Sync {
    fn observe<'a>(
        &'a self,
        target: &'a ResourceLocator,
        max_bytes: u64,
    ) -> IoFuture<'a, Result<DocumentSnapshot, AppError>>;

    fn replace<'a>(
        &'a self,
        target: &'a ResourceLocator,
        expected: DocumentSnapshot,
        bytes: Vec<u8>,
    ) -> IoFuture<'a, Result<DocumentCommitReceipt, DocumentWriteFailure>>;

    fn remove<'a>(
        &'a self,
        target: &'a ResourceLocator,
        expected: DocumentSnapshot,
    ) -> IoFuture<'a, Result<(), DocumentWriteFailure>>;
}

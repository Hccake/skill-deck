use std::fmt;
use std::path::PathBuf;

use crate::atomic_document::{PublicationState, WritePhase};

#[cfg(target_os = "linux")]
use std::fs;
#[cfg(target_os = "linux")]
use std::io::Read;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentRequest {
    pub queries: Vec<DocumentQuery>,
    pub aggregate_limit: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentQuery {
    pub path: PathBuf,
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
    pub path: PathBuf,
    pub state: DocumentState,
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentResponse {
    pub facts: Vec<DocumentFact>,
    pub total_content_bytes: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocumentError {
    UnsupportedPlatform,
    InvalidRequest,
    Cancelled,
}

impl fmt::Display for DocumentError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedPlatform => formatter.write_str("Linux document read is unavailable"),
            Self::InvalidRequest => formatter.write_str("invalid bounded document read request"),
            Self::Cancelled => formatter.write_str("document read was cancelled"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocumentWriteError {
    UnsupportedPlatform,
    Io {
        phase: WritePhase,
        publication: PublicationState,
    },
    Conflict,
    InvalidTarget,
}

impl fmt::Display for DocumentWriteError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedPlatform => formatter.write_str("Linux document write is unavailable"),
            Self::Io { phase, publication } => {
                write!(
                    formatter,
                    "document write failed ({phase:?}, {publication:?})"
                )
            }
            Self::Conflict => formatter.write_str("document changed since it was read"),
            Self::InvalidTarget => formatter.write_str("document target is not a regular file"),
        }
    }
}

pub fn write_document_atomic(
    path: &std::path::Path,
    expected_revision: Option<&str>,
    bytes: &[u8],
    max_current_bytes: usize,
) -> Result<String, DocumentWriteError> {
    write_document_platform(path, expected_revision, bytes, max_current_bytes)
}

pub fn remove_document_if_revision(
    path: &std::path::Path,
    expected_revision: Option<&str>,
    max_current_bytes: usize,
) -> Result<(), DocumentWriteError> {
    remove_document_platform(path, expected_revision, max_current_bytes)
}

#[cfg(not(target_os = "linux"))]
fn write_document_platform(
    _path: &std::path::Path,
    _expected_revision: Option<&str>,
    _bytes: &[u8],
    _max_current_bytes: usize,
) -> Result<String, DocumentWriteError> {
    Err(DocumentWriteError::UnsupportedPlatform)
}

#[cfg(not(target_os = "linux"))]
fn remove_document_platform(
    _path: &std::path::Path,
    _expected_revision: Option<&str>,
    _max_current_bytes: usize,
) -> Result<(), DocumentWriteError> {
    Err(DocumentWriteError::UnsupportedPlatform)
}

#[cfg(target_os = "linux")]
fn write_document_platform(
    path: &std::path::Path,
    expected_revision: Option<&str>,
    bytes: &[u8],
    max_current_bytes: usize,
) -> Result<String, DocumentWriteError> {
    if !path.is_absolute() || path.file_name().is_none() {
        return Err(DocumentWriteError::InvalidTarget);
    }
    let current = match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
            return Err(DocumentWriteError::InvalidTarget)
        }
        Ok(_) => crate::atomic_document::read_optional_bounded(path, max_current_bytes).map_err(
            |_| DocumentWriteError::Io {
                phase: WritePhase::BeforePublish,
                publication: PublicationState::NotPublished,
            },
        )?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(_) => {
            return Err(DocumentWriteError::Io {
                phase: WritePhase::BeforePublish,
                publication: PublicationState::NotPublished,
            })
        }
    };
    let current_revision = current.as_deref().map(document_revision);
    if current_revision.as_deref() != expected_revision {
        return Err(DocumentWriteError::Conflict);
    }
    let parent = path.parent().ok_or(DocumentWriteError::InvalidTarget)?;
    fs::create_dir_all(parent).map_err(|_| DocumentWriteError::Io {
        phase: WritePhase::Preparing,
        publication: PublicationState::NotPublished,
    })?;
    crate::atomic_document::replace_if_unchanged(path, current.as_deref(), bytes).map_err(
        |error| {
            if error.is_conflict() {
                DocumentWriteError::Conflict
            } else {
                DocumentWriteError::Io {
                    phase: error.phase,
                    publication: error.publication,
                }
            }
        },
    )?;
    Ok(document_revision(bytes))
}

#[cfg(target_os = "linux")]
fn remove_document_platform(
    path: &std::path::Path,
    expected_revision: Option<&str>,
    max_current_bytes: usize,
) -> Result<(), DocumentWriteError> {
    if !path.is_absolute() || path.file_name().is_none() {
        return Err(DocumentWriteError::InvalidTarget);
    }
    let current = match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
            return Err(DocumentWriteError::InvalidTarget)
        }
        Ok(_) => crate::atomic_document::read_optional_bounded(path, max_current_bytes).map_err(
            |_| DocumentWriteError::Io {
                phase: WritePhase::BeforePublish,
                publication: PublicationState::NotPublished,
            },
        )?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(_) => {
            return Err(DocumentWriteError::Io {
                phase: WritePhase::BeforePublish,
                publication: PublicationState::NotPublished,
            })
        }
    };
    if current.as_deref().map(document_revision).as_deref() != expected_revision {
        return Err(DocumentWriteError::Conflict);
    }
    if current.is_none() {
        return Ok(());
    }
    crate::atomic_document::remove_if_unchanged(path, current.as_deref()).map_err(|error| {
        if error.is_conflict() {
            DocumentWriteError::Conflict
        } else {
            DocumentWriteError::Io {
                phase: error.phase,
                publication: error.publication,
            }
        }
    })
}

#[cfg(target_os = "linux")]
fn document_revision(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    format!("sha256:{:x}", Sha256::digest(bytes))
}

impl std::error::Error for DocumentError {}
impl std::error::Error for DocumentWriteError {}

pub fn read_documents(request: &DocumentRequest) -> Result<DocumentResponse, DocumentError> {
    read_documents_with_cancel(request, || false)
}

pub fn read_documents_with_cancel<F>(
    request: &DocumentRequest,
    is_cancelled: F,
) -> Result<DocumentResponse, DocumentError>
where
    F: Fn() -> bool,
{
    if request.queries.is_empty()
        || request.aggregate_limit == 0
        || request
            .queries
            .iter()
            .any(|query| !query.path.is_absolute() || query.limit == 0)
    {
        return Err(DocumentError::InvalidRequest);
    }
    read_platform(request, &is_cancelled)
}

#[cfg(not(target_os = "linux"))]
fn read_platform(
    _request: &DocumentRequest,
    _is_cancelled: &impl Fn() -> bool,
) -> Result<DocumentResponse, DocumentError> {
    Err(DocumentError::UnsupportedPlatform)
}

#[cfg(target_os = "linux")]
fn read_platform(
    request: &DocumentRequest,
    is_cancelled: &impl Fn() -> bool,
) -> Result<DocumentResponse, DocumentError> {
    let mut total_content_bytes = 0usize;
    let mut facts = Vec::with_capacity(request.queries.len());
    for query in &request.queries {
        if is_cancelled() {
            return Err(DocumentError::Cancelled);
        }
        let metadata = match fs::metadata(&query.path) {
            Ok(metadata) if metadata.is_file() => metadata,
            Ok(_) => {
                facts.push(fact(query.path.clone(), DocumentState::NotFile, false));
                continue;
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                facts.push(fact(query.path.clone(), DocumentState::Missing, false));
                continue;
            }
            Err(_) => {
                facts.push(fact(query.path.clone(), DocumentState::Unreadable, false));
                continue;
            }
        };
        let remaining = (request.aggregate_limit as usize).saturating_sub(total_content_bytes);
        let limit = remaining.min(query.limit as usize);
        let mut bytes = Vec::new();
        let state = match fs::File::open(&query.path) {
            Ok(file) => {
                if file.take(limit as u64).read_to_end(&mut bytes).is_ok() {
                    total_content_bytes += bytes.len();
                    DocumentState::Bytes(bytes)
                } else {
                    DocumentState::Unreadable
                }
            }
            Err(_) => DocumentState::Unreadable,
        };
        facts.push(fact(
            query.path.clone(),
            state,
            metadata.len() > limit as u64,
        ));
    }
    Ok(DocumentResponse {
        facts,
        total_content_bytes: total_content_bytes as u32,
    })
}

#[cfg(target_os = "linux")]
fn fact(path: PathBuf, state: DocumentState, truncated: bool) -> DocumentFact {
    DocumentFact {
        path,
        state,
        truncated,
    }
}

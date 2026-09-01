use std::fmt;
use std::path::PathBuf;

#[cfg(target_os = "linux")]
use std::fs;
#[cfg(target_os = "linux")]
use std::io::Read;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathRequest {
    pub queries: Vec<PathQuery>,
    pub aggregate_content_limit: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathQuery {
    pub path: PathBuf,
    pub content_limit: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathKind {
    Missing,
    Directory,
    SymlinkDirectory,
    SymlinkOther,
    Other,
    BrokenLink,
    Inaccessible,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContentState {
    NotRequested,
    Empty,
    Unreadable,
    Bytes(Vec<u8>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathFact {
    pub path: PathBuf,
    pub kind: PathKind,
    pub content: ContentState,
    pub content_truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathResponse {
    pub facts: Vec<PathFact>,
    pub total_content_bytes: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathError {
    UnsupportedPlatform,
    InvalidRequest,
    Cancelled,
}

impl fmt::Display for PathError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedPlatform => {
                formatter.write_str("Linux path inspection is unavailable")
            }
            Self::InvalidRequest => formatter.write_str("invalid bounded path inspection request"),
            Self::Cancelled => formatter.write_str("path inspection was cancelled"),
        }
    }
}

impl std::error::Error for PathError {}

pub fn inspect_paths(request: &PathRequest) -> Result<PathResponse, PathError> {
    inspect_paths_with_cancel(request, || false)
}

pub fn inspect_paths_with_cancel<F>(
    request: &PathRequest,
    is_cancelled: F,
) -> Result<PathResponse, PathError>
where
    F: Fn() -> bool,
{
    if request.queries.is_empty()
        || request.aggregate_content_limit == 0
        || request.queries.iter().any(|query| {
            !query.path.is_absolute() || query.content_limit.is_some_and(|limit| limit == 0)
        })
    {
        return Err(PathError::InvalidRequest);
    }
    inspect_platform(request, &is_cancelled)
}

#[cfg(not(target_os = "linux"))]
fn inspect_platform(
    _request: &PathRequest,
    _is_cancelled: &impl Fn() -> bool,
) -> Result<PathResponse, PathError> {
    Err(PathError::UnsupportedPlatform)
}

#[cfg(target_os = "linux")]
fn inspect_platform(
    request: &PathRequest,
    is_cancelled: &impl Fn() -> bool,
) -> Result<PathResponse, PathError> {
    let mut total_content_bytes = 0usize;
    let mut facts = Vec::with_capacity(request.queries.len());
    for query in &request.queries {
        if is_cancelled() {
            return Err(PathError::Cancelled);
        }
        facts.push(inspect_one(
            query,
            request.aggregate_content_limit as usize,
            &mut total_content_bytes,
        ));
    }
    Ok(PathResponse {
        facts,
        total_content_bytes: total_content_bytes as u32,
    })
}

#[cfg(target_os = "linux")]
fn inspect_one(query: &PathQuery, aggregate_limit: usize, total: &mut usize) -> PathFact {
    let kind = match fs::symlink_metadata(&query.path) {
        Ok(metadata) if metadata.file_type().is_symlink() => match fs::metadata(&query.path) {
            Ok(target) if target.is_dir() => PathKind::SymlinkDirectory,
            Ok(_) => PathKind::SymlinkOther,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => PathKind::BrokenLink,
            Err(_) => PathKind::Inaccessible,
        },
        Ok(metadata) if metadata.is_dir() => PathKind::Directory,
        Ok(_) => PathKind::Other,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => PathKind::Missing,
        Err(_) => PathKind::Inaccessible,
    };
    let Some(per_file_limit) = query.content_limit else {
        return PathFact {
            path: query.path.clone(),
            kind,
            content: ContentState::NotRequested,
            content_truncated: false,
        };
    };
    let metadata = match fs::metadata(&query.path) {
        Ok(metadata) if metadata.is_file() => metadata,
        _ => {
            return PathFact {
                path: query.path.clone(),
                kind,
                content: ContentState::NotRequested,
                content_truncated: false,
            };
        }
    };
    let remaining = aggregate_limit.saturating_sub(*total);
    let limit = remaining.min(per_file_limit as usize);
    let mut bytes = Vec::new();
    let content = match fs::File::open(&query.path) {
        Ok(file) => {
            if file.take(limit as u64).read_to_end(&mut bytes).is_ok() {
                *total += bytes.len();
                if metadata.len() == 0 {
                    ContentState::Empty
                } else {
                    ContentState::Bytes(bytes)
                }
            } else {
                ContentState::Unreadable
            }
        }
        _ => ContentState::Unreadable,
    };
    PathFact {
        path: query.path.clone(),
        kind,
        content,
        content_truncated: metadata.len() > limit as u64,
    }
}

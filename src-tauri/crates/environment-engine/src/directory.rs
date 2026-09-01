use std::fmt;
use std::path::PathBuf;

#[cfg(target_os = "linux")]
use std::fs;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectoryCountRequest {
    pub paths: Vec<PathBuf>,
    pub limit: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectoryCountFact {
    pub path: PathBuf,
    pub observed_count: Option<u32>,
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectoryCountResponse {
    pub facts: Vec<DirectoryCountFact>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectoryListRequest {
    pub path: PathBuf,
    pub limit: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectoryListResponse {
    pub names: Vec<PathBuf>,
    pub truncated: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DirectoryCountError {
    UnsupportedPlatform,
    InvalidRequest,
    Cancelled,
}

impl fmt::Display for DirectoryCountError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedPlatform => {
                formatter.write_str("Linux directory count is unavailable")
            }
            Self::InvalidRequest => formatter.write_str("invalid bounded directory count request"),
            Self::Cancelled => formatter.write_str("directory count was cancelled"),
        }
    }
}

impl std::error::Error for DirectoryCountError {}

pub fn count_entries(
    request: &DirectoryCountRequest,
) -> Result<DirectoryCountResponse, DirectoryCountError> {
    count_entries_with_cancel(request, || false)
}

pub fn list_child_directories(
    request: &DirectoryListRequest,
) -> Result<DirectoryListResponse, DirectoryCountError> {
    list_child_directories_with_cancel(request, || false)
}

pub fn list_child_directories_with_cancel<F>(
    request: &DirectoryListRequest,
    is_cancelled: F,
) -> Result<DirectoryListResponse, DirectoryCountError>
where
    F: Fn() -> bool,
{
    if !request.path.is_absolute() || request.limit == 0 {
        return Err(DirectoryCountError::InvalidRequest);
    }
    list_platform(request, &is_cancelled)
}

#[cfg(not(target_os = "linux"))]
fn list_platform(
    _request: &DirectoryListRequest,
    _is_cancelled: &impl Fn() -> bool,
) -> Result<DirectoryListResponse, DirectoryCountError> {
    Err(DirectoryCountError::UnsupportedPlatform)
}

#[cfg(target_os = "linux")]
fn list_platform(
    request: &DirectoryListRequest,
    is_cancelled: &impl Fn() -> bool,
) -> Result<DirectoryListResponse, DirectoryCountError> {
    let mut names = match fs::read_dir(&request.path) {
        Ok(entries) => entries
            .filter_map(Result::ok)
            .filter(|entry| entry.path().is_dir())
            .map(|entry| PathBuf::from(entry.file_name()))
            .collect::<Vec<_>>(),
        Err(_) => Vec::new(),
    };
    if is_cancelled() {
        return Err(DirectoryCountError::Cancelled);
    }
    names.sort();
    let truncated = names.len() > request.limit as usize;
    names.truncate(request.limit as usize);
    Ok(DirectoryListResponse { names, truncated })
}

pub fn count_entries_with_cancel<F>(
    request: &DirectoryCountRequest,
    is_cancelled: F,
) -> Result<DirectoryCountResponse, DirectoryCountError>
where
    F: Fn() -> bool,
{
    if request.paths.is_empty()
        || request.limit == 0
        || request.paths.iter().any(|path| !path.is_absolute())
    {
        return Err(DirectoryCountError::InvalidRequest);
    }
    count_platform(request, &is_cancelled)
}

#[cfg(not(target_os = "linux"))]
fn count_platform(
    _request: &DirectoryCountRequest,
    _is_cancelled: &impl Fn() -> bool,
) -> Result<DirectoryCountResponse, DirectoryCountError> {
    Err(DirectoryCountError::UnsupportedPlatform)
}

#[cfg(target_os = "linux")]
fn count_platform(
    request: &DirectoryCountRequest,
    is_cancelled: &impl Fn() -> bool,
) -> Result<DirectoryCountResponse, DirectoryCountError> {
    let mut facts = Vec::with_capacity(request.paths.len());
    for path in &request.paths {
        if is_cancelled() {
            return Err(DirectoryCountError::Cancelled);
        }
        let observed = fs::read_dir(path).ok().and_then(|entries| {
            let mut count = 0u32;
            for entry in entries.take(request.limit as usize + 1) {
                if is_cancelled() || entry.is_err() {
                    return None;
                }
                count += 1;
            }
            Some((count.min(request.limit), count > request.limit))
        });
        facts.push(DirectoryCountFact {
            path: path.clone(),
            observed_count: observed.map(|(count, _)| count),
            truncated: observed.is_some_and(|(_, truncated)| truncated),
        });
    }
    Ok(DirectoryCountResponse { facts })
}

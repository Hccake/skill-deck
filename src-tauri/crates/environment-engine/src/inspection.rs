use std::fmt;
use std::path::PathBuf;
#[cfg(target_os = "linux")]
use std::{fs, io::Read, path::Path};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InspectionRequest {
    pub roots: Vec<InspectionRoot>,
    pub per_file_limit: u32,
    pub aggregate_limit: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InspectionRoot {
    pub path: PathBuf,
    pub stat_only: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    Missing,
    File,
    Directory,
    Symlink,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCode {
    PathUnavailable,
    ReadFailed,
    ReadLinkFailed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathFact {
    pub root_index: u32,
    pub relative_path: PathBuf,
    pub kind: EntryKind,
    pub resolved_target: Option<PathBuf>,
    pub content_bytes: Vec<u8>,
    pub truncated: bool,
    pub error_code: Option<ErrorCode>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InspectionSnapshot {
    pub facts: Vec<PathFact>,
    pub total_content_bytes: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InspectionError {
    UnsupportedPlatform,
    InvalidRequest,
    Cancelled,
}

impl fmt::Display for InspectionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedPlatform => formatter.write_str("Linux inspection is unavailable"),
            Self::InvalidRequest => formatter.write_str("invalid bounded inspection request"),
            Self::Cancelled => formatter.write_str("inspection was cancelled"),
        }
    }
}

impl std::error::Error for InspectionError {}

pub fn inspect(request: &InspectionRequest) -> Result<InspectionSnapshot, InspectionError> {
    inspect_with_cancel(request, || false)
}

pub fn inspect_with_cancel<F>(
    request: &InspectionRequest,
    is_cancelled: F,
) -> Result<InspectionSnapshot, InspectionError>
where
    F: Fn() -> bool,
{
    validate(request)?;
    inspect_platform(request, &is_cancelled)
}

fn validate(request: &InspectionRequest) -> Result<(), InspectionError> {
    if request.roots.is_empty()
        || request.per_file_limit == 0
        || request.aggregate_limit == 0
        || request.per_file_limit > request.aggregate_limit
        || request.roots.len() > u32::MAX as usize
    {
        return Err(InspectionError::InvalidRequest);
    }
    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn inspect_platform(
    _request: &InspectionRequest,
    _is_cancelled: &impl Fn() -> bool,
) -> Result<InspectionSnapshot, InspectionError> {
    Err(InspectionError::UnsupportedPlatform)
}

#[cfg(target_os = "linux")]
fn inspect_platform(
    request: &InspectionRequest,
    is_cancelled: &impl Fn() -> bool,
) -> Result<InspectionSnapshot, InspectionError> {
    let mut facts = Vec::new();
    let mut total_content_bytes = 0usize;

    for (root_index, root) in request.roots.iter().enumerate() {
        ensure_not_cancelled(is_cancelled)?;
        let root_fact = inspect_path(
            &root.path,
            root_index as u32,
            PathBuf::new(),
            false,
            request,
            &mut total_content_bytes,
        );
        let root_is_directory = root_fact.kind == EntryKind::Directory;
        facts.push(root_fact);

        if root.stat_only || !root_is_directory {
            continue;
        }

        let entries = match fs::read_dir(&root.path) {
            Ok(entries) => entries,
            Err(_) => {
                if let Some(root_fact) = facts.last_mut() {
                    root_fact.error_code = Some(ErrorCode::PathUnavailable);
                }
                continue;
            }
        };
        let mut children = entries.filter_map(Result::ok).collect::<Vec<_>>();
        children.sort_by_key(fs::DirEntry::file_name);

        for child in children {
            ensure_not_cancelled(is_cancelled)?;
            let relative_path = PathBuf::from(child.file_name());
            let child_path = child.path();
            let child_fact = inspect_path(
                &child_path,
                root_index as u32,
                relative_path.clone(),
                false,
                request,
                &mut total_content_bytes,
            );
            let can_contain_skill =
                matches!(child_fact.kind, EntryKind::Directory | EntryKind::Symlink);
            facts.push(child_fact);

            if can_contain_skill {
                let skill_path = child_path.join("SKILL.md");
                if fs::symlink_metadata(&skill_path).is_ok() {
                    facts.push(inspect_path(
                        &skill_path,
                        root_index as u32,
                        relative_path.join("SKILL.md"),
                        true,
                        request,
                        &mut total_content_bytes,
                    ));
                }
            }
        }
    }

    Ok(InspectionSnapshot {
        facts,
        total_content_bytes: total_content_bytes as u32,
    })
}

#[cfg(target_os = "linux")]
fn ensure_not_cancelled(is_cancelled: &impl Fn() -> bool) -> Result<(), InspectionError> {
    if is_cancelled() {
        Err(InspectionError::Cancelled)
    } else {
        Ok(())
    }
}

#[cfg(target_os = "linux")]
fn inspect_path(
    path: &Path,
    root_index: u32,
    relative_path: PathBuf,
    read_content: bool,
    request: &InspectionRequest,
    total_content_bytes: &mut usize,
) -> PathFact {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return empty_fact(root_index, relative_path, EntryKind::Missing, None);
        }
        Err(_) => {
            return empty_fact(
                root_index,
                relative_path,
                EntryKind::Other,
                Some(ErrorCode::PathUnavailable),
            );
        }
    };
    let file_type = metadata.file_type();
    let kind = if file_type.is_symlink() {
        EntryKind::Symlink
    } else if file_type.is_file() {
        EntryKind::File
    } else if file_type.is_dir() {
        EntryKind::Directory
    } else {
        EntryKind::Other
    };
    let (resolved_target, mut error_code) = if kind == EntryKind::Symlink {
        match fs::read_link(path) {
            Ok(target) => (Some(target), None),
            Err(_) => (None, Some(ErrorCode::ReadLinkFailed)),
        }
    } else {
        (None, None)
    };
    let mut content_bytes = Vec::new();
    let mut truncated = false;

    if kind == EntryKind::File && read_content {
        let remaining = (request.aggregate_limit as usize).saturating_sub(*total_content_bytes);
        let limit = remaining.min(request.per_file_limit as usize);
        match fs::File::open(path) {
            Ok(file) => {
                if file
                    .take(limit as u64)
                    .read_to_end(&mut content_bytes)
                    .is_err()
                {
                    content_bytes.clear();
                    error_code = Some(ErrorCode::ReadFailed);
                } else {
                    truncated = metadata.len() > content_bytes.len() as u64;
                    *total_content_bytes += content_bytes.len();
                }
            }
            Err(_) => error_code = Some(ErrorCode::ReadFailed),
        }
    }

    PathFact {
        root_index,
        relative_path,
        kind,
        resolved_target,
        content_bytes,
        truncated,
        error_code,
    }
}

#[cfg(target_os = "linux")]
fn empty_fact(
    root_index: u32,
    relative_path: PathBuf,
    kind: EntryKind,
    error_code: Option<ErrorCode>,
) -> PathFact {
    PathFact {
        root_index,
        relative_path,
        kind,
        resolved_target: None,
        content_bytes: Vec::new(),
        truncated: false,
        error_code,
    }
}

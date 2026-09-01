use std::fmt;
#[cfg(target_os = "linux")]
use std::path::Path;
use std::path::PathBuf;

#[cfg(target_os = "linux")]
use std::fs;
#[cfg(target_os = "linux")]
use std::io::Read;

#[cfg(target_os = "linux")]
const MAX_RECURSIVE_DEPTH: usize = 6;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceInventoryRequest {
    pub roots: Vec<SourceRoot>,
    pub mode: SourceScanMode,
    pub per_file_limit: u32,
    pub aggregate_limit: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceRoot {
    pub path: PathBuf,
    pub stat_only: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceScanMode {
    Recursive,
    PriorityDirectories,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceEntryKind {
    Missing,
    File,
    Directory,
    Symlink,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceEntryError {
    PathUnavailable,
    ReadFailed,
    ReadLinkFailed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceEntry {
    pub root_index: u32,
    pub relative_path: PathBuf,
    pub kind: SourceEntryKind,
    pub link_target: Option<PathBuf>,
    pub content_bytes: Vec<u8>,
    pub truncated: bool,
    pub error: Option<SourceEntryError>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceInventoryResponse {
    pub entries: Vec<SourceEntry>,
    pub total_content_bytes: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceInventoryError {
    UnsupportedPlatform,
    InvalidRequest,
    Cancelled,
}

impl fmt::Display for SourceInventoryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for SourceInventoryError {}

pub fn scan_source(
    request: &SourceInventoryRequest,
) -> Result<SourceInventoryResponse, SourceInventoryError> {
    scan_source_with_cancel(request, || false)
}

pub fn scan_source_with_cancel<F>(
    request: &SourceInventoryRequest,
    is_cancelled: F,
) -> Result<SourceInventoryResponse, SourceInventoryError>
where
    F: Fn() -> bool,
{
    if request.roots.is_empty()
        || request.roots.len() > u32::MAX as usize
        || request.per_file_limit == 0
        || request.aggregate_limit == 0
        || request.per_file_limit > request.aggregate_limit
        || request.roots.iter().any(|root| !root.path.is_absolute())
    {
        return Err(SourceInventoryError::InvalidRequest);
    }
    scan_platform(request, &is_cancelled)
}

#[cfg(not(target_os = "linux"))]
fn scan_platform(
    _request: &SourceInventoryRequest,
    _is_cancelled: &impl Fn() -> bool,
) -> Result<SourceInventoryResponse, SourceInventoryError> {
    Err(SourceInventoryError::UnsupportedPlatform)
}

#[cfg(target_os = "linux")]
fn scan_platform(
    request: &SourceInventoryRequest,
    is_cancelled: &impl Fn() -> bool,
) -> Result<SourceInventoryResponse, SourceInventoryError> {
    let mut entries = Vec::new();
    let mut total_content_bytes = 0usize;
    for (root_index, root) in request.roots.iter().enumerate() {
        ensure_not_cancelled(is_cancelled)?;
        let read_root_content = root
            .path
            .file_name()
            .is_some_and(|name| name == "skills-lock.json");
        let root_entry = inspect_entry(
            &root.path,
            root_index as u32,
            PathBuf::new(),
            read_root_content,
            request,
            &mut total_content_bytes,
        );
        let root_is_directory = root_entry.kind == SourceEntryKind::Directory;
        entries.push(root_entry);
        if root.stat_only || !root_is_directory {
            continue;
        }
        let visit_result = match request.mode {
            SourceScanMode::Recursive => visit_recursive(
                &root.path,
                Path::new(""),
                1,
                root_index as u32,
                request,
                is_cancelled,
                &mut total_content_bytes,
                &mut entries,
            ),
            SourceScanMode::PriorityDirectories => visit_priority_directories(
                &root.path,
                root_index as u32,
                request,
                is_cancelled,
                &mut total_content_bytes,
                &mut entries,
            ),
        };
        if let Err(error) = visit_result {
            if error == SourceInventoryError::Cancelled {
                return Err(error);
            }
            if let Some(root_entry) = entries.iter_mut().find(|entry| {
                entry.root_index == root_index as u32 && entry.relative_path.as_os_str().is_empty()
            }) {
                root_entry.error = Some(SourceEntryError::PathUnavailable);
            }
        }
    }
    Ok(SourceInventoryResponse {
        entries,
        total_content_bytes: total_content_bytes as u32,
    })
}

#[cfg(target_os = "linux")]
fn visit_priority_directories(
    root: &Path,
    root_index: u32,
    request: &SourceInventoryRequest,
    is_cancelled: &impl Fn() -> bool,
    total_content_bytes: &mut usize,
    entries: &mut Vec<SourceEntry>,
) -> Result<(), SourceInventoryError> {
    let mut directories = read_directory(root)?;
    directories.sort_by_key(fs::DirEntry::file_name);
    for directory in directories {
        ensure_not_cancelled(is_cancelled)?;
        let metadata = match fs::symlink_metadata(directory.path()) {
            Ok(metadata) => metadata,
            Err(_) => continue,
        };
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            continue;
        }
        let mut children = read_directory(&directory.path())?;
        children.sort_by_key(fs::DirEntry::file_name);
        for child in children {
            ensure_not_cancelled(is_cancelled)?;
            let file_name = child.file_name();
            if !file_name
                .to_str()
                .is_some_and(|name| name.eq_ignore_ascii_case("SKILL.md"))
            {
                continue;
            }
            let relative_path = PathBuf::from(directory.file_name()).join(file_name);
            let metadata = match fs::symlink_metadata(child.path()) {
                Ok(metadata) => metadata,
                Err(_) => continue,
            };
            if metadata.is_file() || metadata.file_type().is_symlink() {
                entries.push(inspect_entry(
                    &child.path(),
                    root_index,
                    relative_path,
                    true,
                    request,
                    total_content_bytes,
                ));
            }
        }
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn read_directory(directory: &Path) -> Result<Vec<fs::DirEntry>, SourceInventoryError> {
    fs::read_dir(directory)
        .map_err(|_| SourceInventoryError::InvalidRequest)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| SourceInventoryError::InvalidRequest)
}

#[cfg(target_os = "linux")]
#[allow(
    clippy::too_many_arguments,
    reason = "recursive source scan carries one bounded accumulator"
)]
fn visit_recursive(
    directory: &Path,
    relative_parent: &Path,
    depth: usize,
    root_index: u32,
    request: &SourceInventoryRequest,
    is_cancelled: &impl Fn() -> bool,
    total_content_bytes: &mut usize,
    entries: &mut Vec<SourceEntry>,
) -> Result<(), SourceInventoryError> {
    if depth > MAX_RECURSIVE_DEPTH {
        return Ok(());
    }
    let mut children = fs::read_dir(directory)
        .map_err(|_| SourceInventoryError::InvalidRequest)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| SourceInventoryError::InvalidRequest)?;
    children.sort_by_key(fs::DirEntry::file_name);
    for child in children {
        ensure_not_cancelled(is_cancelled)?;
        let path = child.path();
        let relative_path = relative_parent.join(child.file_name());
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(_) => continue,
        };
        if metadata.is_dir() && !metadata.file_type().is_symlink() {
            if pruned_directory(child.file_name().as_os_str()) {
                continue;
            }
            visit_recursive(
                &path,
                &relative_path,
                depth + 1,
                root_index,
                request,
                is_cancelled,
                total_content_bytes,
                entries,
            )?;
            continue;
        }
        if relevant_document(&relative_path) {
            entries.push(inspect_entry(
                &path,
                root_index,
                relative_path,
                true,
                request,
                total_content_bytes,
            ));
        }
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn relevant_document(relative_path: &Path) -> bool {
    if relative_path == Path::new(".claude-plugin/marketplace.json")
        || relative_path == Path::new(".claude-plugin/plugin.json")
        || relative_path == Path::new("skills-lock.json")
    {
        return true;
    }
    relative_path
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.eq_ignore_ascii_case("SKILL.md"))
}

#[cfg(target_os = "linux")]
fn pruned_directory(name: &std::ffi::OsStr) -> bool {
    matches!(
        name.to_str(),
        Some(".git" | "node_modules" | "dist" | "build" | "__pycache__" | "__pypackages__")
    )
}

#[cfg(target_os = "linux")]
fn inspect_entry(
    path: &Path,
    root_index: u32,
    relative_path: PathBuf,
    read_content: bool,
    request: &SourceInventoryRequest,
    total_content_bytes: &mut usize,
) -> SourceEntry {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return empty_entry(root_index, relative_path, SourceEntryKind::Missing, None);
        }
        Err(_) => {
            return empty_entry(
                root_index,
                relative_path,
                SourceEntryKind::Other,
                Some(SourceEntryError::PathUnavailable),
            );
        }
    };
    let file_type = metadata.file_type();
    let kind = if file_type.is_symlink() {
        SourceEntryKind::Symlink
    } else if metadata.is_file() {
        SourceEntryKind::File
    } else if metadata.is_dir() {
        SourceEntryKind::Directory
    } else {
        SourceEntryKind::Other
    };
    let (link_target, mut error) = if kind == SourceEntryKind::Symlink {
        match fs::read_link(path) {
            Ok(target) => (Some(target), None),
            Err(_) => (None, Some(SourceEntryError::ReadLinkFailed)),
        }
    } else {
        (None, None)
    };
    let mut content_bytes = Vec::new();
    let mut truncated = false;
    let content_length = match kind {
        SourceEntryKind::File => Some(metadata.len()),
        SourceEntryKind::Symlink => fs::metadata(path)
            .ok()
            .filter(fs::Metadata::is_file)
            .map(|target| target.len()),
        _ => None,
    };
    if let Some(content_length) = content_length.filter(|_| read_content) {
        let remaining = (request.aggregate_limit as usize).saturating_sub(*total_content_bytes);
        let limit = remaining.min(request.per_file_limit as usize);
        match fs::File::open(path) {
            Ok(file) => {
                if file
                    .take(limit as u64)
                    .read_to_end(&mut content_bytes)
                    .is_ok()
                {
                    *total_content_bytes += content_bytes.len();
                    truncated = content_length > content_bytes.len() as u64;
                } else {
                    content_bytes.clear();
                    error = Some(SourceEntryError::ReadFailed);
                }
            }
            Err(_) => error = Some(SourceEntryError::ReadFailed),
        }
    }
    SourceEntry {
        root_index,
        relative_path,
        kind,
        link_target,
        content_bytes,
        truncated,
        error,
    }
}

#[cfg(target_os = "linux")]
fn empty_entry(
    root_index: u32,
    relative_path: PathBuf,
    kind: SourceEntryKind,
    error: Option<SourceEntryError>,
) -> SourceEntry {
    SourceEntry {
        root_index,
        relative_path,
        kind,
        link_target: None,
        content_bytes: Vec::new(),
        truncated: false,
        error,
    }
}

#[cfg(target_os = "linux")]
fn ensure_not_cancelled(is_cancelled: &impl Fn() -> bool) -> Result<(), SourceInventoryError> {
    if is_cancelled() {
        Err(SourceInventoryError::Cancelled)
    } else {
        Ok(())
    }
}

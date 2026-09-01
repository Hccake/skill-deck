use std::fmt;
use std::path::PathBuf;

#[cfg(target_os = "linux")]
use std::fs;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryRequest {
    pub paths: Vec<PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    Missing,
    File,
    Directory,
    Symlink,
    BrokenLink,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryMetadata {
    pub device: u64,
    pub inode: u64,
    pub mode: u32,
    pub size: u64,
    pub mtime_seconds: i64,
    pub mtime_nanos: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryFact {
    pub kind: EntryKind,
    pub metadata: Option<EntryMetadata>,
    pub link_target: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryResponse {
    pub facts: Vec<EntryFact>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryError {
    UnsupportedPlatform,
    InvalidRequest,
    Unavailable,
    Cancelled,
}

impl fmt::Display for EntryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for EntryError {}

pub fn inspect_entries(request: &EntryRequest) -> Result<EntryResponse, EntryError> {
    inspect_entries_with_cancel(request, || false)
}

pub fn inspect_entries_with_cancel<F>(
    request: &EntryRequest,
    is_cancelled: F,
) -> Result<EntryResponse, EntryError>
where
    F: Fn() -> bool,
{
    if request.paths.is_empty() || request.paths.iter().any(|path| !path.is_absolute()) {
        return Err(EntryError::InvalidRequest);
    }
    inspect_platform(request, &is_cancelled)
}

#[cfg(not(target_os = "linux"))]
fn inspect_platform(
    _request: &EntryRequest,
    _is_cancelled: &impl Fn() -> bool,
) -> Result<EntryResponse, EntryError> {
    Err(EntryError::UnsupportedPlatform)
}

#[cfg(target_os = "linux")]
fn inspect_platform(
    request: &EntryRequest,
    is_cancelled: &impl Fn() -> bool,
) -> Result<EntryResponse, EntryError> {
    use std::os::unix::fs::MetadataExt;

    let mut facts = Vec::with_capacity(request.paths.len());
    for path in &request.paths {
        if is_cancelled() {
            return Err(EntryError::Cancelled);
        }
        let metadata = match fs::symlink_metadata(path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                facts.push(EntryFact {
                    kind: EntryKind::Missing,
                    metadata: None,
                    link_target: None,
                });
                continue;
            }
            Err(_) => return Err(EntryError::Unavailable),
        };
        let file_type = metadata.file_type();
        let (kind, link_target) = if file_type.is_symlink() {
            let target = fs::read_link(path).map_err(|_| EntryError::Unavailable)?;
            let kind = match fs::metadata(path) {
                Ok(_) => EntryKind::Symlink,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => EntryKind::BrokenLink,
                Err(_) => return Err(EntryError::Unavailable),
            };
            (kind, Some(target))
        } else if metadata.is_dir() {
            (EntryKind::Directory, None)
        } else if metadata.is_file() {
            (EntryKind::File, None)
        } else {
            (EntryKind::Other, None)
        };
        facts.push(EntryFact {
            kind,
            metadata: Some(EntryMetadata {
                device: metadata.dev(),
                inode: metadata.ino(),
                mode: metadata.mode(),
                size: metadata.size(),
                mtime_seconds: metadata.mtime(),
                mtime_nanos: metadata.mtime_nsec(),
            }),
            link_target,
        });
    }
    Ok(EntryResponse { facts })
}

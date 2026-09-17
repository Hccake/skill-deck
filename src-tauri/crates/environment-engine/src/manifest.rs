use std::fmt;
#[cfg(target_os = "linux")]
use std::path::Path;
use std::path::PathBuf;

#[cfg(target_os = "linux")]
use std::fs;
#[cfg(target_os = "linux")]
use std::io::Read;

#[cfg(target_os = "linux")]
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManifestRequest {
    pub root: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManifestKind {
    Directory,
    File,
    Symlink,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManifestRecord {
    pub relative_path: PathBuf,
    pub kind: ManifestKind,
    pub digest: Option<String>,
    pub executable: bool,
    pub symlink_target: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManifestResponse {
    pub records: Vec<ManifestRecord>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManifestError {
    UnsupportedPlatform,
    InvalidRequest,
    Unavailable,
    UnsupportedEntry,
    Cancelled,
}

impl fmt::Display for ManifestError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for ManifestError {}

pub fn build_manifest(request: &ManifestRequest) -> Result<ManifestResponse, ManifestError> {
    build_manifest_with_cancel(request, || false)
}

pub fn build_manifest_with_cancel<F>(
    request: &ManifestRequest,
    is_cancelled: F,
) -> Result<ManifestResponse, ManifestError>
where
    F: Fn() -> bool,
{
    if !request.root.is_absolute() {
        return Err(ManifestError::InvalidRequest);
    }
    build_platform(request, &is_cancelled)
}

#[cfg(not(target_os = "linux"))]
fn build_platform(
    _request: &ManifestRequest,
    _is_cancelled: &impl Fn() -> bool,
) -> Result<ManifestResponse, ManifestError> {
    Err(ManifestError::UnsupportedPlatform)
}

#[cfg(target_os = "linux")]
fn build_platform(
    request: &ManifestRequest,
    is_cancelled: &impl Fn() -> bool,
) -> Result<ManifestResponse, ManifestError> {
    let metadata = fs::symlink_metadata(&request.root).map_err(|_| ManifestError::Unavailable)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(ManifestError::InvalidRequest);
    }
    let mut records = Vec::new();
    visit(&request.root, Path::new(""), is_cancelled, &mut records)?;
    Ok(ManifestResponse { records })
}

#[cfg(target_os = "linux")]
fn visit(
    directory: &Path,
    relative_parent: &Path,
    is_cancelled: &impl Fn() -> bool,
    records: &mut Vec<ManifestRecord>,
) -> Result<(), ManifestError> {
    let mut entries = fs::read_dir(directory)
        .map_err(|_| ManifestError::Unavailable)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| ManifestError::Unavailable)?;
    entries.sort_by_key(fs::DirEntry::file_name);
    for entry in entries {
        if is_cancelled() {
            return Err(ManifestError::Cancelled);
        }
        let relative_path = relative_parent.join(entry.file_name());
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path).map_err(|_| ManifestError::Unavailable)?;
        if metadata.file_type().is_symlink() {
            records.push(ManifestRecord {
                relative_path,
                kind: ManifestKind::Symlink,
                digest: None,
                executable: false,
                symlink_target: Some(fs::read_link(path).map_err(|_| ManifestError::Unavailable)?),
            });
        } else if metadata.is_dir() {
            records.push(ManifestRecord {
                relative_path: relative_path.clone(),
                kind: ManifestKind::Directory,
                digest: None,
                executable: false,
                symlink_target: None,
            });
            visit(&path, &relative_path, is_cancelled, records)?;
        } else if metadata.is_file() {
            use std::os::unix::fs::PermissionsExt;
            records.push(ManifestRecord {
                relative_path,
                kind: ManifestKind::File,
                digest: Some(digest_file(&path)?),
                executable: metadata.permissions().mode() & 0o111 != 0,
                symlink_target: None,
            });
        } else {
            return Err(ManifestError::UnsupportedEntry);
        }
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn digest_file(path: &Path) -> Result<String, ManifestError> {
    let mut file = fs::File::open(path).map_err(|_| ManifestError::Unavailable)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|_| ManifestError::Unavailable)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

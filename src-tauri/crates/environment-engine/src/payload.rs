use std::collections::BTreeSet;
use std::fmt;
use std::fs;
use std::io::{BufReader, Read, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const EXCLUDED_PAYLOAD_FILES: &[&str] = &["metadata.json"];
const EXCLUDED_PAYLOAD_DIRS: &[&str] = &[".git", "__pycache__", "__pypackages__"];
const EXCLUDED_CLI_DIRS: &[&str] = &[".git", "node_modules"];
const MANIFEST_FILE: &str = "manifest.json";
const BLOB_LIST_FILE: &str = "blob-list";
const BLOBS_DIRECTORY: &str = "blobs";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PayloadEntryKind {
    File,
    Directory,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PayloadEntry {
    pub relative_path: String,
    pub kind: PayloadEntryKind,
    pub blob_id: Option<String>,
    pub content_hash: Option<String>,
    pub size: u64,
    pub executable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PayloadManifest {
    pub entries: Vec<PayloadEntry>,
    pub payload_root_hash: String,
    pub payload_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuiltPayload {
    pub manifest: PayloadManifest,
    pub total_bytes: u64,
    pub computed_hash: String,
}

#[derive(Debug)]
pub enum PayloadError {
    Io(std::io::Error),
    Json(serde_json::Error),
    InvalidSource,
    DestinationExists,
    UnsafeSourceLink { path: PathBuf },
    InvalidPayload,
    UnsupportedPlatform,
    Cancelled,
}

impl fmt::Display for PayloadError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for PayloadError {}

impl From<std::io::Error> for PayloadError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<serde_json::Error> for PayloadError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

pub fn build_payload(
    source_root: &Path,
    payload_root: &Path,
) -> Result<BuiltPayload, PayloadError> {
    build_payload_with_cancel(source_root, payload_root, || false)
}

pub fn build_payload_with_cancel<F>(
    source_root: &Path,
    payload_root: &Path,
    is_cancelled: F,
) -> Result<BuiltPayload, PayloadError>
where
    F: Fn() -> bool,
{
    let physical_root = fs::canonicalize(source_root).map_err(|_| PayloadError::InvalidSource)?;
    if !physical_root.is_dir() {
        return Err(PayloadError::InvalidSource);
    }
    if fs::symlink_metadata(payload_root).is_ok() {
        return Err(PayloadError::DestinationExists);
    }
    fs::create_dir(payload_root)?;
    let result = build_payload_in_root(&physical_root, payload_root, &is_cancelled);
    if result.is_err() {
        let _ = fs::remove_dir_all(payload_root);
    }
    result
}

pub fn verify_payload(payload_root: &Path) -> Result<PayloadManifest, PayloadError> {
    ensure_payload_root(payload_root)?;
    let manifest_path = payload_root.join(MANIFEST_FILE);
    ensure_regular_file(&manifest_path)?;
    let manifest: PayloadManifest = serde_json::from_reader(fs::File::open(manifest_path)?)?;
    verify_manifest(&manifest)?;

    let blob_ids = manifest_blob_ids(&manifest)?;
    let blob_list_path = payload_root.join(BLOB_LIST_FILE);
    ensure_regular_file(&blob_list_path)?;
    let blob_list = fs::read_to_string(blob_list_path)?;
    let listed = blob_list
        .lines()
        .map(str::to_owned)
        .collect::<BTreeSet<_>>();
    if listed != blob_ids || blob_list.lines().count() != blob_ids.len() {
        return Err(PayloadError::InvalidPayload);
    }
    for blob_id in blob_ids {
        let blob = payload_root.join(BLOBS_DIRECTORY).join(&blob_id);
        ensure_regular_file(&blob)?;
        if file_sha256(&blob)? != blob_id {
            return Err(PayloadError::InvalidPayload);
        }
    }
    Ok(manifest)
}

pub fn read_blob(payload_root: &Path, blob_id: &str) -> Result<Option<fs::File>, PayloadError> {
    if !valid_blob_id(blob_id) {
        return Err(PayloadError::InvalidPayload);
    }
    ensure_payload_root(payload_root)?;
    let path = payload_root.join(BLOBS_DIRECTORY).join(blob_id);
    match fs::symlink_metadata(&path) {
        Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => {
            if file_sha256(&path)? != blob_id {
                return Err(PayloadError::InvalidPayload);
            }
            Ok(Some(fs::File::open(path)?))
        }
        Ok(_) => Err(PayloadError::InvalidPayload),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

pub fn source_metadata_fingerprint(source_root: &Path) -> Result<String, PayloadError> {
    source_metadata_fingerprint_platform(source_root)
}

#[cfg(not(target_os = "linux"))]
fn source_metadata_fingerprint_platform(_source_root: &Path) -> Result<String, PayloadError> {
    Err(PayloadError::UnsupportedPlatform)
}

#[cfg(target_os = "linux")]
fn source_metadata_fingerprint_platform(source_root: &Path) -> Result<String, PayloadError> {
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::MetadataExt;

    let physical_root = fs::canonicalize(source_root).map_err(|_| PayloadError::InvalidSource)?;
    if !physical_root.is_dir() {
        return Err(PayloadError::InvalidSource);
    }
    let mut paths = Vec::new();
    collect_fingerprint_paths(&physical_root, &physical_root, Path::new(""), &mut paths)?;
    paths.sort_by(|left, right| left.1.cmp(&right.1));

    let mut hasher = Sha256::new();
    for (path, relative) in paths {
        let link_metadata = fs::symlink_metadata(&path)?;
        let (kind, target, metadata) = if link_metadata.file_type().is_symlink() {
            let _ = safe_internal_link_target(&physical_root, &path)?;
            (
                b"link".as_slice(),
                fs::read_link(&path)?.into_os_string().as_bytes().to_vec(),
                fs::metadata(&path).map_err(|_| unsafe_link(&path))?,
            )
        } else if link_metadata.is_dir() {
            (b"directory".as_slice(), Vec::new(), link_metadata)
        } else if link_metadata.is_file() {
            (b"file".as_slice(), Vec::new(), link_metadata)
        } else {
            (b"other".as_slice(), Vec::new(), link_metadata)
        };
        hasher.update(relative.as_os_str().as_bytes());
        hasher.update([0]);
        hasher.update(kind);
        hasher.update([0]);
        hasher.update(metadata.len().to_string().as_bytes());
        hasher.update([0]);
        hasher.update(metadata.mtime().to_string().as_bytes());
        hasher.update([0]);
        hasher.update(format!("{:o}", metadata.mode() & 0o7777).as_bytes());
        hasher.update([0]);
        hasher.update(target);
        hasher.update([0]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

#[cfg(target_os = "linux")]
fn collect_fingerprint_paths(
    physical_root: &Path,
    current: &Path,
    relative_root: &Path,
    paths: &mut Vec<(PathBuf, PathBuf)>,
) -> Result<(), PayloadError> {
    for entry in sorted_directory_entries(current)? {
        let name = entry.file_name();
        let name_text = name.to_string_lossy();
        let path = entry.path();
        let relative = relative_root.join(&name);
        let metadata = fs::symlink_metadata(&path)?;
        if metadata.is_dir() && EXCLUDED_PAYLOAD_DIRS.contains(&name_text.as_ref()) {
            continue;
        }
        if metadata.is_file() && EXCLUDED_PAYLOAD_FILES.contains(&name_text.as_ref()) {
            continue;
        }
        if metadata.file_type().is_symlink() {
            let _ = safe_internal_link_target(physical_root, &path)?;
            paths.push((path, relative));
        } else if metadata.is_dir() {
            paths.push((path.clone(), relative.clone()));
            collect_fingerprint_paths(physical_root, &path, &relative, paths)?;
        } else {
            paths.push((path, relative));
        }
    }
    Ok(())
}

fn build_payload_in_root(
    physical_root: &Path,
    payload_root: &Path,
    is_cancelled: &impl Fn() -> bool,
) -> Result<BuiltPayload, PayloadError> {
    ensure_not_cancelled(is_cancelled)?;
    let blobs_root = payload_root.join(BLOBS_DIRECTORY);
    fs::create_dir(&blobs_root)?;
    let mut entries = Vec::new();
    let mut ancestors = vec![physical_root.to_path_buf()];
    collect_payload_entries(
        physical_root,
        physical_root,
        Path::new(""),
        &blobs_root,
        &mut entries,
        &mut ancestors,
        is_cancelled,
    )?;
    entries.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
    let payload_root_hash = compute_payload_root_hash(&entries);
    let manifest = PayloadManifest {
        entries,
        payload_id: payload_root_hash.clone(),
        payload_root_hash,
    };
    verify_manifest(&manifest)?;
    let blob_ids = manifest_blob_ids(&manifest)?;
    write_blob_list(&payload_root.join(BLOB_LIST_FILE), &blob_ids)?;
    serde_json::to_writer(
        fs::File::create(payload_root.join(MANIFEST_FILE))?,
        &manifest,
    )?;
    let total_bytes = manifest
        .entries
        .iter()
        .filter(|entry| entry.kind == PayloadEntryKind::File)
        .map(|entry| (entry.blob_id.as_deref().unwrap(), entry.size))
        .collect::<std::collections::BTreeMap<_, _>>()
        .values()
        .sum();
    let computed_hash = compute_cli_hash(&manifest, &blobs_root)?;
    Ok(BuiltPayload {
        manifest,
        total_bytes,
        computed_hash,
    })
}

fn collect_payload_entries(
    physical_root: &Path,
    current: &Path,
    relative_root: &Path,
    blobs_root: &Path,
    entries: &mut Vec<PayloadEntry>,
    ancestors: &mut Vec<PathBuf>,
    is_cancelled: &impl Fn() -> bool,
) -> Result<(), PayloadError> {
    for entry in sorted_directory_entries(current)? {
        ensure_not_cancelled(is_cancelled)?;
        let name = entry.file_name();
        let name_text = name.to_string_lossy();
        if EXCLUDED_PAYLOAD_DIRS.contains(&name_text.as_ref()) {
            continue;
        }
        let path = entry.path();
        let relative = relative_root.join(&name);
        let relative_path = normalized_relative_path(&relative)?;
        let metadata = fs::symlink_metadata(&path)?;
        if metadata.file_type().is_symlink() {
            let target = safe_internal_link_target(physical_root, &path)?;
            let target_metadata = fs::metadata(&target).map_err(|_| unsafe_link(&path))?;
            if target_metadata.is_dir() {
                if ancestors.contains(&target) {
                    return Err(unsafe_link(&path));
                }
                entries.push(directory_entry(relative_path));
                ancestors.push(target.clone());
                let result = collect_payload_entries(
                    physical_root,
                    &target,
                    &relative,
                    blobs_root,
                    entries,
                    ancestors,
                    is_cancelled,
                );
                ancestors.pop();
                result?;
            } else if target_metadata.is_file() {
                add_payload_file(
                    &target,
                    relative_path,
                    &target_metadata,
                    blobs_root,
                    entries,
                    is_cancelled,
                )?;
            } else {
                return Err(unsafe_link(&path));
            }
        } else if metadata.is_dir() {
            entries.push(directory_entry(relative_path));
            let canonical = fs::canonicalize(&path).map_err(|_| unsafe_link(&path))?;
            ancestors.push(canonical);
            let result = collect_payload_entries(
                physical_root,
                &path,
                &relative,
                blobs_root,
                entries,
                ancestors,
                is_cancelled,
            );
            ancestors.pop();
            result?;
        } else if metadata.is_file() && !EXCLUDED_PAYLOAD_FILES.contains(&name_text.as_ref()) {
            add_payload_file(
                &path,
                relative_path,
                &metadata,
                blobs_root,
                entries,
                is_cancelled,
            )?;
        }
    }
    Ok(())
}

fn add_payload_file(
    source: &Path,
    relative_path: String,
    metadata: &fs::Metadata,
    blobs_root: &Path,
    entries: &mut Vec<PayloadEntry>,
    is_cancelled: &impl Fn() -> bool,
) -> Result<(), PayloadError> {
    let temporary = blobs_root.join(format!(".copy-{}", entries.len()));
    let mut input = fs::File::open(source)?;
    let mut output = fs::File::create(&temporary)?;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        ensure_not_cancelled(is_cancelled)?;
        let read = input.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        output.write_all(&buffer[..read])?;
    }
    output.flush()?;
    let blob_id = file_sha256(&temporary)?;
    let blob = blobs_root.join(&blob_id);
    if fs::symlink_metadata(&blob).is_ok() {
        fs::remove_file(&temporary)?;
    } else {
        fs::rename(&temporary, &blob)?;
    }
    set_private_permissions(&blob)?;
    let copied_size = fs::metadata(&blob)?.len();
    entries.push(PayloadEntry {
        relative_path,
        kind: PayloadEntryKind::File,
        blob_id: Some(blob_id.clone()),
        content_hash: Some(blob_id),
        size: copied_size,
        executable: is_executable(metadata),
    });
    Ok(())
}

fn verify_manifest(manifest: &PayloadManifest) -> Result<(), PayloadError> {
    if manifest.payload_id != manifest.payload_root_hash
        || compute_payload_root_hash(&manifest.entries) != manifest.payload_root_hash
    {
        return Err(PayloadError::InvalidPayload);
    }
    let mut previous_path = None;
    for entry in &manifest.entries {
        if entry.relative_path.is_empty()
            || entry.relative_path.starts_with('/')
            || entry.relative_path.contains('\\')
            || entry
                .relative_path
                .split('/')
                .any(|component| component.is_empty() || matches!(component, "." | ".."))
            || previous_path.is_some_and(|previous| previous >= entry.relative_path.as_str())
        {
            return Err(PayloadError::InvalidPayload);
        }
        previous_path = Some(entry.relative_path.as_str());
        match entry.kind {
            PayloadEntryKind::Directory
                if entry.blob_id.is_some()
                    || entry.content_hash.is_some()
                    || entry.size != 0
                    || entry.executable =>
            {
                return Err(PayloadError::InvalidPayload);
            }
            PayloadEntryKind::File
                if entry.blob_id.is_none()
                    || entry.blob_id.as_deref() != entry.content_hash.as_deref()
                    || !entry.blob_id.as_deref().is_some_and(valid_blob_id) =>
            {
                return Err(PayloadError::InvalidPayload);
            }
            _ => {}
        }
    }
    Ok(())
}

fn manifest_blob_ids(manifest: &PayloadManifest) -> Result<BTreeSet<String>, PayloadError> {
    verify_manifest(manifest)?;
    Ok(manifest
        .entries
        .iter()
        .filter_map(|entry| entry.blob_id.clone())
        .collect())
}

fn compute_payload_root_hash(entries: &[PayloadEntry]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"skill-deck-payload-v1\0");
    for entry in entries {
        hasher.update(entry.relative_path.as_bytes());
        hasher.update([0]);
        hasher.update([match entry.kind {
            PayloadEntryKind::File => 1,
            PayloadEntryKind::Directory => 2,
        }]);
        hasher.update(entry.size.to_le_bytes());
        hasher.update([u8::from(entry.executable)]);
        if let Some(content_hash) = &entry.content_hash {
            hasher.update(content_hash.as_bytes());
        }
        hasher.update([0]);
    }
    format!("{:x}", hasher.finalize())
}

fn compute_cli_hash(manifest: &PayloadManifest, blobs_root: &Path) -> Result<String, PayloadError> {
    let mut files = manifest
        .entries
        .iter()
        .filter(|entry| entry.kind == PayloadEntryKind::File)
        .filter(|entry| {
            !entry
                .relative_path
                .split('/')
                .any(|component| EXCLUDED_CLI_DIRS.contains(&component))
        })
        .collect::<Vec<_>>();
    files.sort_by(|left, right| {
        left.relative_path
            .to_ascii_lowercase()
            .cmp(&right.relative_path.to_ascii_lowercase())
            .then_with(|| left.relative_path.cmp(&right.relative_path))
    });
    let mut hasher = Sha256::new();
    for entry in files {
        hasher.update(entry.relative_path.as_bytes());
        let blob_id = entry
            .blob_id
            .as_deref()
            .ok_or(PayloadError::InvalidPayload)?;
        hash_file_into(&blobs_root.join(blob_id), &mut hasher)?;
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn file_sha256(path: &Path) -> Result<String, PayloadError> {
    let mut hasher = Sha256::new();
    hash_file_into(path, &mut hasher)?;
    Ok(format!("{:x}", hasher.finalize()))
}

fn hash_file_into(path: &Path, hasher: &mut Sha256) -> Result<(), PayloadError> {
    let mut reader = BufReader::new(fs::File::open(path)?);
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            return Ok(());
        }
        hasher.update(&buffer[..read]);
    }
}

fn write_blob_list(path: &Path, blob_ids: &BTreeSet<String>) -> Result<(), PayloadError> {
    let mut file = fs::File::create(path)?;
    for blob_id in blob_ids {
        writeln!(file, "{blob_id}")?;
    }
    Ok(())
}

fn ensure_payload_root(path: &Path) -> Result<(), PayloadError> {
    let metadata = fs::symlink_metadata(path).map_err(|_| PayloadError::InvalidPayload)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(PayloadError::InvalidPayload);
    }
    let blobs = path.join(BLOBS_DIRECTORY);
    let metadata = fs::symlink_metadata(blobs).map_err(|_| PayloadError::InvalidPayload)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(PayloadError::InvalidPayload);
    }
    Ok(())
}

fn ensure_regular_file(path: &Path) -> Result<(), PayloadError> {
    let metadata = fs::symlink_metadata(path).map_err(|_| PayloadError::InvalidPayload)?;
    if metadata.is_file() && !metadata.file_type().is_symlink() {
        Ok(())
    } else {
        Err(PayloadError::InvalidPayload)
    }
}

fn safe_internal_link_target(physical_root: &Path, link: &Path) -> Result<PathBuf, PayloadError> {
    let target = fs::canonicalize(link).map_err(|_| unsafe_link(link))?;
    if target.starts_with(physical_root) {
        Ok(target)
    } else {
        Err(unsafe_link(link))
    }
}

fn unsafe_link(path: &Path) -> PayloadError {
    PayloadError::UnsafeSourceLink {
        path: path.to_path_buf(),
    }
}

fn directory_entry(relative_path: String) -> PayloadEntry {
    PayloadEntry {
        relative_path,
        kind: PayloadEntryKind::Directory,
        blob_id: None,
        content_hash: None,
        size: 0,
        executable: false,
    }
}

fn sorted_directory_entries(path: &Path) -> Result<Vec<fs::DirEntry>, PayloadError> {
    let mut entries = fs::read_dir(path)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(fs::DirEntry::file_name);
    Ok(entries)
}

fn normalized_relative_path(path: &Path) -> Result<String, PayloadError> {
    path.components()
        .map(|component| {
            component
                .as_os_str()
                .to_str()
                .map(str::to_string)
                .ok_or(PayloadError::InvalidSource)
        })
        .collect::<Result<Vec<_>, _>>()
        .map(|components| components.join("/"))
}

fn valid_blob_id(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

fn ensure_not_cancelled(is_cancelled: &impl Fn() -> bool) -> Result<(), PayloadError> {
    if is_cancelled() {
        Err(PayloadError::Cancelled)
    } else {
        Ok(())
    }
}

#[cfg(unix)]
fn is_executable(metadata: &fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;
    metadata.permissions().mode() & 0o111 != 0
}

#[cfg(not(unix))]
fn is_executable(_metadata: &fs::Metadata) -> bool {
    false
}

#[cfg(unix)]
fn set_private_permissions(path: &Path) -> Result<(), PayloadError> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    Ok(())
}

#[cfg(not(unix))]
fn set_private_permissions(_path: &Path) -> Result<(), PayloadError> {
    Ok(())
}

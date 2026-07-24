use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use unicode_normalization::UnicodeNormalization as UnicodeNormalizationExt;

use crate::environment::types::ResourceLocator;
use crate::error::AppError;

const EXCLUDED_PAYLOAD_FILES: &[&str] = &["metadata.json"];
const EXCLUDED_PAYLOAD_DIRS: &[&str] = &[".git", "__pycache__", "__pypackages__"];
const EXCLUDED_CLI_DIRS: &[&str] = &[".git", "node_modules"];

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

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PayloadId(String);

impl PayloadId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl SkillPayload {
    pub fn manifest(&self) -> SkillPayloadManifest {
        SkillPayloadManifest {
            entries: self.entries.clone(),
            payload_root_hash: self.payload_root_hash.clone(),
            payload_id: self.payload_id.clone(),
        }
    }

    pub(crate) fn restore_verified(
        entries: Vec<PayloadEntry>,
        blobs: BTreeMap<String, Vec<u8>>,
        payload_root_hash: String,
        payload_id: String,
    ) -> Result<Self, AppError> {
        let payload = Self {
            entries,
            blobs,
            payload_root_hash,
            payload_id: PayloadId(payload_id),
        };
        verify_skill_payload_integrity(&payload)?;
        Ok(payload)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillPayload {
    pub entries: Vec<PayloadEntry>,
    pub blobs: BTreeMap<String, Vec<u8>>,
    pub payload_root_hash: String,
    pub payload_id: PayloadId,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillPayloadManifest {
    pub entries: Vec<PayloadEntry>,
    pub payload_root_hash: String,
    pub payload_id: PayloadId,
}

impl SkillPayloadManifest {
    pub fn payload_id(&self) -> &PayloadId {
        &self.payload_id
    }

    pub(crate) fn from_entries(mut entries: Vec<PayloadEntry>) -> Result<Self, AppError> {
        entries.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
        let payload_root_hash = compute_payload_root_hash(&entries);
        let manifest = Self {
            entries,
            payload_id: PayloadId(payload_root_hash.clone()),
            payload_root_hash,
        };
        verify_skill_payload_manifest(&manifest)?;
        Ok(manifest)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnicodeNormalization {
    None,
    Nfc,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetPathProfile {
    pub case_sensitive: bool,
    pub unicode_normalization: UnicodeNormalization,
    pub windows_names: bool,
    pub max_total_path_units: Option<usize>,
}

impl TargetPathProfile {
    pub fn native_unix() -> Self {
        Self {
            case_sensitive: true,
            unicode_normalization: UnicodeNormalization::None,
            windows_names: false,
            max_total_path_units: None,
        }
    }

    pub fn native_windows() -> Self {
        Self {
            case_sensitive: false,
            unicode_normalization: UnicodeNormalization::Nfc,
            windows_names: true,
            max_total_path_units: Some(32_767),
        }
    }
}

pub fn build_skill_payload(root: &Path) -> Result<SkillPayload, AppError> {
    let physical_root = fs::canonicalize(root).map_err(|error| AppError::UnsafePath {
        path: root.to_string_lossy().into_owned(),
        reason: error.to_string(),
    })?;
    if !physical_root.is_dir() {
        return Err(AppError::UnsafePath {
            path: root.to_string_lossy().into_owned(),
            reason: "payload root is not a directory".to_string(),
        });
    }

    let mut entries = Vec::new();
    let mut blobs = BTreeMap::new();
    let mut ancestors = vec![physical_root.clone()];
    collect_payload_entries(
        &physical_root,
        &physical_root,
        Path::new(""),
        &mut entries,
        &mut blobs,
        &mut ancestors,
    )?;
    entries.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));

    let payload_root_hash = compute_payload_root_hash(&entries);
    Ok(SkillPayload {
        entries,
        blobs,
        payload_id: PayloadId(payload_root_hash.clone()),
        payload_root_hash,
    })
}

pub fn verify_skill_payload_integrity(payload: &SkillPayload) -> Result<(), AppError> {
    verify_skill_payload_manifest(&payload.manifest())?;

    let mut previous_path: Option<&str> = None;
    let mut referenced_blobs = std::collections::BTreeSet::new();
    for entry in &payload.entries {
        if previous_path.is_some_and(|previous| previous >= entry.relative_path.as_str()) {
            return Err(AppError::StalePayload);
        }
        previous_path = Some(&entry.relative_path);

        match entry.kind {
            PayloadEntryKind::Directory => {
                if entry.blob_id.is_some()
                    || entry.content_hash.is_some()
                    || entry.size != 0
                    || entry.executable
                {
                    return Err(AppError::StalePayload);
                }
            }
            PayloadEntryKind::File => {
                let blob_id = entry.blob_id.as_deref().ok_or(AppError::StalePayload)?;
                if entry.content_hash.as_deref() != Some(blob_id) {
                    return Err(AppError::StalePayload);
                }
                let blob = payload.blobs.get(blob_id).ok_or(AppError::StalePayload)?;
                if entry.size != blob.len() as u64 || sha256_hex(blob) != blob_id {
                    return Err(AppError::StalePayload);
                }
                referenced_blobs.insert(blob_id);
            }
        }
    }

    if referenced_blobs.len() != payload.blobs.len()
        || payload
            .blobs
            .keys()
            .any(|blob_id| !referenced_blobs.contains(blob_id.as_str()))
    {
        return Err(AppError::StalePayload);
    }
    Ok(())
}

pub fn verify_skill_payload_manifest(manifest: &SkillPayloadManifest) -> Result<(), AppError> {
    if manifest.payload_id.as_str() != manifest.payload_root_hash
        || compute_payload_root_hash(&manifest.entries) != manifest.payload_root_hash
    {
        return Err(AppError::StalePayload);
    }
    let mut previous_path: Option<&str> = None;
    for entry in &manifest.entries {
        if previous_path.is_some_and(|previous| previous >= entry.relative_path.as_str()) {
            return Err(AppError::StalePayload);
        }
        previous_path = Some(&entry.relative_path);
        match entry.kind {
            PayloadEntryKind::Directory
                if entry.blob_id.is_some()
                    || entry.content_hash.is_some()
                    || entry.size != 0
                    || entry.executable =>
            {
                return Err(AppError::StalePayload);
            }
            PayloadEntryKind::File
                if entry.blob_id.is_none()
                    || entry.blob_id.as_deref() != entry.content_hash.as_deref() =>
            {
                return Err(AppError::StalePayload);
            }
            _ => {}
        }
    }
    Ok(())
}

#[cfg(test)]
pub fn compute_cli_project_hash(root: &Path) -> Result<String, AppError> {
    let mut files = Vec::new();
    collect_cli_hash_files(root, root, &mut files)?;
    files.sort_by(|left, right| compare_cli_paths(&left.0, &right.0));

    let mut hasher = Sha256::new();
    for (relative_path, content) in files {
        hasher.update(relative_path.as_bytes());
        hasher.update(content);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

pub fn compute_cli_project_hash_from_payload(payload: &SkillPayload) -> Result<String, AppError> {
    verify_skill_payload_integrity(payload)?;
    let mut files = payload
        .entries
        .iter()
        .filter(|entry| entry.kind == PayloadEntryKind::File)
        .filter(|entry| {
            !entry
                .relative_path
                .split('/')
                .any(|component| EXCLUDED_CLI_DIRS.contains(&component))
        })
        .map(|entry| {
            let blob_id = entry.blob_id.as_deref().ok_or(AppError::StalePayload)?;
            let content = payload.blobs.get(blob_id).ok_or(AppError::StalePayload)?;
            Ok((entry.relative_path.as_str(), content.as_slice()))
        })
        .collect::<Result<Vec<_>, AppError>>()?;
    files.sort_by(|left, right| compare_cli_paths(left.0, right.0));

    let mut hasher = Sha256::new();
    for (relative_path, content) in files {
        hasher.update(relative_path.as_bytes());
        hasher.update(content);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

pub fn validate_manifest_for_target(
    payload: &SkillPayload,
    destination_root: &ResourceLocator,
    profile: &TargetPathProfile,
) -> Result<(), AppError> {
    let mut normalized_entries = BTreeMap::new();
    for entry in &payload.entries {
        validate_relative_manifest_path(&entry.relative_path, profile)?;
        validate_total_path_length(&entry.relative_path, destination_root, profile)?;
        let normalized = normalized_target_path(&entry.relative_path, profile);
        if normalized_entries.insert(normalized, entry).is_some() {
            return Err(unsafe_manifest_path(
                &entry.relative_path,
                "duplicate or target-normalized path collision",
            ));
        }
    }

    for (normalized, entry) in &normalized_entries {
        let mut prefix = String::new();
        let components = normalized.split('/').collect::<Vec<_>>();
        for component in components.iter().take(components.len().saturating_sub(1)) {
            if !prefix.is_empty() {
                prefix.push('/');
            }
            prefix.push_str(component);
            if normalized_entries
                .get(&prefix)
                .is_some_and(|parent| parent.kind == PayloadEntryKind::File)
            {
                return Err(unsafe_manifest_path(
                    &entry.relative_path,
                    "file entry is a prefix of another entry",
                ));
            }
        }
    }

    Ok(())
}

fn validate_relative_manifest_path(
    relative_path: &str,
    profile: &TargetPathProfile,
) -> Result<(), AppError> {
    if relative_path.is_empty()
        || relative_path.starts_with('/')
        || relative_path.starts_with('\\')
        || relative_path.contains('\\')
        || relative_path.as_bytes().get(1) == Some(&b':')
    {
        return Err(unsafe_manifest_path(
            relative_path,
            "path is not relative POSIX form",
        ));
    }

    for component in relative_path.split('/') {
        if component.is_empty() || component == "." || component == ".." {
            return Err(unsafe_manifest_path(
                relative_path,
                "path contains traversal",
            ));
        }
        if profile.windows_names {
            validate_windows_component(relative_path, component)?;
        }
    }
    Ok(())
}

fn validate_windows_component(relative_path: &str, component: &str) -> Result<(), AppError> {
    if component.ends_with([' ', '.'])
        || component.contains(':')
        || component.chars().any(|character| character < '\u{20}')
    {
        return Err(unsafe_manifest_path(
            relative_path,
            "path contains a Windows-unsafe component",
        ));
    }

    let basename = component
        .split('.')
        .next()
        .unwrap_or(component)
        .to_ascii_uppercase();
    let reserved = matches!(basename.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || basename
            .strip_prefix("COM")
            .or_else(|| basename.strip_prefix("LPT"))
            .is_some_and(|suffix| {
                matches!(suffix, "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9")
            });
    if reserved {
        return Err(unsafe_manifest_path(
            relative_path,
            "path contains a Windows reserved name",
        ));
    }
    Ok(())
}

fn validate_total_path_length(
    relative_path: &str,
    destination_root: &ResourceLocator,
    profile: &TargetPathProfile,
) -> Result<(), AppError> {
    let Some(limit) = profile.max_total_path_units else {
        return Ok(());
    };
    let separator = if destination_root.native_path.ends_with(['/', '\\']) {
        ""
    } else {
        "/"
    };
    let full_path = format!(
        "{}{}{}",
        destination_root.native_path, separator, relative_path
    );
    let units = if profile.windows_names {
        full_path.encode_utf16().count()
    } else {
        full_path.chars().count()
    };
    if units > limit {
        return Err(unsafe_manifest_path(
            relative_path,
            "target path exceeds the limit",
        ));
    }
    Ok(())
}

fn normalized_target_path(relative_path: &str, profile: &TargetPathProfile) -> String {
    let normalized = match profile.unicode_normalization {
        UnicodeNormalization::None => relative_path.to_string(),
        UnicodeNormalization::Nfc => relative_path.nfc().collect(),
    };
    if profile.case_sensitive {
        normalized
    } else {
        normalized.to_lowercase()
    }
}

fn unsafe_manifest_path(path: &str, reason: &str) -> AppError {
    AppError::UnsafePath {
        path: path.to_string(),
        reason: reason.to_string(),
    }
}

fn collect_payload_entries(
    physical_root: &Path,
    current: &Path,
    relative_root: &Path,
    entries: &mut Vec<PayloadEntry>,
    blobs: &mut BTreeMap<String, Vec<u8>>,
    ancestors: &mut Vec<std::path::PathBuf>,
) -> Result<(), AppError> {
    for entry in sorted_directory_entries(current)? {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let path = entry.path();
        let relative = relative_root.join(name.as_ref());
        let relative_path = normalized_relative_path(&relative);
        let metadata = fs::symlink_metadata(&path)?;

        if EXCLUDED_PAYLOAD_DIRS.contains(&name.as_ref()) {
            continue;
        }
        if metadata.file_type().is_symlink() {
            let target = safe_internal_link_target(physical_root, &path)?;
            let target_metadata =
                fs::metadata(&target).map_err(|_| AppError::UnsafeSourceLink {
                    path: path.to_string_lossy().into_owned(),
                })?;
            if target_metadata.is_dir() {
                if ancestors.contains(&target) {
                    return Err(AppError::UnsafeSourceLink {
                        path: path.to_string_lossy().into_owned(),
                    });
                }
                entries.push(PayloadEntry {
                    relative_path,
                    kind: PayloadEntryKind::Directory,
                    blob_id: None,
                    content_hash: None,
                    size: 0,
                    executable: false,
                });
                ancestors.push(target.clone());
                let result = collect_payload_entries(
                    physical_root,
                    &target,
                    &relative,
                    entries,
                    blobs,
                    ancestors,
                );
                ancestors.pop();
                result?;
            } else if target_metadata.is_file() {
                add_payload_file(&target, relative_path, &target_metadata, entries, blobs)?;
            } else {
                return Err(AppError::UnsafeSourceLink {
                    path: path.to_string_lossy().into_owned(),
                });
            }
            continue;
        }
        if metadata.is_dir() {
            entries.push(PayloadEntry {
                relative_path,
                kind: PayloadEntryKind::Directory,
                blob_id: None,
                content_hash: None,
                size: 0,
                executable: false,
            });
            let canonical = fs::canonicalize(&path).map_err(|_| AppError::UnsafeSourceLink {
                path: path.to_string_lossy().into_owned(),
            })?;
            ancestors.push(canonical);
            let result =
                collect_payload_entries(physical_root, &path, &relative, entries, blobs, ancestors);
            ancestors.pop();
            result?;
            continue;
        }
        if !metadata.is_file() || EXCLUDED_PAYLOAD_FILES.contains(&name.as_ref()) {
            continue;
        }

        add_payload_file(&path, relative_path, &metadata, entries, blobs)?;
    }

    debug_assert!(current.starts_with(physical_root));
    Ok(())
}

fn safe_internal_link_target(
    physical_root: &Path,
    link: &Path,
) -> Result<std::path::PathBuf, AppError> {
    let target = fs::canonicalize(link).map_err(|_| AppError::UnsafeSourceLink {
        path: link.to_string_lossy().into_owned(),
    })?;
    if !target.starts_with(physical_root) {
        return Err(AppError::UnsafeSourceLink {
            path: link.to_string_lossy().into_owned(),
        });
    }
    Ok(target)
}

fn add_payload_file(
    path: &Path,
    relative_path: String,
    metadata: &fs::Metadata,
    entries: &mut Vec<PayloadEntry>,
    blobs: &mut BTreeMap<String, Vec<u8>>,
) -> Result<(), AppError> {
    let content = fs::read(path)?;
    let content_hash = sha256_hex(&content);
    blobs
        .entry(content_hash.clone())
        .or_insert_with(|| content.clone());
    entries.push(PayloadEntry {
        relative_path,
        kind: PayloadEntryKind::File,
        blob_id: Some(content_hash.clone()),
        content_hash: Some(content_hash),
        size: content.len() as u64,
        executable: is_executable(metadata),
    });
    Ok(())
}

#[cfg(test)]
fn collect_cli_hash_files(
    root: &Path,
    current: &Path,
    files: &mut Vec<(String, Vec<u8>)>,
) -> Result<(), AppError> {
    for entry in sorted_directory_entries(current)? {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if EXCLUDED_CLI_DIRS.contains(&name.as_ref()) {
            continue;
        }

        let path = entry.path();
        let metadata = fs::symlink_metadata(&path)?;
        if metadata.is_dir() {
            collect_cli_hash_files(root, &path, files)?;
        } else if metadata.is_file() {
            let relative = path
                .strip_prefix(root)
                .map_err(|error| AppError::UnsafePath {
                    path: path.to_string_lossy().into_owned(),
                    reason: error.to_string(),
                })?;
            files.push((normalized_relative_path(relative), fs::read(path)?));
        }
    }
    Ok(())
}

fn sorted_directory_entries(path: &Path) -> Result<Vec<fs::DirEntry>, AppError> {
    let mut entries = fs::read_dir(path)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(fs::DirEntry::file_name);
    Ok(entries)
}

fn normalized_relative_path(path: &Path) -> String {
    path.components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

fn compare_cli_paths(left: &str, right: &str) -> std::cmp::Ordering {
    left.to_ascii_lowercase()
        .cmp(&right.to_ascii_lowercase())
        .then_with(|| left.cmp(right))
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

fn sha256_hex(content: &[u8]) -> String {
    format!("{:x}", Sha256::digest(content))
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

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::*;
    use crate::environment::types::{EnvironmentRef, ResourceLocator};

    fn synthetic_payload(paths: &[(&str, PayloadEntryKind)]) -> SkillPayload {
        SkillPayload {
            entries: paths
                .iter()
                .map(|(path, kind)| PayloadEntry {
                    relative_path: (*path).to_string(),
                    kind: *kind,
                    blob_id: (*kind == PayloadEntryKind::File).then(|| "blob".to_string()),
                    content_hash: (*kind == PayloadEntryKind::File).then(|| "content".to_string()),
                    size: u64::from(*kind == PayloadEntryKind::File),
                    executable: false,
                })
                .collect(),
            blobs: BTreeMap::new(),
            payload_root_hash: "payload".to_string(),
            payload_id: PayloadId("payload".to_string()),
        }
    }

    #[test]
    fn payload_contains_full_allowed_tree_and_keeps_hashes_independent() {
        let temp = tempdir().expect("tempdir");
        let root = temp.path().join("demo");
        fs::create_dir_all(root.join("scripts")).expect("scripts");
        fs::create_dir_all(root.join("references")).expect("references");
        fs::create_dir_all(root.join("assets")).expect("assets");
        fs::create_dir_all(root.join("node_modules/pkg")).expect("node_modules");
        fs::create_dir_all(root.join(".git")).expect("git");
        fs::create_dir_all(root.join("__pycache__")).expect("pycache");
        fs::create_dir_all(root.join("__pypackages__")).expect("pypackages");

        fs::write(root.join("SKILL.md"), "---\nname: demo\n---\n").expect("skill");
        fs::write(root.join("scripts/run.sh"), b"#!/bin/sh\necho ok\n").expect("script");
        fs::write(root.join("references/guide.md"), b"guide").expect("reference");
        fs::write(root.join("assets/logo.bin"), [0_u8, 1, 2, 255]).expect("asset");
        fs::write(root.join(".config"), b"enabled=true").expect("dotfile");
        fs::write(root.join("metadata.json"), b"{\"private\":true}").expect("metadata");
        fs::write(root.join("node_modules/pkg/keep.js"), b"keep in payload").expect("node module");
        fs::write(root.join(".git/config"), b"excluded").expect("git config");
        fs::write(root.join("__pycache__/cache.pyc"), b"excluded").expect("pycache file");
        fs::write(root.join("__pypackages__/pkg.py"), b"excluded").expect("pypackage file");

        let payload = build_skill_payload(&root).expect("build payload");
        let paths = payload
            .entries
            .iter()
            .map(|entry| entry.relative_path.as_str())
            .collect::<Vec<_>>();

        assert!(paths.contains(&"SKILL.md"));
        assert!(paths.contains(&"scripts/run.sh"));
        assert!(paths.contains(&"references/guide.md"));
        assert!(paths.contains(&"assets/logo.bin"));
        assert!(paths.contains(&".config"));
        assert!(paths.contains(&"node_modules/pkg/keep.js"));
        assert!(!paths.iter().any(|path| path.contains("metadata.json")));
        assert!(!paths.iter().any(|path| path.starts_with(".git")));
        assert!(!paths.iter().any(|path| path.starts_with("__pycache__")));
        assert!(!paths.iter().any(|path| path.starts_with("__pypackages__")));
        assert_eq!(payload.payload_root_hash.len(), 64);
        assert_eq!(payload.payload_id.as_str(), payload.payload_root_hash);

        let cli_hash = compute_cli_project_hash(&root).expect("CLI hash");
        assert_ne!(payload.payload_root_hash, cli_hash);

        let before = payload.payload_root_hash;
        fs::write(
            root.join("node_modules/pkg/keep.js"),
            b"changed payload only",
        )
        .expect("change node module");
        let changed = build_skill_payload(&root).expect("changed payload");
        assert_ne!(changed.payload_root_hash, before);
        assert_eq!(
            compute_cli_project_hash(&root).expect("unchanged CLI hash"),
            cli_hash
        );
    }

    #[test]
    fn manifest_rebuilt_from_backend_entries_matches_native_payload_identity() {
        let temp = tempdir().expect("tempdir");
        let root = temp.path().join("demo");
        fs::create_dir_all(root.join("scripts")).expect("scripts");
        fs::write(root.join("SKILL.md"), b"skill").expect("skill");
        fs::write(root.join("scripts/run.sh"), b"#!/bin/sh\n").expect("script");
        let payload = build_skill_payload(&root).expect("payload");

        let rebuilt = SkillPayloadManifest::from_entries(payload.entries.clone())
            .expect("manifest from entries");

        assert_eq!(rebuilt, payload.manifest());
    }

    #[test]
    fn cli_project_hash_can_be_rebuilt_from_the_canonical_payload() {
        let temp = tempdir().expect("tempdir");
        let root = temp.path().join("demo");
        fs::create_dir_all(root.join("scripts")).expect("scripts");
        fs::create_dir_all(root.join("node_modules/pkg")).expect("node_modules");
        fs::write(root.join("SKILL.md"), b"skill").expect("skill");
        fs::write(root.join("scripts/run.sh"), b"#!/bin/sh\n").expect("script");
        fs::write(root.join("node_modules/pkg/index.js"), b"ignored").expect("dependency");

        let payload = build_skill_payload(&root).expect("payload");
        assert_eq!(
            compute_cli_project_hash_from_payload(&payload).expect("payload CLI hash"),
            compute_cli_project_hash(&root).expect("filesystem CLI hash")
        );

        let before = compute_cli_project_hash_from_payload(&payload).expect("before");
        fs::write(root.join("node_modules/pkg/index.js"), b"changed").expect("change dependency");
        let changed_payload = build_skill_payload(&root).expect("changed payload");
        assert_ne!(changed_payload.payload_root_hash, payload.payload_root_hash);
        assert_eq!(
            compute_cli_project_hash_from_payload(&changed_payload).expect("after"),
            before
        );
    }

    #[test]
    fn committed_fixture_has_fixed_payload_and_cli_hash_vectors() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/payload/demo");
        let payload = build_skill_payload(&root).expect("fixture payload");

        assert_eq!(
            compute_cli_project_hash(&root).expect("fixture CLI hash"),
            "05e752629100fb12fd9bf4197908b5fb7dd3feeadc7ac50e4a999fc9ad3ee418"
        );
        assert_eq!(
            payload.payload_root_hash,
            if cfg!(unix) {
                "1e970bd3d1b2da10d37000f4bc5e3964eed67957b14b663f50a3e50794c13bdd"
            } else {
                "1f426a8f630882dae9008ce8ccddf8d945276cb99953e43e5f249222ba2c316c"
            }
        );
    }

    #[cfg(unix)]
    #[test]
    fn internal_links_are_dereferenced_into_regular_payload_entries() {
        use std::os::unix::fs::symlink;

        let temp = tempdir().expect("tempdir");
        let root = temp.path().join("demo");
        fs::create_dir_all(root.join("real")).expect("real directory");
        fs::write(root.join("SKILL.md"), b"skill").expect("skill");
        fs::write(root.join("real/tool.sh"), b"#!/bin/sh\n").expect("tool");
        symlink("real", root.join("alias-dir")).expect("directory link");
        symlink("real/tool.sh", root.join("alias-file.sh")).expect("file link");

        let payload = build_skill_payload(&root).expect("payload with internal links");
        let paths = payload
            .entries
            .iter()
            .map(|entry| (entry.relative_path.as_str(), entry.kind))
            .collect::<Vec<_>>();

        assert!(paths.contains(&("alias-dir", PayloadEntryKind::Directory)));
        assert!(paths.contains(&("alias-dir/tool.sh", PayloadEntryKind::File)));
        assert!(paths.contains(&("alias-file.sh", PayloadEntryKind::File)));
        assert_eq!(
            payload
                .entries
                .iter()
                .filter(|entry| entry.relative_path.ends_with("tool.sh"))
                .filter_map(|entry| entry.blob_id.as_deref())
                .collect::<std::collections::BTreeSet<_>>()
                .len(),
            1,
            "dereferenced copies should share one content-addressed blob"
        );
    }

    #[cfg(unix)]
    #[test]
    fn unsafe_links_are_rejected_before_payload_creation() {
        use std::os::unix::fs::symlink;

        let temp = tempdir().expect("tempdir");
        let root = temp.path().join("demo");
        fs::create_dir_all(&root).expect("root");
        fs::write(root.join("SKILL.md"), b"skill").expect("skill");
        fs::write(temp.path().join("outside.txt"), b"outside").expect("outside");

        for (name, target) in [
            ("external", "../outside.txt"),
            ("dangling", "missing.txt"),
            ("circular", "."),
        ] {
            let link = root.join(name);
            symlink(target, &link).expect("create unsafe link");
            let error = build_skill_payload(&root).expect_err("unsafe link must fail");
            assert!(matches!(error, AppError::UnsafeSourceLink { .. }));
            fs::remove_file(link).expect("remove link");
        }
    }

    #[test]
    fn manifest_rejects_traversal_duplicates_and_file_prefix_conflicts() {
        let destination = ResourceLocator {
            environment: EnvironmentRef::Host,
            native_path: "/tmp/skills/demo".to_string(),
        };
        let profile = TargetPathProfile::native_unix();

        for payload in [
            synthetic_payload(&[("../escape", PayloadEntryKind::File)]),
            synthetic_payload(&[
                ("same", PayloadEntryKind::File),
                ("same", PayloadEntryKind::File),
            ]),
            synthetic_payload(&[
                ("file", PayloadEntryKind::File),
                ("file/child", PayloadEntryKind::File),
            ]),
        ] {
            assert!(matches!(
                validate_manifest_for_target(&payload, &destination, &profile),
                Err(AppError::UnsafePath { .. })
            ));
        }
    }

    #[test]
    fn target_profile_rejects_case_unicode_and_windows_name_collisions() {
        let destination = ResourceLocator {
            environment: EnvironmentRef::Host,
            native_path: "C:\\Users\\alice\\skills\\demo".to_string(),
        };
        let windows = TargetPathProfile::native_windows();

        for payload in [
            synthetic_payload(&[
                ("Foo.md", PayloadEntryKind::File),
                ("foo.md", PayloadEntryKind::File),
            ]),
            synthetic_payload(&[
                ("caf\u{e9}.md", PayloadEntryKind::File),
                ("cafe\u{301}.md", PayloadEntryKind::File),
            ]),
            synthetic_payload(&[("CON/readme.md", PayloadEntryKind::File)]),
            synthetic_payload(&[("name:stream", PayloadEntryKind::File)]),
            synthetic_payload(&[("trailing. ", PayloadEntryKind::File)]),
        ] {
            assert!(matches!(
                validate_manifest_for_target(&payload, &destination, &windows),
                Err(AppError::UnsafePath { .. })
            ));
        }
    }

    #[test]
    fn target_length_limit_includes_destination_root() {
        let payload = synthetic_payload(&[("nested/file.md", PayloadEntryKind::File)]);
        let destination = ResourceLocator {
            environment: EnvironmentRef::Host,
            native_path: "/already/long/root".to_string(),
        };
        let profile = TargetPathProfile {
            case_sensitive: true,
            unicode_normalization: UnicodeNormalization::None,
            windows_names: false,
            max_total_path_units: Some(destination.native_path.len() + "nested/file.md".len()),
        };

        assert!(matches!(
            validate_manifest_for_target(&payload, &destination, &profile),
            Err(AppError::UnsafePath { .. })
        ));
    }
}

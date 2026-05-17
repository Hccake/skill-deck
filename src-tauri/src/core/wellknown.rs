//! Well-Known Skills protocol support (RFC 8615 `.well-known/agent-skills`).
//!
//! Implements discovery of skills hosted under the `.well-known/agent-skills/`
//! path on websites, with fallback to the legacy `.well-known/skills/` path.
//! This is the Rust equivalent of the CLI's `providers/wellknown.ts`.

use crate::error::AppError;
use reqwest::Client;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs;
use std::io::{Cursor, Read};
use std::path::PathBuf;
use std::path::{Component, Path};
use std::time::Duration;
use url::Url;

const DISCOVERY_SCHEMA_V2: &str = "https://schemas.agentskills.io/discovery/0.2.0/schema.json";
const MAX_ARCHIVE_UNPACKED_BYTES: u64 = 50 * 1024 * 1024;
const MAX_ARCHIVE_FILES: usize = 1000;
const WELL_KNOWN_PATHS: &[&str] = &[".well-known/agent-skills", ".well-known/skills"];
const INDEX_FILE: &str = "index.json";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

// ---------------------------------------------------------------------------
// Data structures
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub struct WellKnownIndex {
    pub skills: Vec<WellKnownSkillEntry>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WellKnownSkillEntry {
    pub name: String,
    pub description: String,
    pub files: Vec<String>,
}

#[derive(Debug, Clone)]
enum NormalizedWellKnownEntry {
    Legacy {
        name: String,
        #[allow(dead_code)]
        description: String,
        files: Vec<String>,
        base_url: String,
        #[allow(dead_code)]
        well_known_path: String,
    },
    V2 {
        name: String,
        #[allow(dead_code)]
        description: String,
        entry_type: String,
        artifact_url: String,
        digest: String,
    },
}

impl NormalizedWellKnownEntry {
    fn name(&self) -> &str {
        match self {
            Self::Legacy { name, .. } | Self::V2 { name, .. } => name,
        }
    }

    fn trust_metadata(&self) -> WellKnownTrustMetadata {
        match self {
            Self::Legacy { .. } => WellKnownTrustMetadata {
                well_known_version: Some("0.1.0".to_string()),
                well_known_entry_type: Some("legacy".to_string()),
                artifact_url_host: None,
                digest_verified: None,
                trust_reason: Some("legacy".to_string()),
            },
            Self::V2 {
                entry_type,
                artifact_url,
                ..
            } => WellKnownTrustMetadata {
                well_known_version: Some("0.2.0".to_string()),
                well_known_entry_type: Some(entry_type.clone()),
                artifact_url_host: extract_hostname(artifact_url),
                digest_verified: Some(true),
                trust_reason: Some("digest-verified".to_string()),
            },
        }
    }
}

/// Result of a successful well-known skill fetch — carries the local path
/// where files were downloaded and an identifier suitable for lock-file storage.
#[derive(Debug, Clone)]
pub struct WellKnownFetchResult {
    pub repo_path: PathBuf,
    #[allow(dead_code)]
    pub source_identifier: String,
    pub trust_metadata: HashMap<String, WellKnownTrustMetadata>,
}

#[derive(Debug, Clone, Default)]
pub struct WellKnownTrustMetadata {
    pub well_known_version: Option<String>,
    pub well_known_entry_type: Option<String>,
    pub artifact_url_host: Option<String>,
    pub digest_verified: Option<bool>,
    pub trust_reason: Option<String>,
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

struct IndexUrlCandidate {
    index_url: String,
    #[allow(dead_code)]
    base_url: String,
    #[allow(dead_code)]
    well_known_path: String,
}

/// Extract the hostname from a URL, stripping a leading `www.` prefix.
pub fn extract_hostname(url: &str) -> Option<String> {
    let parsed = Url::parse(url).ok()?;
    let host = parsed.host_str()?;
    let stripped = host.strip_prefix("www.").unwrap_or(host);
    Some(stripped.to_string())
}

/// Build candidate index URLs for a given page URL.
///
/// For each well-known path (`agent-skills` first, then legacy `skills`), and
/// for a URL with a non-trivial path (e.g. `https://example.com/docs`), we
/// generate:
///   1. Path-relative: `https://example.com/docs/.well-known/<wk>/index.json`
///   2. Root fallback: `https://example.com/.well-known/<wk>/index.json`
///
/// For a root URL (`https://example.com` or `https://example.com/`) only the
/// root candidate is generated per well-known path.
///
/// The resulting list is ordered so that `agent-skills` candidates are tried
/// before `skills` candidates (new path preferred, legacy as fallback).
fn build_index_urls(url: &str) -> Vec<IndexUrlCandidate> {
    let Ok(parsed) = Url::parse(url) else {
        return vec![];
    };

    let origin = parsed.origin().ascii_serialization();
    let path = parsed.path().trim_end_matches('/');

    let mut candidates = Vec::new();

    for &wk_path in WELL_KNOWN_PATHS {
        if !path.is_empty() {
            candidates.push(IndexUrlCandidate {
                index_url: format!("{origin}{path}/{wk_path}/{INDEX_FILE}"),
                base_url: format!("{origin}{path}/{wk_path}"),
                well_known_path: wk_path.to_string(),
            });
        }

        candidates.push(IndexUrlCandidate {
            index_url: format!("{origin}/{wk_path}/{INDEX_FILE}"),
            base_url: format!("{origin}/{wk_path}"),
            well_known_path: wk_path.to_string(),
        });
    }

    candidates
}

fn is_valid_skill_name(name: &str) -> bool {
    if name.is_empty() || name.len() > 64 {
        return false;
    }
    if name.starts_with('-') || name.ends_with('-') || name.contains("--") {
        return false;
    }
    name.chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

fn validate_skill_name(name: &str) -> Result<(), AppError> {
    if name.trim().is_empty() {
        return Err(AppError::InvalidSource {
            value: "Skill name must not be empty".into(),
        });
    }
    if !is_valid_skill_name(name) {
        return Err(AppError::InvalidSource {
            value: format!("Invalid skill name: {name}"),
        });
    }
    Ok(())
}

/// Validate a single skill entry from the index.
fn validate_skill_entry(entry: &WellKnownSkillEntry) -> Result<(), AppError> {
    validate_skill_name(&entry.name)?;
    if entry.description.trim().is_empty() {
        return Err(AppError::InvalidSource {
            value: "Skill description must not be empty".into(),
        });
    }

    let has_skill_md = entry
        .files
        .iter()
        .any(|f| f.eq_ignore_ascii_case("SKILL.md"));
    if !has_skill_md {
        return Err(AppError::InvalidSource {
            value: format!(
                "Skill '{}' must include SKILL.md in its files list",
                entry.name
            ),
        });
    }

    for file in &entry.files {
        if file.trim().is_empty() {
            return Err(AppError::InvalidSource {
                value: "Empty filename not allowed".into(),
            });
        }
        if file.contains('\0') {
            return Err(AppError::InvalidSource {
                value: format!("Null byte not allowed: {file:?}"),
            });
        }
        if file.starts_with('/') || file.starts_with('\\') {
            return Err(AppError::InvalidSource {
                value: format!("Absolute path not allowed: {file}"),
            });
        }
        let normalized = file.replace('\\', "/");
        if normalized.contains("..") {
            return Err(AppError::InvalidSource {
                value: format!("Path traversal not allowed: {file}"),
            });
        }
    }

    Ok(())
}

fn normalize_wellknown_index(
    raw: &serde_json::Value,
    index_url: &str,
    well_known_path: &str,
) -> Option<Vec<NormalizedWellKnownEntry>> {
    let object = raw.as_object()?;
    let skills = object.get("skills")?.as_array()?;
    let schema = object.get("$schema").and_then(|value| value.as_str());

    if schema == Some(DISCOVERY_SCHEMA_V2) {
        let mut entries = Vec::new();
        for value in skills {
            let object = value.as_object()?;
            let name = object.get("name")?.as_str()?.to_string();
            let description = object.get("description")?.as_str()?.to_string();
            let entry_type = object.get("type")?.as_str()?.to_string();
            let url = object.get("url")?.as_str()?;
            let digest = object.get("digest")?.as_str()?.to_string();

            if validate_skill_name(&name).is_err()
                || description.trim().is_empty()
                || description.len() > 1024
                || !matches!(entry_type.as_str(), "skill-md" | "archive")
                || !is_valid_sha256_digest(&digest)
            {
                continue;
            }

            let artifact_url = Url::parse(index_url).ok()?.join(url).ok()?.to_string();
            entries.push(NormalizedWellKnownEntry::V2 {
                name,
                description,
                entry_type,
                artifact_url,
                digest,
            });
        }
        return (!entries.is_empty()).then_some(entries);
    }

    if schema.is_some() {
        return None;
    }

    let base_url = index_url
        .trim_end_matches(&format!("/{INDEX_FILE}"))
        .to_string();
    let mut entries = Vec::new();
    for value in skills {
        let entry: WellKnownSkillEntry = serde_json::from_value(value.clone()).ok()?;
        if validate_skill_entry(&entry).is_err() {
            return None;
        }
        entries.push(NormalizedWellKnownEntry::Legacy {
            name: entry.name,
            description: entry.description,
            files: entry.files,
            base_url: base_url.clone(),
            well_known_path: well_known_path.to_string(),
        });
    }

    (!entries.is_empty()).then_some(entries)
}

fn is_valid_sha256_digest(value: &str) -> bool {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return false;
    };
    hex.len() == 64
        && hex
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn compute_digest(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("sha256:{:x}", hasher.finalize())
}

fn verify_artifact_digest(bytes: &[u8], expected: &str) -> Result<(), AppError> {
    if !is_valid_sha256_digest(expected) {
        return Err(AppError::InvalidSource {
            value: format!("Invalid digest format: {expected}"),
        });
    }
    let actual = compute_digest(bytes);
    if actual != expected {
        return Err(AppError::InvalidSource {
            value: format!("digest-mismatch: expected {expected}, got {actual}"),
        });
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WellKnownArchiveFormat {
    Zip,
    TarGz,
}

#[derive(Debug, Default)]
struct ArchiveExtractionState {
    files: usize,
    unpacked_bytes: u64,
}

impl ArchiveExtractionState {
    fn track_file(&mut self, bytes: u64) -> Result<(), AppError> {
        if self.files >= MAX_ARCHIVE_FILES {
            return Err(AppError::InvalidSource {
                value: "archive-too-many-files: archive contains too many files".to_string(),
            });
        }
        self.unpacked_bytes =
            self.unpacked_bytes
                .checked_add(bytes)
                .ok_or_else(|| AppError::InvalidSource {
                    value: "archive-too-large: archive exceeds maximum unpacked size".to_string(),
                })?;
        if self.unpacked_bytes > MAX_ARCHIVE_UNPACKED_BYTES {
            return Err(AppError::InvalidSource {
                value: "archive-too-large: archive exceeds maximum unpacked size".to_string(),
            });
        }
        self.files += 1;
        Ok(())
    }
}

fn detect_archive_format(
    bytes: &[u8],
    artifact_url: &str,
    content_type: &str,
) -> Option<WellKnownArchiveFormat> {
    let lower = artifact_url.to_ascii_lowercase();
    if content_type.contains("application/zip")
        || lower.ends_with(".zip")
        || bytes.starts_with(&[0x50, 0x4b])
    {
        return Some(WellKnownArchiveFormat::Zip);
    }
    if content_type.contains("application/gzip")
        || content_type.contains("application/x-gzip")
        || lower.ends_with(".tar.gz")
        || lower.ends_with(".tgz")
        || bytes.starts_with(&[0x1f, 0x8b])
    {
        return Some(WellKnownArchiveFormat::TarGz);
    }
    None
}

fn normalize_archive_path(raw_path: &str) -> Result<PathBuf, AppError> {
    if raw_path.trim().is_empty() || raw_path.contains('\0') || raw_path.contains('\\') {
        return Err(AppError::InvalidSource {
            value: format!("unsafe-archive-path: {raw_path}"),
        });
    }

    let path = Path::new(raw_path);
    if path.is_absolute() {
        return Err(AppError::InvalidSource {
            value: format!("unsafe-archive-path: {raw_path}"),
        });
    }

    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => normalized.push(part),
            _ => {
                return Err(AppError::InvalidSource {
                    value: format!("unsafe-archive-path: {raw_path}"),
                });
            }
        }
    }

    if normalized.as_os_str().is_empty() {
        return Err(AppError::InvalidSource {
            value: format!("unsafe-archive-path: {raw_path}"),
        });
    }

    Ok(normalized)
}

fn write_archive_file(
    target_dir: &Path,
    relative_path: &str,
    content: &[u8],
    state: &mut ArchiveExtractionState,
) -> Result<(), AppError> {
    let relative = normalize_archive_path(relative_path)?;
    state.track_file(content.len() as u64)?;
    let target_path = target_dir.join(relative);
    if !target_path.starts_with(target_dir) {
        return Err(AppError::InvalidSource {
            value: format!("unsafe-archive-path: {relative_path}"),
        });
    }
    if let Some(parent) = target_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(target_path, content)?;
    Ok(())
}

fn extract_archive_to_skill_dir(
    bytes: &[u8],
    format: WellKnownArchiveFormat,
    target_dir: &Path,
) -> Result<(), AppError> {
    fs::create_dir_all(target_dir)?;
    let mut state = ArchiveExtractionState::default();

    match format {
        WellKnownArchiveFormat::Zip => extract_zip_archive(bytes, target_dir, &mut state)?,
        WellKnownArchiveFormat::TarGz => extract_targz_archive(bytes, target_dir, &mut state)?,
    }

    if crate::core::skill_paths::find_skill_md_case_insensitive(target_dir).is_none() {
        return Err(AppError::InvalidSource {
            value: "Archive missing root SKILL.md".to_string(),
        });
    }

    Ok(())
}

fn extract_zip_archive(
    bytes: &[u8],
    target_dir: &Path,
    state: &mut ArchiveExtractionState,
) -> Result<(), AppError> {
    let reader = Cursor::new(bytes);
    let mut archive = zip::ZipArchive::new(reader).map_err(|e| AppError::InvalidSource {
        value: format!("Invalid zip archive: {e}"),
    })?;

    for i in 0..archive.len() {
        let mut file = archive.by_index(i).map_err(|e| AppError::InvalidSource {
            value: format!("Invalid zip entry: {e}"),
        })?;
        if file.is_dir() {
            continue;
        }
        if file.enclosed_name().is_none() {
            return Err(AppError::InvalidSource {
                value: format!("unsafe-archive-path: {}", file.name()),
            });
        }
        if let Some(mode) = file.unix_mode() {
            if mode & 0o170000 == 0o120000 {
                return Err(AppError::InvalidSource {
                    value: "Archive links are not supported".to_string(),
                });
            }
        }

        let name = file.name().to_string();
        let mut content = Vec::new();
        file.read_to_end(&mut content)
            .map_err(|e| AppError::InvalidSource {
                value: format!("Invalid zip entry: {e}"),
            })?;
        write_archive_file(target_dir, &name, &content, state)?;
    }

    Ok(())
}

fn extract_targz_archive(
    bytes: &[u8],
    target_dir: &Path,
    state: &mut ArchiveExtractionState,
) -> Result<(), AppError> {
    let decoder = flate2::read::GzDecoder::new(Cursor::new(bytes));
    let mut archive = tar::Archive::new(decoder);

    let entries = archive.entries().map_err(|e| AppError::InvalidSource {
        value: format!("Invalid tar.gz archive: {e}"),
    })?;

    for entry in entries {
        let mut entry = entry.map_err(|e| AppError::InvalidSource {
            value: format!("Invalid tar.gz entry: {e}"),
        })?;
        let entry_type = entry.header().entry_type();
        if entry_type.is_dir() {
            continue;
        }
        if entry_type.is_symlink() || entry_type.is_hard_link() {
            return Err(AppError::InvalidSource {
                value: "Archive links are not supported".to_string(),
            });
        }
        if !entry_type.is_file() {
            continue;
        }

        let path = entry.path().map_err(|e| AppError::InvalidSource {
            value: format!("Invalid tar.gz path: {e}"),
        })?;
        let path = path.to_string_lossy().to_string();
        let mut content = Vec::new();
        entry
            .read_to_end(&mut content)
            .map_err(|e| AppError::InvalidSource {
                value: format!("Invalid tar.gz entry: {e}"),
            })?;
        write_archive_file(target_dir, &path, &content, state)?;
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// HTTP fetch
// ---------------------------------------------------------------------------

/// Fetch the well-known skills index from a URL, then download every declared
/// file into a temporary directory. Returns the temp path and a hostname-based
/// source identifier for lock-file storage.
pub async fn fetch_wellknown_skills(url: &str) -> Result<WellKnownFetchResult, AppError> {
    let source_identifier = extract_hostname(url).unwrap_or_else(|| "unknown".to_string());

    let client = Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .build()
        .map_err(|e| AppError::GitNetworkError {
            message: e.to_string(),
        })?;

    let entries = fetch_index(&client, url).await?;

    if entries.is_empty() {
        return Err(AppError::NoSkillsFound);
    }

    let temp_path = tempfile::TempDir::new()?.keep();
    let mut trust_metadata = HashMap::new();

    for entry in &entries {
        match entry {
            NormalizedWellKnownEntry::Legacy {
                name,
                files,
                base_url,
                ..
            } => {
                download_legacy_entry(&client, &temp_path, name, files, base_url).await?;
            }
            NormalizedWellKnownEntry::V2 {
                name,
                entry_type,
                artifact_url,
                digest,
                ..
            } => {
                download_v2_entry(&client, &temp_path, name, entry_type, artifact_url, digest)
                    .await?;
            }
        }
        trust_metadata.insert(entry.name().to_string(), entry.trust_metadata());
    }

    Ok(WellKnownFetchResult {
        repo_path: temp_path,
        source_identifier,
        trust_metadata,
    })
}

async fn download_legacy_entry(
    client: &Client,
    temp_path: &std::path::Path,
    name: &str,
    files: &[String],
    base_url: &str,
) -> Result<(), AppError> {
    let skill_dir = temp_path.join(name);
    fs::create_dir_all(&skill_dir)?;

    let mut skill_ok = true;

    for file_path in files {
        let file_url = format!("{base_url}/{name}/{file_path}");

        let response = match client.get(&file_url).send().await {
            Ok(resp) if resp.status().is_success() => resp,
            _ => {
                if file_path.eq_ignore_ascii_case("SKILL.md") {
                    skill_ok = false;
                    break;
                }
                continue;
            }
        };

        let target_path = skill_dir.join(file_path);

        if !target_path.starts_with(&skill_dir) {
            continue;
        }

        if let Some(parent) = target_path.parent() {
            fs::create_dir_all(parent)?;
        }

        let bytes = response
            .bytes()
            .await
            .map_err(|e| AppError::GitNetworkError {
                message: e.to_string(),
            })?;
        fs::write(&target_path, &bytes)?;
    }

    if !skill_ok {
        let _ = fs::remove_dir_all(&skill_dir);
    }

    Ok(())
}

async fn download_v2_entry(
    client: &Client,
    temp_path: &std::path::Path,
    name: &str,
    entry_type: &str,
    artifact_url: &str,
    digest: &str,
) -> Result<(), AppError> {
    let response =
        client
            .get(artifact_url)
            .send()
            .await
            .map_err(|e| AppError::GitNetworkError {
                message: e.to_string(),
            })?;
    if !response.status().is_success() {
        return Err(AppError::InvalidSource {
            value: format!(
                "http-{}: failed to download well-known artifact",
                response.status().as_u16()
            ),
        });
    }

    let bytes = response
        .bytes()
        .await
        .map_err(|e| AppError::GitNetworkError {
            message: e.to_string(),
        })?;
    verify_artifact_digest(&bytes, digest)?;

    let skill_dir = temp_path.join(name);
    fs::create_dir_all(&skill_dir)?;

    match entry_type {
        "skill-md" => {
            fs::write(skill_dir.join("SKILL.md"), &bytes)?;
            Ok(())
        }
        "archive" => {
            let format = detect_archive_format(&bytes, artifact_url, "");
            let Some(format) = format else {
                return Err(AppError::InvalidSource {
                    value: "Unsupported archive format".to_string(),
                });
            };
            extract_archive_to_skill_dir(&bytes, format, &skill_dir)
        }
        other => Err(AppError::InvalidSource {
            value: format!("Unsupported well-known entry type: {other}"),
        }),
    }
}

/// Try each candidate index URL in order; return the first that responds with
/// a non-empty skills list, together with its `base_url` (which already
/// includes the matched well-known path, e.g. `.well-known/agent-skills`).
async fn fetch_index(
    client: &Client,
    url: &str,
) -> Result<Vec<NormalizedWellKnownEntry>, AppError> {
    let candidates = build_index_urls(url);
    if candidates.is_empty() {
        return Err(AppError::InvalidSource {
            value: format!("Cannot build index URL from: {url}"),
        });
    }

    for candidate in &candidates {
        let resp = match client.get(&candidate.index_url).send().await {
            Ok(r) if r.status().is_success() => r,
            _ => continue,
        };

        let raw: serde_json::Value = match resp.json().await {
            Ok(idx) => idx,
            Err(_) => continue,
        };

        if let Some(entries) =
            normalize_wellknown_index(&raw, &candidate.index_url, &candidate.well_known_path)
        {
            return Ok(entries);
        }
    }

    Err(AppError::InvalidSource {
        value: format!("No well-known skills index found at: {url}"),
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;

    #[test]
    fn test_parse_valid_index() {
        let json = r#"{ "skills": [{ "name": "my-skill", "description": "A skill", "files": ["SKILL.md"] }] }"#;
        let index: WellKnownIndex = serde_json::from_str(json).unwrap();
        assert_eq!(index.skills.len(), 1);
        assert_eq!(index.skills[0].name, "my-skill");
        assert_eq!(index.skills[0].files, vec!["SKILL.md"]);
    }

    #[test]
    fn test_normalize_index_accepts_legacy_without_schema() {
        let raw: serde_json::Value = serde_json::from_str(
            r#"{ "skills": [{ "name": "legacy", "description": "Legacy", "files": ["SKILL.md"] }] }"#,
        )
        .unwrap();

        let entries = normalize_wellknown_index(
            &raw,
            "https://example.com/.well-known/agent-skills/index.json",
            ".well-known/agent-skills",
        )
        .expect("legacy index should normalize");

        assert!(matches!(
            &entries[0],
            NormalizedWellKnownEntry::Legacy { name, files, .. }
                if name == "legacy" && files == &vec!["SKILL.md".to_string()]
        ));
    }

    #[test]
    fn test_legacy_index_rejects_all_entries_when_any_entry_invalid() {
        let raw: serde_json::Value = serde_json::from_str(
            r#"{
                "skills": [
                    { "name": "good", "description": "Good", "files": ["SKILL.md"] },
                    { "name": "bad", "description": "Bad", "files": ["README.md"] }
                ]
            }"#,
        )
        .unwrap();

        let entries = normalize_wellknown_index(
            &raw,
            "https://example.com/.well-known/agent-skills/index.json",
            ".well-known/agent-skills",
        );

        assert!(entries.is_none());
    }

    #[test]
    fn test_normalize_index_accepts_v2_skill_md_entry() {
        let digest = format!("sha256:{}", "0".repeat(64));
        let raw: serde_json::Value = serde_json::from_str(&format!(
            r#"{{
              "$schema": "https://schemas.agentskills.io/discovery/0.2.0/schema.json",
              "skills": [{{
                "name": "demo",
                "description": "Demo",
                "type": "skill-md",
                "url": "./demo/SKILL.md",
                "digest": "{digest}"
              }}]
            }}"#
        ))
        .unwrap();

        let entries = normalize_wellknown_index(
            &raw,
            "https://example.com/.well-known/agent-skills/index.json",
            ".well-known/agent-skills",
        )
        .expect("v2 index should normalize");

        assert!(matches!(
            &entries[0],
            NormalizedWellKnownEntry::V2 {
                name,
                entry_type,
                artifact_url,
                digest: entry_digest,
                ..
            } if name == "demo"
                && entry_type == "skill-md"
                && artifact_url == "https://example.com/.well-known/agent-skills/demo/SKILL.md"
                && entry_digest == &digest
        ));
    }

    #[test]
    fn test_normalize_index_rejects_v2_invalid_digest_format() {
        let raw: serde_json::Value = serde_json::from_str(
            r#"{
              "$schema": "https://schemas.agentskills.io/discovery/0.2.0/schema.json",
              "skills": [{
                "name": "demo",
                "description": "Demo",
                "type": "skill-md",
                "url": "./demo/SKILL.md",
                "digest": "sha256:abc"
              }]
            }"#,
        )
        .unwrap();

        let entries = normalize_wellknown_index(
            &raw,
            "https://example.com/.well-known/agent-skills/index.json",
            ".well-known/agent-skills",
        );

        assert!(entries.is_none());
    }

    #[test]
    fn test_normalize_index_rejects_v2_invalid_skill_name() {
        let digest = format!("sha256:{}", "0".repeat(64));
        let raw: serde_json::Value = serde_json::from_str(&format!(
            r#"{{
              "$schema": "https://schemas.agentskills.io/discovery/0.2.0/schema.json",
              "skills": [{{
                "name": "Bad_Skill",
                "description": "Demo",
                "type": "skill-md",
                "url": "./demo/SKILL.md",
                "digest": "{digest}"
              }}]
            }}"#
        ))
        .unwrap();

        let entries = normalize_wellknown_index(
            &raw,
            "https://example.com/.well-known/agent-skills/index.json",
            ".well-known/agent-skills",
        );

        assert!(entries.is_none());
    }

    #[test]
    fn test_normalize_index_rejects_unknown_schema() {
        let raw: serde_json::Value = serde_json::from_str(
            r#"{
              "$schema": "https://schemas.agentskills.io/discovery/9.9.9/schema.json",
              "skills": [{ "name": "legacy", "description": "Legacy", "files": ["SKILL.md"] }]
            }"#,
        )
        .unwrap();

        let entries = normalize_wellknown_index(
            &raw,
            "https://example.com/.well-known/agent-skills/index.json",
            ".well-known/agent-skills",
        );

        assert!(entries.is_none());
    }

    #[test]
    fn test_verify_artifact_digest_returns_digest_mismatch() {
        let err = verify_artifact_digest(b"hello", &format!("sha256:{}", "0".repeat(64)))
            .expect_err("digest should mismatch");

        assert!(err.to_string().contains("digest-mismatch"));
    }

    fn zip_bytes(files: &[(&str, &[u8])]) -> Vec<u8> {
        use std::io::{Cursor, Write};
        use zip::write::SimpleFileOptions;

        let cursor = Cursor::new(Vec::new());
        let mut writer = zip::ZipWriter::new(cursor);
        for (path, content) in files {
            writer
                .start_file(*path, SimpleFileOptions::default())
                .unwrap();
            writer.write_all(content).unwrap();
        }
        writer.finish().unwrap().into_inner()
    }

    fn tar_gz_bytes(files: &[(&str, &[u8])]) -> Vec<u8> {
        use flate2::write::GzEncoder;
        use flate2::Compression;

        let encoder = GzEncoder::new(Vec::new(), Compression::default());
        let mut builder = tar::Builder::new(encoder);
        for (path, content) in files {
            let mut header = tar::Header::new_gnu();
            header.set_size(content.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            builder.append_data(&mut header, *path, *content).unwrap();
        }
        builder.into_inner().unwrap().finish().unwrap()
    }

    #[test]
    fn test_extract_zip_archive_to_skill_dir() {
        let temp = tempfile::tempdir().unwrap();
        let bytes = zip_bytes(&[
            ("SKILL.md", b"---\nname: demo\ndescription: Demo\n---\n"),
            ("lib/helper.txt", b"helper"),
        ]);

        extract_archive_to_skill_dir(&bytes, WellKnownArchiveFormat::Zip, temp.path()).unwrap();

        assert!(temp.path().join("SKILL.md").exists());
        assert_eq!(
            fs::read_to_string(temp.path().join("lib/helper.txt")).unwrap(),
            "helper"
        );
    }

    #[test]
    fn test_extract_targz_archive_to_skill_dir() {
        let temp = tempfile::tempdir().unwrap();
        let bytes = tar_gz_bytes(&[
            ("skill.md", b"---\nname: demo\ndescription: Demo\n---\n"),
            ("assets/data.txt", b"data"),
        ]);

        extract_archive_to_skill_dir(&bytes, WellKnownArchiveFormat::TarGz, temp.path()).unwrap();

        assert!(temp.path().join("skill.md").exists());
        assert_eq!(
            fs::read_to_string(temp.path().join("assets/data.txt")).unwrap(),
            "data"
        );
    }

    #[test]
    fn test_extract_archive_rejects_traversal_path() {
        let temp = tempfile::tempdir().unwrap();
        let bytes = zip_bytes(&[("../evil", b"bad"), ("SKILL.md", b"ok")]);

        let err = extract_archive_to_skill_dir(&bytes, WellKnownArchiveFormat::Zip, temp.path())
            .expect_err("traversal path should be rejected");

        assert!(err.to_string().contains("unsafe-archive-path"));
    }

    #[test]
    fn test_archive_limits_reject_too_many_files() {
        let mut state = ArchiveExtractionState::default();
        for _ in 0..MAX_ARCHIVE_FILES {
            state.track_file(0).unwrap();
        }

        let err = state
            .track_file(0)
            .expect_err("1001st file should be rejected");

        assert!(err.to_string().contains("too many files"));
    }

    #[test]
    fn test_archive_limits_reject_too_many_unpacked_bytes() {
        let mut state = ArchiveExtractionState::default();
        let err = state
            .track_file(MAX_ARCHIVE_UNPACKED_BYTES + 1)
            .expect_err("oversized archive should be rejected");

        assert!(err.to_string().contains("archive-too-large"));
    }

    fn spawn_response_server(response: &'static str) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut buffer = [0_u8; 1024];
                let _ = stream.read(&mut buffer);
                stream.write_all(response.as_bytes()).unwrap();
            }
        });
        format!("http://{addr}/artifact.md")
    }

    #[test]
    fn test_download_v2_entry_returns_http_reason() {
        tauri::async_runtime::block_on(async {
            let temp = tempfile::tempdir().unwrap();
            let artifact_url = spawn_response_server(
                "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            );

            let err = download_v2_entry(
                &Client::new(),
                temp.path(),
                "demo",
                "skill-md",
                &artifact_url,
                &format!("sha256:{}", "0".repeat(64)),
            )
            .await
            .expect_err("non-success artifact response should be reported");

            assert!(err.to_string().contains("http-404"));
        });
    }

    #[test]
    fn test_parse_index_multiple_skills() {
        let json = r#"{
            "skills": [
                { "name": "alpha", "description": "First", "files": ["SKILL.md", "lib.py"] },
                { "name": "beta", "description": "Second", "files": ["SKILL.md"] }
            ]
        }"#;
        let index: WellKnownIndex = serde_json::from_str(json).unwrap();
        assert_eq!(index.skills.len(), 2);
        assert_eq!(index.skills[0].name, "alpha");
        assert_eq!(index.skills[1].name, "beta");
    }

    #[test]
    fn test_validate_entry_valid() {
        let entry = WellKnownSkillEntry {
            name: "good-skill".into(),
            description: "Does things".into(),
            files: vec!["SKILL.md".into(), "utils.py".into()],
        };
        assert!(validate_skill_entry(&entry).is_ok());
    }

    #[test]
    fn test_validate_entry_missing_skill_md() {
        let entry = WellKnownSkillEntry {
            name: "bad-skill".into(),
            description: "Missing SKILL.md".into(),
            files: vec!["README.md".into()],
        };
        assert!(validate_skill_entry(&entry).is_err());
    }

    #[test]
    fn test_validate_entry_path_traversal() {
        let entry = WellKnownSkillEntry {
            name: "evil".into(),
            description: "Traversal".into(),
            files: vec!["SKILL.md".into(), "../etc/passwd".into()],
        };
        let err = validate_skill_entry(&entry).unwrap_err();
        assert!(err.to_string().contains("Path traversal"));
    }

    #[test]
    fn test_validate_entry_rejects_double_dot_inside_filename() {
        let entry = WellKnownSkillEntry {
            name: "ok".into(),
            description: "Legacy paths reject any double-dot occurrence".into(),
            files: vec!["SKILL.md".into(), "my..file.txt".into()],
        };
        let err = validate_skill_entry(&entry).unwrap_err();
        assert!(err.to_string().contains("Path traversal"));
    }

    #[test]
    fn test_validate_entry_rejects_null_byte_path() {
        let entry = WellKnownSkillEntry {
            name: "evil".into(),
            description: "Null byte".into(),
            files: vec!["SKILL.md".into(), "evil\0name.txt".into()],
        };
        let err = validate_skill_entry(&entry).unwrap_err();
        assert!(err.to_string().contains("Null byte"));
    }

    #[test]
    fn test_validate_entry_absolute_path() {
        let entry = WellKnownSkillEntry {
            name: "evil".into(),
            description: "Absolute".into(),
            files: vec!["SKILL.md".into(), "/etc/passwd".into()],
        };
        let err = validate_skill_entry(&entry).unwrap_err();
        assert!(err.to_string().contains("Absolute path"));
    }

    #[test]
    fn test_validate_entry_empty_name() {
        let entry = WellKnownSkillEntry {
            name: "  ".into(),
            description: "Has description".into(),
            files: vec!["SKILL.md".into()],
        };
        let err = validate_skill_entry(&entry).unwrap_err();
        assert!(err.to_string().contains("name must not be empty"));
    }

    #[test]
    fn test_validate_entry_rejects_invalid_skill_name() {
        let entry = WellKnownSkillEntry {
            name: "Bad_Skill".into(),
            description: "Has description".into(),
            files: vec!["SKILL.md".into()],
        };
        let err = validate_skill_entry(&entry).unwrap_err();
        assert!(err.to_string().contains("Invalid skill name"));
    }

    #[test]
    fn test_validate_entry_empty_description() {
        let entry = WellKnownSkillEntry {
            name: "some-skill".into(),
            description: "".into(),
            files: vec!["SKILL.md".into()],
        };
        let err = validate_skill_entry(&entry).unwrap_err();
        assert!(err.to_string().contains("description must not be empty"));
    }

    #[test]
    fn test_validate_entry_empty_filename() {
        let entry = WellKnownSkillEntry {
            name: "bad".into(),
            description: "Has empty filename".into(),
            files: vec!["SKILL.md".into(), "  ".into()],
        };
        let err = validate_skill_entry(&entry).unwrap_err();
        assert!(err.to_string().contains("Empty filename"));
    }

    #[test]
    fn test_extract_hostname_basic() {
        assert_eq!(
            extract_hostname("https://mintlify.com/docs"),
            Some("mintlify.com".into())
        );
    }

    #[test]
    fn test_extract_hostname_strips_www() {
        assert_eq!(
            extract_hostname("https://www.example.com"),
            Some("example.com".into())
        );
    }

    #[test]
    fn test_extract_hostname_preserves_subdomain() {
        assert_eq!(
            extract_hostname("https://docs.lovable.dev"),
            Some("docs.lovable.dev".into())
        );
    }

    #[test]
    fn test_extract_hostname_invalid_url() {
        assert_eq!(extract_hostname("not-a-url"), None);
    }

    #[test]
    fn test_build_index_urls_with_path() {
        let candidates = build_index_urls("https://example.com/docs");
        // 2 well-known paths × 2 locations (path-relative + root) = 4 candidates
        assert_eq!(candidates.len(), 4);

        // agent-skills path-relative (preferred)
        assert_eq!(
            candidates[0].index_url,
            "https://example.com/docs/.well-known/agent-skills/index.json"
        );
        assert_eq!(
            candidates[0].base_url,
            "https://example.com/docs/.well-known/agent-skills"
        );
        assert_eq!(candidates[0].well_known_path, ".well-known/agent-skills");

        // agent-skills root
        assert_eq!(
            candidates[1].index_url,
            "https://example.com/.well-known/agent-skills/index.json"
        );
        assert_eq!(
            candidates[1].base_url,
            "https://example.com/.well-known/agent-skills"
        );

        // legacy skills path-relative (fallback)
        assert_eq!(
            candidates[2].index_url,
            "https://example.com/docs/.well-known/skills/index.json"
        );
        assert_eq!(
            candidates[2].base_url,
            "https://example.com/docs/.well-known/skills"
        );
        assert_eq!(candidates[2].well_known_path, ".well-known/skills");

        // legacy skills root
        assert_eq!(
            candidates[3].index_url,
            "https://example.com/.well-known/skills/index.json"
        );
        assert_eq!(
            candidates[3].base_url,
            "https://example.com/.well-known/skills"
        );
    }

    #[test]
    fn test_build_index_urls_root() {
        let candidates = build_index_urls("https://example.com");
        // 2 well-known paths × 1 location (root only) = 2 candidates
        assert_eq!(candidates.len(), 2);
        assert_eq!(
            candidates[0].index_url,
            "https://example.com/.well-known/agent-skills/index.json"
        );
        assert_eq!(candidates[0].well_known_path, ".well-known/agent-skills");
        assert_eq!(
            candidates[1].index_url,
            "https://example.com/.well-known/skills/index.json"
        );
        assert_eq!(candidates[1].well_known_path, ".well-known/skills");
    }

    #[test]
    fn test_build_index_urls_agent_skills_tried_before_legacy() {
        let candidates = build_index_urls("https://example.com/app");
        // Verify ordering: all agent-skills candidates come before legacy skills
        let agent_skills_indices: Vec<usize> = candidates
            .iter()
            .enumerate()
            .filter(|(_, c)| c.well_known_path.contains("agent-skills"))
            .map(|(i, _)| i)
            .collect();
        let legacy_indices: Vec<usize> = candidates
            .iter()
            .enumerate()
            .filter(|(_, c)| !c.well_known_path.contains("agent-skills"))
            .map(|(i, _)| i)
            .collect();
        assert!(
            agent_skills_indices
                .iter()
                .all(|&a| legacy_indices.iter().all(|&l| a < l)),
            "agent-skills candidates must come before legacy skills candidates"
        );
    }
}

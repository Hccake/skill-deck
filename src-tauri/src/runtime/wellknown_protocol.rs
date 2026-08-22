//! Well-Known Skills protocol support (RFC 8615 `.well-known/agent-skills`).
//!
//! Implements discovery of skills hosted under the `.well-known/agent-skills/`
//! path on websites, with fallback to the legacy `.well-known/skills/` path.
//! This is the Rust equivalent of the CLI's `providers/wellknown.ts`.

use crate::application::download_source::{extract_archive, ArchiveFormat, ArchiveLimits};
use crate::application::wellknown_access::{
    extract_hostname, WellKnownFetchError, WellKnownFetchResult, WellKnownTrustMetadata,
};
use crate::core::mutation::CancellationSignal;
use crate::error::AppError;
use crate::runtime::http_transport::{HttpGetRequest, HttpTransport, HttpTransportError};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs;
use std::path::Path;
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
pub struct WellKnownSkillEntry {
    pub name: String,
    pub description: String,
    pub files: Vec<String>,
}

#[derive(Debug, Clone)]
enum NormalizedWellKnownEntry {
    Legacy {
        name: String,
        files: Vec<String>,
        base_url: String,
    },
    V2 {
        name: String,
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

    fn trust_metadata(&self, digest: String) -> WellKnownTrustMetadata {
        match self {
            Self::Legacy { name, base_url, .. } => WellKnownTrustMetadata {
                well_known_version: Some("0.1.0".to_string()),
                well_known_entry_type: Some("legacy".to_string()),
                artifact_url_host: None,
                digest_verified: None,
                trust_reason: Some("legacy".to_string()),
                artifact_url: Some(format!(
                    "{}/{name}/SKILL.md",
                    base_url.trim_end_matches('/')
                )),
                digest: Some(digest),
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
                artifact_url: Some(artifact_url.clone()),
                digest: Some(digest),
            },
        }
    }
}

struct FetchedWellKnownIndex {
    index_url: String,
    entries: Vec<NormalizedWellKnownEntry>,
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

struct IndexUrlCandidate {
    index_url: String,
}

struct ScopedWellKnownInput {
    scope_path: String,
    root_url: String,
    scoped_candidate_count: usize,
}

struct IndexUrlPlan {
    candidates: Vec<IndexUrlCandidate>,
    scope: Option<ScopedWellKnownInput>,
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
fn build_index_url_plan(url: &str) -> IndexUrlPlan {
    let Ok(parsed) = Url::parse(url) else {
        return IndexUrlPlan {
            candidates: vec![],
            scope: None,
        };
    };

    if parsed.path().trim_end_matches('/').ends_with("/index.json") {
        let mut index_url = parsed;
        index_url.set_fragment(None);
        let index_url = index_url.to_string();
        return IndexUrlPlan {
            candidates: vec![IndexUrlCandidate { index_url }],
            scope: None,
        };
    }

    let origin = parsed.origin().ascii_serialization();
    let path = parsed.path().trim_end_matches('/');

    let mut candidates = Vec::new();

    if !path.is_empty() {
        for &wk_path in WELL_KNOWN_PATHS {
            candidates.push(IndexUrlCandidate {
                index_url: format!("{origin}{path}/{wk_path}/{INDEX_FILE}"),
            });
        }
    }

    let scoped_candidate_count = candidates.len();
    for &wk_path in WELL_KNOWN_PATHS {
        candidates.push(IndexUrlCandidate {
            index_url: format!("{origin}/{wk_path}/{INDEX_FILE}"),
        });
    }

    IndexUrlPlan {
        candidates,
        scope: (!path.is_empty()).then(|| ScopedWellKnownInput {
            scope_path: path.to_string(),
            root_url: origin,
            scoped_candidate_count,
        }),
    }
}

#[cfg(test)]
fn build_index_urls(url: &str) -> Vec<IndexUrlCandidate> {
    build_index_url_plan(url).candidates
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
) -> Option<Vec<NormalizedWellKnownEntry>> {
    let object = raw.as_object()?;
    let skills = object.get("skills")?.as_array()?;
    let schema = object.get("$schema").and_then(|value| value.as_str());

    if schema == Some(DISCOVERY_SCHEMA_V2) {
        if skills.is_empty() {
            return Some(Vec::new());
        }
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

    if skills.is_empty() {
        return Some(Vec::new());
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
            files: entry.files,
            base_url: base_url.clone(),
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

fn extract_archive_to_skill_dir(
    bytes: &[u8],
    format: WellKnownArchiveFormat,
    target_dir: &Path,
) -> Result<(), AppError> {
    fs::create_dir_all(target_dir)?;
    extract_archive(
        bytes,
        match format {
            WellKnownArchiveFormat::Zip => ArchiveFormat::Zip,
            WellKnownArchiveFormat::TarGz => ArchiveFormat::TarGz,
        },
        target_dir,
        ArchiveLimits {
            max_unpacked_bytes: MAX_ARCHIVE_UNPACKED_BYTES,
            max_entries: MAX_ARCHIVE_FILES,
        },
    )?;

    if crate::core::skill_paths::find_skill_md_case_insensitive(target_dir).is_none() {
        return Err(AppError::InvalidSource {
            value: "Archive missing root SKILL.md".to_string(),
        });
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// HTTP fetch
// ---------------------------------------------------------------------------

/// Fetch the well-known skills index from a URL, then download every declared
/// file into a temporary directory. Returns the temp path and a hostname-based
/// source identifier for lock-file storage.
#[cfg(test)]
pub(crate) async fn fetch_wellknown_skills_with_client(
    http: &HttpTransport,
    url: &str,
    cancellation: &CancellationSignal,
) -> Result<WellKnownFetchResult, AppError> {
    fetch_wellknown_skills_attempt_with_client(http, url, cancellation)
        .await
        .map_err(WellKnownFetchError::into_error)
}

pub(crate) async fn fetch_wellknown_skills_attempt_with_client(
    http: &HttpTransport,
    url: &str,
    cancellation: &CancellationSignal,
) -> Result<WellKnownFetchResult, WellKnownFetchError> {
    let operation_id = uuid::Uuid::new_v4().simple().to_string();

    let fetched_index = fetch_index(http, url, cancellation, &operation_id)
        .await
        .map_err(|error| {
            if matches!(error, AppError::WellKnownScopeNotFound { .. }) {
                WellKnownFetchError::catalog_established(error)
            } else {
                WellKnownFetchError::unproven(error)
            }
        })?;
    materialize_fetched_index(http, fetched_index, cancellation, &operation_id)
        .await
        .map_err(WellKnownFetchError::catalog_established)
}

async fn materialize_fetched_index(
    http: &HttpTransport,
    fetched_index: FetchedWellKnownIndex,
    cancellation: &CancellationSignal,
    operation_id: &str,
) -> Result<WellKnownFetchResult, AppError> {
    let entries = fetched_index.entries;

    if entries.is_empty() {
        return Err(AppError::NoSkillsFound);
    }

    let temp_path = tempfile::TempDir::new()?.keep();
    let mut trust_metadata = HashMap::new();
    let mut redirected_download_host = None;
    let download = WellKnownDownloadContext {
        http,
        temp_path: &temp_path,
        cancellation,
        operation_id,
    };

    for entry in &entries {
        match entry {
            NormalizedWellKnownEntry::Legacy {
                name,
                files,
                base_url,
                ..
            } => {
                let redirected = download_legacy_entry(&download, name, files, base_url).await?;
                redirected_download_host = redirected_download_host.or(redirected);
            }
            NormalizedWellKnownEntry::V2 {
                name,
                entry_type,
                artifact_url,
                digest,
                ..
            } => {
                let redirected =
                    download_v2_entry(&download, name, entry_type, artifact_url, digest).await?;
                redirected_download_host = redirected_download_host.or(redirected);
            }
        }
        let digest = match entry {
            NormalizedWellKnownEntry::V2 { digest, .. } => digest.clone(),
            NormalizedWellKnownEntry::Legacy { name, .. } => {
                compute_legacy_skill_digest(&temp_path.join(name))?
            }
        };
        trust_metadata.insert(entry.name().to_string(), entry.trust_metadata(digest));
    }

    Ok(WellKnownFetchResult {
        repo_path: temp_path,
        trust_metadata,
        redirected_download_host,
    })
}

pub(crate) async fn check_wellknown_updates_with_client(
    http: &HttpTransport,
    url: &str,
    skill_names: &[String],
    cancellation: &CancellationSignal,
) -> Result<crate::application::wellknown_access::WellKnownIndexEvidence, AppError> {
    let operation_id = uuid::Uuid::new_v4().simple().to_string();
    let fetched = fetch_index(http, url, cancellation, &operation_id).await?;
    let complete_skill_catalog = fetched
        .entries
        .iter()
        .map(|entry| entry.name().to_string())
        .collect::<Vec<_>>();
    let requested = skill_names
        .iter()
        .map(String::as_str)
        .collect::<std::collections::HashSet<_>>();
    let temp = tempfile::TempDir::new()?;
    let download = WellKnownDownloadContext {
        http,
        temp_path: temp.path(),
        cancellation,
        operation_id: &operation_id,
    };
    let mut digests = HashMap::new();
    for entry in &fetched.entries {
        if !requested.contains(entry.name()) {
            continue;
        }
        match entry {
            NormalizedWellKnownEntry::V2 { name, digest, .. } => {
                digests.insert(name.clone(), digest.clone());
            }
            NormalizedWellKnownEntry::Legacy {
                name,
                files,
                base_url,
                ..
            } => {
                download_legacy_entry(&download, name, files, base_url).await?;
                digests.insert(
                    name.clone(),
                    compute_legacy_skill_digest(&temp.path().join(name))?,
                );
            }
        }
    }
    Ok(
        crate::application::wellknown_access::WellKnownIndexEvidence {
            index_url: fetched.index_url,
            complete_skill_catalog,
            digests,
        },
    )
}

fn compute_legacy_skill_digest(root: &Path) -> Result<String, AppError> {
    let mut files = walkdir::WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| AppError::Path {
            message: error.to_string(),
        })?;
    files.retain(|entry| entry.file_type().is_file());
    files.sort_by_key(|entry| entry.path().to_path_buf());
    let mut hash = Sha256::new();
    for entry in files {
        let relative = entry
            .path()
            .strip_prefix(root)
            .map_err(|error| AppError::Path {
                message: error.to_string(),
            })?
            .to_string_lossy()
            .replace('\\', "/");
        hash.update(relative.as_bytes());
        hash.update([0]);
        hash.update(fs::read(entry.path())?);
        hash.update([0]);
    }
    Ok(format!("sha256:{:x}", hash.finalize()))
}

struct WellKnownDownloadContext<'a> {
    http: &'a HttpTransport,
    temp_path: &'a std::path::Path,
    cancellation: &'a CancellationSignal,
    operation_id: &'a str,
}

async fn download_legacy_entry(
    context: &WellKnownDownloadContext<'_>,
    name: &str,
    files: &[String],
    base_url: &str,
) -> Result<Option<String>, AppError> {
    let skill_dir = context.temp_path.join(name);
    fs::create_dir_all(&skill_dir)?;

    let mut skill_ok = true;
    let mut redirected_download_host = None;

    for file_path in files {
        if context.cancellation.is_cancelled() {
            return Err(AppError::MutationCancelled);
        }
        let file_url = format!("{base_url}/{name}/{file_path}");

        let response = match context
            .http
            .get(
                HttpGetRequest::new(
                    &file_url,
                    REQUEST_TIMEOUT,
                    MAX_ARCHIVE_UNPACKED_BYTES as usize,
                )
                .operation_id(context.operation_id)
                .cancellation(context.cancellation.clone()),
            )
            .await
        {
            Ok(resp) if resp.status.is_success() => resp,
            Err(error) if network_request_was_cancelled(&error) => {
                return Err(AppError::MutationCancelled);
            }
            _ => {
                if file_path.eq_ignore_ascii_case("SKILL.md") {
                    skill_ok = false;
                    break;
                }
                continue;
            }
        };

        redirected_download_host = redirected_download_host.or_else(|| {
            crate::application::source_acquisition::redirected_host(
                &file_url,
                response.final_url.as_str(),
            )
        });
        let target_path = skill_dir.join(file_path);

        if !target_path.starts_with(&skill_dir) {
            continue;
        }

        if let Some(parent) = target_path.parent() {
            fs::create_dir_all(parent)?;
        }

        fs::write(&target_path, &response.body)?;
    }

    if !skill_ok {
        let _ = fs::remove_dir_all(&skill_dir);
    }

    Ok(redirected_download_host)
}

async fn download_v2_entry(
    context: &WellKnownDownloadContext<'_>,
    name: &str,
    entry_type: &str,
    artifact_url: &str,
    digest: &str,
) -> Result<Option<String>, AppError> {
    let response = context
        .http
        .get(
            HttpGetRequest::new(
                artifact_url,
                REQUEST_TIMEOUT,
                MAX_ARCHIVE_UNPACKED_BYTES as usize,
            )
            .operation_id(context.operation_id)
            .cancellation(context.cancellation.clone()),
        )
        .await
        .map_err(|error| map_network_error(error, "无法下载 well-known Skill 制品"))?;
    if !response.status.is_success() {
        return Err(AppError::WellKnownSourceFailed {
            reason: source_failure_reason_from_status(response.status.as_u16()),
        });
    }

    let bytes = response.body;
    verify_artifact_digest(&bytes, digest)?;

    let skill_dir = context.temp_path.join(name);
    fs::create_dir_all(&skill_dir)?;

    match entry_type {
        "skill-md" => {
            fs::write(skill_dir.join("SKILL.md"), &bytes)?;
            Ok(crate::application::source_acquisition::redirected_host(
                artifact_url,
                response.final_url.as_str(),
            ))
        }
        "archive" => {
            let format = detect_archive_format(&bytes, artifact_url, "");
            let Some(format) = format else {
                return Err(AppError::InvalidSource {
                    value: "Unsupported archive format".to_string(),
                });
            };
            extract_archive_to_skill_dir(&bytes, format, &skill_dir)?;
            Ok(crate::application::source_acquisition::redirected_host(
                artifact_url,
                response.final_url.as_str(),
            ))
        }
        other => Err(AppError::InvalidSource {
            value: format!("Unsupported well-known entry type: {other}"),
        }),
    }
}

/// Try each candidate index URL in order; return the first valid catalog,
/// including an empty one. The selected entries retain their resolved base URL.
async fn fetch_index(
    http: &HttpTransport,
    url: &str,
    cancellation: &CancellationSignal,
    operation_id: &str,
) -> Result<FetchedWellKnownIndex, AppError> {
    let plan = build_index_url_plan(url);
    if plan.candidates.is_empty() {
        return Err(AppError::InvalidSource {
            value: format!("Cannot build index URL from: {url}"),
        });
    }

    let mut saw_authentication_required = false;
    let mut transport_failure = None;
    let mut saw_not_found = false;
    let mut empty_scoped_catalog = None;

    for (candidate_index, candidate) in plan.candidates.iter().enumerate() {
        if cancellation.is_cancelled() {
            return Err(AppError::MutationCancelled);
        }
        let resp = match http
            .get(
                HttpGetRequest::new(&candidate.index_url, REQUEST_TIMEOUT, 1024 * 1024)
                    .operation_id(operation_id)
                    .cancellation(cancellation.clone()),
            )
            .await
        {
            Ok(r) if r.status.is_success() => r,
            Ok(r) if matches!(r.status.as_u16(), 401 | 403) => {
                saw_authentication_required = true;
                continue;
            }
            Ok(r) if r.status.as_u16() == 404 => {
                saw_not_found = true;
                continue;
            }
            Err(error) if network_request_was_cancelled(&error) => {
                return Err(AppError::MutationCancelled);
            }
            Err(error) => {
                log::warn!("Well-known index request failed: {error}");
                transport_failure
                    .get_or_insert_with(|| source_failure_reason_from_http_error(&error));
                continue;
            }
            Ok(r) => {
                transport_failure
                    .get_or_insert_with(|| source_failure_reason_from_status(r.status.as_u16()));
                continue;
            }
        };

        let raw: serde_json::Value = match serde_json::from_slice(&resp.body) {
            Ok(idx) => idx,
            Err(_) => continue,
        };

        if let Some(entries) = normalize_wellknown_index(&raw, resp.final_url.as_str()) {
            if let Some(scope) = &plan.scope {
                if candidate_index >= scope.scoped_candidate_count {
                    return Err(AppError::WellKnownScopeNotFound {
                        scope_path: scope.scope_path.clone(),
                        root_url: scope.root_url.clone(),
                    });
                }
                if entries.is_empty() {
                    empty_scoped_catalog = Some(FetchedWellKnownIndex {
                        index_url: resp.final_url.to_string(),
                        entries,
                    });
                    continue;
                }
            }
            return Ok(FetchedWellKnownIndex {
                index_url: resp.final_url.to_string(),
                entries,
            });
        }
    }

    if let Some(catalog) = empty_scoped_catalog {
        return Ok(catalog);
    }

    if saw_authentication_required {
        return Err(AppError::WellKnownSourceFailed {
            reason: crate::error::SourceAcquisitionFailureReason::AuthenticationRequired,
        });
    }
    if let Some(reason) = transport_failure {
        return Err(AppError::WellKnownSourceFailed { reason });
    }
    if saw_not_found {
        return Err(AppError::WellKnownSourceFailed {
            reason: crate::error::SourceAcquisitionFailureReason::NotFound,
        });
    }
    Err(AppError::InvalidSource {
        value: format!("No well-known skills index found at: {url}"),
    })
}

fn map_network_error(error: HttpTransportError, message: &str) -> AppError {
    if network_request_was_cancelled(&error) {
        AppError::MutationCancelled
    } else {
        log::warn!("{message}: {error}");
        AppError::WellKnownSourceFailed {
            reason: source_failure_reason_from_http_error(&error),
        }
    }
}

fn source_failure_reason_from_http_error(
    error: &HttpTransportError,
) -> crate::error::SourceAcquisitionFailureReason {
    use crate::error::SourceAcquisitionFailureReason;

    match error {
        HttpTransportError::Request {
            reason: "timeout", ..
        } => SourceAcquisitionFailureReason::Timeout,
        HttpTransportError::ResponseTooLarge => SourceAcquisitionFailureReason::LimitExceeded,
        HttpTransportError::Settings(_) | HttpTransportError::Request { .. } => {
            SourceAcquisitionFailureReason::Network
        }
    }
}

fn source_failure_reason_from_status(status: u16) -> crate::error::SourceAcquisitionFailureReason {
    use crate::error::SourceAcquisitionFailureReason;

    match status {
        401 | 403 => SourceAcquisitionFailureReason::AuthenticationRequired,
        404 => SourceAcquisitionFailureReason::NotFound,
        _ => SourceAcquisitionFailureReason::Network,
    }
}

fn network_request_was_cancelled(error: &HttpTransportError) -> bool {
    matches!(
        error,
        HttpTransportError::Request {
            reason: "cancelled",
            ..
        }
    )
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{NetworkProxySettings, ProxyMode};
    use crate::runtime::proxy_settings::ProxySettingsStore;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::Arc;
    use std::thread;

    #[test]
    fn persisted_index_url_is_requested_directly() {
        let index_url = "https://example.com/catalog/index.json";

        let candidates = build_index_urls(index_url);

        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].index_url, index_url);
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

        assert!(matches!(
            err,
            AppError::DirectDownloadFailed {
                reason: crate::error::DirectDownloadFailureReason::UnsafeArchive
            }
        ));
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
    fn valid_empty_catalog_is_established_protocol_evidence() {
        tauri::async_runtime::block_on(async {
            let server = tiny_http::Server::http("127.0.0.1:0").expect("server");
            let base_url = format!("http://{}", server.server_addr());
            let worker = thread::spawn(move || {
                let request = server
                    .recv_timeout(Duration::from_secs(2))
                    .expect("index receive")
                    .expect("index request");
                request
                    .respond(tiny_http::Response::from_string(r#"{"skills":[]}"#))
                    .expect("index response");
            });
            let http = HttpTransport::new(Arc::new(ProxySettingsStore::new(
                NetworkProxySettings::default(),
            )));

            let error = fetch_wellknown_skills_attempt_with_client(
                &http,
                &base_url,
                &CancellationSignal::default(),
            )
            .await
            .expect_err("empty catalog should not produce a source");

            worker.join().expect("worker");
            assert!(matches!(
                error,
                WellKnownFetchError::CatalogEstablished(AppError::NoSkillsFound)
            ));
        });
    }

    #[test]
    fn artifact_failure_after_valid_catalog_keeps_established_evidence() {
        tauri::async_runtime::block_on(async {
            let server = tiny_http::Server::http("127.0.0.1:0").expect("server");
            let base_url = format!("http://{}", server.server_addr());
            let digest = format!("sha256:{}", "0".repeat(64));
            let worker = thread::spawn(move || {
                let index = server
                    .recv_timeout(Duration::from_secs(2))
                    .expect("index receive")
                    .expect("index request");
                index
                    .respond(tiny_http::Response::from_string(format!(
                        r#"{{
                          "$schema": "{DISCOVERY_SCHEMA_V2}",
                          "skills": [{{
                            "name": "demo",
                            "description": "Demo",
                            "type": "skill-md",
                            "url": "demo/SKILL.md",
                            "digest": "{digest}"
                          }}]
                        }}"#
                    )))
                    .expect("index response");
                let artifact = server
                    .recv_timeout(Duration::from_secs(2))
                    .expect("artifact receive")
                    .expect("artifact request");
                artifact
                    .respond(tiny_http::Response::empty(404))
                    .expect("artifact response");
            });
            let http = HttpTransport::new(Arc::new(ProxySettingsStore::new(
                NetworkProxySettings::default(),
            )));

            let error = fetch_wellknown_skills_attempt_with_client(
                &http,
                &base_url,
                &CancellationSignal::default(),
            )
            .await
            .expect_err("artifact failure should fail acquisition");

            worker.join().expect("worker");
            assert!(matches!(error, WellKnownFetchError::CatalogEstablished(_)));
        });
    }

    #[test]
    fn missing_catalog_keeps_direct_download_fallback_eligible() {
        tauri::async_runtime::block_on(async {
            let server = tiny_http::Server::http("127.0.0.1:0").expect("server");
            let base_url = format!("http://{}", server.server_addr());
            let worker = thread::spawn(move || {
                for _ in WELL_KNOWN_PATHS {
                    let request = server
                        .recv_timeout(Duration::from_secs(2))
                        .expect("index receive")
                        .expect("index request");
                    request
                        .respond(tiny_http::Response::empty(404))
                        .expect("index response");
                }
            });
            let http = HttpTransport::new(Arc::new(ProxySettingsStore::new(
                NetworkProxySettings::default(),
            )));

            let error = fetch_wellknown_skills_attempt_with_client(
                &http,
                &base_url,
                &CancellationSignal::default(),
            )
            .await
            .expect_err("missing catalog should fail Well-known acquisition");

            worker.join().expect("worker");
            assert!(matches!(error, WellKnownFetchError::Unproven(_)));
        });
    }

    #[test]
    fn redirected_index_resolves_relative_artifact_from_the_final_url() {
        tauri::async_runtime::block_on(async {
            let server = tiny_http::Server::http("127.0.0.1:0").expect("origin server");
            let base_url = format!("http://{}", server.server_addr());
            let skill = b"---\nname: demo\ndescription: Demo\n---\n";
            let digest = compute_digest(skill);
            let worker = thread::spawn(move || {
                let redirect = server
                    .recv_timeout(Duration::from_secs(2))
                    .expect("redirect receive")
                    .expect("index request");
                assert_eq!(redirect.url(), "/.well-known/agent-skills/index.json");
                redirect
                    .respond(
                        tiny_http::Response::empty(302).with_header(
                            tiny_http::Header::from_bytes("Location", "/catalog/index.json")
                                .expect("location header"),
                        ),
                    )
                    .expect("redirect response");

                let index = server
                    .recv_timeout(Duration::from_secs(2))
                    .expect("index receive")
                    .expect("redirected index request");
                assert_eq!(index.url(), "/catalog/index.json");
                index
                    .respond(tiny_http::Response::from_string(format!(
                        r#"{{
                          "$schema": "{DISCOVERY_SCHEMA_V2}",
                          "skills": [{{
                            "name": "demo",
                            "description": "Demo",
                            "type": "skill-md",
                            "url": "demo/SKILL.md",
                            "digest": "{digest}"
                          }}]
                        }}"#
                    )))
                    .expect("index response");

                let artifact = server
                    .recv_timeout(Duration::from_secs(2))
                    .expect("artifact receive")
                    .expect("artifact request");
                assert_eq!(artifact.url(), "/catalog/demo/SKILL.md");
                artifact
                    .respond(tiny_http::Response::from_data(skill))
                    .expect("artifact response");
            });
            let http = HttpTransport::new(Arc::new(ProxySettingsStore::new(
                NetworkProxySettings::default(),
            )));

            let result = fetch_wellknown_skills_with_client(
                &http,
                &base_url,
                &CancellationSignal::default(),
            )
            .await
            .expect("well-known fetch");

            worker.join().expect("origin worker");
            assert_eq!(
                fs::read(result.repo_path.join("demo/SKILL.md")).expect("downloaded skill"),
                skill
            );
            fs::remove_dir_all(result.repo_path).expect("remove downloaded source");
        });
    }

    #[test]
    fn scoped_source_reports_scope_not_found_without_using_the_root_catalog() {
        tauri::async_runtime::block_on(async {
            let server = tiny_http::Server::http("127.0.0.1:0").expect("server");
            let base_url = format!("http://{}", server.server_addr());
            let expected_root_url = base_url.clone();
            let worker = thread::spawn(move || {
                for expected_path in [
                    "/collections/team/.well-known/agent-skills/index.json",
                    "/collections/team/.well-known/skills/index.json",
                ] {
                    let request = server
                        .recv_timeout(Duration::from_secs(2))
                        .expect("scoped request receive")
                        .expect("scoped index request");
                    assert_eq!(request.url(), expected_path);
                    request
                        .respond(tiny_http::Response::empty(404))
                        .expect("scoped response");
                }

                let root = server
                    .recv_timeout(Duration::from_secs(2))
                    .expect("root request receive")
                    .expect("root index request");
                assert_eq!(root.url(), "/.well-known/agent-skills/index.json");
                root.respond(tiny_http::Response::from_string(
                    r#"{
                      "skills": [
                        {"name":"root-only","description":"Root skill","files":["SKILL.md"]}
                      ]
                    }"#,
                ))
                .expect("root response");

                assert!(server
                    .recv_timeout(Duration::from_millis(300))
                    .expect("artifact request probe")
                    .is_none());
            });
            let http = HttpTransport::new(Arc::new(ProxySettingsStore::new(
                NetworkProxySettings::default(),
            )));

            let error = fetch_wellknown_skills_with_client(
                &http,
                &format!("{base_url}/collections/team?token=secret#private"),
                &CancellationSignal::default(),
            )
            .await
            .expect_err("root catalog must not satisfy a scoped source");

            worker.join().expect("worker");
            assert_eq!(
                error,
                AppError::WellKnownScopeNotFound {
                    scope_path: "/collections/team".to_string(),
                    root_url: expected_root_url,
                }
            );
        });
    }

    #[test]
    fn empty_scoped_catalog_probes_root_before_reporting_scope_not_found() {
        tauri::async_runtime::block_on(async {
            let server = tiny_http::Server::http("127.0.0.1:0").expect("server");
            let base_url = format!("http://{}", server.server_addr());
            let expected_root_url = base_url.clone();
            let worker = thread::spawn(move || {
                let scoped = server
                    .recv_timeout(Duration::from_secs(2))
                    .expect("scoped request receive")
                    .expect("scoped index request");
                assert_eq!(
                    scoped.url(),
                    "/collections/team/.well-known/agent-skills/index.json"
                );
                scoped
                    .respond(tiny_http::Response::from_string(r#"{"skills":[]}"#))
                    .expect("scoped response");

                let scoped_legacy = server
                    .recv_timeout(Duration::from_secs(2))
                    .expect("scoped legacy request receive")
                    .expect("scoped legacy index request");
                assert_eq!(
                    scoped_legacy.url(),
                    "/collections/team/.well-known/skills/index.json"
                );
                scoped_legacy
                    .respond(tiny_http::Response::empty(404))
                    .expect("scoped legacy response");

                let root = server
                    .recv_timeout(Duration::from_secs(2))
                    .expect("root request receive")
                    .expect("root index request");
                assert_eq!(root.url(), "/.well-known/agent-skills/index.json");
                root.respond(tiny_http::Response::from_string(
                    r#"{
                      "skills": [
                        {"name":"root-only","description":"Root skill","files":["SKILL.md"]}
                      ]
                    }"#,
                ))
                .expect("root response");
            });
            let http = HttpTransport::new(Arc::new(ProxySettingsStore::new(
                NetworkProxySettings::default(),
            )));

            let error = fetch_wellknown_skills_with_client(
                &http,
                &format!("{base_url}/collections/team"),
                &CancellationSignal::default(),
            )
            .await
            .expect_err("root catalog should prove the scoped source is absent");

            worker.join().expect("worker");
            assert_eq!(
                error,
                AppError::WellKnownScopeNotFound {
                    scope_path: "/collections/team".to_string(),
                    root_url: expected_root_url,
                }
            );
        });
    }

    #[test]
    fn empty_scoped_v2_catalog_still_allows_a_scoped_legacy_catalog() {
        tauri::async_runtime::block_on(async {
            let server = tiny_http::Server::http("127.0.0.1:0").expect("server");
            let base_url = format!("http://{}", server.server_addr());
            let skill = b"---\nname: demo\ndescription: Demo\n---\n";
            let worker = thread::spawn(move || {
                let scoped_v2 = server
                    .recv_timeout(Duration::from_secs(2))
                    .expect("scoped v2 request receive")
                    .expect("scoped v2 index request");
                scoped_v2
                    .respond(tiny_http::Response::from_string(r#"{"skills":[]}"#))
                    .expect("scoped v2 response");

                let scoped_legacy = server
                    .recv_timeout(Duration::from_secs(2))
                    .expect("scoped legacy request receive")
                    .expect("scoped legacy index request");
                assert_eq!(
                    scoped_legacy.url(),
                    "/collections/team/.well-known/skills/index.json"
                );
                scoped_legacy
                    .respond(tiny_http::Response::from_string(
                        r#"{
                          "skills": [
                            {"name":"demo","description":"Demo","files":["SKILL.md"]}
                          ]
                        }"#,
                    ))
                    .expect("scoped legacy response");

                let artifact = server
                    .recv_timeout(Duration::from_secs(2))
                    .expect("artifact request receive")
                    .expect("artifact request");
                assert_eq!(
                    artifact.url(),
                    "/collections/team/.well-known/skills/demo/SKILL.md"
                );
                artifact
                    .respond(tiny_http::Response::from_data(skill))
                    .expect("artifact response");

                assert!(server
                    .recv_timeout(Duration::from_millis(300))
                    .expect("root request probe")
                    .is_none());
            });
            let http = HttpTransport::new(Arc::new(ProxySettingsStore::new(
                NetworkProxySettings::default(),
            )));

            let result = fetch_wellknown_skills_with_client(
                &http,
                &format!("{base_url}/collections/team"),
                &CancellationSignal::default(),
            )
            .await
            .expect("scoped legacy catalog should remain eligible");

            worker.join().expect("worker");
            assert_eq!(
                fs::read(result.repo_path.join("demo/SKILL.md")).expect("downloaded skill"),
                skill
            );
            fs::remove_dir_all(result.repo_path).expect("remove downloaded source");
        });
    }

    #[test]
    fn update_check_reports_scope_not_found_after_an_empty_scoped_catalog() {
        tauri::async_runtime::block_on(async {
            let server = tiny_http::Server::http("127.0.0.1:0").expect("server");
            let base_url = format!("http://{}", server.server_addr());
            let expected_root_url = base_url.clone();
            let worker = thread::spawn(move || {
                let scoped = server
                    .recv_timeout(Duration::from_secs(2))
                    .expect("scoped request receive")
                    .expect("scoped index request");
                scoped
                    .respond(tiny_http::Response::from_string(r#"{"skills":[]}"#))
                    .expect("scoped response");

                let scoped_legacy = server
                    .recv_timeout(Duration::from_secs(2))
                    .expect("scoped legacy request receive")
                    .expect("scoped legacy index request");
                scoped_legacy
                    .respond(tiny_http::Response::empty(404))
                    .expect("scoped legacy response");

                let root = server
                    .recv_timeout(Duration::from_secs(2))
                    .expect("root request receive")
                    .expect("root index request");
                root.respond(tiny_http::Response::from_string(
                    r#"{
                      "skills": [
                        {"name":"root-only","description":"Root skill","files":["SKILL.md"]}
                      ]
                    }"#,
                ))
                .expect("root response");
            });
            let http = HttpTransport::new(Arc::new(ProxySettingsStore::new(
                NetworkProxySettings::default(),
            )));

            let error = check_wellknown_updates_with_client(
                &http,
                &format!("{base_url}/collections/team"),
                &["demo".to_string()],
                &CancellationSignal::default(),
            )
            .await
            .expect_err("root catalog should prove the scoped source is absent");

            worker.join().expect("worker");
            assert_eq!(
                error,
                AppError::WellKnownScopeNotFound {
                    scope_path: "/collections/team".to_string(),
                    root_url: expected_root_url,
                }
            );
        });
    }

    #[test]
    fn v2_update_check_reads_only_the_index_digest() {
        tauri::async_runtime::block_on(async {
            let server = tiny_http::Server::http("127.0.0.1:0").expect("server");
            let base_url = format!("http://{}", server.server_addr());
            let digest = format!("sha256:{}", "a".repeat(64));
            let expected_digest = digest.clone();
            let worker = thread::spawn(move || {
                let index = server
                    .recv_timeout(Duration::from_secs(2))
                    .expect("index receive")
                    .expect("index request");
                assert_eq!(index.url(), "/.well-known/agent-skills/index.json");
                index
                    .respond(tiny_http::Response::from_string(format!(
                        r#"{{
                          "$schema": "{DISCOVERY_SCHEMA_V2}",
                          "skills": [{{
                            "name": "demo",
                            "description": "Demo",
                            "type": "skill-md",
                            "url": "demo/SKILL.md",
                            "digest": "{digest}"
                          }}]
                        }}"#
                    )))
                    .expect("index response");
                assert!(server
                    .recv_timeout(Duration::from_millis(300))
                    .expect("artifact probe")
                    .is_none());
            });
            let http = HttpTransport::new(Arc::new(ProxySettingsStore::new(
                NetworkProxySettings::default(),
            )));

            let evidence = check_wellknown_updates_with_client(
                &http,
                &base_url,
                &["demo".to_string()],
                &CancellationSignal::default(),
            )
            .await
            .expect("v2 evidence");

            worker.join().expect("worker");
            assert_eq!(evidence.complete_skill_catalog, vec!["demo"]);
            assert_eq!(evidence.digests.get("demo"), Some(&expected_digest));
        });
    }

    #[test]
    fn legacy_update_check_downloads_only_requested_installed_skills() {
        tauri::async_runtime::block_on(async {
            let server = tiny_http::Server::http("127.0.0.1:0").expect("server");
            let base_url = format!("http://{}", server.server_addr());
            let beta = b"---\nname: beta\ndescription: Beta\n---\n";
            let worker = thread::spawn(move || {
                let index = server
                    .recv_timeout(Duration::from_secs(2))
                    .expect("index receive")
                    .expect("index request");
                index
                    .respond(tiny_http::Response::from_string(
                        r#"{
                          "skills": [
                            {"name":"alpha","description":"Alpha","files":["SKILL.md"]},
                            {"name":"beta","description":"Beta","files":["SKILL.md"]}
                          ]
                        }"#,
                    ))
                    .expect("index response");

                let artifact = server
                    .recv_timeout(Duration::from_secs(2))
                    .expect("artifact receive")
                    .expect("requested artifact");
                assert_eq!(artifact.url(), "/.well-known/agent-skills/beta/SKILL.md");
                artifact
                    .respond(tiny_http::Response::from_data(beta))
                    .expect("artifact response");
                assert!(server
                    .recv_timeout(Duration::from_millis(300))
                    .expect("unexpected request probe")
                    .is_none());
            });
            let http = HttpTransport::new(Arc::new(ProxySettingsStore::new(
                NetworkProxySettings::default(),
            )));

            let evidence = check_wellknown_updates_with_client(
                &http,
                &base_url,
                &["beta".to_string()],
                &CancellationSignal::default(),
            )
            .await
            .expect("legacy evidence");

            worker.join().expect("worker");
            assert_eq!(evidence.complete_skill_catalog, vec!["alpha", "beta"]);
            assert_eq!(evidence.digests.len(), 1);
            assert!(evidence.digests.contains_key("beta"));
        });
    }

    #[test]
    fn well_known_source_uses_the_configured_custom_proxy() {
        tauri::async_runtime::block_on(async {
            let proxy = tiny_http::Server::http("127.0.0.1:0").expect("well-known proxy");
            let proxy_url = format!("http://{}", proxy.server_addr());
            let base_url = "http://127.0.0.1:45678";
            let skill = b"---\nname: demo\ndescription: Demo\n---\n";
            let digest = compute_digest(skill);
            let worker = thread::spawn(move || {
                let index = proxy
                    .recv_timeout(Duration::from_secs(2))
                    .expect("proxy index receive")
                    .expect("proxied well-known index");
                assert_eq!(
                    index.url(),
                    "http://127.0.0.1:45678/.well-known/agent-skills/index.json"
                );
                index
                    .respond(tiny_http::Response::from_string(format!(
                        r#"{{
                          "$schema": "{DISCOVERY_SCHEMA_V2}",
                          "skills": [{{
                            "name": "demo",
                            "description": "Demo",
                            "type": "skill-md",
                            "url": "demo/SKILL.md",
                            "digest": "{digest}"
                          }}]
                        }}"#
                    )))
                    .expect("proxy index response");

                let artifact = proxy
                    .recv_timeout(Duration::from_secs(2))
                    .expect("proxy artifact receive")
                    .expect("proxied well-known artifact");
                assert_eq!(
                    artifact.url(),
                    "http://127.0.0.1:45678/.well-known/agent-skills/demo/SKILL.md"
                );
                artifact
                    .respond(tiny_http::Response::from_data(skill))
                    .expect("proxy artifact response");
            });
            let http =
                HttpTransport::new(Arc::new(ProxySettingsStore::new(NetworkProxySettings {
                    mode: ProxyMode::Custom,
                    custom_proxy_url: Some(proxy_url),
                    ..NetworkProxySettings::default()
                })));

            let result =
                fetch_wellknown_skills_with_client(&http, base_url, &CancellationSignal::default())
                    .await
                    .expect("proxied well-known fetch");

            worker.join().expect("well-known proxy worker");
            assert_eq!(
                fs::read(result.repo_path.join("demo/SKILL.md")).expect("downloaded skill"),
                skill
            );
            fs::remove_dir_all(result.repo_path).expect("remove downloaded source");
        });
    }

    #[test]
    fn test_download_v2_entry_returns_http_reason() {
        tauri::async_runtime::block_on(async {
            let temp = tempfile::tempdir().unwrap();
            let artifact_url = spawn_response_server(
                "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            );
            let http =
                HttpTransport::new(Arc::new(ProxySettingsStore::new(NetworkProxySettings {
                    mode: ProxyMode::Direct,
                    ..NetworkProxySettings::default()
                })));
            let cancellation = CancellationSignal::default();
            let context = WellKnownDownloadContext {
                http: &http,
                temp_path: temp.path(),
                cancellation: &cancellation,
                operation_id: "well-known-test",
            };
            let err = download_v2_entry(
                &context,
                "demo",
                "skill-md",
                &artifact_url,
                &format!("sha256:{}", "0".repeat(64)),
            )
            .await
            .expect_err("non-success artifact response should be reported");

            assert!(matches!(
                err,
                AppError::WellKnownSourceFailed {
                    reason: crate::error::SourceAcquisitionFailureReason::NotFound,
                }
            ));
        });
    }

    #[test]
    fn test_legacy_download_preserves_cancellation() {
        tauri::async_runtime::block_on(async {
            let temp = tempfile::tempdir().unwrap();
            let http =
                HttpTransport::new(Arc::new(ProxySettingsStore::new(NetworkProxySettings {
                    mode: ProxyMode::Direct,
                    ..NetworkProxySettings::default()
                })));
            let cancellation = CancellationSignal::default();
            cancellation.cancel();
            let context = WellKnownDownloadContext {
                http: &http,
                temp_path: temp.path(),
                cancellation: &cancellation,
                operation_id: "well-known-cancel-test",
            };

            let error = download_legacy_entry(
                &context,
                "demo",
                &["SKILL.md".to_string()],
                "https://example.com/.well-known/agent-skills",
            )
            .await
            .expect_err("cancelled legacy download");

            assert_eq!(error, AppError::MutationCancelled);
        });
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

        // legacy skills path-relative
        assert_eq!(
            candidates[1].index_url,
            "https://example.com/docs/.well-known/skills/index.json"
        );

        // agent-skills root probe
        assert_eq!(
            candidates[2].index_url,
            "https://example.com/.well-known/agent-skills/index.json"
        );

        // legacy skills root
        assert_eq!(
            candidates[3].index_url,
            "https://example.com/.well-known/skills/index.json"
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
        assert_eq!(
            candidates[1].index_url,
            "https://example.com/.well-known/skills/index.json"
        );
    }

    #[test]
    fn test_build_index_urls_keep_scope_before_root_and_new_protocol_before_legacy() {
        let candidates = build_index_urls("https://example.com/app");
        assert_eq!(
            candidates
                .iter()
                .map(|candidate| candidate.index_url.as_str())
                .collect::<Vec<_>>(),
            vec![
                "https://example.com/app/.well-known/agent-skills/index.json",
                "https://example.com/app/.well-known/skills/index.json",
                "https://example.com/.well-known/agent-skills/index.json",
                "https://example.com/.well-known/skills/index.json",
            ]
        );
    }

    #[test]
    fn maps_well_known_transport_failures_to_stable_reasons() {
        use crate::error::SourceAcquisitionFailureReason;

        assert_eq!(
            source_failure_reason_from_http_error(&HttpTransportError::Request {
                stage: "request",
                reason: "timeout",
            }),
            SourceAcquisitionFailureReason::Timeout
        );
        assert_eq!(
            source_failure_reason_from_http_error(&HttpTransportError::Request {
                stage: "request",
                reason: "request-failed",
            }),
            SourceAcquisitionFailureReason::Network
        );
        assert_eq!(
            source_failure_reason_from_http_error(&HttpTransportError::ResponseTooLarge),
            SourceAcquisitionFailureReason::LimitExceeded
        );
    }
}

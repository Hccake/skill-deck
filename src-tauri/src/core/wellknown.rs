//! Well-Known Skills protocol support (RFC 8615 `.well-known/agent-skills`).
//!
//! Implements discovery of skills hosted under the `.well-known/agent-skills/`
//! path on websites, with fallback to the legacy `.well-known/skills/` path.
//! This is the Rust equivalent of the CLI's `providers/wellknown.ts`.

use crate::error::AppError;
use reqwest::Client;
use serde::Deserialize;
use std::fs;
use std::path::PathBuf;
use std::time::Duration;
use url::Url;

const WELL_KNOWN_PATHS: &[&str] = &[".well-known/agent-skills", ".well-known/skills"];
const INDEX_FILE: &str = "index.json";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

// ---------------------------------------------------------------------------
// Data structures
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
pub struct WellKnownIndex {
    pub skills: Vec<WellKnownSkillEntry>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WellKnownSkillEntry {
    pub name: String,
    pub description: String,
    pub files: Vec<String>,
}

/// Result of a successful well-known skill fetch — carries the local path
/// where files were downloaded and an identifier suitable for lock-file storage.
#[derive(Debug, Clone)]
pub struct WellKnownFetchResult {
    pub repo_path: PathBuf,
    #[allow(dead_code)]
    pub source_identifier: String,
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

struct IndexUrlCandidate {
    index_url: String,
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

/// Validate a single skill entry from the index.
fn validate_skill_entry(entry: &WellKnownSkillEntry) -> Result<(), AppError> {
    if entry.name.trim().is_empty() {
        return Err(AppError::InvalidSource {
            value: "Skill name must not be empty".into(),
        });
    }
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
        if file.starts_with('/') || file.starts_with('\\') {
            return Err(AppError::InvalidSource {
                value: format!("Absolute path not allowed: {file}"),
            });
        }
        let normalized = file.replace('\\', "/");
        if normalized.split('/').any(|seg| seg == "..") {
            return Err(AppError::InvalidSource {
                value: format!("Path traversal not allowed: {file}"),
            });
        }
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

    let (index, base_url) = fetch_index(&client, url).await?;

    let valid_entries: Vec<&WellKnownSkillEntry> = index
        .skills
        .iter()
        .filter(|e| validate_skill_entry(e).is_ok())
        .collect();

    if valid_entries.is_empty() {
        return Err(AppError::NoSkillsFound);
    }

    let temp_path = tempfile::TempDir::new()?.keep();

    for entry in &valid_entries {
        let skill_dir = temp_path.join(&entry.name);
        fs::create_dir_all(&skill_dir)?;

        let mut skill_ok = true;

        for file_path in &entry.files {
            let file_url = format!("{base_url}/{}/{file_path}", entry.name);

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
    }

    Ok(WellKnownFetchResult {
        repo_path: temp_path,
        source_identifier,
    })
}

/// Try each candidate index URL in order; return the first that responds with
/// a non-empty skills list, together with its `base_url` (which already
/// includes the matched well-known path, e.g. `.well-known/agent-skills`).
async fn fetch_index(client: &Client, url: &str) -> Result<(WellKnownIndex, String), AppError> {
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

        let index: WellKnownIndex = match resp.json().await {
            Ok(idx) => idx,
            Err(_) => continue,
        };

        if !index.skills.is_empty() {
            return Ok((index, candidate.base_url.clone()));
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

    #[test]
    fn test_parse_valid_index() {
        let json = r#"{ "skills": [{ "name": "my-skill", "description": "A skill", "files": ["SKILL.md"] }] }"#;
        let index: WellKnownIndex = serde_json::from_str(json).unwrap();
        assert_eq!(index.skills.len(), 1);
        assert_eq!(index.skills[0].name, "my-skill");
        assert_eq!(index.skills[0].files, vec!["SKILL.md"]);
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
    fn test_validate_entry_path_traversal_no_false_positive() {
        let entry = WellKnownSkillEntry {
            name: "ok".into(),
            description: "Has double-dot in filename".into(),
            files: vec!["SKILL.md".into(), "my..file.txt".into()],
        };
        assert!(validate_skill_entry(&entry).is_ok());
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

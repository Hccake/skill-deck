use crate::core::builtin_agent_catalog::eve_agent_id;
use crate::core::skill::sanitize_name;
use crate::core::skill_payload::{
    verify_skill_payload_integrity, PayloadEntry, PayloadEntryKind, SkillPayload,
    SkillPayloadManifest,
};
use crate::error::AppError;
use crate::models::InstallTargetInfo;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PackageJson {
    #[serde(default)]
    dependencies: std::collections::BTreeMap<String, serde_json::Value>,
    #[serde(default)]
    dev_dependencies: std::collections::BTreeMap<String, serde_json::Value>,
}

pub const EVE_SUBAGENTS_DIR: &str = "agent/subagents";

pub fn is_eve_project(cwd: &str) -> bool {
    let root = Path::new(cwd);
    if !root.join("agent").is_dir() {
        return false;
    }

    let Ok(raw) = std::fs::read_to_string(root.join("package.json")) else {
        return false;
    };
    let Ok(package_json) = serde_json::from_str::<PackageJson>(&raw) else {
        return false;
    };

    package_json.dependencies.contains_key("eve")
        || package_json.dev_dependencies.contains_key("eve")
}

pub fn eve_subagents_dir(cwd: &str) -> PathBuf {
    Path::new(cwd).join(EVE_SUBAGENTS_DIR)
}

pub fn list_eve_subagents(cwd: &str) -> Vec<String> {
    let dir = eve_subagents_dir(cwd);
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };

    let mut names: Vec<String> = entries
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().map(|ty| ty.is_dir()).unwrap_or(false))
        .filter_map(|entry| entry.file_name().to_str().map(ToOwned::to_owned))
        .collect();
    names.sort();
    names
}

pub fn eve_root_skills_dir(cwd: &str) -> PathBuf {
    Path::new(cwd).join("agent").join("skills")
}

pub fn eve_subagent_skills_dir(cwd: &str, subagent: &str) -> PathBuf {
    Path::new(cwd)
        .join("agent")
        .join("subagents")
        .join(sanitize_name(subagent))
        .join("skills")
}

pub fn eve_target_id(subagent: Option<&str>) -> String {
    match subagent {
        Some(name) if !name.is_empty() => format!("eve:{}", name),
        _ => "eve:root".to_string(),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EveTargetRef<'a> {
    Root,
    Subagent(&'a str),
}

pub fn parse_eve_target_id(target_id: &str) -> Option<EveTargetRef<'_>> {
    if target_id == "eve:root" {
        return Some(EveTargetRef::Root);
    }
    target_id
        .strip_prefix("eve:")
        .filter(|subagent| !subagent.is_empty())
        .map(EveTargetRef::Subagent)
}

pub fn eve_target_label(subagent: Option<&str>) -> String {
    match subagent {
        Some(name) if !name.is_empty() => format!("Eve ({})", name),
        _ => "Eve (root)".to_string(),
    }
}

pub fn eve_skills_dir_for_target(cwd: &str, subagent: Option<&str>) -> PathBuf {
    match subagent {
        Some(name) if !name.is_empty() => eve_subagent_skills_dir(cwd, name),
        _ => eve_root_skills_dir(cwd),
    }
}

pub fn lock_subagent_value(subagent: Option<&str>) -> String {
    subagent.unwrap_or("").to_string()
}

pub fn eve_install_targets_for_project(cwd: &str) -> Vec<InstallTargetInfo> {
    if !is_eve_project(cwd) {
        return Vec::new();
    }

    eve_install_targets(cwd, list_eve_subagents(cwd))
}

pub fn eve_install_targets(
    project_path: &str,
    subagents: impl IntoIterator<Item = String>,
) -> Vec<InstallTargetInfo> {
    let project_path = project_path.trim_end_matches(['/', '\\']);

    let mut targets = vec![InstallTargetInfo {
        target_id: eve_target_id(None),
        agent: eve_agent_id(),
        display_name: eve_target_label(None),
        subagent: None,
        path: eve_skills_dir_for_target(project_path, None)
            .to_string_lossy()
            .to_string(),
    }];

    targets.extend(subagents.into_iter().map(|subagent| {
        let subagent = lock_subagent_value(Some(&subagent));
        InstallTargetInfo {
            target_id: eve_target_id(Some(&subagent)),
            agent: eve_agent_id(),
            display_name: eve_target_label(Some(&subagent)),
            path: eve_skills_dir_for_target(project_path, Some(&subagent))
                .to_string_lossy()
                .to_string(),
            subagent: Some(subagent),
        }
    }));

    targets
}

pub fn normalize_eve_skill_md(raw: &str) -> String {
    let Some(rest) = raw.strip_prefix("---") else {
        return raw.to_string();
    };
    let Some(end) = rest.find("\n---") else {
        return raw.to_string();
    };

    let yaml_content = rest[..end].trim_start_matches(['\r', '\n']);
    let content = rest[end + "\n---".len()..]
        .trim_start_matches(['\r', '\n'])
        .to_string();

    let Ok(value) = serde_yaml::from_str::<serde_yaml::Value>(yaml_content) else {
        return raw.to_string();
    };
    let Some(mapping) = value.as_mapping() else {
        return content;
    };

    let mut kept = serde_yaml::Mapping::new();
    for key in ["description", "license"] {
        if let Some(value) = mapping
            .get(serde_yaml::Value::String(key.to_string()))
            .and_then(|value| value.as_str())
        {
            kept.insert(
                serde_yaml::Value::String(key.to_string()),
                serde_yaml::Value::String(value.to_string()),
            );
        }
    }

    if let Some(metadata) = mapping
        .get(serde_yaml::Value::String("metadata".to_string()))
        .and_then(|value| value.as_mapping())
    {
        let mut meta = serde_yaml::Mapping::new();
        for (key, value) in metadata {
            if key.as_str().is_some() && value.as_str().is_some() {
                meta.insert(key.clone(), value.clone());
            }
        }
        if !meta.is_empty() {
            kept.insert(
                serde_yaml::Value::String("metadata".to_string()),
                serde_yaml::Value::Mapping(meta),
            );
        }
    }

    if kept.is_empty() {
        return content;
    }

    let yaml = serde_yaml::to_string(&kept).unwrap_or_default();
    format!("---\n{}---\n{}", yaml, content)
}

pub fn derive_eve_skill_payload(canonical: &SkillPayload) -> Result<SkillPayload, AppError> {
    verify_skill_payload_integrity(canonical)?;
    let skill_entry = canonical
        .entries
        .iter()
        .filter(|entry| {
            entry.kind == PayloadEntryKind::File
                && !entry.relative_path.contains('/')
                && entry.relative_path.eq_ignore_ascii_case("SKILL.md")
        })
        .min_by_key(|entry| (entry.relative_path != "SKILL.md", &entry.relative_path))
        .ok_or_else(|| AppError::InvalidSkillMd {
            message: "canonical payload does not contain a root SKILL.md".to_string(),
        })?;
    let raw = canonical
        .blobs
        .get(
            skill_entry
                .blob_id
                .as_deref()
                .ok_or(AppError::StalePayload)?,
        )
        .ok_or(AppError::StalePayload)?;
    let raw = std::str::from_utf8(raw).map_err(|error| AppError::InvalidSkillMd {
        message: format!("SKILL.md is not valid UTF-8: {error}"),
    })?;
    let normalized = normalize_eve_skill_md(raw).into_bytes();
    let normalized_hash = format!("{:x}", Sha256::digest(&normalized));

    let mut entries = canonical
        .entries
        .iter()
        .filter(|entry| {
            entry.relative_path.contains('/')
                || !entry.relative_path.eq_ignore_ascii_case("SKILL.md")
        })
        .cloned()
        .collect::<Vec<_>>();
    entries.push(PayloadEntry {
        relative_path: "SKILL.md".to_string(),
        kind: PayloadEntryKind::File,
        blob_id: Some(normalized_hash.clone()),
        content_hash: Some(normalized_hash.clone()),
        size: normalized.len() as u64,
        executable: false,
    });
    let manifest = SkillPayloadManifest::from_entries(entries)?;
    let mut blobs = BTreeMap::new();
    for entry in &manifest.entries {
        let Some(blob_id) = entry.blob_id.as_deref() else {
            continue;
        };
        let blob = if blob_id == normalized_hash {
            normalized.clone()
        } else {
            canonical
                .blobs
                .get(blob_id)
                .cloned()
                .ok_or(AppError::StalePayload)?
        };
        blobs.entry(blob_id.to_string()).or_insert(blob);
    }
    let payload = SkillPayload {
        entries: manifest.entries,
        blobs,
        payload_root_hash: manifest.payload_root_hash,
        payload_id: manifest.payload_id,
    };
    verify_skill_payload_integrity(&payload)?;
    Ok(payload)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn detects_eve_project_from_agent_dir_and_dependency() {
        let temp = tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join("agent")).unwrap();
        std::fs::write(
            temp.path().join("package.json"),
            r#"{"dependencies":{"eve":"^0.11.5"}}"#,
        )
        .unwrap();

        assert!(is_eve_project(&temp.path().to_string_lossy()));
    }

    #[test]
    fn rejects_project_without_eve_dependency() {
        let temp = tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join("agent")).unwrap();
        std::fs::write(
            temp.path().join("package.json"),
            r#"{"dependencies":{"react":"19.0.0"}}"#,
        )
        .unwrap();

        assert!(!is_eve_project(&temp.path().to_string_lossy()));
    }

    #[test]
    fn lists_eve_subagents_sorted_and_ignores_files() {
        let temp = tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join("agent/subagents/writer")).unwrap();
        std::fs::create_dir_all(temp.path().join("agent/subagents/research")).unwrap();
        std::fs::write(temp.path().join("agent/subagents/notes.txt"), "x").unwrap();

        assert_eq!(
            list_eve_subagents(&temp.path().to_string_lossy()),
            vec!["research".to_string(), "writer".to_string()]
        );
    }

    #[test]
    fn eve_target_helpers_map_root_and_named_subagent() {
        assert_eq!(eve_target_id(None), "eve:root");
        assert_eq!(eve_target_id(Some("research")), "eve:research");
        assert_eq!(eve_target_label(None), "Eve (root)");
        assert_eq!(eve_target_label(Some("research")), "Eve (research)");
        assert_eq!(lock_subagent_value(None), "");
        assert_eq!(lock_subagent_value(Some("research")), "research");
        assert_eq!(parse_eve_target_id("eve:root"), Some(EveTargetRef::Root));
        assert_eq!(
            parse_eve_target_id("eve:research"),
            Some(EveTargetRef::Subagent("research"))
        );
        assert_eq!(parse_eve_target_id("cursor:root"), None);
    }

    #[test]
    fn eve_install_targets_include_root_and_discovered_subagents() {
        let temp = tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join("agent/subagents/research")).unwrap();
        std::fs::write(
            temp.path().join("package.json"),
            r#"{"dependencies":{"eve":"^0.11.5"}}"#,
        )
        .unwrap();

        let targets = eve_install_targets_for_project(&temp.path().to_string_lossy());
        assert_eq!(targets.len(), 2);
        assert_eq!(targets[0].target_id, "eve:root");
        assert_eq!(targets[1].target_id, "eve:research");
        assert_eq!(
            targets[1].path,
            temp.path()
                .join("agent")
                .join("subagents")
                .join("research")
                .join("skills")
                .to_string_lossy()
                .to_string()
        );
    }

    #[test]
    fn eve_skill_md_normalizer_removes_unsupported_name_field() {
        let raw = "---\nname: demo\ndescription: Demo\nlicense: MIT\nmetadata:\n  keep: value\n  skip: 1\n---\n# Demo\n";
        let normalized = normalize_eve_skill_md(raw);

        assert!(!normalized.contains("name:"));
        assert!(normalized.contains("description: Demo"));
        assert!(normalized.contains("license: MIT"));
        assert!(normalized.contains("keep: value"));
        assert!(!normalized.contains("skip: 1"));
        assert!(normalized.contains("# Demo"));
    }

    #[test]
    fn eve_payload_derivation_changes_only_root_skill_md() {
        let temp = tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join("scripts")).unwrap();
        std::fs::write(
            temp.path().join("SKILL.md"),
            "---\nname: demo\ndescription: Demo\n---\n# Demo\n",
        )
        .unwrap();
        std::fs::write(temp.path().join("scripts/run.sh"), "#!/bin/sh\necho demo\n").unwrap();
        let canonical = crate::core::skill_payload::build_skill_payload(temp.path()).unwrap();
        let canonical_id = canonical.payload_id.clone();

        let derived = derive_eve_skill_payload(&canonical).unwrap();

        assert_eq!(canonical.payload_id, canonical_id);
        assert_ne!(derived.payload_id, canonical.payload_id);
        let skill_entry = derived
            .entries
            .iter()
            .find(|entry| entry.relative_path == "SKILL.md")
            .unwrap();
        let skill_md = std::str::from_utf8(
            derived
                .blobs
                .get(skill_entry.blob_id.as_deref().unwrap())
                .unwrap(),
        )
        .unwrap();
        assert!(!skill_md.contains("name:"));
        assert!(skill_md.contains("description: Demo"));
        let script_entry = derived
            .entries
            .iter()
            .find(|entry| entry.relative_path == "scripts/run.sh")
            .unwrap();
        assert_eq!(
            derived
                .blobs
                .get(script_entry.blob_id.as_deref().unwrap())
                .unwrap(),
            b"#!/bin/sh\necho demo\n"
        );
        crate::core::skill_payload::verify_skill_payload_integrity(&derived).unwrap();
    }
}

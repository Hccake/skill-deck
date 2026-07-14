use crate::core::agents::AgentType;
use crate::core::skill::sanitize_name;
use crate::models::InstallTargetInfo;
use serde::{Deserialize, Serialize};
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

    let mut targets = vec![InstallTargetInfo {
        target_id: eve_target_id(None),
        agent: AgentType::Eve,
        display_name: eve_target_label(None),
        subagent: None,
        path: eve_skills_dir_for_target(cwd, None)
            .to_string_lossy()
            .to_string(),
    }];

    targets.extend(list_eve_subagents(cwd).into_iter().map(|subagent| {
        let subagent = lock_subagent_value(Some(&subagent));
        InstallTargetInfo {
            target_id: eve_target_id(Some(&subagent)),
            agent: AgentType::Eve,
            display_name: eve_target_label(Some(&subagent)),
            path: eve_skills_dir_for_target(cwd, Some(&subagent))
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

pub fn paths_overlap(left: &Path, right: &Path) -> bool {
    let normalize = |path: &Path| {
        path.canonicalize().unwrap_or_else(|_| {
            let mut normalized = PathBuf::new();
            for component in path.components() {
                match component {
                    std::path::Component::CurDir => {}
                    std::path::Component::ParentDir => {
                        normalized.pop();
                    }
                    other => normalized.push(other.as_os_str()),
                }
            }
            normalized
        })
    };

    let left = normalize(left);
    let right = normalize(right);
    left.starts_with(&right) || right.starts_with(&left)
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
                .join("agent/subagents/research/skills")
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
    fn paths_overlap_when_source_is_target_or_parent() {
        let temp = tempdir().unwrap();
        let source = temp.path().join("skills/demo");
        let child = source.join("nested");
        std::fs::create_dir_all(&child).unwrap();

        assert!(paths_overlap(&source, &source));
        assert!(paths_overlap(&source, &child));
        assert!(!paths_overlap(&source, &temp.path().join("other")));
    }
}

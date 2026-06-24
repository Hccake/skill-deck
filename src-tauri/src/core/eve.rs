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
}

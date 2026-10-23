use crate::error::AppError;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdateCandidate {
    pub path: String,
    pub plugin_name: Option<String>,
}

/// 同一来源快照的完整候选；名称保持原样，同名路径全部保留。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdateLocationCatalog {
    pub by_name: BTreeMap<String, Vec<UpdateCandidate>>,
}

impl UpdateLocationCatalog {
    pub fn from_documents(documents: Vec<(String, Vec<u8>)>) -> Result<Self, AppError> {
        use crate::core::discovery::{
            is_in_cli_agent_project_skill_dir, is_locked_project_skill, locked_project_skill_names,
        };
        use std::path::Path;
        let text = documents
            .into_iter()
            .map(|(path, bytes)| {
                String::from_utf8(bytes)
                    .map(|content| (path, content))
                    .map_err(|_| incomplete())
            })
            .collect::<Result<BTreeMap<_, _>, _>>()?;
        let lock = text.get("skills-lock.json");
        if let Some(lock) = lock {
            serde_json::from_str::<serde_json::Value>(lock).map_err(|_| incomplete())?;
        }
        let locked = locked_project_skill_names(lock.map(String::as_str));
        let plugins = crate::core::plugin_manifest::get_relative_plugin_groupings(
            text.get(".claude-plugin/marketplace.json")
                .map(String::as_str),
            text.get(".claude-plugin/plugin.json").map(String::as_str),
        );
        let mut catalog = Self::default();
        let mut folders = std::collections::BTreeSet::new();
        for (path, content) in text {
            let Some(file) = path.rsplit('/').next() else {
                continue;
            };
            if !file.eq_ignore_ascii_case("SKILL.md") {
                continue;
            }
            if path.starts_with('/')
                || path.contains('\\')
                || path.split('/').any(|part| matches!(part, "" | "." | ".."))
            {
                return Err(incomplete());
            }
            let folder = crate::core::skill_paths::normalize_skill_folder_path(&path);
            if !folders.insert(folder.clone()) {
                return Err(incomplete());
            }
            let metadata =
                crate::core::skill::parse_skill_md_content(&content).map_err(|_| incomplete())?;
            if is_in_cli_agent_project_skill_dir(Path::new(&folder))
                && is_locked_project_skill(&metadata.name, Path::new(&folder), &locked)
            {
                continue;
            }
            let plugin_name = plugins.get(Path::new(&folder)).cloned();
            catalog
                .by_name
                .entry(metadata.name)
                .or_default()
                .push(UpdateCandidate { path, plugin_name });
        }
        Ok(catalog)
    }

    pub fn candidates(&self, name: &str) -> &[UpdateCandidate] {
        self.by_name
            .get(name)
            .map(Vec::as_slice)
            .unwrap_or_default()
    }

    pub fn candidate_at(&self, path: &str) -> Option<&UpdateCandidate> {
        let folder = crate::core::skill_paths::normalize_skill_folder_path(path);
        self.by_name.values().flatten().find(|candidate| {
            crate::core::skill_paths::normalize_skill_folder_path(&candidate.path) == folder
        })
    }
}

pub(crate) fn incomplete() -> AppError {
    AppError::InvalidSkillMd {
        message: "Source candidate metadata could not be read completely".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn skill(path: &str, name: &str) -> (String, Vec<u8>) {
        (
            path.into(),
            format!("---\nname: {name}\ndescription: test\n---\nbody").into_bytes(),
        )
    }

    #[test]
    fn relocation_catalog_preserves_duplicates_and_exact_names() {
        let catalog = UpdateLocationCatalog::from_documents(vec![
            skill("a/SKILL.md", "Code Review"),
            skill("b/SKILL.md", "Code Review"),
            skill("c/SKILL.md", "code-review"),
            skill("d/SKILL.md", "code_review"),
        ])
        .unwrap();
        assert_eq!(catalog.by_name["Code Review"].len(), 2);
        assert_eq!(catalog.by_name["code-review"][0].path, "c/SKILL.md");
        assert_eq!(catalog.by_name["code_review"][0].path, "d/SKILL.md");
        assert!(!catalog.by_name.contains_key("absent"));
    }

    #[test]
    fn relocation_catalog_rejects_unreadable_metadata_before_claiming_uniqueness() {
        let documents = vec![
            skill("a/SKILL.md", "demo"),
            ("b/SKILL.md".into(), b"not frontmatter".to_vec()),
        ];
        assert!(UpdateLocationCatalog::from_documents(documents).is_err());
    }

    #[test]
    fn relocation_catalog_excludes_locked_installs_and_retains_plugin_metadata() {
        let catalog = UpdateLocationCatalog::from_documents(vec![
            skill(".agents/skills/demo/SKILL.md", "demo"),
            skill("tools/demo/SKILL.md", "demo"),
            (
                "skills-lock.json".into(),
                br#"{"skills":{"demo":{}}}"#.to_vec(),
            ),
            (
                ".claude-plugin/plugin.json".into(),
                br#"{"name":"tools","skills":["./tools/demo"]}"#.to_vec(),
            ),
        ])
        .unwrap();
        assert_eq!(
            catalog.by_name["demo"],
            vec![UpdateCandidate {
                path: "tools/demo/SKILL.md".into(),
                plugin_name: Some("tools".into())
            }]
        );
    }
}

use std::path::{Path, PathBuf};

pub fn find_skill_md_case_insensitive(dir: &Path) -> Option<PathBuf> {
    let entries = std::fs::read_dir(dir).ok()?;
    let mut fallback = None;
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }

        let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };

        if file_name == "SKILL.md" {
            return Some(path);
        }

        if file_name.eq_ignore_ascii_case("SKILL.md") {
            fallback.get_or_insert(path);
        }
    }

    fallback
}

pub fn normalize_skill_folder_path(skill_path: &str) -> String {
    let normalized = skill_path.replace('\\', "/");
    let mut trimmed = normalized.trim_matches('/').to_string();
    let lower = trimmed.to_lowercase();

    if lower == "skill.md" {
        return String::new();
    }

    if lower.ends_with("/skill.md") {
        let new_len = trimmed.len() - "/skill.md".len();
        trimmed.truncate(new_len);
    }

    trimmed.trim_end_matches('/').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn finds_skill_md_case_insensitively() {
        let temp = tempdir().unwrap();
        std::fs::write(temp.path().join("skill.md"), "---\nname: demo\n---\n").unwrap();

        let found = find_skill_md_case_insensitive(temp.path()).unwrap();

        assert_eq!(found.file_name().unwrap(), "skill.md");
    }

    #[test]
    fn normalizes_skill_md_suffix_case_insensitively() {
        assert_eq!(
            normalize_skill_folder_path("skills/demo/SKILL.md"),
            "skills/demo"
        );
        assert_eq!(
            normalize_skill_folder_path("skills/demo/skill.md"),
            "skills/demo"
        );
        assert_eq!(
            normalize_skill_folder_path("skills\\demo\\Skill.md"),
            "skills/demo"
        );
        assert_eq!(normalize_skill_folder_path("Skill.md"), "");
    }
}

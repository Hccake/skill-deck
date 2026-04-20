#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateSourceParts {
    pub source_type: String,
    pub source_url: String,
    pub ref_name: Option<String>,
    pub skill_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateTarget {
    pub fetch_source_url: String,
    pub git_ref: Option<String>,
    pub discover_subpath: Option<String>,
}

pub type UpdateGroupKey = (String, String, Option<String>);

pub fn build_update_target(parts: UpdateSourceParts) -> UpdateTarget {
    UpdateTarget {
        fetch_source_url: parts.source_url,
        git_ref: parts.ref_name,
        discover_subpath: parts.skill_path.as_deref().and_then(strip_skill_md_suffix),
    }
}

pub fn build_update_group_key(
    source_type: &str,
    source_url: &str,
    ref_name: Option<&str>,
) -> UpdateGroupKey {
    (
        source_type.to_string(),
        source_url.to_string(),
        ref_name.map(str::to_string),
    )
}

fn strip_skill_md_suffix(path: &str) -> Option<String> {
    let mut skill_folder = path.replace('\\', "/");

    if skill_folder.ends_with("/SKILL.md") {
        skill_folder.truncate(skill_folder.len() - 9);
    } else if skill_folder.ends_with("SKILL.md") {
        skill_folder.truncate(skill_folder.len() - 8);
    }

    let skill_folder = skill_folder.trim_end_matches('/').to_string();
    if skill_folder.is_empty() {
        None
    } else {
        Some(skill_folder)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_update_target_preserves_gitlab_source_url_and_ref() {
        let target = build_update_target(UpdateSourceParts {
            source_type: "gitlab".to_string(),
            source_url: "https://gitlab.com/group/repo".to_string(),
            ref_name: Some("feature/my-branch".to_string()),
            skill_path: Some("skills/demo/SKILL.md".to_string()),
        });

        assert_eq!(target.fetch_source_url, "https://gitlab.com/group/repo");
        assert_eq!(target.git_ref.as_deref(), Some("feature/my-branch"));
        assert_eq!(target.discover_subpath.as_deref(), Some("skills/demo"));
    }

    #[test]
    fn test_build_update_group_key_distinguishes_refs() {
        assert_ne!(
            build_update_group_key("github", "https://github.com/owner/repo", Some("main")),
            build_update_group_key("github", "https://github.com/owner/repo", Some("dev"))
        );
    }
}

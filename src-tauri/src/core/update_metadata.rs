use crate::core::local_lock::LocalSkillLockEntry;
use crate::core::skill_lock::SkillLockEntry;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NormalizedUpdateMetadata {
    pub source: String,
    pub source_type: String,
    pub source_url: Option<String>,
    pub ref_name: Option<String>,
    pub skill_path: Option<String>,
    pub remote_hash: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateCapability {
    pub can_check_for_updates: bool,
    pub can_run_update: bool,
    pub reason: Option<String>,
}

pub fn recover_source_url(
    source: &str,
    source_type: &str,
    source_url: Option<&str>,
) -> Option<String> {
    source_url
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .or_else(|| {
            if source.is_empty() {
                None
            } else if source_type == "github" {
                Some(format!("https://github.com/{}", source))
            } else {
                Some(source.to_string())
            }
        })
}

#[cfg(test)]
#[allow(clippy::items_after_test_module)]
mod tests {
    use super::recover_source_url;

    #[test]
    fn test_recover_source_url_does_not_invent_github_url_for_empty_source() {
        assert_eq!(recover_source_url("", "github", None), None);
        assert_eq!(recover_source_url("", "github", Some("")), None);
    }
}

pub fn normalize_global_lock_entry(entry: &SkillLockEntry) -> NormalizedUpdateMetadata {
    NormalizedUpdateMetadata {
        source: entry.source.clone(),
        source_type: entry.source_type.clone(),
        source_url: recover_source_url(
            &entry.source,
            &entry.source_type,
            Some(entry.source_url.as_str()),
        ),
        ref_name: entry.ref_name.clone(),
        skill_path: entry.skill_path.clone(),
        remote_hash: if entry.skill_folder_hash.is_empty() {
            None
        } else {
            Some(entry.skill_folder_hash.clone())
        },
    }
}

pub fn normalize_local_lock_entry(entry: &LocalSkillLockEntry) -> NormalizedUpdateMetadata {
    NormalizedUpdateMetadata {
        source: entry.source.clone(),
        source_type: entry.source_type.clone(),
        source_url: recover_source_url(
            &entry.source,
            &entry.source_type,
            entry.source_url.as_deref(),
        ),
        ref_name: entry.ref_name.clone(),
        skill_path: entry.skill_path.clone(),
        remote_hash: entry.remote_hash.clone(),
    }
}

pub fn derive_update_capability(metadata: &NormalizedUpdateMetadata) -> UpdateCapability {
    let can_run_update = !metadata.source.is_empty() && metadata.source_type != "local";

    if !can_run_update {
        return UpdateCapability {
            can_check_for_updates: false,
            can_run_update: false,
            reason: Some("local-source".to_string()),
        };
    }

    if metadata.source_type != "github" {
        return UpdateCapability {
            can_check_for_updates: false,
            can_run_update: true,
            reason: Some("unsupported-source-type".to_string()),
        };
    }

    if metadata.skill_path.as_deref().unwrap_or("").is_empty() {
        return UpdateCapability {
            can_check_for_updates: false,
            can_run_update: false,
            reason: Some("missing-skill-path".to_string()),
        };
    }

    if metadata.remote_hash.as_deref().unwrap_or("").is_empty() {
        return UpdateCapability {
            can_check_for_updates: false,
            can_run_update: true,
            reason: Some("missing-remote-hash".to_string()),
        };
    }

    UpdateCapability {
        can_check_for_updates: true,
        can_run_update: true,
        reason: None,
    }
}

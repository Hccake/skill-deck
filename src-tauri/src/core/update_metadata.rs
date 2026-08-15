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
    pub computed_hash: Option<String>,
    pub well_known_digest: Option<String>,
}

impl NormalizedUpdateMetadata {
    pub fn comparison_baseline(&self) -> Option<&str> {
        match self.source_type.as_str() {
            "well-known" => self
                .well_known_digest
                .as_deref()
                .filter(|value| !value.is_empty()),
            "github" => self
                .remote_hash
                .as_deref()
                .filter(|value| !value.is_empty()),
            "git" | "gitlab" => self
                .computed_hash
                .as_deref()
                .filter(|value| !value.is_empty())
                .or_else(|| {
                    self.remote_hash
                        .as_deref()
                        .filter(|value| !value.is_empty())
                }),
            _ => None,
        }
    }
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
    use super::*;

    fn global(source_type: &str, baseline: &str) -> NormalizedUpdateMetadata {
        normalize_global_lock_entry(&SkillLockEntry {
            source: "acme/tools".into(),
            source_type: source_type.into(),
            source_url: format!("https://example.com/acme/tools-{source_type}.git"),
            source_base_url: None,
            well_known_digest: None,
            ref_name: Some("main".into()),
            skill_path: Some("skills/demo".into()),
            skill_folder_hash: baseline.into(),
            installed_at: String::new(),
            updated_at: String::new(),
            plugin_name: None,
        })
    }

    fn project(
        source_type: &str,
        computed_hash: &str,
        upstream_revision: Option<&str>,
    ) -> NormalizedUpdateMetadata {
        normalize_local_lock_entry(&LocalSkillLockEntry {
            source: "acme/tools".into(),
            ref_name: Some("main".into()),
            source_type: source_type.into(),
            source_url: Some(format!("https://example.com/acme/tools-{source_type}.git")),
            well_known_digest: None,
            computed_hash: computed_hash.into(),
            remote_hash: upstream_revision.map(str::to_string),
            skill_path: Some("skills/demo".into()),
            subagents: None,
            plugin_name: None,
        })
    }

    #[test]
    fn test_recover_source_url_does_not_invent_github_url_for_empty_source() {
        assert_eq!(recover_source_url("", "github", None), None);
        assert_eq!(recover_source_url("", "github", Some("")), None);
    }

    #[test]
    fn provider_and_scope_choose_the_correct_comparison_baseline() {
        let cases = [
            (global("github", "github-tree"), Some("github-tree")),
            (global("gitlab", "global-cli-hash"), Some("global-cli-hash")),
            (global("git", "global-cli-hash"), Some("global-cli-hash")),
            (
                project("github", "project-local-hash", Some("github-tree")),
                Some("github-tree"),
            ),
            (
                project("gitlab", "project-cli-hash", None),
                Some("project-cli-hash"),
            ),
            (
                project("git", "project-cli-hash", None),
                Some("project-cli-hash"),
            ),
        ];

        for (metadata, expected) in cases {
            assert_eq!(metadata.comparison_baseline(), expected);
            assert!(derive_update_capability(&metadata).can_check_for_updates);
        }
    }

    #[test]
    fn project_generic_git_keeps_computed_hash_out_of_upstream_revision() {
        let metadata = project("git", "project-cli-hash", None);

        assert_eq!(metadata.computed_hash.as_deref(), Some("project-cli-hash"));
        assert_eq!(metadata.remote_hash, None);
        assert_eq!(metadata.comparison_baseline(), Some("project-cli-hash"));
    }

    #[test]
    fn local_source_cannot_check_or_run_update() {
        let capability = derive_update_capability(&project("local", "local-hash", None));

        assert!(!capability.can_check_for_updates);
        assert!(!capability.can_run_update);
        assert_eq!(capability.reason.as_deref(), Some("local-source"));
    }

    #[test]
    fn well_known_capability_uses_index_and_digest_evidence_only() {
        let global = |source_base_url: Option<&str>, digest: Option<&str>| {
            normalize_global_lock_entry(&SkillLockEntry {
                source: "example.com".into(),
                source_type: "well-known".into(),
                source_url: "https://cdn.example.com/demo/SKILL.md".into(),
                source_base_url: source_base_url.map(str::to_string),
                well_known_digest: digest.map(str::to_string),
                ref_name: None,
                skill_path: None,
                skill_folder_hash: String::new(),
                installed_at: String::new(),
                updated_at: String::new(),
                plugin_name: None,
            })
        };

        let complete = global(
            Some("https://example.com/catalog/index.json"),
            Some("sha256:v2"),
        );
        assert!(derive_update_capability(&complete).can_run_update);
        assert!(derive_update_capability(&complete).can_check_for_updates);

        let index_only = global(Some("https://example.com/catalog/index.json"), None);
        assert!(derive_update_capability(&index_only).can_run_update);
        assert!(!derive_update_capability(&index_only).can_check_for_updates);

        let artifact_only = global(None, Some("sha256:v2"));
        assert_eq!(artifact_only.source_url, None);
        assert!(!derive_update_capability(&artifact_only).can_run_update);

        let source_host_only = global(None, None);
        assert_eq!(source_host_only.source_url, None);
        assert!(!derive_update_capability(&source_host_only).can_run_update);
    }

    #[test]
    fn project_well_known_source_url_is_the_index() {
        let metadata = normalize_local_lock_entry(&LocalSkillLockEntry {
            source: "example.com".into(),
            ref_name: None,
            source_type: "well-known".into(),
            source_url: Some("https://example.com/catalog/index.json".into()),
            well_known_digest: Some("sha256:v2".into()),
            computed_hash: "local-content".into(),
            remote_hash: None,
            skill_path: None,
            subagents: None,
            plugin_name: None,
        });

        assert_eq!(
            metadata.source_url.as_deref(),
            Some("https://example.com/catalog/index.json")
        );
        assert!(derive_update_capability(&metadata).can_check_for_updates);
    }
}

pub fn normalize_global_lock_entry(entry: &SkillLockEntry) -> NormalizedUpdateMetadata {
    let well_known = is_well_known_source_type(&entry.source_type);
    NormalizedUpdateMetadata {
        source: entry.source.clone(),
        source_type: entry.source_type.clone(),
        source_url: if well_known {
            entry
                .source_base_url
                .as_deref()
                .filter(|value| !value.is_empty())
                .map(str::to_string)
        } else {
            recover_source_url(
                &entry.source,
                &entry.source_type,
                Some(entry.source_url.as_str()),
            )
        },
        ref_name: entry.ref_name.clone(),
        skill_path: entry.skill_path.clone(),
        remote_hash: if entry.skill_folder_hash.is_empty() {
            None
        } else {
            Some(entry.skill_folder_hash.clone())
        },
        computed_hash: None,
        well_known_digest: entry.well_known_digest.clone(),
    }
}

pub fn normalize_local_lock_entry(entry: &LocalSkillLockEntry) -> NormalizedUpdateMetadata {
    let well_known = is_well_known_source_type(&entry.source_type);
    NormalizedUpdateMetadata {
        source: entry.source.clone(),
        source_type: entry.source_type.clone(),
        source_url: if well_known {
            entry
                .source_url
                .as_deref()
                .filter(|value| !value.is_empty())
                .map(str::to_string)
        } else {
            recover_source_url(
                &entry.source,
                &entry.source_type,
                entry.source_url.as_deref(),
            )
        },
        ref_name: entry.ref_name.clone(),
        skill_path: entry.skill_path.clone(),
        remote_hash: entry.remote_hash.clone(),
        computed_hash: (!entry.computed_hash.is_empty()).then(|| entry.computed_hash.clone()),
        well_known_digest: entry.well_known_digest.clone(),
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

    if metadata.source_type == "well-known" {
        if metadata.source_url.is_none() {
            return UpdateCapability {
                can_check_for_updates: false,
                can_run_update: false,
                reason: Some("missing-source".to_string()),
            };
        }
        return UpdateCapability {
            can_check_for_updates: metadata.comparison_baseline().is_some()
                && metadata.source_url.is_some(),
            can_run_update: metadata.source_url.is_some(),
            reason: metadata
                .comparison_baseline()
                .is_none()
                .then(|| "missing-remote-hash".to_string()),
        };
    }

    if !matches!(metadata.source_type.as_str(), "github" | "gitlab" | "git") {
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

    if metadata.comparison_baseline().is_none() {
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

fn is_well_known_source_type(source_type: &str) -> bool {
    matches!(
        source_type,
        "well-known" | "wellknown" | "direct-url" | "directurl"
    )
}

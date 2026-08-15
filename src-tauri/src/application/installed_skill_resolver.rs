use crate::core::lossless_lock::LosslessLockDocument;
use crate::error::AppError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedInstalledSkill {
    pub skill_name: String,
    pub install_dir_name: String,
    pub lock_key: String,
    pub lock_entry_exists: bool,
}

impl ResolvedInstalledSkill {
    pub fn requires_lock_key_migration(&self) -> bool {
        self.lock_entry_exists && self.lock_key != self.skill_name
    }
}

pub struct InstalledSkillResolver;

impl InstalledSkillResolver {
    pub fn install_dir_name(skill_name: &str) -> Result<String, AppError> {
        if skill_name.trim().is_empty()
            || skill_name.len() > 255
            || skill_name == "."
            || skill_name == ".."
            || skill_name.contains(['/', '\\', '\0'])
        {
            return Err(AppError::UnsafePath {
                path: skill_name.to_string(),
                reason: "Skill identity must contain one entry name".to_string(),
            });
        }
        Ok(crate::core::skill::sanitize_name(skill_name))
    }

    pub fn resolve(
        skill_name: &str,
        document: &LosslessLockDocument,
    ) -> Result<ResolvedInstalledSkill, AppError> {
        let install_dir_name = Self::install_dir_name(skill_name)?;
        if document.entry_snapshot(skill_name).value().is_some() {
            return Ok(ResolvedInstalledSkill {
                skill_name: skill_name.to_string(),
                install_dir_name,
                lock_key: skill_name.to_string(),
                lock_entry_exists: true,
            });
        }
        let legacy_entry_exists = install_dir_name != skill_name
            && document.entry_snapshot(&install_dir_name).value().is_some();
        let lock_key = if legacy_entry_exists {
            install_dir_name.clone()
        } else {
            skill_name.to_string()
        };
        Ok(ResolvedInstalledSkill {
            skill_name: skill_name.to_string(),
            install_dir_name,
            lock_entry_exists: legacy_entry_exists,
            lock_key,
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::core::lossless_lock::LosslessLockDocument;

    use super::InstalledSkillResolver;

    fn document(keys: &[&str]) -> LosslessLockDocument {
        let skills = keys
            .iter()
            .map(|key| ((*key).to_string(), serde_json::json!({ "source": "test" })))
            .collect::<serde_json::Map<_, _>>();
        LosslessLockDocument::parse(
            &serde_json::to_vec(&serde_json::json!({ "version": 1, "skills": skills })).unwrap(),
        )
        .unwrap()
    }

    #[test]
    fn exact_raw_name_and_unique_legacy_key_resolve_to_one_disk_directory() {
        let exact = InstalledSkillResolver::resolve("ce:review", &document(&["ce:review"]))
            .expect("exact raw name");
        assert_eq!(exact.skill_name, "ce:review");
        assert_eq!(exact.install_dir_name, "ce-review");
        assert_eq!(exact.lock_key, "ce:review");

        let legacy = InstalledSkillResolver::resolve("ce:review", &document(&["ce-review"]))
            .expect("unique legacy key");
        assert_eq!(legacy.install_dir_name, "ce-review");
        assert_eq!(legacy.lock_key, "ce-review");
        assert!(legacy.requires_lock_key_migration());
    }

    #[test]
    fn exact_match_is_case_sensitive_and_missing_lock_still_resolves_the_disk_name() {
        let missing =
            InstalledSkillResolver::resolve("CE:Review", &document(&[])).expect("unlocked Skill");
        assert_eq!(missing.skill_name, "CE:Review");
        assert_eq!(missing.install_dir_name, "ce-review");
        assert_eq!(missing.lock_key, "CE:Review");
        assert!(!missing.lock_entry_exists);
    }

    #[test]
    fn only_the_raw_name_and_deterministic_legacy_key_participate_in_resolution() {
        let legacy = InstalledSkillResolver::resolve(
            "CE:Review",
            &document(&["ce:review", "ce-review", "ce review"]),
        )
        .expect("the deterministic legacy key must win");
        assert_eq!(legacy.lock_key, "ce-review");
        assert!(legacy.lock_entry_exists);

        let missing =
            InstalledSkillResolver::resolve("CE:Review", &document(&["ce:review", "ce review"]))
                .expect("unrelated colliding names must be ignored");
        assert_eq!(missing.lock_key, "CE:Review");
        assert!(!missing.lock_entry_exists);
    }

    #[test]
    fn exact_raw_lock_key_wins_before_legacy_fallback() {
        let resolved =
            InstalledSkillResolver::resolve("ce:review", &document(&["ce:review", "ce-review"]))
                .expect("exact key");

        assert_eq!(resolved.lock_key, "ce:review");
        assert!(!resolved.requires_lock_key_migration());
    }
}

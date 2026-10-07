use super::*;
use crate::models::translation::StoredTranslationSettings;

impl ConfigStore {
    pub(crate) fn translation_settings(&self) -> Result<StoredTranslationSettings, AppError> {
        let current = self.document.lock().expect("config lock poisoned");
        let object = writable_object(&current)?;
        match object.get("translation") {
            None => Ok(StoredTranslationSettings::default()),
            Some(value) => serde_json::from_value(value.clone())
                .map_err(|_| configuration_error("translation settings are invalid")),
        }
    }

    pub(crate) fn save_translation_settings(
        &self,
        expected: &str,
        settings: StoredTranslationSettings,
    ) -> Result<(), AppError> {
        let mut current = self.document.lock().expect("config lock poisoned");
        let mut object = writable_object(&current)?;
        let previous: StoredTranslationSettings = match object.get("translation") {
            None => StoredTranslationSettings::default(),
            Some(value) => serde_json::from_value(value.clone())?,
        };
        if previous.revision != expected {
            return Err(AppError::StaleTarget);
        }
        object.insert("translation".into(), serde_json::to_value(settings)?);
        self.publish(&mut current, object)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::translation::{StoredTranslationSettings, TranslationServiceRef};

    #[test]
    fn translation_save_preserves_unrelated_fields_and_rejects_stale_revision() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        fs::write(
            &path,
            br#"{"gitCloneTimeoutSecs":321,"extension":{"preserve":true}}"#,
        )
        .unwrap();
        let store = ConfigStore::open(path.clone());
        let original = store.translation_settings().unwrap();
        assert_eq!(original.service, TranslationServiceRef::Microsoft);
        assert!(!original.auto_translate);
        let mut next = original.clone();
        next.auto_translate = true;
        next.revision = "changed".into();
        store
            .save_translation_settings(&original.revision, next.clone())
            .unwrap();
        let saved: serde_json::Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
        assert_eq!(saved["extension"]["preserve"], true);
        assert_eq!(saved["gitCloneTimeoutSecs"], 321);
        assert!(store
            .save_translation_settings(&original.revision, next)
            .is_err());
    }

    #[test]
    fn invalid_translation_does_not_discard_other_settings() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        fs::write(
            &path,
            br#"{"gitCloneTimeoutSecs":321,"translation":"broken"}"#,
        )
        .unwrap();
        let store = ConfigStore::open(path);
        assert_eq!(store.git_clone_timeout_secs(), 321);
        assert!(store.translation_settings().is_err());
        assert_eq!(
            StoredTranslationSettings::default().service,
            TranslationServiceRef::Microsoft
        );
    }
}

use crate::core::app_config::ConfigStore;
use crate::core::mutation::CancellationSignal;
use crate::models::translation::*;
use crate::runtime::http_transport::HttpTransport;
use crate::runtime::translation::{chat_endpoint, valid_segments};
use crate::storage::translation::{
    KeyringTranslationCredentials, TranslationCache, TranslationCredentials,
};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

struct Session {
    settings: StoredTranslationSettings,
    key: Option<String>,
    cancellation: CancellationSignal,
    cache_generation: u64,
    touched: Mutex<Instant>,
}

pub(crate) struct TranslationModule {
    config: Arc<ConfigStore>,
    settings: Mutex<Option<StoredTranslationSettings>>,
    credentials: Arc<dyn TranslationCredentials>,
    cache: TranslationCache,
    http: HttpTransport,
    sessions: Mutex<HashMap<String, Arc<Session>>>,
    permits: tokio::sync::Semaphore,
    saving: tokio::sync::Mutex<()>,
}

impl TranslationModule {
    pub(crate) fn new(config: Arc<ConfigStore>, http: HttpTransport, cache_root: PathBuf) -> Self {
        let settings = config.translation_settings().ok();
        Self {
            config,
            settings: Mutex::new(settings),
            credentials: Arc::new(KeyringTranslationCredentials),
            cache: TranslationCache::new(cache_root),
            http: http.for_translation(),
            sessions: Mutex::new(HashMap::new()),
            permits: tokio::sync::Semaphore::new(2),
            saving: tokio::sync::Mutex::new(()),
        }
    }
    fn stored(&self) -> Result<StoredTranslationSettings, TranslationError> {
        self.settings
            .lock()
            .expect("translation settings lock")
            .clone()
            .ok_or(TranslationError::InvalidSettings)
    }
    pub(crate) fn settings(&self) -> Result<TranslationSettings, TranslationError> {
        let mut active = self.settings.lock().expect("translation settings lock");
        self.config.reload();
        let latest = self
            .config
            .translation_settings()
            .map_err(|_| TranslationError::InvalidSettings)?;
        if active
            .as_ref()
            .is_some_and(|old| service_identity(old) != service_identity(&latest))
        {
            self.cancel_all();
        }
        let snapshot = latest.snapshot();
        *active = Some(latest);
        Ok(snapshot)
    }

    pub(crate) async fn save(
        &self,
        draft: TranslationSettingsDraft,
    ) -> Result<TranslationSettings, TranslationError> {
        let _save = self.saving.lock().await;
        let old = self.stored()?;
        if old.snapshot().revision != draft.expected_revision {
            return Err(TranslationError::StaleSettings);
        }
        if draft.profiles.len() > 32 {
            return Err(TranslationError::InvalidSettings);
        }
        let mut profiles = Vec::new();
        let mut pending_keys = Vec::new();
        for profile in draft.profiles {
            if profile.id.is_empty()
                || profile.id.len() > 128
                || profiles
                    .iter()
                    .any(|p: &StoredTranslationProfile| p.id == profile.id)
                || profile.name.trim().is_empty()
                || profile.name.len() > 256
                || profile.model.trim().is_empty()
                || profile.model.len() > 256
            {
                return Err(TranslationError::InvalidSettings);
            }
            let endpoint = chat_endpoint(&profile.base_url)?;
            let previous = old.profiles.iter().find(|p| p.id == profile.id);
            let credential_ref = match profile.key {
                TranslationKeyChange::Keep => {
                    if let Some(previous) = previous {
                        if chat_endpoint(&previous.base_url)?.origin() != endpoint.origin() {
                            return Err(TranslationError::InvalidSettings);
                        }
                    }
                    previous.and_then(|p| p.credential_ref.clone())
                }
                TranslationKeyChange::Clear => None,
                TranslationKeyChange::Replace { value } => {
                    if value.trim().is_empty() || value.len() > 16384 {
                        return Err(TranslationError::InvalidSettings);
                    }
                    let reference = uuid::Uuid::new_v4().to_string();
                    pending_keys.push((reference.clone(), value));
                    Some(reference)
                }
            };
            profiles.push(StoredTranslationProfile {
                id: profile.id,
                name: profile.name.trim().into(),
                base_url: profile.base_url.trim().trim_end_matches('/').into(),
                model: profile.model.trim().into(),
                credential_ref,
            });
        }
        if let TranslationServiceRef::OpenAiCompatible { ref profile_id } = draft.service {
            if !profiles.iter().any(|p| &p.id == profile_id) {
                return Err(TranslationError::InvalidSettings);
            }
        }
        let next = StoredTranslationSettings {
            revision: uuid::Uuid::new_v4().to_string(),
            service: draft.service,
            profiles,
            auto_translate: draft.auto_translate,
            notice_accepted: draft.notice_accepted,
        };
        let mut written: Vec<String> = Vec::new();
        for (reference, value) in pending_keys {
            if let Err(error) = self.credentials.write(&reference, value.trim()) {
                for reference in written {
                    let _ = self.credentials.delete(&reference);
                }
                return Err(error);
            }
            written.push(reference);
        }
        if let Err(error) = self
            .config
            .save_translation_settings(&old.revision, next.clone())
        {
            if !matches!(error, crate::error::AppError::ConfigurationWriteUnconfirmed) {
                for reference in written {
                    let _ = self.credentials.delete(&reference);
                }
            }
            return Err(TranslationError::StaleSettings);
        }
        {
            let mut active = self.settings.lock().expect("translation settings lock");
            if service_identity(&old) != service_identity(&next) {
                self.cancel_all();
            }
            *active = Some(next.clone());
        }
        for profile in old.profiles {
            if let Some(reference) = profile.credential_ref {
                if !next
                    .profiles
                    .iter()
                    .any(|p| p.credential_ref.as_ref() == Some(&reference))
                    && self.credentials.delete(&reference).is_err()
                {
                    log::warn!("旧翻译凭据清理未完成");
                }
            }
        }
        Ok(next.snapshot())
    }

    pub(crate) fn begin(&self, revision: &str) -> Result<String, TranslationError> {
        let active = self.settings.lock().expect("translation settings lock");
        let settings = active.clone().ok_or(TranslationError::InvalidSettings)?;
        if settings.snapshot().revision != revision {
            return Err(TranslationError::StaleSettings);
        }
        if !settings.notice_accepted {
            return Err(TranslationError::InvalidSettings);
        }
        let key = self.key(&settings)?;
        let mut sessions = self.sessions.lock().expect("translation sessions lock");
        sessions.retain(|_, session| {
            let live = session.touched.lock().expect("session time lock").elapsed()
                < Duration::from_secs(300);
            if !live {
                session.cancellation.cancel();
            }
            live
        });
        if sessions.len() >= 32 {
            return Err(TranslationError::RateLimited);
        }
        let id = uuid::Uuid::new_v4().to_string();
        sessions.insert(
            id.clone(),
            Arc::new(Session {
                settings,
                key,
                cancellation: CancellationSignal::default(),
                cache_generation: self.cache.generation(),
                touched: Mutex::new(Instant::now()),
            }),
        );
        Ok(id)
    }
    fn key(
        &self,
        settings: &StoredTranslationSettings,
    ) -> Result<Option<String>, TranslationError> {
        if let TranslationServiceRef::OpenAiCompatible { profile_id } = &settings.service {
            let profile = settings
                .profiles
                .iter()
                .find(|p| &p.id == profile_id)
                .ok_or(TranslationError::InvalidSettings)?;
            if let Some(reference) = &profile.credential_ref {
                return self
                    .credentials
                    .read(reference)?
                    .map(Some)
                    .ok_or(TranslationError::CredentialUnavailable);
            }
        }
        Ok(None)
    }
    pub(crate) fn cancel(&self, id: &str) {
        if let Some(session) = self
            .sessions
            .lock()
            .expect("translation sessions lock")
            .remove(id)
        {
            session.cancellation.cancel();
        }
    }
    fn cancel_all(&self) {
        for (_, session) in self
            .sessions
            .lock()
            .expect("translation sessions lock")
            .drain()
        {
            session.cancellation.cancel();
        }
    }
    pub(crate) fn cache_info(&self) -> TranslationCacheInfo {
        self.cache.info()
    }
    pub(crate) fn clear_cache(&self) -> Result<TranslationCacheInfo, TranslationError> {
        let _active = self.settings.lock().expect("translation settings lock");
        self.cancel_all();
        self.cache.clear()
    }

    pub(crate) async fn units(
        &self,
        id: &str,
        units: Vec<TranslationUnit>,
    ) -> Result<Vec<TranslationResult>, TranslationError> {
        if units
            .iter()
            .map(|u| u.text.len() + u.context.len())
            .sum::<usize>()
            > 1024 * 1024
            || units.is_empty()
            || units.len() > 8
            || units.iter().any(|u| {
                u.id.len() > 256
                    || u.text.len() > 1024 * 1024
                    || u.context.len() > 8000
                    || u.protected_tokens.len() > 256
                    || u.protected_tokens
                        .iter()
                        .any(|s| s.len() > 128 || s.is_empty())
            })
        {
            return Err(TranslationError::InvalidSettings);
        }
        let mut ids = std::collections::HashSet::new();
        if units.iter().any(|u| !ids.insert(&u.id)) {
            return Err(TranslationError::InvalidSettings);
        }
        let session = self
            .sessions
            .lock()
            .expect("translation sessions lock")
            .get(id)
            .cloned()
            .ok_or(TranslationError::Cancelled)?;
        *session.touched.lock().expect("session time lock") = Instant::now();
        let _permit = tokio::select! {
            result = self.permits.acquire() => result.map_err(|_| TranslationError::Cancelled)?,
            () = session.cancellation.cancelled() => return Err(TranslationError::Cancelled),
        };
        if session.cancellation.is_cancelled() {
            return Err(TranslationError::Cancelled);
        }
        let mut results = Vec::new();
        let mut missing = Vec::new();
        for unit in units {
            let key = cache_key(&session.settings, &unit);
            if let Some(text) = self
                .cache
                .get(&key)
                .filter(|s| valid_segments(s, &unit.protected_tokens))
            {
                results.push(TranslationResult::Translated { id: unit.id, text });
            } else {
                missing.push(unit);
            }
        }
        if !missing.is_empty() {
            let translated = crate::runtime::translation::translate(
                &self.http,
                &session.settings,
                session.key.as_deref(),
                &missing,
                session.cancellation.clone(),
            )
            .await;
            if session.cancellation.is_cancelled() {
                return Err(TranslationError::Cancelled);
            }
            for (index, unit) in missing.into_iter().enumerate() {
                match &translated {
                    Ok(texts)
                        if texts
                            .get(index)
                            .is_some_and(|text| valid_segments(text, &unit.protected_tokens)) =>
                    {
                        let text = texts[index].clone();
                        if self
                            .cache
                            .put(
                                &cache_key(&session.settings, &unit),
                                &text,
                                session.cache_generation,
                            )
                            .is_err()
                        {
                            log::warn!("翻译缓存写入失败，当前译文继续可用");
                        }
                        results.push(TranslationResult::Translated { id: unit.id, text });
                    }
                    Ok(_) => results.push(TranslationResult::Failed {
                        id: unit.id,
                        code: TranslationError::InvalidSegments,
                    }),
                    Err(code) => results.push(TranslationResult::Failed {
                        id: unit.id,
                        code: *code,
                    }),
                }
            }
        }
        Ok(results)
    }

    pub(crate) async fn test(
        &self,
        service: TranslationServiceRef,
        profile: Option<TranslationProfileDraft>,
    ) -> Result<String, TranslationError> {
        let mut settings = StoredTranslationSettings {
            service,
            ..Default::default()
        };
        let mut key = None;
        if let TranslationServiceRef::OpenAiCompatible { ref profile_id } = settings.service {
            let profile = profile
                .filter(|p| &p.id == profile_id)
                .ok_or(TranslationError::InvalidSettings)?;
            chat_endpoint(&profile.base_url)?;
            key = match profile.key {
                TranslationKeyChange::Clear => None,
                TranslationKeyChange::Replace { value } => Some(value),
                TranslationKeyChange::Keep => {
                    let saved = self.stored()?;
                    let old = saved
                        .profiles
                        .iter()
                        .find(|p| p.id == profile.id)
                        .ok_or(TranslationError::InvalidSettings)?;
                    if chat_endpoint(&old.base_url)?.origin()
                        != chat_endpoint(&profile.base_url)?.origin()
                    {
                        return Err(TranslationError::InvalidSettings);
                    }
                    match &old.credential_ref {
                        Some(r) => self.credentials.read(r)?,
                        None => None,
                    }
                }
            };
            settings.profiles.push(StoredTranslationProfile {
                id: profile.id,
                name: profile.name,
                base_url: profile.base_url,
                model: profile.model,
                credential_ref: None,
            });
        }
        let _permit = self
            .permits
            .acquire()
            .await
            .map_err(|_| TranslationError::Cancelled)?;
        let units = [TranslationUnit {
            id: "test".into(),
            text: "Read the skill instructions before making changes.".into(),
            context: String::new(),
            protected_tokens: Vec::new(),
        }];
        let result = crate::runtime::translation::translate(
            &self.http,
            &settings,
            key.as_deref(),
            &units,
            CancellationSignal::default(),
        )
        .await?;
        result
            .into_iter()
            .next()
            .filter(|s| !s.trim().is_empty())
            .ok_or(TranslationError::UnsupportedResponse)
    }
}

fn service_identity(settings: &StoredTranslationSettings) -> String {
    let profile = match &settings.service {
        TranslationServiceRef::OpenAiCompatible { profile_id } => {
            settings.profiles.iter().find(|p| &p.id == profile_id)
        }
        _ => None,
    };
    serde_json::to_string(&(&settings.service, profile))
        .expect("translation identity serialization")
}
fn cache_key(settings: &StoredTranslationSettings, unit: &TranslationUnit) -> String {
    let payload = serde_json::to_vec(&(
        1,
        "zh-CN",
        service_identity(settings),
        &unit.text,
        &unit.context,
        &unit.protected_tokens,
    ))
    .expect("translation cache serialization");
    format!("{:x}", Sha256::digest(payload))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[derive(Default)]
    struct MemoryCredentials(Mutex<HashMap<String, String>>);
    impl TranslationCredentials for MemoryCredentials {
        fn read(&self, key: &str) -> Result<Option<String>, TranslationError> {
            Ok(self.0.lock().unwrap().get(key).cloned())
        }
        fn write(&self, key: &str, value: &str) -> Result<(), TranslationError> {
            self.0.lock().unwrap().insert(key.into(), value.into());
            Ok(())
        }
        fn delete(&self, key: &str) -> Result<(), TranslationError> {
            self.0.lock().unwrap().remove(key);
            Ok(())
        }
    }
    fn draft(revision: String) -> TranslationSettingsDraft {
        TranslationSettingsDraft {
            expected_revision: revision,
            service: TranslationServiceRef::Microsoft,
            profiles: vec![TranslationProfileDraft {
                id: "local".into(),
                name: "Example".into(),
                base_url: "https://example.com/v1".into(),
                model: "model".into(),
                key: TranslationKeyChange::Replace {
                    value: "test-secret".into(),
                },
            }],
            auto_translate: false,
            notice_accepted: true,
        }
    }
    fn module(dir: &std::path::Path) -> TranslationModule {
        let config = Arc::new(ConfigStore::open(dir.join("config.json")));
        let http = HttpTransport::new(Arc::new(
            crate::runtime::proxy_settings::ProxySettingsStore::from_config(config.clone()),
        ));
        TranslationModule::new(config, http, dir.join("cache"))
    }
    #[tokio::test]
    async fn translation_credentials_are_not_returned_and_failed_publication_keeps_old_key() {
        let dir = tempfile::tempdir().unwrap();
        let credentials = Arc::new(MemoryCredentials::default());
        let mut module = module(dir.path());
        module.credentials = credentials.clone();
        let first = module
            .save(draft(module.settings().unwrap().revision))
            .await
            .unwrap();
        assert!(!serde_json::to_string(&first)
            .unwrap()
            .contains("test-secret"));
        assert!(!std::fs::read_to_string(dir.path().join("config.json"))
            .unwrap()
            .contains("test-secret"));
        std::fs::write(dir.path().join("config.json"), "{}").unwrap();
        assert!(module.save(draft(first.revision.clone())).await.is_err());
        assert_eq!(credentials.0.lock().unwrap().len(), 1);
        assert_eq!(module.stored().unwrap().snapshot().revision, first.revision);
    }
    #[tokio::test]
    async fn translation_cancellation_cannot_reopen_a_session() {
        let dir = tempfile::tempdir().unwrap();
        let mut module = module(dir.path());
        module.credentials = Arc::new(MemoryCredentials::default());
        let settings = module
            .save(draft(module.settings().unwrap().revision))
            .await
            .unwrap();
        let session = module.begin(&settings.revision).unwrap();
        module.cancel(&session);
        let units = vec![TranslationUnit {
            id: "one".into(),
            text: "Hello".into(),
            context: String::new(),
            protected_tokens: vec![],
        }];
        assert!(matches!(
            module.units(&session, units).await,
            Err(TranslationError::Cancelled)
        ));
    }

    #[test]
    fn translation_settings_reload_detects_external_edits_without_a_revision_change() {
        let dir = tempfile::tempdir().unwrap();
        let module = module(dir.path());
        let before = module.settings().unwrap();
        let changed = StoredTranslationSettings {
            auto_translate: true,
            ..Default::default()
        };
        std::fs::write(
            dir.path().join("config.json"),
            serde_json::to_vec(&serde_json::json!({ "translation": changed })).unwrap(),
        )
        .unwrap();
        let after = module.settings().unwrap();
        assert!(after.auto_translate);
        assert_ne!(before.revision, after.revision);
    }
}

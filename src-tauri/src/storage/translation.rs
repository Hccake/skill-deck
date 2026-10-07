use crate::models::translation::{TranslationCacheInfo, TranslationError};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

pub(crate) trait TranslationCredentials: Send + Sync {
    fn read(&self, reference: &str) -> Result<Option<String>, TranslationError>;
    fn write(&self, reference: &str, value: &str) -> Result<(), TranslationError>;
    fn delete(&self, reference: &str) -> Result<(), TranslationError>;
}

pub(crate) struct KeyringTranslationCredentials;
impl KeyringTranslationCredentials {
    fn entry(reference: &str) -> Result<keyring::Entry, TranslationError> {
        keyring::Entry::new("com.hccake.skill-deck", &format!("translation-{reference}"))
            .map_err(|_| TranslationError::CredentialUnavailable)
    }
}
impl TranslationCredentials for KeyringTranslationCredentials {
    fn read(&self, reference: &str) -> Result<Option<String>, TranslationError> {
        match Self::entry(reference)?.get_password() {
            Ok(value) => Ok(Some(value)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(_) => Err(TranslationError::CredentialUnavailable),
        }
    }
    fn write(&self, reference: &str, value: &str) -> Result<(), TranslationError> {
        Self::entry(reference)?
            .set_password(value)
            .map_err(|_| TranslationError::CredentialUnavailable)
    }
    fn delete(&self, reference: &str) -> Result<(), TranslationError> {
        match Self::entry(reference)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(_) => Err(TranslationError::CredentialUnavailable),
        }
    }
}

pub(crate) struct TranslationCache {
    root: PathBuf,
    generation: Mutex<u64>,
}
impl TranslationCache {
    pub(crate) fn new(root: PathBuf) -> Self {
        Self {
            root,
            generation: Mutex::new(0),
        }
    }
    pub(crate) fn generation(&self) -> u64 {
        *self.generation.lock().expect("cache lock")
    }
    fn path(&self, key: &str) -> Result<PathBuf, TranslationError> {
        if key.is_empty() || key.len() > 64 || !key.bytes().all(|c| c.is_ascii_hexdigit()) {
            return Err(TranslationError::CacheUnavailable);
        }
        if fs::symlink_metadata(&self.root).is_ok_and(|m| !m.is_dir() || m.file_type().is_symlink())
        {
            return Err(TranslationError::CacheUnavailable);
        }
        Ok(self.root.join(format!("{key}.json")))
    }
    pub(crate) fn get(&self, key: &str) -> Option<String> {
        let _guard = self.generation.lock().ok()?;
        let path = self.path(key).ok()?;
        let metadata = fs::symlink_metadata(&path).ok()?;
        if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > 65536 {
            return None;
        }
        let value = serde_json::from_slice::<String>(&fs::read(&path).ok()?).ok()?;
        if let Ok(file) = fs::OpenOptions::new().write(true).open(path) {
            let _ = file.set_modified(std::time::SystemTime::now());
        }
        Some(value)
    }
    pub(crate) fn put(
        &self,
        key: &str,
        text: &str,
        generation: u64,
    ) -> Result<(), TranslationError> {
        let guard = self
            .generation
            .lock()
            .map_err(|_| TranslationError::CacheUnavailable)?;
        if *guard != generation {
            return Ok(());
        }
        let path = self.path(key)?;
        let bytes = serde_json::to_vec(text).map_err(|_| TranslationError::CacheUnavailable)?;
        if bytes.len() > 65536 {
            return Ok(());
        }
        fs::create_dir_all(&self.root).map_err(|_| TranslationError::CacheUnavailable)?;
        if fs::symlink_metadata(&path).is_ok_and(|m| !m.is_file() || m.file_type().is_symlink()) {
            return Err(TranslationError::CacheUnavailable);
        }
        let expected = fs::read(&path).ok();
        environment_engine::atomic_document::replace_if_unchanged(
            &path,
            expected.as_deref(),
            &bytes,
        )
        .map_err(|_| TranslationError::CacheUnavailable)?;
        let mut entries = cache_entries(&self.root);
        entries.sort_by_key(|(_, metadata)| metadata.modified().ok());
        let mut size: u64 = entries.iter().map(|(_, m)| m.len()).sum();
        let mut count = entries.len();
        for (path, metadata) in entries {
            if size <= 100 * 1024 * 1024 && count <= 10_000 {
                break;
            }
            if fs::remove_file(path).is_ok() {
                size -= metadata.len();
                count -= 1;
            }
        }
        Ok(())
    }
    pub(crate) fn info(&self) -> TranslationCacheInfo {
        let _guard = self.generation.lock().expect("cache lock");
        let entries = cache_entries(&self.root);
        TranslationCacheInfo {
            bytes: entries.iter().map(|(_, m)| m.len() as f64).sum(),
            entries: entries.len() as u32,
        }
    }
    pub(crate) fn clear(&self) -> Result<TranslationCacheInfo, TranslationError> {
        let mut guard = self
            .generation
            .lock()
            .map_err(|_| TranslationError::CacheUnavailable)?;
        *guard += 1;
        for (path, _) in cache_entries(&self.root) {
            fs::remove_file(path).map_err(|_| TranslationError::CacheUnavailable)?;
        }
        Ok(TranslationCacheInfo::default())
    }
}
fn cache_entries(root: &Path) -> Vec<(PathBuf, fs::Metadata)> {
    if fs::symlink_metadata(root).is_ok_and(|m| !m.is_dir() || m.file_type().is_symlink()) {
        return Vec::new();
    }
    fs::read_dir(root)
        .into_iter()
        .flatten()
        .filter_map(|entry| {
            let path = entry.ok()?.path();
            if path.extension()?.to_str()? != "json" {
                return None;
            }
            let stem = path.file_stem()?.to_str()?;
            if stem.is_empty() || !stem.bytes().all(|c| c.is_ascii_hexdigit()) {
                return None;
            }
            let metadata = fs::symlink_metadata(&path).ok()?;
            (metadata.is_file() && !metadata.file_type().is_symlink()).then_some((path, metadata))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn translation_cache_survives_restart_and_clear_invalidates_old_writer() {
        let dir = tempfile::tempdir().unwrap();
        let cache = TranslationCache::new(dir.path().join("translations"));
        let generation = cache.generation();
        cache.put("abcd", "译文", generation).unwrap();
        let reopened = TranslationCache::new(dir.path().join("translations"));
        assert_eq!(reopened.get("abcd"), Some("译文".into()));
        cache.clear().unwrap();
        cache.put("abcd", "旧译文", generation).unwrap();
        assert!(cache.get("abcd").is_none());
        assert!(cache.put("../escape", "text", cache.generation()).is_err());
    }
}

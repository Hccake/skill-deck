use crate::error::AppError;
use crate::models::{
    NetworkProxySettings, ProxySettingsIssue, ProxySettingsIssueCode, ProxySettingsSnapshot,
    ProxySettingsTarget, SkillDeckConfig,
};
use crate::storage::atomic_document::{DocumentWriteFailure, PublicationState};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

mod network;

const MAX_CONFIG_BYTES: usize = environment_protocol::MAX_DOCUMENT_BYTES as usize;

fn get_skill_deck_home() -> Result<PathBuf, AppError> {
    let home = dirs::home_dir().ok_or(AppError::Path {
        message: "无法获取用户主目录".to_string(),
    })?;
    Ok(home.join(".skill-deck"))
}

/// 获取配置文件路径: ~/.skill-deck/config.json
pub fn get_config_path() -> Result<PathBuf, AppError> {
    Ok(get_skill_deck_home()?.join("config.json"))
}

/// 获取 Skill 库根目录: ~/.skill-deck/skill-libraries
pub fn get_skill_library_root() -> Result<PathBuf, AppError> {
    Ok(get_skill_deck_home()?.join("skill-libraries"))
}

struct ConfigDocument {
    bytes: Option<Vec<u8>>,
    object: Option<serde_json::Map<String, serde_json::Value>>,
    config: SkillDeckConfig,
    issues: Vec<ProxySettingsIssue>,
    read_error: Option<AppError>,
    config_error: Option<AppError>,
    physical_path: Option<PathBuf>,
}

impl ConfigDocument {
    fn read(path: &Path) -> Self {
        let physical_path = match fs::canonicalize(path) {
            Ok(path) => path,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                // A dangling configuration link is not an absent document.
                if fs::symlink_metadata(path).is_ok() {
                    return Self::unreadable(error.into());
                }
                path.to_path_buf()
            }
            Err(error) => return Self::unreadable(error.into()),
        };
        match environment_engine::atomic_document::read_optional_bounded(
            &physical_path,
            MAX_CONFIG_BYTES,
        ) {
            Ok(bytes) => {
                let mut document = Self::decode(bytes);
                document.physical_path = Some(physical_path);
                document
            }
            Err(error) => Self::unreadable(error.into()),
        }
    }

    fn unreadable(error: AppError) -> Self {
        Self {
            bytes: None,
            object: None,
            config: SkillDeckConfig::default(),
            issues: vec![network::issue(
                ProxySettingsTarget::All,
                ProxySettingsIssueCode::UnreadableDocument,
            )],
            read_error: Some(error),
            config_error: None,
            physical_path: None,
        }
    }

    fn decode(bytes: Option<Vec<u8>>) -> Self {
        let object = match bytes.as_deref() {
            None => Some(serde_json::Map::new()),
            Some(bytes) => serde_json::from_slice::<serde_json::Value>(bytes)
                .ok()
                .and_then(|value| value.as_object().cloned()),
        };
        let Some(object) = object else {
            return Self {
                bytes,
                object: None,
                config: SkillDeckConfig::default(),
                issues: vec![network::issue(
                    ProxySettingsTarget::All,
                    ProxySettingsIssueCode::InvalidDocument,
                )],
                read_error: None,
                config_error: Some(configuration_error(
                    "configuration must be a valid JSON object",
                )),
                physical_path: None,
            };
        };
        let (network_proxy, issues) = network::decode(object.get("networkProxy"));
        let mut general = object.clone();
        general.remove("networkProxy");
        let (mut config, config_error) =
            match serde_json::from_value::<SkillDeckConfig>(general.into()) {
                Ok(config) => (config, None),
                Err(_) => (
                    SkillDeckConfig::default(),
                    Some(configuration_error(
                        "application settings have invalid field types",
                    )),
                ),
            };
        config.network_proxy = network_proxy;
        Self {
            bytes,
            object: Some(object),
            config,
            issues,
            read_error: None,
            config_error,
            physical_path: None,
        }
    }

    fn snapshot(&self, path: Option<&Path>) -> ProxySettingsSnapshot {
        ProxySettingsSnapshot {
            settings: self.config.network_proxy.clone(),
            issues: self.issues.clone(),
            config_path: path.map(|path| path.to_string_lossy().into_owned()),
            repairable: self.object.is_some(),
        }
    }
}

/// Owns the document snapshot and the settings currently used by the application.
/// Reading a snapshot performs no filesystem access and emits no diagnostics.
pub(crate) struct ConfigStore {
    path: Option<PathBuf>,
    document: Mutex<ConfigDocument>,
}

impl ConfigStore {
    pub(crate) fn open(path: PathBuf) -> Self {
        let document = ConfigDocument::read(&path);
        report_issues(&path, &document);
        Self {
            path: Some(path),
            document: Mutex::new(document),
        }
    }

    pub(crate) fn from_proxy_settings(settings: NetworkProxySettings) -> Self {
        let config = SkillDeckConfig {
            network_proxy: settings,
            ..Default::default()
        };
        let bytes = serde_json::to_vec(&config).expect("configuration serialization");
        Self {
            path: None,
            document: Mutex::new(ConfigDocument::decode(Some(bytes))),
        }
    }

    pub(crate) fn config(&self) -> SkillDeckConfig {
        self.document
            .lock()
            .expect("config lock poisoned")
            .config
            .clone()
    }

    pub(crate) fn git_clone_timeout_secs(&self) -> u32 {
        self.document
            .lock()
            .expect("config lock poisoned")
            .config
            .git_clone_timeout_secs
    }

    pub(crate) fn proxy_snapshot(&self) -> ProxySettingsSnapshot {
        self.document
            .lock()
            .expect("config lock poisoned")
            .snapshot(self.path.as_deref())
    }

    pub(crate) fn reload(&self) -> ProxySettingsSnapshot {
        let mut current = self.document.lock().expect("config lock poisoned");
        if let Some(path) = &self.path {
            let mut loaded = ConfigDocument::read(path);
            preserve_available_settings(&mut loaded, &current);
            if loaded.bytes != current.bytes || loaded.issues != current.issues {
                report_issues(path, &loaded);
            }
            *current = loaded;
        }
        current.snapshot(self.path.as_deref())
    }

    pub(crate) fn save_proxy(
        &self,
        settings: NetworkProxySettings,
    ) -> Result<NetworkProxySettings, AppError> {
        let normalized =
            settings
                .validate_and_normalize()
                .map_err(|error| AppError::InvalidProxySettings {
                    code: error.code().to_string(),
                })?;
        let mut current = self.document.lock().expect("config lock poisoned");
        let mut object = writable_object(&current)?;
        object.insert("networkProxy".into(), serde_json::to_value(&normalized)?);
        self.publish(&mut current, object)?;
        Ok(normalized)
    }

    pub(crate) fn update(
        &self,
        update: impl FnOnce(&mut SkillDeckConfig),
    ) -> Result<SkillDeckConfig, AppError> {
        let mut current = self.document.lock().expect("config lock poisoned");
        let mut object = writable_object(&current)?;
        if let Some(error) = &current.config_error {
            return Err(error.clone());
        }
        let before = serde_json::to_value(&current.config)?;
        let mut updated = current.config.clone();
        update(&mut updated);
        if updated.network_proxy != current.config.network_proxy {
            updated.network_proxy =
                updated
                    .network_proxy
                    .validate_and_normalize()
                    .map_err(|error| AppError::InvalidProxySettings {
                        code: error.code().to_string(),
                    })?;
        }
        let after = serde_json::to_value(&updated)?;
        for (key, value) in after.as_object().expect("configuration object") {
            if before.get(key) != Some(value) {
                object.insert(key.clone(), value.clone());
            }
        }
        self.publish(&mut current, object)?;
        Ok(current.config.clone())
    }

    fn publish(
        &self,
        current: &mut ConfigDocument,
        object: serde_json::Map<String, serde_json::Value>,
    ) -> Result<(), AppError> {
        self.publish_with(current, object, |path, expected, bytes| {
            environment_engine::atomic_document::replace_if_unchanged(path, expected, bytes)
                .map_err(DocumentWriteFailure::from_engine)
        })
    }

    fn publish_with(
        &self,
        current: &mut ConfigDocument,
        object: serde_json::Map<String, serde_json::Value>,
        write: impl FnOnce(&Path, Option<&[u8]>, &[u8]) -> Result<(), DocumentWriteFailure>,
    ) -> Result<(), AppError> {
        let bytes = serde_json::to_vec_pretty(&object)?;
        if bytes.len() > MAX_CONFIG_BYTES {
            return Err(configuration_error("configuration exceeds its size limit"));
        }
        if let Some(path) = &self.path {
            let target = current.physical_path.as_deref().ok_or_else(|| {
                configuration_error("configuration must be readable before saving")
            })?;
            if current.bytes.is_some() && fs::canonicalize(path).ok().as_deref() != Some(target) {
                return Err(AppError::StaleTarget);
            }
            let parent = target
                .parent()
                .filter(|parent| !parent.as_os_str().is_empty())
                .ok_or_else(|| {
                    configuration_error("configuration needs an explicit parent directory")
                })?;
            fs::create_dir_all(parent)?;
            if let Err(failure) = write(target, current.bytes.as_deref(), &bytes) {
                if failure.publication != PublicationState::NotPublished {
                    self.reconcile_unconfirmed_write(current, &bytes, &failure);
                    return Err(AppError::ConfigurationWriteUnconfirmed);
                }
                return Err(failure.into_error());
            }
        }
        let previous_issues = current.issues.clone();
        let mut saved = ConfigDocument::decode(Some(bytes));
        saved.physical_path = self
            .path
            .as_ref()
            .map(|path| fs::canonicalize(path).unwrap_or_else(|_| path.clone()));
        preserve_available_settings(&mut saved, current);
        *current = saved;
        if let Some(path) = &self.path {
            if current.issues != previous_issues {
                report_issues(path, current);
            }
        }
        Ok(())
    }

    fn reconcile_unconfirmed_write(
        &self,
        current: &mut ConfigDocument,
        proposed: &[u8],
        failure: &DocumentWriteFailure,
    ) {
        let Some(path) = &self.path else {
            return;
        };
        let mut observed = ConfigDocument::read(path);
        let observation = if observed.read_error.is_some() {
            "unreadable"
        } else if observed.bytes.as_deref() == Some(proposed) {
            "proposed"
        } else if observed.bytes == current.bytes {
            "previous"
        } else {
            "changed"
        };
        log::warn!(
            "配置写入未确认: path={}, phase={:?}, publication={:?}, observed={}, error={}",
            path.display(),
            failure.phase,
            failure.publication,
            observation,
            failure.error,
        );
        observed.config = current.config.clone();
        observed.issues = current
            .issues
            .iter()
            .filter(|issue| issue.code != ProxySettingsIssueCode::WriteUnconfirmed)
            .cloned()
            .collect();
        observed.issues.push(ProxySettingsIssue {
            target: ProxySettingsTarget::All,
            code: ProxySettingsIssueCode::WriteUnconfirmed,
            using_previous: true,
        });
        if observed.issues != current.issues {
            report_issues(path, &observed);
        }
        *current = observed;
    }
}

fn writable_object(
    document: &ConfigDocument,
) -> Result<serde_json::Map<String, serde_json::Value>, AppError> {
    if let Some(error) = &document.read_error {
        return Err(error.clone());
    }
    document.object.clone().ok_or_else(|| {
        configuration_error("configuration must be repaired as a JSON object before saving")
    })
}

fn preserve_available_settings(loaded: &mut ConfigDocument, previous: &ConfigDocument) {
    let old = previous.snapshot(None);
    if loaded.config_error.is_some() {
        let network_proxy = loaded.config.network_proxy.clone();
        loaded.config = previous.config.clone();
        loaded.config.network_proxy = network_proxy;
    }
    for mut issue in std::mem::take(&mut loaded.issues) {
        if matches!(
            issue.target,
            ProxySettingsTarget::All | ProxySettingsTarget::WslGit { distro: None }
        ) {
            let blocked_whole_scope = old.issues.iter().any(|old_issue| {
                !old_issue.using_previous && old_issue.target.includes(&issue.target)
            });
            if !blocked_whole_scope {
                if issue.target == ProxySettingsTarget::All {
                    loaded.config.network_proxy = old.settings.clone();
                } else {
                    loaded.config.network_proxy.wsl_git = old.settings.wsl_git.clone();
                }
                loaded.issues.extend(
                    old.issues
                        .iter()
                        .filter(|old_issue| {
                            !old_issue.using_previous && issue.target.includes(&old_issue.target)
                        })
                        .cloned(),
                );
                issue.using_previous = true;
            }
            loaded.issues.push(issue);
            continue;
        }
        if !old.is_available(&issue.target) {
            loaded.issues.push(issue);
            continue;
        }
        match &issue.target {
            ProxySettingsTarget::All => unreachable!("whole scopes handled above"),
            ProxySettingsTarget::Http => {
                loaded.config.network_proxy.mode = old.settings.mode;
                loaded.config.network_proxy.custom_proxy_url =
                    old.settings.custom_proxy_url.clone();
            }
            ProxySettingsTarget::NativeGit => {
                loaded.config.network_proxy.native_git = old.settings.native_git.clone()
            }
            ProxySettingsTarget::WslGit { distro: None } => {
                unreachable!("whole scopes handled above")
            }
            ProxySettingsTarget::WslGit {
                distro: Some(distro),
            } => {
                if let Some(settings) = old.settings.wsl_git.get(distro) {
                    loaded
                        .config
                        .network_proxy
                        .wsl_git
                        .insert(distro.clone(), settings.clone());
                }
            }
        }
        issue.using_previous = true;
        loaded.issues.push(issue);
    }
}

fn report_issues(path: &Path, document: &ConfigDocument) {
    if document.issues.is_empty() && document.config_error.is_none() {
        log::info!("代理配置已就绪: path={}", path.display());
    }
    for issue in &document.issues {
        log::warn!(
            "代理配置需要修复: path={}, target={:?}, code={:?}, action={}",
            path.display(),
            issue.target,
            issue.code,
            if issue.using_previous {
                "keep_previous"
            } else {
                "block_affected_requests"
            }
        );
    }
    if document.config_error.is_some() {
        log::warn!(
            "应用配置需要修复: path={}, code=invalidDocument",
            path.display()
        );
    }
}

fn configuration_error(message: &str) -> AppError {
    AppError::ConfigurationCorrupted {
        message: message.to_string(),
    }
}

#[cfg(test)]
fn update_config_at_path(
    path: &Path,
    update: impl FnOnce(&mut SkillDeckConfig),
) -> Result<SkillDeckConfig, AppError> {
    ConfigStore::open(path.to_path_buf()).update(update)
}

#[cfg(test)]
fn read_config_from_path(path: &Path) -> Result<SkillDeckConfig, AppError> {
    Ok(ConfigStore::open(path.to_path_buf()).config())
}

#[cfg(test)]
fn write_config_to_path(config: &SkillDeckConfig, path: &Path) -> Result<(), AppError> {
    ConfigStore::open(path.to_path_buf()).update(|current| *current = config.clone())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::mpsc;
    use std::time::Duration;

    use super::{
        get_skill_library_root, read_config_from_path, update_config_at_path, write_config_to_path,
        ConfigStore,
    };
    use crate::error::AppError;
    use crate::models::{NativeGitProxySettings, ProxyMode, SkillDeckConfig};
    use tempfile::tempdir;

    fn custom_proxy(port: u16) -> crate::models::NetworkProxySettings {
        crate::models::NetworkProxySettings {
            mode: ProxyMode::Custom,
            custom_proxy_url: Some(format!("http://127.0.0.1:{port}")),
            ..Default::default()
        }
    }

    #[test]
    fn snapshots_do_not_reread_disk_and_saving_rejects_external_changes() {
        let temp = tempdir().unwrap();
        let path = temp.path().join("config.json");
        let store = ConfigStore::open(path.clone());
        store.save_proxy(custom_proxy(7890)).unwrap();
        let external = br#"{"futureSetting":"external","networkProxy":{"mode":"direct"}}"#;
        fs::write(&path, external).unwrap();

        for _ in 0..3 {
            assert_eq!(store.proxy_snapshot().settings, custom_proxy(7890));
        }
        assert!(matches!(
            store.save_proxy(custom_proxy(7891)),
            Err(AppError::StaleTarget)
        ));
        assert_eq!(fs::read(&path).unwrap(), external);
        assert_eq!(store.proxy_snapshot().settings, custom_proxy(7890));
    }

    #[test]
    fn reload_keeps_previous_valid_proxy_and_successful_save_clears_the_issue() {
        let temp = tempdir().unwrap();
        let path = temp.path().join("config.json");
        let store = ConfigStore::open(path.clone());
        store.save_proxy(custom_proxy(7890)).unwrap();
        fs::write(
            &path,
            br#"{"networkProxy":{"mode":"custom","customProxyUrl":"invalid"}}"#,
        )
        .unwrap();

        let reloaded = store.reload();
        assert_eq!(reloaded.settings, custom_proxy(7890));
        assert_eq!(reloaded.issues.len(), 1);
        assert!(reloaded.issues[0].using_previous);
        assert!(reloaded.is_available(&crate::models::ProxySettingsTarget::Http));
        store.save_proxy(custom_proxy(7891)).unwrap();
        assert!(store.proxy_snapshot().issues.is_empty());
        assert_eq!(store.proxy_snapshot().settings, custom_proxy(7891));
    }

    #[test]
    fn explicitly_saving_default_settings_repairs_an_invalid_section() {
        let temp = tempdir().unwrap();
        let path = temp.path().join("config.json");
        fs::write(
            &path,
            br#"{"networkProxy":{"mode":"system"},"futureSetting":true}"#,
        )
        .unwrap();
        let store = ConfigStore::open(path.clone());
        assert!(!store.proxy_snapshot().issues.is_empty());

        store.save_proxy(Default::default()).unwrap();
        assert!(store.proxy_snapshot().issues.is_empty());
        let saved: serde_json::Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
        assert_eq!(saved["networkProxy"]["mode"], "direct");
        assert_eq!(saved["futureSetting"], true);
    }

    #[test]
    fn unconfirmed_publication_reads_back_the_document_without_activating_new_settings() {
        use crate::models::{ProxySettingsIssueCode, ProxySettingsTarget};
        use crate::storage::atomic_document::{DocumentWriteFailure, PublicationState, WritePhase};

        let temp = tempdir().unwrap();
        let path = temp.path().join("config.json");
        let store = ConfigStore::open(path.clone());
        store.save_proxy(custom_proxy(7890)).unwrap();
        let mut current = store.document.lock().unwrap();
        let mut object = current.object.clone().unwrap();
        object.insert(
            "networkProxy".into(),
            serde_json::to_value(custom_proxy(7891)).unwrap(),
        );
        for _ in 0..2 {
            let result = store.publish_with(&mut current, object.clone(), |path, _, bytes| {
                fs::write(path, bytes).unwrap();
                Err(DocumentWriteFailure {
                    error: AppError::Io {
                        message: "confirmation failed".into(),
                    },
                    publication: PublicationState::PublishedUnconfirmed,
                    phase: WritePhase::Confirming,
                })
            });
            assert!(matches!(
                result,
                Err(AppError::ConfigurationWriteUnconfirmed)
            ));
            assert_eq!(current.issues.len(), 1);
        }
        assert_eq!(
            current.bytes.as_deref(),
            Some(fs::read(&path).unwrap().as_slice())
        );
        drop(current);
        let snapshot = store.proxy_snapshot();
        assert_eq!(snapshot.settings, custom_proxy(7890));
        assert_eq!(
            snapshot.issues[0].code,
            ProxySettingsIssueCode::WriteUnconfirmed
        );
        assert!(snapshot.is_available(&ProxySettingsTarget::Http));
        assert_eq!(store.reload().settings, custom_proxy(7891));
        assert!(store.proxy_snapshot().issues.is_empty());
    }

    #[test]
    fn configuration_in_a_unicode_directory_round_trips() {
        let temp = tempdir().unwrap();
        let path = temp.path().join("配置 directory").join("config.json");
        let store = ConfigStore::open(path.clone());
        store.save_proxy(custom_proxy(7890)).unwrap();
        store
            .update(|config| config.git_clone_timeout_secs = 300)
            .unwrap();
        let reloaded = ConfigStore::open(path);
        assert_eq!(reloaded.proxy_snapshot().settings, custom_proxy(7890));
        assert_eq!(reloaded.config().git_clone_timeout_secs, 300);
    }

    #[cfg(unix)]
    #[test]
    fn saving_through_a_file_symlink_preserves_the_link_and_rejects_retargeting() {
        let temp = tempdir().unwrap();
        let first = temp.path().join("first.json");
        let second = temp.path().join("second.json");
        let link = temp.path().join("config.json");
        fs::write(&first, b"{}").unwrap();
        std::os::unix::fs::symlink(&first, &link).unwrap();
        let store = ConfigStore::open(link.clone());
        store.save_proxy(custom_proxy(7890)).unwrap();
        assert!(fs::symlink_metadata(&link).unwrap().is_symlink());
        assert_eq!(
            ConfigStore::open(first.clone()).proxy_snapshot().settings,
            custom_proxy(7890)
        );

        fs::copy(&first, &second).unwrap();
        fs::remove_file(&link).unwrap();
        std::os::unix::fs::symlink(&second, &link).unwrap();
        assert!(matches!(
            store.save_proxy(custom_proxy(7891)),
            Err(AppError::StaleTarget)
        ));
        assert_eq!(fs::read(first).unwrap(), fs::read(second).unwrap());
    }

    #[cfg(windows)]
    #[test]
    fn configuration_through_a_directory_junction_preserves_the_junction() {
        let temp = tempdir().unwrap();
        let target = temp.path().join("settings");
        let junction_path = temp.path().join("linked settings");
        fs::create_dir(&target).unwrap();
        junction::create(&target, &junction_path).unwrap();
        let store = ConfigStore::open(junction_path.join("config.json"));
        store.save_proxy(custom_proxy(7890)).unwrap();
        store
            .update(|config| config.git_clone_timeout_secs = 300)
            .unwrap();
        assert!(junction::exists(&junction_path).unwrap());
        assert_eq!(
            ConfigStore::open(target.join("config.json"))
                .proxy_snapshot()
                .settings,
            custom_proxy(7890)
        );
        junction::delete(junction_path).unwrap();
    }

    #[test]
    fn skill_library_root_is_stored_under_the_shared_skill_deck_home() {
        let home = dirs::home_dir().expect("home directory");

        assert_eq!(
            get_skill_library_root().expect("Skill Library root"),
            home.join(".skill-deck").join("skill-libraries")
        );
    }

    #[test]
    fn test_read_config_from_missing_file_returns_default() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("config.json");

        let config = read_config_from_path(&path).expect("config");

        assert_eq!(config.git_clone_timeout_secs, 120);
        assert!(config.projects.is_empty());
    }

    #[test]
    fn test_write_then_read_config_round_trip() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("nested").join("config.json");
        let config = SkillDeckConfig {
            projects: vec!["/demo".to_string()],
            git_clone_timeout_secs: 300,
            ..SkillDeckConfig::default()
        };

        write_config_to_path(&config, &path).expect("write");
        let read_back = read_config_from_path(&path).expect("read");

        assert_eq!(read_back.projects, vec!["/demo"]);
        assert_eq!(read_back.git_clone_timeout_secs, 300);
    }

    #[test]
    fn invalid_network_settings_preserve_other_config() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("config.json");
        fs::write(
            &path,
            r#"{
                "projects": ["/must-survive"],
                "networkProxy": {
                    "mode": "system",
                    "customProxyUrl": "http://127.0.0.1:7890",
                    "bypassRules": ["github.com"],
                    "nativeGit": "followProxySettings",
                    "wslGitDefault": "followProxySettings",
                    "wslGitOverrides": {"Ubuntu": "followProxySettings"}
                }
            }"#,
        )
        .expect("unsupported config");

        let config = read_config_from_path(&path).expect("config");

        assert_eq!(config.projects, vec!["/must-survive"]);
        assert_eq!(config.network_proxy.mode, ProxyMode::Direct);
        assert_eq!(config.network_proxy.custom_proxy_url, None);
        assert_eq!(
            config.network_proxy.native_git,
            NativeGitProxySettings::UseExistingGitConfig
        );
        assert!(config.network_proxy.wsl_git.is_empty());
    }

    #[test]
    fn invalid_network_settings_are_not_overwritten_by_an_unrelated_update() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("config.json");
        let original = br#"{
            "projects": ["/must-survive"],
            "networkProxy": {
                "mode": "system",
                "customProxyUrl": "http://127.0.0.1:7890"
            }
        }"#;
        fs::write(&path, original).expect("invalid network settings");

        let result = update_config_at_path(&path, |config| {
            config.git_clone_timeout_secs = 60;
        });

        result.expect("unrelated settings remain writable");
        let saved: serde_json::Value =
            serde_json::from_slice(&fs::read(path).expect("saved config")).unwrap();
        let original: serde_json::Value = serde_json::from_slice(original).unwrap();
        assert_eq!(saved["networkProxy"], original["networkProxy"]);
        assert_eq!(saved["projects"], original["projects"]);
        assert_eq!(saved["gitCloneTimeoutSecs"], 60);
    }

    #[test]
    fn valid_proxy_settings_repair_a_legacy_proxy_section() {
        let temp = tempdir().unwrap();
        let path = temp.path().join("config.json");
        let original = serde_json::json!({
            "projects": ["/must-survive"],
            "futureSetting": {"keep": true},
            "networkProxy": {
                "mode": "direct",
                "customProxyUrl": null,
                "nativeGit": "useProxy",
                "nativeGitProxyUrl": "http://127.0.0.1:7890",
                "nativeGitProxyScope": "githubOnly",
                "wslGitProxyUrls": {},
                "wslGitBehaviors": {},
                "wslGitProxyScopes": {}
            }
        });
        fs::write(&path, serde_json::to_vec(&original).unwrap()).unwrap();

        update_config_at_path(&path, |config| {
            config.network_proxy = crate::models::NetworkProxySettings {
                native_git: NativeGitProxySettings::UseProxy {
                    proxy_url: "http://127.0.0.1:7890".to_string(),
                    scope: crate::models::GitProxyScope::GithubOnly,
                },
                ..Default::default()
            };
        })
        .expect("valid settings repair the invalid section");

        let saved: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(saved["projects"], original["projects"]);
        assert_eq!(saved["futureSetting"], original["futureSetting"]);
        assert_eq!(saved["networkProxy"]["nativeGit"]["behavior"], "useProxy");
        assert!(saved["networkProxy"].get("nativeGitProxyUrl").is_none());
    }

    #[test]
    fn invalid_git_settings_preserve_a_valid_http_proxy() {
        let temp = tempdir().unwrap();
        let path = temp.path().join("config.json");
        fs::write(
            &path,
            br#"{"networkProxy":{"mode":"custom","customProxyUrl":"http://127.0.0.1:7890","nativeGit":"useProxy"}}"#,
        )
        .unwrap();

        let config = read_config_from_path(&path).unwrap();
        assert_eq!(config.network_proxy.mode, ProxyMode::Custom);
        assert_eq!(
            config.network_proxy.custom_proxy_url.as_deref(),
            Some("http://127.0.0.1:7890")
        );
    }

    #[test]
    fn invalid_wsl_distribution_preserves_other_network_settings() {
        let temp = tempdir().unwrap();
        let path = temp.path().join("config.json");
        fs::write(
            &path,
            br#"{"networkProxy":{"mode":"custom","customProxyUrl":"http://127.0.0.1:7890","nativeGit":{"behavior":"useProxy","proxyUrl":"http://127.0.0.1:7891","scope":"githubOnly"},"wslGit":{"Ubuntu":"useProxy","Debian":{"behavior":"useProxy","proxyUrl":"http://127.0.0.1:7892","scope":"allHttpHttps"}}}}"#,
        )
        .unwrap();

        let config = read_config_from_path(&path).unwrap();
        assert_eq!(config.network_proxy.mode, ProxyMode::Custom);
        assert!(matches!(
            config.network_proxy.native_git,
            NativeGitProxySettings::UseProxy { .. }
        ));
        assert!(config.network_proxy.wsl_git.contains_key("Debian"));
    }

    #[test]
    fn config_updates_are_serialized_across_read_modify_write() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("config.json");
        let store = std::sync::Arc::new(ConfigStore::open(path.clone()));
        let (first_entered_tx, first_entered_rx) = mpsc::channel();
        let (release_first_tx, release_first_rx) = mpsc::channel();
        let (second_attempting_tx, second_attempting_rx) = mpsc::channel();
        let (second_entered_tx, second_entered_rx) = mpsc::channel();

        std::thread::scope(|scope| {
            let first_store = store.clone();
            scope.spawn(move || {
                first_store
                    .update(|config| {
                        config.git_clone_timeout_secs = 300;
                        first_entered_tx.send(()).expect("first entered");
                        release_first_rx.recv().expect("release first");
                    })
                    .expect("first update");
            });
            first_entered_rx.recv().expect("first update started");

            let second_store = store.clone();
            scope.spawn(move || {
                second_attempting_tx.send(()).expect("second attempting");
                second_store
                    .update(|config| {
                        second_entered_tx.send(()).expect("second entered");
                        config.wsl_integration_enabled = true;
                    })
                    .expect("second update");
            });
            second_attempting_rx
                .recv()
                .expect("second update attempted");
            assert!(second_entered_rx
                .recv_timeout(Duration::from_millis(100))
                .is_err());

            release_first_tx.send(()).expect("release first update");
            second_entered_rx
                .recv_timeout(Duration::from_secs(1))
                .expect("second update entered after first completed");
        });

        let config = read_config_from_path(&path).expect("final config");
        assert_eq!(config.git_clone_timeout_secs, 300);
        assert!(config.wsl_integration_enabled);
    }
    #[test]
    fn corrupt_config_can_degrade_for_reading_but_is_not_overwritten_by_an_update() {
        let temp = tempdir().unwrap();
        let path = temp.path().join("config.json");
        let original = b"{\"projects\": [\"/keep-me\"]";
        fs::write(&path, original).unwrap();
        assert!(read_config_from_path(&path).is_ok());
        assert!(update_config_at_path(&path, |config| config.git_clone_timeout_secs = 60).is_err());
        assert_eq!(fs::read(path).unwrap(), original);
    }

    #[test]
    fn invalid_typed_config_is_preserved_instead_of_saved_as_defaults() {
        let temp = tempdir().unwrap();
        let path = temp.path().join("config.json");
        let original = br#"{"projects":"not-an-array"}"#;
        fs::write(&path, original).unwrap();
        assert!(update_config_at_path(&path, |config| config.git_clone_timeout_secs = 60).is_err());
        assert_eq!(fs::read(path).unwrap(), original);
    }

    #[test]
    fn an_unreadable_document_is_not_treated_as_missing_during_update() {
        let temp = tempdir().unwrap();
        let path = temp.path().join("config.json");
        fs::create_dir(&path).unwrap();
        assert!(update_config_at_path(&path, |config| config.git_clone_timeout_secs = 60).is_err());
        assert!(path.is_dir());
    }

    #[test]
    fn first_config_update_initializes_missing_file_without_removing_a_backup() {
        let temp = tempdir().unwrap();
        let path = temp.path().join("config.json");
        let backup = temp.path().join("config.json.bak");
        fs::write(&backup, b"owned-by-another-maintenance-flow").unwrap();
        update_config_at_path(&path, |config| config.git_clone_timeout_secs = 60).unwrap();
        assert_eq!(
            read_config_from_path(&path).unwrap().git_clone_timeout_secs,
            60
        );
        assert_eq!(
            fs::read(backup).unwrap(),
            b"owned-by-another-maintenance-flow"
        );
    }

    #[test]
    fn oversized_config_is_rejected_before_it_can_be_saved() {
        let temp = tempdir().unwrap();
        let path = temp.path().join("config.json");
        let file = fs::File::create(&path).unwrap();
        file.set_len(u64::from(environment_protocol::MAX_DOCUMENT_BYTES) + 1)
            .unwrap();

        let error = update_config_at_path(&path, |config| {
            config.git_clone_timeout_secs = 60;
        })
        .unwrap_err();

        assert!(matches!(
            error,
            AppError::Io { ref message } if message.contains("exceeds its read limit")
        ));
        assert_eq!(
            fs::metadata(path).unwrap().len(),
            u64::from(environment_protocol::MAX_DOCUMENT_BYTES) + 1
        );
    }
}

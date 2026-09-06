use crate::error::AppError;
use crate::models::{NetworkProxySettings, SkillDeckConfig};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

static CONFIG_UPDATE_LOCK: Mutex<()> = Mutex::new(());
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

pub fn read_config() -> Result<SkillDeckConfig, AppError> {
    let path = get_config_path()?;
    let _guard = CONFIG_UPDATE_LOCK
        .lock()
        .expect("config update lock poisoned");
    read_config_from_path(&path)
}

pub fn update_config(
    update: impl FnOnce(&mut SkillDeckConfig),
) -> Result<SkillDeckConfig, AppError> {
    let path = get_config_path()?;
    update_config_at_path(&path, update)
}

fn update_config_at_path(
    path: &Path,
    update: impl FnOnce(&mut SkillDeckConfig),
) -> Result<SkillDeckConfig, AppError> {
    let _guard = CONFIG_UPDATE_LOCK
        .lock()
        .expect("config update lock poisoned");
    let mut config = read_config_document(path, true)?;
    update(&mut config);
    write_config_to_path(&config, path)?;
    Ok(config)
}

fn read_config_from_path(path: &Path) -> Result<SkillDeckConfig, AppError> {
    read_config_document(path, false)
}

fn read_config_document(path: &Path, require_writable: bool) -> Result<SkillDeckConfig, AppError> {
    let bytes =
        match environment_engine::atomic_document::read_optional_bounded(path, MAX_CONFIG_BYTES) {
            Ok(Some(bytes)) => bytes,
            Ok(None) => return Ok(SkillDeckConfig::default()),
            Err(error) if require_writable => return Err(error.into()),
            Err(error) => {
                log::warn!("读取配置文件失败，运行时使用默认配置，原文件保持不变: {error}");
                return Ok(SkillDeckConfig::default());
            }
        };
    let content = match String::from_utf8(bytes) {
        Ok(content) => content,
        Err(error) if require_writable => {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, error).into());
        }
        Err(error) => {
            log::warn!("配置文件编码无效，运行时使用默认配置，原文件保持不变: {error}");
            return Ok(SkillDeckConfig::default());
        }
    };
    let mut value: serde_json::Value = match serde_json::from_str(&content) {
        Ok(value) => value,
        Err(error) if require_writable => {
            return Err(AppError::ConfigurationCorrupted {
                message: format!("configuration must be repaired before saving: {error}"),
            });
        }
        Err(error) => {
            log::warn!("配置损坏，运行时使用默认配置，原文件保持不变: {error}");
            return Ok(SkillDeckConfig::default());
        }
    };
    let network_proxy = value
        .as_object_mut()
        .and_then(|object| object.remove("networkProxy"))
        .map_or_else(
            || Ok(NetworkProxySettings::default()),
            |network_proxy| {
                serde_json::from_value::<NetworkProxySettings>(network_proxy)
                    .map_err(|error| error.to_string())
                    .and_then(|settings| {
                        settings
                            .validate_and_normalize()
                            .map_err(|error| format!("code={}", error.code()))
                    })
            },
        );
    let network_proxy = match network_proxy {
        Ok(settings) => settings,
        Err(error) if require_writable => {
            return Err(AppError::ConfigurationCorrupted {
                message: format!("network proxy settings must be repaired before saving: {error}"),
            });
        }
        Err(error) => {
            log::warn!("代理设置无效，使用直接连接: {}", error);
            NetworkProxySettings::default()
        }
    };
    let mut config: SkillDeckConfig = match serde_json::from_value(value) {
        Ok(config) => config,
        Err(error) if require_writable => {
            return Err(AppError::ConfigurationCorrupted {
                message: format!("configuration must be repaired before saving: {error}"),
            });
        }
        Err(error) => {
            log::warn!("解析配置失败，运行时使用默认配置，原文件保持不变: {error}");
            SkillDeckConfig::default()
        }
    };
    config.network_proxy = network_proxy;
    Ok(config)
}

fn write_config_to_path(config: &SkillDeckConfig, path: &Path) -> Result<(), AppError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let content = serde_json::to_string_pretty(config)?;
    crate::environment::native::atomic_file::write_native_atomic(path, content.as_bytes())?;

    log::info!("配置已保存到: {:?}", path);
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::mpsc;
    use std::time::Duration;

    use super::{
        get_skill_library_root, read_config_from_path, update_config_at_path, write_config_to_path,
    };
    use crate::error::AppError;
    use crate::models::{NativeGitProxySettings, ProxyMode, SkillDeckConfig};
    use tempfile::tempdir;

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

        assert!(matches!(
            result,
            Err(AppError::ConfigurationCorrupted { .. })
        ));
        assert_eq!(fs::read(path).expect("original config"), original);
    }

    #[test]
    fn config_updates_are_serialized_across_read_modify_write() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("config.json");
        let (first_entered_tx, first_entered_rx) = mpsc::channel();
        let (release_first_tx, release_first_rx) = mpsc::channel();
        let (second_attempting_tx, second_attempting_rx) = mpsc::channel();
        let (second_entered_tx, second_entered_rx) = mpsc::channel();

        std::thread::scope(|scope| {
            let first_path = path.clone();
            scope.spawn(move || {
                update_config_at_path(&first_path, |config| {
                    config.git_clone_timeout_secs = 300;
                    first_entered_tx.send(()).expect("first entered");
                    release_first_rx.recv().expect("release first");
                })
                .expect("first update");
            });
            first_entered_rx.recv().expect("first update started");

            let second_path = path.clone();
            scope.spawn(move || {
                second_attempting_tx.send(()).expect("second attempting");
                update_config_at_path(&second_path, |config| {
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

use crate::error::AppError;
use crate::models::{NetworkProxySettings, SkillDeckConfig};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

static CONFIG_UPDATE_LOCK: Mutex<()> = Mutex::new(());

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
    let mut config = read_config_from_path(path)?;
    update(&mut config);
    write_config_to_path(&config, path)?;
    Ok(config)
}

fn read_config_from_path(path: &Path) -> Result<SkillDeckConfig, AppError> {
    if !path.exists() {
        log::info!("配置文件不存在，返回默认配置");
        return Ok(SkillDeckConfig::default());
    }

    let content = match fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) => {
            log::warn!("读取配置文件失败: {}，返回默认配置", e);
            return Ok(SkillDeckConfig::default());
        }
    };

    let mut value: serde_json::Value = serde_json::from_str(&content).unwrap_or_else(|e| {
        log::warn!("解析配置文件失败: {}，返回默认配置", e);
        serde_json::json!({})
    });
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
        )
        .unwrap_or_else(|error| {
            log::warn!("代理设置无效，使用直接连接: {}", error);
            NetworkProxySettings::default()
        });
    let mut config: SkillDeckConfig = serde_json::from_value(value).unwrap_or_else(|e| {
        log::warn!("解析应用配置失败: {}，返回默认配置", e);
        SkillDeckConfig::default()
    });
    config.network_proxy = network_proxy;
    Ok(config)
}

fn write_config_to_path(config: &SkillDeckConfig, path: &Path) -> Result<(), AppError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let content = serde_json::to_string_pretty(config)?;
    fs::write(path, content)?;

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
}

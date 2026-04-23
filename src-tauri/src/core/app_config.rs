use crate::error::AppError;
use crate::models::SkillDeckConfig;
use std::fs;
use std::path::{Path, PathBuf};

/// 获取配置文件路径: ~/.skill-deck/config.json
pub fn get_config_path() -> Result<PathBuf, AppError> {
    let home = dirs::home_dir().ok_or(AppError::Path {
        message: "无法获取用户主目录".to_string(),
    })?;
    Ok(home.join(".skill-deck").join("config.json"))
}

pub fn read_config() -> Result<SkillDeckConfig, AppError> {
    let path = get_config_path()?;
    read_config_from_path(&path)
}

pub fn write_config(config: &SkillDeckConfig) -> Result<(), AppError> {
    let path = get_config_path()?;
    write_config_to_path(config, &path)
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

    Ok(serde_json::from_str(&content).unwrap_or_else(|e| {
        log::warn!("解析配置文件失败: {}，返回默认配置", e);
        SkillDeckConfig::default()
    }))
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
    use super::{read_config_from_path, write_config_to_path};
    use crate::models::SkillDeckConfig;
    use tempfile::tempdir;

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
        };

        write_config_to_path(&config, &path).expect("write");
        let read_back = read_config_from_path(&path).expect("read");

        assert_eq!(read_back.projects, vec!["/demo"]);
        assert_eq!(read_back.git_clone_timeout_secs, 300);
    }
}

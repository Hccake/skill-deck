use crate::core::skill_lock;
use crate::error::AppError;
use crate::models::SkillDeckConfig;
use std::fs;
use std::path::PathBuf;

/// 获取配置文件路径: ~/.skill-deck/config.json
fn get_config_path() -> Result<PathBuf, AppError> {
    let home = dirs::home_dir().ok_or(AppError::Path { message: "无法获取用户主目录".to_string() })?;
    Ok(home.join(".skill-deck").join("config.json"))
}

/// 获取配置
/// 文件不存在或解析失败时返回默认配置
#[tauri::command]
#[specta::specta]
pub fn get_config() -> Result<SkillDeckConfig, AppError> {
    let path = get_config_path()?;

    if !path.exists() {
        log::info!("配置文件不存在，返回默认配置");
        return Ok(SkillDeckConfig::default());
    }

    let content = match fs::read_to_string(&path) {
        Ok(c) => c,
        Err(e) => {
            log::warn!("读取配置文件失败: {}，返回默认配置", e);
            return Ok(SkillDeckConfig::default());
        }
    };

    // 解析失败时返回默认配置，而非错误
    Ok(serde_json::from_str(&content).unwrap_or_else(|e| {
        log::warn!("解析配置文件失败: {}，返回默认配置", e);
        SkillDeckConfig::default()
    }))
}

/// 保存配置
/// 目录不存在时自动创建
#[tauri::command]
#[specta::specta]
pub fn save_config(config: SkillDeckConfig) -> Result<(), AppError> {
    let path = get_config_path()?;

    // 确保目录存在
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let content = serde_json::to_string_pretty(&config)?;

    fs::write(&path, content)?;

    log::info!("配置已保存到: {:?}", path);
    Ok(())
}

/// 获取上次选择的 agents
/// 读取 ~/.agents/.skill-lock.json 中的 lastSelectedAgents
#[tauri::command]
#[specta::specta]
pub fn get_last_selected_agents() -> Vec<String> {
    skill_lock::get_last_selected_agents().unwrap_or_default()
}

/// 保存选择的 agents
/// 写入 ~/.agents/.skill-lock.json 中的 lastSelectedAgents
#[tauri::command]
#[specta::specta]
pub fn save_last_selected_agents(agents: Vec<String>) -> Result<(), AppError> {
    skill_lock::save_selected_agents(&agents)?;
    Ok(())
}

/// 添加项目路径
/// 已存在则忽略，返回更新后的 projects 列表
#[tauri::command]
#[specta::specta]
pub fn add_project(path: String) -> Result<Vec<String>, AppError> {
    let mut config = get_config()?;
    if !config.projects.contains(&path) {
        config.projects.push(path);
        save_config(config.clone())?;
    }
    Ok(config.projects)
}

/// 移除项目路径
/// 返回更新后的 projects 列表
#[tauri::command]
#[specta::specta]
pub fn remove_project(path: String) -> Result<Vec<String>, AppError> {
    let mut config = get_config()?;
    config.projects.retain(|p| p != &path);
    save_config(config.clone())?;
    Ok(config.projects)
}

/// 检查项目路径是否存在
#[tauri::command]
#[specta::specta]
pub fn check_project_path(path: String) -> bool {
    std::path::Path::new(&path).is_dir()
}

/// 在系统文件管理器中打开路径
#[tauri::command]
#[specta::specta]
pub fn open_in_explorer(path: String) -> Result<(), AppError> {
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer")
            .arg(&path)
            .spawn()?;
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(&path)
            .spawn()?;
    }
    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open")
            .arg(&path)
            .spawn()?;
    }
    Ok(())
}

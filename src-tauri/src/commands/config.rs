use crate::core::mutation::{MutationKind, SingleMutationController};
use crate::core::{read_config, skill_lock, write_config};
use crate::environment::lock_io::EnvironmentLockIo;
use crate::environment::types::{ContextRef, EnvironmentRef};
use crate::environment::wsl::EnvironmentRegistry;
use crate::error::AppError;
use crate::models::SkillDeckConfig;
use tauri::State;

/// 获取配置
/// 文件不存在或解析失败时返回默认配置
#[tauri::command]
#[specta::specta]
pub fn get_config() -> Result<SkillDeckConfig, AppError> {
    read_config()
}

/// 保存配置
/// 目录不存在时自动创建
#[tauri::command]
#[specta::specta]
pub fn save_config(config: SkillDeckConfig) -> Result<(), AppError> {
    write_config(&config)
}

/// 获取上次选择的 agents
/// 读取 ~/.agents/.skill-lock.json 中的 lastSelectedAgents
#[tauri::command]
#[specta::specta]
pub fn get_last_selected_agents() -> Vec<String> {
    skill_lock::get_last_selected_agents().unwrap_or_default()
}

/// 获取 GUI scope-aware 默认安装目标
#[tauri::command]
#[specta::specta]
pub fn get_default_target_agents() -> Option<skill_lock::DefaultTargetAgents> {
    skill_lock::get_default_target_agents()
}

#[tauri::command]
#[specta::specta]
pub async fn get_default_target_agents_v2(
    context: ContextRef,
    registry: State<'_, EnvironmentRegistry>,
) -> Result<Option<skill_lock::DefaultTargetAgents>, AppError> {
    match &context.environment {
        EnvironmentRef::Host => Ok(get_default_target_agents()),
        EnvironmentRef::Wsl { distro_name } => {
            let session = registry.get(distro_name).ok_or_else(|| AppError::Custom {
                message: format!("WSL distro '{distro_name}' is not connected"),
            })?;
            let (locator, _) = crate::commands::remove::wsl_remove_lock_locators(
                &ContextRef {
                    environment: context.environment.clone(),
                    scope: crate::environment::types::ContextScope::Global,
                },
                &session,
                None,
            );
            let Some(bytes) = EnvironmentLockIo::Wsl(session)
                .read_optional(&locator)
                .await?
            else {
                return Ok(None);
            };
            let lock: skill_lock::SkillLockFile = serde_json::from_slice(&bytes)?;
            Ok(lock.default_target_agents)
        }
    }
}

/// 保存 GUI scope-aware 默认安装目标
#[tauri::command]
#[specta::specta]
pub fn save_default_target_agents(
    defaults: skill_lock::DefaultTargetAgents,
) -> Result<(), AppError> {
    skill_lock::save_default_target_agents(defaults)?;
    Ok(())
}

fn apply_default_target_agents_to_lock(
    current: &[u8],
    defaults: &skill_lock::DefaultTargetAgents,
) -> Result<Vec<u8>, AppError> {
    let mut root: serde_json::Value = serde_json::from_slice(current)?;
    let object = root.as_object_mut().ok_or_else(|| AppError::Json {
        message: "lock root must be a JSON object".to_string(),
    })?;
    if !object
        .get("skills")
        .is_some_and(serde_json::Value::is_object)
    {
        return Err(AppError::Json {
            message: "lock skills must be a JSON object".to_string(),
        });
    }
    object.insert(
        "defaultTargetAgents".to_string(),
        serde_json::to_value(defaults)?,
    );
    let mut bytes = serde_json::to_vec_pretty(&root)?;
    bytes.push(b'\n');
    Ok(bytes)
}

#[tauri::command]
#[specta::specta]
pub async fn save_default_target_agents_v2(
    context: ContextRef,
    defaults: skill_lock::DefaultTargetAgents,
    registry: State<'_, EnvironmentRegistry>,
    controller: State<'_, SingleMutationController>,
) -> Result<(), AppError> {
    let guard = controller.begin(
        MutationKind::SaveAgentDefaults,
        context.clone(),
        "Saving Agent defaults",
    )?;
    match &context.environment {
        EnvironmentRef::Host => save_default_target_agents(defaults),
        EnvironmentRef::Wsl { distro_name } => {
            let session = registry.get(distro_name).ok_or_else(|| AppError::Custom {
                message: format!("WSL distro '{distro_name}' is not connected"),
            })?;
            let (locator, _) = crate::commands::remove::wsl_remove_lock_locators(
                &ContextRef {
                    environment: context.environment.clone(),
                    scope: crate::environment::types::ContextScope::Global,
                },
                &session,
                None,
            );
            let io = EnvironmentLockIo::Wsl(session);
            let empty = serde_json::to_vec(&crate::core::skill_lock::SkillLockFile::empty())?;
            let initial = io
                .read_optional(&locator)
                .await?
                .unwrap_or_else(|| empty.clone());
            let initial_value: serde_json::Value = serde_json::from_slice(&initial)?;
            let expected_defaults = initial_value.get("defaultTargetAgents").cloned();
            if guard.cancellation().is_cancelled() {
                return Err(AppError::Custom {
                    message: "Saving Agent defaults was cancelled".to_string(),
                });
            }
            guard.set_cancelable(false);
            let latest = io
                .read_optional(&locator)
                .await?
                .unwrap_or_else(|| empty.clone());
            let latest_value: serde_json::Value = serde_json::from_slice(&latest)?;
            if latest_value.get("defaultTargetAgents").cloned() != expected_defaults {
                return Err(AppError::Custom {
                    message: "default Agent settings changed externally".to_string(),
                });
            }
            io.write_atomic(
                &locator,
                apply_default_target_agents_to_lock(&latest, &defaults)?,
            )
            .await
        }
    }
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
        std::process::Command::new("explorer").arg(&path).spawn()?;
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open").arg(&path).spawn()?;
    }
    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open").arg(&path).spawn()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn environment_default_agents_update_preserves_unknown_lock_fields() {
        let current = br#"{
          "version": 3,
          "customRoot": { "keep": true },
          "skills": { "demo": { "source": "owner/repo", "future": 7 } },
          "defaultTargetAgents": { "global": ["codex"], "project": [] }
        }"#;
        let defaults = skill_lock::DefaultTargetAgents {
            global: vec!["claude-code".to_string()],
            project: vec!["cursor".to_string()],
        };

        let updated = apply_default_target_agents_to_lock(current, &defaults)
            .expect("update defaults losslessly");
        let value: serde_json::Value = serde_json::from_slice(&updated).unwrap();

        assert_eq!(value["customRoot"]["keep"], true);
        assert_eq!(value["skills"]["demo"]["future"], 7);
        assert_eq!(value["defaultTargetAgents"]["global"][0], "claude-code");
        assert_eq!(value["defaultTargetAgents"]["project"][0], "cursor");
    }
}

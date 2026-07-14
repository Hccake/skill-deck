use crate::core::lock_repository::{LockMutationTargets, LockRepository, LockTarget};
use crate::core::lossless_lock::LockSchema;
use crate::core::mutation::{MutationKind, MutationPhase, SingleMutationController};
use crate::core::{read_config, skill_lock, write_config};
use crate::environment::context_resolver::ContextResolver;
use crate::environment::lock_io::EnvironmentLockIo;
use crate::environment::types::{ContextRef, EnvironmentRef, ResourceLocator};
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

/// 获取 GUI scope-aware 默认安装目标
pub fn get_default_target_agents_host() -> Option<skill_lock::DefaultTargetAgents> {
    skill_lock::get_default_target_agents_host()
}

#[tauri::command]
#[specta::specta]
pub async fn get_default_target_agents(
    context: ContextRef,
    registry: State<'_, EnvironmentRegistry>,
) -> Result<Option<skill_lock::DefaultTargetAgents>, AppError> {
    ensure_global_context(&context)?;
    match &context.environment {
        EnvironmentRef::Host => {
            ContextResolver::resolve_host(context)?;
            Ok(get_default_target_agents_host())
        }
        EnvironmentRef::Wsl { distro_name } => {
            let distro_name = distro_name.clone();
            let retry_context = context.clone();
            registry
                .with_session_retry(&distro_name, move |session| {
                    let context = retry_context.clone();
                    async move {
                        let locator = ContextResolver::resolve_wsl(context, &session).await?.lock;
                        let Some(bytes) = EnvironmentLockIo::Wsl(session)
                            .read_optional(&locator)
                            .await?
                        else {
                            return Ok(None);
                        };
                        let lock: skill_lock::SkillLockFile = serde_json::from_slice(&bytes)?;
                        Ok(lock.default_target_agents)
                    }
                })
                .await
        }
    }
}

async fn commit_default_target_agents(
    io: EnvironmentLockIo,
    target: LockTarget,
    defaults: skill_lock::DefaultTargetAgents,
) -> Result<(), AppError> {
    let repository = LockRepository::new(io);
    let mut transaction = repository
        .begin(
            target,
            LockMutationTargets {
                entries: Vec::new(),
                default_target_agents: true,
            },
        )
        .await?;
    let mut last_selected_agents = Vec::new();
    for agent in defaults.global.iter().chain(defaults.project.iter()) {
        if !last_selected_agents.contains(agent) {
            last_selected_agents.push(agent.clone());
        }
    }
    transaction.set_default_target_agents(
        serde_json::to_value(defaults)?,
        serde_json::to_value(last_selected_agents)?,
    )?;
    transaction.commit().await
}

#[tauri::command]
#[specta::specta]
pub async fn save_default_target_agents(
    context: ContextRef,
    defaults: skill_lock::DefaultTargetAgents,
    registry: State<'_, EnvironmentRegistry>,
    controller: State<'_, SingleMutationController>,
) -> Result<(), AppError> {
    ensure_global_context(&context)?;
    let guard = controller.begin(MutationKind::SaveAgentDefaults, context.clone())?;
    match &context.environment {
        EnvironmentRef::Host => {
            ContextResolver::resolve_host(context)?;
            guard.transition(MutationPhase::Committing, None, false);
            commit_default_target_agents(
                EnvironmentLockIo::Host,
                LockTarget {
                    primary: ResourceLocator {
                        environment: EnvironmentRef::Host,
                        native_path: crate::core::skill_lock::get_skill_lock_path()
                            .to_string_lossy()
                            .to_string(),
                    },
                    legacy: None,
                    schema: LockSchema::Global,
                },
                defaults,
            )
            .await
        }
        EnvironmentRef::Wsl { distro_name } => {
            let distro_name = distro_name.clone();
            let retry_context = context.clone();
            let guard = &guard;
            registry
                .with_session_retry(&distro_name, move |session| {
                    let context = retry_context.clone();
                    let defaults = defaults.clone();
                    async move {
                        let locator = ContextResolver::resolve_wsl(context, &session).await?.lock;
                        guard.transition(MutationPhase::Committing, None, false);
                        commit_default_target_agents(
                            EnvironmentLockIo::Wsl(session),
                            LockTarget {
                                primary: locator,
                                legacy: None,
                                schema: LockSchema::Global,
                            },
                            defaults,
                        )
                        .await
                    }
                })
                .await
        }
    }
}

fn ensure_global_context(context: &ContextRef) -> Result<(), AppError> {
    if matches!(
        context.scope,
        crate::environment::types::ContextScope::Global
    ) {
        Ok(())
    } else {
        Err(AppError::Custom {
            message: "default Agent settings require global context".to_string(),
        })
    }
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

    #[tokio::test]
    async fn environment_default_agents_update_preserves_unknown_lock_fields() {
        let temp = tempfile::tempdir().expect("tempdir");
        let lock_path = temp.path().join("skill-lock.json");
        let current = br#"{
          "version": 3,
          "customRoot": { "keep": true },
          "skills": { "demo": { "source": "owner/repo", "future": 7 } },
          "defaultTargetAgents": { "global": ["codex"], "project": [] },
          "lastSelectedAgents": ["codex"]
        }"#;
        std::fs::write(&lock_path, current).expect("write lock");
        let defaults = skill_lock::DefaultTargetAgents {
            global: vec!["claude-code".to_string(), "cursor".to_string()],
            project: vec!["cursor".to_string()],
        };

        commit_default_target_agents(
            EnvironmentLockIo::Host,
            LockTarget {
                primary: ResourceLocator {
                    environment: EnvironmentRef::Host,
                    native_path: lock_path.to_string_lossy().to_string(),
                },
                legacy: None,
                schema: LockSchema::Global,
            },
            defaults,
        )
        .await
        .expect("update defaults losslessly");
        let value: serde_json::Value =
            serde_json::from_slice(&std::fs::read(lock_path).unwrap()).unwrap();

        assert_eq!(value["customRoot"]["keep"], true);
        assert_eq!(value["skills"]["demo"]["future"], 7);
        assert_eq!(value["defaultTargetAgents"]["global"][0], "claude-code");
        assert_eq!(value["defaultTargetAgents"]["project"][0], "cursor");
        assert_eq!(
            value["lastSelectedAgents"],
            serde_json::json!(["claude-code", "cursor"])
        );
    }

    #[test]
    fn default_agent_settings_reject_project_context() {
        let context = ContextRef {
            environment: EnvironmentRef::Host,
            scope: crate::environment::types::ContextScope::Project {
                project_id: "app".to_string(),
            },
        };

        let error = ensure_global_context(&context).unwrap_err();

        assert!(matches!(error, AppError::Custom { .. }));
    }
}

// src-tauri/src/commands/wizard.rs
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

use crate::environment::types::ContextRef;

const INSTALL_WIZARD_WIDTH: f64 = 680.0;
const INSTALL_WIZARD_HEIGHT: f64 = 560.0;
const INSTALL_WIZARD_MIN_WIDTH: f64 = 620.0;
const INSTALL_WIZARD_MIN_HEIGHT: f64 = 480.0;

fn build_wizard_query(
    entry_point: &str,
    scope: &str,
    project_path: Option<&str>,
    prefill_source: Option<&str>,
    prefill_skill_name: Option<&str>,
    context: Option<&ContextRef>,
) -> Result<String, crate::error::AppError> {
    let mut query_parts = vec![
        format!("entryPoint={}", urlencoding::encode(entry_point)),
        format!("scope={}", urlencoding::encode(scope)),
    ];
    if let Some(path) = project_path {
        query_parts.push(format!("projectPath={}", urlencoding::encode(path)));
    }
    if let Some(source) = prefill_source {
        query_parts.push(format!("prefillSource={}", urlencoding::encode(source)));
    }
    if let Some(name) = prefill_skill_name {
        query_parts.push(format!("prefillSkillName={}", urlencoding::encode(name)));
    }
    if let Some(context) = context {
        let context_json = serde_json::to_string(context)?;
        query_parts.push(format!("context={}", urlencoding::encode(&context_json)));
    }
    Ok(query_parts.join("&"))
}

/// 打开安装向导独立窗口
///
/// 必须为 async —— 同步 command 在主线程执行，
/// 而 WebviewWindowBuilder::build() 也需要主线程，会导致死锁。
/// async command 在异步线程执行，build() 可以安全回调主线程。
#[tauri::command]
#[specta::specta]
pub async fn open_install_wizard(
    app: AppHandle,
    entry_point: String,
    scope: String,
    project_path: Option<String>,
    prefill_source: Option<String>,
    prefill_skill_name: Option<String>,
    context: Option<ContextRef>,
) -> Result<(), crate::error::AppError> {
    // 如果窗口已存在，聚焦并返回
    if let Some(window) = app.get_webview_window("install-wizard") {
        window.set_focus().map_err(|e| crate::error::AppError::Io {
            message: e.to_string(),
        })?;
        return Ok(());
    }

    let query = build_wizard_query(
        &entry_point,
        &scope,
        project_path.as_deref(),
        prefill_source.as_deref(),
        prefill_skill_name.as_deref(),
        context.as_ref(),
    )?;
    let url = WebviewUrl::App(format!("/wizard?{}", query).into());

    let main_window = app
        .get_webview_window("main")
        .ok_or_else(|| crate::error::AppError::Io {
            message: "Main window not found".to_string(),
        })?;

    let _wizard_window = WebviewWindowBuilder::new(&app, "install-wizard", url)
        .title("Install Skills")
        .inner_size(INSTALL_WIZARD_WIDTH, INSTALL_WIZARD_HEIGHT)
        .min_inner_size(INSTALL_WIZARD_MIN_WIDTH, INSTALL_WIZARD_MIN_HEIGHT)
        .resizable(true)
        .maximizable(false)
        .minimizable(false)
        .center()
        .parent(&main_window)
        .map_err(|e| crate::error::AppError::Io {
            message: e.to_string(),
        })?
        .build()
        .map_err(|e| crate::error::AppError::Io {
            message: e.to_string(),
        })?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::environment::types::{ContextRef, ContextScope, EnvironmentRef};

    #[test]
    fn install_wizard_uses_wider_default_window() {
        assert_eq!(INSTALL_WIZARD_WIDTH, 680.0);
        assert_eq!(INSTALL_WIZARD_HEIGHT, 560.0);
        assert_eq!(INSTALL_WIZARD_MIN_WIDTH, 620.0);
        assert_eq!(INSTALL_WIZARD_MIN_HEIGHT, 480.0);
    }

    #[test]
    fn wizard_query_keeps_explicit_context() {
        let query = build_wizard_query(
            "skills-panel",
            "project",
            Some("/home/me/project"),
            None,
            None,
            Some(&ContextRef {
                environment: EnvironmentRef::Wsl {
                    distro_name: "Ubuntu".to_string(),
                },
                scope: ContextScope::Project {
                    project_id: "project-1".to_string(),
                },
            }),
        )
        .expect("build query");

        assert!(query.contains("context="));
        assert!(query.contains("projectPath=%2Fhome%2Fme%2Fproject"));
    }
}

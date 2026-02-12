// src-tauri/src/commands/wizard.rs
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

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
) -> Result<(), crate::error::AppError> {
    // 如果窗口已存在，聚焦并返回
    if let Some(window) = app.get_webview_window("install-wizard") {
        window.set_focus().map_err(|e| crate::error::AppError::Io {
            message: e.to_string(),
        })?;
        return Ok(());
    }

    // 构建 query string
    let mut query_parts = vec![
        format!("entryPoint={}", entry_point),
        format!("scope={}", scope),
    ];
    if let Some(ref path) = project_path {
        query_parts.push(format!(
            "projectPath={}",
            urlencoding::encode(path)
        ));
    }
    if let Some(ref source) = prefill_source {
        query_parts.push(format!(
            "prefillSource={}",
            urlencoding::encode(source)
        ));
    }
    if let Some(ref name) = prefill_skill_name {
        query_parts.push(format!(
            "prefillSkillName={}",
            urlencoding::encode(name)
        ));
    }
    let query = query_parts.join("&");
    let url = WebviewUrl::App(format!("/wizard?{}", query).into());

    let main_window = app
        .get_webview_window("main")
        .ok_or_else(|| crate::error::AppError::Io {
            message: "Main window not found".to_string(),
        })?;

    let _wizard_window = WebviewWindowBuilder::new(&app, "install-wizard", url)
        .title("Add Skills")
        .inner_size(620.0, 560.0)
        .min_inner_size(560.0, 480.0)
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

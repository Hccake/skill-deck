// src-tauri/src/commands/wizard.rs
use tauri::{AppHandle, Manager, State, WebviewUrl, WebviewWindowBuilder};

use crate::application::install_wizard_session::InstallWizardSessionSnapshot;
use crate::environment::types::{ContextRef, ContextScope};
use crate::runtime::RuntimeServiceGraph;

pub const INSTALL_WIZARD_LABEL: &str = "install-wizard";

const INSTALL_WIZARD_WIDTH: f64 = 680.0;
const INSTALL_WIZARD_HEIGHT: f64 = 560.0;
const INSTALL_WIZARD_MIN_WIDTH: f64 = 620.0;
const INSTALL_WIZARD_MIN_HEIGHT: f64 = 480.0;

fn build_wizard_query(
    entry_point: &str,
    context: &ContextRef,
    project_path: Option<&str>,
    prefill_source: Option<&str>,
    prefill_skill_name: Option<&str>,
) -> Result<String, crate::error::AppError> {
    let scope = match context.scope {
        ContextScope::Global => "global",
        ContextScope::Project { .. } => "project",
    };
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
    let context_json = serde_json::to_string(context)?;
    query_parts.push(format!("context={}", urlencoding::encode(&context_json)));
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
    runtime: State<'_, RuntimeServiceGraph>,
    entry_point: String,
    context: ContextRef,
    project_path: Option<String>,
    prefill_source: Option<String>,
    prefill_skill_name: Option<String>,
) -> Result<(), crate::error::AppError> {
    // 如果窗口已存在，聚焦并返回
    if let Some(window) = app.get_webview_window(INSTALL_WIZARD_LABEL) {
        window.set_focus().map_err(|e| crate::error::AppError::Io {
            message: e.to_string(),
        })?;
        runtime.install_wizard_session().activate();
        return Ok(());
    }

    let query = build_wizard_query(
        &entry_point,
        &context,
        project_path.as_deref(),
        prefill_source.as_deref(),
        prefill_skill_name.as_deref(),
    )?;
    let url = WebviewUrl::App(format!("/wizard?{}", query).into());

    let main_window = app
        .get_webview_window("main")
        .ok_or_else(|| crate::error::AppError::Io {
            message: "Main window not found".to_string(),
        })?;

    let _wizard_window = WebviewWindowBuilder::new(&app, INSTALL_WIZARD_LABEL, url)
        .title("Skill Deck")
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

    runtime.install_wizard_session().activate();

    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn get_install_wizard_session(
    app: AppHandle,
    runtime: State<'_, RuntimeServiceGraph>,
) -> InstallWizardSessionSnapshot {
    runtime
        .install_wizard_session()
        .reconcile_window_presence(app.get_webview_window(INSTALL_WIZARD_LABEL).is_some())
}

#[tauri::command]
#[specta::specta]
pub fn focus_install_wizard(
    app: AppHandle,
    runtime: State<'_, RuntimeServiceGraph>,
) -> Result<bool, crate::error::AppError> {
    let Some(window) = app.get_webview_window(INSTALL_WIZARD_LABEL) else {
        runtime
            .install_wizard_session()
            .reconcile_window_presence(false);
        return Ok(false);
    };
    window.show().map_err(|error| crate::error::AppError::Io {
        message: error.to_string(),
    })?;
    window
        .unminimize()
        .map_err(|error| crate::error::AppError::Io {
            message: error.to_string(),
        })?;
    window
        .set_focus()
        .map_err(|error| crate::error::AppError::Io {
            message: error.to_string(),
        })?;
    runtime
        .install_wizard_session()
        .reconcile_window_presence(true);
    Ok(true)
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
        let context = ContextRef {
            environment: EnvironmentRef::Wsl {
                distro_name: "Ubuntu".to_string(),
            },
            scope: ContextScope::Project {
                project_id: "project-1".to_string(),
            },
        };
        let query = build_wizard_query(
            "skills-panel",
            &context,
            Some("/home/me/project"),
            None,
            None,
        )
        .expect("build query");

        assert!(query.contains("context="));
        assert!(query.contains("scope=project"));
        assert!(query.contains("projectPath=%2Fhome%2Fme%2Fproject"));
    }

    #[test]
    fn wizard_query_derives_global_scope_from_context() {
        let context = ContextRef {
            environment: EnvironmentRef::Host,
            scope: ContextScope::Global,
        };

        let query =
            build_wizard_query("skills-panel", &context, None, None, None).expect("build query");

        assert!(query.contains("scope=global"));
        assert!(query.contains("context="));
    }
}

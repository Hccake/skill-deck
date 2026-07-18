use tauri::{AppHandle, Manager, WebviewWindow};
use tauri_specta::Event;

use crate::core::agent_definition::AgentId;
use crate::error::AppError;

const MAIN_WINDOW_LABEL: &str = "main";
const INSTALL_WIZARD_LABEL: &str = "install-wizard";

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub enum AgentConfigurationOutcome {
    Saved,
    Cancelled,
}

#[derive(Debug, Clone, serde::Serialize, specta::Type, tauri_specta::Event)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct AgentConfigurationRequestedEvent {
    pub agent_id: AgentId,
}

#[derive(Debug, Clone, serde::Serialize, specta::Type, tauri_specta::Event)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct AgentConfigurationCompletedEvent {
    pub agent_id: AgentId,
    pub outcome: AgentConfigurationOutcome,
}

#[tauri::command]
#[specta::specta]
pub fn request_agent_configuration(
    app: AppHandle,
    window: WebviewWindow,
    agent_id: AgentId,
) -> Result<(), AppError> {
    require_window(&window, INSTALL_WIZARD_LABEL)?;
    let main = app.get_webview_window(MAIN_WINDOW_LABEL).ok_or_else(|| {
        AppError::CapabilityUnavailable {
            capability: "mainWindow".to_string(),
            path: None,
        }
    })?;
    let _ = main.unminimize();
    let _ = main.set_focus();
    AgentConfigurationRequestedEvent { agent_id }
        .emit_to(&app, MAIN_WINDOW_LABEL)
        .map_err(event_error)
}

#[tauri::command]
#[specta::specta]
pub fn complete_agent_configuration(
    app: AppHandle,
    window: WebviewWindow,
    agent_id: AgentId,
    outcome: AgentConfigurationOutcome,
) -> Result<(), AppError> {
    require_window(&window, MAIN_WINDOW_LABEL)?;
    let _ =
        AgentConfigurationCompletedEvent { agent_id, outcome }.emit_to(&app, INSTALL_WIZARD_LABEL);
    Ok(())
}

fn require_window(window: &WebviewWindow, expected: &str) -> Result<(), AppError> {
    if window.label() == expected {
        Ok(())
    } else {
        Err(AppError::CapabilityUnavailable {
            capability: "agentConfigurationWindow".to_string(),
            path: None,
        })
    }
}

fn event_error(error: tauri::Error) -> AppError {
    AppError::Io {
        message: error.to_string(),
    }
}

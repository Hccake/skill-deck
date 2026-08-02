use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{AppHandle, Manager, State, WebviewWindow};
use tauri_specta::Event;

use crate::core::mutation::{BackendActivitySnapshot, TerminationAdmission};
use crate::error::AppError;
use crate::runtime::RuntimeServiceGraph;

const MAIN_WINDOW_LABEL: &str = "main";
const INSTALL_WIZARD_LABEL: &str = "install-wizard";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub enum LifecycleAction {
    CloseCurrentWindow,
    QuitApplication,
    RestartApplication,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(tag = "status", rename_all = "camelCase")]
#[specta(tag = "status", rename_all = "camelCase")]
pub enum LifecycleActionOutcome {
    Performed,
    Delegated,
    Blocked { snapshot: BackendActivitySnapshot },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type, tauri_specta::Event)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct LifecycleActionRequestedEvent {
    pub action: LifecycleAction,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LifecycleRoute {
    HandleLocally,
    DelegateToWizard,
}

fn resolve_blocked_route(
    action: LifecycleAction,
    origin_label: &str,
    wizard_exists: bool,
) -> LifecycleRoute {
    if origin_label == MAIN_WINDOW_LABEL
        && wizard_exists
        && matches!(
            action,
            LifecycleAction::QuitApplication | LifecycleAction::RestartApplication
        )
    {
        LifecycleRoute::DelegateToWizard
    } else {
        LifecycleRoute::HandleLocally
    }
}

#[tauri::command]
#[specta::specta]
pub fn execute_lifecycle_action(
    app: AppHandle,
    window: WebviewWindow,
    runtime: State<'_, RuntimeServiceGraph>,
    action: LifecycleAction,
) -> Result<LifecycleActionOutcome, AppError> {
    if action == LifecycleAction::CloseCurrentWindow {
        return match runtime.admission().with_idle(|| window.destroy()) {
            Ok(Ok(())) => Ok(LifecycleActionOutcome::Performed),
            Ok(Err(error)) => Err(AppError::Io {
                message: error.to_string(),
            }),
            Err(snapshot) => Ok(LifecycleActionOutcome::Blocked {
                snapshot: *snapshot,
            }),
        };
    }

    match runtime.admission().request_termination() {
        TerminationAdmission::Blocked(snapshot) => {
            let wizard = app.get_webview_window(INSTALL_WIZARD_LABEL);
            if resolve_blocked_route(action, window.label(), wizard.is_some())
                == LifecycleRoute::DelegateToWizard
            {
                let wizard = wizard.expect("wizard existence checked");
                wizard.set_focus().map_err(|error| AppError::Io {
                    message: error.to_string(),
                })?;
                LifecycleActionRequestedEvent { action }
                    .emit_to(&app, wizard.label())
                    .map_err(|error| AppError::Io {
                        message: error.to_string(),
                    })?;
                Ok(LifecycleActionOutcome::Delegated)
            } else {
                Ok(LifecycleActionOutcome::Blocked { snapshot })
            }
        }
        TerminationAdmission::AlreadyRequested => Ok(LifecycleActionOutcome::Performed),
        TerminationAdmission::Acquired => match action {
            LifecycleAction::QuitApplication => {
                app.exit(0);
                Ok(LifecycleActionOutcome::Performed)
            }
            LifecycleAction::RestartApplication => app.restart(),
            LifecycleAction::CloseCurrentWindow => unreachable!("handled before admission"),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::{resolve_blocked_route, LifecycleAction, LifecycleRoute};

    #[test]
    fn main_application_action_delegates_to_existing_wizard() {
        assert_eq!(
            resolve_blocked_route(LifecycleAction::QuitApplication, "main", true),
            LifecycleRoute::DelegateToWizard
        );
        assert_eq!(
            resolve_blocked_route(LifecycleAction::RestartApplication, "main", true),
            LifecycleRoute::DelegateToWizard
        );
    }

    #[test]
    fn close_current_window_never_delegates() {
        assert_eq!(
            resolve_blocked_route(LifecycleAction::CloseCurrentWindow, "main", true),
            LifecycleRoute::HandleLocally
        );
    }

    #[test]
    fn wizard_application_action_does_not_delegate_back_to_itself() {
        assert_eq!(
            resolve_blocked_route(LifecycleAction::QuitApplication, "install-wizard", true,),
            LifecycleRoute::HandleLocally
        );
    }

    #[test]
    fn main_handles_blocked_action_locally_without_a_wizard() {
        assert_eq!(
            resolve_blocked_route(LifecycleAction::QuitApplication, "main", false),
            LifecycleRoute::HandleLocally
        );
    }
}

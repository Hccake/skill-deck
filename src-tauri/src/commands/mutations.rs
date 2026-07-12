use tauri::State;

use crate::core::mutation::{ActiveMutation, SingleMutationController};
use crate::error::AppError;

#[tauri::command]
#[specta::specta]
pub fn get_active_mutation(
    controller: State<'_, SingleMutationController>,
) -> Option<ActiveMutation> {
    controller.active()
}

#[tauri::command]
#[specta::specta]
pub fn request_cancel_active_mutation(
    controller: State<'_, SingleMutationController>,
) -> Result<bool, AppError> {
    controller.request_cancel()
}

#[cfg(test)]
mod tests {
    use crate::core::mutation::SingleMutationController;

    #[test]
    fn new_controller_has_no_active_mutation() {
        assert!(SingleMutationController::default().active().is_none());
    }
}

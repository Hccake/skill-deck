use tauri::State;

use crate::core::mutation::{MutationSnapshot, SingleMutationController};
use crate::error::AppError;

#[tauri::command]
#[specta::specta]
pub fn get_active_mutation(controller: State<'_, SingleMutationController>) -> MutationSnapshot {
    controller.snapshot()
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
        let snapshot = SingleMutationController::default().snapshot();
        assert_eq!(snapshot.revision, 0);
        assert!(snapshot.active.is_none());
    }
}

use tauri::State;

use crate::core::mutation::{BackendActivitySnapshot, MutationSnapshot};
use crate::error::AppError;
use crate::runtime::RuntimeServiceGraph;

#[tauri::command]
#[specta::specta]
pub fn get_active_mutation(runtime: State<'_, RuntimeServiceGraph>) -> MutationSnapshot {
    runtime.mutation().snapshot()
}

#[tauri::command]
#[specta::specta]
pub fn get_backend_activity(runtime: State<'_, RuntimeServiceGraph>) -> BackendActivitySnapshot {
    runtime.mutation().activity_snapshot()
}

#[tauri::command]
#[specta::specta]
pub fn request_cancel_active_mutation(
    runtime: State<'_, RuntimeServiceGraph>,
) -> Result<bool, AppError> {
    runtime.mutation().request_cancel()
}

#[cfg(test)]
mod tests {
    use crate::core::mutation::SingleMutationController;

    #[test]
    fn new_controller_has_no_active_mutation() {
        let controller = SingleMutationController::default();
        let snapshot = controller.snapshot();
        assert_eq!(snapshot.revision, 0);
        assert!(snapshot.active.is_none());
        let activity = controller.activity_snapshot();
        assert_eq!(activity.revision, 0);
        assert!(activity.mutation.is_none());
        assert!(activity.lifecycle.is_none());
    }
}

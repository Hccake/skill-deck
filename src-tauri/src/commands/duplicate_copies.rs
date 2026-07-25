use tauri::State;

pub use crate::application::duplicate_cleanup::DuplicateCleanupResult;
use crate::core::agent_definition::AgentId;
use crate::environment::types::ContextRef;
use crate::error::AppError;
use crate::runtime::RuntimeServiceGraph;

#[tauri::command]
#[specta::specta]
pub async fn cleanup_duplicate_agent_copies(
    context: ContextRef,
    skill_name: String,
    agents: Vec<AgentId>,
    runtime: State<'_, RuntimeServiceGraph>,
) -> Result<Vec<DuplicateCleanupResult>, AppError> {
    let diagnostic_context = context.clone();
    let result = runtime
        .duplicate_cleanup()
        .execute(
            context,
            skill_name,
            agents,
            runtime.remove(),
            runtime.mutation(),
        )
        .await;
    crate::diagnostics::record_command_result(
        crate::diagnostics::DiagnosticOperation::DuplicateCleanup,
        &result,
        &diagnostic_context,
    );
    result
}

use tauri::State;

use crate::application::agent_selection::{
    AgentSelectionIntent, AgentSelectionSubmission, ConfirmInstallAgentSelectionOutcome,
    InstallAgentSelectionSnapshot,
};
use crate::application::install_agent_selection;
use crate::environment::types::SkillLocationRef;
use crate::error::AppError;
use crate::runtime::RuntimeServiceGraph;

#[tauri::command]
#[specta::specta]
pub async fn get_install_agent_selection(
    context: SkillLocationRef,
    agent_selection_intent: AgentSelectionIntent,
    runtime: State<'_, RuntimeServiceGraph>,
) -> Result<InstallAgentSelectionSnapshot, AppError> {
    install_agent_selection::get_install_agent_selection(
        context,
        agent_selection_intent,
        runtime.agent_selection_facts(),
        runtime.agent_selection_targets(),
        runtime.wsl(),
    )
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn confirm_install_agent_selection(
    context: SkillLocationRef,
    submission: AgentSelectionSubmission,
    agent_selection_intent: AgentSelectionIntent,
    runtime: State<'_, RuntimeServiceGraph>,
) -> Result<ConfirmInstallAgentSelectionOutcome, AppError> {
    install_agent_selection::confirm_install_agent_selection(
        context,
        submission,
        agent_selection_intent,
        runtime.agent_selection_facts(),
        runtime.agent_selection_targets(),
        runtime.wsl(),
        runtime.admission(),
    )
    .await
}

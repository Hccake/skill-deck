use tauri::State;

use crate::application::agent_selection::{
    apply_initial_agent_selection, build_agent_selection_catalog, DefaultSelectionWarning,
    InstallAgentSelectionSnapshot,
};
use crate::application::default_agents;
use crate::application::install_planner::InstallPlanningFactSource;
use crate::environment::agent_environment::DetectionState;
use crate::environment::types::{SkillLocation, SkillLocationRef};
use crate::error::AppError;
use crate::runtime::RuntimeServiceGraph;

#[tauri::command]
#[specta::specta]
pub async fn get_install_agent_selection(
    context: SkillLocationRef,
    explicit_agent_ids: Vec<String>,
    runtime: State<'_, RuntimeServiceGraph>,
) -> Result<InstallAgentSelectionSnapshot, AppError> {
    let facts = runtime.agent_selection_facts().current(&context).await?;
    let mut catalog = build_agent_selection_catalog(
        &context,
        &facts.agent_runtime,
        &facts.eve_targets,
        runtime.agent_selection_targets(),
    )
    .await?;

    let explicit = !explicit_agent_ids.is_empty();
    let (initial_agent_ids, default_selection_warning) = if explicit {
        (explicit_agent_ids, None)
    } else {
        match default_agents::get_default_target_agents(
            SkillLocationRef {
                environment: context.environment.clone(),
                scope: SkillLocation::Global,
            },
            runtime.wsl(),
            runtime.agents(),
        )
        .await
        {
            Ok(Some(defaults)) => (
                match context.scope {
                    SkillLocation::Global => defaults.global,
                    SkillLocation::Project { .. } => defaults.project,
                },
                None,
            ),
            Ok(None) => (fallback_agent_ids(&catalog.snapshot.agents), None),
            Err(_) => (
                fallback_agent_ids(&catalog.snapshot.agents),
                Some(DefaultSelectionWarning::ReadFailed),
            ),
        }
    };
    apply_initial_agent_selection(&mut catalog, &initial_agent_ids);
    if !explicit {
        catalog.snapshot.unavailable_explicit_agents.clear();
    }

    Ok(InstallAgentSelectionSnapshot {
        selection: catalog.snapshot,
        default_selection_warning,
    })
}

fn fallback_agent_ids(
    agents: &[crate::application::agent_selection::AgentSelectionAgent],
) -> Vec<String> {
    ["claude-code", "cursor"]
        .into_iter()
        .filter(|candidate| {
            agents.iter().any(|agent| {
                agent.id.as_str() == *candidate && agent.detection == DetectionState::Detected
            })
        })
        .map(str::to_string)
        .collect()
}

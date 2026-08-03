use tauri::State;

use crate::application::runtime_admission::RuntimeAdmissionCoordinator;
use crate::application::{default_agents, environment_settings};
use crate::commands::agents::AgentCommandError;
use crate::core::mutation::MutationKind;
use crate::core::{read_config, skill_lock};
use crate::environment::types::{ContextRef, ContextScope, EnvironmentRef};
use crate::error::AppError;
use crate::models::SkillDeckConfig;
use crate::runtime::RuntimeServiceGraph;

#[tauri::command]
#[specta::specta]
pub fn get_config() -> Result<SkillDeckConfig, AppError> {
    read_config()
}

#[tauri::command]
#[specta::specta]
pub fn save_config(
    config: SkillDeckConfig,
    runtime: State<'_, RuntimeServiceGraph>,
) -> Result<(), AppError> {
    save_config_with_admission(config, runtime.admission(), |config| {
        environment_settings::save_config_preserving_wsl_setting(config)
    })
}

fn save_config_with_admission(
    config: SkillDeckConfig,
    admission: &RuntimeAdmissionCoordinator,
    persist: impl FnOnce(SkillDeckConfig) -> Result<(), AppError>,
) -> Result<(), AppError> {
    let _permit = admission.begin_mutation(
        MutationKind::UpdateSettings,
        ContextRef {
            environment: EnvironmentRef::Host,
            scope: ContextScope::Global,
        },
    )?;
    persist(config)
}

#[tauri::command]
#[specta::specta]
pub async fn set_wsl_integration_enabled(
    enabled: bool,
    runtime: State<'_, RuntimeServiceGraph>,
) -> Result<crate::environment::project_service::EnvironmentDiscoverySnapshot, AppError> {
    environment_settings::WslIntegrationSettings::new(
        runtime.wsl(),
        runtime.admission(),
        runtime.payloads(),
    )
    .set_enabled(enabled)
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn get_default_target_agents(
    context: ContextRef,
    runtime: State<'_, RuntimeServiceGraph>,
) -> Result<Option<skill_lock::DefaultTargetAgents>, AppError> {
    default_agents::get_default_target_agents(context, runtime.wsl(), runtime.agents()).await
}

#[tauri::command]
#[specta::specta]
pub async fn save_default_target_agents(
    context: ContextRef,
    defaults: skill_lock::DefaultTargetAgents,
    expected_registry_revision: String,
    runtime: State<'_, RuntimeServiceGraph>,
) -> Result<(), AgentCommandError> {
    default_agents::save_default_target_agents(
        context,
        defaults,
        expected_registry_revision,
        runtime.wsl(),
        runtime.agents(),
        runtime.admission(),
    )
    .await
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;
    use crate::application::runtime_admission::{
        RuntimeAdmissionCoordinator, WizardAdmission, WizardWindowPresence,
    };

    #[test]
    fn save_config_does_not_persist_after_wizard_admission_denial() {
        let admission = RuntimeAdmissionCoordinator::default();
        let WizardAdmission::Reserved(_reservation) = admission
            .admit_install_wizard(WizardWindowPresence::Absent)
            .expect("wizard reservation")
        else {
            panic!("expected wizard reservation");
        };
        let persisted = Cell::new(false);

        let error = save_config_with_admission(SkillDeckConfig::default(), &admission, |_| {
            persisted.set(true);
            Ok(())
        })
        .expect_err("wizard must block config persistence");

        assert_eq!(error, AppError::InstallWizardActive);
        assert!(!persisted.get());
    }
}

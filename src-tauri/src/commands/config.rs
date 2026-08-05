use tauri::State;

use crate::application::environment_settings;
use crate::application::runtime_admission::RuntimeAdmissionCoordinator;
use crate::core::mutation::MutationKind;
use crate::core::read_config;
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

use tauri::State;

use crate::application::recovery::RecoveryResourceStatus;
use crate::application::runtime_admission::RuntimeAdmissionCoordinator;
use crate::core::mutation::MutationKind;
use crate::environment::types::{ContextRef, ContextScope, EnvironmentRef};
use crate::error::{AppError, RecoveryResourceId};
use crate::runtime::RuntimeServiceGraph;

#[tauri::command]
#[specta::specta]
pub async fn list_recovery_resources(
    runtime: State<'_, RuntimeServiceGraph>,
) -> Result<Vec<RecoveryResourceStatus>, AppError> {
    runtime.recovery().list().await
}

#[tauri::command]
#[specta::specta]
pub async fn get_recovery_resource_status(
    resource_id: RecoveryResourceId,
    runtime: State<'_, RuntimeServiceGraph>,
) -> Result<RecoveryResourceStatus, AppError> {
    runtime.recovery().status(&resource_id).await
}

#[tauri::command]
#[specta::specta]
pub async fn confirm_recovery_resource_resolved(
    resource_id: RecoveryResourceId,
    expected_revision: String,
    runtime: State<'_, RuntimeServiceGraph>,
) -> Result<(), AppError> {
    let status = runtime.recovery().status(&resource_id).await?;
    confirm_recovery_with_admission(
        runtime.admission(),
        status.environment.unwrap_or(EnvironmentRef::Host),
        || async {
            runtime
                .recovery()
                .confirm_resolved(&resource_id, &expected_revision)
                .await
        },
    )
    .await
}

async fn confirm_recovery_with_admission<Operation, OperationFuture>(
    admission: &RuntimeAdmissionCoordinator,
    environment: EnvironmentRef,
    operation: Operation,
) -> Result<(), AppError>
where
    Operation: FnOnce() -> OperationFuture,
    OperationFuture: std::future::Future<Output = Result<(), AppError>>,
{
    let _permit = admission.begin_mutation(
        MutationKind::ResolveRecovery,
        ContextRef {
            environment,
            scope: ContextScope::Global,
        },
    )?;
    operation().await
}

#[tauri::command]
#[specta::specta]
pub async fn open_recovery_resource(
    resource_id: RecoveryResourceId,
    runtime: State<'_, RuntimeServiceGraph>,
) -> Result<(), AppError> {
    let target = runtime.recovery().open_target(&resource_id)?;
    crate::environment::opener::open_authorized_resource(&target)
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;
    use crate::application::runtime_admission::{
        RuntimeAdmissionCoordinator, WizardAdmission, WizardWindowPresence,
    };

    #[tokio::test]
    async fn recovery_cleanup_does_not_start_after_wizard_admission_denial() {
        let admission = RuntimeAdmissionCoordinator::default();
        let WizardAdmission::Reserved(_reservation) = admission
            .admit_install_wizard(WizardWindowPresence::Absent)
            .expect("wizard reservation")
        else {
            panic!("expected wizard reservation");
        };
        let cleaned = Cell::new(false);

        let error = confirm_recovery_with_admission(&admission, EnvironmentRef::Host, || async {
            cleaned.set(true);
            Ok(())
        })
        .await
        .expect_err("wizard must block recovery cleanup");

        assert_eq!(error, AppError::InstallWizardActive);
        assert!(!cleaned.get());
    }
}

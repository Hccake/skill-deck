use std::future::Future;

use tauri::{State, WebviewWindow};

use crate::application::install::{
    InstallOperation, InstallPreviewOutcome, InstallRequest, InstallResponse,
};
use crate::application::mutation::plan::PreviewToken;
use crate::application::runtime_admission::{MutationPermit, RuntimeAdmissionCoordinator};
use crate::commands::window_role::WindowRole;
use crate::core::mutation::MutationPhase;
use crate::environment::types::SkillLocationRef;
use crate::error::AppError;
use crate::runtime::RuntimeServiceGraph;

#[tauri::command]
#[specta::specta]
pub async fn preview_install(
    request: InstallRequest,
    window: WebviewWindow,
    runtime: State<'_, RuntimeServiceGraph>,
) -> Result<InstallPreviewOutcome, AppError> {
    let operation =
        install_operation_for_window(WindowRole::from_label(window.label()), "preview_install")?;
    runtime.install().preview(operation, &request).await
}

#[tauri::command]
#[specta::specta]
pub async fn install_skills(
    request: InstallRequest,
    expected_token: PreviewToken,
    window: WebviewWindow,
    runtime: State<'_, RuntimeServiceGraph>,
) -> Result<InstallResponse, AppError> {
    let admission = runtime.admission().clone();
    let role = WindowRole::from_label(window.label());
    execute_install_for_window(
        &admission,
        role,
        request.context.clone(),
        |operation, guard| async move {
            guard.transition(MutationPhase::Acquiring, None, true);
            runtime
                .install()
                .execute(operation, &request, expected_token, guard.cancellation())
                .await
        },
    )
    .await
}

async fn execute_install_for_window<T, Execute, ExecuteFuture>(
    admission: &RuntimeAdmissionCoordinator,
    role: WindowRole,
    context: SkillLocationRef,
    execute: Execute,
) -> Result<T, AppError>
where
    Execute: FnOnce(InstallOperation, MutationPermit) -> ExecuteFuture,
    ExecuteFuture: Future<Output = Result<T, AppError>>,
{
    let operation = install_operation_for_window(role, "install_skills")?;
    let guard = begin_install_for_window(admission, role, operation, context)?;
    execute(operation, guard).await
}

fn begin_install_for_window(
    admission: &RuntimeAdmissionCoordinator,
    role: WindowRole,
    operation: InstallOperation,
    context: SkillLocationRef,
) -> Result<MutationPermit, AppError> {
    match role {
        WindowRole::Main => admission.begin_mutation(operation.mutation_kind(), context),
        WindowRole::InstallWizard => admission.begin_install_from_active_wizard(context),
        WindowRole::Unknown => Err(AppError::CapabilityUnavailable {
            capability: "install_skills".to_string(),
            path: None,
        }),
    }
}

fn install_operation_for_window(
    role: WindowRole,
    capability: &str,
) -> Result<InstallOperation, AppError> {
    match role {
        WindowRole::Main => Ok(InstallOperation::Repair),
        WindowRole::InstallWizard => Ok(InstallOperation::Install),
        WindowRole::Unknown => Err(AppError::CapabilityUnavailable {
            capability: capability.to_string(),
            path: None,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::runtime_admission::{
        RuntimeAdmissionCoordinator, WizardAdmission, WizardWindowPresence,
    };
    use crate::commands::window_role::WindowRole;
    use crate::environment::types::{EnvironmentRef, SkillLocation, SkillLocationRef};
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn native_global() -> SkillLocationRef {
        SkillLocationRef {
            environment: EnvironmentRef::Native,
            scope: SkillLocation::Global,
        }
    }

    #[test]
    fn window_role_selects_install_or_repair_identity() {
        assert_eq!(
            install_operation_for_window(WindowRole::Main, "preview_install").unwrap(),
            crate::application::install::InstallOperation::Repair
        );
        assert_eq!(
            install_operation_for_window(WindowRole::InstallWizard, "preview_install").unwrap(),
            crate::application::install::InstallOperation::Install
        );
        assert!(matches!(
            install_operation_for_window(WindowRole::Unknown, "preview_install"),
            Err(AppError::CapabilityUnavailable { capability, path: None })
                if capability == "preview_install"
        ));
    }

    #[tokio::test]
    async fn main_window_source_repair_registers_repair_activity() {
        let admission = RuntimeAdmissionCoordinator::default();
        let observed_admission = admission.clone();

        execute_install_for_window(
            &admission,
            WindowRole::Main,
            native_global(),
            |operation, guard| async move {
                let _guard = guard;
                assert_eq!(operation, InstallOperation::Repair);
                assert_eq!(
                    observed_admission.active().map(|mutation| mutation.kind),
                    Some(crate::core::mutation::MutationKind::Repair)
                );
                Ok(())
            },
        )
        .await
        .expect("main-window source repair admitted");

        assert!(admission.active().is_none());
    }

    #[tokio::test]
    async fn install_handler_runs_the_executor_only_after_role_admission() {
        let admission = RuntimeAdmissionCoordinator::default();
        let WizardAdmission::Reserved(reservation) = admission
            .admit_install_wizard(WizardWindowPresence::Absent)
            .expect("wizard reservation admitted")
        else {
            panic!("expected reservation");
        };
        reservation.activate("wizard-1".to_string());

        let calls = AtomicUsize::new(0);
        execute_install_for_window(
            &admission,
            WindowRole::InstallWizard,
            native_global(),
            |_, _| async {
                calls.fetch_add(1, Ordering::SeqCst);
                Ok(())
            },
        )
        .await
        .expect("wizard-owned install admitted");
        assert_eq!(calls.load(Ordering::SeqCst), 1);

        assert_eq!(
            execute_install_for_window(
                &admission,
                WindowRole::Main,
                native_global(),
                |_, _| async {
                    calls.fetch_add(1, Ordering::SeqCst);
                    Ok(())
                }
            )
            .await
            .unwrap_err(),
            AppError::InstallWizardActive
        );
        assert!(matches!(
            execute_install_for_window(
                &admission,
                WindowRole::Unknown,
                native_global(),
                |_, _| async {
                    calls.fetch_add(1, Ordering::SeqCst);
                    Ok(())
                },
            )
            .await,
            Err(AppError::CapabilityUnavailable { capability, path: None })
                if capability == "install_skills"
        ));
        assert_eq!(calls.load(Ordering::SeqCst), 1);

        let unavailable_admission = RuntimeAdmissionCoordinator::default();
        assert_eq!(
            execute_install_for_window(
                &unavailable_admission,
                WindowRole::InstallWizard,
                native_global(),
                |_, _| async {
                    calls.fetch_add(1, Ordering::SeqCst);
                    Ok(())
                },
            )
            .await
            .unwrap_err(),
            AppError::InstallWizardSessionUnavailable
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }
}

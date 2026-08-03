use std::future::Future;

use tauri::{State, WebviewWindow};

use crate::application::install::{InstallPreview, InstallRequest, InstallResponse};
use crate::application::mutation::plan::PreviewToken;
use crate::application::runtime_admission::{MutationPermit, RuntimeAdmissionCoordinator};
use crate::commands::window_role::WindowRole;
use crate::core::mutation::{MutationKind, MutationPhase};
use crate::environment::types::ContextRef;
use crate::error::AppError;
use crate::runtime::RuntimeServiceGraph;

#[tauri::command]
#[specta::specta]
pub async fn preview_install(
    request: InstallRequest,
    runtime: State<'_, RuntimeServiceGraph>,
) -> Result<InstallPreview, AppError> {
    runtime.install().preview(&request).await
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
    execute_install_for_window(
        &admission,
        WindowRole::from_label(window.label()),
        request.context.clone(),
        |guard| async move {
            guard.transition(MutationPhase::Acquiring, None, true);
            runtime
                .install()
                .execute(&request, expected_token, guard.cancellation())
                .await
        },
    )
    .await
}

async fn execute_install_for_window<T, Execute, ExecuteFuture>(
    admission: &RuntimeAdmissionCoordinator,
    role: WindowRole,
    context: ContextRef,
    execute: Execute,
) -> Result<T, AppError>
where
    Execute: FnOnce(MutationPermit) -> ExecuteFuture,
    ExecuteFuture: Future<Output = Result<T, AppError>>,
{
    let guard = begin_install_for_window(admission, role, context)?;
    execute(guard).await
}

fn begin_install_for_window(
    admission: &RuntimeAdmissionCoordinator,
    role: WindowRole,
    context: ContextRef,
) -> Result<MutationPermit, AppError> {
    match role {
        WindowRole::Main => admission.begin_mutation(MutationKind::Install, context),
        WindowRole::InstallWizard => admission.begin_install_from_active_wizard(context),
        WindowRole::Unknown => Err(AppError::CapabilityUnavailable {
            capability: "install_skills".to_string(),
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
    use crate::environment::types::{ContextRef, ContextScope, EnvironmentRef};
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn host_global() -> ContextRef {
        ContextRef {
            environment: EnvironmentRef::Host,
            scope: ContextScope::Global,
        }
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
            host_global(),
            |_| async {
                calls.fetch_add(1, Ordering::SeqCst);
                Ok(())
            },
        )
        .await
        .expect("wizard-owned install admitted");
        assert_eq!(calls.load(Ordering::SeqCst), 1);

        assert_eq!(
            execute_install_for_window(&admission, WindowRole::Main, host_global(), |_| async {
                calls.fetch_add(1, Ordering::SeqCst);
                Ok(())
            })
            .await
            .unwrap_err(),
            AppError::InstallWizardActive
        );
        assert!(matches!(
            execute_install_for_window(
                &admission,
                WindowRole::Unknown,
                host_global(),
                |_| async {
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
                host_global(),
                |_| async {
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

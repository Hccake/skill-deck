use tauri::State;

use crate::application::github_credentials::{
    GithubCredentialClearResult, GithubCredentialSaveResult, GithubCredentialStatus,
};
use crate::application::runtime_admission::RuntimeAdmissionCoordinator;
use crate::core::mutation::MutationKind;
use crate::environment::types::{ContextRef, ContextScope, EnvironmentRef};
use crate::error::AppError;
use crate::runtime::RuntimeServiceGraph;

fn host_global() -> ContextRef {
    ContextRef {
        environment: EnvironmentRef::Host,
        scope: ContextScope::Global,
    }
}

#[tauri::command]
#[specta::specta]
pub async fn get_github_credential_status(
    runtime: State<'_, RuntimeServiceGraph>,
) -> Result<GithubCredentialStatus, AppError> {
    Ok(runtime.github_credentials().status().await)
}

#[tauri::command]
#[specta::specta]
pub async fn save_github_credential(
    token: String,
    runtime: State<'_, RuntimeServiceGraph>,
) -> Result<GithubCredentialSaveResult, AppError> {
    with_github_credential_write(runtime.admission(), || async {
        runtime.github_credentials().save(&token).await
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn clear_github_credential(
    runtime: State<'_, RuntimeServiceGraph>,
) -> Result<GithubCredentialClearResult, AppError> {
    with_github_credential_write(runtime.admission(), || async {
        runtime.github_credentials().clear().await
    })
    .await
}

async fn with_github_credential_write<T, Operation, OperationFuture>(
    admission: &RuntimeAdmissionCoordinator,
    operation: Operation,
) -> Result<T, AppError>
where
    Operation: FnOnce() -> OperationFuture,
    OperationFuture: std::future::Future<Output = T>,
{
    let _permit = admission.begin_mutation(MutationKind::ManageGithubCredential, host_global())?;
    Ok(operation().await)
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;
    use crate::application::runtime_admission::{
        RuntimeAdmissionCoordinator, WizardAdmission, WizardWindowPresence,
    };

    #[tokio::test]
    async fn credential_write_does_not_start_after_wizard_admission_denial() {
        let admission = RuntimeAdmissionCoordinator::default();
        let WizardAdmission::Reserved(_reservation) = admission
            .admit_install_wizard(WizardWindowPresence::Absent)
            .expect("wizard reservation")
        else {
            panic!("expected wizard reservation");
        };
        let started = Cell::new(false);

        let error = with_github_credential_write(&admission, || async {
            started.set(true);
        })
        .await
        .expect_err("wizard must block credential writes");

        assert_eq!(error, AppError::InstallWizardActive);
        assert!(!started.get());
    }
}

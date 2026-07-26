use tauri::State;

use crate::application::github_credentials::{
    GithubCredentialClearResult, GithubCredentialSaveResult, GithubCredentialStatus,
};
use crate::error::AppError;
use crate::runtime::RuntimeServiceGraph;

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
    Ok(runtime.github_credentials().save(&token).await)
}

#[tauri::command]
#[specta::specta]
pub async fn clear_github_credential(
    runtime: State<'_, RuntimeServiceGraph>,
) -> Result<GithubCredentialClearResult, AppError> {
    Ok(runtime.github_credentials().clear().await)
}

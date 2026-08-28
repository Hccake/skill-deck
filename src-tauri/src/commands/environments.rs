use tauri::State;

use crate::application::environment_projects;
use crate::environment::project_service;
pub use crate::environment::project_service::{EnvironmentDiscoverySnapshot, EnvironmentInfo};
use crate::environment::types::{AddProjectResult, EnvironmentRef, ProjectInfo};
use crate::error::AppError;
use crate::runtime::RuntimeServiceGraph;

#[tauri::command]
#[specta::specta]
pub async fn list_environments(
    runtime: State<'_, RuntimeServiceGraph>,
) -> Result<EnvironmentDiscoverySnapshot, AppError> {
    project_service::list_environments(runtime.wsl()).await
}

#[tauri::command]
#[specta::specta]
pub async fn connect_environment(
    distro_name: String,
    runtime: State<'_, RuntimeServiceGraph>,
) -> Result<EnvironmentInfo, AppError> {
    project_service::connect_environment(distro_name, runtime.wsl()).await
}

#[tauri::command]
#[specta::specta]
pub async fn map_environment_path(
    environment: EnvironmentRef,
    path: String,
    runtime: State<'_, RuntimeServiceGraph>,
) -> Result<String, AppError> {
    project_service::map_environment_path(environment, path, runtime.wsl()).await
}

#[tauri::command]
#[specta::specta]
pub async fn list_environment_projects(
    environment: EnvironmentRef,
    runtime: State<'_, RuntimeServiceGraph>,
) -> Result<Vec<ProjectInfo>, AppError> {
    project_service::list_environment_projects(environment, runtime.wsl(), runtime.projects()).await
}

#[tauri::command]
#[specta::specta]
pub async fn add_environment_project(
    environment: EnvironmentRef,
    native_path: String,
    runtime: State<'_, RuntimeServiceGraph>,
) -> Result<AddProjectResult, AppError> {
    environment_projects::add_environment_project(
        environment,
        native_path,
        runtime.wsl(),
        runtime.projects(),
        runtime.admission(),
    )
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn remove_environment_project(
    environment: EnvironmentRef,
    project_id: String,
    runtime: State<'_, RuntimeServiceGraph>,
) -> Result<Vec<ProjectInfo>, AppError> {
    environment_projects::remove_environment_project(
        environment,
        project_id,
        runtime.wsl(),
        runtime.projects(),
        runtime.admission(),
        runtime.library_application(),
    )
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn set_environment_project_cross_storage_warning(
    environment: EnvironmentRef,
    project_id: String,
    suppressed: bool,
    runtime: State<'_, RuntimeServiceGraph>,
) -> Result<ProjectInfo, AppError> {
    environment_projects::set_environment_project_cross_storage_warning(
        environment,
        project_id,
        suppressed,
        runtime.wsl(),
        runtime.projects(),
        runtime.admission(),
    )
    .await
}

#[tauri::command]
#[specta::specta]
pub fn retry_native_project_migration(
    runtime: State<'_, RuntimeServiceGraph>,
) -> Result<Vec<ProjectInfo>, AppError> {
    environment_projects::retry_native_project_migration(runtime.projects(), runtime.admission())
}

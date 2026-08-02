use crate::application::runtime_admission::{MutationPermit, RuntimeAdmissionCoordinator};
use crate::core::mutation::MutationKind;
use crate::core::projects::ProjectMigrationRegistry;
use crate::environment::project_service;
use crate::environment::types::{
    AddProjectResult, ContextRef, ContextScope, EnvironmentRef, ProjectInfo,
};
use crate::environment::wsl::WslRuntime;
use crate::error::AppError;

fn begin_project_mutation(
    admission: &RuntimeAdmissionCoordinator,
    kind: MutationKind,
    environment: EnvironmentRef,
    project_id: Option<&str>,
) -> Result<MutationPermit, AppError> {
    let scope = project_id.map_or(ContextScope::Global, |project_id| ContextScope::Project {
        project_id: project_id.to_string(),
    });
    admission.begin_mutation(kind, ContextRef { environment, scope })
}

pub async fn add_environment_project(
    environment: EnvironmentRef,
    native_path: String,
    registry: &WslRuntime,
    migration: &ProjectMigrationRegistry,
    admission: &RuntimeAdmissionCoordinator,
) -> Result<AddProjectResult, AppError> {
    let _permit = begin_project_mutation(
        admission,
        MutationKind::AddProject,
        environment.clone(),
        None,
    )?;
    project_service::add_environment_project(environment, native_path, registry, migration).await
}

pub async fn remove_environment_project(
    environment: EnvironmentRef,
    project_id: String,
    registry: &WslRuntime,
    migration: &ProjectMigrationRegistry,
    admission: &RuntimeAdmissionCoordinator,
) -> Result<Vec<ProjectInfo>, AppError> {
    let _permit = begin_project_mutation(
        admission,
        MutationKind::RemoveProject,
        environment.clone(),
        Some(&project_id),
    )?;
    project_service::remove_environment_project(environment, project_id, registry, migration).await
}

pub async fn set_environment_project_cross_storage_warning(
    environment: EnvironmentRef,
    project_id: String,
    suppressed: bool,
    registry: &WslRuntime,
    migration: &ProjectMigrationRegistry,
    admission: &RuntimeAdmissionCoordinator,
) -> Result<ProjectInfo, AppError> {
    let _permit = begin_project_mutation(
        admission,
        MutationKind::UpdateProjectPreference,
        environment.clone(),
        Some(&project_id),
    )?;
    project_service::set_environment_project_cross_storage_warning(
        environment,
        project_id,
        suppressed,
        registry,
        migration,
    )
    .await
}

pub fn retry_host_project_migration(
    migration: &ProjectMigrationRegistry,
    admission: &RuntimeAdmissionCoordinator,
) -> Result<Vec<ProjectInfo>, AppError> {
    let _permit = begin_project_mutation(
        admission,
        MutationKind::ProjectMigration,
        EnvironmentRef::Host,
        None,
    )?;
    project_service::retry_host_project_migration(migration)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_mutation_captures_environment_and_scope() {
        let admission = RuntimeAdmissionCoordinator::default();
        let environment = EnvironmentRef::Wsl {
            distro_name: "Ubuntu".to_string(),
        };
        let permit = begin_project_mutation(
            &admission,
            MutationKind::RemoveProject,
            environment.clone(),
            Some("project-1"),
        )
        .expect("begin project mutation");

        let active = admission.snapshot().active.expect("active mutation");
        assert_eq!(active.context.environment, environment);
        assert_eq!(
            active.context.scope,
            ContextScope::Project {
                project_id: "project-1".to_string(),
            }
        );
        assert!(!active.cancelable);
        drop(permit);
    }

    #[test]
    fn project_mutation_is_rejected_while_another_write_is_active() {
        let admission = RuntimeAdmissionCoordinator::default();
        let _permit = admission
            .begin_mutation(
                MutationKind::Install,
                ContextRef {
                    environment: EnvironmentRef::Host,
                    scope: ContextScope::Global,
                },
            )
            .expect("begin install");

        assert!(matches!(
            begin_project_mutation(
                &admission,
                MutationKind::AddProject,
                EnvironmentRef::Host,
                None,
            ),
            Err(AppError::MutationBusy)
        ));
    }
}

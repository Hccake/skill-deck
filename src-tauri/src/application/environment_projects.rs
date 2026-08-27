use std::future::Future;

use crate::application::library_application::{
    ApplyLibraryApplicationRequest, LibraryApplicationDraft, LibraryApplicationResponse,
    ProjectLibraryDetachment,
};
use crate::application::mutation::result::MutationUnitStatus;
use crate::application::runtime_admission::{MutationPermit, RuntimeAdmissionCoordinator};
use crate::core::mutation::{CancellationSignal, MutationKind};
use crate::core::projects::ProjectMigrationRegistry;
use crate::environment::project_service;
use crate::environment::types::{
    AddProjectResult, EnvironmentRef, ProjectInfo, SkillLocation, SkillLocationRef,
};
use crate::environment::wsl::WslRuntime;
use crate::error::AppError;

fn begin_project_mutation(
    admission: &RuntimeAdmissionCoordinator,
    kind: MutationKind,
    environment: EnvironmentRef,
    project_id: Option<&str>,
) -> Result<MutationPermit, AppError> {
    let scope = project_id.map_or(SkillLocation::Global, |project_id| SkillLocation::Project {
        project_id: project_id.to_string(),
    });
    admission.begin_mutation(kind, SkillLocationRef { environment, scope })
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

pub fn retry_native_project_migration(
    migration: &ProjectMigrationRegistry,
    admission: &RuntimeAdmissionCoordinator,
) -> Result<Vec<ProjectInfo>, AppError> {
    let _permit = begin_project_mutation(
        admission,
        MutationKind::ProjectMigration,
        EnvironmentRef::Native,
        None,
    )?;
    project_service::retry_native_project_migration(migration)
}

pub async fn remove_environment_project(
    environment: EnvironmentRef,
    project_id: String,
    registry: &WslRuntime,
    migration: &ProjectMigrationRegistry,
    admission: &RuntimeAdmissionCoordinator,
    libraries: &dyn ProjectLibraryDetachment,
) -> Result<Vec<ProjectInfo>, AppError> {
    let permit = begin_project_mutation(
        admission,
        MutationKind::RemoveProject,
        environment.clone(),
        Some(&project_id),
    )?;
    let context = SkillLocationRef {
        environment: environment.clone(),
        scope: SkillLocation::Project {
            project_id: project_id.clone(),
        },
    };
    remove_project_after_detaching_libraries(context, libraries, permit.cancellation(), || {
        project_service::remove_environment_project(environment, project_id, registry, migration)
    })
    .await
}

/// 解除该 Project 的 Skill 库应用关系，确认解除完成后才移除 Project 记录。
async fn remove_project_after_detaching_libraries<Remove, RemoveFuture>(
    context: SkillLocationRef,
    libraries: &dyn ProjectLibraryDetachment,
    cancellation: CancellationSignal,
    remove: Remove,
) -> Result<Vec<ProjectInfo>, AppError>
where
    Remove: FnOnce() -> RemoveFuture,
    RemoveFuture: Future<Output = Result<Vec<ProjectInfo>, AppError>>,
{
    let mut application = libraries.read(context.clone()).await?;
    if application.pending {
        let response = libraries
            .retry_pending(context.clone(), cancellation.clone())
            .await?;
        ensure_library_application_complete(&response)?;
        application = response.application;
    }
    if !application.ordered_libraries.is_empty() || !application.selected_agent_ids.is_empty() {
        let draft = LibraryApplicationDraft {
            context: context.clone(),
            ordered_library_ids: Vec::new(),
            selected_agent_ids: Vec::new(),
        };
        let preview = libraries.preview(draft.clone()).await?;
        let response = libraries
            .apply(
                ApplyLibraryApplicationRequest {
                    draft,
                    expected_token: preview.token,
                },
                cancellation,
            )
            .await?;
        ensure_library_application_complete(&response)?;
    }
    libraries.forget_project(context).await?;
    remove().await
}

fn ensure_library_application_complete(
    response: &LibraryApplicationResponse,
) -> Result<(), AppError> {
    let completed = response.units.iter().all(|unit| {
        matches!(
            unit.status,
            MutationUnitStatus::Succeeded | MutationUnitStatus::Skipped
        )
    });
    if completed && !response.application.pending {
        Ok(())
    } else {
        Err(AppError::ExecutionFailed {
            message: "Skill Library links must be resolved before removing the Project".to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::library_application::{
        LibraryApplicationFuture, LibraryApplicationPreview, LibraryApplicationState,
        LibraryApplicationSummary,
    };
    use crate::application::mutation::plan::PreviewToken;
    use crate::application::mutation::result::MutationUnitResult;
    use crate::application::skill_libraries::{LibraryId, SkillLibrarySummary};
    use crate::core::agent_definition::AgentId;
    use crate::environment::runtime::ContextSnapshotRevision;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Mutex;

    struct FakeLibraries {
        calls: Mutex<Vec<String>>,
        drafts: Mutex<Vec<LibraryApplicationDraft>>,
        read_summary: LibraryApplicationSummary,
        retry_response: Option<LibraryApplicationResponse>,
        apply_response: Option<LibraryApplicationResponse>,
        forget_fails: bool,
    }

    impl FakeLibraries {
        fn new(read_summary: LibraryApplicationSummary) -> Self {
            Self {
                calls: Mutex::new(Vec::new()),
                drafts: Mutex::new(Vec::new()),
                read_summary,
                retry_response: None,
                apply_response: None,
                forget_fails: false,
            }
        }

        fn with_retry(mut self, response: LibraryApplicationResponse) -> Self {
            self.retry_response = Some(response);
            self
        }

        fn with_apply(mut self, response: LibraryApplicationResponse) -> Self {
            self.apply_response = Some(response);
            self
        }

        fn failing_forget(mut self) -> Self {
            self.forget_fails = true;
            self
        }

        fn record(&self, call: &str) {
            self.calls.lock().expect("calls").push(call.to_string());
        }

        fn calls(&self) -> Vec<String> {
            self.calls.lock().expect("calls").clone()
        }

        fn drafts(&self) -> Vec<LibraryApplicationDraft> {
            self.drafts.lock().expect("drafts").clone()
        }
    }

    impl ProjectLibraryDetachment for FakeLibraries {
        fn read<'a>(
            &'a self,
            _context: SkillLocationRef,
        ) -> LibraryApplicationFuture<'a, Result<LibraryApplicationSummary, AppError>> {
            self.record("read");
            let summary = self.read_summary.clone();
            Box::pin(async move { Ok(summary) })
        }

        fn retry_pending<'a>(
            &'a self,
            _context: SkillLocationRef,
            _cancellation: CancellationSignal,
        ) -> LibraryApplicationFuture<'a, Result<LibraryApplicationResponse, AppError>> {
            self.record("retry_pending");
            let response = self.retry_response.clone();
            Box::pin(async move { response.ok_or(AppError::MutationBusy) })
        }

        fn preview<'a>(
            &'a self,
            draft: LibraryApplicationDraft,
        ) -> LibraryApplicationFuture<'a, Result<LibraryApplicationPreview, AppError>> {
            self.record("preview");
            self.drafts.lock().expect("drafts").push(draft);
            Box::pin(async move { Ok(preview_result()) })
        }

        fn apply<'a>(
            &'a self,
            request: ApplyLibraryApplicationRequest,
            _cancellation: CancellationSignal,
        ) -> LibraryApplicationFuture<'a, Result<LibraryApplicationResponse, AppError>> {
            self.record("apply");
            self.drafts.lock().expect("drafts").push(request.draft);
            let response = self.apply_response.clone();
            Box::pin(async move { response.ok_or(AppError::MutationBusy) })
        }

        fn forget_project<'a>(
            &'a self,
            _context: SkillLocationRef,
        ) -> LibraryApplicationFuture<'a, Result<(), AppError>> {
            self.record("forget_project");
            let fails = self.forget_fails;
            Box::pin(async move {
                if fails {
                    Err(AppError::MutationBusy)
                } else {
                    Ok(())
                }
            })
        }
    }

    fn project_context() -> SkillLocationRef {
        SkillLocationRef {
            environment: EnvironmentRef::Native,
            scope: SkillLocation::Project {
                project_id: "project-1".to_string(),
            },
        }
    }

    fn summary(pending: bool, has_library: bool, has_agent: bool) -> LibraryApplicationSummary {
        LibraryApplicationSummary {
            ordered_libraries: has_library
                .then(|| SkillLibrarySummary {
                    id: LibraryId::parse("backend"),
                    name: "Backend".to_string(),
                    skill_count: 1,
                })
                .into_iter()
                .collect(),
            selected_agent_ids: has_agent
                .then(|| AgentId::parse("codex").expect("valid Agent ID"))
                .into_iter()
                .collect(),
            pending,
        }
    }

    fn unit(status: MutationUnitStatus) -> MutationUnitResult {
        MutationUnitResult {
            unit_id: "unit-1".to_string(),
            skill_name: "demo".to_string(),
            source: None,
            target: project_context(),
            status,
            retryable: false,
            lock_committed: false,
            actual_mode: None,
            fallback_reason: None,
            agent_targets: Vec::new(),
            warnings: Vec::new(),
            error: None,
            recovery: None,
        }
    }

    fn response(
        status: MutationUnitStatus,
        application: LibraryApplicationSummary,
    ) -> LibraryApplicationResponse {
        LibraryApplicationResponse {
            application,
            units: vec![unit(status)],
        }
    }

    fn preview_result() -> LibraryApplicationPreview {
        LibraryApplicationPreview {
            token: PreviewToken {
                generation: "generation".to_string(),
                registry_revision: "registry".to_string(),
                environment_revision: "environment".to_string(),
                context_revision: ContextSnapshotRevision::parse("context-v1-remove")
                    .expect("valid context revision"),
            },
            current: LibraryApplicationState::default(),
            target: LibraryApplicationState::default(),
            added_skill_names: Vec::new(),
            removed_skill_names: Vec::new(),
            switched_skill_names: Vec::new(),
            changed_directory_skill_names: Vec::new(),
            overridden_by_direct_skill_names: Vec::new(),
        }
    }

    async fn detach(
        libraries: &FakeLibraries,
        removed: &AtomicBool,
    ) -> Result<Vec<ProjectInfo>, AppError> {
        remove_project_after_detaching_libraries(
            project_context(),
            libraries,
            CancellationSignal::default(),
            || async {
                removed.store(true, Ordering::SeqCst);
                Ok(Vec::new())
            },
        )
        .await
    }

    #[tokio::test]
    async fn removes_project_without_touching_libraries_when_no_application() {
        let libraries = FakeLibraries::new(summary(false, false, false));
        let removed = AtomicBool::new(false);

        detach(&libraries, &removed).await.expect("remove project");

        assert_eq!(libraries.calls(), vec!["read", "forget_project"]);
        assert!(removed.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn applies_an_empty_draft_when_libraries_are_applied() {
        let libraries = FakeLibraries::new(summary(false, true, false)).with_apply(response(
            MutationUnitStatus::Succeeded,
            summary(false, false, false),
        ));
        let removed = AtomicBool::new(false);

        detach(&libraries, &removed).await.expect("remove project");

        assert_eq!(
            libraries.calls(),
            vec!["read", "preview", "apply", "forget_project"]
        );
        assert!(removed.load(Ordering::SeqCst));
        for draft in libraries.drafts() {
            assert!(draft.ordered_library_ids.is_empty());
            assert!(draft.selected_agent_ids.is_empty());
            assert_eq!(draft.context, project_context());
        }
    }

    #[tokio::test]
    async fn applies_an_empty_draft_when_only_agents_are_selected() {
        let libraries = FakeLibraries::new(summary(false, false, true)).with_apply(response(
            MutationUnitStatus::Succeeded,
            summary(false, false, false),
        ));
        let removed = AtomicBool::new(false);

        detach(&libraries, &removed).await.expect("remove project");

        assert_eq!(
            libraries.calls(),
            vec!["read", "preview", "apply", "forget_project"]
        );
        assert!(removed.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn retried_pending_result_decides_whether_an_empty_draft_is_applied() {
        let libraries = FakeLibraries::new(summary(true, true, false)).with_retry(response(
            MutationUnitStatus::Succeeded,
            summary(false, false, false),
        ));
        let removed = AtomicBool::new(false);

        detach(&libraries, &removed).await.expect("remove project");

        assert_eq!(
            libraries.calls(),
            vec!["read", "retry_pending", "forget_project"]
        );
        assert!(removed.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn failed_pending_retry_blocks_removal() {
        let libraries = FakeLibraries::new(summary(true, true, false)).with_retry(response(
            MutationUnitStatus::Failed,
            summary(false, true, false),
        ));
        let removed = AtomicBool::new(false);

        assert!(matches!(
            detach(&libraries, &removed).await,
            Err(AppError::ExecutionFailed { .. })
        ));
        assert_eq!(libraries.calls(), vec!["read", "retry_pending"]);
        assert!(!removed.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn failed_unit_blocks_removal() {
        let libraries = FakeLibraries::new(summary(false, true, false)).with_apply(response(
            MutationUnitStatus::Failed,
            summary(false, false, false),
        ));
        let removed = AtomicBool::new(false);

        assert!(matches!(
            detach(&libraries, &removed).await,
            Err(AppError::ExecutionFailed { .. })
        ));
        assert_eq!(libraries.calls(), vec!["read", "preview", "apply"]);
        assert!(!removed.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn still_pending_application_blocks_removal() {
        let libraries = FakeLibraries::new(summary(false, true, false)).with_apply(response(
            MutationUnitStatus::Succeeded,
            summary(true, false, false),
        ));
        let removed = AtomicBool::new(false);

        assert!(matches!(
            detach(&libraries, &removed).await,
            Err(AppError::ExecutionFailed { .. })
        ));
        assert!(!libraries.calls().contains(&"forget_project".to_string()));
        assert!(!removed.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn skipped_units_complete_the_detachment() {
        let libraries = FakeLibraries::new(summary(false, true, false)).with_apply(response(
            MutationUnitStatus::Skipped,
            summary(false, false, false),
        ));
        let removed = AtomicBool::new(false);

        detach(&libraries, &removed).await.expect("remove project");

        assert!(removed.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn forget_project_failure_blocks_removal() {
        let libraries = FakeLibraries::new(summary(false, false, false)).failing_forget();
        let removed = AtomicBool::new(false);

        assert!(detach(&libraries, &removed).await.is_err());
        assert_eq!(libraries.calls(), vec!["read", "forget_project"]);
        assert!(!removed.load(Ordering::SeqCst));
    }

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
        let crate::core::mutation::MutationTargetRef::SkillLocation {
            environment: target_environment,
            scope,
        } = active.target
        else {
            panic!("project mutation must target a Skill location");
        };
        assert_eq!(target_environment, environment);
        assert_eq!(
            scope,
            SkillLocation::Project {
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
                SkillLocationRef {
                    environment: EnvironmentRef::Native,
                    scope: SkillLocation::Global,
                },
            )
            .expect("begin install");

        assert!(matches!(
            begin_project_mutation(
                &admission,
                MutationKind::AddProject,
                EnvironmentRef::Native,
                None,
            ),
            Err(AppError::MutationBusy)
        ));
    }
}

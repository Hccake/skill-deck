use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use specta::Type;

mod state;

use crate::application::library_application::{
    ApplicationInventory, ApplicationRegistry, LibraryApplicationModule,
    LibraryApplicationResponse, LibraryApplicationScopePlan, LibraryApplicationSyncState,
    ReconciliationAttention,
};
use crate::application::mutation::executor::MutationPlanExecutor;
use crate::application::payload_session::PayloadSessionManager;
use crate::application::planning_facts::ScopePlanningSnapshotSource;
use crate::application::runtime_admission::RuntimeAdmissionCoordinator;
use crate::application::scope_skill_planning::ScopeSkillPlanner;
use crate::application::skill_libraries::{
    ExecuteAddLibrarySkillsRequest, ExecuteRetireLibrarySkillRequest, LibraryAddPreview,
    LibraryAddResponse, LibraryCatalog, LibraryId, LibraryRetirePreview, LibraryRetireResponse,
    PreviewAddLibrarySkillsRequest, PurgeRetiredLibraryMemberRequest, RemoveLibrarySkillRequest,
    SkillLibraryModule, SkillLibraryRepository,
};
use crate::core::mutation::CancellationSignal;
use crate::core::mutation::MutationKind;
use crate::environment::content_manifest::ContentManifestReader;
use crate::environment::planning::TargetFactResolver;
use crate::environment::types::{EnvironmentRef, SkillLocationRef};
use crate::error::AppError;
#[cfg(test)]
use state::MembershipChangeKind;
pub(crate) use state::{apply_membership_change, MembershipChange};

pub(crate) trait LibraryTargetFacts: TargetFactResolver + ContentManifestReader {}

impl<T> LibraryTargetFacts for T where T: TargetFactResolver + ContentManifestReader {}

pub(crate) type MembershipFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

pub(crate) trait LibraryApplicationMembership: Send + Sync {
    fn plan<'a>(
        &'a self,
        context: SkillLocationRef,
    ) -> MembershipFuture<'a, Result<LibraryApplicationScopePlan, AppError>>;

    fn plan_with_catalog<'a>(
        &'a self,
        context: SkillLocationRef,
        catalog: LibraryCatalog,
    ) -> MembershipFuture<'a, Result<LibraryApplicationScopePlan, AppError>>;

    fn record_attention<'a>(
        &'a self,
        context: SkillLocationRef,
        attention: ReconciliationAttention,
    ) -> MembershipFuture<'a, Result<(), AppError>>;

    fn resume<'a>(
        &'a self,
        context: SkillLocationRef,
        cancellation: CancellationSignal,
    ) -> MembershipFuture<'a, Result<LibraryApplicationResponse, AppError>>;
}

impl<F, T, E> LibraryApplicationMembership for LibraryApplicationModule<F, T, E>
where
    F: ScopePlanningSnapshotSource + Send + Sync,
    T: TargetFactResolver + Send + Sync,
    E: MutationPlanExecutor + Send + Sync,
{
    fn plan<'a>(
        &'a self,
        context: SkillLocationRef,
    ) -> MembershipFuture<'a, Result<LibraryApplicationScopePlan, AppError>> {
        Box::pin(async move { LibraryApplicationModule::plan_resume(self, context).await })
    }

    fn plan_with_catalog<'a>(
        &'a self,
        context: SkillLocationRef,
        catalog: LibraryCatalog,
    ) -> MembershipFuture<'a, Result<LibraryApplicationScopePlan, AppError>> {
        Box::pin(async move {
            LibraryApplicationModule::plan_resume_with_catalog(self, context, catalog).await
        })
    }

    fn record_attention<'a>(
        &'a self,
        context: SkillLocationRef,
        attention: ReconciliationAttention,
    ) -> MembershipFuture<'a, Result<(), AppError>> {
        Box::pin(async move {
            LibraryApplicationModule::record_reconciliation_attention(self, context, attention)
                .await
        })
    }

    fn resume<'a>(
        &'a self,
        context: SkillLocationRef,
        cancellation: CancellationSignal,
    ) -> MembershipFuture<'a, Result<LibraryApplicationResponse, AppError>> {
        Box::pin(async move { LibraryApplicationModule::resume(self, context, cancellation).await })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct LibraryMembershipPreview {
    pub environment: EnvironmentRef,
    pub library_id: LibraryId,
    pub scopes: Vec<SkillLocationRef>,
    pub impacts: Vec<MembershipScopeImpact>,
    pub inventory_complete: bool,
    pub inventory_token: String,
    pub token: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub enum MembershipScopeImpactKind {
    Added,
    Switched,
    Fallback,
    Removed,
    Unchanged,
    Unverified,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct MembershipSkillImpact {
    pub skill_name: String,
    pub kind: MembershipScopeImpactKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct MembershipScopeImpact {
    pub context: SkillLocationRef,
    pub skills: Vec<MembershipSkillImpact>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub enum MembershipScopeState {
    Synced,
    Pending,
    Unverified,
    RecoveryRequired,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct MembershipScopeResult {
    pub context: SkillLocationRef,
    pub state: MembershipScopeState,
    pub error: Option<AppError>,
}

#[derive(Debug)]
pub(crate) struct LibraryMembershipExecution<L> {
    pub library: Option<L>,
    pub scopes: Vec<MembershipScopeResult>,
    pub cleanup: Vec<RetiredCleanupResult>,
    pub snapshot_error: Option<AppError>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub enum RetiredCleanupState {
    Purged,
    Retained,
    Failed,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct RetiredCleanupResult {
    pub library_id: LibraryId,
    pub member_name: String,
    pub retirement_id: String,
    pub state: RetiredCleanupState,
    pub error: Option<AppError>,
}

#[derive(Debug, Clone, Default, Serialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct LibraryMembershipOutcome {
    pub scopes: Vec<MembershipScopeResult>,
    pub cleanup: Vec<RetiredCleanupResult>,
    pub snapshot_error: Option<AppError>,
}

impl<L> LibraryMembershipExecution<L> {
    pub(crate) fn into_parts(self) -> (Option<L>, LibraryMembershipOutcome) {
        (
            self.library,
            LibraryMembershipOutcome {
                scopes: self.scopes,
                cleanup: self.cleanup,
                snapshot_error: self.snapshot_error,
            },
        )
    }
}

struct RetiredMemberCollector {
    applications: Arc<dyn ApplicationRegistry>,
    libraries: Arc<dyn SkillLibraryRepository>,
}

struct MembershipReconciliationModule {
    applications: Arc<dyn ApplicationRegistry>,
    reconciler: Arc<dyn LibraryApplicationMembership>,
    collector: RetiredMemberCollector,
}

impl MembershipReconciliationModule {
    fn new(
        applications: Arc<dyn ApplicationRegistry>,
        reconciler: Arc<dyn LibraryApplicationMembership>,
        libraries: Arc<dyn SkillLibraryRepository>,
    ) -> Self {
        Self {
            collector: RetiredMemberCollector {
                applications: applications.clone(),
                libraries,
            },
            applications,
            reconciler,
        }
    }

    pub(crate) async fn preview(
        &self,
        environment: EnvironmentRef,
        library_id: LibraryId,
    ) -> Result<LibraryMembershipPreview, AppError> {
        let inventory = self.applications.enumerate(&environment).await?;
        membership_preview(environment, library_id, &inventory)
    }

    pub(crate) async fn execute<L, F>(
        &self,
        preview: LibraryMembershipPreview,
        commit: F,
        cancellation: CancellationSignal,
    ) -> Result<LibraryMembershipExecution<L>, AppError>
    where
        F: Future<Output = Result<L, AppError>>,
    {
        let current = self
            .preview(preview.environment.clone(), preview.library_id.clone())
            .await?;
        if current.inventory_token != preview.inventory_token {
            return Err(AppError::StaleContext);
        }
        let library = commit.await?;
        let mut result = self
            .reconcile_scopes(current.scopes, current.inventory_complete, cancellation)
            .await;
        match self
            .collect_retired(&preview.environment, Some(&preview.library_id))
            .await
        {
            Ok(cleanup) => result.cleanup = cleanup,
            Err(error) if result.snapshot_error.is_none() => result.snapshot_error = Some(error),
            Err(_) => {}
        }
        result.library = Some(library);
        Ok(result)
    }

    pub(crate) async fn resume(
        &self,
        environment: EnvironmentRef,
        library_id: Option<LibraryId>,
        cancellation: CancellationSignal,
    ) -> Result<LibraryMembershipExecution<()>, AppError> {
        let inventory = self.applications.enumerate(&environment).await?;
        let mut scopes = inventory
            .records
            .iter()
            .filter(|application| {
                library_id
                    .as_ref()
                    .is_none_or(|library_id| references_library(&application.record, library_id))
            })
            .map(|application| application.context.clone())
            .collect::<Vec<_>>();
        sort_scopes(&mut scopes);
        let mut result = self
            .reconcile_scopes(scopes, inventory.complete, cancellation)
            .await;
        match self
            .collect_retired(&environment, library_id.as_ref())
            .await
        {
            Ok(cleanup) => result.cleanup = cleanup,
            Err(error) if result.snapshot_error.is_none() => result.snapshot_error = Some(error),
            Err(_) => {}
        }
        Ok(result)
    }

    async fn reconcile_scopes<L>(
        &self,
        scopes: Vec<SkillLocationRef>,
        inventory_complete: bool,
        cancellation: CancellationSignal,
    ) -> LibraryMembershipExecution<L> {
        let mut snapshot_error = None;
        let mut plans = Vec::with_capacity(scopes.len());
        let mut planning_results = Vec::with_capacity(scopes.len());
        for context in &scopes {
            if cancellation.is_cancelled() {
                plans.push(None);
                planning_results.push(Some(MembershipScopeResult {
                    context: context.clone(),
                    state: MembershipScopeState::Cancelled,
                    error: Some(AppError::MutationCancelled),
                }));
                continue;
            }
            match self.reconciler.plan(context.clone()).await {
                Ok(plan) => {
                    plans.push(Some(plan));
                    planning_results.push(None);
                }
                Err(error) => {
                    plans.push(None);
                    planning_results.push(Some(MembershipScopeResult {
                        context: context.clone(),
                        state: membership_error_state(&error),
                        error: Some(error),
                    }));
                }
            }
        }
        let conflicts = conflicting_scope_plans(plans.iter().flatten());
        let mut results = Vec::with_capacity(scopes.len());
        for (index, context) in scopes.into_iter().enumerate() {
            if let Some(result) = planning_results[index].take() {
                let attention = match result.state {
                    MembershipScopeState::Unverified => ReconciliationAttention::Unverified,
                    _ => ReconciliationAttention::Pending,
                };
                if let Err(error) = self
                    .reconciler
                    .record_attention(context.clone(), attention)
                    .await
                {
                    snapshot_error.get_or_insert(error);
                }
                results.push(result);
                continue;
            }
            if conflicts.contains(&context) {
                if let Err(error) = self
                    .reconciler
                    .record_attention(context.clone(), ReconciliationAttention::Pending)
                    .await
                {
                    snapshot_error.get_or_insert(error);
                }
                results.push(MembershipScopeResult {
                    context,
                    state: MembershipScopeState::Pending,
                    error: Some(AppError::StaleTarget),
                });
                continue;
            }
            match self
                .reconciler
                .resume(context.clone(), cancellation.clone())
                .await
            {
                Ok(application) => results.push(MembershipScopeResult {
                    state: membership_scope_state(application.application.sync_state),
                    context,
                    error: None,
                }),
                Err(error) => results.push(MembershipScopeResult {
                    state: membership_error_state(&error),
                    context,
                    error: Some(error),
                }),
            }
        }
        LibraryMembershipExecution {
            library: None,
            scopes: results,
            cleanup: Vec::new(),
            snapshot_error: snapshot_error.or_else(|| {
                (!inventory_complete).then(|| AppError::ConfigurationCorrupted {
                    message: "Skill Library application inventory is incomplete".to_string(),
                })
            }),
        }
    }

    async fn collect_retired(
        &self,
        environment: &EnvironmentRef,
        library_id: Option<&LibraryId>,
    ) -> Result<Vec<RetiredCleanupResult>, AppError> {
        self.collector.collect(environment, library_id).await
    }
}

pub(crate) struct LibraryMembershipModule {
    reconciliation: MembershipReconciliationModule,
    libraries: Arc<SkillLibraryModule>,
    payloads: Arc<PayloadSessionManager>,
    targets: Arc<dyn LibraryTargetFacts>,
    admission: Arc<RuntimeAdmissionCoordinator>,
}

impl LibraryMembershipModule {
    pub(crate) fn new(
        applications: Arc<dyn ApplicationRegistry>,
        reconciler: Arc<dyn LibraryApplicationMembership>,
        repository: Arc<dyn SkillLibraryRepository>,
        admission: Arc<RuntimeAdmissionCoordinator>,
        libraries: Arc<SkillLibraryModule>,
        payloads: Arc<PayloadSessionManager>,
        targets: Arc<dyn LibraryTargetFacts>,
    ) -> Self {
        Self {
            reconciliation: MembershipReconciliationModule::new(
                applications,
                reconciler,
                repository,
            ),
            libraries,
            payloads,
            targets,
            admission,
        }
    }

    pub(crate) async fn preview_add_skills(
        &self,
        request: PreviewAddLibrarySkillsRequest,
    ) -> Result<LibraryAddPreview, AppError> {
        let preview_request = request.clone();
        let membership = self
            .reconciliation
            .preview(request.environment.clone(), request.library_id.clone())
            .await?;
        let (mut preview, projected) = self
            .libraries
            .preview_add_skills_with_catalog(
                self.payloads.as_ref(),
                self.targets.as_ref(),
                request,
                membership,
            )
            .await?;
        let membership = self
            .preview_impacts(
                preview.membership.clone(),
                projected,
                preview
                    .skills
                    .iter()
                    .map(|skill| skill.skill_name.clone())
                    .collect(),
                false,
            )
            .await?;
        preview.bind_membership(&preview_request, membership)?;
        Ok(preview)
    }

    pub(crate) async fn execute_add_skills(
        &self,
        request: ExecuteAddLibrarySkillsRequest,
        cancellation: CancellationSignal,
    ) -> Result<LibraryAddResponse, AppError> {
        let _permit = self.admission.begin_library_mutation(
            MutationKind::ManageLibraries,
            request.request.environment.clone(),
            request.request.library_id.as_str().to_string(),
        )?;
        let current = self.preview_add_skills(request.request.clone()).await?;
        if current.token != request.expected_token
            || current.membership.token != request.membership.token
        {
            return Err(AppError::StaleContext);
        }
        let preview = current.membership.clone();
        let request = ExecuteAddLibrarySkillsRequest {
            membership: current.membership,
            ..request
        };
        let commit = self.libraries.execute_add_skills(
            self.payloads.as_ref(),
            self.targets.as_ref(),
            request,
        );
        let execution = self
            .reconciliation
            .execute(preview, commit, cancellation)
            .await?;
        let (commit, mut membership) = execution.into_parts();
        let commit = commit.ok_or(AppError::StaleContext)?;
        if membership.snapshot_error.is_none() {
            membership.snapshot_error = commit.snapshot_error;
        }
        Ok(LibraryAddResponse {
            results: commit.results,
            library: commit.library,
            membership,
        })
    }

    pub(crate) async fn preview_retire_skill(
        &self,
        request: RemoveLibrarySkillRequest,
    ) -> Result<LibraryRetirePreview, AppError> {
        let membership = self
            .reconciliation
            .preview(request.environment.clone(), request.library_id.clone())
            .await?;
        let (prepared, projected) = self
            .libraries
            .prepare_retire_skill_with_catalog(self.targets.as_ref(), request, membership.clone())
            .await?;
        let skill_name = prepared.skill_name().to_string();
        let membership = self
            .preview_impacts(membership, projected, vec![skill_name], true)
            .await?;
        prepared.bind(membership)
    }

    pub(crate) async fn retire_skill(
        &self,
        request: ExecuteRetireLibrarySkillRequest,
        cancellation: CancellationSignal,
    ) -> Result<LibraryRetireResponse, AppError> {
        let _permit = self.admission.begin_library_mutation(
            MutationKind::ManageLibraries,
            request.request.environment.clone(),
            request.request.library_id.as_str().to_string(),
        )?;
        let current = self.preview_retire_skill(request.request.clone()).await?;
        if current.token != request.expected_token
            || current.membership.token != request.membership.token
        {
            return Err(AppError::StaleContext);
        }
        let preview = current.membership.clone();
        let request = ExecuteRetireLibrarySkillRequest {
            membership: current.membership,
            ..request
        };
        let commit = self.libraries.retire_skill(self.targets.as_ref(), request);
        let execution = self
            .reconciliation
            .execute(preview, commit, cancellation)
            .await?;
        let (commit, mut membership) = execution.into_parts();
        let commit = commit.ok_or(AppError::StaleContext)?;
        if membership.snapshot_error.is_none() {
            membership.snapshot_error = commit.snapshot_error;
        }
        Ok(LibraryRetireResponse {
            library: commit.library,
            membership,
        })
    }

    pub(crate) async fn resume(
        &self,
        environment: EnvironmentRef,
        library_id: Option<LibraryId>,
        cancellation: CancellationSignal,
    ) -> Result<LibraryMembershipExecution<()>, AppError> {
        let _permit = self.admission.begin_library_mutation(
            MutationKind::ManageLibraries,
            environment.clone(),
            library_id
                .as_ref()
                .map(|id| id.as_str())
                .unwrap_or("all")
                .to_string(),
        )?;
        self.reconciliation
            .resume(environment, library_id, cancellation)
            .await
    }

    async fn preview_impacts(
        &self,
        mut membership: LibraryMembershipPreview,
        catalog: LibraryCatalog,
        skill_names: Vec<String>,
        retiring: bool,
    ) -> Result<LibraryMembershipPreview, AppError> {
        let mut impacts = Vec::with_capacity(membership.scopes.len());
        let mut evidence = Vec::with_capacity(membership.scopes.len());
        let mut planned = Vec::new();
        for context in &membership.scopes {
            match self
                .reconciliation
                .reconciler
                .plan_with_catalog(context.clone(), catalog.clone())
                .await
            {
                Ok(plan) => {
                    let skills =
                        membership_skill_impacts(plan.preview.as_ref(), &skill_names, retiring);
                    let preview_token = plan
                        .preview
                        .as_ref()
                        .map(|preview| preview.token.generation.clone())
                        .unwrap_or_default();
                    evidence.push((
                        context.clone(),
                        preview_token,
                        plan.entries
                            .iter()
                            .map(|(key, action)| {
                                crate::application::mutation::plan::stable_digest(&(
                                    key,
                                    format!("{action:?}"),
                                ))
                            })
                            .collect::<Result<Vec<_>, _>>()?,
                    ));
                    impacts.push(MembershipScopeImpact {
                        context: context.clone(),
                        skills,
                    });
                    planned.push(plan);
                }
                Err(error) => {
                    evidence.push((
                        context.clone(),
                        String::new(),
                        vec![format!("unverified:{error}")],
                    ));
                    impacts.push(MembershipScopeImpact {
                        context: context.clone(),
                        skills: skill_names
                            .iter()
                            .map(|skill_name| MembershipSkillImpact {
                                skill_name: skill_name.clone(),
                                kind: MembershipScopeImpactKind::Unverified,
                            })
                            .collect(),
                    });
                }
            }
        }
        if !conflicting_scope_plans(planned.iter()).is_empty() {
            return Err(AppError::StaleTarget);
        }
        membership.token = crate::application::mutation::plan::stable_digest(&(
            "library-membership-impact-v1",
            membership.inventory_token.as_str(),
            &impacts,
            evidence,
        ))?;
        membership.impacts = impacts;
        Ok(membership)
    }
}

fn membership_skill_impacts(
    preview: Option<&crate::application::library_application::LibraryApplicationPreview>,
    skill_names: &[String],
    retiring: bool,
) -> Vec<MembershipSkillImpact> {
    skill_names
        .iter()
        .map(|skill_name| {
            let kind = match preview {
                Some(preview) if preview.added_skill_names.contains(skill_name) => {
                    MembershipScopeImpactKind::Added
                }
                Some(preview) if preview.switched_skill_names.contains(skill_name) => {
                    if retiring {
                        MembershipScopeImpactKind::Fallback
                    } else {
                        MembershipScopeImpactKind::Switched
                    }
                }
                Some(preview) if preview.removed_skill_names.contains(skill_name) => {
                    MembershipScopeImpactKind::Removed
                }
                _ => MembershipScopeImpactKind::Unchanged,
            };
            MembershipSkillImpact {
                skill_name: skill_name.clone(),
                kind,
            }
        })
        .collect()
}

fn conflicting_scope_plans<'a>(
    plans: impl IntoIterator<Item = &'a LibraryApplicationScopePlan>,
) -> Vec<SkillLocationRef> {
    let plans = plans.into_iter().collect::<Vec<_>>();
    let conflicting_targets = ScopeSkillPlanner::conflicting_mutations(
        plans
            .iter()
            .flat_map(|plan| plan.entries.iter())
            .map(|(key, action)| (key, action)),
    );
    plans
        .into_iter()
        .filter(|plan| {
            plan.entries
                .iter()
                .any(|(key, _)| conflicting_targets.contains(key))
        })
        .map(|plan| plan.context.clone())
        .collect()
}

impl RetiredMemberCollector {
    async fn collect(
        &self,
        environment: &EnvironmentRef,
        selected_library: Option<&LibraryId>,
    ) -> Result<Vec<RetiredCleanupResult>, AppError> {
        let catalog = self.libraries.load(environment).await?;
        let inventory = match self.applications.enumerate(environment).await {
            Ok(inventory) => inventory,
            Err(error) => {
                return Ok(catalog
                    .libraries
                    .iter()
                    .filter(|library| selected_library.is_none_or(|id| id == &library.id))
                    .flat_map(|library| {
                        library.retired_skills.iter().map(|retired| {
                            cleanup_failure(
                                library.id.clone(),
                                retired.member.name.clone(),
                                retired.retirement_id.as_str().to_string(),
                                error.clone(),
                            )
                        })
                    })
                    .collect())
            }
        };
        let mut results = Vec::new();
        for library in catalog
            .libraries
            .iter()
            .filter(|library| selected_library.is_none_or(|id| id == &library.id))
        {
            for retired in &library.retired_skills {
                let referenced = inventory.records.iter().any(|application| {
                    references_member(&application.record, &library.id, &retired.member.name)
                });
                if !inventory.complete || referenced {
                    results.push(RetiredCleanupResult {
                        library_id: library.id.clone(),
                        member_name: retired.member.name.clone(),
                        retirement_id: retired.retirement_id.as_str().to_string(),
                        state: RetiredCleanupState::Retained,
                        error: None,
                    });
                    continue;
                }
                let request = PurgeRetiredLibraryMemberRequest {
                    environment: environment.clone(),
                    library_id: library.id.clone(),
                    skill_name: retired.member.name.clone(),
                    retirement_id: retired.retirement_id.clone(),
                };
                match self.libraries.purge_retired(request).await {
                    Ok(()) => results.push(RetiredCleanupResult {
                        library_id: library.id.clone(),
                        member_name: retired.member.name.clone(),
                        retirement_id: retired.retirement_id.as_str().to_string(),
                        state: RetiredCleanupState::Purged,
                        error: None,
                    }),
                    Err(error) => results.push(cleanup_failure(
                        library.id.clone(),
                        retired.member.name.clone(),
                        retired.retirement_id.as_str().to_string(),
                        error,
                    )),
                }
            }
        }
        Ok(results)
    }
}

fn references_member(
    record: &crate::application::library_application::LibraryApplicationRecord,
    library_id: &LibraryId,
    member_name: &str,
) -> bool {
    record
        .checkpoint
        .members
        .iter()
        .chain(record.pending.iter().flat_map(|pending| {
            pending
                .recognized_members
                .iter()
                .chain(&pending.target_members)
        }))
        .any(|member| &member.library_id == library_id && member.member_name == member_name)
}

fn cleanup_failure(
    library_id: LibraryId,
    member_name: String,
    retirement_id: String,
    error: AppError,
) -> RetiredCleanupResult {
    RetiredCleanupResult {
        library_id,
        member_name,
        retirement_id,
        state: RetiredCleanupState::Failed,
        error: Some(error),
    }
}

fn membership_preview(
    environment: EnvironmentRef,
    library_id: LibraryId,
    inventory: &ApplicationInventory,
) -> Result<LibraryMembershipPreview, AppError> {
    let mut scopes = inventory
        .records
        .iter()
        .filter(|application| references_library(&application.record, &library_id))
        .map(|application| application.context.clone())
        .collect::<Vec<_>>();
    sort_scopes(&mut scopes);
    let mut records = inventory
        .records
        .iter()
        .map(|application| (&application.context, &application.record))
        .collect::<Vec<_>>();
    records.sort_by(|(left, _), (right, _)| format!("{left:?}").cmp(&format!("{right:?}")));
    let mut problems = inventory
        .problems
        .iter()
        .map(|problem| problem.storage_key.as_str())
        .collect::<Vec<_>>();
    problems.sort_unstable();
    let token = crate::application::mutation::plan::stable_digest(&(
        &records,
        &problems,
        inventory.complete,
    ))?;
    Ok(LibraryMembershipPreview {
        environment,
        library_id,
        scopes,
        impacts: Vec::new(),
        inventory_complete: inventory.complete,
        inventory_token: token.clone(),
        token,
    })
}

fn references_library(
    record: &crate::application::library_application::LibraryApplicationRecord,
    library_id: &LibraryId,
) -> bool {
    record.current.ordered_library_ids.contains(library_id)
        || record.pending.as_ref().is_some_and(|pending| {
            pending
                .before_application
                .ordered_library_ids
                .contains(library_id)
                || pending
                    .target_application
                    .ordered_library_ids
                    .contains(library_id)
                || pending
                    .recognized_members
                    .iter()
                    .chain(&pending.target_members)
                    .any(|member| &member.library_id == library_id)
        })
        || record
            .checkpoint
            .members
            .iter()
            .any(|member| &member.library_id == library_id)
}

fn sort_scopes(scopes: &mut [SkillLocationRef]) {
    scopes.sort_by(|left, right| format!("{left:?}").cmp(&format!("{right:?}")));
}

fn membership_error_state(error: &AppError) -> MembershipScopeState {
    match error {
        AppError::RecoveryRequired { .. } | AppError::RestoreFailed { .. } => {
            MembershipScopeState::RecoveryRequired
        }
        AppError::EnvironmentUnavailable { .. }
        | AppError::StorageUnsupported { .. }
        | AppError::PathNotFound { .. } => MembershipScopeState::Unverified,
        AppError::MutationCancelled => MembershipScopeState::Cancelled,
        _ => MembershipScopeState::Pending,
    }
}

fn membership_scope_state(state: LibraryApplicationSyncState) -> MembershipScopeState {
    match state {
        LibraryApplicationSyncState::Synced => MembershipScopeState::Synced,
        LibraryApplicationSyncState::Pending => MembershipScopeState::Pending,
        LibraryApplicationSyncState::Unverified => MembershipScopeState::Unverified,
        LibraryApplicationSyncState::RecoveryRequired => MembershipScopeState::RecoveryRequired,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    use serde_json::json;

    use super::*;
    use crate::application::library_application::{
        ApplicationInventory, ApplicationRegistry, LibraryApplicationFuture,
        LibraryApplicationRecord, LibraryApplicationResponse, LibraryApplicationScopePlan,
        LibraryApplicationSummary, VersionedApplicationRecord,
    };
    use crate::application::mutation::plan::PreparedEntryAction;
    use crate::application::skill_libraries::{
        CommitLibraryMemberRequest, LibraryCatalog, LibraryFuture, LibraryId, LibrarySkillRecord,
        PurgeRetiredLibraryMemberRequest, RetirementId, SkillLibraryRecord, SkillLibraryRepository,
        LIBRARY_SCHEMA_VERSION,
    };
    use crate::core::mutation::CancellationSignal;
    use crate::environment::runtime::{
        ExecutionBackend, PhysicalParentIdentity, PhysicalTargetKey,
    };
    use crate::environment::types::{EnvironmentRef, SkillLocation, SkillLocationRef};
    use crate::error::AppError;

    #[test]
    fn one_transition_handles_add_update_retire_and_reactivate() {
        let mut catalog = catalog();
        assert_eq!(
            apply_membership_change(
                &mut catalog,
                &LibraryId::parse("library-1"),
                "demo",
                MembershipChange::Upsert(record("demo", "v1")),
            )
            .unwrap(),
            MembershipChangeKind::Added
        );
        assert_eq!(
            apply_membership_change(
                &mut catalog,
                &LibraryId::parse("library-1"),
                "demo",
                MembershipChange::Upsert(record("demo", "v2")),
            )
            .unwrap(),
            MembershipChangeKind::Updated
        );
        assert_eq!(
            apply_membership_change(
                &mut catalog,
                &LibraryId::parse("library-1"),
                "demo",
                MembershipChange::Retire {
                    retirement_id: RetirementId::parse("retirement-1"),
                    retired_at: "2026-09-06T00:00:00Z".to_string(),
                },
            )
            .unwrap(),
            MembershipChangeKind::Retired
        );
        assert!(catalog.libraries[0].skills.is_empty());
        assert_eq!(catalog.libraries[0].retired_skills.len(), 1);
        assert_eq!(
            apply_membership_change(
                &mut catalog,
                &LibraryId::parse("library-1"),
                "demo",
                MembershipChange::Upsert(record("demo", "v3")),
            )
            .unwrap(),
            MembershipChangeKind::Reactivated
        );
        assert_eq!(catalog.libraries[0].skills[0].description, "v3");
        assert!(catalog.libraries[0].retired_skills.is_empty());
    }

    #[test]
    fn another_identity_cannot_claim_a_retired_directory() {
        let mut catalog = catalog();
        apply_membership_change(
            &mut catalog,
            &LibraryId::parse("library-1"),
            "CE:Review",
            MembershipChange::Upsert(record("CE:Review", "v1")),
        )
        .unwrap();
        apply_membership_change(
            &mut catalog,
            &LibraryId::parse("library-1"),
            "CE:Review",
            MembershipChange::Retire {
                retirement_id: RetirementId::parse("retirement-1"),
                retired_at: "2026-09-06T00:00:00Z".to_string(),
            },
        )
        .unwrap();

        assert!(matches!(
            apply_membership_change(
                &mut catalog,
                &LibraryId::parse("library-1"),
                "ce-review",
                MembershipChange::Upsert(record("ce-review", "v2")),
            ),
            Err(AppError::Validation { .. })
        ));
    }

    #[test]
    fn purge_requires_the_exact_retirement_identity() {
        let mut catalog = catalog();
        apply_membership_change(
            &mut catalog,
            &LibraryId::parse("library-1"),
            "demo",
            MembershipChange::Upsert(record("demo", "v1")),
        )
        .unwrap();
        apply_membership_change(
            &mut catalog,
            &LibraryId::parse("library-1"),
            "demo",
            MembershipChange::Retire {
                retirement_id: RetirementId::parse("retirement-1"),
                retired_at: "2026-09-06T00:00:00Z".to_string(),
            },
        )
        .unwrap();

        assert!(matches!(
            apply_membership_change(
                &mut catalog,
                &LibraryId::parse("library-1"),
                "demo",
                MembershipChange::Purge {
                    retirement_id: RetirementId::parse("stale"),
                },
            ),
            Err(AppError::StaleTarget)
        ));
        assert_eq!(catalog.libraries[0].retired_skills.len(), 1);
    }

    #[tokio::test]
    async fn execute_rejects_an_expanded_scope_before_committing_library_state() {
        let registry = Arc::new(MemoryRegistry::new(vec![application(
            SkillLocation::Global,
            "library-1",
        )]));
        let module = reconciliation_module(
            registry.clone(),
            Arc::new(SuccessfulReconciler),
            Arc::new(MemoryLibraries(Mutex::new(catalog()))),
        );
        let preview = module
            .preview(EnvironmentRef::Native, LibraryId::parse("library-1"))
            .await
            .unwrap();
        registry.records.lock().unwrap().push(application(
            SkillLocation::Project {
                project_id: "project-1".to_string(),
            },
            "library-1",
        ));
        let committed = Arc::new(AtomicBool::new(false));
        let committed_in_future = Arc::clone(&committed);

        let result = module
            .execute(
                preview,
                async move {
                    committed_in_future.store(true, Ordering::SeqCst);
                    Ok::<_, AppError>("saved")
                },
                CancellationSignal::default(),
            )
            .await;

        assert!(matches!(result, Err(AppError::StaleContext)));
        assert!(!committed.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn collector_purges_only_without_application_references() {
        let mut state = catalog();
        apply_membership_change(
            &mut state,
            &LibraryId::parse("library-1"),
            "demo",
            MembershipChange::Upsert(record("demo", "v1")),
        )
        .unwrap();
        apply_membership_change(
            &mut state,
            &LibraryId::parse("library-1"),
            "demo",
            MembershipChange::Retire {
                retirement_id: RetirementId::parse("retirement-1"),
                retired_at: "2026-09-06T00:00:00Z".to_string(),
            },
        )
        .unwrap();
        let libraries = Arc::new(MemoryLibraries(Mutex::new(state)));
        let module = reconciliation_module(
            Arc::new(MemoryRegistry::new(Vec::new())),
            Arc::new(SuccessfulReconciler),
            libraries.clone(),
        );

        let result = module
            .resume(
                EnvironmentRef::Native,
                Some(LibraryId::parse("library-1")),
                CancellationSignal::default(),
            )
            .await
            .unwrap();

        assert_eq!(result.cleanup.len(), 1);
        assert_eq!(result.cleanup[0].state, RetiredCleanupState::Purged);
        assert!(libraries.0.lock().unwrap().libraries[0]
            .retired_skills
            .is_empty());
    }

    #[tokio::test]
    async fn conflicting_physical_targets_block_all_affected_scopes_before_writes() {
        let registry = Arc::new(MemoryRegistry::new(vec![
            application(SkillLocation::Global, "library-1"),
            application(
                SkillLocation::Project {
                    project_id: "project-1".to_string(),
                },
                "library-1",
            ),
        ]));
        let reconciler = Arc::new(ConflictingReconciler::default());
        let module = reconciliation_module(
            registry,
            reconciler.clone(),
            Arc::new(MemoryLibraries(Mutex::new(catalog()))),
        );

        let result = module
            .resume(
                EnvironmentRef::Native,
                Some(LibraryId::parse("library-1")),
                CancellationSignal::default(),
            )
            .await
            .unwrap();

        assert_eq!(result.scopes.len(), 2);
        assert!(result.scopes.iter().all(|scope| {
            scope.state == MembershipScopeState::Pending
                && matches!(scope.error, Some(AppError::StaleTarget))
        }));
        assert_eq!(reconciler.resume_calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn reconciliation_preserves_the_application_recovery_state() {
        let registry = Arc::new(MemoryRegistry::new(vec![application(
            SkillLocation::Global,
            "library-1",
        )]));
        let module = reconciliation_module(
            registry,
            Arc::new(FixedStateReconciler(
                LibraryApplicationSyncState::RecoveryRequired,
            )),
            Arc::new(MemoryLibraries(Mutex::new(catalog()))),
        );

        let result = module
            .resume(
                EnvironmentRef::Native,
                Some(LibraryId::parse("library-1")),
                CancellationSignal::default(),
            )
            .await
            .unwrap();

        assert_eq!(
            result.scopes[0].state,
            MembershipScopeState::RecoveryRequired
        );
    }

    #[tokio::test]
    async fn reconciliation_reports_an_attention_persistence_failure() {
        let registry = Arc::new(MemoryRegistry::new(vec![application(
            SkillLocation::Global,
            "library-1",
        )]));
        let module = reconciliation_module(
            registry,
            Arc::new(FailingAttentionReconciler),
            Arc::new(MemoryLibraries(Mutex::new(catalog()))),
        );

        let result = module
            .resume(
                EnvironmentRef::Native,
                Some(LibraryId::parse("library-1")),
                CancellationSignal::default(),
            )
            .await
            .unwrap();

        assert_eq!(result.scopes[0].state, MembershipScopeState::Unverified);
        assert!(matches!(result.snapshot_error, Some(AppError::StaleTarget)));
    }

    struct MemoryRegistry {
        records: Mutex<Vec<VersionedApplicationRecord>>,
    }

    impl MemoryRegistry {
        fn new(records: Vec<VersionedApplicationRecord>) -> Self {
            Self {
                records: Mutex::new(records),
            }
        }
    }

    impl ApplicationRegistry for MemoryRegistry {
        fn load_application<'a>(
            &'a self,
            context: &'a SkillLocationRef,
        ) -> LibraryApplicationFuture<'a, Result<VersionedApplicationRecord, AppError>> {
            Box::pin(async move {
                self.records
                    .lock()
                    .unwrap()
                    .iter()
                    .find(|record| &record.context == context)
                    .cloned()
                    .ok_or_else(|| AppError::PathNotFound {
                        path: format!("{:?}", context.scope),
                    })
            })
        }

        fn save_application_if<'a>(
            &'a self,
            _observed: &'a VersionedApplicationRecord,
            _record: &'a LibraryApplicationRecord,
        ) -> LibraryApplicationFuture<'a, Result<VersionedApplicationRecord, AppError>> {
            Box::pin(async { Err(AppError::StaleTarget) })
        }

        fn enumerate<'a>(
            &'a self,
            _environment: &'a EnvironmentRef,
        ) -> LibraryApplicationFuture<'a, Result<ApplicationInventory, AppError>> {
            Box::pin(async move {
                Ok(ApplicationInventory {
                    records: self.records.lock().unwrap().clone(),
                    problems: Vec::new(),
                    complete: true,
                })
            })
        }
    }

    struct SuccessfulReconciler;

    impl LibraryApplicationMembership for SuccessfulReconciler {
        fn plan<'a>(
            &'a self,
            context: SkillLocationRef,
        ) -> MembershipFuture<'a, Result<LibraryApplicationScopePlan, AppError>> {
            Box::pin(async move {
                Ok(LibraryApplicationScopePlan {
                    context,
                    entries: Vec::new(),
                    preview: None,
                })
            })
        }

        fn plan_with_catalog<'a>(
            &'a self,
            context: SkillLocationRef,
            _catalog: LibraryCatalog,
        ) -> MembershipFuture<'a, Result<LibraryApplicationScopePlan, AppError>> {
            self.plan(context)
        }

        fn record_attention<'a>(
            &'a self,
            _context: SkillLocationRef,
            _attention: ReconciliationAttention,
        ) -> MembershipFuture<'a, Result<(), AppError>> {
            Box::pin(async { Ok(()) })
        }

        fn resume<'a>(
            &'a self,
            _context: SkillLocationRef,
            _cancellation: CancellationSignal,
        ) -> MembershipFuture<'a, Result<LibraryApplicationResponse, AppError>> {
            Box::pin(async {
                Ok(LibraryApplicationResponse {
                    application: LibraryApplicationSummary {
                        ordered_libraries: Vec::new(),
                        selected_agent_ids: Vec::new(),
                        pending: false,
                        sync_state: crate::application::library_application::LibraryApplicationSyncState::Synced,
                    },
                    units: Vec::new(),
                })
            })
        }
    }

    struct FixedStateReconciler(LibraryApplicationSyncState);

    struct FailingAttentionReconciler;

    impl LibraryApplicationMembership for FailingAttentionReconciler {
        fn plan<'a>(
            &'a self,
            _context: SkillLocationRef,
        ) -> MembershipFuture<'a, Result<LibraryApplicationScopePlan, AppError>> {
            Box::pin(async {
                Err(AppError::EnvironmentUnavailable {
                    environment: EnvironmentRef::Native,
                    message: "offline".to_string(),
                })
            })
        }

        fn plan_with_catalog<'a>(
            &'a self,
            context: SkillLocationRef,
            _catalog: LibraryCatalog,
        ) -> MembershipFuture<'a, Result<LibraryApplicationScopePlan, AppError>> {
            self.plan(context)
        }

        fn record_attention<'a>(
            &'a self,
            _context: SkillLocationRef,
            _attention: ReconciliationAttention,
        ) -> MembershipFuture<'a, Result<(), AppError>> {
            Box::pin(async { Err(AppError::StaleTarget) })
        }

        fn resume<'a>(
            &'a self,
            _context: SkillLocationRef,
            _cancellation: CancellationSignal,
        ) -> MembershipFuture<'a, Result<LibraryApplicationResponse, AppError>> {
            Box::pin(async { panic!("planning failure must not resume the Scope") })
        }
    }

    impl LibraryApplicationMembership for FixedStateReconciler {
        fn plan<'a>(
            &'a self,
            context: SkillLocationRef,
        ) -> MembershipFuture<'a, Result<LibraryApplicationScopePlan, AppError>> {
            Box::pin(async move {
                Ok(LibraryApplicationScopePlan {
                    context,
                    entries: Vec::new(),
                    preview: None,
                })
            })
        }

        fn plan_with_catalog<'a>(
            &'a self,
            context: SkillLocationRef,
            _catalog: LibraryCatalog,
        ) -> MembershipFuture<'a, Result<LibraryApplicationScopePlan, AppError>> {
            self.plan(context)
        }

        fn record_attention<'a>(
            &'a self,
            _context: SkillLocationRef,
            _attention: ReconciliationAttention,
        ) -> MembershipFuture<'a, Result<(), AppError>> {
            Box::pin(async { Ok(()) })
        }

        fn resume<'a>(
            &'a self,
            _context: SkillLocationRef,
            _cancellation: CancellationSignal,
        ) -> MembershipFuture<'a, Result<LibraryApplicationResponse, AppError>> {
            let state = self.0;
            Box::pin(async move {
                Ok(LibraryApplicationResponse {
                    application: LibraryApplicationSummary {
                        ordered_libraries: Vec::new(),
                        selected_agent_ids: Vec::new(),
                        pending: state != LibraryApplicationSyncState::Synced,
                        sync_state: state,
                    },
                    units: Vec::new(),
                })
            })
        }
    }

    #[derive(Default)]
    struct ConflictingReconciler {
        resume_calls: AtomicUsize,
    }

    impl LibraryApplicationMembership for ConflictingReconciler {
        fn plan<'a>(
            &'a self,
            context: SkillLocationRef,
        ) -> MembershipFuture<'a, Result<LibraryApplicationScopePlan, AppError>> {
            Box::pin(async move {
                let action = match context.scope {
                    SkillLocation::Global => PreparedEntryAction::Keep,
                    SkillLocation::Project { .. } => PreparedEntryAction::Remove,
                };
                Ok(LibraryApplicationScopePlan {
                    context,
                    entries: vec![(physical_target(), action)],
                    preview: None,
                })
            })
        }

        fn plan_with_catalog<'a>(
            &'a self,
            context: SkillLocationRef,
            _catalog: LibraryCatalog,
        ) -> MembershipFuture<'a, Result<LibraryApplicationScopePlan, AppError>> {
            self.plan(context)
        }

        fn record_attention<'a>(
            &'a self,
            _context: SkillLocationRef,
            _attention: ReconciliationAttention,
        ) -> MembershipFuture<'a, Result<(), AppError>> {
            Box::pin(async { Ok(()) })
        }

        fn resume<'a>(
            &'a self,
            _context: SkillLocationRef,
            _cancellation: CancellationSignal,
        ) -> MembershipFuture<'a, Result<LibraryApplicationResponse, AppError>> {
            self.resume_calls.fetch_add(1, Ordering::SeqCst);
            Box::pin(async { Err(AppError::StaleTarget) })
        }
    }

    fn physical_target() -> PhysicalTargetKey {
        PhysicalTargetKey {
            backend: ExecutionBackend::NativeUnix,
            physical_parent: PhysicalParentIdentity::Unix {
                device: 1,
                inode: 2,
            },
            normalized_final_child_name: "demo".to_string(),
        }
    }

    struct MemoryLibraries(Mutex<LibraryCatalog>);

    fn reconciliation_module(
        applications: Arc<MemoryRegistry>,
        reconciler: Arc<dyn LibraryApplicationMembership>,
        libraries: Arc<MemoryLibraries>,
    ) -> MembershipReconciliationModule {
        MembershipReconciliationModule::new(applications, reconciler, libraries)
    }

    impl SkillLibraryRepository for MemoryLibraries {
        fn resolve_collection<'a>(
            &'a self,
            _environment: &'a EnvironmentRef,
            _library_id: &'a LibraryId,
        ) -> LibraryFuture<'a, Result<crate::application::skill_paths::ResolvedSkillRoot, AppError>>
        {
            Box::pin(async { Err(AppError::StaleTarget) })
        }

        fn load<'a>(
            &'a self,
            _environment: &'a EnvironmentRef,
        ) -> LibraryFuture<'a, Result<LibraryCatalog, AppError>> {
            Box::pin(async move { Ok(self.0.lock().unwrap().clone()) })
        }

        fn save<'a>(
            &'a self,
            _environment: &'a EnvironmentRef,
            catalog: &'a LibraryCatalog,
        ) -> LibraryFuture<'a, Result<(), AppError>> {
            Box::pin(async move {
                *self.0.lock().unwrap() = catalog.clone();
                Ok(())
            })
        }

        fn commit_member<'a>(
            &'a self,
            _request: CommitLibraryMemberRequest,
        ) -> LibraryFuture<'a, Result<(), AppError>> {
            Box::pin(async { Err(AppError::StaleTarget) })
        }

        fn purge_retired<'a>(
            &'a self,
            request: PurgeRetiredLibraryMemberRequest,
        ) -> LibraryFuture<'a, Result<(), AppError>> {
            Box::pin(async move {
                apply_membership_change(
                    &mut self.0.lock().unwrap(),
                    &request.library_id,
                    &request.skill_name,
                    MembershipChange::Purge {
                        retirement_id: request.retirement_id,
                    },
                )?;
                Ok(())
            })
        }

        fn delete_library<'a>(
            &'a self,
            _environment: &'a EnvironmentRef,
            _library_id: &'a LibraryId,
        ) -> LibraryFuture<'a, Result<LibraryCatalog, AppError>> {
            Box::pin(async { Err(AppError::StaleTarget) })
        }

        fn read_skill_content<'a>(
            &'a self,
            _environment: &'a EnvironmentRef,
            _library_id: &'a LibraryId,
            _skill_name: &'a str,
        ) -> LibraryFuture<'a, Result<String, AppError>> {
            Box::pin(async { Err(AppError::StaleTarget) })
        }
    }

    fn application(scope: SkillLocation, library_id: &str) -> VersionedApplicationRecord {
        let mut record = LibraryApplicationRecord::empty();
        record.current.ordered_library_ids = vec![LibraryId::parse(library_id)];
        VersionedApplicationRecord::in_memory(
            SkillLocationRef {
                environment: EnvironmentRef::Native,
                scope,
            },
            record,
        )
    }

    fn catalog() -> LibraryCatalog {
        LibraryCatalog {
            schema_version: LIBRARY_SCHEMA_VERSION,
            libraries: vec![SkillLibraryRecord {
                id: LibraryId::parse("library-1"),
                name: "Library".to_string(),
                skills: Vec::new(),
                retired_skills: Vec::new(),
                extra: serde_json::Map::new(),
            }],
            extra: serde_json::Map::new(),
        }
    }

    fn record(name: &str, version: &str) -> LibrarySkillRecord {
        LibrarySkillRecord {
            name: name.to_string(),
            description: version.to_string(),
            source_record: json!({ "sourceType": "local", "source": version }),
            content_manifest_hash: format!("manifest-{version}"),
            updated_at: Some("2026-09-06T00:00:00Z".to_string()),
            extra: serde_json::Map::new(),
        }
    }
}

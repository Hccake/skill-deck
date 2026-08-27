use std::collections::{BTreeMap, BTreeSet};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::application::agent_selection::{
    build_agent_selection_catalog, AgentSelectionAgentKind, AgentSelectionCatalog,
    AgentSelectionSnapshot, DirectoryPlacementId,
};
use crate::application::installed_skill_resolver::SkillDirectoryName;
use crate::application::library_agent_placements::LibraryAgentPlacementMap;
use crate::application::library_candidates::{
    LibraryCandidateSet, LibraryCatalogMember, LibraryCatalogMemberIndex, LibraryVersionCandidate,
};
use crate::application::mutation::executor::MutationPlanExecutor;
use crate::application::mutation::plan::PreviewToken;
use crate::application::mutation::planning::{
    assemble_plan, issue_preview_token, validate_exact_preview, MutationPlanDraft,
    MutationUnitDraft, PreviewTokenDraft,
};
use crate::application::mutation::result::MutationUnitResult;
use crate::application::mutation::result::MutationUnitStatus;
use crate::application::planning_facts::{ScopePlanningSnapshot, ScopePlanningSnapshotSource};
use crate::application::scope_skill_planning::{
    DirectoryPlacementRef, DirectoryUpdate, ElectedVersion, LegacyLibraryPlacement,
    LibraryElectionState, LibrarySkillChangeRequest, ObservedVersion, ScopeSkillPlacementSet,
    ScopeSkillPlanner,
};
use crate::application::skill_libraries::{
    LibraryCatalog, LibraryId, LibraryUsageProjection, LibraryUsageState, SkillLibrarySummary,
};
use crate::core::agent_definition::AgentId;
use crate::core::agent_definition::{LegacyPathBehavior, LegacyPathScope};
use crate::core::mutation::{CancellationSignal, MutationKind};
use crate::environment::planning::ResolvedTargetFact;
#[cfg(test)]
use crate::environment::planning::TargetEntryKind;
use crate::environment::planning::TargetFactResolver;
use crate::environment::runtime::{EntryFingerprint, PhysicalTargetKey};
use crate::environment::types::{ResourceLocator, SkillLocationRef, StorageAccess};
use crate::error::AppError;

pub const LIBRARY_APPLICATION_SCHEMA_VERSION: u32 = 1;
pub type LibraryApplicationFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct LibraryApplicationState {
    pub ordered_library_ids: Vec<LibraryId>,
    pub selected_agent_ids: Vec<AgentId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingLibraryApplication {
    pub operation_id: String,
    pub before_application: LibraryApplicationState,
    pub target_application: LibraryApplicationState,
    pub preview_fingerprint: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryApplicationRecord {
    pub schema_version: u32,
    pub target: SkillLocationRef,
    pub current: LibraryApplicationState,
    pub pending_operation: Option<PendingLibraryApplication>,
}

impl LibraryApplicationRecord {
    pub fn empty(target: SkillLocationRef) -> Self {
        Self {
            schema_version: LIBRARY_APPLICATION_SCHEMA_VERSION,
            target,
            current: LibraryApplicationState::default(),
            pending_operation: None,
        }
    }
}

/// 判断某个库在这一份应用记录中的使用状态。
///
/// `None` 表示该位置既没有确认使用该库，也没有未完成操作引用它。已确认生效优先于
/// pending：同时出现时按 `Confirmed` 归类，避免同一个位置被重复计数。
pub fn library_usage_state(
    record: &LibraryApplicationRecord,
    library_id: &LibraryId,
) -> Option<LibraryUsageState> {
    if record.current.ordered_library_ids.contains(library_id) {
        return Some(LibraryUsageState::Confirmed);
    }
    let pending = record.pending_operation.as_ref().is_some_and(|pending| {
        pending
            .before_application
            .ordered_library_ids
            .contains(library_id)
            || pending
                .target_application
                .ordered_library_ids
                .contains(library_id)
    });
    pending.then_some(LibraryUsageState::PendingAdjustment)
}

/// 逐份应用记录累积各库的使用计数。
///
/// 每个 Skill 位置只观察一次，读取次数等于位置数量而不是"库数量 × 位置数量"。
#[derive(Debug, Default)]
pub struct LibraryUsageAccumulator {
    confirmed: BTreeMap<LibraryId, u32>,
    pending: BTreeMap<LibraryId, u32>,
}

impl LibraryUsageAccumulator {
    pub fn observe(&mut self, record: &LibraryApplicationRecord) {
        let mut confirmed: BTreeSet<&LibraryId> = BTreeSet::new();
        for id in &record.current.ordered_library_ids {
            confirmed.insert(id);
        }
        let mut pending: BTreeSet<&LibraryId> = BTreeSet::new();
        if let Some(operation) = record.pending_operation.as_ref() {
            let referenced = operation
                .before_application
                .ordered_library_ids
                .iter()
                .chain(operation.target_application.ordered_library_ids.iter());
            for id in referenced {
                if !confirmed.contains(id) {
                    pending.insert(id);
                }
            }
        }
        for id in confirmed {
            *self.confirmed.entry(id.clone()).or_default() += 1;
        }
        for id in pending {
            *self.pending.entry(id.clone()).or_default() += 1;
        }
    }

    pub fn finish(self) -> Vec<LibraryUsageProjection> {
        let mut ids: BTreeSet<LibraryId> = BTreeSet::new();
        ids.extend(self.confirmed.keys().cloned());
        ids.extend(self.pending.keys().cloned());
        ids.into_iter()
            .map(|library_id| LibraryUsageProjection {
                confirmed_count: self.confirmed.get(&library_id).copied().unwrap_or(0),
                pending_count: self.pending.get(&library_id).copied().unwrap_or(0),
                library_id,
            })
            .collect()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct LibraryApplicationSummary {
    pub ordered_libraries: Vec<SkillLibrarySummary>,
    pub selected_agent_ids: Vec<AgentId>,
    pub pending: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct LibraryAgentOptions {
    pub selection: AgentSelectionSnapshot,
    pub migrations: Vec<LibraryAgentMigration>,
    pub unsupported_agent_names: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct LibraryAgentMigration {
    pub agent_id: AgentId,
    pub display_name: String,
    pub from_path: String,
    pub to_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct LibraryApplicationDraft {
    pub context: SkillLocationRef,
    pub ordered_library_ids: Vec<LibraryId>,
    pub selected_agent_ids: Vec<AgentId>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct LibraryApplicationPreview {
    pub token: PreviewToken,
    pub current: LibraryApplicationState,
    pub target: LibraryApplicationState,
    pub added_skill_names: Vec<String>,
    pub removed_skill_names: Vec<String>,
    pub switched_skill_names: Vec<String>,
    pub changed_directory_skill_names: Vec<String>,
    pub overridden_by_direct_skill_names: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct ApplyLibraryApplicationRequest {
    pub draft: LibraryApplicationDraft,
    pub expected_token: PreviewToken,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct LibraryApplicationResponse {
    pub application: LibraryApplicationSummary,
    pub units: Vec<MutationUnitResult>,
}

/// Project 删除流程解除库应用关系时使用的操作集合。
///
/// 该 Interface 只覆盖解除流程实际调用的五个操作，使用例可以在不构造
/// `ScopePlanningSnapshotSource`、`TargetFactResolver` 和 `MutationPlanExecutor`
/// 的前提下被测试。它不改变 `LibraryApplicationModule` 既有方法的签名和调用方。
pub trait ProjectLibraryDetachment: Send + Sync {
    fn read<'a>(
        &'a self,
        context: SkillLocationRef,
    ) -> LibraryApplicationFuture<'a, Result<LibraryApplicationSummary, AppError>>;

    fn retry_pending<'a>(
        &'a self,
        context: SkillLocationRef,
        cancellation: CancellationSignal,
    ) -> LibraryApplicationFuture<'a, Result<LibraryApplicationResponse, AppError>>;

    fn preview<'a>(
        &'a self,
        draft: LibraryApplicationDraft,
    ) -> LibraryApplicationFuture<'a, Result<LibraryApplicationPreview, AppError>>;

    fn apply<'a>(
        &'a self,
        request: ApplyLibraryApplicationRequest,
        cancellation: CancellationSignal,
    ) -> LibraryApplicationFuture<'a, Result<LibraryApplicationResponse, AppError>>;

    fn forget_project<'a>(
        &'a self,
        context: SkillLocationRef,
    ) -> LibraryApplicationFuture<'a, Result<(), AppError>>;
}

pub trait LibraryApplicationRepository: Send + Sync {
    fn load_application<'a>(
        &'a self,
        context: &'a SkillLocationRef,
    ) -> LibraryApplicationFuture<'a, Result<LibraryApplicationRecord, AppError>>;

    fn save_application<'a>(
        &'a self,
        record: &'a LibraryApplicationRecord,
    ) -> LibraryApplicationFuture<'a, Result<(), AppError>>;

    fn library_skill_locator<'a>(
        &'a self,
        context: &'a SkillLocationRef,
        library_id: &'a LibraryId,
        skill_name: &'a str,
    ) -> LibraryApplicationFuture<'a, Result<ResourceLocator, AppError>>;

    fn load_catalog<'a>(
        &'a self,
        context: &'a SkillLocationRef,
    ) -> LibraryApplicationFuture<'a, Result<LibraryCatalog, AppError>>;

    fn remove_application<'a>(
        &'a self,
        context: &'a SkillLocationRef,
    ) -> LibraryApplicationFuture<'a, Result<(), AppError>>;
}

pub struct LibraryApplicationModule<F, T, E> {
    repository: Arc<dyn LibraryApplicationRepository>,
    facts: F,
    targets: T,
    executor: E,
}

impl<F, T, E> LibraryApplicationModule<F, T, E>
where
    F: ScopePlanningSnapshotSource,
    T: TargetFactResolver,
    E: MutationPlanExecutor,
{
    pub fn new(
        repository: Arc<dyn LibraryApplicationRepository>,
        facts: F,
        targets: T,
        executor: E,
    ) -> Self {
        Self {
            repository,
            facts,
            targets,
            executor,
        }
    }

    pub async fn read(
        &self,
        context: SkillLocationRef,
    ) -> Result<LibraryApplicationSummary, AppError> {
        let record = self.repository.load_application(&context).await?;
        let catalog = self.repository.load_catalog(&context).await?;
        summary(&record, &catalog)
    }

    pub async fn agent_options(
        &self,
        context: SkillLocationRef,
    ) -> Result<LibraryAgentOptions, AppError> {
        let facts = self.facts.snapshot(&context).await?;
        let resolved = resolve_library_agent_options(&context, &facts, &self.targets).await?;
        Ok(LibraryAgentOptions {
            selection: resolved.selection,
            migrations: resolved.migrations,
            unsupported_agent_names: resolved.unsupported_agent_names,
        })
    }

    pub async fn managed_skill_names(
        &self,
        context: SkillLocationRef,
    ) -> Result<BTreeSet<String>, AppError> {
        let record = self.repository.load_application(&context).await?;
        let catalog = self.repository.load_catalog(&context).await?;
        let member_index =
            LibraryCatalogMemberIndex::build(&catalog).map_err(library_member_index_error)?;
        let current = member_index
            .members_for(&record.current.ordered_library_ids)
            .map_err(library_member_index_error)?;
        let pending = record
            .pending_operation
            .as_ref()
            .map(|pending| {
                member_index
                    .members_for(&pending.target_application.ordered_library_ids)
                    .map_err(library_member_index_error)
            })
            .transpose()?
            .unwrap_or_default();
        let groups = merge_library_skill_groups(current, pending);
        if groups.is_empty() {
            return Ok(BTreeSet::new());
        }
        let facts = self.facts.snapshot(&context).await?;
        let destinations = groups
            .iter()
            .map(|group| {
                facts
                    .resolved_context
                    .skill_root
                    .join_child(group.directory_name.as_ref())
            })
            .collect::<Vec<_>>();
        let target_facts = self.targets.resolve(&context, &destinations, None).await?;
        if target_facts.len() != groups.len() {
            return Err(AppError::StaleTarget);
        }
        ensure_library_link_targets_supported(target_facts.iter())?;
        let mut managed = BTreeSet::new();
        for (group, fact) in groups.into_iter().zip(target_facts) {
            for member in group
                .current_members
                .iter()
                .chain(group.target_members.iter())
            {
                let locator = self
                    .repository
                    .library_skill_locator(&context, &member.library_id, &member.member_name)
                    .await?;
                if fact
                    .link_target_identity
                    .as_ref()
                    .is_some_and(|identity| identity.matches(&locator))
                {
                    managed.insert(member.member_name.clone());
                    break;
                }
            }
        }
        Ok(managed)
    }

    pub async fn preview(
        &self,
        draft: LibraryApplicationDraft,
    ) -> Result<LibraryApplicationPreview, AppError> {
        Ok(self.build(&draft, false, false).await?.preview)
    }

    pub async fn apply(
        &self,
        request: ApplyLibraryApplicationRequest,
        cancellation: CancellationSignal,
    ) -> Result<LibraryApplicationResponse, AppError> {
        let built = self.build(&request.draft, true, false).await?;
        validate_exact_preview(&request.expected_token, &built.preview.token)?;
        let mut pending_record = built.record.clone();
        pending_record.pending_operation = Some(PendingLibraryApplication {
            operation_id: built.plan.operation_id.clone(),
            before_application: built.record.current.clone(),
            target_application: built.preview.target.clone(),
            preview_fingerprint: built.preview.token.generation.clone(),
        });
        self.repository.save_application(&pending_record).await?;
        self.execute(built, pending_record, cancellation).await
    }

    pub async fn retry_pending(
        &self,
        context: SkillLocationRef,
        cancellation: CancellationSignal,
    ) -> Result<LibraryApplicationResponse, AppError> {
        let record = self.repository.load_application(&context).await?;
        let pending = record
            .pending_operation
            .clone()
            .ok_or_else(|| AppError::Validation {
                field: Some("context".to_string()),
                message: "the Scope has no pending Skill Library operation".to_string(),
            })?;
        let draft = LibraryApplicationDraft {
            context,
            ordered_library_ids: pending.target_application.ordered_library_ids.clone(),
            selected_agent_ids: pending.target_application.selected_agent_ids.clone(),
        };
        let mut built = self.build(&draft, true, true).await?;
        built.plan.operation_id = pending.operation_id;
        self.execute(built, record, cancellation).await
    }

    pub async fn forget_project(&self, context: SkillLocationRef) -> Result<(), AppError> {
        if !matches!(
            context.scope,
            crate::environment::types::SkillLocation::Project { .. }
        ) {
            return Err(AppError::Validation {
                field: Some("context".to_string()),
                message: "only Project Skill Library applications can be forgotten".to_string(),
            });
        }
        let record = self.repository.load_application(&context).await?;
        if record.pending_operation.is_some()
            || !record.current.ordered_library_ids.is_empty()
            || !record.current.selected_agent_ids.is_empty()
        {
            return Err(AppError::MutationBusy);
        }
        self.repository.remove_application(&context).await
    }

    async fn execute(
        &self,
        built: BuiltLibraryApplication,
        pending_record: LibraryApplicationRecord,
        cancellation: CancellationSignal,
    ) -> Result<LibraryApplicationResponse, AppError> {
        let context = built.record.target.clone();
        let expected_unit_count = built.plan.units.len();
        let units = self.executor.execute(built.plan, cancellation).await;
        let completed = library_execution_completed(expected_unit_count, &units);
        let final_record = if completed {
            LibraryApplicationRecord {
                current: built.preview.target,
                pending_operation: None,
                ..pending_record
            }
        } else {
            pending_record
        };
        self.repository.save_application(&final_record).await?;
        let catalog = self.repository.load_catalog(&context).await?;
        Ok(LibraryApplicationResponse {
            application: summary(&final_record, &catalog)?,
            units,
        })
    }

    async fn build(
        &self,
        draft: &LibraryApplicationDraft,
        include_plan: bool,
        allow_pending: bool,
    ) -> Result<BuiltLibraryApplication, AppError> {
        let record = self.repository.load_application(&draft.context).await?;
        if record.pending_operation.is_some() && !allow_pending {
            return Err(AppError::MutationBusy);
        }
        let catalog = self.repository.load_catalog(&draft.context).await?;
        let member_index =
            LibraryCatalogMemberIndex::build(&catalog).map_err(library_member_index_error)?;
        let facts = self.facts.snapshot(&draft.context).await?;
        let agent_options =
            resolve_library_agent_options(&draft.context, &facts, &self.targets).await?;
        let ordered_library_ids = validated_library_ids(&catalog, &draft.ordered_library_ids)?;
        if ordered_library_ids.is_empty() && !draft.selected_agent_ids.is_empty() {
            return Err(AppError::Validation {
                field: Some("selectedAgentIds".to_string()),
                message: "Agent targets require at least one applied Skill Library".to_string(),
            });
        }
        let target = LibraryApplicationState {
            ordered_library_ids,
            selected_agent_ids: validated_agent_ids(
                &agent_options.placement_map,
                &draft.selected_agent_ids,
            )?,
        };
        let current_agent_ids = validated_agent_ids(
            &agent_options.placement_map,
            &record.current.selected_agent_ids,
        )?;
        if let Some(pending) = &record.pending_operation {
            if pending.target_application != target {
                return Err(AppError::StaleContext);
            }
        }
        let groups = merge_library_skill_groups(
            member_index
                .members_for(&record.current.ordered_library_ids)
                .map_err(library_member_index_error)?,
            member_index
                .members_for(&target.ordered_library_ids)
                .map_err(library_member_index_error)?,
        );
        let current_selected = current_agent_ids.iter().collect::<BTreeSet<_>>();
        let mut units = Vec::new();
        let mut direct_skill_names = BTreeSet::new();
        let mut directory_change_skill_names = BTreeSet::new();
        let mut added_skill_names = BTreeSet::new();
        let mut removed_skill_names = BTreeSet::new();
        let mut switched_skill_names = BTreeSet::new();
        let mut observed_targets = Vec::new();
        for group in groups {
            let current_candidates = load_version_candidates(
                self.repository.as_ref(),
                &draft.context,
                &group.current_members,
            )
            .await?;
            let target_candidates = load_version_candidates(
                self.repository.as_ref(),
                &draft.context,
                &group.target_members,
            )
            .await?;
            let current_candidate_set = LibraryCandidateSet::for_skill(
                &draft.context.environment,
                &group.directory_name,
                current_candidates.clone(),
                current_candidates.clone(),
            )
            .map_err(|_| AppError::StaleContext)?;
            let target_candidate_set = LibraryCandidateSet::for_skill(
                &draft.context.environment,
                &group.directory_name,
                target_candidates.clone(),
                target_candidates.clone(),
            )
            .map_err(|_| AppError::StaleContext)?;
            let mut logical_targets = vec![LogicalLibraryTarget {
                placement: DirectoryPlacementRef::Catalog(DirectoryPlacementId::Standard),
                destination: facts
                    .resolved_context
                    .skill_root
                    .join_child(group.directory_name.as_ref()),
                reader_agent_ids: Vec::new(),
                library_link_target: true,
            }];
            let eligible_option_ids = agent_options
                .options
                .iter()
                .map(|option| &option.option_id)
                .collect::<BTreeSet<_>>();
            logical_targets.extend(agent_options.catalog.options().map(|resolved| {
                let option_id = resolved.public.id.clone();
                LogicalLibraryTarget {
                    placement: DirectoryPlacementRef::Catalog(DirectoryPlacementId::Option(
                        option_id.clone(),
                    )),
                    destination: resolved
                        .placement
                        .root
                        .join_child(group.directory_name.as_ref()),
                    reader_agent_ids: Vec::new(),
                    library_link_target: eligible_option_ids.contains(&option_id),
                }
            }));
            logical_targets.extend(
                agent_options
                    .legacy_options
                    .iter()
                    .filter(|option| {
                        option
                            .agent_ids
                            .iter()
                            .all(|id| current_selected.contains(id))
                    })
                    .map(|option| LogicalLibraryTarget {
                        placement: DirectoryPlacementRef::Legacy,
                        destination: option.root.join_child(group.directory_name.as_ref()),
                        reader_agent_ids: option.agent_ids.clone(),
                        library_link_target: true,
                    }),
            );
            let destinations = logical_targets
                .iter()
                .map(|logical| logical.destination.clone())
                .collect::<Vec<_>>();
            let target_facts = self
                .targets
                .resolve(&draft.context, &destinations, None)
                .await?;
            if target_facts.len() != destinations.len() {
                return Err(AppError::StaleTarget);
            }
            ensure_library_link_targets_supported(
                logical_targets
                    .iter()
                    .zip(&target_facts)
                    .filter_map(|(logical, fact)| logical.library_link_target.then_some(fact)),
            )?;
            let mut resolved = BTreeMap::new();
            let mut legacy = Vec::new();
            for (logical, fact) in logical_targets.into_iter().zip(target_facts) {
                observed_targets.push((fact.key.clone(), fact.fingerprint.clone()));
                match logical.placement {
                    DirectoryPlacementRef::Catalog(id) => {
                        resolved.insert(id, fact);
                    }
                    DirectoryPlacementRef::Legacy => legacy.push(LegacyLibraryPlacement {
                        fact,
                        reader_agent_ids: logical.reader_agent_ids,
                    }),
                }
            }
            let scope_plan = ScopeSkillPlanner::plan_library_change(LibrarySkillChangeRequest {
                skill: group.directory_name.clone(),
                catalog: &agent_options.catalog,
                placements: ScopeSkillPlacementSet::new(draft.context.clone(), resolved),
                before: LibraryElectionState {
                    candidates: &current_candidate_set,
                    selected_agent_ids: &current_agent_ids,
                },
                after: LibraryElectionState {
                    candidates: &target_candidate_set,
                    selected_agent_ids: &target.selected_agent_ids,
                },
                legacy,
            })
            .map_err(|error| error.into_app_error())?;
            let primary = scope_plan
                .directories()
                .iter()
                .find(|directory| {
                    directory
                        .placements()
                        .contains(&DirectoryPlacementRef::Catalog(
                            DirectoryPlacementId::Standard,
                        ))
                })
                .expect("validated plan contains Standard placement");
            let display_name = group
                .target_members
                .first()
                .or_else(|| group.current_members.first())
                .map(|member| member.member_name.clone())
                .unwrap_or_else(|| group.directory_name.as_ref().to_string());
            let overridden = matches!(primary.elected(), ElectedVersion::Direct(_));
            let library_directory_changed = scope_plan.directories().iter().any(|directory| {
                directory.update() != DirectoryUpdate::Unchanged
                    && (matches!(directory.observed(), ObservedVersion::Library(_))
                        || matches!(directory.elected(), ElectedVersion::Library(_)))
            });
            if overridden {
                direct_skill_names.insert(display_name.clone());
            }
            if library_directory_changed {
                directory_change_skill_names.insert(display_name.clone());
            }
            let visible = !overridden || library_directory_changed;
            match (group.current_members.first(), group.target_members.first()) {
                (None, Some(target_member)) if visible => {
                    added_skill_names.insert(target_member.member_name.clone());
                }
                (Some(current_member), None) if visible => {
                    removed_skill_names.insert(current_member.member_name.clone());
                }
                (Some(current_member), Some(target_member))
                    if visible && current_member != target_member =>
                {
                    switched_skill_names.insert(target_member.member_name.clone());
                }
                _ => {}
            }
            let state_changed = record.current != target;
            let has_write = scope_plan.directories().iter().any(|directory| {
                directory.action() != &crate::application::mutation::plan::PreparedEntryAction::Keep
            });
            if !state_changed && !has_write {
                continue;
            }
            units.push(MutationUnitDraft {
                id: format!("library:{}", group.directory_name.as_ref()),
                skill_name: display_name,
                source: None,
                target: draft.context.clone(),
                expected_revisions: facts.revisions.clone(),
                entries: scope_plan.compile_entries(),
                lock_mutation: None,
            });
        }
        let observed_state_digest =
            library_application_observed_digest(&record, &target, &catalog, &observed_targets)?;
        let token = issue_preview_token(PreviewTokenDraft {
            kind: MutationKind::ManageLibraries,
            request: draft,
            revisions: facts.revisions.clone(),
            observed_state_digest,
            planner_contract_version: 3,
        })?;
        let plan = assemble_plan(MutationPlanDraft {
            kind: MutationKind::ManageLibraries,
            payloads: BTreeMap::new(),
            units: if include_plan { units } else { Vec::new() },
        });
        Ok(BuiltLibraryApplication {
            preview: LibraryApplicationPreview {
                token,
                current: record.current.clone(),
                target,
                added_skill_names: added_skill_names.into_iter().collect(),
                removed_skill_names: removed_skill_names.into_iter().collect(),
                switched_skill_names: switched_skill_names.into_iter().collect(),
                changed_directory_skill_names: directory_change_skill_names.into_iter().collect(),
                overridden_by_direct_skill_names: direct_skill_names.into_iter().collect(),
            },
            record,
            plan,
        })
    }
}

fn library_execution_completed(expected: usize, units: &[MutationUnitResult]) -> bool {
    units.len() == expected
        && units.iter().all(|unit| {
            matches!(
                unit.status,
                MutationUnitStatus::Succeeded | MutationUnitStatus::Skipped
            )
        })
}

fn library_application_observed_digest(
    record: &LibraryApplicationRecord,
    target: &LibraryApplicationState,
    catalog: &LibraryCatalog,
    observed_targets: &[(PhysicalTargetKey, EntryFingerprint)],
) -> Result<String, AppError> {
    crate::application::mutation::plan::stable_digest(&(record, target, catalog, observed_targets))
}

impl<F, T, E> ProjectLibraryDetachment for LibraryApplicationModule<F, T, E>
where
    F: ScopePlanningSnapshotSource,
    T: TargetFactResolver,
    E: MutationPlanExecutor,
{
    fn read<'a>(
        &'a self,
        context: SkillLocationRef,
    ) -> LibraryApplicationFuture<'a, Result<LibraryApplicationSummary, AppError>> {
        Box::pin(async move { LibraryApplicationModule::read(self, context).await })
    }

    fn retry_pending<'a>(
        &'a self,
        context: SkillLocationRef,
        cancellation: CancellationSignal,
    ) -> LibraryApplicationFuture<'a, Result<LibraryApplicationResponse, AppError>> {
        Box::pin(async move {
            LibraryApplicationModule::retry_pending(self, context, cancellation).await
        })
    }

    fn preview<'a>(
        &'a self,
        draft: LibraryApplicationDraft,
    ) -> LibraryApplicationFuture<'a, Result<LibraryApplicationPreview, AppError>> {
        Box::pin(async move { LibraryApplicationModule::preview(self, draft).await })
    }

    fn apply<'a>(
        &'a self,
        request: ApplyLibraryApplicationRequest,
        cancellation: CancellationSignal,
    ) -> LibraryApplicationFuture<'a, Result<LibraryApplicationResponse, AppError>> {
        Box::pin(async move { LibraryApplicationModule::apply(self, request, cancellation).await })
    }

    fn forget_project<'a>(
        &'a self,
        context: SkillLocationRef,
    ) -> LibraryApplicationFuture<'a, Result<(), AppError>> {
        Box::pin(async move { LibraryApplicationModule::forget_project(self, context).await })
    }
}

struct ResolvedLibraryAgentOptions {
    catalog: AgentSelectionCatalog,
    selection: AgentSelectionSnapshot,
    placement_map: LibraryAgentPlacementMap,
    options: Vec<LibraryDirectoryEligibility>,
    legacy_options: Vec<LegacyLibraryDirectory>,
    migrations: Vec<LibraryAgentMigration>,
    unsupported_agent_names: Vec<String>,
}

struct LogicalLibraryTarget {
    placement: DirectoryPlacementRef,
    destination: ResourceLocator,
    reader_agent_ids: Vec<AgentId>,
    library_link_target: bool,
}

struct LibrarySkillGroup {
    directory_name: SkillDirectoryName,
    current_members: Vec<LibraryCatalogMember>,
    target_members: Vec<LibraryCatalogMember>,
}

#[derive(Clone)]
struct LibraryDirectoryEligibility {
    option_id: crate::application::agent_selection::AgentInstallOptionId,
    agent_ids: Vec<AgentId>,
}

#[derive(Clone)]
struct LegacyLibraryDirectory {
    root: ResourceLocator,
    agent_ids: Vec<AgentId>,
}

async fn resolve_library_agent_options<T: TargetFactResolver>(
    context: &SkillLocationRef,
    facts: &ScopePlanningSnapshot,
    targets: &T,
) -> Result<ResolvedLibraryAgentOptions, AppError> {
    let catalog = build_agent_selection_catalog(
        context,
        &facts.agent_runtime,
        &facts.eve_targets,
        &facts.resolved_context.skill_root,
        targets,
    )
    .await?;
    let placement_map = LibraryAgentPlacementMap::from_catalog(&catalog);
    let selection = placement_map.selection_snapshot().clone();
    let private_agent_ids = selection
        .agents
        .iter()
        .map(|agent| agent.id.clone())
        .collect::<BTreeSet<_>>();
    let mut options = placement_map
        .placements()
        .filter_map(|(placement_id, placement)| match placement_id {
            DirectoryPlacementId::Option(option_id) => Some(LibraryDirectoryEligibility {
                option_id: option_id.clone(),
                agent_ids: placement.selection_agent_ids().to_vec(),
            }),
            DirectoryPlacementId::Standard => None,
        })
        .collect::<Vec<_>>();
    options.sort_by(|left, right| {
        let left = catalog
            .option(&left.option_id)
            .expect("eligible option belongs to catalog");
        let right = catalog
            .option(&right.option_id)
            .expect("eligible option belongs to catalog");
        left.public
            .display_name
            .cmp(&right.public.display_name)
            .then_with(|| {
                left.placement
                    .root
                    .native_path
                    .cmp(&right.placement.root.native_path)
            })
    });
    let mut unsupported_agent_names = selection
        .agents
        .iter()
        .filter(|agent| agent.kind == AgentSelectionAgentKind::Grouped)
        .map(|agent| agent.display_name.clone())
        .collect::<Vec<_>>();
    unsupported_agent_names.sort();
    unsupported_agent_names.dedup();
    let legacy_scope = if matches!(
        context.scope,
        crate::environment::types::SkillLocation::Global
    ) {
        LegacyPathScope::Global
    } else {
        LegacyPathScope::Project
    };
    let mut legacy_candidates = Vec::new();
    let mut migrations = Vec::new();
    for agent in &selection.agents {
        if !private_agent_ids.contains(&agent.id) {
            continue;
        }
        let Some(resolved) = facts.agent_runtime.agents.get(&agent.id) else {
            continue;
        };
        let resolved_scope = if legacy_scope == LegacyPathScope::Global {
            &resolved.global
        } else {
            &resolved.project
        };
        for (legacy, presence) in resolved
            .definition
            .legacy_paths
            .iter()
            .filter(|legacy| legacy.scope == legacy_scope)
            .zip(&resolved_scope.legacy_paths)
        {
            if legacy.behavior != LegacyPathBehavior::OfferMigration {
                continue;
            }
            if let Some(path) = &presence.path {
                let root = ResourceLocator {
                    environment: context.environment.clone(),
                    native_path: path.clone(),
                };
                legacy_candidates.push((root, agent.id.clone(), agent.display_name.clone()));
            }
        }
    }
    let legacy_destinations = legacy_candidates
        .iter()
        .map(|(root, _, _)| root.clone())
        .collect::<Vec<_>>();
    let legacy_facts = if legacy_destinations.is_empty() {
        Vec::new()
    } else {
        targets.resolve(context, &legacy_destinations, None).await?
    };
    if legacy_facts.len() != legacy_candidates.len() {
        return Err(AppError::StaleTarget);
    }
    let mut legacy_options_by_key = BTreeMap::<PhysicalTargetKey, LegacyLibraryDirectory>::new();
    for ((root, agent_id, display_name), fact) in legacy_candidates.into_iter().zip(legacy_facts) {
        if fact.storage_access != StorageAccess::Native {
            continue;
        }
        if let Some(current) = options
            .iter()
            .find(|option| option.agent_ids.contains(&agent_id))
        {
            let current = catalog
                .option(&current.option_id)
                .expect("eligible option belongs to catalog");
            migrations.push(LibraryAgentMigration {
                agent_id: agent_id.clone(),
                display_name: display_name.clone(),
                from_path: root.native_path.clone(),
                to_path: current.placement.root.native_path.clone(),
            });
        }
        let option = legacy_options_by_key
            .entry(fact.key.clone())
            .or_insert_with(|| LegacyLibraryDirectory {
                root,
                agent_ids: Vec::new(),
            });
        option.agent_ids.push(agent_id);
        option.agent_ids.sort();
        option.agent_ids.dedup();
    }
    Ok(ResolvedLibraryAgentOptions {
        catalog,
        selection,
        placement_map,
        options,
        legacy_options: legacy_options_by_key.into_values().collect(),
        migrations,
        unsupported_agent_names,
    })
}

fn validated_agent_ids(
    placements: &LibraryAgentPlacementMap,
    requested: &[AgentId],
) -> Result<Vec<AgentId>, AppError> {
    placements
        .placements_for(requested)
        .map_err(|error| match error {
            crate::application::library_agent_placements::LibraryAgentPlacementError::UnknownAgent(
                agent,
            ) => AppError::InvalidAgent {
                agent: agent.as_str().to_string(),
            },
            crate::application::library_agent_placements::LibraryAgentPlacementError::PartialSelection(
                _,
            ) => AppError::AgentSelectionInvalid {
                reason: crate::error::AgentSelectionInvalidReason::OptionUnavailable,
            },
        })?;
    Ok(requested
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect())
}

fn ensure_library_link_targets_supported<'a>(
    targets: impl IntoIterator<Item = &'a ResolvedTargetFact>,
) -> Result<(), AppError> {
    if let Some(target) = targets
        .into_iter()
        .find(|target| target.storage_access != StorageAccess::Native)
    {
        return Err(AppError::CapabilityUnavailable {
            capability: "skillLibraryLinks".to_string(),
            path: Some(target.destination.native_path.clone()),
        });
    }
    Ok(())
}

struct BuiltLibraryApplication {
    preview: LibraryApplicationPreview,
    record: LibraryApplicationRecord,
    plan: crate::application::mutation::plan::MutationPlan,
}

fn merge_library_skill_groups(
    current: BTreeMap<SkillDirectoryName, Vec<LibraryCatalogMember>>,
    target: BTreeMap<SkillDirectoryName, Vec<LibraryCatalogMember>>,
) -> Vec<LibrarySkillGroup> {
    let directory_names = current
        .keys()
        .chain(target.keys())
        .cloned()
        .collect::<BTreeSet<_>>();
    directory_names
        .into_iter()
        .map(|directory_name| LibrarySkillGroup {
            current_members: current.get(&directory_name).cloned().unwrap_or_default(),
            target_members: target.get(&directory_name).cloned().unwrap_or_default(),
            directory_name,
        })
        .collect()
}

async fn load_version_candidates(
    repository: &dyn LibraryApplicationRepository,
    context: &SkillLocationRef,
    members: &[LibraryCatalogMember],
) -> Result<Vec<LibraryVersionCandidate>, AppError> {
    let mut candidates = Vec::with_capacity(members.len());
    for member in members {
        candidates.push(LibraryVersionCandidate::new(
            member.library_id.clone(),
            member.member_name.clone(),
            repository
                .library_skill_locator(context, &member.library_id, &member.member_name)
                .await?,
        ));
    }
    Ok(candidates)
}

fn validated_library_ids(
    catalog: &LibraryCatalog,
    requested: &[LibraryId],
) -> Result<Vec<LibraryId>, AppError> {
    let mut seen = BTreeSet::new();
    let mut result = Vec::new();
    for id in requested {
        if !seen.insert(id.clone()) {
            return Err(AppError::Validation {
                field: Some("orderedLibraryIds".to_string()),
                message: "Skill Library cannot be selected more than once".to_string(),
            });
        }
        let library = catalog
            .libraries
            .iter()
            .find(|library| &library.id == id)
            .ok_or_else(|| AppError::PathNotFound {
                path: id.as_str().to_string(),
            })?;
        if library.skills.is_empty() {
            return Err(AppError::Validation {
                field: Some("orderedLibraryIds".to_string()),
                message: "empty Skill Library cannot be applied".to_string(),
            });
        }
        result.push(id.clone());
    }
    Ok(result)
}

fn library_member_index_error(error: impl std::fmt::Debug) -> AppError {
    AppError::ConfigurationCorrupted {
        message: format!("invalid Skill Library member index: {error:?}"),
    }
}

fn summary(
    record: &LibraryApplicationRecord,
    catalog: &LibraryCatalog,
) -> Result<LibraryApplicationSummary, AppError> {
    let mut ordered_libraries = Vec::new();
    for id in &record.current.ordered_library_ids {
        let library = catalog
            .libraries
            .iter()
            .find(|library| &library.id == id)
            .ok_or_else(|| AppError::PathNotFound {
                path: id.as_str().to_string(),
            })?;
        ordered_libraries.push(SkillLibrarySummary {
            id: library.id.clone(),
            name: library.name.clone(),
            skill_count: library.skills.len() as u32,
        });
    }
    Ok(LibraryApplicationSummary {
        ordered_libraries,
        selected_agent_ids: record.current.selected_agent_ids.clone(),
        pending: record.pending_operation.is_some(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    use crate::application::install::InstallFuture;
    use crate::application::mutation::executor::MutationFuture;
    use crate::application::mutation::plan::{MutationPlan, PreparedEntryAction, RuntimeRevisions};
    use crate::application::skill_libraries::{
        LibrarySkillRecord, LibrarySkillSourceRecord, SkillLibraryRecord, LIBRARY_SCHEMA_VERSION,
    };
    use crate::core::agent_definition::{
        AgentAdapter, AgentDefinition, AgentSource, DetectionSpec, PathSpec, ScopeDefinition,
    };
    use crate::core::lossless_lock::{LockSchema, LosslessLockDocument};
    use crate::environment::agent_environment::{
        AgentRuntimeSnapshot, DetectionState, DirectoryPresenceState, ResolvedAgent,
        ResolvedAgentScope,
    };
    use crate::environment::context_resolver::ResolvedContext;
    use crate::environment::planning::{TargetFactFuture, TargetFactResolver};
    use crate::environment::runtime::{
        ContextSnapshotRevision, EntryFingerprint, ExecutionBackend, PhysicalParentIdentity,
        PhysicalTargetKey,
    };
    use crate::environment::types::{
        EnvironmentRef, EnvironmentStatus, SkillLocation, StorageAccess,
    };

    const TEST_SKILL_ROOT: &str = "/scope/.agents/skills";
    const TEST_AGENT_ROOT: &str = "/agents/private/skills";
    const TEST_LIBRARY_ROOT: &str = "/libraries/lib-one/skills";

    #[derive(Clone)]
    struct FixedFacts(ScopePlanningSnapshot);

    impl ScopePlanningSnapshotSource for FixedFacts {
        fn snapshot<'a>(
            &'a self,
            _context: &'a SkillLocationRef,
        ) -> InstallFuture<'a, Result<ScopePlanningSnapshot, AppError>> {
            Box::pin(async move { Ok(self.0.clone()) })
        }
    }

    struct MemoryApplicationRepository {
        record: Mutex<LibraryApplicationRecord>,
        catalog: Mutex<LibraryCatalog>,
    }

    impl LibraryApplicationRepository for MemoryApplicationRepository {
        fn load_application<'a>(
            &'a self,
            _context: &'a SkillLocationRef,
        ) -> LibraryApplicationFuture<'a, Result<LibraryApplicationRecord, AppError>> {
            Box::pin(async move { Ok(self.record.lock().unwrap().clone()) })
        }

        fn save_application<'a>(
            &'a self,
            record: &'a LibraryApplicationRecord,
        ) -> LibraryApplicationFuture<'a, Result<(), AppError>> {
            Box::pin(async move {
                *self.record.lock().unwrap() = record.clone();
                Ok(())
            })
        }

        fn library_skill_locator<'a>(
            &'a self,
            context: &'a SkillLocationRef,
            library_id: &'a LibraryId,
            skill_name: &'a str,
        ) -> LibraryApplicationFuture<'a, Result<ResourceLocator, AppError>> {
            Box::pin(async move {
                Ok(ResourceLocator {
                    environment: context.environment.clone(),
                    native_path: format!("/libraries/{}/skills/{skill_name}", library_id.as_str()),
                })
            })
        }

        fn load_catalog<'a>(
            &'a self,
            _context: &'a SkillLocationRef,
        ) -> LibraryApplicationFuture<'a, Result<LibraryCatalog, AppError>> {
            Box::pin(async move { Ok(self.catalog.lock().unwrap().clone()) })
        }

        fn remove_application<'a>(
            &'a self,
            _context: &'a SkillLocationRef,
        ) -> LibraryApplicationFuture<'a, Result<(), AppError>> {
            Box::pin(async move { Ok(()) })
        }
    }

    #[derive(Clone)]
    struct FixedTargets {
        primary_entry_kind: TargetEntryKind,
        agent_entry_kind: TargetEntryKind,
        agent_link_target: Option<String>,
    }

    impl TargetFactResolver for FixedTargets {
        fn resolve<'a>(
            &'a self,
            _context: &'a SkillLocationRef,
            logical_destinations: &'a [ResourceLocator],
            _cancellation: Option<CancellationSignal>,
        ) -> TargetFactFuture<'a, Result<Vec<ResolvedTargetFact>, AppError>> {
            Box::pin(async move {
                Ok(logical_destinations
                    .iter()
                    .map(|destination| {
                        let agent_skill = locator(TEST_AGENT_ROOT).join_child("demo");
                        let (name, entry_kind, link_target) =
                            if destination.native_path == TEST_AGENT_ROOT {
                                ("agent-root", TargetEntryKind::Directory, None)
                            } else if destination.native_path == agent_skill.native_path {
                                (
                                    "agent-skill",
                                    self.agent_entry_kind,
                                    self.agent_link_target.clone(),
                                )
                            } else {
                                ("canonical-skill", self.primary_entry_kind, None)
                            };
                        let link_target_identity = link_target.as_deref().and_then(|raw| {
                            crate::environment::planning::resolve_link_target_identity(
                                destination,
                                raw,
                            )
                        });
                        ResolvedTargetFact {
                            key: physical_key(name),
                            destination: destination.clone(),
                            storage_access: StorageAccess::Native,
                            fingerprint: EntryFingerprint(format!("entry-v1-{name}")),
                            entry_kind,
                            link_target,
                            link_target_identity,
                        }
                    })
                    .collect())
            })
        }
    }

    #[derive(Clone, Default)]
    struct RecordingExecutor(Arc<Mutex<Option<MutationPlan>>>);

    impl MutationPlanExecutor for RecordingExecutor {
        fn execute<'a>(
            &'a self,
            plan: MutationPlan,
            _cancellation: CancellationSignal,
        ) -> MutationFuture<'a, Vec<MutationUnitResult>> {
            Box::pin(async move {
                let results = plan
                    .units
                    .iter()
                    .map(|unit| MutationUnitResult {
                        unit_id: unit.id.clone(),
                        skill_name: unit.skill_name.clone(),
                        source: unit.source.clone(),
                        target: unit.target.clone(),
                        status: MutationUnitStatus::Succeeded,
                        retryable: false,
                        lock_committed: false,
                        actual_mode: None,
                        fallback_reason: None,
                        agent_targets: Vec::new(),
                        warnings: Vec::new(),
                        error: None,
                        recovery: None,
                    })
                    .collect();
                *self.0.lock().unwrap() = Some(plan);
                results
            })
        }
    }

    fn application_fixture(
        agent_entry_kind: TargetEntryKind,
        agent_link_target: Option<&str>,
    ) -> (
        LibraryApplicationModule<FixedFacts, FixedTargets, RecordingExecutor>,
        RecordingExecutor,
        LibraryApplicationDraft,
    ) {
        application_fixture_with(
            TargetEntryKind::Missing,
            agent_entry_kind,
            agent_link_target,
            vec![SkillLibraryRecord {
                id: LibraryId::parse("lib-one"),
                name: "Library One".to_string(),
                skills: vec![skill("demo")],
                extra: serde_json::Map::new(),
            }],
            Vec::new(),
            vec![LibraryId::parse("lib-one")],
        )
    }

    fn application_fixture_with(
        primary_entry_kind: TargetEntryKind,
        agent_entry_kind: TargetEntryKind,
        agent_link_target: Option<&str>,
        libraries: Vec<SkillLibraryRecord>,
        current_library_ids: Vec<LibraryId>,
        target_library_ids: Vec<LibraryId>,
    ) -> (
        LibraryApplicationModule<FixedFacts, FixedTargets, RecordingExecutor>,
        RecordingExecutor,
        LibraryApplicationDraft,
    ) {
        let context = SkillLocationRef {
            environment: EnvironmentRef::Native,
            scope: SkillLocation::Global,
        };
        let agent_id = AgentId::parse("private-agent").unwrap();
        let disabled_scope = ScopeDefinition {
            enabled: false,
            reads_standard: false,
            private_path: None,
        };
        let disabled_resolved_scope = ResolvedAgentScope {
            enabled: false,
            reads_standard: false,
            standard_path: None,
            private_path: None,
            read_paths: Vec::new(),
            standard_presence: None,
            private_presence: None,
            legacy_paths: Vec::new(),
        };
        let runtime = AgentRuntimeSnapshot {
            registry_revision: "registry-1".to_string(),
            environment_revision: "environment-1".to_string(),
            environment: EnvironmentRef::Native,
            availability: EnvironmentStatus::Available,
            project_path: None,
            agents: BTreeMap::from([(
                agent_id.clone(),
                ResolvedAgent {
                    definition: AgentDefinition {
                        id: agent_id.clone(),
                        display_name: "Private Agent".to_string(),
                        source: AgentSource::Custom,
                        aliases: Vec::new(),
                        global: ScopeDefinition {
                            enabled: true,
                            reads_standard: false,
                            private_path: Some(PathSpec::home(".private-agent/skills")),
                        },
                        project: disabled_scope,
                        detection: DetectionSpec::AnyPathExists {
                            paths: vec![PathSpec::home(".private-agent")],
                        },
                        legacy_paths: Vec::new(),
                        adapter: AgentAdapter::Standard,
                    },
                    detection: DetectionState::Detected,
                    detection_reason: None,
                    global: ResolvedAgentScope {
                        enabled: true,
                        reads_standard: false,
                        standard_path: None,
                        private_path: Some(TEST_AGENT_ROOT.to_string()),
                        read_paths: vec![TEST_AGENT_ROOT.to_string()],
                        standard_presence: None,
                        private_presence: Some(DirectoryPresenceState::Present),
                        legacy_paths: Vec::new(),
                    },
                    project: disabled_resolved_scope,
                },
            )]),
        };
        let revisions = RuntimeRevisions {
            registry: "registry-1".to_string(),
            environment: "environment-1".to_string(),
            context: ContextSnapshotRevision::parse("context-v1-library-application").unwrap(),
        };
        let facts = FixedFacts(ScopePlanningSnapshot {
            resolved_context: ResolvedContext {
                context: context.clone(),
                project: None,
                home: locator("/home/test"),
                skill_root: locator(TEST_SKILL_ROOT),
                lock: locator("/home/test/.agents/.skill-lock.json"),
            },
            agent_runtime: runtime,
            revisions,
            lock_schema: LockSchema::Global,
            lock_document: LosslessLockDocument::empty(LockSchema::Global),
            eve_targets: Vec::new(),
        });
        let current_agent_ids = (!current_library_ids.is_empty()).then(|| agent_id.clone());
        let repository = Arc::new(MemoryApplicationRepository {
            record: Mutex::new(LibraryApplicationRecord {
                current: LibraryApplicationState {
                    ordered_library_ids: current_library_ids,
                    selected_agent_ids: current_agent_ids.into_iter().collect(),
                },
                ..LibraryApplicationRecord::empty(context.clone())
            }),
            catalog: Mutex::new(LibraryCatalog {
                schema_version: LIBRARY_SCHEMA_VERSION,
                libraries,
                extra: serde_json::Map::new(),
            }),
        });
        let executor = RecordingExecutor::default();
        let module = LibraryApplicationModule::new(
            repository,
            facts,
            FixedTargets {
                primary_entry_kind,
                agent_entry_kind,
                agent_link_target: agent_link_target.map(str::to_string),
            },
            executor.clone(),
        );
        let draft = LibraryApplicationDraft {
            context,
            ordered_library_ids: target_library_ids,
            selected_agent_ids: vec![agent_id],
        };
        (module, executor, draft)
    }

    async fn applied_result(
        module: &LibraryApplicationModule<FixedFacts, FixedTargets, RecordingExecutor>,
        executor: &RecordingExecutor,
        draft: LibraryApplicationDraft,
    ) -> (MutationPlan, LibraryApplicationResponse) {
        let preview = module.preview(draft.clone()).await.unwrap();
        let response = module
            .apply(
                ApplyLibraryApplicationRequest {
                    draft,
                    expected_token: preview.token,
                },
                CancellationSignal::default(),
            )
            .await
            .unwrap();
        let plan = executor.0.lock().unwrap().take().unwrap();
        (plan, response)
    }

    fn record_with(
        current: &[&str],
        pending: Option<(&[&str], &[&str])>,
    ) -> LibraryApplicationRecord {
        let context = SkillLocationRef {
            environment: EnvironmentRef::Native,
            scope: crate::environment::types::SkillLocation::Global,
        };
        let ids = |values: &[&str]| values.iter().map(|id| LibraryId::parse(*id)).collect();
        LibraryApplicationRecord {
            schema_version: LIBRARY_APPLICATION_SCHEMA_VERSION,
            target: context,
            current: LibraryApplicationState {
                ordered_library_ids: ids(current),
                selected_agent_ids: Vec::new(),
            },
            pending_operation: pending.map(|(before, target)| PendingLibraryApplication {
                operation_id: "operation".to_string(),
                before_application: LibraryApplicationState {
                    ordered_library_ids: ids(before),
                    selected_agent_ids: Vec::new(),
                },
                target_application: LibraryApplicationState {
                    ordered_library_ids: ids(target),
                    selected_agent_ids: Vec::new(),
                },
                preview_fingerprint: "fingerprint".to_string(),
            }),
        }
    }

    #[test]
    fn confirmed_and_pending_library_usage_stay_distinct() {
        let record = record_with(&["applied"], Some((&["applied"], &["applied", "incoming"])));

        assert_eq!(
            library_usage_state(&record, &LibraryId::parse("applied")),
            Some(LibraryUsageState::Confirmed)
        );
        assert_eq!(
            library_usage_state(&record, &LibraryId::parse("incoming")),
            Some(LibraryUsageState::PendingAdjustment)
        );
        assert_eq!(
            library_usage_state(&record, &LibraryId::parse("other")),
            None
        );
    }

    #[test]
    fn a_library_leaving_in_a_pending_operation_still_counts_as_locked() {
        // 目标状态已经不含该库，但操作尚未完成，成员仍需锁定。
        let record = record_with(&[], Some((&["leaving"], &[])));

        assert_eq!(
            library_usage_state(&record, &LibraryId::parse("leaving")),
            Some(LibraryUsageState::PendingAdjustment)
        );
    }

    #[test]
    fn usage_projection_counts_each_location_once_per_state() {
        let mut accumulator = LibraryUsageAccumulator::default();
        // 全局：applied 已生效，incoming 只在未完成操作中。
        accumulator.observe(&record_with(
            &["applied"],
            Some((&["applied"], &["applied", "incoming"])),
        ));
        // 项目 A：applied 已生效，且未完成操作重复引用它，仍只计一次。
        accumulator.observe(&record_with(
            &["applied"],
            Some((&["applied"], &["applied"])),
        ));
        // 项目 B：没有任何引用。
        accumulator.observe(&record_with(&[], None));

        assert_eq!(
            accumulator.finish(),
            vec![
                LibraryUsageProjection {
                    library_id: LibraryId::parse("applied"),
                    confirmed_count: 2,
                    pending_count: 0,
                },
                LibraryUsageProjection {
                    library_id: LibraryId::parse("incoming"),
                    confirmed_count: 0,
                    pending_count: 1,
                },
            ]
        );
    }

    #[test]
    fn usage_projection_omits_libraries_without_any_reference() {
        let mut accumulator = LibraryUsageAccumulator::default();
        accumulator.observe(&record_with(&[], None));

        assert!(accumulator.finish().is_empty());
    }

    #[test]
    fn resolves_one_library_and_rejects_an_empty_library() {
        let filled_id = LibraryId::parse("filled");
        let empty_id = LibraryId::parse("empty");
        let catalog = LibraryCatalog {
            schema_version: LIBRARY_SCHEMA_VERSION,
            libraries: vec![
                SkillLibraryRecord {
                    id: filled_id.clone(),
                    name: "Backend".to_string(),
                    skills: vec![skill("api-design")],
                    extra: serde_json::Map::new(),
                },
                SkillLibraryRecord {
                    id: empty_id.clone(),
                    name: "Empty".to_string(),
                    skills: Vec::new(),
                    extra: serde_json::Map::new(),
                },
            ],
            extra: serde_json::Map::new(),
        };

        let grouped = LibraryCatalogMemberIndex::build(&catalog)
            .unwrap()
            .members_for(std::slice::from_ref(&filled_id))
            .unwrap();
        assert_eq!(
            grouped
                .get(&SkillDirectoryName::try_from("api-design").unwrap())
                .unwrap()[0]
                .library_id,
            filled_id
        );
        assert!(validated_library_ids(&catalog, &[empty_id]).is_err());
    }

    #[test]
    fn resolves_duplicate_skill_names_from_the_first_library() {
        let first = LibraryId::parse("first");
        let second = LibraryId::parse("second");
        let catalog = LibraryCatalog {
            schema_version: LIBRARY_SCHEMA_VERSION,
            libraries: vec![
                SkillLibraryRecord {
                    id: first.clone(),
                    name: "First".to_string(),
                    skills: vec![skill("review")],
                    extra: serde_json::Map::new(),
                },
                SkillLibraryRecord {
                    id: second.clone(),
                    name: "Second".to_string(),
                    skills: vec![skill("review")],
                    extra: serde_json::Map::new(),
                },
            ],
            extra: serde_json::Map::new(),
        };

        let skill = SkillDirectoryName::try_from("review").unwrap();
        let index = LibraryCatalogMemberIndex::build(&catalog).unwrap();
        assert_eq!(
            index.members_for(&[first.clone(), second.clone()]).unwrap()[&skill][0].library_id,
            first
        );
        assert_eq!(
            index.members_for(&[second.clone(), first]).unwrap()[&skill][0].library_id,
            second
        );
    }

    #[test]
    fn allows_different_libraries_to_use_aliases_for_the_same_skill_directory() {
        let first = LibraryId::parse("first");
        let second = LibraryId::parse("second");
        let catalog = LibraryCatalog {
            schema_version: LIBRARY_SCHEMA_VERSION,
            libraries: vec![
                SkillLibraryRecord {
                    id: first.clone(),
                    name: "First".to_string(),
                    skills: vec![skill("CE:Review")],
                    extra: serde_json::Map::new(),
                },
                SkillLibraryRecord {
                    id: second.clone(),
                    name: "Second".to_string(),
                    skills: vec![skill("ce-review")],
                    extra: serde_json::Map::new(),
                },
            ],
            extra: serde_json::Map::new(),
        };

        assert_eq!(
            validated_library_ids(&catalog, &[first.clone(), second.clone()]).unwrap(),
            vec![first, second]
        );
    }

    #[tokio::test]
    async fn switching_physical_aliases_uses_one_task_and_the_target_member() {
        let first = LibraryId::parse("first");
        let second = LibraryId::parse("second");
        let (module, executor, draft) = application_fixture_with(
            TargetEntryKind::Missing,
            TargetEntryKind::Missing,
            None,
            vec![
                SkillLibraryRecord {
                    id: first.clone(),
                    name: "First".to_string(),
                    skills: vec![skill("CE:Review")],
                    extra: serde_json::Map::new(),
                },
                SkillLibraryRecord {
                    id: second.clone(),
                    name: "Second".to_string(),
                    skills: vec![skill("ce-review")],
                    extra: serde_json::Map::new(),
                },
            ],
            vec![first],
            vec![second],
        );

        let preview = module.preview(draft.clone()).await.unwrap();
        assert_eq!(preview.switched_skill_names, vec!["ce-review"]);

        let (plan, _response) = applied_result(&module, &executor, draft).await;

        assert_eq!(plan.units.len(), 1);
        assert_eq!(plan.units[0].skill_name, "ce-review");
        assert!(matches!(
            plan.units[0]
                .primary_entry
                .as_ref()
                .map(|entry| &entry.action),
            Some(PreparedEntryAction::Link { target })
                if target.native_path == "/libraries/second/skills/ce-review"
        ));
    }

    #[test]
    fn incomplete_execution_results_do_not_complete_an_application() {
        assert!(!library_execution_completed(1, &[]));
    }

    #[test]
    fn library_application_preview_evidence_changes_with_the_catalog() {
        let context = SkillLocationRef {
            environment: EnvironmentRef::Native,
            scope: SkillLocation::Global,
        };
        let record = LibraryApplicationRecord::empty(context);
        let target = LibraryApplicationState::default();
        let catalog = LibraryCatalog {
            schema_version: LIBRARY_SCHEMA_VERSION,
            libraries: vec![SkillLibraryRecord {
                id: LibraryId::parse("lib-one"),
                name: "Library One".to_string(),
                skills: Vec::new(),
                extra: serde_json::Map::new(),
            }],
            extra: serde_json::Map::new(),
        };
        let before = library_application_observed_digest(&record, &target, &catalog, &[]).unwrap();
        let mut changed = catalog.clone();
        changed.libraries[0].skills.push(skill("demo"));

        let after = library_application_observed_digest(&record, &target, &changed, &[]).unwrap();

        assert_ne!(before, after);
    }

    #[tokio::test]
    async fn applying_a_library_preserves_a_direct_skill_in_an_agent_directory() {
        let (module, executor, draft) = application_fixture(TargetEntryKind::Directory, None);

        let (plan, response) = applied_result(&module, &executor, draft).await;

        assert_eq!(plan.units.len(), 1);
        assert_eq!(
            response.application.selected_agent_ids,
            vec![AgentId::parse("private-agent").unwrap()]
        );
        assert!(matches!(
            plan.units[0]
                .primary_entry
                .as_ref()
                .map(|entry| &entry.action),
            Some(PreparedEntryAction::Link { target })
                if target.native_path == format!("{TEST_LIBRARY_ROOT}/demo")
        ));
        assert!(plan.units[0]
            .additional_entries
            .iter()
            .all(|entry| entry.action == PreparedEntryAction::Keep));
    }

    #[tokio::test]
    async fn applying_a_library_replaces_a_broken_agent_link() {
        let (module, executor, draft) =
            application_fixture(TargetEntryKind::BrokenLink, Some("/missing/direct-skill"));

        let (plan, _response) = applied_result(&module, &executor, draft).await;

        assert_eq!(plan.units.len(), 1);
        assert!(matches!(
            plan.units[0]
                .additional_entries
                .first()
                .map(|entry| &entry.action),
            Some(PreparedEntryAction::Link { target })
                if target.native_path == format!("{TEST_LIBRARY_ROOT}/demo")
        ));
    }

    #[tokio::test]
    async fn applying_a_library_preserves_a_valid_direct_agent_link() {
        let (module, executor, draft) =
            application_fixture(TargetEntryKind::Symlink, Some("/direct-skill/demo"));

        let (plan, response) = applied_result(&module, &executor, draft).await;

        assert!(plan.units[0]
            .additional_entries
            .iter()
            .all(|entry| entry.action == PreparedEntryAction::Keep));
        assert_eq!(
            response.application.selected_agent_ids,
            vec![AgentId::parse("private-agent").unwrap()]
        );
    }

    #[tokio::test]
    async fn applying_a_library_identifies_an_unsupported_agent_entry() {
        let (module, _executor, draft) = application_fixture(TargetEntryKind::File, None);

        let error = module.preview(draft).await.unwrap_err();

        assert!(matches!(
            error,
            AppError::SkillPlacementTargetConflict {
                skill_name,
                agent_ids,
                target_path,
                target_kind,
            } if skill_name == "demo"
                && agent_ids == vec![AgentId::parse("private-agent").unwrap()]
                && target_path.ends_with("demo")
                && target_kind == crate::error::SkillPlacementTargetKind::File
        ));
    }

    #[tokio::test]
    async fn reordering_libraries_only_switches_directories_using_library_versions() {
        let first = LibraryId::parse("first");
        let second = LibraryId::parse("second");
        let library = |id: LibraryId, name: &str| SkillLibraryRecord {
            id,
            name: name.to_string(),
            skills: vec![skill("demo")],
            extra: serde_json::Map::new(),
        };
        let (module, executor, draft) = application_fixture_with(
            TargetEntryKind::Directory,
            TargetEntryKind::Symlink,
            Some("/libraries/first/skills/demo"),
            vec![
                library(first.clone(), "First"),
                library(second.clone(), "Second"),
            ],
            vec![first, second.clone()],
            vec![second, LibraryId::parse("first")],
        );

        let preview = module.preview(draft.clone()).await.unwrap();

        assert_eq!(preview.switched_skill_names, vec!["demo"]);

        let (plan, response) = applied_result(&module, &executor, draft).await;

        assert_eq!(plan.units.len(), 1);
        assert_eq!(
            plan.units[0]
                .primary_entry
                .as_ref()
                .map(|entry| &entry.action),
            Some(&PreparedEntryAction::Keep)
        );
        assert_eq!(plan.units[0].additional_entries.len(), 1);
        assert!(matches!(
            &plan.units[0].additional_entries[0].action,
            PreparedEntryAction::Link { target }
                if target.native_path == "/libraries/second/skills/demo"
        ));
        assert_eq!(
            response.application.ordered_libraries[0].id,
            LibraryId::parse("second")
        );
        assert_eq!(
            response.application.selected_agent_ids,
            vec![AgentId::parse("private-agent").unwrap()]
        );
    }

    #[tokio::test]
    async fn removing_a_broken_agent_library_link_is_visible_when_canonical_is_direct() {
        let library_id = LibraryId::parse("lib-one");
        let (module, _executor, mut draft) = application_fixture_with(
            TargetEntryKind::Directory,
            TargetEntryKind::BrokenLink,
            Some("/libraries/lib-one/skills/demo"),
            vec![SkillLibraryRecord {
                id: library_id.clone(),
                name: "Library One".to_string(),
                skills: vec![skill("demo")],
                extra: serde_json::Map::new(),
            }],
            vec![library_id],
            Vec::new(),
        );
        draft.selected_agent_ids.clear();

        let preview = module.preview(draft).await.unwrap();

        assert_eq!(preview.removed_skill_names, vec!["demo"]);
        assert_eq!(preview.overridden_by_direct_skill_names, vec!["demo"]);
    }

    #[tokio::test]
    async fn changing_only_library_agent_associations_reports_directory_changes() {
        let library_id = LibraryId::parse("lib-one");
        let (module, _executor, mut draft) = application_fixture_with(
            TargetEntryKind::Directory,
            TargetEntryKind::Symlink,
            Some("/libraries/lib-one/skills/demo"),
            vec![SkillLibraryRecord {
                id: library_id.clone(),
                name: "Library One".to_string(),
                skills: vec![skill("demo")],
                extra: serde_json::Map::new(),
            }],
            vec![library_id.clone()],
            vec![library_id],
        );
        draft.selected_agent_ids.clear();

        let preview = module.preview(draft).await.unwrap();

        assert!(preview.added_skill_names.is_empty());
        assert!(preview.removed_skill_names.is_empty());
        assert!(preview.switched_skill_names.is_empty());
        assert_eq!(preview.changed_directory_skill_names, vec!["demo"]);
    }

    #[test]
    fn library_links_reject_non_native_storage_facts() {
        let mut cross_storage = fact(TargetEntryKind::Missing, None);
        cross_storage.storage_access = StorageAccess::CrossStorage;
        assert!(matches!(
            ensure_library_link_targets_supported(&[cross_storage]),
            Err(AppError::CapabilityUnavailable { .. })
        ));
        assert!(
            ensure_library_link_targets_supported(&[fact(TargetEntryKind::Missing, None,)]).is_ok()
        );
    }

    fn skill(name: &str) -> LibrarySkillRecord {
        LibrarySkillRecord {
            name: name.to_string(),
            description: "description".to_string(),
            source_record: serde_json::to_value(LibrarySkillSourceRecord {
                source_type: "git".to_string(),
                source: "source".to_string(),
                reacquisition_url: None,
                ref_name: None,
                skill_path: Some(name.to_string()),
                installed_revision: None,
                computed_hash: Some("hash".to_string()),
                artifact_url: None,
                plugin_name: None,
                well_known: None,
                extra: serde_json::Map::new(),
            })
            .unwrap(),
            content_manifest_hash: "hash".to_string(),
            updated_at: None,
            extra: serde_json::Map::new(),
        }
    }

    fn locator(path: &str) -> ResourceLocator {
        ResourceLocator {
            environment: EnvironmentRef::Native,
            native_path: path.to_string(),
        }
    }

    fn fact(entry_kind: TargetEntryKind, link_target: Option<&str>) -> ResolvedTargetFact {
        let destination = locator("/skills/demo");
        ResolvedTargetFact {
            key: physical_key("demo"),
            link_target_identity: link_target.and_then(|raw| {
                crate::environment::planning::resolve_link_target_identity(&destination, raw)
            }),
            destination,
            storage_access: StorageAccess::Native,
            fingerprint: EntryFingerprint("entry-v1-test".to_string()),
            entry_kind,
            link_target: link_target.map(str::to_string),
        }
    }

    fn physical_key(name: &str) -> PhysicalTargetKey {
        PhysicalTargetKey {
            backend: ExecutionBackend::NativeUnix,
            physical_parent: PhysicalParentIdentity::Unix {
                device: 1,
                inode: 2,
            },
            normalized_final_child_name: name.to_string(),
        }
    }
}

use std::collections::{BTreeMap, BTreeSet};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::application::agent_selection::{
    build_agent_selection_catalog, AgentSelectionAgentKind, AgentSelectionCatalog,
    AgentSelectionSnapshot, DirectoryPlacementId, UnavailableAgentSelection,
    UnavailableAgentSelectionReason,
};
use crate::application::installed_skill_resolver::SkillDirectoryName;
use crate::application::library_agent_placements::LibraryAgentPlacementMap;
use crate::application::library_candidates::{
    LibraryCandidateSet, LibraryCatalogMember, LibraryCatalogMemberIndex,
    ResolvedLibraryCandidateIndex,
};
use crate::application::mutation::executor::MutationPlanExecutor;
use crate::application::mutation::plan::{PreparedEntryAction, PreviewToken};
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
use crate::storage::atomic_document::DocumentSnapshot;

pub const LIBRARY_APPLICATION_SCHEMA_VERSION: u32 = 1;
pub type LibraryApplicationFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct LibraryApplicationState {
    pub ordered_library_ids: Vec<LibraryId>,
    pub selected_agent_ids: Vec<AgentId>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryMemberIdentity {
    pub(crate) library_id: LibraryId,
    pub(crate) member_name: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReconciliationCheckpoint {
    pub members: Vec<LibraryMemberIdentity>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ReconciliationReason {
    ApplicationChanged,
    MembershipChanged,
    ReapplyRequested,
    VerificationRequired,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ReconciliationAttention {
    Pending,
    Unverified,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingReconciliation {
    pub reconciliation_id: String,
    pub attention: ReconciliationAttention,
    pub reasons: Vec<ReconciliationReason>,
    pub before_application: LibraryApplicationState,
    pub target_application: LibraryApplicationState,
    pub recognized_members: Vec<LibraryMemberIdentity>,
    pub affected_members: Vec<LibraryMemberIdentity>,
    pub target_members: Vec<LibraryMemberIdentity>,
}

impl PendingReconciliation {
    fn merged_with(
        mut self,
        reasons: Vec<ReconciliationReason>,
        target_application: LibraryApplicationState,
        recognized_members: Vec<LibraryMemberIdentity>,
        target_members: Vec<LibraryMemberIdentity>,
    ) -> Self {
        self.affected_members = self
            .affected_members
            .into_iter()
            .chain(changed_member_identities(
                &self.target_members,
                &target_members,
            ))
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        self.reasons = self
            .reasons
            .into_iter()
            .chain(reasons)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        self.recognized_members = self
            .recognized_members
            .into_iter()
            .chain(recognized_members)
            .chain(target_members.iter().cloned())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        self.target_application = target_application;
        self.target_members = target_members;
        self
    }
}

fn reconciliation_members(
    checkpoint: &ReconciliationCheckpoint,
    pending: Option<&PendingReconciliation>,
    desired: &[LibraryMemberIdentity],
) -> Vec<LibraryMemberIdentity> {
    checkpoint
        .members
        .iter()
        .chain(
            pending
                .into_iter()
                .flat_map(|pending| pending.recognized_members.iter()),
        )
        .chain(desired)
        .cloned()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryApplicationRecord {
    pub schema_version: u32,
    pub current: LibraryApplicationState,
    pub checkpoint: ReconciliationCheckpoint,
    pub pending: Option<PendingReconciliation>,
}

impl LibraryApplicationRecord {
    pub fn empty() -> Self {
        Self {
            schema_version: LIBRARY_APPLICATION_SCHEMA_VERSION,
            current: LibraryApplicationState::default(),
            checkpoint: ReconciliationCheckpoint::default(),
            pending: None,
        }
    }
}

pub(crate) fn validate_application_record(
    record: &LibraryApplicationRecord,
) -> Result<(), AppError> {
    let invalid = || AppError::ConfigurationCorrupted {
        message: "invalid Skill Library application record".to_string(),
    };
    if record.schema_version != LIBRARY_APPLICATION_SCHEMA_VERSION
        || has_duplicates(&record.current.ordered_library_ids)
        || has_duplicates(&record.current.selected_agent_ids)
        || !is_canonical_members(&record.checkpoint.members)
    {
        return Err(invalid());
    }
    let Some(pending) = &record.pending else {
        return Ok(());
    };
    if pending.reconciliation_id.is_empty()
        || pending.reconciliation_id.contains(['/', '\\', '\0'])
        || pending.reasons.is_empty()
        || !is_strictly_sorted(&pending.reasons)
        || pending.before_application != record.current
        || has_duplicates(&pending.target_application.ordered_library_ids)
        || has_duplicates(&pending.target_application.selected_agent_ids)
        || !is_canonical_members(&pending.recognized_members)
        || !is_canonical_members(&pending.affected_members)
        || !is_canonical_members(&pending.target_members)
    {
        return Err(invalid());
    }
    let recognized = pending.recognized_members.iter().collect::<BTreeSet<_>>();
    if record
        .checkpoint
        .members
        .iter()
        .chain(&pending.target_members)
        .chain(&pending.affected_members)
        .any(|member| !recognized.contains(member))
    {
        return Err(invalid());
    }
    Ok(())
}

fn has_duplicates<T: Ord>(values: &[T]) -> bool {
    values.iter().collect::<BTreeSet<_>>().len() != values.len()
}

fn is_strictly_sorted<T: Ord>(values: &[T]) -> bool {
    values.windows(2).all(|pair| pair[0] < pair[1])
}

fn is_canonical_members(values: &[LibraryMemberIdentity]) -> bool {
    values
        .iter()
        .all(|member| !member.library_id.as_str().is_empty() && !member.member_name.is_empty())
        && is_strictly_sorted(values)
}

#[derive(Debug, Clone)]
pub struct VersionedApplicationRecord {
    pub context: SkillLocationRef,
    pub record: LibraryApplicationRecord,
    pub(crate) target: ResourceLocator,
    pub(crate) snapshot: DocumentSnapshot,
}

#[derive(Debug)]
pub struct ApplicationInventoryProblem {
    pub storage_key: String,
    pub error: AppError,
}

#[derive(Debug)]
pub struct ApplicationInventory {
    pub records: Vec<VersionedApplicationRecord>,
    pub problems: Vec<ApplicationInventoryProblem>,
    pub complete: bool,
}

impl VersionedApplicationRecord {
    #[cfg(test)]
    pub(crate) fn in_memory(context: SkillLocationRef, record: LibraryApplicationRecord) -> Self {
        let bytes = serde_json::to_vec_pretty(&record).expect("in-memory application record");
        Self {
            target: ResourceLocator {
                environment: context.environment.clone(),
                native_path: format!("memory://{:?}", context.scope),
            },
            snapshot: DocumentSnapshot {
                bytes: Some(bytes),
                generation: None,
            },
            context,
            record,
        }
    }
}

impl std::ops::Deref for VersionedApplicationRecord {
    type Target = LibraryApplicationRecord;

    fn deref(&self) -> &Self::Target {
        &self.record
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
    let pending = record.pending.as_ref().is_some_and(|pending| {
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
        if let Some(operation) = record.pending.as_ref() {
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
    pub sync_state: LibraryApplicationSyncState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub enum LibraryApplicationSyncState {
    Synced,
    Pending,
    Unverified,
    RecoveryRequired,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ScopeRecoveryState {
    Clear,
    Unverified,
    Required,
}

pub(crate) trait LibraryApplicationRecoveryStatus: Send + Sync {
    fn status<'a>(
        &'a self,
        context: &'a SkillLocationRef,
    ) -> LibraryApplicationFuture<'a, Result<ScopeRecoveryState, AppError>>;
}

#[cfg(test)]
struct ClearRecoveryStatus;

#[cfg(test)]
impl LibraryApplicationRecoveryStatus for ClearRecoveryStatus {
    fn status<'a>(
        &'a self,
        _context: &'a SkillLocationRef,
    ) -> LibraryApplicationFuture<'a, Result<ScopeRecoveryState, AppError>> {
        Box::pin(async { Ok(ScopeRecoveryState::Clear) })
    }
}

fn application_sync_state(
    record: &LibraryApplicationRecord,
    target_members: &[LibraryMemberIdentity],
    recovery: ScopeRecoveryState,
) -> LibraryApplicationSyncState {
    if recovery == ScopeRecoveryState::Required {
        LibraryApplicationSyncState::RecoveryRequired
    } else if recovery == ScopeRecoveryState::Unverified
        || record
            .pending
            .as_ref()
            .is_some_and(|pending| pending.attention == ReconciliationAttention::Unverified)
    {
        LibraryApplicationSyncState::Unverified
    } else if record.pending.is_some() || record.checkpoint.members != target_members {
        LibraryApplicationSyncState::Pending
    } else {
        LibraryApplicationSyncState::Synced
    }
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

#[derive(Debug, Clone)]
pub(crate) struct LibraryApplicationScopePlan {
    pub context: SkillLocationRef,
    pub entries: Vec<(PhysicalTargetKey, PreparedEntryAction)>,
    pub preview: Option<LibraryApplicationPreview>,
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

pub trait ApplicationRegistry: Send + Sync {
    fn load_application<'a>(
        &'a self,
        context: &'a SkillLocationRef,
    ) -> LibraryApplicationFuture<'a, Result<VersionedApplicationRecord, AppError>>;

    fn save_application_if<'a>(
        &'a self,
        observed: &'a VersionedApplicationRecord,
        record: &'a LibraryApplicationRecord,
    ) -> LibraryApplicationFuture<'a, Result<VersionedApplicationRecord, AppError>>;

    fn enumerate<'a>(
        &'a self,
        environment: &'a crate::environment::types::EnvironmentRef,
    ) -> LibraryApplicationFuture<'a, Result<ApplicationInventory, AppError>>;
}

pub(crate) trait LibraryApplicationResources: Send + Sync {
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

    fn remove_application_if<'a>(
        &'a self,
        observed: &'a VersionedApplicationRecord,
    ) -> LibraryApplicationFuture<'a, Result<(), AppError>>;
}

pub(crate) trait LibraryApplicationBackend:
    ApplicationRegistry + LibraryApplicationResources
{
}

impl<T> LibraryApplicationBackend for T where
    T: ApplicationRegistry + LibraryApplicationResources + ?Sized
{
}

pub struct LibraryApplicationModule<F, T, E> {
    repository: Arc<dyn LibraryApplicationBackend>,
    facts: F,
    targets: T,
    executor: E,
    recovery: Arc<dyn LibraryApplicationRecoveryStatus>,
}

impl<F, T, E> LibraryApplicationModule<F, T, E>
where
    F: ScopePlanningSnapshotSource,
    T: TargetFactResolver,
    E: MutationPlanExecutor,
{
    #[cfg(test)]
    pub fn new(
        repository: Arc<dyn LibraryApplicationBackend>,
        facts: F,
        targets: T,
        executor: E,
    ) -> Self {
        Self::with_recovery_status(
            repository,
            facts,
            targets,
            executor,
            Arc::new(ClearRecoveryStatus),
        )
    }

    pub fn with_recovery_status(
        repository: Arc<dyn LibraryApplicationBackend>,
        facts: F,
        targets: T,
        executor: E,
        recovery: Arc<dyn LibraryApplicationRecoveryStatus>,
    ) -> Self {
        Self {
            repository,
            facts,
            targets,
            executor,
            recovery,
        }
    }

    pub async fn read(
        &self,
        context: SkillLocationRef,
    ) -> Result<LibraryApplicationSummary, AppError> {
        let record = self.repository.load_application(&context).await?;
        let catalog = self.repository.load_catalog(&context).await?;
        let recovery = self
            .recovery
            .status(&context)
            .await
            .unwrap_or(ScopeRecoveryState::Unverified);
        summary(&record, &catalog, recovery)
    }

    pub async fn agent_options(
        &self,
        context: SkillLocationRef,
    ) -> Result<LibraryAgentOptions, AppError> {
        let record = self.repository.load_application(&context).await?;
        let facts = self.facts.snapshot(&context).await?;
        let resolved = resolve_library_agent_options(
            &context,
            &facts,
            &self.targets,
            &persisted_agent_ids(&record.record),
        )
        .await?;
        Ok(LibraryAgentOptions {
            selection: resolved.selection,
            migrations: resolved.migrations,
            unsupported_agent_names: resolved.unsupported_agent_names,
        })
    }

    pub async fn preview(
        &self,
        draft: LibraryApplicationDraft,
    ) -> Result<LibraryApplicationPreview, AppError> {
        Ok(self.build(&draft, false, false, None, None).await?.preview)
    }

    pub async fn apply(
        &self,
        request: ApplyLibraryApplicationRequest,
        cancellation: CancellationSignal,
    ) -> Result<LibraryApplicationResponse, AppError> {
        let built = self.build(&request.draft, true, false, None, None).await?;
        validate_exact_preview(&request.expected_token, &built.preview.token)?;
        let mut reasons = reconciliation_reasons(
            &built.record.current,
            &built.preview.target,
            &built.record.checkpoint,
            &built.target_members,
        );
        if reasons.is_empty() && built.plan.units.is_empty() {
            return Ok(LibraryApplicationResponse {
                application: self.read(built.context).await?,
                units: Vec::new(),
            });
        }
        if built.record.current == built.preview.target && !built.plan.units.is_empty() {
            reasons.push(ReconciliationReason::ReapplyRequested);
        }
        let mut pending_record = built.record.clone();
        pending_record.pending = Some(PendingReconciliation {
            reconciliation_id: uuid::Uuid::new_v4().simple().to_string(),
            attention: ReconciliationAttention::Pending,
            reasons,
            before_application: built.record.current.clone(),
            target_application: built.preview.target.clone(),
            recognized_members: reconciliation_members(
                &built.record.checkpoint,
                built.record.pending.as_ref(),
                &built.target_members,
            ),
            affected_members: changed_member_identities(
                &built.record.checkpoint.members,
                &built.target_members,
            ),
            target_members: built.target_members.clone(),
        });
        let pending = self
            .repository
            .save_application_if(&built.observed, &pending_record)
            .await?;
        self.execute(built, pending, cancellation).await
    }

    pub async fn retry_pending(
        &self,
        context: SkillLocationRef,
        cancellation: CancellationSignal,
    ) -> Result<LibraryApplicationResponse, AppError> {
        let observed = self.repository.load_application(&context).await?;
        if observed.pending.is_none() {
            return Err(AppError::Validation {
                field: Some("context".to_string()),
                message: "the Scope has no pending Skill Library operation".to_string(),
            });
        }
        self.resume_observed(context, observed, cancellation).await
    }

    pub async fn resume(
        &self,
        context: SkillLocationRef,
        cancellation: CancellationSignal,
    ) -> Result<LibraryApplicationResponse, AppError> {
        let observed = self.repository.load_application(&context).await?;
        self.resume_observed(context, observed, cancellation).await
    }

    pub(crate) async fn plan_resume(
        &self,
        context: SkillLocationRef,
    ) -> Result<LibraryApplicationScopePlan, AppError> {
        let observed = self.repository.load_application(&context).await?;
        let catalog = self.repository.load_catalog(&context).await?;
        if !reconciliation_required(&observed.record, &catalog)? {
            return Ok(LibraryApplicationScopePlan {
                context,
                entries: Vec::new(),
                preview: None,
            });
        }
        let target_application = observed
            .pending
            .as_ref()
            .map(|pending| pending.target_application.clone())
            .unwrap_or_else(|| observed.current.clone());
        let draft = LibraryApplicationDraft {
            context: context.clone(),
            ordered_library_ids: target_application.ordered_library_ids,
            selected_agent_ids: target_application.selected_agent_ids,
        };
        let built = self.build(&draft, true, true, Some(observed), None).await?;
        let entries = built
            .plan
            .units
            .iter()
            .flat_map(|unit| unit.primary_entry.iter().chain(&unit.additional_entries))
            .map(|entry| (entry.key.clone(), entry.action.clone()))
            .collect();
        Ok(LibraryApplicationScopePlan {
            context,
            entries,
            preview: Some(built.preview),
        })
    }

    pub(crate) async fn plan_resume_with_catalog(
        &self,
        context: SkillLocationRef,
        catalog: LibraryCatalog,
    ) -> Result<LibraryApplicationScopePlan, AppError> {
        let observed = self.repository.load_application(&context).await?;
        if !reconciliation_required(&observed.record, &catalog)? {
            return Ok(LibraryApplicationScopePlan {
                context,
                entries: Vec::new(),
                preview: None,
            });
        }
        let target_application = observed
            .pending
            .as_ref()
            .map(|pending| pending.target_application.clone())
            .unwrap_or_else(|| observed.current.clone());
        let draft = LibraryApplicationDraft {
            context: context.clone(),
            ordered_library_ids: target_application.ordered_library_ids,
            selected_agent_ids: target_application.selected_agent_ids,
        };
        let built = self
            .build(&draft, true, true, Some(observed), Some(catalog))
            .await?;
        let entries = built
            .plan
            .units
            .iter()
            .flat_map(|unit| unit.primary_entry.iter().chain(&unit.additional_entries))
            .map(|entry| (entry.key.clone(), entry.action.clone()))
            .collect();
        Ok(LibraryApplicationScopePlan {
            context,
            entries,
            preview: Some(built.preview),
        })
    }

    pub(crate) async fn record_reconciliation_attention(
        &self,
        context: SkillLocationRef,
        attention: ReconciliationAttention,
    ) -> Result<(), AppError> {
        let observed = self.repository.load_application(&context).await?;
        let catalog = self.repository.load_catalog(&context).await?;
        let member_index =
            LibraryCatalogMemberIndex::build(&catalog).map_err(library_member_index_error)?;
        let target_application = observed
            .record
            .pending
            .as_ref()
            .map(|pending| pending.target_application.clone())
            .unwrap_or_else(|| observed.record.current.clone());
        let target_members = member_index
            .members_for(&target_application.ordered_library_ids)
            .map_err(library_member_index_error)?
            .into_values()
            .flatten()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        let reasons = reconciliation_reasons(
            &observed.record.current,
            &target_application,
            &observed.record.checkpoint,
            &target_members,
        );
        if reasons.is_empty() && observed.record.pending.is_none() {
            return Ok(());
        }
        let recognized = reconciliation_members(
            &observed.record.checkpoint,
            observed.record.pending.as_ref(),
            &target_members,
        );
        let mut pending = match observed.record.pending.clone() {
            Some(pending) => {
                pending.merged_with(reasons, target_application, recognized, target_members)
            }
            None => PendingReconciliation {
                reconciliation_id: uuid::Uuid::new_v4().simple().to_string(),
                attention: ReconciliationAttention::Pending,
                reasons,
                before_application: observed.record.current.clone(),
                target_application,
                recognized_members: recognized,
                affected_members: changed_member_identities(
                    &observed.record.checkpoint.members,
                    &target_members,
                ),
                target_members,
            },
        };
        pending.attention = pending.attention.max(attention);
        let mut record = observed.record.clone();
        record.pending = Some(pending);
        self.repository
            .save_application_if(&observed, &record)
            .await?;
        Ok(())
    }

    async fn resume_observed(
        &self,
        context: SkillLocationRef,
        observed: VersionedApplicationRecord,
        cancellation: CancellationSignal,
    ) -> Result<LibraryApplicationResponse, AppError> {
        let catalog = self.repository.load_catalog(&context).await?;
        if !reconciliation_required(&observed.record, &catalog)? {
            if observed.pending.is_some() {
                let mut verified = observed.record.clone();
                verified.pending = None;
                self.repository
                    .save_application_if(&observed, &verified)
                    .await?;
            }
            return Ok(LibraryApplicationResponse {
                application: self.read(context).await?,
                units: Vec::new(),
            });
        }
        let target_application = observed
            .pending
            .as_ref()
            .map(|pending| pending.target_application.clone())
            .unwrap_or_else(|| observed.current.clone());
        let draft = LibraryApplicationDraft {
            context,
            ordered_library_ids: target_application.ordered_library_ids.clone(),
            selected_agent_ids: target_application.selected_agent_ids.clone(),
        };
        let built = self.build(&draft, true, true, Some(observed), None).await?;
        let reasons = reconciliation_reasons(
            &built.record.current,
            &built.preview.target,
            &built.record.checkpoint,
            &built.target_members,
        );
        if reasons.is_empty() && built.record.pending.is_none() {
            return Ok(LibraryApplicationResponse {
                application: self.read(built.context).await?,
                units: Vec::new(),
            });
        }
        let recognized = reconciliation_members(
            &built.record.checkpoint,
            built.record.pending.as_ref(),
            &built.target_members,
        );
        let pending = match built.record.pending.clone() {
            Some(pending) => pending.merged_with(
                reasons,
                built.preview.target.clone(),
                recognized,
                built.target_members.clone(),
            ),
            None => PendingReconciliation {
                reconciliation_id: uuid::Uuid::new_v4().simple().to_string(),
                attention: ReconciliationAttention::Pending,
                reasons,
                before_application: built.record.current.clone(),
                target_application: built.preview.target.clone(),
                recognized_members: recognized,
                affected_members: changed_member_identities(
                    &built.record.checkpoint.members,
                    &built.target_members,
                ),
                target_members: built.target_members.clone(),
            },
        };
        let mut pending_record = built.record.clone();
        pending_record.pending = Some(pending);
        let pending = self
            .repository
            .save_application_if(&built.observed, &pending_record)
            .await?;
        self.execute(built, pending, cancellation).await
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
        if record.pending.is_some()
            || !record.current.ordered_library_ids.is_empty()
            || !record.current.selected_agent_ids.is_empty()
            || !record.checkpoint.members.is_empty()
        {
            return Err(AppError::MutationBusy);
        }
        self.repository.remove_application_if(&record).await
    }

    async fn execute(
        &self,
        built: BuiltLibraryApplication,
        pending: VersionedApplicationRecord,
        cancellation: CancellationSignal,
    ) -> Result<LibraryApplicationResponse, AppError> {
        let context = built.context.clone();
        let expected_unit_count = built.plan.units.len();
        let units = self.executor.execute(built.plan, cancellation).await;
        let completed = library_execution_completed(expected_unit_count, &units);
        let final_record = if completed {
            LibraryApplicationRecord {
                current: built.preview.target,
                checkpoint: ReconciliationCheckpoint {
                    members: built.target_members.clone(),
                },
                pending: None,
                ..pending.record.clone()
            }
        } else {
            let mut record = pending.record.clone();
            if let Some(pending) = &mut record.pending {
                pending.attention = execution_attention(&units);
            }
            record
        };
        self.repository
            .save_application_if(&pending, &final_record)
            .await?;
        let catalog = self.repository.load_catalog(&context).await?;
        let recovery = self
            .recovery
            .status(&context)
            .await
            .unwrap_or(ScopeRecoveryState::Unverified);
        Ok(LibraryApplicationResponse {
            application: summary(&final_record, &catalog, recovery)?,
            units,
        })
    }

    async fn build(
        &self,
        draft: &LibraryApplicationDraft,
        include_plan: bool,
        allow_pending: bool,
        observed: Option<VersionedApplicationRecord>,
        catalog: Option<LibraryCatalog>,
    ) -> Result<BuiltLibraryApplication, AppError> {
        let observed = match observed {
            Some(observed) => observed,
            None => self.repository.load_application(&draft.context).await?,
        };
        let record = observed.record.clone();
        if record.pending.is_some() && !allow_pending {
            return Err(AppError::MutationBusy);
        }
        let observed_catalog = self.repository.load_catalog(&draft.context).await?;
        let catalog = catalog.unwrap_or_else(|| observed_catalog.clone());
        let current_member_index = LibraryCatalogMemberIndex::build(&observed_catalog)
            .map_err(library_member_index_error)?;
        let target_member_index =
            LibraryCatalogMemberIndex::build(&catalog).map_err(library_member_index_error)?;
        let facts = self.facts.snapshot(&draft.context).await?;
        let persisted_agent_ids = persisted_agent_ids(&record);
        let agent_options = resolve_library_agent_options(
            &draft.context,
            &facts,
            &self.targets,
            &persisted_agent_ids,
        )
        .await?;
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
                &persisted_agent_ids,
            )?,
        };
        let current_agent_ids = record.current.selected_agent_ids.clone();
        if let Some(pending) = &record.pending {
            if pending.target_application != target {
                return Err(AppError::StaleContext);
            }
        }
        let current_members = current_member_index
            .members_for(&record.current.ordered_library_ids)
            .map_err(library_member_index_error)?;
        let current_member_identities = current_members
            .values()
            .flatten()
            .cloned()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        let target_members_by_directory = target_member_index
            .members_for(&target.ordered_library_ids)
            .map_err(library_member_index_error)?;
        let target_members = target_members_by_directory
            .values()
            .flatten()
            .cloned()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        let recognized_members =
            reconciliation_members(&record.checkpoint, record.pending.as_ref(), &target_members);
        let recognized = merge_catalog_member_maps(
            current_member_index
                .recognized_members(
                    &recognized_members
                        .iter()
                        .filter(|member| current_member_index.contains_identity(member))
                        .cloned()
                        .collect::<Vec<_>>(),
                )
                .map_err(library_member_index_error)?,
            target_member_index
                .recognized_members(
                    &recognized_members
                        .iter()
                        .filter(|member| target_member_index.contains_identity(member))
                        .cloned()
                        .collect::<Vec<_>>(),
                )
                .map_err(library_member_index_error)?,
        );
        let before_recognized = current_member_index
            .recognized_members(&reconciliation_members(
                &record.checkpoint,
                record.pending.as_ref(),
                &current_member_identities,
            ))
            .map_err(library_member_index_error)?;
        let mut groups = merge_library_skill_groups(
            current_members,
            target_members_by_directory,
            recognized,
            before_recognized,
        );
        let confirmed_reapply = record.pending.as_ref().is_some_and(|pending| {
            pending
                .reasons
                .contains(&ReconciliationReason::ReapplyRequested)
        });
        if allow_pending && !confirmed_reapply && record.current == target {
            let changed = membership_changed_skill_directories(
                &record.checkpoint,
                record.pending.as_ref(),
                &target_members,
            )?;
            groups.retain(|group| changed.contains(&group.directory_name));
        }
        let candidate_members = library_group_members(&groups);
        let candidate_index = ResolvedLibraryCandidateIndex::load(
            self.repository.as_ref(),
            &self.targets,
            &draft.context,
            &candidate_members,
        )
        .await?;
        let current_selected = current_agent_ids.iter().collect::<BTreeSet<_>>();
        let eligible_option_ids = agent_options
            .options
            .iter()
            .map(|option| &option.option_id)
            .collect::<BTreeSet<_>>();
        let mut prepared_groups = Vec::with_capacity(groups.len());
        let mut destinations = Vec::new();
        for group in groups {
            let current_candidates = candidate_index.candidates_for(&group.current_members)?;
            let target_candidates = candidate_index.candidates_for(&group.target_members)?;
            let recognized_candidates =
                candidate_index.candidates_for(&group.recognized_members)?;
            let before_recognized_candidates =
                candidate_index.candidates_for(&group.before_recognized_members)?;
            let before_applied = !before_recognized_candidates.is_empty();
            let current_candidate_set = LibraryCandidateSet::for_skill(
                &draft.context.environment,
                &group.directory_name,
                before_recognized_candidates,
                current_candidates,
            )
            .map_err(|_| AppError::StaleContext)?;
            let target_candidate_set = LibraryCandidateSet::for_skill(
                &draft.context.environment,
                &group.directory_name,
                recognized_candidates,
                target_candidates,
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
            destinations.extend(
                logical_targets
                    .iter()
                    .map(|logical| logical.destination.clone()),
            );
            prepared_groups.push((
                group,
                current_candidate_set,
                target_candidate_set,
                before_applied,
                logical_targets,
            ));
        }
        let target_facts = if destinations.is_empty() {
            Vec::new()
        } else {
            self.targets
                .resolve(&draft.context, &destinations, None)
                .await?
        };
        if target_facts.len() != destinations.len() {
            return Err(AppError::StaleTarget);
        }
        let mut target_facts = target_facts.into_iter();
        let mut units = Vec::new();
        let mut direct_skill_names = BTreeSet::new();
        let mut directory_change_skill_names = BTreeSet::new();
        let mut added_skill_names = BTreeSet::new();
        let mut removed_skill_names = BTreeSet::new();
        let mut switched_skill_names = BTreeSet::new();
        let mut observed_targets = Vec::new();
        for (group, current_candidate_set, target_candidate_set, before_applied, logical_targets) in
            prepared_groups
        {
            let group_facts = target_facts
                .by_ref()
                .take(logical_targets.len())
                .collect::<Vec<_>>();
            ensure_library_link_targets_supported(
                logical_targets
                    .iter()
                    .zip(&group_facts)
                    .filter_map(|(logical, fact)| logical.library_link_target.then_some(fact)),
            )?;
            let mut resolved = BTreeMap::new();
            let mut legacy = Vec::new();
            for (logical, fact) in logical_targets.into_iter().zip(group_facts) {
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
                before_applied,
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
            context: draft.context.clone(),
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
            observed,
            plan,
            target_members,
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

fn execution_attention(units: &[MutationUnitResult]) -> ReconciliationAttention {
    if units.iter().any(|unit| {
        unit.error.as_ref().is_some_and(|error| {
            matches!(
                error.code,
                crate::application::mutation::result::OperationErrorCode::EnvironmentUnavailable
                    | crate::application::mutation::result::OperationErrorCode::StorageUnsupported
            )
        })
    }) {
        ReconciliationAttention::Unverified
    } else {
        ReconciliationAttention::Pending
    }
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
        Box::pin(async move { LibraryApplicationModule::resume(self, context, cancellation).await })
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
    recognized_members: Vec<LibraryCatalogMember>,
    before_recognized_members: Vec<LibraryCatalogMember>,
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
    saved_agent_ids: &BTreeSet<AgentId>,
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
    let mut selection = placement_map.selection_snapshot().clone();
    selection.baseline_selected_option_ids = selection
        .install_options
        .iter()
        .filter(|option| {
            option
                .agent_ids
                .iter()
                .all(|agent_id| saved_agent_ids.contains(agent_id))
        })
        .map(|option| option.id.clone())
        .collect();
    let represented_saved_agent_ids = selection
        .install_options
        .iter()
        .filter(|option| {
            option
                .agent_ids
                .iter()
                .all(|agent_id| saved_agent_ids.contains(agent_id))
        })
        .flat_map(|option| option.agent_ids.iter().cloned())
        .collect::<BTreeSet<_>>();
    let known_agent_ids = catalog
        .snapshot()
        .agents
        .iter()
        .map(|agent| agent.id.clone())
        .collect::<BTreeSet<_>>();
    selection.unavailable_explicit_agents = saved_agent_ids
        .iter()
        .filter(|agent_id| !represented_saved_agent_ids.contains(*agent_id))
        .map(|agent_id| UnavailableAgentSelection {
            agent_id: agent_id.as_str().to_string(),
            reason: if known_agent_ids.contains(agent_id) {
                UnavailableAgentSelectionReason::OptionUnavailable
            } else {
                UnavailableAgentSelectionReason::DefinitionMissing
            },
        })
        .collect();
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
    for agent_id in saved_agent_ids {
        let Some(resolved) = facts.agent_runtime.agents.get(agent_id) else {
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
                legacy_candidates.push((
                    root,
                    agent_id.clone(),
                    resolved.definition.display_name.clone(),
                ));
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
    persisted: &BTreeSet<AgentId>,
) -> Result<Vec<AgentId>, AppError> {
    placements
        .validate_selection_with_persisted(requested, persisted)
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

fn persisted_agent_ids(record: &LibraryApplicationRecord) -> BTreeSet<AgentId> {
    record
        .current
        .selected_agent_ids
        .iter()
        .chain(record.pending.iter().flat_map(|pending| {
            pending
                .before_application
                .selected_agent_ids
                .iter()
                .chain(&pending.target_application.selected_agent_ids)
        }))
        .cloned()
        .collect()
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
    context: SkillLocationRef,
    preview: LibraryApplicationPreview,
    record: LibraryApplicationRecord,
    observed: VersionedApplicationRecord,
    plan: crate::application::mutation::plan::MutationPlan,
    target_members: Vec<LibraryMemberIdentity>,
}

fn reconciliation_reasons(
    current: &LibraryApplicationState,
    target: &LibraryApplicationState,
    checkpoint: &ReconciliationCheckpoint,
    target_members: &[LibraryMemberIdentity],
) -> Vec<ReconciliationReason> {
    let mut reasons = Vec::new();
    if current != target {
        reasons.push(ReconciliationReason::ApplicationChanged);
    }
    if checkpoint.members != target_members {
        reasons.push(ReconciliationReason::MembershipChanged);
    }
    reasons
}

fn reconciliation_required(
    record: &LibraryApplicationRecord,
    catalog: &LibraryCatalog,
) -> Result<bool, AppError> {
    if record.pending.as_ref().is_some_and(|pending| {
        pending.target_application != record.current
            || pending
                .reasons
                .iter()
                .any(|reason| *reason != ReconciliationReason::VerificationRequired)
    }) {
        return Ok(true);
    }
    let target = record
        .pending
        .as_ref()
        .map(|pending| &pending.target_application)
        .unwrap_or(&record.current);
    let target_members = LibraryCatalogMemberIndex::build(catalog)
        .map_err(library_member_index_error)?
        .members_for(&target.ordered_library_ids)
        .map_err(library_member_index_error)?
        .into_values()
        .flatten()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    Ok(record.checkpoint.members != target_members)
}

fn membership_changed_skill_directories(
    checkpoint: &ReconciliationCheckpoint,
    pending: Option<&PendingReconciliation>,
    target_members: &[LibraryMemberIdentity],
) -> Result<BTreeSet<SkillDirectoryName>, AppError> {
    let mut affected = changed_member_identities(&checkpoint.members, target_members);
    if let Some(pending) = pending {
        affected.extend(pending.affected_members.iter().cloned());
        affected.extend(changed_member_identities(
            &pending.target_members,
            target_members,
        ));
    }
    affected
        .into_iter()
        .map(|member| SkillDirectoryName::try_from(member.member_name.as_str()))
        .collect()
}

fn changed_member_identities(
    before: &[LibraryMemberIdentity],
    after: &[LibraryMemberIdentity],
) -> Vec<LibraryMemberIdentity> {
    let before = before.iter().cloned().collect::<BTreeSet<_>>();
    let after = after.iter().cloned().collect::<BTreeSet<_>>();
    before.symmetric_difference(&after).cloned().collect()
}

fn merge_library_skill_groups(
    current: BTreeMap<SkillDirectoryName, Vec<LibraryCatalogMember>>,
    target: BTreeMap<SkillDirectoryName, Vec<LibraryCatalogMember>>,
    recognized: BTreeMap<SkillDirectoryName, Vec<LibraryCatalogMember>>,
    before_recognized: BTreeMap<SkillDirectoryName, Vec<LibraryCatalogMember>>,
) -> Vec<LibrarySkillGroup> {
    let directory_names = current
        .keys()
        .chain(target.keys())
        .chain(recognized.keys())
        .chain(before_recognized.keys())
        .cloned()
        .collect::<BTreeSet<_>>();
    directory_names
        .into_iter()
        .map(|directory_name| LibrarySkillGroup {
            current_members: current.get(&directory_name).cloned().unwrap_or_default(),
            target_members: target.get(&directory_name).cloned().unwrap_or_default(),
            recognized_members: recognized.get(&directory_name).cloned().unwrap_or_default(),
            before_recognized_members: before_recognized
                .get(&directory_name)
                .cloned()
                .unwrap_or_default(),
            directory_name,
        })
        .collect()
}

fn merge_catalog_member_maps(
    mut left: BTreeMap<SkillDirectoryName, Vec<LibraryCatalogMember>>,
    right: BTreeMap<SkillDirectoryName, Vec<LibraryCatalogMember>>,
) -> BTreeMap<SkillDirectoryName, Vec<LibraryCatalogMember>> {
    for (directory, members) in right {
        let combined = left.entry(directory).or_default();
        combined.extend(members);
        combined.sort();
        combined.dedup();
    }
    left
}

fn library_group_members(groups: &[LibrarySkillGroup]) -> Vec<LibraryCatalogMember> {
    groups
        .iter()
        .flat_map(|group| group.recognized_members.iter())
        .cloned()
        .collect()
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
        let _ = library;
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
    recovery: ScopeRecoveryState,
) -> Result<LibraryApplicationSummary, AppError> {
    let member_index =
        LibraryCatalogMemberIndex::build(catalog).map_err(library_member_index_error)?;
    let target_ids = record
        .pending
        .as_ref()
        .map(|pending| pending.target_application.ordered_library_ids.as_slice())
        .unwrap_or(&record.current.ordered_library_ids);
    let target_members = member_index
        .members_for(target_ids)
        .map_err(library_member_index_error)?
        .into_values()
        .flatten()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
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
    let sync_state = application_sync_state(record, &target_members, recovery);
    Ok(LibraryApplicationSummary {
        ordered_libraries,
        selected_agent_ids: record.current.selected_agent_ids.clone(),
        pending: sync_state != LibraryApplicationSyncState::Synced,
        sync_state,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::Mutex;

    use crate::application::install::InstallFuture;
    use crate::application::mutation::executor::MutationFuture;
    use crate::application::mutation::plan::{MutationPlan, PreparedEntryAction, RuntimeRevisions};
    use crate::application::skill_libraries::{
        LibrarySkillRecord, LibrarySkillSourceRecord, RetiredLibrarySkillRecord,
        SkillLibraryRecord, LIBRARY_SCHEMA_VERSION,
    };
    use crate::core::agent_definition::{
        AgentAdapter, AgentDefinition, AgentSource, DetectionSpec, LegacyMigrationTarget,
        LegacyPath, PathSpec, ScopeDefinition,
    };
    use crate::core::lossless_lock::{LockSchema, LosslessLockDocument};
    use crate::environment::agent_environment::{
        AgentRuntimeSnapshot, DetectionState, DirectoryPresenceState, ResolvedAgent,
        ResolvedAgentScope, ResolvedPathPresence,
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
    const TEST_LEGACY_ROOT: &str = "/agents/legacy/skills";
    const TEST_LIBRARY_ROOT: &str = "/libraries/lib-one/skills";

    #[derive(Clone)]
    struct FixedFacts(ScopePlanningSnapshot);

    struct FixedRecoveryStatus(ScopeRecoveryState);

    impl LibraryApplicationRecoveryStatus for FixedRecoveryStatus {
        fn status<'a>(
            &'a self,
            _context: &'a SkillLocationRef,
        ) -> LibraryApplicationFuture<'a, Result<ScopeRecoveryState, AppError>> {
            Box::pin(async move { Ok(self.0) })
        }
    }

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
        fail_next_checkpoint: AtomicBool,
    }

    impl ApplicationRegistry for MemoryApplicationRepository {
        fn load_application<'a>(
            &'a self,
            context: &'a SkillLocationRef,
        ) -> LibraryApplicationFuture<'a, Result<VersionedApplicationRecord, AppError>> {
            Box::pin(async move {
                Ok(VersionedApplicationRecord::in_memory(
                    context.clone(),
                    self.record.lock().unwrap().clone(),
                ))
            })
        }

        fn save_application_if<'a>(
            &'a self,
            observed: &'a VersionedApplicationRecord,
            record: &'a LibraryApplicationRecord,
        ) -> LibraryApplicationFuture<'a, Result<VersionedApplicationRecord, AppError>> {
            Box::pin(async move {
                if record.pending.is_none()
                    && self.fail_next_checkpoint.swap(false, Ordering::SeqCst)
                {
                    return Err(AppError::Io {
                        message: "injected checkpoint commit failure".to_string(),
                    });
                }
                *self.record.lock().unwrap() = record.clone();
                Ok(VersionedApplicationRecord::in_memory(
                    observed.context.clone(),
                    record.clone(),
                ))
            })
        }

        fn enumerate<'a>(
            &'a self,
            environment: &'a EnvironmentRef,
        ) -> LibraryApplicationFuture<'a, Result<ApplicationInventory, AppError>> {
            Box::pin(async move {
                let context = SkillLocationRef {
                    environment: environment.clone(),
                    scope: SkillLocation::Global,
                };
                Ok(ApplicationInventory {
                    records: vec![VersionedApplicationRecord::in_memory(
                        context,
                        self.record.lock().unwrap().clone(),
                    )],
                    problems: Vec::new(),
                    complete: true,
                })
            })
        }
    }

    impl LibraryApplicationResources for MemoryApplicationRepository {
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

        fn remove_application_if<'a>(
            &'a self,
            _observed: &'a VersionedApplicationRecord,
        ) -> LibraryApplicationFuture<'a, Result<(), AppError>> {
            Box::pin(async move { Ok(()) })
        }
    }

    #[derive(Clone)]
    struct FixedTargets {
        primary_entry_kind: TargetEntryKind,
        agent_entry_kind: TargetEntryKind,
        agent_link_target: Option<String>,
        resolve_calls: Arc<TargetResolveCalls>,
    }

    #[derive(Default)]
    struct TargetResolveCalls {
        candidates: AtomicUsize,
        placements: AtomicUsize,
    }

    impl TargetFactResolver for FixedTargets {
        fn resolve<'a>(
            &'a self,
            _context: &'a SkillLocationRef,
            logical_destinations: &'a [ResourceLocator],
            _cancellation: Option<CancellationSignal>,
        ) -> TargetFactFuture<'a, Result<Vec<ResolvedTargetFact>, AppError>> {
            Box::pin(async move {
                if !logical_destinations.is_empty()
                    && logical_destinations
                        .iter()
                        .all(|destination| destination.native_path.starts_with("/libraries/"))
                {
                    self.resolve_calls.candidates.fetch_add(1, Ordering::SeqCst);
                } else if !logical_destinations.is_empty()
                    && logical_destinations.iter().all(|destination| {
                        let parent = std::path::Path::new(&destination.native_path).parent();
                        parent == Some(std::path::Path::new(TEST_SKILL_ROOT))
                            || parent == Some(std::path::Path::new(TEST_AGENT_ROOT))
                    })
                {
                    self.resolve_calls.placements.fetch_add(1, Ordering::SeqCst);
                }
                Ok(logical_destinations
                    .iter()
                    .map(|destination| {
                        let path = std::path::Path::new(&destination.native_path);
                        let skill_name = path
                            .file_name()
                            .and_then(|name| name.to_str())
                            .unwrap_or("target");
                        let parent = path.parent();
                        let (name, entry_kind, link_target) = if path
                            .starts_with(std::path::Path::new("/libraries"))
                        {
                            (
                                format!(
                                    "library-{}",
                                    crate::application::mutation::plan::stable_digest(destination)
                                        .unwrap()
                                ),
                                TargetEntryKind::Directory,
                                None,
                            )
                        } else if destination.native_path == TEST_AGENT_ROOT {
                            ("agent-root".to_string(), TargetEntryKind::Directory, None)
                        } else if parent == Some(std::path::Path::new(TEST_AGENT_ROOT))
                            || parent == Some(std::path::Path::new(TEST_LEGACY_ROOT))
                        {
                            (
                                format!("agent-skill-{skill_name}"),
                                self.agent_entry_kind,
                                self.agent_link_target.clone(),
                            )
                        } else {
                            (
                                format!("canonical-skill-{skill_name}"),
                                self.primary_entry_kind,
                                None,
                            )
                        };
                        let link_target_identity = link_target.as_deref().and_then(|raw| {
                            crate::environment::planning::resolve_link_target_identity(
                                destination,
                                raw,
                            )
                        });
                        ResolvedTargetFact {
                            key: physical_key(&name),
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

    #[derive(Clone)]
    struct RecoveryExecutor;

    impl MutationPlanExecutor for RecoveryExecutor {
        fn execute<'a>(
            &'a self,
            plan: MutationPlan,
            _cancellation: CancellationSignal,
        ) -> MutationFuture<'a, Vec<MutationUnitResult>> {
            Box::pin(async move {
                plan.units
                    .iter()
                    .map(|unit| {
                        MutationUnitResult::recovery_required(
                            unit.id.clone(),
                            unit.skill_name.clone(),
                            unit.target.clone(),
                            crate::application::mutation::result::ErrorReport::recovery_required(
                                crate::application::mutation::result::RecoveryResourceId::parse(
                                    "recovery-1",
                                )
                                .unwrap(),
                                "restore required",
                            ),
                        )
                    })
                    .collect()
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
        let (module, executor, draft, _, _) = application_fixture_with(
            TargetEntryKind::Missing,
            agent_entry_kind,
            agent_link_target,
            vec![SkillLibraryRecord {
                id: LibraryId::parse("lib-one"),
                name: "Library One".to_string(),
                skills: vec![skill("demo")],
                retired_skills: Vec::new(),
                extra: serde_json::Map::new(),
            }],
            Vec::new(),
            vec![LibraryId::parse("lib-one")],
        );
        (module, executor, draft)
    }

    type ApplicationFixture = (
        LibraryApplicationModule<FixedFacts, FixedTargets, RecordingExecutor>,
        RecordingExecutor,
        LibraryApplicationDraft,
        Arc<TargetResolveCalls>,
        Arc<MemoryApplicationRepository>,
    );

    fn application_fixture_with(
        primary_entry_kind: TargetEntryKind,
        agent_entry_kind: TargetEntryKind,
        agent_link_target: Option<&str>,
        libraries: Vec<SkillLibraryRecord>,
        current_library_ids: Vec<LibraryId>,
        target_library_ids: Vec<LibraryId>,
    ) -> ApplicationFixture {
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
        let current_ids = current_library_ids.iter().collect::<BTreeSet<_>>();
        let checkpoint = libraries
            .iter()
            .filter(|library| current_ids.contains(&library.id))
            .flat_map(|library| {
                library
                    .skills
                    .iter()
                    .chain(library.retired_skills.iter().map(|retired| &retired.member))
                    .map(|member| LibraryMemberIdentity {
                        library_id: library.id.clone(),
                        member_name: member.name.clone(),
                    })
            })
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        let catalog = LibraryCatalog {
            schema_version: LIBRARY_SCHEMA_VERSION,
            libraries,
            extra: serde_json::Map::new(),
        };
        let repository = Arc::new(MemoryApplicationRepository {
            record: Mutex::new(LibraryApplicationRecord {
                current: LibraryApplicationState {
                    ordered_library_ids: current_library_ids,
                    selected_agent_ids: current_agent_ids.into_iter().collect(),
                },
                checkpoint: ReconciliationCheckpoint {
                    members: checkpoint,
                },
                ..LibraryApplicationRecord::empty()
            }),
            catalog: Mutex::new(catalog),
            fail_next_checkpoint: AtomicBool::new(false),
        });
        let executor = RecordingExecutor::default();
        let resolve_calls = Arc::new(TargetResolveCalls::default());
        let module = LibraryApplicationModule::new(
            repository.clone(),
            facts,
            FixedTargets {
                primary_entry_kind,
                agent_entry_kind,
                agent_link_target: agent_link_target.map(str::to_string),
                resolve_calls: Arc::clone(&resolve_calls),
            },
            executor.clone(),
        );
        let draft = LibraryApplicationDraft {
            context,
            ordered_library_ids: target_library_ids,
            selected_agent_ids: vec![agent_id],
        };
        (module, executor, draft, resolve_calls, repository)
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
        let ids = |values: &[&str]| values.iter().map(|id| LibraryId::parse(*id)).collect();
        LibraryApplicationRecord {
            schema_version: LIBRARY_APPLICATION_SCHEMA_VERSION,
            current: LibraryApplicationState {
                ordered_library_ids: ids(current),
                selected_agent_ids: Vec::new(),
            },
            checkpoint: Default::default(),
            pending: pending.map(|(before, target)| PendingReconciliation {
                reconciliation_id: "reconciliation".to_string(),
                attention: ReconciliationAttention::Pending,
                reasons: vec![ReconciliationReason::ApplicationChanged],
                before_application: LibraryApplicationState {
                    ordered_library_ids: ids(before),
                    selected_agent_ids: Vec::new(),
                },
                target_application: LibraryApplicationState {
                    ordered_library_ids: ids(target),
                    selected_agent_ids: Vec::new(),
                },
                recognized_members: Vec::new(),
                affected_members: Vec::new(),
                target_members: Vec::new(),
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
    fn a_library_leaving_in_a_pending_still_counts_as_locked() {
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
    fn empty_application_uses_only_the_final_schema_fields() {
        let value = serde_json::to_value(LibraryApplicationRecord::empty()).unwrap();

        assert_eq!(value["schemaVersion"], LIBRARY_APPLICATION_SCHEMA_VERSION);
        assert_eq!(value["current"]["orderedLibraryIds"], serde_json::json!([]));
        assert_eq!(value["checkpoint"]["members"], serde_json::json!([]));
        assert!(value["pending"].is_null());
        assert!(value.get("pendingOperation").is_none());
    }

    #[test]
    fn sync_state_uses_recovery_unverified_pending_synced_priority() {
        let mut record = record_with(&["library-a"], Some((&["library-a"], &["library-a"])));
        record.pending.as_mut().unwrap().attention = ReconciliationAttention::Unverified;
        let catalog = LibraryCatalog {
            schema_version: LIBRARY_SCHEMA_VERSION,
            libraries: vec![SkillLibraryRecord {
                id: LibraryId::parse("library-a"),
                name: "Library A".to_string(),
                skills: Vec::new(),
                retired_skills: Vec::new(),
                extra: serde_json::Map::new(),
            }],
            extra: serde_json::Map::new(),
        };

        assert_eq!(
            summary(&record, &catalog, ScopeRecoveryState::Required)
                .unwrap()
                .sync_state,
            LibraryApplicationSyncState::RecoveryRequired
        );
        assert_eq!(
            summary(&record, &catalog, ScopeRecoveryState::Clear)
                .unwrap()
                .sync_state,
            LibraryApplicationSyncState::Unverified
        );
    }

    #[test]
    fn recovery_facts_override_persisted_pending_attention() {
        let record = record_with(&["library-a"], Some((&["library-a"], &["library-a"])));

        assert_eq!(
            application_sync_state(&record, &[], ScopeRecoveryState::Required),
            LibraryApplicationSyncState::RecoveryRequired
        );
    }

    #[tokio::test]
    async fn application_read_uses_the_recovery_status_source() {
        let (module, _executor, draft) = application_fixture(TargetEntryKind::Missing, None);
        let LibraryApplicationModule {
            repository,
            facts,
            targets,
            executor,
            ..
        } = module;
        let module = LibraryApplicationModule::with_recovery_status(
            repository,
            facts,
            targets,
            executor,
            Arc::new(FixedRecoveryStatus(ScopeRecoveryState::Required)),
        );

        assert_eq!(
            module.read(draft.context).await.unwrap().sync_state,
            LibraryApplicationSyncState::RecoveryRequired
        );
    }

    #[test]
    fn summary_reports_membership_drift_without_a_pending_record() {
        let library_id = LibraryId::parse("library-one");
        let record = LibraryApplicationRecord {
            current: LibraryApplicationState {
                ordered_library_ids: vec![library_id.clone()],
                selected_agent_ids: Vec::new(),
            },
            ..LibraryApplicationRecord::empty()
        };
        let catalog = LibraryCatalog {
            schema_version: LIBRARY_SCHEMA_VERSION,
            libraries: vec![SkillLibraryRecord {
                id: library_id,
                name: "Library One".to_string(),
                skills: vec![skill("demo")],
                retired_skills: Vec::new(),
                extra: serde_json::Map::new(),
            }],
            extra: serde_json::Map::new(),
        };

        assert!(
            summary(&record, &catalog, ScopeRecoveryState::Clear)
                .unwrap()
                .pending
        );
    }

    #[test]
    fn resolves_members_and_keeps_an_empty_library_selectable() {
        let filled_id = LibraryId::parse("filled");
        let empty_id = LibraryId::parse("empty");
        let catalog = LibraryCatalog {
            schema_version: LIBRARY_SCHEMA_VERSION,
            libraries: vec![
                SkillLibraryRecord {
                    id: filled_id.clone(),
                    name: "Backend".to_string(),
                    skills: vec![skill("api-design")],
                    retired_skills: Vec::new(),
                    extra: serde_json::Map::new(),
                },
                SkillLibraryRecord {
                    id: empty_id.clone(),
                    name: "Empty".to_string(),
                    skills: Vec::new(),
                    retired_skills: Vec::new(),
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
        assert_eq!(
            validated_library_ids(&catalog, std::slice::from_ref(&empty_id)).unwrap(),
            vec![empty_id]
        );
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
                    retired_skills: Vec::new(),
                    extra: serde_json::Map::new(),
                },
                SkillLibraryRecord {
                    id: second.clone(),
                    name: "Second".to_string(),
                    skills: vec![skill("review")],
                    retired_skills: Vec::new(),
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
                    retired_skills: Vec::new(),
                    extra: serde_json::Map::new(),
                },
                SkillLibraryRecord {
                    id: second.clone(),
                    name: "Second".to_string(),
                    skills: vec![skill("ce-review")],
                    retired_skills: Vec::new(),
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
        let (module, executor, draft, resolve_calls, _) = application_fixture_with(
            TargetEntryKind::Missing,
            TargetEntryKind::Missing,
            None,
            vec![
                SkillLibraryRecord {
                    id: first.clone(),
                    name: "First".to_string(),
                    skills: vec![skill("CE:Review")],
                    retired_skills: Vec::new(),
                    extra: serde_json::Map::new(),
                },
                SkillLibraryRecord {
                    id: second.clone(),
                    name: "Second".to_string(),
                    skills: vec![skill("ce-review")],
                    retired_skills: Vec::new(),
                    extra: serde_json::Map::new(),
                },
            ],
            vec![first],
            vec![second],
        );

        let preview = module.preview(draft.clone()).await.unwrap();
        assert_eq!(resolve_calls.candidates.load(Ordering::SeqCst), 1);
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

    #[tokio::test]
    async fn multi_skill_application_resolves_candidates_and_placements_once() {
        let library_id = LibraryId::parse("library-one");
        let (module, _executor, draft, resolve_calls, _) = application_fixture_with(
            TargetEntryKind::Missing,
            TargetEntryKind::Missing,
            None,
            vec![SkillLibraryRecord {
                id: library_id.clone(),
                name: "Library One".to_string(),
                skills: vec![skill("alpha"), skill("beta")],
                retired_skills: Vec::new(),
                extra: serde_json::Map::new(),
            }],
            Vec::new(),
            vec![library_id],
        );

        module.preview(draft).await.unwrap();

        assert_eq!(resolve_calls.candidates.load(Ordering::SeqCst), 1);
        assert_eq!(resolve_calls.placements.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn incomplete_execution_results_do_not_complete_an_application() {
        assert!(!library_execution_completed(1, &[]));
    }

    #[test]
    fn library_application_preview_evidence_changes_with_the_catalog() {
        let record = LibraryApplicationRecord::empty();
        let target = LibraryApplicationState::default();
        let catalog = LibraryCatalog {
            schema_version: LIBRARY_SCHEMA_VERSION,
            libraries: vec![SkillLibraryRecord {
                id: LibraryId::parse("lib-one"),
                name: "Library One".to_string(),
                skills: Vec::new(),
                retired_skills: Vec::new(),
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
    async fn reapplying_an_unchanged_application_repairs_a_missing_library_link() {
        let (module, executor, draft, _, _) = application_fixture_with(
            TargetEntryKind::Missing,
            TargetEntryKind::Symlink,
            Some("/libraries/lib-one/skills/demo"),
            vec![SkillLibraryRecord {
                id: LibraryId::parse("lib-one"),
                name: "Library One".to_string(),
                skills: vec![skill("demo")],
                retired_skills: Vec::new(),
                extra: serde_json::Map::new(),
            }],
            vec![LibraryId::parse("lib-one")],
            vec![LibraryId::parse("lib-one")],
        );

        let (plan, response) = applied_result(&module, &executor, draft).await;

        assert_eq!(response.units.len(), 1);
        assert!(plan.units[0]
            .primary_entry
            .as_ref()
            .is_some_and(|entry| { matches!(entry.action, PreparedEntryAction::Link { .. }) }));
    }

    #[tokio::test]
    async fn verification_only_resume_does_not_repair_an_unconfirmed_directory_change() {
        let (mut module, executor, draft, _, repository) = application_fixture_with(
            TargetEntryKind::Missing,
            TargetEntryKind::Symlink,
            Some("/libraries/lib-one/skills/demo"),
            vec![SkillLibraryRecord {
                id: LibraryId::parse("lib-one"),
                name: "Library One".to_string(),
                skills: vec![skill("demo")],
                retired_skills: Vec::new(),
                extra: serde_json::Map::new(),
            }],
            vec![LibraryId::parse("lib-one")],
            vec![LibraryId::parse("lib-one")],
        );
        {
            let mut record = repository.record.lock().unwrap();
            record.pending = Some(PendingReconciliation {
                reconciliation_id: "verification-only".to_string(),
                attention: ReconciliationAttention::Unverified,
                reasons: vec![ReconciliationReason::VerificationRequired],
                before_application: record.current.clone(),
                target_application: record.current.clone(),
                recognized_members: record.checkpoint.members.clone(),
                affected_members: Vec::new(),
                target_members: record.checkpoint.members.clone(),
            });
        }
        module.facts.0.agent_runtime.agents.clear();

        let response = module
            .resume(draft.context, CancellationSignal::default())
            .await
            .expect("resume verification");

        assert!(response.units.is_empty());
        assert!(!response.application.pending);
        assert!(executor.0.lock().unwrap().is_none());
    }

    #[tokio::test]
    async fn removing_an_application_allows_a_saved_agent_that_now_reads_standard() {
        let (mut module, executor, mut draft, _, _) = application_fixture_with(
            TargetEntryKind::Directory,
            TargetEntryKind::Symlink,
            Some("/libraries/lib-one/skills/demo"),
            vec![SkillLibraryRecord {
                id: LibraryId::parse("lib-one"),
                name: "Library One".to_string(),
                skills: vec![skill("demo")],
                retired_skills: Vec::new(),
                extra: serde_json::Map::new(),
            }],
            vec![LibraryId::parse("lib-one")],
            vec![LibraryId::parse("lib-one")],
        );
        let agent = module
            .facts
            .0
            .agent_runtime
            .agents
            .values_mut()
            .next()
            .expect("saved Agent");
        agent.definition.global.reads_standard = true;
        agent.global.reads_standard = true;
        agent.global.standard_path = Some(TEST_SKILL_ROOT.to_string());
        agent.global.standard_presence = Some(DirectoryPresenceState::Present);
        agent.global.read_paths.push(TEST_SKILL_ROOT.to_string());
        draft.ordered_library_ids.clear();
        draft.selected_agent_ids.clear();

        let options = module.agent_options(draft.context.clone()).await.unwrap();
        assert_eq!(
            options.selection.unavailable_explicit_agents[0].reason,
            UnavailableAgentSelectionReason::OptionUnavailable
        );

        let (plan, response) = applied_result(&module, &executor, draft).await;

        assert!(response.application.ordered_libraries.is_empty());
        assert!(response.application.selected_agent_ids.is_empty());
        assert!(plan.units[0]
            .additional_entries
            .iter()
            .any(|entry| entry.action == PreparedEntryAction::Remove));
        assert_eq!(
            plan.units[0]
                .primary_entry
                .as_ref()
                .map(|entry| &entry.action),
            Some(&PreparedEntryAction::Keep)
        );
    }

    #[tokio::test]
    async fn removing_a_saved_agent_cleans_its_declared_legacy_path_after_it_reads_standard() {
        let (mut module, executor, mut draft, _, _) = application_fixture_with(
            TargetEntryKind::Directory,
            TargetEntryKind::Symlink,
            Some("/libraries/lib-one/skills/demo"),
            vec![SkillLibraryRecord {
                id: LibraryId::parse("lib-one"),
                name: "Library One".to_string(),
                skills: vec![skill("demo")],
                retired_skills: Vec::new(),
                extra: serde_json::Map::new(),
            }],
            vec![LibraryId::parse("lib-one")],
            vec![LibraryId::parse("lib-one")],
        );
        let agent = module
            .facts
            .0
            .agent_runtime
            .agents
            .values_mut()
            .next()
            .unwrap();
        agent.definition.global.reads_standard = true;
        agent.definition.global.private_path = None;
        agent.definition.legacy_paths.push(LegacyPath {
            scope: LegacyPathScope::Global,
            path: PathSpec::home(".legacy-agent/skills"),
            behavior: LegacyPathBehavior::OfferMigration,
            migration_target: LegacyMigrationTarget::StandardCanonical,
        });
        agent.global.reads_standard = true;
        agent.global.standard_path = Some(TEST_SKILL_ROOT.to_string());
        agent.global.private_path = None;
        agent.global.standard_presence = Some(DirectoryPresenceState::Present);
        agent.global.private_presence = None;
        agent.global.read_paths = vec![TEST_SKILL_ROOT.to_string()];
        agent.global.legacy_paths.push(ResolvedPathPresence {
            path: Some(TEST_LEGACY_ROOT.to_string()),
            presence: DirectoryPresenceState::Present,
        });
        draft.ordered_library_ids.clear();
        draft.selected_agent_ids.clear();

        let (plan, _) = applied_result(&module, &executor, draft).await;

        let legacy_skill = locator(TEST_LEGACY_ROOT).join_child("demo");
        assert!(plan.units[0].additional_entries.iter().any(|entry| {
            entry.destination == legacy_skill && entry.action == PreparedEntryAction::Remove
        }));
    }

    #[tokio::test]
    async fn removing_an_application_drops_an_unknown_saved_agent_without_guessing_its_path() {
        let (mut module, executor, mut draft, _, _) = application_fixture_with(
            TargetEntryKind::Directory,
            TargetEntryKind::Symlink,
            Some("/libraries/lib-one/skills/demo"),
            vec![SkillLibraryRecord {
                id: LibraryId::parse("lib-one"),
                name: "Library One".to_string(),
                skills: vec![skill("demo")],
                retired_skills: Vec::new(),
                extra: serde_json::Map::new(),
            }],
            vec![LibraryId::parse("lib-one")],
            vec![LibraryId::parse("lib-one")],
        );
        module.facts.0.agent_runtime.agents.clear();
        draft.ordered_library_ids.clear();
        draft.selected_agent_ids.clear();

        let (plan, response) = applied_result(&module, &executor, draft).await;

        assert!(response.application.ordered_libraries.is_empty());
        assert!(response.application.selected_agent_ids.is_empty());
        assert!(plan.units[0].additional_entries.is_empty());
        assert_eq!(
            plan.units[0]
                .primary_entry
                .as_ref()
                .map(|entry| &entry.action),
            Some(&PreparedEntryAction::Keep)
        );
    }

    #[tokio::test]
    async fn an_unknown_saved_agent_can_be_carried_forward_without_becoming_a_new_selection() {
        let (mut module, _executor, draft, _, _) = application_fixture_with(
            TargetEntryKind::Directory,
            TargetEntryKind::Missing,
            None,
            vec![SkillLibraryRecord {
                id: LibraryId::parse("lib-one"),
                name: "Library One".to_string(),
                skills: vec![skill("demo")],
                retired_skills: Vec::new(),
                extra: serde_json::Map::new(),
            }],
            vec![LibraryId::parse("lib-one")],
            vec![LibraryId::parse("lib-one")],
        );
        module.facts.0.agent_runtime.agents.clear();

        let preview = module
            .preview(draft)
            .await
            .expect("an existing unavailable association can be retained");

        assert_eq!(preview.target.selected_agent_ids.len(), 1);
        assert_eq!(
            preview.target.selected_agent_ids[0].as_str(),
            "private-agent"
        );
    }

    #[tokio::test]
    async fn agent_options_use_a_saved_association_as_the_selection_baseline() {
        let (module, _executor, draft, _, _) = application_fixture_with(
            TargetEntryKind::Directory,
            TargetEntryKind::Symlink,
            Some("/libraries/lib-one/skills/demo"),
            vec![SkillLibraryRecord {
                id: LibraryId::parse("lib-one"),
                name: "Library One".to_string(),
                skills: vec![skill("demo")],
                retired_skills: Vec::new(),
                extra: serde_json::Map::new(),
            }],
            vec![LibraryId::parse("lib-one")],
            vec![LibraryId::parse("lib-one")],
        );

        let options = module.agent_options(draft.context).await.unwrap();
        let option = options.selection.install_options.first().unwrap();

        assert_eq!(
            options.selection.baseline_selected_option_ids,
            vec![option.id.clone()]
        );
    }

    #[tokio::test]
    async fn agent_options_expose_an_unknown_saved_association() {
        let (mut module, _executor, draft, _, _) = application_fixture_with(
            TargetEntryKind::Directory,
            TargetEntryKind::Missing,
            None,
            vec![SkillLibraryRecord {
                id: LibraryId::parse("lib-one"),
                name: "Library One".to_string(),
                skills: vec![skill("demo")],
                retired_skills: Vec::new(),
                extra: serde_json::Map::new(),
            }],
            vec![LibraryId::parse("lib-one")],
            vec![LibraryId::parse("lib-one")],
        );
        module.facts.0.agent_runtime.agents.clear();

        let options = module.agent_options(draft.context).await.unwrap();

        assert_eq!(options.selection.unavailable_explicit_agents.len(), 1);
        assert_eq!(
            options.selection.unavailable_explicit_agents[0].agent_id,
            "private-agent"
        );
        assert_eq!(
            options.selection.unavailable_explicit_agents[0].reason,
            crate::application::agent_selection::UnavailableAgentSelectionReason::DefinitionMissing
        );
    }

    #[tokio::test]
    async fn an_unknown_agent_cannot_be_added_as_a_new_association() {
        let (mut module, _executor, mut draft, _, _) = application_fixture_with(
            TargetEntryKind::Directory,
            TargetEntryKind::Missing,
            None,
            vec![SkillLibraryRecord {
                id: LibraryId::parse("lib-one"),
                name: "Library One".to_string(),
                skills: vec![skill("demo")],
                retired_skills: Vec::new(),
                extra: serde_json::Map::new(),
            }],
            vec![LibraryId::parse("lib-one")],
            vec![LibraryId::parse("lib-one")],
        );
        module.facts.0.agent_runtime.agents.clear();
        draft.selected_agent_ids = vec![AgentId::parse("new-agent").unwrap()];

        let error = module.preview(draft).await.unwrap_err();

        assert_eq!(
            error,
            AppError::InvalidAgent {
                agent: "new-agent".to_string()
            }
        );
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
            retired_skills: Vec::new(),
            extra: serde_json::Map::new(),
        };
        let (module, executor, draft, _, _) = application_fixture_with(
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
        let (module, _executor, mut draft, _, _) = application_fixture_with(
            TargetEntryKind::Directory,
            TargetEntryKind::BrokenLink,
            Some("/libraries/lib-one/skills/demo"),
            vec![SkillLibraryRecord {
                id: library_id.clone(),
                name: "Library One".to_string(),
                skills: vec![skill("demo")],
                retired_skills: Vec::new(),
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
        let (module, _executor, mut draft, _, _) = application_fixture_with(
            TargetEntryKind::Directory,
            TargetEntryKind::Symlink,
            Some("/libraries/lib-one/skills/demo"),
            vec![SkillLibraryRecord {
                id: library_id.clone(),
                name: "Library One".to_string(),
                skills: vec![skill("demo")],
                retired_skills: Vec::new(),
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

    #[tokio::test]
    async fn retired_checkpoint_member_is_recognized_for_link_removal_only() {
        let library_id = LibraryId::parse("lib-one");
        let (module, executor, mut draft, _, _) = application_fixture_with(
            TargetEntryKind::Missing,
            TargetEntryKind::Symlink,
            Some("/libraries/lib-one/skills/demo"),
            vec![SkillLibraryRecord {
                id: library_id.clone(),
                name: "Library One".to_string(),
                skills: Vec::new(),
                retired_skills: vec![RetiredLibrarySkillRecord {
                    retirement_id: crate::application::skill_libraries::RetirementId::parse(
                        "retirement-1",
                    ),
                    member: skill("demo"),
                    retired_at: "2026-09-06T00:00:00Z".to_string(),
                    extra: serde_json::Map::new(),
                }],
                extra: serde_json::Map::new(),
            }],
            vec![library_id],
            Vec::new(),
        );
        draft.selected_agent_ids.clear();

        let (plan, _) = applied_result(&module, &executor, draft).await;

        assert_eq!(plan.units.len(), 1);
        assert!(plan.units[0]
            .additional_entries
            .iter()
            .any(|entry| entry.action == PreparedEntryAction::Remove));
    }

    #[tokio::test]
    async fn failed_checkpoint_commit_is_completed_by_a_keep_only_resume() {
        let library_id = LibraryId::parse("lib-one");
        let (module, executor, draft, _, repository) = application_fixture_with(
            TargetEntryKind::Directory,
            TargetEntryKind::Directory,
            None,
            vec![SkillLibraryRecord {
                id: library_id.clone(),
                name: "Library One".to_string(),
                skills: vec![skill("demo")],
                retired_skills: Vec::new(),
                extra: serde_json::Map::new(),
            }],
            Vec::new(),
            vec![library_id.clone()],
        );
        repository
            .fail_next_checkpoint
            .store(true, Ordering::SeqCst);
        let preview = module.preview(draft.clone()).await.unwrap();

        let error = module
            .apply(
                ApplyLibraryApplicationRequest {
                    draft: draft.clone(),
                    expected_token: preview.token,
                },
                CancellationSignal::default(),
            )
            .await
            .unwrap_err();

        assert!(matches!(error, AppError::Io { .. }));
        let pending = repository.record.lock().unwrap().clone();
        assert!(pending.pending.is_some());
        assert!(pending.checkpoint.members.is_empty());

        module
            .retry_pending(draft.context, CancellationSignal::default())
            .await
            .unwrap();

        let completed = repository.record.lock().unwrap().clone();
        assert!(completed.pending.is_none());
        assert_eq!(
            completed.checkpoint.members,
            vec![LibraryMemberIdentity {
                library_id,
                member_name: "demo".to_string(),
            }]
        );
        let recorded = executor.0.lock().unwrap();
        let plan = recorded.as_ref().unwrap();
        assert!(plan
            .units
            .iter()
            .flat_map(|unit| unit.primary_entry.iter().chain(&unit.additional_entries))
            .all(|entry| entry.action == PreparedEntryAction::Keep));
    }

    #[tokio::test]
    async fn recovery_required_execution_remains_visible_after_reload() {
        let (module, _executor, draft) = application_fixture(TargetEntryKind::Missing, None);
        let LibraryApplicationModule {
            repository,
            facts,
            targets,
            ..
        } = module;
        let module = LibraryApplicationModule::with_recovery_status(
            repository,
            facts,
            targets,
            RecoveryExecutor,
            Arc::new(FixedRecoveryStatus(ScopeRecoveryState::Required)),
        );
        let preview = module.preview(draft.clone()).await.unwrap();

        let response = module
            .apply(
                ApplyLibraryApplicationRequest {
                    draft: draft.clone(),
                    expected_token: preview.token,
                },
                CancellationSignal::default(),
            )
            .await
            .unwrap();

        assert_eq!(
            response.application.sync_state,
            LibraryApplicationSyncState::RecoveryRequired
        );
        assert_eq!(
            module.read(draft.context).await.unwrap().sync_state,
            LibraryApplicationSyncState::RecoveryRequired
        );
    }

    #[tokio::test]
    async fn inaccessible_membership_drift_is_persisted_as_unverified() {
        let library_id = LibraryId::parse("lib-one");
        let (module, _executor, draft, _, repository) = application_fixture_with(
            TargetEntryKind::Missing,
            TargetEntryKind::Missing,
            None,
            vec![SkillLibraryRecord {
                id: library_id.clone(),
                name: "Library One".to_string(),
                skills: vec![skill("demo")],
                retired_skills: Vec::new(),
                extra: serde_json::Map::new(),
            }],
            vec![library_id.clone()],
            vec![library_id.clone()],
        );
        crate::application::library_membership::apply_membership_change(
            &mut repository.catalog.lock().unwrap(),
            &library_id,
            "demo",
            crate::application::library_membership::MembershipChange::Retire {
                retirement_id: crate::application::skill_libraries::RetirementId::parse(
                    "retirement-1",
                ),
                retired_at: "2026-09-06T00:00:00Z".to_string(),
            },
        )
        .unwrap();

        module
            .record_reconciliation_attention(
                draft.context.clone(),
                ReconciliationAttention::Unverified,
            )
            .await
            .unwrap();

        assert_eq!(
            module.read(draft.context).await.unwrap().sync_state,
            LibraryApplicationSyncState::Unverified
        );
    }

    #[tokio::test]
    async fn inaccessible_synced_scope_does_not_create_an_actionable_pending() {
        let library_id = LibraryId::parse("lib-one");
        let (module, _executor, draft, _, repository) = application_fixture_with(
            TargetEntryKind::Missing,
            TargetEntryKind::Missing,
            None,
            vec![SkillLibraryRecord {
                id: library_id.clone(),
                name: "Library One".to_string(),
                skills: vec![skill("demo")],
                retired_skills: Vec::new(),
                extra: serde_json::Map::new(),
            }],
            vec![library_id.clone()],
            vec![library_id],
        );

        module
            .record_reconciliation_attention(
                draft.context.clone(),
                ReconciliationAttention::Unverified,
            )
            .await
            .unwrap();

        assert!(repository.record.lock().unwrap().pending.is_none());
        assert_eq!(
            module.read(draft.context).await.unwrap().sync_state,
            LibraryApplicationSyncState::Synced
        );
    }

    #[tokio::test]
    async fn membership_resume_rebuilds_pending_from_persisted_drift() {
        let library_id = LibraryId::parse("lib-one");
        let (module, executor, draft, _, repository) = application_fixture_with(
            TargetEntryKind::Missing,
            TargetEntryKind::Symlink,
            Some("/libraries/lib-one/skills/demo"),
            vec![SkillLibraryRecord {
                id: library_id.clone(),
                name: "Library One".to_string(),
                skills: vec![skill("demo")],
                retired_skills: Vec::new(),
                extra: serde_json::Map::new(),
            }],
            vec![library_id.clone()],
            vec![library_id.clone()],
        );
        crate::application::library_membership::apply_membership_change(
            &mut repository.catalog.lock().unwrap(),
            &library_id,
            "demo",
            crate::application::library_membership::MembershipChange::Retire {
                retirement_id: crate::application::skill_libraries::RetirementId::parse(
                    "retirement-1",
                ),
                retired_at: "2026-09-06T00:00:00Z".to_string(),
            },
        )
        .unwrap();

        module
            .resume(draft.context, CancellationSignal::default())
            .await
            .unwrap();

        let record = repository.record.lock().unwrap().clone();
        assert!(record.pending.is_none());
        assert!(record.checkpoint.members.is_empty());
        let recorded = executor.0.lock().unwrap();
        let plan = recorded.as_ref().unwrap();
        assert!(plan
            .units
            .iter()
            .flat_map(|unit| unit.primary_entry.iter().chain(&unit.additional_entries))
            .any(|entry| entry.action == PreparedEntryAction::Remove));
    }

    #[tokio::test]
    async fn projected_catalog_reports_a_high_priority_member_switch_before_commit() {
        let high = LibraryId::parse("high");
        let low = LibraryId::parse("low");
        let (module, _executor, draft, _, repository) = application_fixture_with(
            TargetEntryKind::Symlink,
            TargetEntryKind::Symlink,
            Some("/libraries/low/skills/demo"),
            vec![
                SkillLibraryRecord {
                    id: high.clone(),
                    name: "High".to_string(),
                    skills: Vec::new(),
                    retired_skills: Vec::new(),
                    extra: serde_json::Map::new(),
                },
                SkillLibraryRecord {
                    id: low.clone(),
                    name: "Low".to_string(),
                    skills: vec![skill("demo")],
                    retired_skills: Vec::new(),
                    extra: serde_json::Map::new(),
                },
            ],
            vec![high.clone(), low.clone()],
            vec![high.clone(), low],
        );
        let mut projected = repository.catalog.lock().unwrap().clone();
        crate::application::library_membership::apply_membership_change(
            &mut projected,
            &high,
            "demo",
            crate::application::library_membership::MembershipChange::Upsert(skill("demo")),
        )
        .unwrap();

        let plan = module
            .plan_resume_with_catalog(draft.context, projected)
            .await
            .unwrap();

        assert_eq!(plan.preview.unwrap().switched_skill_names, vec!["demo"]);
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

    #[test]
    fn pending_merge_preserves_identity_and_expands_recognized_members() {
        let original = PendingReconciliation {
            reconciliation_id: "reconciliation-1".to_string(),
            attention: ReconciliationAttention::Pending,
            reasons: vec![ReconciliationReason::ApplicationChanged],
            before_application: LibraryApplicationState::default(),
            target_application: LibraryApplicationState::default(),
            recognized_members: vec![member("library-a", "alpha")],
            affected_members: Vec::new(),
            target_members: vec![member("library-a", "alpha")],
        };
        let latest_target = LibraryApplicationState {
            ordered_library_ids: vec![LibraryId::parse("library-b")],
            selected_agent_ids: Vec::new(),
        };

        let merged = original.merged_with(
            vec![ReconciliationReason::MembershipChanged],
            latest_target.clone(),
            vec![member("library-a", "beta")],
            vec![member("library-b", "gamma")],
        );

        assert_eq!(merged.reconciliation_id, "reconciliation-1");
        assert_eq!(
            merged.reasons,
            vec![
                ReconciliationReason::ApplicationChanged,
                ReconciliationReason::MembershipChanged,
            ]
        );
        assert_eq!(
            merged.recognized_members,
            vec![
                member("library-a", "alpha"),
                member("library-a", "beta"),
                member("library-b", "gamma"),
            ]
        );
        assert_eq!(merged.target_application, latest_target);
        assert_eq!(merged.target_members, vec![member("library-b", "gamma")]);
    }

    #[test]
    fn membership_reconciliation_only_targets_changed_skill_directories() {
        let checkpoint = ReconciliationCheckpoint {
            members: vec![member("lib-one", "alpha"), member("lib-one", "shared")],
        };
        let target = vec![
            member("lib-one", "alpha"),
            member("lib-two", "shared"),
            member("lib-one", "beta"),
        ];

        let changed = membership_changed_skill_directories(&checkpoint, None, &target).unwrap();

        assert_eq!(
            changed,
            BTreeSet::from([
                SkillDirectoryName::try_from("beta").unwrap(),
                SkillDirectoryName::try_from("shared").unwrap(),
            ])
        );
    }

    #[tokio::test]
    async fn projected_member_addition_does_not_repair_an_unrelated_missing_skill() {
        let library_id = LibraryId::parse("lib-one");
        let (mut module, _, draft, _, repository) = application_fixture_with(
            TargetEntryKind::Missing,
            TargetEntryKind::Missing,
            None,
            vec![SkillLibraryRecord {
                id: library_id.clone(),
                name: "Library One".to_string(),
                skills: vec![skill("alpha")],
                retired_skills: Vec::new(),
                extra: serde_json::Map::new(),
            }],
            vec![library_id.clone()],
            vec![library_id.clone()],
        );
        module.facts.0.agent_runtime.agents.clear();
        repository
            .record
            .lock()
            .unwrap()
            .current
            .selected_agent_ids
            .clear();
        let projected = LibraryCatalog {
            schema_version: LIBRARY_SCHEMA_VERSION,
            libraries: vec![SkillLibraryRecord {
                id: library_id,
                name: "Library One".to_string(),
                skills: vec![skill("alpha"), skill("beta")],
                retired_skills: Vec::new(),
                extra: serde_json::Map::new(),
            }],
            extra: serde_json::Map::new(),
        };

        let plan = module
            .plan_resume_with_catalog(draft.context, projected)
            .await
            .expect("plan member propagation");
        let preview = plan.preview.expect("member preview");

        assert_eq!(preview.added_skill_names, vec!["beta"]);
        assert_eq!(plan.entries.len(), 1);
    }

    #[tokio::test]
    async fn reapply_with_membership_drift_retains_its_full_scope_after_failure() {
        let library_id = LibraryId::parse("lib-one");
        let (mut module, _, mut draft, _, repository) = application_fixture_with(
            TargetEntryKind::Missing,
            TargetEntryKind::Missing,
            None,
            vec![SkillLibraryRecord {
                id: library_id.clone(),
                name: "Library One".to_string(),
                skills: vec![skill("alpha")],
                retired_skills: Vec::new(),
                extra: serde_json::Map::new(),
            }],
            vec![library_id.clone()],
            vec![library_id],
        );
        module.facts.0.agent_runtime.agents.clear();
        draft.selected_agent_ids.clear();
        repository
            .record
            .lock()
            .unwrap()
            .current
            .selected_agent_ids
            .clear();
        repository.catalog.lock().unwrap().libraries[0]
            .skills
            .push(skill("beta"));
        repository
            .fail_next_checkpoint
            .store(true, Ordering::SeqCst);
        let preview = module.preview(draft.clone()).await.unwrap();

        let result = module
            .apply(
                ApplyLibraryApplicationRequest {
                    draft: draft.clone(),
                    expected_token: preview.token,
                },
                CancellationSignal::default(),
            )
            .await;
        assert!(matches!(result, Err(AppError::Io { .. })));

        let resumed = module
            .retry_pending(draft.context, CancellationSignal::default())
            .await
            .unwrap();

        assert_eq!(
            resumed
                .units
                .iter()
                .map(|unit| unit.skill_name.as_str())
                .collect::<Vec<_>>(),
            vec!["alpha", "beta"]
        );
        assert!(!resumed.application.pending);
    }

    #[tokio::test]
    async fn projected_member_addition_preserves_a_confirmed_reapply_scope() {
        let library_id = LibraryId::parse("lib-one");
        let (mut module, _, draft, _, repository) = application_fixture_with(
            TargetEntryKind::Missing,
            TargetEntryKind::Missing,
            None,
            vec![SkillLibraryRecord {
                id: library_id.clone(),
                name: "Library One".to_string(),
                skills: vec![skill("alpha")],
                retired_skills: Vec::new(),
                extra: serde_json::Map::new(),
            }],
            vec![library_id.clone()],
            vec![library_id.clone()],
        );
        module.facts.0.agent_runtime.agents.clear();
        {
            let mut record = repository.record.lock().unwrap();
            record.current.selected_agent_ids.clear();
            record.pending = Some(PendingReconciliation {
                reconciliation_id: "reapply-alpha".to_string(),
                attention: ReconciliationAttention::Pending,
                reasons: vec![ReconciliationReason::ReapplyRequested],
                before_application: record.current.clone(),
                target_application: record.current.clone(),
                recognized_members: record.checkpoint.members.clone(),
                affected_members: Vec::new(),
                target_members: record.checkpoint.members.clone(),
            });
        }
        let projected = LibraryCatalog {
            schema_version: LIBRARY_SCHEMA_VERSION,
            libraries: vec![SkillLibraryRecord {
                id: library_id,
                name: "Library One".to_string(),
                skills: vec![skill("alpha"), skill("beta")],
                retired_skills: Vec::new(),
                extra: serde_json::Map::new(),
            }],
            extra: serde_json::Map::new(),
        };

        let plan = module
            .plan_resume_with_catalog(draft.context, projected)
            .await
            .expect("plan membership and confirmed reapply");

        assert_eq!(plan.entries.len(), 2);
        assert_eq!(
            plan.preview.unwrap().changed_directory_skill_names,
            vec!["alpha", "beta"]
        );
    }

    #[test]
    fn reconciliation_scope_is_checkpoint_pending_and_desired_union() {
        let checkpoint = ReconciliationCheckpoint {
            members: vec![member("library-a", "old")],
        };
        let pending = PendingReconciliation {
            reconciliation_id: "reconciliation-1".to_string(),
            attention: ReconciliationAttention::Pending,
            reasons: vec![ReconciliationReason::MembershipChanged],
            before_application: LibraryApplicationState::default(),
            target_application: LibraryApplicationState::default(),
            recognized_members: vec![member("library-a", "pending")],
            affected_members: vec![member("library-a", "pending")],
            target_members: Vec::new(),
        };

        assert_eq!(
            reconciliation_members(
                &checkpoint,
                Some(&pending),
                &[member("library-b", "desired")],
            ),
            vec![
                member("library-a", "old"),
                member("library-a", "pending"),
                member("library-b", "desired"),
            ]
        );
    }

    #[tokio::test]
    async fn membership_resume_cleans_a_published_member_retired_before_checkpoint() {
        let library_id = LibraryId::parse("lib-one");
        let (module, executor, draft, _, repository) = application_fixture_with(
            TargetEntryKind::Missing,
            TargetEntryKind::Symlink,
            Some("/libraries/lib-one/skills/beta"),
            vec![SkillLibraryRecord {
                id: library_id.clone(),
                name: "Library One".to_string(),
                skills: vec![skill("alpha")],
                retired_skills: Vec::new(),
                extra: serde_json::Map::new(),
            }],
            vec![library_id.clone()],
            vec![library_id.clone()],
        );
        repository.catalog.lock().unwrap().libraries[0]
            .skills
            .extend([skill("beta"), skill("gamma")]);
        module
            .record_reconciliation_attention(
                draft.context.clone(),
                ReconciliationAttention::Pending,
            )
            .await
            .unwrap();
        crate::application::library_membership::apply_membership_change(
            &mut repository.catalog.lock().unwrap(),
            &library_id,
            "beta",
            crate::application::library_membership::MembershipChange::Retire {
                retirement_id: crate::application::skill_libraries::RetirementId::parse(
                    "retire-beta",
                ),
                retired_at: "2026-09-07T00:00:00Z".to_string(),
            },
        )
        .unwrap();

        let response = module
            .resume(draft.context, CancellationSignal::default())
            .await
            .unwrap();

        assert!(!response.application.pending);
        let plan = executor.0.lock().unwrap();
        let beta_path = locator(TEST_AGENT_ROOT).join_child("beta");
        assert!(
            plan.as_ref().unwrap().units.iter().any(|unit| {
                unit.skill_name == "beta"
                    && unit.additional_entries.iter().any(|entry| {
                        entry.destination == beta_path
                            && entry.action == PreparedEntryAction::Remove
                    })
            }),
            "the published beta link must be removed before its pending references are released"
        );
    }

    #[tokio::test]
    async fn net_zero_membership_resume_preserves_an_unrelated_missing_link() {
        let library_id = LibraryId::parse("lib-one");
        let (mut module, _, draft, _, repository) = application_fixture_with(
            TargetEntryKind::Missing,
            TargetEntryKind::Missing,
            None,
            vec![SkillLibraryRecord {
                id: library_id.clone(),
                name: "Library One".to_string(),
                skills: vec![skill("alpha")],
                retired_skills: Vec::new(),
                extra: serde_json::Map::new(),
            }],
            vec![library_id.clone()],
            vec![library_id.clone()],
        );
        module.facts.0.agent_runtime.agents.clear();
        repository
            .record
            .lock()
            .unwrap()
            .current
            .selected_agent_ids
            .clear();
        repository.catalog.lock().unwrap().libraries[0]
            .skills
            .push(skill("beta"));
        module
            .record_reconciliation_attention(
                draft.context.clone(),
                ReconciliationAttention::Pending,
            )
            .await
            .unwrap();
        crate::application::library_membership::apply_membership_change(
            &mut repository.catalog.lock().unwrap(),
            &library_id,
            "beta",
            crate::application::library_membership::MembershipChange::Retire {
                retirement_id: crate::application::skill_libraries::RetirementId::parse(
                    "retire-beta",
                ),
                retired_at: "2026-09-07T00:00:00Z".to_string(),
            },
        )
        .unwrap();

        let response = module
            .resume(draft.context, CancellationSignal::default())
            .await
            .unwrap();

        assert!(!response.application.pending);
        assert!(response.units.iter().all(|unit| unit.skill_name != "alpha"));
    }

    fn member(library_id: &str, member_name: &str) -> LibraryMemberIdentity {
        LibraryMemberIdentity {
            library_id: LibraryId::parse(library_id),
            member_name: member_name.to_string(),
        }
    }

    #[tokio::test]
    async fn membership_resume_remembers_a_member_removed_and_readded_before_completion() {
        let library_id = LibraryId::parse("lib-one");
        let (mut module, _, draft, _, repository) = application_fixture_with(
            TargetEntryKind::Missing,
            TargetEntryKind::Missing,
            None,
            vec![SkillLibraryRecord {
                id: library_id.clone(),
                name: "Library One".to_string(),
                skills: vec![skill("alpha"), skill("beta")],
                retired_skills: Vec::new(),
                extra: serde_json::Map::new(),
            }],
            vec![library_id.clone()],
            vec![library_id.clone()],
        );
        module.facts.0.agent_runtime.agents.clear();
        repository
            .record
            .lock()
            .unwrap()
            .current
            .selected_agent_ids
            .clear();
        crate::application::library_membership::apply_membership_change(
            &mut repository.catalog.lock().unwrap(),
            &library_id,
            "beta",
            crate::application::library_membership::MembershipChange::Retire {
                retirement_id: crate::application::skill_libraries::RetirementId::parse(
                    "retire-beta",
                ),
                retired_at: "2026-09-07T00:00:00Z".to_string(),
            },
        )
        .unwrap();
        module
            .record_reconciliation_attention(
                draft.context.clone(),
                ReconciliationAttention::Pending,
            )
            .await
            .unwrap();
        crate::application::library_membership::apply_membership_change(
            &mut repository.catalog.lock().unwrap(),
            &library_id,
            "beta",
            crate::application::library_membership::MembershipChange::Upsert(skill("beta")),
        )
        .unwrap();
        module
            .record_reconciliation_attention(
                draft.context.clone(),
                ReconciliationAttention::Pending,
            )
            .await
            .unwrap();

        let response = module
            .resume(draft.context, CancellationSignal::default())
            .await
            .unwrap();

        assert!(!response.application.pending);
        assert_eq!(
            response
                .units
                .iter()
                .map(|unit| unit.skill_name.as_str())
                .collect::<Vec<_>>(),
            vec!["beta"]
        );
    }

    #[tokio::test]
    async fn synced_scope_background_resume_does_not_access_the_filesystem() {
        struct UnavailableFacts;

        impl ScopePlanningSnapshotSource for UnavailableFacts {
            fn snapshot<'a>(
                &'a self,
                _context: &'a SkillLocationRef,
            ) -> InstallFuture<'a, Result<ScopePlanningSnapshot, AppError>> {
                Box::pin(async { Err(AppError::StaleEnvironment) })
            }
        }

        let library_id = LibraryId::parse("lib-one");
        let (module, executor, draft, _, repository) = application_fixture_with(
            TargetEntryKind::Directory,
            TargetEntryKind::Missing,
            None,
            vec![SkillLibraryRecord {
                id: library_id.clone(),
                name: "Library One".to_string(),
                skills: vec![skill("alpha")],
                retired_skills: Vec::new(),
                extra: serde_json::Map::new(),
            }],
            vec![library_id.clone()],
            vec![library_id],
        );
        let module = LibraryApplicationModule::new(
            repository.clone(),
            UnavailableFacts,
            module.targets,
            executor.clone(),
        );

        let plan = module.plan_resume(draft.context.clone()).await.unwrap();
        assert!(plan.entries.is_empty());
        assert!(plan.preview.is_none());
        let response = module
            .resume(draft.context.clone(), CancellationSignal::default())
            .await
            .unwrap();
        assert_eq!(
            response.application.sync_state,
            LibraryApplicationSyncState::Synced
        );
        assert!(response.units.is_empty());
        assert!(executor.0.lock().unwrap().is_none());

        let mut projected = repository.catalog.lock().unwrap().clone();
        assert!(module
            .plan_resume_with_catalog(draft.context.clone(), projected.clone())
            .await
            .unwrap()
            .entries
            .is_empty());
        projected.libraries[0].skills.push(skill("beta"));
        assert!(matches!(
            module
                .plan_resume_with_catalog(draft.context, projected)
                .await,
            Err(AppError::StaleEnvironment)
        ));
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
            backend: if cfg!(windows) {
                ExecutionBackend::NativeWindows
            } else {
                ExecutionBackend::NativeUnix
            },
            physical_parent: if cfg!(windows) {
                PhysicalParentIdentity::Windows {
                    volume_serial: 1,
                    file_id: 2,
                }
            } else {
                PhysicalParentIdentity::Unix {
                    device: 1,
                    inode: 2,
                }
            },
            normalized_final_child_name: name.to_string(),
        }
    }
}

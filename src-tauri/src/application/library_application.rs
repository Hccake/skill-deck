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


use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::application::install::InstallPlanExecutor;
use crate::application::install_planner::InstallPlanningFactSource;
use crate::application::mutation::plan::{
    preview_token, stable_digest, ExecutionUnit, ExpectedTargetEntry, MutationPlan,
    PreparedEntryAction, PreparedEntryMutation, PreviewFingerprint, PreviewToken,
};
use crate::application::mutation::result::MutationUnitResult;
use crate::application::skill_entries::SkillEntryObserver;
use crate::core::agent_definition::AgentId;
use crate::core::mutation::{CancellationSignal, MutationKind};
use crate::environment::planning::TargetFactResolver;
use crate::environment::runtime::ObservedEntryId;
use crate::environment::types::{ContextRef, ResourceLocator};
use crate::error::AppError;
use crate::storage::lock_plan::{LockExpectedState, PreparedLockMutation};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct ObservedEntryOwner {
    pub agent_id: AgentId,
    pub display_name: String,
    pub logical_target_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub enum ObservedEntryKind {
    Missing,
    Directory,
    Symlink,
    Junction,
    BrokenLink,
    Other,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct ObservedPhysicalEntry {
    pub entry_id: ObservedEntryId,
    pub display_path: ResourceLocator,
    pub kind: ObservedEntryKind,
    pub physical_target_key: String,
    pub owners: Vec<ObservedEntryOwner>,
    pub will_break_if_canonical_removed: bool,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct RemovePreview {
    pub token: PreviewToken,
    pub context: ContextRef,
    pub skill_name: String,
    pub canonical: ObservedEntryKind,
    pub physical_entries: Vec<ObservedPhysicalEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct RemoveSelection {
    pub remove_canonical: bool,
    pub entry_ids: Vec<ObservedEntryId>,
    pub confirm_entity_directories: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct RemoveRequest {
    pub token: PreviewToken,
    pub context: ContextRef,
    pub skill_name: String,
    pub selection: RemoveSelection,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct RemoveResponse {
    pub units: Vec<MutationUnitResult>,
}

pub struct RemoveService<F, T, E> {
    observer: SkillEntryObserver<F, T>,
    executor: E,
}

impl<F, T, E> RemoveService<F, T, E>
where
    F: InstallPlanningFactSource,
    T: TargetFactResolver,
    E: InstallPlanExecutor,
{
    pub fn new(observer: SkillEntryObserver<F, T>, executor: E) -> Self {
        Self { observer, executor }
    }

    pub async fn preview(
        &self,
        context: &ContextRef,
        skill_name: &str,
    ) -> Result<RemovePreview, AppError> {
        if skill_name.trim().is_empty() {
            return Err(AppError::Validation {
                field: Some("skillName".to_string()),
                message: "Skill name is required".to_string(),
            });
        }
        let snapshot = self.observer.observe(context, skill_name).await?;
        remove_preview(context, skill_name, &snapshot)
    }

    pub async fn execute(
        &self,
        request: &RemoveRequest,
        cancellation: CancellationSignal,
    ) -> Result<RemoveResponse, AppError> {
        let snapshot = self
            .observer
            .observe(&request.context, &request.skill_name)
            .await?;
        let preview = remove_preview(&request.context, &request.skill_name, &snapshot)?;
        validate_token(&request.token, &preview.token)?;
        validate_remove_execution(&preview, &request.selection)?;
        let selected = request.selection.entry_ids.iter().collect::<BTreeSet<_>>();
        let canonical_entry = request
            .selection
            .remove_canonical
            .then(|| PreparedEntryMutation {
                key: snapshot.canonical.key.clone(),
                destination: snapshot.canonical.destination.clone(),
                action: PreparedEntryAction::Remove,
                owner_agent_ids: Vec::new(),
            });
        let required_agent_entries = snapshot
            .entries
            .iter()
            .filter(|entry| selected.contains(&entry.public.entry_id))
            .map(|entry| PreparedEntryMutation {
                key: entry.fact.key.clone(),
                destination: entry.fact.destination.clone(),
                action: PreparedEntryAction::Remove,
                owner_agent_ids: entry
                    .public
                    .owners
                    .iter()
                    .map(|owner| owner.agent_id.clone())
                    .collect(),
            })
            .collect();
        let lock_mutation = request
            .selection
            .remove_canonical
            .then(|| PreparedLockMutation {
                target: snapshot.facts.resolved_context.lock.clone(),
                legacy_target: None,
                schema: snapshot.facts.lock_schema,
                skill_name: request.skill_name.clone(),
                replacement: None,
                root_replacements: BTreeMap::new(),
                expected: LockExpectedState::capture(
                    &snapshot.facts.lock_document,
                    [&request.skill_name],
                    std::iter::empty::<&str>(),
                ),
            });
        let plan = MutationPlan {
            operation_id: Uuid::new_v4().simple().to_string(),
            payloads: BTreeMap::new(),
            units: vec![ExecutionUnit {
                id: format!("remove:{}", request.skill_name),
                skill_name: request.skill_name.clone(),
                source: None,
                target: request.context.clone(),
                expected_revisions: snapshot.facts.revisions.clone(),
                canonical_entry,
                required_agent_entries,
                lock_mutation,
                expected_targets: std::iter::once(&snapshot.canonical)
                    .chain(snapshot.entries.iter().map(|entry| &entry.fact))
                    .map(|fact| ExpectedTargetEntry {
                        key: fact.key.clone(),
                        fingerprint: fact.fingerprint.clone(),
                        expected_content_manifest_hash: None,
                    })
                    .collect(),
            }],
        };
        Ok(RemoveResponse {
            units: self.executor.execute(plan, cancellation).await,
        })
    }
}

pub fn validate_remove_selection(selection: &RemoveSelection) -> Result<(), AppError> {
    let mut ids = BTreeSet::new();
    if selection.entry_ids.iter().any(|id| !ids.insert(id)) {
        return Err(AppError::Validation {
            field: Some("entryIds".to_string()),
            message: "duplicate observed entry selection".to_string(),
        });
    }
    if !selection.remove_canonical && selection.entry_ids.is_empty() {
        return Err(AppError::Validation {
            field: Some("selection".to_string()),
            message: "nothing is selected for removal".to_string(),
        });
    }
    Ok(())
}

pub fn validate_remove_execution(
    preview: &RemovePreview,
    selection: &RemoveSelection,
) -> Result<(), AppError> {
    validate_remove_selection(selection)?;
    let available = preview
        .physical_entries
        .iter()
        .map(|entry| (&entry.entry_id, entry))
        .collect::<std::collections::BTreeMap<_, _>>();
    let mut selected_directory = false;
    for id in &selection.entry_ids {
        let entry = available.get(id).ok_or(AppError::StaleTarget)?;
        selected_directory |= entry.kind == ObservedEntryKind::Directory;
    }
    if selected_directory && !selection.confirm_entity_directories {
        return Err(AppError::Validation {
            field: Some("confirmEntityDirectories".to_string()),
            message: "selected entity directories require confirmation".to_string(),
        });
    }
    Ok(())
}

fn remove_preview(
    context: &ContextRef,
    skill_name: &str,
    snapshot: &crate::application::skill_entries::ObservedSkillSnapshot,
) -> Result<RemovePreview, AppError> {
    let observed_state_digest = stable_digest(&(
        &snapshot.canonical.key,
        &snapshot.canonical.fingerprint,
        snapshot
            .entries
            .iter()
            .map(|entry| (&entry.public.entry_id, &entry.fact.fingerprint))
            .collect::<Vec<_>>(),
        snapshot
            .facts
            .lock_document
            .entry_snapshot(skill_name)
            .value()
            .cloned(),
    ))?;
    let token = preview_token(&PreviewFingerprint {
        kind: MutationKind::Remove,
        request_digest: stable_digest(&(context, skill_name))?,
        revisions: snapshot.facts.revisions.clone(),
        observed_state_digest,
        planner_contract_version: 1,
    })?;
    Ok(RemovePreview {
        token,
        context: context.clone(),
        skill_name: skill_name.to_string(),
        canonical: crate::application::skill_entries::observed_entry_kind(
            snapshot.canonical.entry_kind,
        ),
        physical_entries: snapshot
            .entries
            .iter()
            .map(|entry| entry.public.clone())
            .collect(),
    })
}

fn validate_token(expected: &PreviewToken, actual: &PreviewToken) -> Result<(), AppError> {
    if expected.registry_revision != actual.registry_revision {
        return Err(AppError::StaleRegistry);
    }
    if expected.environment_revision != actual.environment_revision {
        return Err(AppError::StaleEnvironment);
    }
    if expected.context_revision != actual.context_revision
        || expected.generation != actual.generation
    {
        return Err(AppError::StaleContext);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::environment::runtime::ObservedEntryId;

    #[test]
    fn remove_selection_rejects_duplicate_observed_ids() {
        let id = ObservedEntryId::parse("entry-v1-demo").unwrap();
        assert!(validate_remove_selection(&RemoveSelection {
            remove_canonical: false,
            entry_ids: vec![id.clone(), id],
            confirm_entity_directories: false,
        })
        .is_err());
    }

    #[test]
    fn selected_entity_directory_requires_explicit_confirmation() {
        let id = ObservedEntryId::parse("entry-v1-copy").unwrap();
        let preview = RemovePreview {
            token: crate::application::mutation::plan::PreviewToken {
                generation: "preview-v1-remove".to_string(),
                registry_revision: "registry-1".to_string(),
                environment_revision: "environment-1".to_string(),
                context_revision: crate::environment::runtime::ContextSnapshotRevision::parse(
                    "context-v1-remove",
                )
                .unwrap(),
            },
            context: ContextRef {
                environment: crate::environment::types::EnvironmentRef::Host,
                scope: crate::environment::types::ContextScope::Global,
            },
            skill_name: "demo".to_string(),
            canonical: ObservedEntryKind::Directory,
            physical_entries: vec![ObservedPhysicalEntry {
                entry_id: id.clone(),
                display_path: ResourceLocator {
                    environment: crate::environment::types::EnvironmentRef::Host,
                    native_path: "/agent/skills/demo".to_string(),
                },
                kind: ObservedEntryKind::Directory,
                physical_target_key: "target-v1-copy".to_string(),
                owners: Vec::new(),
                will_break_if_canonical_removed: false,
            }],
        };
        let selection = RemoveSelection {
            remove_canonical: false,
            entry_ids: vec![id],
            confirm_entity_directories: false,
        };

        assert!(validate_remove_execution(&preview, &selection).is_err());
    }
}

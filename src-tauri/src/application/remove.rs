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
#[specta(rename_all = "camelCase")]
#[derive(PartialEq, Eq)]
#[serde(tag = "kind", content = "entryIds", rename_all = "camelCase")]
pub enum RemoveIntent {
    FullSkill,
    AgentEntries(Vec<ObservedEntryId>),
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct RemoveRequest {
    pub token: PreviewToken,
    pub context: ContextRef,
    pub skill_name: String,
    pub intent: RemoveIntent,
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
        let selected = selected_entry_ids(&preview, &request.intent)?;
        let remove_canonical = request.intent == RemoveIntent::FullSkill;
        let canonical_entry = remove_canonical.then(|| PreparedEntryMutation {
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
        let lock_mutation = (remove_canonical
            && snapshot
                .facts
                .lock_document
                .entry_snapshot(&request.skill_name)
                .value()
                .is_some())
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
            kind: MutationKind::Remove,
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

fn selected_entry_ids(
    preview: &RemovePreview,
    intent: &RemoveIntent,
) -> Result<BTreeSet<ObservedEntryId>, AppError> {
    if intent == &RemoveIntent::FullSkill {
        return Ok(preview
            .physical_entries
            .iter()
            .map(|entry| entry.entry_id.clone())
            .collect());
    }
    let RemoveIntent::AgentEntries(entry_ids) = intent else {
        unreachable!("FullSkill is handled above")
    };
    let mut ids = BTreeSet::new();
    if entry_ids.iter().any(|id| !ids.insert(id.clone())) {
        return Err(AppError::Validation {
            field: Some("entryIds".to_string()),
            message: "duplicate observed entry selection".to_string(),
        });
    }
    if ids.is_empty() {
        return Err(AppError::Validation {
            field: Some("selection".to_string()),
            message: "nothing is selected for removal".to_string(),
        });
    }
    let available = preview
        .physical_entries
        .iter()
        .map(|entry| &entry.entry_id)
        .collect::<BTreeSet<_>>();
    if ids.iter().any(|id| !available.contains(id)) {
        return Err(AppError::StaleTarget);
    }
    Ok(ids)
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

    #[test]
    fn remove_intent_has_explicit_wire_shape() {
        let full: RemoveIntent = serde_json::from_str(r#"{"kind":"fullSkill"}"#).unwrap();
        assert_eq!(full, RemoveIntent::FullSkill);

        let entries: RemoveIntent =
            serde_json::from_str(r#"{"kind":"agentEntries","entryIds":["entry-v1-demo"]}"#)
                .unwrap();
        assert_eq!(
            entries,
            RemoveIntent::AgentEntries(vec![crate::environment::runtime::ObservedEntryId::parse(
                "entry-v1-demo"
            )
            .unwrap()])
        );
    }

    #[test]
    fn full_skill_request_serializes_without_entry_selection() {
        let request = RemoveRequest {
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
            intent: RemoveIntent::FullSkill,
        };

        let json = serde_json::to_value(request).unwrap();
        assert_eq!(json["intent"], serde_json::json!({ "kind": "fullSkill" }));
        assert!(json.get("selection").is_none());
    }

    #[test]
    fn agent_entry_intent_rejects_unknown_entry() {
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
            physical_entries: Vec::new(),
        };

        assert_eq!(
            selected_entry_ids(&preview, &RemoveIntent::AgentEntries(vec![id])),
            Err(AppError::StaleTarget)
        );
    }
}

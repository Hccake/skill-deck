use std::collections::BTreeMap;
#[cfg(test)]
use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use specta::Type;

use crate::application::payload_session::PinnedPayloadLease;
use crate::core::agent_definition::AgentId;
use crate::core::mutation::MutationKind;
use crate::core::skill_payload::PayloadId;
use crate::environment::content_manifest::ContentManifestHash;
#[cfg(test)]
use crate::environment::runtime::ObservedEntryId;
use crate::environment::runtime::{ContextSnapshotRevision, EntryFingerprint, PhysicalTargetKey};
use crate::environment::types::ResourceLocator;
use crate::error::AppError;
use crate::models::InstallMode;
use crate::storage::lock_plan::PreparedLockMutation;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PreparedEntryAction {
    Keep,
    Replace {
        payload_id: PayloadId,
        requested_mode: InstallMode,
    },
    Remove,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedEntryMutation {
    pub key: PhysicalTargetKey,
    pub destination: ResourceLocator,
    pub action: PreparedEntryAction,
    pub owner_agent_ids: Vec<AgentId>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExpectedTargetEntry {
    pub key: PhysicalTargetKey,
    pub fingerprint: EntryFingerprint,
    pub expected_content_manifest_hash: Option<ContentManifestHash>,
}

#[derive(Debug, Clone)]
pub struct ExecutionUnit {
    pub id: String,
    pub skill_name: String,
    pub source: Option<crate::environment::types::SkillLocationRef>,
    pub target: crate::environment::types::SkillLocationRef,
    pub expected_revisions: RuntimeRevisions,
    pub canonical_entry: Option<PreparedEntryMutation>,
    pub required_agent_entries: Vec<PreparedEntryMutation>,
    pub lock_mutation: Option<PreparedLockMutation>,
    pub expected_targets: Vec<ExpectedTargetEntry>,
}

pub struct MutationPlan {
    pub kind: MutationKind,
    pub operation_id: String,
    pub payloads: BTreeMap<PayloadId, PinnedPayloadLease>,
    pub units: Vec<ExecutionUnit>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct PreviewToken {
    pub generation: String,
    pub registry_revision: String,
    pub environment_revision: String,
    pub context_revision: ContextSnapshotRevision,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeRevisions {
    pub registry: String,
    pub environment: String,
    pub context: ContextSnapshotRevision,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct PreviewFingerprint {
    pub(super) kind: MutationKind,
    pub(super) request_digest: String,
    pub(super) revisions: RuntimeRevisions,
    pub(super) observed_state_digest: String,
    pub(super) planner_contract_version: u32,
}

pub(super) fn preview_token(fingerprint: &PreviewFingerprint) -> Result<PreviewToken, AppError> {
    Ok(PreviewToken {
        generation: format!(
            "preview-v1-{}",
            stable_digest(fingerprint)?.trim_start_matches("digest-v1-")
        ),
        registry_revision: fingerprint.revisions.registry.clone(),
        environment_revision: fingerprint.revisions.environment.clone(),
        context_revision: fingerprint.revisions.context.clone(),
    })
}

pub fn stable_digest<T>(value: &T) -> Result<String, AppError>
where
    T: Serialize,
{
    Ok(format!(
        "digest-v1-{:x}",
        Sha256::digest(serde_json::to_vec(value)?)
    ))
}

#[cfg(test)]
pub fn validate_observed_subset(
    available: &BTreeSet<ObservedEntryId>,
    selected: &[ObservedEntryId],
) -> Result<(), AppError> {
    if selected.iter().all(|id| available.contains(id)) {
        Ok(())
    } else {
        Err(AppError::StaleTarget)
    }
}

pub fn group_physical_mutations(
    mutations: Vec<PreparedEntryMutation>,
) -> Result<Vec<PreparedEntryMutation>, AppError> {
    let mut grouped: BTreeMap<PhysicalTargetKey, PreparedEntryMutation> = BTreeMap::new();
    for mut mutation in mutations {
        mutation.owner_agent_ids.sort();
        mutation.owner_agent_ids.dedup();
        match grouped.get_mut(&mutation.key) {
            Some(existing)
                if existing.destination == mutation.destination
                    && existing.action == mutation.action =>
            {
                existing.owner_agent_ids.extend(mutation.owner_agent_ids);
                existing.owner_agent_ids.sort();
                existing.owner_agent_ids.dedup();
            }
            Some(_) => return Err(AppError::StaleTarget),
            None => {
                grouped.insert(mutation.key.clone(), mutation);
            }
        }
    }
    Ok(grouped.into_values().collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::agent_definition::AgentId;
    use crate::core::mutation::MutationKind;
    use crate::environment::runtime::{ContextSnapshotRevision, ObservedEntryId};
    use crate::environment::runtime::{
        ExecutionBackend, PhysicalParentIdentity, PhysicalTargetKey,
    };
    use crate::environment::types::{EnvironmentRef, ResourceLocator};
    use crate::error::AppError;

    fn key(name: &str) -> PhysicalTargetKey {
        PhysicalTargetKey {
            backend: ExecutionBackend::NativeUnix,
            physical_parent: PhysicalParentIdentity::Unix {
                device: 1,
                inode: 2,
            },
            normalized_final_child_name: name.to_string(),
        }
    }

    fn destination(path: &str) -> ResourceLocator {
        ResourceLocator {
            environment: EnvironmentRef::Native,
            native_path: path.to_string(),
        }
    }

    fn agent(id: &str) -> AgentId {
        AgentId::parse(id).expect("agent")
    }

    #[test]
    fn groups_one_physical_mutation_and_fans_out_sorted_owners() {
        let grouped = group_physical_mutations(vec![
            PreparedEntryMutation {
                key: key("demo"),
                destination: destination("/skills/demo"),
                action: PreparedEntryAction::Remove,
                owner_agent_ids: vec![agent("z-agent")],
            },
            PreparedEntryMutation {
                key: key("demo"),
                destination: destination("/skills/demo"),
                action: PreparedEntryAction::Remove,
                owner_agent_ids: vec![agent("a-agent"), agent("z-agent")],
            },
        ])
        .expect("grouped");

        assert_eq!(grouped.len(), 1);
        assert_eq!(
            grouped[0]
                .owner_agent_ids
                .iter()
                .map(AgentId::as_str)
                .collect::<Vec<_>>(),
            vec!["a-agent", "z-agent"]
        );
    }

    #[test]
    fn rejects_conflicting_intent_for_one_physical_target() {
        let result = group_physical_mutations(vec![
            PreparedEntryMutation {
                key: key("demo"),
                destination: destination("/skills/demo"),
                action: PreparedEntryAction::Remove,
                owner_agent_ids: vec![agent("a-agent")],
            },
            PreparedEntryMutation {
                key: key("demo"),
                destination: destination("/other/demo"),
                action: PreparedEntryAction::Remove,
                owner_agent_ids: vec![agent("b-agent")],
            },
        ]);

        assert!(matches!(result, Err(AppError::StaleTarget)));
    }

    fn revisions() -> RuntimeRevisions {
        RuntimeRevisions {
            registry: "registry-1".to_string(),
            environment: "environment-1".to_string(),
            context: ContextSnapshotRevision::parse("context-v1-test").expect("context revision"),
        }
    }

    #[test]
    fn preview_generation_is_deterministic_and_contract_versioned() {
        let fingerprint = PreviewFingerprint {
            kind: MutationKind::Install,
            request_digest: stable_digest(&vec!["agent-a", "agent-b"]).expect("request digest"),
            revisions: revisions(),
            observed_state_digest: "observed-1".to_string(),
            planner_contract_version: 1,
        };
        let first = preview_token(&fingerprint).expect("token");
        let same = preview_token(&fingerprint).expect("same token");
        assert_eq!(first, same);

        let mut upgraded = fingerprint;
        upgraded.planner_contract_version = 2;
        assert_ne!(first, preview_token(&upgraded).expect("upgraded token"));
    }

    #[test]
    fn stable_digest_ignores_btree_insertion_order_but_not_semantic_changes() {
        let first = BTreeMap::from([("a", 1), ("b", 2)]);
        let mut reversed = BTreeMap::new();
        reversed.insert("b", 2);
        reversed.insert("a", 1);
        assert_eq!(
            stable_digest(&first).unwrap(),
            stable_digest(&reversed).unwrap()
        );
        reversed.insert("b", 3);
        assert_ne!(
            stable_digest(&first).unwrap(),
            stable_digest(&reversed).unwrap()
        );
    }

    #[test]
    fn observed_selection_accepts_only_a_previewed_subset() {
        let available = ["entry-a", "entry-b"]
            .into_iter()
            .map(|id| ObservedEntryId::parse(id).expect("entry ID"))
            .collect::<BTreeSet<_>>();
        let selected = [ObservedEntryId::parse("entry-b").unwrap()];
        assert!(validate_observed_subset(&available, &selected).is_ok());
        assert!(matches!(
            validate_observed_subset(&available, &[ObservedEntryId::parse("entry-c").unwrap()],),
            Err(AppError::StaleTarget)
        ));
    }
}

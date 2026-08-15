use std::collections::BTreeMap;

use serde::Serialize;
use uuid::Uuid;

use crate::application::mutation::plan::{
    preview_token, stable_digest, ExecutionUnit, ExpectedTargetEntry, MutationPlan,
    PreparedEntryMutation, PreviewFingerprint, PreviewToken, RuntimeRevisions,
};
use crate::application::payload_session::PinnedPayloadLease;
use crate::core::mutation::MutationKind;
use crate::core::skill_payload::PayloadId;
use crate::environment::runtime::ContextSnapshotRevision;
use crate::environment::types::SkillLocationRef;
use crate::error::AppError;
use crate::storage::lock_plan::PreparedLockMutation;

pub struct MutationPlanDraft {
    pub kind: MutationKind,
    pub payloads: BTreeMap<PayloadId, PinnedPayloadLease>,
    pub units: Vec<MutationUnitDraft>,
}

pub struct MutationUnitDraft {
    pub id: String,
    pub skill_name: String,
    pub source: Option<SkillLocationRef>,
    pub target: SkillLocationRef,
    pub expected_revisions: RuntimeRevisions,
    pub entries: PreparedMutationEntries,
    pub lock_mutation: Option<PreparedLockMutation>,
}

pub struct PreparedMutationEntries {
    pub canonical: Option<PreparedEntryMutation>,
    pub required_agents: Vec<PreparedEntryMutation>,
    pub expected_targets: Vec<ExpectedTargetEntry>,
}

pub fn assemble_plan(draft: MutationPlanDraft) -> MutationPlan {
    MutationPlan {
        kind: draft.kind,
        operation_id: Uuid::new_v4().simple().to_string(),
        payloads: draft.payloads,
        units: draft
            .units
            .into_iter()
            .map(|unit| ExecutionUnit {
                id: unit.id,
                skill_name: unit.skill_name,
                source: unit.source,
                target: unit.target,
                expected_revisions: unit.expected_revisions,
                canonical_entry: unit.entries.canonical,
                required_agent_entries: unit.entries.required_agents,
                lock_mutation: unit.lock_mutation,
                expected_targets: unit.entries.expected_targets,
            })
            .collect(),
    }
}

pub struct PreviewTokenDraft<'a, Request> {
    pub kind: MutationKind,
    pub request: &'a Request,
    pub revisions: RuntimeRevisions,
    pub observed_state_digest: String,
    pub planner_contract_version: u32,
}

pub fn issue_preview_token<Request>(
    draft: PreviewTokenDraft<'_, Request>,
) -> Result<PreviewToken, AppError>
where
    Request: Serialize,
{
    preview_token(&PreviewFingerprint {
        kind: draft.kind,
        request_digest: stable_digest(draft.request)?,
        revisions: draft.revisions,
        observed_state_digest: draft.observed_state_digest,
        planner_contract_version: draft.planner_contract_version,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviewScopeRevisions {
    registry: String,
    environment: String,
    context: ContextSnapshotRevision,
}

impl From<&PreviewToken> for PreviewScopeRevisions {
    fn from(token: &PreviewToken) -> Self {
        Self {
            registry: token.registry_revision.clone(),
            environment: token.environment_revision.clone(),
            context: token.context_revision.clone(),
        }
    }
}

pub fn validate_exact_preview(
    expected: &PreviewToken,
    actual: &PreviewToken,
) -> Result<(), AppError> {
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

pub fn validate_same_scope_revisions(
    expected: &PreviewScopeRevisions,
    actual: &PreviewScopeRevisions,
) -> Result<(), AppError> {
    if expected.registry != actual.registry {
        return Err(AppError::StaleRegistry);
    }
    if expected.environment != actual.environment {
        return Err(AppError::StaleEnvironment);
    }
    if expected.context != actual.context {
        return Err(AppError::StaleContext);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::fs;
    use std::path::Path;

    use super::{
        assemble_plan, issue_preview_token, validate_exact_preview, validate_same_scope_revisions,
        MutationPlanDraft, MutationUnitDraft, PreparedMutationEntries, PreviewScopeRevisions,
        PreviewTokenDraft,
    };
    use crate::application::mutation::plan::{
        ExpectedTargetEntry, PreparedEntryAction, PreparedEntryMutation, PreviewToken,
        RuntimeRevisions,
    };
    use crate::application::payload_session::{PayloadSessionLimits, PayloadSessionManager};
    use crate::core::agent_definition::AgentId;
    use crate::core::lossless_lock::LockSchema;
    use crate::core::mutation::MutationKind;
    use crate::core::skill_payload::build_skill_payload;
    use crate::environment::runtime::{
        ContextSnapshotRevision, EntryFingerprint, ExecutionBackend, PhysicalParentIdentity,
        PhysicalTargetKey,
    };
    use crate::environment::types::{
        EnvironmentRef, ResourceLocator, SkillLocation, SkillLocationRef,
    };
    use crate::error::AppError;
    use crate::models::InstallMode;
    use crate::storage::lock_plan::{LockExpectedState, PreparedLockMutation};
    use tempfile::tempdir;

    fn token(registry: &str, environment: &str, context: &str, generation: &str) -> PreviewToken {
        PreviewToken {
            generation: generation.to_string(),
            registry_revision: registry.to_string(),
            environment_revision: environment.to_string(),
            context_revision: ContextSnapshotRevision::parse(context).expect("context revision"),
        }
    }

    #[test]
    fn exact_preview_validation_preserves_stale_error_priority() {
        let expected = token("registry-1", "environment-1", "context-1", "generation-1");
        assert_eq!(
            validate_exact_preview(
                &expected,
                &token("registry-2", "environment-2", "context-2", "generation-2")
            ),
            Err(AppError::StaleRegistry)
        );
        assert_eq!(
            validate_exact_preview(
                &expected,
                &token("registry-1", "environment-2", "context-2", "generation-2")
            ),
            Err(AppError::StaleEnvironment)
        );
        assert_eq!(
            validate_exact_preview(
                &expected,
                &token("registry-1", "environment-1", "context-2", "generation-2")
            ),
            Err(AppError::StaleContext)
        );
        assert_eq!(
            validate_exact_preview(
                &expected,
                &token("registry-1", "environment-1", "context-1", "generation-2")
            ),
            Err(AppError::StaleContext)
        );
        assert_eq!(validate_exact_preview(&expected, &expected), Ok(()));
    }

    #[test]
    fn matching_scope_revisions_ignore_generation_but_do_not_authorize_an_update() {
        let expected_token = token("registry-1", "environment-1", "context-1", "generation-1");
        let expected = PreviewScopeRevisions::from(&expected_token);
        let changed_generation = PreviewScopeRevisions::from(&token(
            "registry-1",
            "environment-1",
            "context-1",
            "generation-2",
        ));
        assert_eq!(
            validate_same_scope_revisions(&expected, &changed_generation),
            Ok(())
        );

        assert_eq!(
            validate_same_scope_revisions(
                &expected,
                &PreviewScopeRevisions::from(&token(
                    "registry-2",
                    "environment-2",
                    "context-2",
                    "generation-2",
                )),
            ),
            Err(AppError::StaleRegistry)
        );
        assert_eq!(
            validate_same_scope_revisions(
                &expected,
                &PreviewScopeRevisions::from(&token(
                    "registry-1",
                    "environment-2",
                    "context-2",
                    "generation-2",
                )),
            ),
            Err(AppError::StaleEnvironment)
        );
        assert_eq!(
            validate_same_scope_revisions(
                &expected,
                &PreviewScopeRevisions::from(&token(
                    "registry-1",
                    "environment-1",
                    "context-2",
                    "generation-2",
                )),
            ),
            Err(AppError::StaleContext)
        );
    }

    #[test]
    fn issued_preview_token_preserves_the_existing_byte_contract() {
        let request = vec!["agent-a", "agent-b"];
        let draft = PreviewTokenDraft {
            kind: MutationKind::Install,
            request: &request,
            revisions: RuntimeRevisions {
                registry: "registry-1".to_string(),
                environment: "environment-1".to_string(),
                context: ContextSnapshotRevision::parse("context-v1-test")
                    .expect("context revision"),
            },
            observed_state_digest: "observed-1".to_string(),
            planner_contract_version: 1,
        };

        let token = issue_preview_token(draft).expect("preview token");
        assert_eq!(
            token.generation,
            "preview-v1-0d07b302635f5782a8ba47649a7daa8c5a0676723246bf0ec071c5de4d00bfb7"
        );
        assert_eq!(token.registry_revision, "registry-1");
        assert_eq!(token.environment_revision, "environment-1");
        assert_eq!(token.context_revision.as_str(), "context-v1-test");

        let upgraded = issue_preview_token(PreviewTokenDraft {
            planner_contract_version: 2,
            ..PreviewTokenDraft {
                kind: MutationKind::Install,
                request: &request,
                revisions: RuntimeRevisions {
                    registry: "registry-1".to_string(),
                    environment: "environment-1".to_string(),
                    context: ContextSnapshotRevision::parse("context-v1-test")
                        .expect("context revision"),
                },
                observed_state_digest: "observed-1".to_string(),
                planner_contract_version: 1,
            }
        })
        .expect("upgraded preview token");
        assert_ne!(token.generation, upgraded.generation);
    }

    fn native_global() -> SkillLocationRef {
        SkillLocationRef {
            environment: EnvironmentRef::Native,
            scope: SkillLocation::Global,
        }
    }

    fn unit(id: &str, skill_name: &str) -> MutationUnitDraft {
        MutationUnitDraft {
            id: id.to_string(),
            skill_name: skill_name.to_string(),
            source: None,
            target: native_global(),
            expected_revisions: RuntimeRevisions {
                registry: "registry-1".to_string(),
                environment: "environment-1".to_string(),
                context: ContextSnapshotRevision::parse("context-v1-test")
                    .expect("context revision"),
            },
            entries: PreparedMutationEntries {
                canonical: None,
                required_agents: Vec::new(),
                expected_targets: Vec::new(),
            },
            lock_mutation: None,
        }
    }

    #[test]
    fn plan_assembly_preserves_empty_plan_semantics() {
        let empty = assemble_plan(MutationPlanDraft {
            kind: MutationKind::Copy,
            payloads: Default::default(),
            units: Vec::new(),
        });
        assert_eq!(empty.kind, MutationKind::Copy);
        assert!(!empty.operation_id.is_empty());
        assert!(empty.payloads.is_empty());
        assert!(empty.units.is_empty());
    }

    fn key(name: &str) -> PhysicalTargetKey {
        let (backend, physical_parent) = if cfg!(windows) {
            (
                ExecutionBackend::NativeWindows,
                PhysicalParentIdentity::Windows {
                    volume_serial: 1,
                    file_id: 2,
                },
            )
        } else {
            (
                ExecutionBackend::NativeUnix,
                PhysicalParentIdentity::Unix {
                    device: 1,
                    inode: 2,
                },
            )
        };
        PhysicalTargetKey {
            backend,
            physical_parent,
            normalized_final_child_name: name.to_string(),
        }
    }

    fn destination(path: &Path) -> ResourceLocator {
        ResourceLocator {
            environment: EnvironmentRef::Native,
            native_path: path.to_string_lossy().into_owned(),
        }
    }

    #[tokio::test]
    async fn plan_assembly_preserves_payloads_units_and_recovery_inputs() {
        let temp = tempdir().expect("tempdir");
        let manager = PayloadSessionManager::in_memory(
            PayloadSessionLimits {
                ttl_ms: 60_000,
                max_sessions: 4,
                max_bytes: 1_000_000,
            },
            || 1_000,
        );
        let discovery = manager
            .discover(EnvironmentRef::Native, "planning-contract")
            .await
            .expect("discovery");
        let mut payloads = BTreeMap::new();
        for name in ["alpha", "beta"] {
            let root = temp.path().join(name);
            fs::create_dir_all(&root).expect("payload root");
            fs::write(
                root.join("SKILL.md"),
                format!("---\nname: {name}\n---\nbody"),
            )
            .expect("skill");
            let handle = manager
                .acquire_payload(
                    &discovery,
                    format!("skills/{name}"),
                    build_skill_payload(&root).expect("payload"),
                )
                .await
                .expect("acquire payload");
            let lease = manager.pin_verified(&handle).await.expect("pin payload");
            payloads.insert(lease.manifest().payload_id().clone(), lease);
        }

        let canonical_destination = temp.path().join("skills").join("alpha");
        let required_agent_destination = temp.path().join("codex").join("skills").join("alpha");
        let lock_target = temp
            .path()
            .join("target")
            .join(".agents")
            .join(".skill-lock.json");
        let canonical = PreparedEntryMutation {
            key: key("alpha"),
            destination: destination(&canonical_destination),
            action: PreparedEntryAction::Replace {
                payload_id: payloads.keys().next().expect("payload ID").clone(),
                requested_mode: InstallMode::Copy,
            },
            owner_agent_ids: vec![AgentId::parse("codex").expect("Agent ID")],
        };
        let expected_target = ExpectedTargetEntry {
            key: canonical.key.clone(),
            fingerprint: EntryFingerprint("fingerprint-alpha".to_string()),
            expected_content_manifest_hash: None,
        };
        let required_agent = PreparedEntryMutation {
            key: key("alpha-codex"),
            destination: destination(&required_agent_destination),
            action: canonical.action.clone(),
            owner_agent_ids: vec![AgentId::parse("codex").expect("Agent ID")],
        };
        let source = SkillLocationRef {
            environment: EnvironmentRef::Native,
            scope: SkillLocation::Project {
                project_id: "source-project".to_string(),
            },
        };
        let target = SkillLocationRef {
            environment: EnvironmentRef::Native,
            scope: SkillLocation::Project {
                project_id: "target-project".to_string(),
            },
        };
        let lock_mutation = PreparedLockMutation {
            target: destination(&lock_target),
            legacy_target: None,
            schema: LockSchema::Project,
            entry: crate::storage::lock_plan::LockEntryMutation::Replace {
                key: "alpha".to_string(),
                replacement: serde_json::json!({ "source": "owner/repo" }),
            },
            root_replacements: BTreeMap::new(),
            expected: LockExpectedState {
                entry_snapshots: BTreeMap::new(),
                root_snapshots: BTreeMap::new(),
            },
        };
        let mut first = unit("copy:alpha:target-project", "alpha");
        first.source = Some(source.clone());
        first.target = target.clone();
        first.entries = PreparedMutationEntries {
            canonical: Some(canonical.clone()),
            required_agents: vec![required_agent.clone()],
            expected_targets: vec![expected_target.clone()],
        };
        first.lock_mutation = Some(lock_mutation);

        let plan = assemble_plan(MutationPlanDraft {
            kind: MutationKind::Repair,
            payloads,
            units: vec![first, unit("install:beta", "beta")],
        });

        assert_eq!(plan.kind, MutationKind::Repair);
        assert_eq!(plan.operation_id.len(), 32);
        assert!(plan
            .operation_id
            .chars()
            .all(|value| value.is_ascii_hexdigit()));
        assert_eq!(plan.payloads.len(), 2);
        assert_eq!(
            plan.units
                .iter()
                .map(|unit| unit.id.as_str())
                .collect::<Vec<_>>(),
            vec!["copy:alpha:target-project", "install:beta"]
        );
        assert_eq!(plan.units[0].skill_name, "alpha");
        assert_eq!(plan.units[0].source, Some(source));
        assert_eq!(plan.units[0].target, target);
        assert_eq!(plan.units[0].expected_revisions.registry, "registry-1");
        assert_eq!(
            plan.units[0].expected_revisions.context.as_str(),
            "context-v1-test"
        );
        assert_eq!(plan.units[0].canonical_entry, Some(canonical));
        assert_eq!(plan.units[0].required_agent_entries, vec![required_agent]);
        assert_eq!(plan.units[0].expected_targets, vec![expected_target]);
        let prepared_lock = plan.units[0].lock_mutation.as_ref().expect("lock mutation");
        assert_eq!(
            Path::new(&prepared_lock.target.native_path)
                .components()
                .collect::<Vec<_>>(),
            lock_target.components().collect::<Vec<_>>()
        );
        assert_eq!(prepared_lock.schema, LockSchema::Project);
        assert_eq!(prepared_lock.skill_name(), "alpha");
        assert_eq!(
            prepared_lock.replacement(),
            Some(&serde_json::json!({ "source": "owner/repo" }))
        );
    }
}

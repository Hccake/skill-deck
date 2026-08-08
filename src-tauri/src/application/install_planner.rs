use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use serde_json::{json, Map, Value};
use uuid::Uuid;

use crate::application::agent_selection::{
    build_agent_selection_catalog, resolve_agent_selection_submission, AgentSelectionResolution,
    InstallAgentSelectionSnapshot,
};
use crate::application::install::{
    InstallFuture, InstallPlanner, InstallPreview, InstallPreviewOutcome, InstallRequest,
    InstallSkillPreview,
};
use crate::application::mutation::plan::{
    group_physical_mutations, preview_token, stable_digest, ExecutionUnit, ExpectedTargetEntry,
    MutationPlan, PreparedEntryAction, PreparedEntryMutation, PreviewFingerprint, PreviewToken,
    RuntimeRevisions,
};
use crate::application::payload_session::{PayloadSessionManager, PinnedPayloadLease};
use crate::application::workflow_planner::AgentEntryPlan;
use crate::core::lossless_lock::{LockSchema, LosslessLockDocument};
use crate::core::mutation::MutationKind;
use crate::core::skill_payload::{validate_manifest_for_target, PayloadId, TargetPathProfile};
use crate::environment::agent_environment::AgentRuntimeSnapshot;
use crate::environment::context_resolver::ResolvedContext;
use crate::environment::planning::{ResolvedTargetFact, TargetFactResolver};
use crate::environment::runtime::ExecutionBackend;
use crate::environment::types::{
    same_environment_identity, EnvironmentRef, ResourceLocator, SkillLocationRef,
};
use crate::error::AppError;
use crate::models::InstallMode;
use crate::models::InstallTargetInfo;
use crate::storage::lock_plan::{LockExpectedState, PreparedLockMutation};

#[derive(Clone)]
pub struct InstallPlanningFacts {
    pub resolved_context: ResolvedContext,
    pub agent_runtime: AgentRuntimeSnapshot,
    pub revisions: RuntimeRevisions,
    pub lock_schema: LockSchema,
    pub lock_document: LosslessLockDocument,
    pub eve_targets: Vec<InstallTargetInfo>,
}

pub trait InstallPlanningFactSource: Send + Sync {
    fn current<'a>(
        &'a self,
        context: &'a SkillLocationRef,
    ) -> InstallFuture<'a, Result<InstallPlanningFacts, AppError>>;

    fn current_for_copy_source<'a>(
        &'a self,
        context: &'a SkillLocationRef,
    ) -> InstallFuture<'a, Result<InstallPlanningFacts, AppError>> {
        self.current(context)
    }
}

pub struct ConcreteInstallPlanner<F, T> {
    facts: F,
    targets: T,
    payloads: Arc<PayloadSessionManager>,
    now: Arc<dyn Fn() -> String + Send + Sync>,
}

impl<F, T> ConcreteInstallPlanner<F, T> {
    pub fn new(
        facts: F,
        targets: T,
        payloads: Arc<PayloadSessionManager>,
        now: impl Fn() -> String + Send + Sync + 'static,
    ) -> Self {
        Self {
            facts,
            targets,
            payloads,
            now: Arc::new(now),
        }
    }
}

impl<F, T> InstallPlanner for ConcreteInstallPlanner<F, T>
where
    F: InstallPlanningFactSource,
    T: TargetFactResolver,
{
    fn preview<'a>(
        &'a self,
        request: &'a InstallRequest,
        payloads: Vec<PinnedPayloadLease>,
    ) -> InstallFuture<'a, Result<InstallPreviewOutcome, AppError>> {
        Box::pin(async move {
            match self.build(request, payloads, false).await? {
                BuiltInstallOutcome::Ready(built) => Ok(InstallPreviewOutcome::Ready {
                    preview: built.preview,
                }),
                BuiltInstallOutcome::SelectionStale(snapshot) => {
                    Ok(InstallPreviewOutcome::SelectionStale { snapshot })
                }
            }
        })
    }

    fn rebuild<'a>(
        &'a self,
        request: &'a InstallRequest,
        payloads: Vec<PinnedPayloadLease>,
    ) -> InstallFuture<'a, Result<(PreviewToken, MutationPlan), AppError>> {
        Box::pin(async move {
            let BuiltInstallOutcome::Ready(built) = self.build(request, payloads, true).await?
            else {
                return Err(AppError::StaleTarget);
            };
            Ok((
                built.preview.token,
                built.plan.expect("execute build produces a plan"),
            ))
        })
    }
}

struct BuiltInstall {
    preview: InstallPreview,
    plan: Option<MutationPlan>,
}

enum BuiltInstallOutcome {
    Ready(BuiltInstall),
    SelectionStale(InstallAgentSelectionSnapshot),
}

struct SkillSeed {
    canonical_payload_index: usize,
    eve_payload_index: Option<usize>,
    fact_start: usize,
    fact_count: usize,
}

impl<F, T> ConcreteInstallPlanner<F, T>
where
    F: InstallPlanningFactSource,
    T: TargetFactResolver,
{
    async fn build(
        &self,
        request: &InstallRequest,
        payloads: Vec<PinnedPayloadLease>,
        include_plan: bool,
    ) -> Result<BuiltInstallOutcome, AppError> {
        let facts = self.facts.current(&request.context).await?;
        validate_facts(request, &facts, &payloads)?;
        let catalog = build_agent_selection_catalog(
            &request.context,
            &facts.agent_runtime,
            &facts.eve_targets,
            &self.targets,
        )
        .await?;
        let resolved_selection =
            match resolve_agent_selection_submission(&catalog, &request.agent_selection)? {
                AgentSelectionResolution::Ready(selection) => selection,
                AgentSelectionResolution::Stale => {
                    return Ok(BuiltInstallOutcome::SelectionStale(
                        InstallAgentSelectionSnapshot {
                            selection: catalog.snapshot,
                            default_selection_warning: None,
                        },
                    ));
                }
            };
        let agent_plan = resolved_selection.entry_plan(true);
        let has_eve_targets = agent_plan
            .required_agent_roots
            .iter()
            .any(|target| target.content.uses_eve_payload());

        let canonical_payload_count = payloads.len();
        let mut payloads = payloads;
        let mut logical_destinations = Vec::new();
        let mut seeds = Vec::with_capacity(canonical_payload_count);
        for canonical_payload_index in 0..canonical_payload_count {
            let eve_payload_index = if has_eve_targets {
                let derived = {
                    let canonical = &payloads[canonical_payload_index];
                    let payload = canonical.load_payload().await?;
                    let derived = crate::core::eve::derive_eve_skill_payload(&payload)?;
                    self.payloads
                        .pin_derived_payload(canonical, "eve", derived)
                        .await?
                };
                let index = payloads.len();
                payloads.push(derived);
                Some(index)
            } else {
                None
            };
            let payload = &payloads[canonical_payload_index];
            let metadata = payload.planning_metadata();
            let start = logical_destinations.len();
            logical_destinations.push(join_entry(
                &facts.resolved_context.skill_root,
                &metadata.install_dir_name,
            ));
            logical_destinations.extend(
                agent_plan
                    .required_agent_roots
                    .iter()
                    .map(|target| join_entry(&target.root, &metadata.install_dir_name)),
            );
            seeds.push(SkillSeed {
                canonical_payload_index,
                eve_payload_index,
                fact_start: start,
                fact_count: 1 + agent_plan.required_agent_roots.len(),
            });
        }
        let target_facts = self
            .targets
            .resolve(&request.context, &logical_destinations, None)
            .await?;
        if target_facts.len() != logical_destinations.len() {
            return Err(AppError::StaleTarget);
        }
        validate_payload_targets(&payloads, &seeds, &agent_plan, &target_facts).await?;

        let observed_state_digest =
            observed_digest(&target_facts, &facts, &payloads[..canonical_payload_count])?;
        let fingerprint = PreviewFingerprint {
            kind: MutationKind::Install,
            request_digest: stable_digest(request)?,
            revisions: facts.revisions.clone(),
            observed_state_digest,
            planner_contract_version: 1,
        };
        let token = preview_token(&fingerprint)?;
        let mut preview_skills = Vec::with_capacity(seeds.len());
        let mut units = Vec::with_capacity(seeds.len());
        for seed in &seeds {
            let payload = &payloads[seed.canonical_payload_index];
            let metadata = payload.planning_metadata();
            let slice = &target_facts[seed.fact_start..seed.fact_start + seed.fact_count];
            preview_skills.push(InstallSkillPreview {
                skill_name: metadata.skill_name.clone(),
                payload: request.payloads[seed.canonical_payload_index].clone(),
                overwrite_targets: slice
                    .iter()
                    .filter(|fact| fact.fingerprint.0 != "entry-v1-missing")
                    .map(|fact| fact.destination.native_path.clone())
                    .collect(),
                blocking_reasons: Vec::new(),
                fallback_forecasts: Vec::new(),
            });
            if include_plan {
                units.push(build_unit(
                    request,
                    &facts,
                    &agent_plan,
                    payload,
                    seed.eve_payload_index.map(|index| &payloads[index]),
                    slice,
                    (self.now)(),
                )?);
            }
        }
        let preview = InstallPreview {
            token,
            skills: preview_skills,
        };
        let plan = include_plan.then(|| MutationPlan {
            kind: MutationKind::Install,
            operation_id: Uuid::new_v4().simple().to_string(),
            payloads: payloads
                .into_iter()
                .map(|lease| (lease.manifest().payload_id().clone(), lease))
                .collect::<BTreeMap<PayloadId, PinnedPayloadLease>>(),
            units,
        });
        Ok(BuiltInstallOutcome::Ready(BuiltInstall { preview, plan }))
    }
}

fn validate_facts(
    request: &InstallRequest,
    facts: &InstallPlanningFacts,
    payloads: &[PinnedPayloadLease],
) -> Result<(), AppError> {
    if facts.resolved_context.context != request.context
        || !same_environment_identity(
            &facts.agent_runtime.environment,
            &request.context.environment,
        )
        || facts.agent_runtime.registry_revision != facts.revisions.registry
        || facts.agent_runtime.environment_revision != facts.revisions.environment
        || payloads.len() != request.skills.len()
        || payloads.iter().enumerate().any(|(index, payload)| {
            payload.planning_metadata().validate().is_err()
                || payload.planning_metadata().skill_name != request.skills[index]
        })
    {
        return Err(AppError::StaleContext);
    }
    Ok(())
}

async fn validate_payload_targets(
    payloads: &[PinnedPayloadLease],
    seeds: &[SkillSeed],
    agent_plan: &AgentEntryPlan,
    facts: &[ResolvedTargetFact],
) -> Result<(), AppError> {
    for seed in seeds {
        let canonical = payloads[seed.canonical_payload_index]
            .load_payload()
            .await?;
        let eve = match seed.eve_payload_index {
            Some(index) => Some(payloads[index].load_payload().await?),
            None => None,
        };
        for (offset, fact) in facts[seed.fact_start..seed.fact_start + seed.fact_count]
            .iter()
            .enumerate()
        {
            let payload = if offset > 0
                && agent_plan.required_agent_roots[offset - 1]
                    .content
                    .uses_eve_payload()
            {
                eve.as_ref().ok_or(AppError::StalePayload)?
            } else {
                &canonical
            };
            let profile = match fact.key.backend {
                ExecutionBackend::NativeWindows => TargetPathProfile::native_windows(),
                ExecutionBackend::NativeUnix | ExecutionBackend::WslPosix { .. } => {
                    TargetPathProfile::native_unix()
                }
            };
            validate_manifest_for_target(payload, &fact.destination, &profile)?;
        }
    }
    Ok(())
}

fn build_unit(
    request: &InstallRequest,
    facts: &InstallPlanningFacts,
    agent_plan: &AgentEntryPlan,
    payload: &PinnedPayloadLease,
    eve_payload: Option<&PinnedPayloadLease>,
    targets: &[ResolvedTargetFact],
    now: String,
) -> Result<ExecutionUnit, AppError> {
    let metadata = payload.planning_metadata();
    let payload_id = payload.manifest().payload_id().clone();
    let canonical = PreparedEntryMutation {
        key: targets[0].key.clone(),
        destination: targets[0].destination.clone(),
        action: PreparedEntryAction::Replace {
            payload_id: payload_id.clone(),
            requested_mode: InstallMode::Copy,
        },
        owner_agent_ids: agent_plan.canonical_owner_agent_ids.clone(),
    };
    let required = agent_plan
        .required_agent_roots
        .iter()
        .zip(&targets[1..])
        .map(|(logical, target)| PreparedEntryMutation {
            key: target.key.clone(),
            destination: target.destination.clone(),
            action: PreparedEntryAction::Replace {
                payload_id: if logical.content.uses_eve_payload() {
                    eve_payload
                        .expect("Eve target planning pins a derived payload")
                        .manifest()
                        .payload_id()
                        .clone()
                } else {
                    payload_id.clone()
                },
                requested_mode: if logical.content.uses_eve_payload() {
                    InstallMode::Copy
                } else {
                    request.agent_selection.requested_mode.clone()
                },
            },
            owner_agent_ids: logical.owner_agent_ids.clone(),
        })
        .collect::<Vec<_>>();
    let grouped =
        group_physical_mutations(std::iter::once(canonical.clone()).chain(required).collect())?;
    let canonical_entry = grouped
        .iter()
        .find(|entry| entry.key == canonical.key)
        .cloned();
    let required_agent_entries = grouped
        .into_iter()
        .filter(|entry| entry.key != canonical.key)
        .collect::<Vec<_>>();
    let expected_targets = targets
        .iter()
        .map(|target| ExpectedTargetEntry {
            key: target.key.clone(),
            fingerprint: target.fingerprint.clone(),
            expected_content_manifest_hash: None,
        })
        .collect();
    Ok(ExecutionUnit {
        id: format!("install:{}", metadata.install_dir_name),
        skill_name: metadata.skill_name.clone(),
        source: None,
        target: request.context.clone(),
        expected_revisions: facts.revisions.clone(),
        canonical_entry,
        required_agent_entries,
        lock_mutation: Some(lock_mutation(facts, metadata, agent_plan, now)?),
        expected_targets,
    })
}

fn lock_mutation(
    facts: &InstallPlanningFacts,
    metadata: &crate::application::payload_session::PayloadPlanningMetadata,
    agent_plan: &AgentEntryPlan,
    now: String,
) -> Result<PreparedLockMutation, AppError> {
    let expected = LockExpectedState::capture(
        &facts.lock_document,
        [&metadata.install_dir_name],
        std::iter::empty::<&str>(),
    );
    let existing = facts
        .lock_document
        .entry_snapshot(&metadata.install_dir_name)
        .value()
        .cloned();
    let replacement = match facts.lock_schema {
        LockSchema::Global => {
            let installed_at = existing
                .as_ref()
                .and_then(|entry| entry.get("installedAt"))
                .and_then(Value::as_str)
                .unwrap_or(&now)
                .to_string();
            json!({
                "source": metadata.source,
                "sourceType": metadata.source_type,
                "sourceUrl": metadata.source_url,
                "ref": metadata.ref_name,
                "skillPath": metadata.skill_path,
                "skillFolderHash": metadata.global_skill_folder_hash(),
                "installedAt": installed_at,
                "updatedAt": now,
                "pluginName": metadata.plugin_name,
            })
        }
        LockSchema::Project => {
            let mut entry = Map::new();
            entry.insert("source".to_string(), json!(metadata.source));
            entry.insert("sourceType".to_string(), json!(metadata.source_type));
            entry.insert("sourceUrl".to_string(), json!(metadata.source_url));
            entry.insert("ref".to_string(), json!(metadata.ref_name));
            entry.insert("skillPath".to_string(), json!(metadata.skill_path));
            entry.insert("computedHash".to_string(), json!(metadata.computed_hash));
            entry.insert("pluginName".to_string(), json!(metadata.plugin_name));
            if let Some(remote_hash) = &metadata.upstream_revision {
                entry.insert("remoteHash".to_string(), json!(remote_hash));
            }
            let eve_subagents = agent_plan
                .required_agent_roots
                .iter()
                .filter_map(|target| target.content.eve_subagent())
                .map(|subagent| subagent.unwrap_or(""))
                .collect::<BTreeSet<_>>();
            if eve_subagents.iter().any(|target| !target.is_empty()) {
                entry.insert("subagents".to_string(), json!(eve_subagents));
            }
            Value::Object(entry)
        }
    };
    Ok(PreparedLockMutation {
        target: facts.resolved_context.lock.clone(),
        legacy_target: None,
        schema: facts.lock_schema,
        skill_name: metadata.install_dir_name.clone(),
        replacement: Some(replacement),
        root_replacements: BTreeMap::new(),
        expected,
    })
}

fn observed_digest(
    target_facts: &[ResolvedTargetFact],
    facts: &InstallPlanningFacts,
    payloads: &[PinnedPayloadLease],
) -> Result<String, AppError> {
    let lock_entries = payloads
        .iter()
        .map(|payload| {
            let name = &payload.planning_metadata().install_dir_name;
            (
                name,
                facts.lock_document.entry_snapshot(name).value().cloned(),
            )
        })
        .collect::<Vec<_>>();
    let targets = target_facts
        .iter()
        .map(|target| (&target.key, &target.fingerprint))
        .collect::<Vec<_>>();
    stable_digest(&(targets, lock_entries))
}

fn join_entry(root: &ResourceLocator, child: &str) -> ResourceLocator {
    let native_path = match root.environment {
        EnvironmentRef::Native if cfg!(windows) => format!(
            "{}\\{}",
            root.native_path.trim_end_matches(['/', '\\']),
            child
        ),
        _ => format!(
            "{}/{}",
            root.native_path.trim_end_matches(['/', '\\']),
            child
        ),
    };
    ResourceLocator {
        environment: root.environment.clone(),
        native_path,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::fs;
    use std::sync::Arc;

    use tempfile::tempdir;

    use super::*;
    use crate::application::agent_selection::test_submission_for_agents;
    use crate::application::install::{InstallPlanner, InstallRequest};
    use crate::application::mutation::plan::RuntimeRevisions;
    use crate::application::payload_session::{
        PayloadPlanningMetadata, PayloadSessionLimits, PayloadSessionManager,
    };
    use crate::core::agent_definition::{
        AgentAdapter, AgentDefinition, AgentId, AgentSource, DetectionSpec, PathSpec,
        ScopeDefinition,
    };
    use crate::core::lossless_lock::{LockSchema, LosslessLockDocument};
    use crate::core::skill_payload::build_skill_payload;
    use crate::environment::agent_environment::{
        AgentRuntimeSnapshot, DetectionState, ResolvedAgent, ResolvedAgentScope,
    };
    use crate::environment::context_resolver::ResolvedContext;
    use crate::environment::planning::RuntimeTargetFactResolver;
    use crate::environment::runtime::ContextSnapshotRevision;
    use crate::environment::types::{
        EnvironmentRef, EnvironmentStatus, ResourceLocator, SkillLocation, SkillLocationRef,
    };
    use crate::environment::wsl::WslRuntime;
    use crate::models::InstallMode;

    struct Facts(InstallPlanningFacts);

    impl InstallPlanningFactSource for Facts {
        fn current<'a>(
            &'a self,
            _context: &'a SkillLocationRef,
        ) -> crate::application::install::InstallFuture<
            'a,
            Result<InstallPlanningFacts, crate::error::AppError>,
        > {
            Box::pin(async move { Ok(self.0.clone()) })
        }
    }

    #[tokio::test]
    async fn planner_builds_canonical_private_and_lock_as_one_unit() {
        let temp = tempdir().unwrap();
        let physical_root = fs::canonicalize(temp.path()).unwrap();
        let canonical_root = physical_root.join(".agents/skills");
        fs::create_dir_all(&canonical_root).unwrap();
        let source = temp.path().join("source/demo");
        fs::create_dir_all(&source).unwrap();
        fs::write(source.join("SKILL.md"), b"---\nname: demo\n---\nbody").unwrap();
        let payload = build_skill_payload(&source).unwrap();
        let payload_id = payload.payload_id.clone();
        let manager = Arc::new(PayloadSessionManager::in_memory(
            PayloadSessionLimits {
                ttl_ms: 60_000,
                max_sessions: 4,
                max_bytes: 1_000_000,
            },
            || 1_000,
        ));
        let discovery = manager
            .discover(EnvironmentRef::Native, "source-fingerprint")
            .await
            .unwrap();
        let handle = manager
            .acquire_payload_with_metadata(
                &discovery,
                "skills/demo",
                payload,
                PayloadPlanningMetadata {
                    skill_name: "demo".to_string(),
                    install_dir_name: "demo".to_string(),
                    source: "/tmp/local-skills".to_string(),
                    source_type: "local".to_string(),
                    source_url: None,
                    ref_name: None,
                    skill_path: "skills/demo".to_string(),
                    plugin_name: None,
                    computed_hash: "cli-computed-hash".to_string(),
                    upstream_revision: None,
                },
            )
            .await
            .unwrap();
        let context = SkillLocationRef {
            environment: EnvironmentRef::Native,
            scope: SkillLocation::Global,
        };
        let private_root = physical_root.join(".custom/skills");
        let facts = InstallPlanningFacts {
            resolved_context: ResolvedContext {
                context: context.clone(),
                project: None,
                home: locator(&physical_root),
                skill_root: locator(&canonical_root),
                lock: locator(&physical_root.join(".agents/.skill-lock.json")),
            },
            agent_runtime: runtime(private_root.to_string_lossy().as_ref()),
            revisions: RuntimeRevisions {
                registry: "registry-1".to_string(),
                environment: "environment-1".to_string(),
                context: ContextSnapshotRevision::parse("context-v1-install").unwrap(),
            },
            lock_schema: LockSchema::Global,
            lock_document: LosslessLockDocument::empty(LockSchema::Global),
            eve_targets: Vec::new(),
        };
        let target_resolver = RuntimeTargetFactResolver::new(Arc::new(WslRuntime::default()));
        let agent_selection = test_submission_for_agents(
            &context,
            &facts.agent_runtime,
            &facts.eve_targets,
            &target_resolver,
            &["custom-private"],
            InstallMode::Symlink,
        )
        .await;
        let planner = ConcreteInstallPlanner::new(
            Facts(facts),
            target_resolver,
            Arc::clone(&manager),
            || "2026-07-18T00:00:00.000Z".to_string(),
        );
        let request = InstallRequest {
            context,
            source: "owner/repo".to_string(),
            discovery_session: discovery,
            payloads: vec![handle.clone()],
            skills: vec!["demo".to_string()],
            agent_selection,
            acknowledge_risk: true,
        };

        let preview_lease = manager.pin_verified(&handle).await.unwrap();
        let preview = planner
            .preview(&request, vec![preview_lease])
            .await
            .unwrap();
        let execute_lease = manager.pin_verified(&handle).await.unwrap();
        let (token, plan) = planner
            .rebuild(&request, vec![execute_lease])
            .await
            .unwrap();

        let InstallPreviewOutcome::Ready { preview } = preview else {
            panic!("expected ready preview");
        };
        assert_eq!(preview.token, token);
        assert_eq!(preview.skills.len(), 1);
        assert_eq!(plan.payloads.len(), 1);
        assert_eq!(plan.units.len(), 1);
        let unit = &plan.units[0];
        let canonical = unit.canonical_entry.as_ref().unwrap();
        assert_eq!(
            canonical.destination.native_path,
            canonical_root.join("demo").to_string_lossy()
        );
        assert!(canonical.owner_agent_ids.is_empty());
        assert_eq!(unit.required_agent_entries.len(), 1);
        assert_eq!(
            unit.required_agent_entries[0].destination.native_path,
            private_root.join("demo").to_string_lossy()
        );
        assert!(matches!(
            unit.required_agent_entries[0].action,
            crate::application::mutation::plan::PreparedEntryAction::Replace {
                requested_mode: InstallMode::Symlink,
                ..
            }
        ));
        let lock = unit.lock_mutation.as_ref().unwrap();
        assert_eq!(lock.skill_name, "demo");
        assert_eq!(
            lock.replacement.as_ref().unwrap()["skillFolderHash"],
            "cli-computed-hash"
        );
        assert_ne!(payload_id.as_str(), "cli-computed-hash");
    }

    #[tokio::test]
    async fn eve_targets_use_a_derived_payload_without_changing_canonical_lock_hashes() {
        let temp = tempdir().unwrap();
        let project = temp.path().join("project");
        let canonical_root = project.join(".agents/skills");
        let source = temp.path().join("source/demo");
        fs::create_dir_all(source.join("scripts")).unwrap();
        fs::create_dir_all(&canonical_root).unwrap();
        fs::write(
            source.join("SKILL.md"),
            "---\nname: demo\ndescription: Demo\n---\n# Demo\n",
        )
        .unwrap();
        fs::write(source.join("scripts/run.sh"), "#!/bin/sh\necho demo\n").unwrap();
        let payload = build_skill_payload(&source).unwrap();
        let canonical_payload_id = payload.payload_id.clone();
        let manager = Arc::new(PayloadSessionManager::in_memory(
            PayloadSessionLimits {
                ttl_ms: 60_000,
                max_sessions: 4,
                max_bytes: 1_000_000,
            },
            || 1_000,
        ));
        let discovery = manager
            .discover(EnvironmentRef::Native, "source-fingerprint")
            .await
            .unwrap();
        let handle = manager
            .acquire_payload_with_metadata(
                &discovery,
                "skills/demo",
                payload,
                PayloadPlanningMetadata {
                    skill_name: "demo".to_string(),
                    install_dir_name: "demo".to_string(),
                    source: "owner/repo".to_string(),
                    source_type: "github".to_string(),
                    source_url: Some("https://github.com/owner/repo.git".to_string()),
                    ref_name: Some("main".to_string()),
                    skill_path: "skills/demo".to_string(),
                    plugin_name: None,
                    computed_hash: "canonical-computed-hash".to_string(),
                    upstream_revision: Some("canonical-tree-hash".to_string()),
                },
            )
            .await
            .unwrap();
        let context = SkillLocationRef {
            environment: EnvironmentRef::Native,
            scope: SkillLocation::Project {
                project_id: "project-1".to_string(),
            },
        };
        let facts = InstallPlanningFacts {
            resolved_context: ResolvedContext {
                context: context.clone(),
                project: Some(crate::environment::types::RegisteredProject {
                    id: "project-1".to_string(),
                    native_path: project.to_string_lossy().into_owned(),
                    display_name: None,
                    order: None,
                    suppress_cross_storage_warning: false,
                }),
                home: locator(temp.path()),
                skill_root: locator(&canonical_root),
                lock: locator(&project.join("skills-lock.json")),
            },
            agent_runtime: eve_runtime(project.to_string_lossy().as_ref()),
            revisions: RuntimeRevisions {
                registry: "registry-1".to_string(),
                environment: "environment-1".to_string(),
                context: ContextSnapshotRevision::parse("context-v1-eve").unwrap(),
            },
            lock_schema: LockSchema::Project,
            lock_document: LosslessLockDocument::empty(LockSchema::Project),
            eve_targets: vec![InstallTargetInfo {
                target_id: "eve:root".to_string(),
                agent: AgentId::parse("eve").unwrap(),
                display_name: "Eve (root)".to_string(),
                subagent: None,
                path: project.join("agent/skills").to_string_lossy().into_owned(),
            }],
        };
        let target_resolver = RuntimeTargetFactResolver::new(Arc::new(WslRuntime::default()));
        let agent_selection = test_submission_for_agents(
            &context,
            &facts.agent_runtime,
            &facts.eve_targets,
            &target_resolver,
            &["eve"],
            InstallMode::Symlink,
        )
        .await;
        let planner = ConcreteInstallPlanner::new(
            Facts(facts),
            target_resolver,
            Arc::clone(&manager),
            || "2026-07-18T00:00:00.000Z".to_string(),
        );
        let request = InstallRequest {
            context,
            source: "owner/repo".to_string(),
            discovery_session: discovery,
            payloads: vec![handle.clone()],
            skills: vec!["demo".to_string()],
            agent_selection,
            acknowledge_risk: true,
        };

        let preview = planner
            .preview(&request, vec![manager.pin_verified(&handle).await.unwrap()])
            .await
            .unwrap();
        let (token, plan) = planner
            .rebuild(&request, vec![manager.pin_verified(&handle).await.unwrap()])
            .await
            .unwrap();

        let InstallPreviewOutcome::Ready { preview } = preview else {
            panic!("expected ready preview");
        };
        assert_eq!(preview.token, token);
        assert_eq!(plan.payloads.len(), 2);
        let unit = &plan.units[0];
        assert_eq!(
            unit.lock_mutation
                .as_ref()
                .and_then(|mutation| mutation.replacement.as_ref())
                .and_then(|entry| entry.get("subagents")),
            None
        );
        let canonical = unit.canonical_entry.as_ref().unwrap();
        let eve = &unit.required_agent_entries[0];
        let PreparedEntryAction::Replace {
            payload_id: canonical_id,
            ..
        } = &canonical.action
        else {
            panic!("canonical install must replace")
        };
        let PreparedEntryAction::Replace {
            payload_id: eve_id, ..
        } = &eve.action
        else {
            panic!("Eve install must replace")
        };
        assert_eq!(canonical_id, &canonical_payload_id);
        assert_ne!(eve_id, canonical_id);
        let canonical_payload = plan
            .payloads
            .get(canonical_id)
            .unwrap()
            .load_payload()
            .await
            .unwrap();
        let eve_payload = plan
            .payloads
            .get(eve_id)
            .unwrap()
            .load_payload()
            .await
            .unwrap();
        assert!(payload_skill_md(&canonical_payload).contains("name: demo"));
        assert!(!payload_skill_md(&eve_payload).contains("name: demo"));
        let lock = unit.lock_mutation.as_ref().unwrap();
        assert_eq!(
            lock.replacement.as_ref().unwrap()["computedHash"],
            "canonical-computed-hash"
        );
        assert_eq!(
            lock.replacement.as_ref().unwrap()["remoteHash"],
            "canonical-tree-hash"
        );
    }

    #[test]
    fn eve_lock_placement_matches_cli_root_and_named_target_semantics() {
        let temp = tempdir().unwrap();
        let project = temp.path().join("project");
        let context = SkillLocationRef {
            environment: EnvironmentRef::Native,
            scope: SkillLocation::Project {
                project_id: "project-1".to_string(),
            },
        };
        let facts = InstallPlanningFacts {
            resolved_context: ResolvedContext {
                context: context.clone(),
                project: None,
                home: locator(temp.path()),
                skill_root: locator(&project.join(".agents/skills")),
                lock: locator(&project.join("skills-lock.json")),
            },
            agent_runtime: eve_runtime(project.to_string_lossy().as_ref()),
            revisions: RuntimeRevisions {
                registry: "registry-1".to_string(),
                environment: "environment-1".to_string(),
                context: ContextSnapshotRevision::parse("context-v1-eve").unwrap(),
            },
            lock_schema: LockSchema::Project,
            lock_document: LosslessLockDocument::empty(LockSchema::Project),
            eve_targets: Vec::new(),
        };
        let metadata = PayloadPlanningMetadata {
            skill_name: "demo".to_string(),
            install_dir_name: "demo".to_string(),
            source: "owner/repo".to_string(),
            source_type: "github".to_string(),
            source_url: Some("https://github.com/owner/repo.git".to_string()),
            ref_name: Some("main".to_string()),
            skill_path: "skills/demo".to_string(),
            plugin_name: None,
            computed_hash: "computed".to_string(),
            upstream_revision: Some("remote".to_string()),
        };
        let root =
            |subagent: Option<&str>| crate::application::workflow_planner::LogicalAgentEntryRoot {
                target_id: crate::core::eve::eve_target_id(subagent),
                root: locator(temp.path()),
                owner_agent_ids: Vec::new(),
                content: crate::application::workflow_planner::AgentEntryContent::EveDerived {
                    subagent: subagent.map(str::to_string),
                },
            };

        let no_targets = AgentEntryPlan {
            canonical_owner_agent_ids: Vec::new(),
            required_agent_roots: Vec::new(),
        };
        let no_targets = lock_mutation(&facts, &metadata, &no_targets, "now".to_string())
            .unwrap()
            .replacement
            .unwrap();
        assert!(!no_targets.as_object().unwrap().contains_key("subagents"));

        let root_only = AgentEntryPlan {
            canonical_owner_agent_ids: Vec::new(),
            required_agent_roots: vec![root(None)],
        };
        let root_only = lock_mutation(&facts, &metadata, &root_only, "now".to_string())
            .unwrap()
            .replacement
            .unwrap();
        assert!(!root_only.as_object().unwrap().contains_key("subagents"));

        let named_and_root = AgentEntryPlan {
            canonical_owner_agent_ids: Vec::new(),
            required_agent_roots: vec![root(Some("builder")), root(None), root(Some("builder"))],
        };
        let named_and_root = lock_mutation(&facts, &metadata, &named_and_root, "now".to_string())
            .unwrap()
            .replacement
            .unwrap();
        assert_eq!(named_and_root["subagents"], json!(["", "builder"]));
    }

    fn payload_skill_md(payload: &crate::core::skill_payload::SkillPayload) -> &str {
        let entry = payload
            .entries
            .iter()
            .find(|entry| entry.relative_path == "SKILL.md")
            .unwrap();
        std::str::from_utf8(
            payload
                .blobs
                .get(entry.blob_id.as_deref().unwrap())
                .unwrap(),
        )
        .unwrap()
    }

    fn locator(path: &std::path::Path) -> ResourceLocator {
        ResourceLocator {
            environment: EnvironmentRef::Native,
            native_path: path.to_string_lossy().into_owned(),
        }
    }

    fn runtime(private_root: &str) -> AgentRuntimeSnapshot {
        let id = AgentId::parse("custom-private").unwrap();
        let scope = ResolvedAgentScope {
            enabled: true,
            reads_standard: false,
            standard_path: None,
            private_path: Some(private_root.to_string()),
            read_paths: Vec::new(),
            standard_presence: None,
            private_presence: None,
            legacy_paths: Vec::new(),
        };
        AgentRuntimeSnapshot {
            registry_revision: "registry-1".to_string(),
            environment_revision: "environment-1".to_string(),
            environment: EnvironmentRef::Native,
            availability: EnvironmentStatus::Available,
            project_path: None,
            agents: BTreeMap::from([(
                id.clone(),
                ResolvedAgent {
                    definition: AgentDefinition {
                        id,
                        display_name: "Custom Private".to_string(),
                        source: AgentSource::Custom,
                        aliases: Vec::new(),
                        global: ScopeDefinition {
                            enabled: true,
                            reads_standard: false,
                            private_path: Some(PathSpec::home(".custom/skills")),
                        },
                        project: ScopeDefinition {
                            enabled: false,
                            reads_standard: false,
                            private_path: None,
                        },
                        detection: DetectionSpec::AnyPathExists {
                            paths: vec![PathSpec::home(".custom")],
                        },
                        legacy_paths: Vec::new(),
                        adapter: AgentAdapter::Standard,
                    },
                    detection: DetectionState::Detected,
                    detection_reason: None,
                    global: scope.clone(),
                    project: scope,
                },
            )]),
        }
    }

    fn eve_runtime(project_path: &str) -> AgentRuntimeSnapshot {
        let id = AgentId::parse("eve").unwrap();
        let disabled = ResolvedAgentScope {
            enabled: false,
            reads_standard: false,
            standard_path: None,
            private_path: None,
            read_paths: Vec::new(),
            standard_presence: None,
            private_presence: None,
            legacy_paths: Vec::new(),
        };
        let project = ResolvedAgentScope {
            enabled: true,
            reads_standard: false,
            standard_path: None,
            private_path: None,
            read_paths: Vec::new(),
            standard_presence: None,
            private_presence: None,
            legacy_paths: Vec::new(),
        };
        AgentRuntimeSnapshot {
            registry_revision: "registry-1".to_string(),
            environment_revision: "environment-1".to_string(),
            environment: EnvironmentRef::Native,
            availability: EnvironmentStatus::Available,
            project_path: Some(project_path.to_string()),
            agents: BTreeMap::from([(
                id.clone(),
                ResolvedAgent {
                    definition: AgentDefinition {
                        id,
                        display_name: "Eve".to_string(),
                        source: AgentSource::Builtin,
                        aliases: Vec::new(),
                        global: ScopeDefinition {
                            enabled: false,
                            reads_standard: false,
                            private_path: None,
                        },
                        project: ScopeDefinition {
                            enabled: true,
                            reads_standard: false,
                            private_path: None,
                        },
                        detection: DetectionSpec::AnyPathExists {
                            paths: vec![
                                PathSpec::project("agent"),
                                PathSpec::project("package.json"),
                            ],
                        },
                        legacy_paths: Vec::new(),
                        adapter: AgentAdapter::Eve,
                    },
                    detection: DetectionState::Detected,
                    detection_reason: None,
                    global: disabled,
                    project,
                },
            )]),
        }
    }
}

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use specta::Type;
use std::sync::Arc;

use crate::application::installed_skill_resolver::SkillDirectoryName;
use crate::application::library_candidates::{LibraryCandidateSnapshot, LibraryCandidateSource};

use crate::application::mutation::executor::MutationPlanExecutor;
#[cfg(test)]
use crate::application::mutation::plan::PreparedEntryAction;
use crate::application::mutation::plan::{stable_digest, PreviewToken};
use crate::application::mutation::planning::{
    assemble_plan, issue_preview_token, validate_exact_preview, MutationPlanDraft,
    MutationUnitDraft, PreparedMutationEntries, PreviewTokenDraft,
};
use crate::application::mutation::result::MutationUnitResult;
use crate::application::planning_facts::{ScopePlanningSnapshot, ScopePlanningSnapshotSource};
use crate::application::scope_skill_placements::{
    ResolvedScopeSkillPlacements, ScopeSkillPlacementResolver,
};
use crate::application::scope_skill_planning::{
    DirectPlacementChange, DirectSkillChangeRequest, LibraryElectionState, ScopeSkillPlanner,
};
use crate::application::skill_entry_projection::{ObservedEntryKind, ObservedPhysicalEntry};
#[cfg(test)]
use crate::core::agent_definition::AgentId;
use crate::core::mutation::{CancellationSignal, MutationKind};
use crate::environment::planning::TargetFactResolver;
use crate::environment::runtime::ObservedEntryId;
use crate::environment::types::SkillLocationRef;
use crate::error::AppError;
use crate::storage::lock_plan::{LockEntryMutation, LockExpectedState, PreparedLockMutation};

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct RemovePreview {
    pub token: PreviewToken,
    pub context: SkillLocationRef,
    pub skill_name: String,
    pub standard: ObservedEntryKind,
    pub physical_entries: Vec<ObservedPhysicalEntry>,
    pub restores_library: bool,
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
    pub context: SkillLocationRef,
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
    facts: F,
    targets: T,
    observer: ScopeSkillPlacementResolver<T>,
    executor: E,
    library_candidates: Arc<dyn LibraryCandidateSource>,
}

impl<F, T, E> RemoveService<F, T, E>
where
    F: ScopePlanningSnapshotSource,
    T: TargetFactResolver + Clone,
    E: MutationPlanExecutor,
{
    pub fn new(
        facts: F,
        targets: T,
        executor: E,
        library_candidates: Arc<dyn LibraryCandidateSource>,
    ) -> Self {
        Self {
            facts,
            observer: ScopeSkillPlacementResolver::new(targets.clone()),
            targets,
            executor,
            library_candidates,
        }
    }

    pub async fn preview(
        &self,
        context: &SkillLocationRef,
        skill_name: &str,
    ) -> Result<RemovePreview, AppError> {
        if skill_name.trim().is_empty() {
            return Err(AppError::Validation {
                field: Some("skillName".to_string()),
                message: "Skill name is required".to_string(),
            });
        }
        let (facts, catalog, observed) = self.observe(context, skill_name).await?;
        let library_candidates = self.library_candidates(context, skill_name).await?;
        let plan = current_remove_plan(skill_name, &catalog, &observed, &library_candidates)?;
        let entries = plan
            .project_observed_entries()
            .map_err(|error| error.into_app_error())?;
        remove_preview(
            context,
            skill_name,
            &facts,
            &plan,
            &entries,
            &observed,
            &library_candidates,
        )
    }

    pub async fn execute(
        &self,
        request: &RemoveRequest,
        cancellation: CancellationSignal,
    ) -> Result<RemoveResponse, AppError> {
        let (facts, catalog, observed) =
            self.observe(&request.context, &request.skill_name).await?;
        let library_candidates = self
            .library_candidates(&request.context, &request.skill_name)
            .await?;
        let current_plan = current_remove_plan(
            &request.skill_name,
            &catalog,
            &observed,
            &library_candidates,
        )?;
        let observed_entries = current_plan
            .project_observed_entries()
            .map_err(|error| error.into_app_error())?;
        let preview = remove_preview(
            &request.context,
            &request.skill_name,
            &facts,
            &current_plan,
            &observed_entries,
            &observed,
            &library_candidates,
        )?;
        validate_exact_preview(&request.token, &preview.token)?;
        let selected = selected_entry_ids(&preview, &request.intent)?;
        let remove_canonical = request.intent == RemoveIntent::FullSkill;
        let directory_entries = build_remove_entries(
            &request.skill_name,
            &catalog,
            &observed,
            &observed_entries,
            &library_candidates,
            remove_canonical,
            &selected,
        )?;
        let snapshot = observed;
        let lock_key = &snapshot.resolved.lock_key;
        let lock_mutation = (remove_canonical && snapshot.resolved.lock_entry_exists).then(|| {
            PreparedLockMutation {
                target: facts.resolved_context.lock.clone(),
                legacy_target: None,
                schema: facts.lock_schema,
                entry: LockEntryMutation::Remove {
                    key: lock_key.clone(),
                },
                root_replacements: BTreeMap::new(),
                expected: LockExpectedState::capture(
                    &facts.lock_document,
                    [lock_key],
                    std::iter::empty::<&str>(),
                ),
            }
        });
        let plan = assemble_plan(MutationPlanDraft {
            kind: MutationKind::Remove,
            payloads: BTreeMap::new(),
            units: vec![MutationUnitDraft {
                id: format!("remove:{}", request.skill_name),
                skill_name: request.skill_name.clone(),
                source: None,
                target: request.context.clone(),
                expected_revisions: facts.revisions.clone(),
                entries: PreparedMutationEntries {
                    primary: directory_entries.primary,
                    additional: directory_entries.additional,
                    expected_targets: directory_entries.expected_targets,
                },
                lock_mutation,
            }],
        });
        Ok(RemoveResponse {
            units: self.executor.execute(plan, cancellation).await,
        })
    }

    async fn library_candidates(
        &self,
        context: &SkillLocationRef,
        skill_name: &str,
    ) -> Result<LibraryCandidateSnapshot, AppError> {
        let skill = SkillDirectoryName::try_from(skill_name)?;
        self.library_candidates
            .load_candidates(context, &skill)
            .await
    }

    async fn observe(
        &self,
        context: &SkillLocationRef,
        skill_name: &str,
    ) -> Result<
        (
            ScopePlanningSnapshot,
            crate::application::agent_selection::AgentSelectionCatalog,
            ResolvedScopeSkillPlacements,
        ),
        AppError,
    > {
        let facts = self.facts.snapshot(context).await?;
        let catalog = crate::application::agent_selection::build_agent_selection_catalog(
            context,
            &facts.agent_runtime,
            &facts.eve_targets,
            &facts.resolved_context.skill_root,
            &self.targets,
        )
        .await?;
        let observed = self
            .observer
            .observe(context, skill_name, &facts, &catalog)
            .await?;
        Ok((facts, catalog, observed))
    }
}

fn current_remove_plan(
    skill_name: &str,
    catalog: &crate::application::agent_selection::AgentSelectionCatalog,
    observed: &ResolvedScopeSkillPlacements,
    library_candidates: &LibraryCandidateSnapshot,
) -> Result<crate::application::scope_skill_planning::ScopeSkillPlan, AppError> {
    ScopeSkillPlanner::plan_direct_change(DirectSkillChangeRequest {
        skill: SkillDirectoryName::try_from(skill_name)?,
        catalog,
        placements: observed.placements.clone(),
        libraries: LibraryElectionState {
            candidates: library_candidates.candidates(),
            selected_agent_ids: library_candidates.selected_agent_ids(),
        },
        direct_changes: BTreeMap::new(),
    })
    .map_err(|error| error.into_app_error())
}

fn build_remove_entries(
    skill_name: &str,
    catalog: &crate::application::agent_selection::AgentSelectionCatalog,
    observed: &ResolvedScopeSkillPlacements,
    observed_entries: &[crate::application::skill_entry_projection::ObservedPlannedEntry],
    library_candidates: &LibraryCandidateSnapshot,
    full_skill: bool,
    selected: &BTreeSet<ObservedEntryId>,
) -> Result<PreparedMutationEntries, AppError> {
    let selected_keys = observed_entries
        .iter()
        .filter(|entry| selected.contains(&entry.public.entry_id))
        .map(|entry| entry.fact.key.clone())
        .collect::<BTreeSet<_>>();
    let mut direct_changes = BTreeMap::new();
    for (placement_id, fact) in observed.placements.facts() {
        let remove_direct = full_skill || selected_keys.contains(&fact.key);
        direct_changes.insert(
            placement_id.clone(),
            if remove_direct {
                DirectPlacementChange::Clear
            } else {
                DirectPlacementChange::Preserve
            },
        );
    }
    ScopeSkillPlanner::plan_direct_change(DirectSkillChangeRequest {
        skill: SkillDirectoryName::try_from(skill_name)?,
        catalog,
        placements: observed.placements.clone(),
        libraries: LibraryElectionState {
            candidates: library_candidates.candidates(),
            selected_agent_ids: library_candidates.selected_agent_ids(),
        },
        direct_changes,
    })
    .map(|plan| plan.compile_entries())
    .map_err(|error| error.into_app_error())
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
    context: &SkillLocationRef,
    skill_name: &str,
    facts: &ScopePlanningSnapshot,
    plan: &crate::application::scope_skill_planning::ScopeSkillPlan,
    entries: &[crate::application::skill_entry_projection::ObservedPlannedEntry],
    snapshot: &crate::application::scope_skill_placements::ResolvedScopeSkillPlacements,
    library_candidates: &LibraryCandidateSnapshot,
) -> Result<RemovePreview, AppError> {
    let observed_state_digest = stable_digest(&(
        &plan
            .standard_fact()
            .map_err(|error| error.into_app_error())?
            .key,
        &plan
            .standard_fact()
            .map_err(|error| error.into_app_error())?
            .fingerprint,
        entries
            .iter()
            .map(|entry| (&entry.public.entry_id, &entry.fact.fingerprint))
            .collect::<Vec<_>>(),
        facts
            .lock_document
            .entry_snapshot(&snapshot.resolved.lock_key)
            .value()
            .cloned(),
        library_candidates.evidence_digest(),
        library_candidates.selected_agent_ids(),
        library_candidates
            .candidates()
            .recognized()
            .iter()
            .map(|candidate| {
                (
                    candidate.library_id(),
                    candidate.member_name(),
                    candidate.locator(),
                )
            })
            .collect::<Vec<_>>(),
        library_candidates
            .candidates()
            .ordered()
            .iter()
            .map(|candidate| {
                (
                    candidate.library_id(),
                    candidate.member_name(),
                    candidate.locator(),
                )
            })
            .collect::<Vec<_>>(),
    ))?;
    let request = (context, skill_name);
    let token = issue_preview_token(PreviewTokenDraft {
        kind: MutationKind::Remove,
        request: &request,
        revisions: facts.revisions.clone(),
        observed_state_digest,
        planner_contract_version: 3,
    })?;
    Ok(RemovePreview {
        token,
        context: context.clone(),
        skill_name: skill_name.to_string(),
        standard: crate::application::skill_entry_projection::observed_entry_kind(
            plan.standard_fact()
                .map_err(|error| error.into_app_error())?
                .entry_kind,
        ),
        physical_entries: entries.iter().map(|entry| entry.public.clone()).collect(),
        restores_library: !library_candidates.candidates().ordered().is_empty(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    use crate::application::install::InstallFuture;
    use crate::application::mutation::executor::{MutationFuture, MutationPlanExecutor};
    use crate::application::mutation::plan::{MutationPlan, RuntimeRevisions};
    use crate::application::planning_facts::ScopePlanningSnapshot;
    use crate::core::agent_definition::{
        AgentAdapter, AgentDefinition, AgentSource, DetectionSpec, PathSpec, ScopeDefinition,
    };
    use crate::core::lossless_lock::{LockSchema, LosslessLockDocument};
    use crate::environment::agent_environment::{
        AgentRuntimeSnapshot, DetectionState, ResolvedAgent, ResolvedAgentScope,
    };
    use crate::environment::context_resolver::ResolvedContext;
    use crate::environment::planning::RuntimeTargetFactResolver;
    use crate::environment::runtime::ContextSnapshotRevision;
    use crate::environment::types::{
        EnvironmentRef, EnvironmentStatus, ResourceLocator, SkillLocation,
    };
    use crate::environment::wsl::WslRuntime;

    #[derive(Clone)]
    struct Facts(ScopePlanningSnapshot);

    impl ScopePlanningSnapshotSource for Facts {
        fn snapshot<'a>(
            &'a self,
            _context: &'a SkillLocationRef,
        ) -> InstallFuture<'a, Result<ScopePlanningSnapshot, AppError>> {
            Box::pin(async move { Ok(self.0.clone()) })
        }
    }

    struct FixedCandidates(LibraryCandidateSnapshot);

    impl LibraryCandidateSource for FixedCandidates {
        fn load_candidates<'a>(
            &'a self,
            _context: &'a SkillLocationRef,
            _skill_name: &'a SkillDirectoryName,
        ) -> crate::application::library_candidates::LibraryCandidateFuture<
            'a,
            Result<LibraryCandidateSnapshot, AppError>,
        > {
            Box::pin(async move { Ok(self.0.clone()) })
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
                *self.0.lock().unwrap() = Some(plan);
                Vec::new()
            })
        }
    }

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
            context: SkillLocationRef {
                environment: crate::environment::types::EnvironmentRef::Native,
                scope: crate::environment::types::SkillLocation::Global,
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
            context: SkillLocationRef {
                environment: crate::environment::types::EnvironmentRef::Native,
                scope: crate::environment::types::SkillLocation::Global,
            },
            skill_name: "demo".to_string(),
            standard: ObservedEntryKind::Directory,
            physical_entries: Vec::new(),
            restores_library: false,
        };

        assert_eq!(
            selected_entry_ids(&preview, &RemoveIntent::AgentEntries(vec![id])),
            Err(AppError::StaleTarget)
        );
    }

    #[tokio::test]
    async fn full_direct_removal_restores_the_library_link_in_the_same_plan() {
        let temp = tempfile::tempdir().unwrap();
        let skill_root = temp.path().join(".agents/skills");
        let canonical_path = skill_root.join("demo");
        std::fs::create_dir_all(&canonical_path).unwrap();
        std::fs::write(
            canonical_path.join("SKILL.md"),
            b"---\nname: demo\ndescription: Demo\n---\nBody\n",
        )
        .unwrap();
        let private_root = temp.path().join(".custom/skills");
        let private_path = private_root.join("demo");
        let library_only_path = temp.path().join(".library-only/skills/demo");
        std::fs::create_dir_all(&private_path).unwrap();
        std::fs::write(
            private_path.join("SKILL.md"),
            b"---\nname: demo\ndescription: Demo\n---\nBody\n",
        )
        .unwrap();
        let library_path = temp.path().join("libraries/lib-1/skills/demo");
        std::fs::create_dir_all(&library_path).unwrap();
        let context = SkillLocationRef {
            environment: EnvironmentRef::Native,
            scope: SkillLocation::Global,
        };
        let locator = |path: &std::path::Path| ResourceLocator {
            environment: EnvironmentRef::Native,
            native_path: path.to_string_lossy().into_owned(),
        };
        let agent_id = AgentId::parse("custom-private").unwrap();
        let enabled_scope = ResolvedAgentScope {
            enabled: true,
            reads_standard: false,
            standard_path: None,
            private_path: Some(private_root.to_string_lossy().into_owned()),
            read_paths: Vec::new(),
            standard_presence: None,
            private_presence: None,
            legacy_paths: Vec::new(),
        };
        let disabled_scope = ResolvedAgentScope {
            enabled: false,
            reads_standard: false,
            standard_path: None,
            private_path: None,
            read_paths: Vec::new(),
            standard_presence: None,
            private_presence: None,
            legacy_paths: Vec::new(),
        };
        let facts = ScopePlanningSnapshot {
            resolved_context: ResolvedContext {
                context: context.clone(),
                project: None,
                home: locator(temp.path()),
                skill_root: locator(&skill_root),
                lock: locator(&temp.path().join("skills-lock.json")),
            },
            agent_runtime: AgentRuntimeSnapshot {
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
                        global: enabled_scope,
                        project: disabled_scope,
                    },
                )]),
            },
            revisions: RuntimeRevisions {
                registry: "registry-1".to_string(),
                environment: "environment-1".to_string(),
                context: ContextSnapshotRevision::parse("context-1").unwrap(),
            },
            lock_schema: LockSchema::Global,
            lock_document: LosslessLockDocument::empty(LockSchema::Global),
            eve_targets: Vec::new(),
        };
        let targets = RuntimeTargetFactResolver::new(Arc::new(WslRuntime::default()));
        let resolved_targets = targets
            .resolve(
                &context,
                &[
                    locator(&canonical_path),
                    locator(&private_path),
                    locator(&library_only_path),
                ],
                None,
            )
            .await
            .unwrap();
        let _ = resolved_targets;
        let candidate = crate::application::library_candidates::LibraryVersionCandidate::new(
            crate::application::skill_libraries::LibraryId::parse("lib-1"),
            "demo",
            locator(&library_path),
        );
        let library_candidates = LibraryCandidateSnapshot::new(
            "library-evidence-1",
            vec![agent_id.clone()],
            crate::application::library_candidates::LibraryCandidateSet::new(
                vec![candidate.clone()],
                vec![candidate],
            )
            .unwrap(),
        )
        .unwrap();
        let executor = RecordingExecutor::default();
        let recorded = executor.0.clone();
        let service = RemoveService::new(
            Facts(facts),
            targets,
            executor,
            Arc::new(FixedCandidates(library_candidates)),
        );

        let preview = service.preview(&context, "demo").await.unwrap();
        assert!(preview.restores_library);
        service
            .execute(
                &RemoveRequest {
                    token: preview.token,
                    context,
                    skill_name: "demo".to_string(),
                    intent: RemoveIntent::FullSkill,
                },
                CancellationSignal::default(),
            )
            .await
            .unwrap();

        let plan = recorded.lock().unwrap().take().unwrap();
        assert!(matches!(
            plan.units[0].primary_entry.as_ref().map(|entry| &entry.action),
            Some(PreparedEntryAction::Link { target }) if target.native_path == library_path.to_string_lossy()
        ));
        let expected_private =
            std::fs::canonicalize(private_path.parent().expect("private parent"))
                .expect("canonical private parent")
                .join("demo");
        let private_entry = plan.units[0]
            .additional_entries
            .iter()
            .find(|entry| std::path::Path::new(&entry.destination.native_path) == expected_private)
            .expect("Agent private entry should be planned");
        assert!(matches!(
            &private_entry.action,
            PreparedEntryAction::Link { target }
                if target.native_path == library_path.to_string_lossy()
        ));
        assert_eq!(private_entry.reader_agent_ids, vec![agent_id]);
    }
}

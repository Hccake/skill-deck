use std::collections::{BTreeMap, BTreeSet};

use crate::application::agent_selection::{
    AgentSelectionCatalog, DirectoryContentKind, DirectoryPlacementId, SkillDirectoryAccess,
};
use crate::application::installed_skill_resolver::{
    InstalledSkillResolver, ResolvedInstalledSkill,
};
use crate::application::library_candidates::{
    LibraryVersionCandidate, ResolvedLibraryCandidateIndex,
};
use crate::application::mutation::plan::stable_digest;
use crate::application::planning_facts::ScopePlanningSnapshot;
use crate::application::scope_skill_planning::ScopeSkillPlacementSet;
use crate::application::skill_entry_projection::{
    observed_entry_kind, ObservedEntryReader, ObservedPhysicalEntry, ObservedPlannedEntry,
};
use crate::environment::planning::{TargetEntryKind, TargetFactResolver};
use crate::environment::runtime::observed_entry_id;
use crate::environment::types::SkillLocationRef;
use crate::error::AppError;

#[derive(Clone)]
pub struct ResolvedScopeSkillPlacements {
    pub resolved: ResolvedInstalledSkill,
    pub(crate) placements: ScopeSkillPlacementSet,
    pub(crate) additional_observations: Vec<crate::environment::planning::ResolvedTargetFact>,
    recorded_eve_targets: Option<Vec<String>>,
}

pub(crate) fn ambiguous_installed_source(path: &str) -> AppError {
    AppError::CapabilityUnavailable {
        capability: "installedSkillSourceAmbiguous".into(),
        path: Some(path.into()),
    }
}

pub(crate) fn eve_target_ids(subagents: Option<&[String]>) -> Result<Vec<String>, AppError> {
    let Some(subagents) = subagents else {
        return Ok(["eve:root".to_string()].into());
    };
    let mut ids = Vec::new();
    let mut seen = BTreeSet::new();
    for subagent in subagents {
        let name = if subagent.is_empty() {
            "root".into()
        } else {
            let name = InstalledSkillResolver::install_dir_name(subagent)?;
            if name == "root" {
                return Err(ambiguous_installed_source(subagent));
            }
            name
        };
        let id = format!("eve:{name}");
        if !seen.insert(id.clone()) {
            return Err(ambiguous_installed_source(subagent));
        }
        ids.push(id);
    }
    Ok(ids)
}

pub(crate) fn recorded_eve_target_ids(
    name: &str,
    document: &crate::core::lossless_lock::LosslessLockDocument,
) -> Result<Option<Vec<String>>, AppError> {
    use crate::core::local_lock::LocalSkillLockEntry;
    let resolved = InstalledSkillResolver::resolve(name, document)?;
    let Some(value) = document.entry_snapshot(&resolved.lock_key).value().cloned() else {
        return Ok(None);
    };
    let Ok(entry) = serde_json::from_value::<LocalSkillLockEntry>(value) else {
        return Ok(None);
    };
    if entry.source.trim().is_empty() || entry.source_type.trim().is_empty() {
        return Ok(None);
    }
    let targets = eve_target_ids(entry.subagents.as_deref())?;
    Ok(Some(targets))
}

#[derive(Debug, Clone)]
pub(crate) struct ObservedSkillPlacement {
    pub id: DirectoryPlacementId,
    pub entry: ObservedPlannedEntry,
    pub content: DirectoryContentKind,
    pub library: Option<LibraryVersionCandidate>,
}

impl ObservedSkillPlacement {
    pub(crate) fn is_direct_directory(&self) -> bool {
        self.library.is_none() && self.entry.fact.entry_kind == TargetEntryKind::Directory
    }
}

impl ResolvedScopeSkillPlacements {
    pub(crate) fn describe(
        &self,
        catalog: &AgentSelectionCatalog,
        libraries: &ResolvedLibraryCandidateIndex,
    ) -> Result<Vec<ObservedSkillPlacement>, AppError> {
        let skill = crate::application::installed_skill_resolver::SkillDirectoryName::try_from(
            self.resolved.skill_name.as_str(),
        )?;
        let mut placements = self
            .placements
            .facts()
            .iter()
            .filter(|(id, _)| match id {
                DirectoryPlacementId::Standard => true,
                DirectoryPlacementId::Option(id) => catalog.option(id).is_none_or(|option| {
                    !option.placement.content.uses_eve_payload()
                        || self.recorded_eve_targets.as_ref().is_some_and(|ids| {
                            option.adapter_target_ids.iter().any(|id| ids.contains(id))
                        })
                }),
            })
            .map(|(id, fact)| {
                let (content, readers) = match id {
                    DirectoryPlacementId::Standard => (
                        DirectoryContentKind::Original,
                        catalog
                            .snapshot()
                            .agents
                            .iter()
                            .filter(|agent| {
                                matches!(
                                    agent.directory_access,
                                    Some(
                                        SkillDirectoryAccess::StandardOnly
                                            | SkillDirectoryAccess::Both
                                    )
                                )
                            })
                            .map(|agent| ObservedEntryReader {
                                agent_id: agent.id.clone(),
                                display_name: agent.display_name.clone(),
                                logical_target_id: "canonical".to_string(),
                            })
                            .collect(),
                    ),
                    DirectoryPlacementId::Option(option_id) => {
                        let option = catalog.option(option_id).ok_or(AppError::StaleTarget)?;
                        let mut readers = Vec::new();
                        for agent_id in &option.public.agent_ids {
                            let agent = catalog
                                .snapshot()
                                .agents
                                .iter()
                                .find(|agent| &agent.id == agent_id)
                                .ok_or(AppError::StaleRegistry)?;
                            let target_ids = if option.adapter_target_ids.is_empty() {
                                vec![format!("agent:{}:private", agent_id.as_str())]
                            } else {
                                option.adapter_target_ids.clone()
                            };
                            for target_id in target_ids {
                                readers.push(ObservedEntryReader {
                                    agent_id: agent_id.clone(),
                                    display_name: if option.placement.content.uses_eve_payload() {
                                        option.public.display_name.clone()
                                    } else {
                                        agent.display_name.clone()
                                    },
                                    logical_target_id: target_id,
                                });
                            }
                        }
                        (option.placement.content.clone(), readers)
                    }
                };
                Ok(ObservedSkillPlacement {
                    id: id.clone(),
                    content,
                    library: libraries.owner(&skill, fact).cloned(),
                    entry: ObservedPlannedEntry {
                        public: ObservedPhysicalEntry {
                            entry_id: observed_entry_id(&fact.key, &fact.fingerprint)?,
                            display_path: fact.destination.clone(),
                            kind: observed_entry_kind(fact.entry_kind),
                            physical_target_key: stable_digest(&fact.key)?,
                            readers,
                            will_break_if_standard_removed: false,
                        },
                        fact: fact.clone(),
                    },
                })
            })
            .collect::<Result<Vec<_>, AppError>>()?;
        let links = placements
            .iter()
            .filter(|placement| {
                placement.library.is_none()
                    && matches!(
                        placement.entry.fact.entry_kind,
                        TargetEntryKind::Symlink | TargetEntryKind::Junction
                    )
            })
            .filter_map(|placement| {
                placement
                    .entry
                    .fact
                    .link_target_identity
                    .clone()
                    .map(|identity| (identity, placement.entry.public.readers.clone()))
            })
            .collect::<Vec<_>>();
        for placement in placements
            .iter_mut()
            .filter(|placement| placement.is_direct_directory())
        {
            for (identity, readers) in &links {
                if identity.matches(&placement.entry.fact.destination) {
                    placement.entry.public.readers.extend(readers.clone());
                }
            }
            placement.entry.public.readers.sort_by(|a, b| {
                (&a.agent_id, &a.logical_target_id).cmp(&(&b.agent_id, &b.logical_target_id))
            });
            placement.entry.public.readers.dedup();
        }
        Ok(placements)
    }
}

pub(crate) fn representative_direct_placement(
    placements: &[ObservedSkillPlacement],
) -> Option<&ObservedSkillPlacement> {
    placements
        .iter()
        .filter(|placement| placement.is_direct_directory())
        .min_by_key(|placement| {
            (
                placement.id != DirectoryPlacementId::Standard,
                placement.content.uses_eve_payload(),
                placement.entry.fact.destination.native_path.as_str(),
            )
        })
}

pub struct ScopeSkillPlacementResolver<T> {
    targets: T,
}

impl<T> ScopeSkillPlacementResolver<T> {
    pub fn new(targets: T) -> Self {
        Self { targets }
    }
}

impl<T> ScopeSkillPlacementResolver<T>
where
    T: TargetFactResolver,
{
    pub(crate) async fn observe(
        &self,
        context: &SkillLocationRef,
        skill_name: &str,
        facts: &ScopePlanningSnapshot,
        catalog: &AgentSelectionCatalog,
    ) -> Result<ResolvedScopeSkillPlacements, AppError> {
        observe_scope_skill_placements(&self.targets, context, skill_name, facts, catalog).await
    }
}

pub(crate) async fn observe_scope_skill_placements<T: TargetFactResolver + ?Sized>(
    targets: &T,
    context: &SkillLocationRef,
    skill_name: &str,
    facts: &ScopePlanningSnapshot,
    catalog: &AgentSelectionCatalog,
) -> Result<ResolvedScopeSkillPlacements, AppError> {
    let standard = catalog.standard();
    if facts.resolved_context.context != *context || catalog.context() != context {
        return Err(AppError::StaleContext);
    }
    let resolved_identity = InstalledSkillResolver::resolve(skill_name, &facts.lock_document)?;
    let install_dir_name = &resolved_identity.install_dir_name;
    let mut destinations = vec![standard.root.join_child(install_dir_name)];
    let mut placement_ids = vec![DirectoryPlacementId::Standard];
    let recorded_eve = if facts.lock_schema == crate::core::lossless_lock::LockSchema::Project {
        recorded_eve_target_ids(skill_name, &facts.lock_document)?
    } else {
        None
    };
    let mut single_files = Vec::new();
    if let Some(ids) = &recorded_eve {
        for id in ids {
            let roots = catalog
                .options()
                .filter(|option| option.adapter_target_ids.contains(id))
                .map(|option| &option.placement.physical_key)
                .collect::<BTreeSet<_>>();
            if roots.len() > 1 {
                return Err(ambiguous_installed_source(skill_name));
            }
        }
    }
    for option in catalog.options() {
        if option.placement.content.uses_eve_payload()
            && recorded_eve
                .as_ref()
                .is_some_and(|ids| option.adapter_target_ids.iter().any(|id| ids.contains(id)))
        {
            single_files.push((
                destinations.len(),
                option
                    .placement
                    .root
                    .join_child(&format!("{install_dir_name}.md")),
            ));
        }
        destinations.push(option.placement.root.join_child(install_dir_name));
        placement_ids.push(option.placement.id.clone());
    }
    destinations.extend(single_files.iter().map(|(_, path)| path.clone()));
    let resolved = targets.resolve(context, &destinations, None).await?;
    if resolved.len() != destinations.len() || resolved.is_empty() {
        return Err(AppError::StaleTarget);
    }
    let additional_observations = resolved[placement_ids.len()..].to_vec();
    for ((directory_index, _), file) in single_files.iter().zip(&additional_observations) {
        if file.entry_kind != TargetEntryKind::Missing {
            if resolved[*directory_index].entry_kind != TargetEntryKind::Missing {
                return Err(ambiguous_installed_source(&file.destination.native_path));
            }
            return Err(AppError::CapabilityUnavailable {
                capability: "eveSingleFile".into(),
                path: Some(file.destination.native_path.clone()),
            });
        }
    }
    let placement_facts = placement_ids
        .into_iter()
        .zip(resolved.iter().cloned())
        .collect::<BTreeMap<_, _>>();
    if let Some(ids) = &recorded_eve {
        let owned = |ids: &[String]| {
            placement_facts
                .iter()
                .filter_map(|(id, fact)| {
                    let DirectoryPlacementId::Option(id) = id else {
                        return None;
                    };
                    catalog
                        .option(id)
                        .filter(|option| {
                            option.adapter_target_ids.iter().any(|id| ids.contains(id))
                        })
                        .map(|_| fact)
                })
                .collect::<Vec<_>>()
        };
        let current = owned(ids);
        if let Some(records) = facts
            .lock_document
            .root_snapshot("skills")
            .value()
            .and_then(serde_json::Value::as_object)
        {
            for name in records.keys() {
                if name == &resolved_identity.lock_key
                    || InstalledSkillResolver::install_dir_name(name)
                        .ok()
                        .as_deref()
                        != Some(install_dir_name.as_str())
                {
                    continue;
                }
                let Some(ids) = recorded_eve_target_ids(name, &facts.lock_document)? else {
                    continue;
                };
                if owned(&ids).iter().any(|other| {
                    current.iter().any(|target| {
                        other.key == target.key
                            || other
                                .link_target_identity
                                .as_ref()
                                .is_some_and(|identity| identity.matches(&target.destination))
                            || target
                                .link_target_identity
                                .as_ref()
                                .is_some_and(|identity| identity.matches(&other.destination))
                    })
                }) {
                    return Err(ambiguous_installed_source(skill_name));
                }
            }
        }
    }
    Ok(ResolvedScopeSkillPlacements {
        resolved: resolved_identity,
        placements: ScopeSkillPlacementSet::new(context.clone(), placement_facts),
        additional_observations,
        recorded_eve_targets: recorded_eve,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::agent_selection::build_agent_selection_catalog;
    use crate::application::installed_skill_payload::InstalledSkillPayloadAcquirer;
    use crate::application::mutation::plan::RuntimeRevisions;
    use crate::application::payload_session::{PayloadSessionLimits, PayloadSessionManager};
    use crate::core::agent_definition::AgentId;
    use crate::core::agent_definition::{
        AgentAdapter, AgentDefinition, AgentSource, DetectionSpec, PathSpec, ScopeDefinition,
    };
    use crate::core::lossless_lock::{LockSchema, LosslessLockDocument};
    use crate::environment::agent_environment::{
        AgentRuntimeSnapshot, DetectionState, ResolvedAgent, ResolvedAgentScope,
    };
    use crate::environment::context_resolver::ResolvedContext;
    use crate::environment::planning::{ResolvedTargetFact, TargetEntryKind};
    use crate::environment::runtime::{
        EntryFingerprint, ExecutionBackend, PhysicalParentIdentity, PhysicalTargetKey,
    };
    use crate::environment::types::{
        EnvironmentRef, EnvironmentStatus, ResourceLocator, SkillLocation, SkillLocationRef,
    };
    use crate::environment::wsl::WslRuntime;
    use std::collections::BTreeSet;
    use std::path::Path;
    use std::sync::Arc;
    use tempfile::tempdir;

    #[derive(Clone)]
    struct PathTargets;

    impl TargetFactResolver for PathTargets {
        fn resolve<'a>(
            &'a self,
            _context: &'a SkillLocationRef,
            logical_destinations: &'a [ResourceLocator],
            _cancellation: Option<crate::core::mutation::CancellationSignal>,
        ) -> crate::environment::planning::TargetFactFuture<
            'a,
            Result<Vec<ResolvedTargetFact>, AppError>,
        > {
            Box::pin(async move {
                Ok(logical_destinations
                    .iter()
                    .map(|destination| {
                        let name = Path::new(&destination.native_path)
                            .file_name()
                            .and_then(|name| name.to_str())
                            .unwrap_or("target");
                        let has_component = |expected: &str| {
                            Path::new(&destination.native_path)
                                .components()
                                .any(|component| component.as_os_str() == expected)
                        };
                        let inode = if has_component("shared") {
                            2
                        } else if has_component("eve") {
                            3
                        } else {
                            1
                        };
                        ResolvedTargetFact {
                            key: PhysicalTargetKey {
                                backend: if cfg!(windows) {
                                    ExecutionBackend::NativeWindows
                                } else {
                                    ExecutionBackend::NativeUnix
                                },
                                physical_parent: if cfg!(windows) {
                                    PhysicalParentIdentity::Windows {
                                        volume_serial: 1,
                                        file_id: u128::from(inode),
                                    }
                                } else {
                                    PhysicalParentIdentity::Unix { device: 1, inode }
                                },
                                normalized_final_child_name: name.to_string(),
                            },
                            destination: destination.clone(),
                            storage_access: crate::environment::types::StorageAccess::Native,
                            fingerprint: EntryFingerprint(format!("entry-v1-{name}")),
                            entry_kind: TargetEntryKind::Directory,
                            link_target: None,
                            link_target_identity: None,
                        }
                    })
                    .collect())
            })
        }
    }

    fn observer_facts() -> ScopePlanningSnapshot {
        let context = SkillLocationRef {
            environment: EnvironmentRef::Native,
            scope: SkillLocation::Global,
        };
        let locator = |path: &str| ResourceLocator {
            environment: EnvironmentRef::Native,
            native_path: path.to_string(),
        };
        ScopePlanningSnapshot {
            resolved_context: ResolvedContext {
                context,
                project: None,
                home: locator("/scope"),
                skill_root: locator("/scope/.agents/skills"),
                lock: locator("/scope/.agents/.skill-lock.json"),
            },
            agent_runtime: AgentRuntimeSnapshot {
                registry_revision: "registry-1".to_string(),
                environment_revision: "environment-1".to_string(),
                environment: EnvironmentRef::Native,
                availability: EnvironmentStatus::Available,
                project_path: None,
                agents: BTreeMap::new(),
            },
            revisions: RuntimeRevisions {
                registry: "registry-1".to_string(),
                environment: "environment-1".to_string(),
                context: crate::environment::runtime::ContextSnapshotRevision::parse("context-1")
                    .unwrap(),
            },
            lock_schema: LockSchema::Global,
            lock_document: LosslessLockDocument::empty(LockSchema::Global),
            eve_targets: Vec::new(),
        }
    }

    fn observed_agent(
        id: &str,
        display_name: &str,
        adapter: AgentAdapter,
        private_path: Option<&str>,
    ) -> (AgentId, ResolvedAgent) {
        let id = AgentId::parse(id).unwrap();
        let scope_definition = ScopeDefinition {
            enabled: true,
            reads_standard: false,
            private_path: private_path.map(PathSpec::home),
        };
        let resolved_scope = ResolvedAgentScope {
            enabled: true,
            reads_standard: false,
            standard_path: Some("/scope/.agents/skills".to_string()),
            private_path: private_path.map(str::to_string),
            read_paths: private_path.into_iter().map(str::to_string).collect(),
            standard_presence: None,
            private_presence: None,
            legacy_paths: Vec::new(),
        };
        (
            id.clone(),
            ResolvedAgent {
                definition: AgentDefinition {
                    id,
                    display_name: display_name.to_string(),
                    source: AgentSource::Builtin,
                    aliases: Vec::new(),
                    global: scope_definition,
                    project: ScopeDefinition {
                        enabled: false,
                        reads_standard: false,
                        private_path: None,
                    },
                    detection: DetectionSpec::AnyPathExists {
                        paths: vec![PathSpec::home(".agent")],
                    },
                    legacy_paths: Vec::new(),
                    adapter,
                },
                detection: DetectionState::Detected,
                detection_reason: None,
                global: resolved_scope,
                project: ResolvedAgentScope {
                    enabled: false,
                    reads_standard: false,
                    standard_path: None,
                    private_path: None,
                    read_paths: Vec::new(),
                    standard_presence: None,
                    private_presence: None,
                    legacy_paths: Vec::new(),
                },
            },
        )
    }

    #[tokio::test]
    async fn direct_placement_includes_readers_of_leaf_links() {
        let temp = tempdir().unwrap();
        let direct_root = temp.path().join("direct");
        let linked_root = temp.path().join("linked");
        std::fs::create_dir_all(direct_root.join("demo")).unwrap();
        std::fs::create_dir_all(&linked_root).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(direct_root.join("demo"), linked_root.join("demo")).unwrap();
        #[cfg(windows)]
        junction::create(direct_root.join("demo"), linked_root.join("demo")).unwrap();
        let mut facts = observer_facts();
        facts.resolved_context.home.native_path = temp.path().to_string_lossy().into_owned();
        facts.resolved_context.skill_root.native_path =
            temp.path().join("standard").to_string_lossy().into_owned();
        facts.agent_runtime.agents = [
            observed_agent(
                "direct",
                "Direct",
                AgentAdapter::Standard,
                direct_root.to_str(),
            ),
            observed_agent(
                "linked",
                "Linked",
                AgentAdapter::Standard,
                linked_root.to_str(),
            ),
        ]
        .into();
        let context = &facts.resolved_context.context;
        let targets = crate::environment::planning::RuntimeTargetFactResolver::new(Arc::new(
            WslRuntime::default(),
        ));
        let catalog = build_agent_selection_catalog(
            context,
            &facts.agent_runtime,
            &[],
            &facts.resolved_context.skill_root,
            &targets,
        )
        .await
        .unwrap();
        let libraries =
            crate::native_workflow_integration_support::update_library_repository(temp.path());
        let known = ResolvedLibraryCandidateIndex::load_known(
            libraries.as_ref(),
            &targets,
            &context.environment,
            &["demo".try_into().unwrap()].into(),
        )
        .await
        .unwrap();
        let observed = observe_scope_skill_placements(&targets, context, "demo", &facts, &catalog)
            .await
            .unwrap();
        let placements = observed.describe(&catalog, &known).unwrap();
        let direct = representative_direct_placement(&placements).unwrap();
        assert_eq!(
            direct
                .entry
                .public
                .readers
                .iter()
                .map(|r| r.agent_id.as_str())
                .collect::<BTreeSet<_>>(),
            ["direct", "linked"].into()
        );
        assert_eq!(
            placements
                .iter()
                .filter(|p| p.is_direct_directory())
                .count(),
            1
        );
    }

    #[tokio::test]
    async fn catalog_observation_reuses_shared_overlap_and_eve_placements() {
        let mut facts = observer_facts();
        let (eve_id, eve) = observed_agent("eve", "Eve", AgentAdapter::Eve, None);
        facts.agent_runtime.agents = [
            observed_agent(
                "shared-a",
                "Shared A",
                AgentAdapter::Standard,
                Some("/scope/shared/skills"),
            ),
            observed_agent(
                "shared-b",
                "Shared B",
                AgentAdapter::Standard,
                Some("/scope/shared/skills"),
            ),
            observed_agent(
                "overlap",
                "Overlap",
                AgentAdapter::Standard,
                Some("/scope/.agents/skills"),
            ),
            (eve_id.clone(), eve),
        ]
        .into_iter()
        .collect();
        facts.eve_targets = vec![crate::models::InstallTargetInfo {
            target_id: "eve:root".to_string(),
            agent: eve_id,
            display_name: "Eve (root)".to_string(),
            subagent: None,
            path: "/scope/eve/skills".to_string(),
        }];
        let context = facts.resolved_context.context.clone();
        let targets = PathTargets;
        let catalog = build_agent_selection_catalog(
            &context,
            &facts.agent_runtime,
            &facts.eve_targets,
            &facts.resolved_context.skill_root,
            &targets,
        )
        .await
        .unwrap();
        let observer = ScopeSkillPlacementResolver::new(targets);

        let observed = observer
            .observe(&context, "demo", &facts, &catalog)
            .await
            .unwrap();
        assert_eq!(observed.placements.facts().len(), 3);
        assert!(observed
            .placements
            .facts()
            .contains_key(&DirectoryPlacementId::Standard));
    }

    #[tokio::test]
    async fn observation_rejects_catalog_from_another_scope_in_the_same_environment() {
        let facts = observer_facts();
        let global = facts.resolved_context.context.clone();
        let targets = PathTargets;
        let catalog = build_agent_selection_catalog(
            &global,
            &facts.agent_runtime,
            &facts.eve_targets,
            &facts.resolved_context.skill_root,
            &targets,
        )
        .await
        .unwrap();
        let project = SkillLocationRef {
            environment: global.environment.clone(),
            scope: SkillLocation::Project {
                project_id: "another-project".to_string(),
            },
        };
        let observer = ScopeSkillPlacementResolver::new(targets);

        assert!(matches!(
            observer.observe(&project, "demo", &facts, &catalog).await,
            Err(AppError::StaleContext)
        ));
    }

    #[tokio::test]
    async fn installed_canonical_acquisition_keeps_the_complete_directory_payload() {
        let temp = tempdir().unwrap();
        let standard = temp.path().join("demo");
        std::fs::create_dir_all(standard.join("scripts")).unwrap();
        std::fs::write(
            standard.join("SKILL.md"),
            b"---\nname: demo\ndescription: Demo\n---\nbody",
        )
        .unwrap();
        std::fs::write(standard.join("scripts/run.sh"), b"#!/bin/sh\n").unwrap();
        let manager = Arc::new(PayloadSessionManager::in_memory(
            PayloadSessionLimits {
                ttl_ms: 60_000,
                max_sessions: 4,
                max_bytes: 1_000_000,
            },
            || 1_000,
        ));
        let acquirer = InstalledSkillPayloadAcquirer::new(
            Arc::clone(&manager),
            Arc::new(WslRuntime::default()),
        );
        let context = crate::environment::types::SkillLocationRef {
            environment: EnvironmentRef::Native,
            scope: crate::environment::types::SkillLocation::Global,
        };
        let canonical_fact = fact(
            "demo",
            standard.to_string_lossy().as_ref(),
            TargetEntryKind::Directory,
            None,
        );

        let handle = acquirer
            .acquire(&context, "demo", &canonical_fact)
            .await
            .unwrap();
        let payload = manager
            .pin_verified(&handle)
            .await
            .unwrap()
            .load_payload()
            .await
            .unwrap();

        assert!(payload
            .entries
            .iter()
            .any(|entry| entry.relative_path == "scripts/run.sh"));
    }

    fn fact(
        name: &str,
        path: &str,
        entry_kind: TargetEntryKind,
        link_target: Option<&str>,
    ) -> ResolvedTargetFact {
        let destination = ResourceLocator {
            environment: EnvironmentRef::Native,
            native_path: path.to_string(),
        };
        ResolvedTargetFact {
            key: PhysicalTargetKey {
                backend: if cfg!(windows) {
                    ExecutionBackend::NativeWindows
                } else {
                    ExecutionBackend::NativeUnix
                },
                physical_parent: if cfg!(windows) {
                    PhysicalParentIdentity::Windows {
                        volume_serial: 1,
                        file_id: if name == "copy" { 2 } else { 3 },
                    }
                } else {
                    PhysicalParentIdentity::Unix {
                        device: 1,
                        inode: if name == "copy" { 2 } else { 3 },
                    }
                },
                normalized_final_child_name: name.to_string(),
            },
            link_target_identity: link_target.and_then(|raw| {
                crate::environment::planning::resolve_link_target_identity(&destination, raw)
            }),
            destination,
            storage_access: crate::environment::types::StorageAccess::Native,
            fingerprint: EntryFingerprint(format!("entry-v1-{name}")),
            entry_kind,
            link_target: link_target.map(str::to_string),
        }
    }
}

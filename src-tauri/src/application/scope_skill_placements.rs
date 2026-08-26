use std::collections::BTreeMap;

use crate::application::agent_selection::{AgentSelectionCatalog, DirectoryPlacementId};
use crate::application::installed_skill_resolver::{
    InstalledSkillResolver, ResolvedInstalledSkill,
};
use crate::application::planning_facts::ScopePlanningSnapshot;
use crate::application::scope_skill_planning::ScopeSkillPlacementSet;
use crate::environment::planning::TargetFactResolver;
use crate::environment::types::SkillLocationRef;
use crate::error::AppError;

#[derive(Clone)]
pub struct ResolvedScopeSkillPlacements {
    pub resolved: ResolvedInstalledSkill,
    pub(crate) placements: ScopeSkillPlacementSet,
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
        let standard = catalog.standard();
        if facts.resolved_context.context != *context || catalog.context() != context {
            return Err(AppError::StaleContext);
        }
        let resolved_identity = InstalledSkillResolver::resolve(skill_name, &facts.lock_document)?;
        let install_dir_name = &resolved_identity.install_dir_name;
        let mut destinations = vec![standard.root.join_child(install_dir_name)];
        let mut placement_ids = vec![DirectoryPlacementId::Standard];
        for option in catalog.options() {
            destinations.push(option.placement.root.join_child(install_dir_name));
            placement_ids.push(option.placement.id.clone());
        }
        let resolved = self.targets.resolve(context, &destinations, None).await?;
        if resolved.len() != destinations.len() || resolved.is_empty() {
            return Err(AppError::StaleTarget);
        }
        let placement_facts = placement_ids
            .into_iter()
            .zip(resolved.iter().cloned())
            .collect::<BTreeMap<_, _>>();
        Ok(ResolvedScopeSkillPlacements {
            resolved: resolved_identity,
            placements: ScopeSkillPlacementSet::new(context.clone(), placement_facts),
        })
    }
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
        std::fs::write(standard.join("SKILL.md"), b"demo").unwrap();
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

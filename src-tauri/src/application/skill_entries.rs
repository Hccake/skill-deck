use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use crate::application::install_planner::{InstallPlanningFactSource, InstallPlanningFacts};
use crate::application::mutation::plan::stable_digest;
use crate::application::payload_session::{
    AcquiredPayloadHandle, DiscoverySourceDescriptor, DiscoverySourceLocation,
    PayloadPlanningMetadata, PayloadSessionManager, PayloadSessionStorage, PayloadStorageKey,
    RetainedDiscoverySource,
};
use crate::application::remove::{ObservedEntryKind, ObservedEntryOwner, ObservedPhysicalEntry};
use crate::core::agent_definition::AgentAdapter;
use crate::environment::agent_environment::DetectionState;
use crate::environment::planning::TargetFactResolver;
use crate::environment::planning::{ResolvedTargetFact, TargetEntryKind};
use crate::environment::runtime::{observed_entry_id, PhysicalTargetKey};
use crate::environment::types::{
    same_environment_identity, ContextRef, ContextScope, EnvironmentRef, ResourceLocator,
};
use crate::environment::wsl::operations::acquire::WslPayloadSessionStorage;
use crate::environment::wsl::WslRuntime;
use crate::error::AppError;

#[derive(Debug, Clone)]
pub struct ObservedEntryCandidate {
    pub fact: ResolvedTargetFact,
    pub owner: ObservedEntryOwner,
}

#[derive(Debug, Clone)]
pub struct ObservedPlannedEntry {
    pub public: ObservedPhysicalEntry,
    pub fact: ResolvedTargetFact,
}

#[derive(Clone)]
pub struct ObservedSkillSnapshot {
    pub facts: InstallPlanningFacts,
    pub canonical: ResolvedTargetFact,
    pub entries: Vec<ObservedPlannedEntry>,
}

pub struct SkillEntryObserver<F, T> {
    facts: F,
    targets: T,
}

pub struct InstalledSkillPayloadAcquirer {
    payloads: Arc<PayloadSessionManager>,
    environments: Arc<WslRuntime>,
}

impl InstalledSkillPayloadAcquirer {
    pub fn new(payloads: Arc<PayloadSessionManager>, environments: Arc<WslRuntime>) -> Self {
        Self {
            payloads,
            environments,
        }
    }

    pub async fn acquire(
        &self,
        context: &ContextRef,
        skill_name: &str,
        canonical: &ResolvedTargetFact,
    ) -> Result<AcquiredPayloadHandle, AppError> {
        if canonical.entry_kind != TargetEntryKind::Directory
            || !same_environment_identity(&canonical.destination.environment, &context.environment)
        {
            return Err(AppError::StaleTarget);
        }
        let source_fingerprint = stable_digest(&(&canonical.key, &canonical.fingerprint))?;
        match &context.environment {
            EnvironmentRef::Host => {
                let payload = crate::core::skill_payload::build_skill_payload(Path::new(
                    &canonical.destination.native_path,
                ))?;
                let computed_hash =
                    crate::core::skill_payload::compute_cli_project_hash_from_payload(&payload)?;
                let discovery = self
                    .payloads
                    .discover(EnvironmentRef::Host, source_fingerprint)
                    .await?;
                self.payloads
                    .acquire_payload_with_metadata(
                        &discovery,
                        skill_name,
                        payload,
                        installed_metadata(skill_name, computed_hash),
                    )
                    .await
            }
            EnvironmentRef::Wsl { distro_name } => {
                let workspace = self.environments.workspace(distro_name)?;
                let canonical_path = canonical.destination.native_path.clone();
                let skill_name = skill_name.to_string();
                let storage = Arc::new(WslPayloadSessionStorage::new(workspace));
                let retained = RetainedDiscoverySource::new(
                    DiscoverySourceLocation::WslNative {
                        distro_name: distro_name.clone(),
                        linux_root: canonical_path.clone(),
                        ref_revision: None,
                    },
                    DiscoverySourceDescriptor {
                        source: "installed-canonical".to_string(),
                        source_type: "installed".to_string(),
                        source_url: None,
                        ref_name: None,
                    },
                    BTreeMap::new(),
                    (),
                );
                let discovery = self
                    .payloads
                    .discover_with_source(
                        context.environment.clone(),
                        source_fingerprint,
                        storage.clone(),
                        retained,
                    )
                    .await?;
                let key = PayloadStorageKey::new(&discovery.session_id, skill_name.clone());
                let acquired = storage
                    .acquire_from_path(&key, &canonical_path, None)
                    .await?;
                self.payloads
                    .register_existing_payload_with_metadata(
                        &discovery,
                        skill_name.clone(),
                        acquired.manifest,
                        acquired.total_bytes,
                        installed_metadata(&skill_name, acquired.computed_hash),
                    )
                    .await
            }
        }
    }

    pub async fn current_manifest_hash(
        &self,
        context: &ContextRef,
        skill_name: &str,
        canonical: &ResolvedTargetFact,
    ) -> Result<String, AppError> {
        if canonical.entry_kind != TargetEntryKind::Directory
            || !same_environment_identity(&canonical.destination.environment, &context.environment)
        {
            return Err(AppError::StaleTarget);
        }
        match &context.environment {
            EnvironmentRef::Host => Ok(crate::core::skill_payload::build_skill_payload(
                Path::new(&canonical.destination.native_path),
            )?
            .manifest()
            .payload_root_hash),
            EnvironmentRef::Wsl { distro_name } => {
                let workspace = self.environments.workspace(distro_name)?;
                let storage = Arc::new(WslPayloadSessionStorage::new(workspace));
                let session_id = format!("copy-source-check-{}", uuid::Uuid::new_v4().simple());
                let key = PayloadStorageKey::new(&session_id, skill_name);
                let acquired = storage
                    .acquire_from_path(&key, &canonical.destination.native_path, None)
                    .await;
                let cleanup = storage.remove_session(&session_id).await;
                match (acquired, cleanup) {
                    (Ok(acquired), Ok(())) => Ok(acquired.manifest.payload_root_hash),
                    (Err(error), _) | (Ok(_), Err(error)) => Err(error),
                }
            }
        }
    }
}

fn installed_metadata(skill_name: &str, computed_hash: String) -> PayloadPlanningMetadata {
    PayloadPlanningMetadata {
        skill_name: skill_name.to_string(),
        install_dir_name: skill_name.to_string(),
        source: "installed-canonical".to_string(),
        source_type: "installed".to_string(),
        source_url: None,
        ref_name: None,
        skill_path: skill_name.to_string(),
        plugin_name: None,
        computed_hash,
        upstream_revision: None,
    }
}

impl<F, T> SkillEntryObserver<F, T> {
    pub fn new(facts: F, targets: T) -> Self {
        Self { facts, targets }
    }
}

impl<F, T> SkillEntryObserver<F, T>
where
    F: InstallPlanningFactSource,
    T: TargetFactResolver,
{
    pub async fn observe(
        &self,
        context: &ContextRef,
        skill_name: &str,
    ) -> Result<ObservedSkillSnapshot, AppError> {
        let facts = self.facts.current(context).await?;
        self.observe_with_facts(context, skill_name, facts).await
    }

    pub async fn observe_for_copy_source(
        &self,
        context: &ContextRef,
        skill_name: &str,
    ) -> Result<ObservedSkillSnapshot, AppError> {
        let facts = self.facts.current_for_copy_source(context).await?;
        self.observe_with_facts(context, skill_name, facts).await
    }

    async fn observe_with_facts(
        &self,
        context: &ContextRef,
        skill_name: &str,
        facts: InstallPlanningFacts,
    ) -> Result<ObservedSkillSnapshot, AppError> {
        let mut destinations = vec![join_entry(&facts.resolved_context.skill_root, skill_name)];
        let mut owners = Vec::new();
        for (agent_id, agent) in &facts.agent_runtime.agents {
            let scope = match &context.scope {
                ContextScope::Global => &agent.global,
                ContextScope::Project { .. } => &agent.project,
            };
            if agent.definition.adapter != AgentAdapter::Standard || !scope.enabled {
                continue;
            }
            let Some(root) = scope.private_path.as_deref() else {
                continue;
            };
            destinations.push(join_entry(
                &ResourceLocator {
                    environment: context.environment.clone(),
                    native_path: root.to_string(),
                },
                skill_name,
            ));
            owners.push(ObservedEntryOwner {
                agent_id: agent_id.clone(),
                display_name: agent.definition.display_name.clone(),
                logical_target_id: format!("agent:{}:private", agent_id.as_str()),
            });
        }
        if let Some((eve_id, eve)) = facts.agent_runtime.agents.iter().find(|(_, agent)| {
            agent.definition.adapter == AgentAdapter::Eve
                && agent.project.enabled
                && agent.detection == DetectionState::Detected
        }) {
            if !facts.eve_targets.is_empty() {
                for target in &facts.eve_targets {
                    destinations.push(join_entry(
                        &ResourceLocator {
                            environment: context.environment.clone(),
                            native_path: target.path.clone(),
                        },
                        skill_name,
                    ));
                    owners.push(ObservedEntryOwner {
                        agent_id: eve_id.clone(),
                        display_name: eve.definition.display_name.clone(),
                        logical_target_id: target.target_id.clone(),
                    });
                }
            }
        }
        let resolved = self.targets.resolve(context, &destinations, None).await?;
        if resolved.len() != destinations.len() || resolved.is_empty() {
            return Err(AppError::StaleTarget);
        }
        let canonical = resolved[0].clone();
        let candidates = resolved
            .into_iter()
            .skip(1)
            .zip(owners)
            .map(|(fact, owner)| ObservedEntryCandidate { fact, owner })
            .collect();
        let entries = group_observed_entries(&canonical, candidates)?;
        Ok(ObservedSkillSnapshot {
            facts,
            canonical,
            entries,
        })
    }
}

#[cfg(test)]
fn eve_target_ids_from_lock_entry(
    entry: Option<&serde_json::Value>,
) -> Result<Vec<String>, AppError> {
    match entry.and_then(|entry| entry.get("subagents")) {
        Some(serde_json::Value::Array(values)) => values
            .iter()
            .map(|value| {
                let subagent = value
                    .as_str()
                    .ok_or_else(|| AppError::ConfigurationCorrupted {
                        message: "Eve placement must contain only strings".to_string(),
                    })?;
                Ok(if subagent.is_empty() {
                    "eve:root".to_string()
                } else {
                    format!("eve:{}", crate::core::skill::sanitize_name(subagent))
                })
            })
            .collect(),
        Some(_) => Err(AppError::ConfigurationCorrupted {
            message: "Eve placement must be an array".to_string(),
        }),
        None => Ok(vec!["eve:root".to_string()]),
    }
}

pub fn join_entry(root: &ResourceLocator, child: &str) -> ResourceLocator {
    let separator = if matches!(root.environment, EnvironmentRef::Host) && cfg!(windows) {
        '\\'
    } else {
        '/'
    };
    ResourceLocator {
        environment: root.environment.clone(),
        native_path: format!(
            "{}{}{}",
            root.native_path.trim_end_matches(['/', '\\']),
            separator,
            child
        ),
    }
}

pub fn group_observed_entries(
    canonical: &ResolvedTargetFact,
    candidates: Vec<ObservedEntryCandidate>,
) -> Result<Vec<ObservedPlannedEntry>, AppError> {
    let mut grouped = BTreeMap::<PhysicalTargetKey, ObservedPlannedEntry>::new();
    for candidate in candidates {
        if candidate.fact.entry_kind == TargetEntryKind::Missing
            || candidate.fact.key == canonical.key
        {
            continue;
        }
        let kind = observed_entry_kind(candidate.fact.entry_kind);
        let will_break = matches!(
            candidate.fact.entry_kind,
            TargetEntryKind::Symlink | TargetEntryKind::Junction
        ) && candidate
            .fact
            .link_target
            .as_deref()
            .is_some_and(|target| link_points_to(&candidate.fact, target, canonical));
        let entry = grouped
            .entry(candidate.fact.key.clone())
            .or_insert_with(|| ObservedPlannedEntry {
                public: ObservedPhysicalEntry {
                    entry_id: observed_entry_id(&candidate.fact.key, &candidate.fact.fingerprint)
                        .expect("validated physical facts produce observed IDs"),
                    display_path: display_locator(&candidate.fact.destination),
                    kind,
                    physical_target_key: stable_digest(&candidate.fact.key)
                        .expect("validated physical keys are serializable"),
                    owners: Vec::new(),
                    will_break_if_canonical_removed: will_break,
                },
                fact: candidate.fact.clone(),
            });
        if entry.fact.destination != candidate.fact.destination
            || entry.fact.fingerprint != candidate.fact.fingerprint
        {
            return Err(AppError::StaleTarget);
        }
        entry.public.owners.push(candidate.owner);
        entry.public.will_break_if_canonical_removed |= will_break;
    }
    for entry in grouped.values_mut() {
        entry
            .public
            .owners
            .sort_by(|left, right| left.agent_id.cmp(&right.agent_id));
        entry
            .public
            .owners
            .dedup_by(|left, right| left.agent_id == right.agent_id);
    }
    Ok(grouped.into_values().collect())
}

fn display_locator(locator: &ResourceLocator) -> ResourceLocator {
    let native_path = if let Some(suffix) = locator.native_path.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{suffix}")
    } else if let Some(suffix) = locator
        .native_path
        .strip_prefix(r"\\?\")
        .or_else(|| locator.native_path.strip_prefix(r"\??\"))
    {
        suffix.to_string()
    } else {
        locator.native_path.clone()
    };
    ResourceLocator {
        environment: locator.environment.clone(),
        native_path,
    }
}

pub fn observed_entry_kind(kind: TargetEntryKind) -> ObservedEntryKind {
    match kind {
        TargetEntryKind::Missing => ObservedEntryKind::Missing,
        TargetEntryKind::Directory => ObservedEntryKind::Directory,
        TargetEntryKind::Symlink => ObservedEntryKind::Symlink,
        TargetEntryKind::Junction => ObservedEntryKind::Junction,
        TargetEntryKind::BrokenLink => ObservedEntryKind::BrokenLink,
        TargetEntryKind::File | TargetEntryKind::Other => ObservedEntryKind::Other,
    }
}

pub(crate) fn link_points_to(
    link: &ResolvedTargetFact,
    raw_target: &str,
    canonical: &ResolvedTargetFact,
) -> bool {
    let target = Path::new(raw_target);
    let resolved = if target.is_absolute() {
        lexical_normalize(target)
    } else {
        let Some(parent) = Path::new(&link.destination.native_path).parent() else {
            return false;
        };
        lexical_normalize(&parent.join(target))
    };
    resolved == lexical_normalize(Path::new(&canonical.destination.native_path))
}

fn lexical_normalize(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            other => normalized.push(other.as_os_str()),
        }
    }
    normalized
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::payload_session::{PayloadSessionLimits, PayloadSessionManager};
    use crate::application::remove::ObservedEntryKind;
    use crate::core::agent_definition::AgentId;
    use crate::environment::planning::{ResolvedTargetFact, TargetEntryKind};
    use crate::environment::runtime::{
        EntryFingerprint, ExecutionBackend, PhysicalParentIdentity, PhysicalTargetKey,
    };
    use crate::environment::types::{EnvironmentRef, ResourceLocator};
    use crate::environment::wsl::WslRuntime;
    use std::sync::Arc;
    use tempfile::tempdir;

    #[test]
    fn eve_targets_distinguish_legacy_missing_metadata_from_explicit_empty_targets() {
        assert_eq!(
            eve_target_ids_from_lock_entry(Some(&serde_json::json!({}))).unwrap(),
            vec!["eve:root"]
        );
        assert!(eve_target_ids_from_lock_entry(Some(&serde_json::json!({
            "subagents": []
        })))
        .unwrap()
        .is_empty());
        assert_eq!(
            eve_target_ids_from_lock_entry(Some(&serde_json::json!({
                "subagents": ["", "Research Team"]
            })))
            .unwrap(),
            vec!["eve:root", "eve:research-team"]
        );
    }

    #[test]
    fn eve_targets_reject_malformed_lock_metadata() {
        for entry in [
            serde_json::json!({ "subagents": "root" }),
            serde_json::json!({ "subagents": ["", 1] }),
        ] {
            assert!(matches!(
                eve_target_ids_from_lock_entry(Some(&entry)),
                Err(AppError::ConfigurationCorrupted { .. })
            ));
        }
    }

    #[test]
    fn physical_entries_group_all_owners_and_mark_links_to_canonical() {
        let canonical = fact(
            "canonical",
            "/skills/demo",
            TargetEntryKind::Directory,
            None,
        );
        let copy = fact("copy", "/agent-copy/demo", TargetEntryKind::Directory, None);
        let link = fact(
            "link",
            "/agent-link/demo",
            TargetEntryKind::Symlink,
            Some("/skills/demo"),
        );
        let entries = group_observed_entries(
            &canonical,
            vec![
                candidate(copy.clone(), "agent-a"),
                candidate(copy, "agent-b"),
                candidate(link, "agent-link"),
            ],
        )
        .unwrap();

        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].public.owners.len(), 2);
        assert_eq!(entries[0].public.kind, ObservedEntryKind::Directory);
        assert_eq!(entries[1].public.kind, ObservedEntryKind::Symlink);
        assert!(entries[1].public.will_break_if_canonical_removed);
    }

    #[test]
    fn physical_entries_hide_windows_verbatim_prefixes_from_display_paths() {
        let canonical = fact(
            "canonical",
            r"D:\Code\skills\.agents\skills\demo",
            TargetEntryKind::Directory,
            None,
        );
        let drive_link = fact(
            "drive-link",
            r"\\?\D:\Code\skills\.kiro\skills\demo",
            TargetEntryKind::Symlink,
            Some(r"D:\Code\skills\.agents\skills\demo"),
        );
        let unc_link = fact(
            "unc-link",
            r"\\?\UNC\server\share\.junie\skills\demo",
            TargetEntryKind::Symlink,
            Some(r"D:\Code\skills\.agents\skills\demo"),
        );

        let entries = group_observed_entries(
            &canonical,
            vec![candidate(drive_link, "kiro"), candidate(unc_link, "junie")],
        )
        .unwrap();

        assert_eq!(
            entries[0].public.display_path.native_path,
            r"D:\Code\skills\.kiro\skills\demo"
        );
        assert_eq!(
            entries[1].public.display_path.native_path,
            r"\\server\share\.junie\skills\demo"
        );
        assert_eq!(
            entries[0].fact.destination.native_path,
            r"\\?\D:\Code\skills\.kiro\skills\demo"
        );
    }

    #[tokio::test]
    async fn installed_canonical_acquisition_keeps_the_complete_directory_payload() {
        let temp = tempdir().unwrap();
        let canonical = temp.path().join("demo");
        std::fs::create_dir_all(canonical.join("scripts")).unwrap();
        std::fs::write(canonical.join("SKILL.md"), b"demo").unwrap();
        std::fs::write(canonical.join("scripts/run.sh"), b"#!/bin/sh\n").unwrap();
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
        let context = crate::environment::types::ContextRef {
            environment: EnvironmentRef::Host,
            scope: crate::environment::types::ContextScope::Global,
        };
        let canonical_fact = fact(
            "demo",
            canonical.to_string_lossy().as_ref(),
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

    fn candidate(fact: ResolvedTargetFact, id: &str) -> ObservedEntryCandidate {
        ObservedEntryCandidate {
            fact,
            owner: crate::application::remove::ObservedEntryOwner {
                agent_id: AgentId::parse(id).unwrap(),
                display_name: id.to_string(),
                logical_target_id: format!("agent:{id}:private"),
            },
        }
    }

    fn fact(
        name: &str,
        path: &str,
        entry_kind: TargetEntryKind,
        link_target: Option<&str>,
    ) -> ResolvedTargetFact {
        ResolvedTargetFact {
            key: PhysicalTargetKey {
                backend: ExecutionBackend::NativeUnix,
                physical_parent: PhysicalParentIdentity::Unix {
                    device: 1,
                    inode: if name == "copy" { 2 } else { 3 },
                },
                normalized_final_child_name: name.to_string(),
            },
            destination: ResourceLocator {
                environment: EnvironmentRef::Host,
                native_path: path.to_string(),
            },
            fingerprint: EntryFingerprint(format!("entry-v1-{name}")),
            entry_kind,
            link_target: link_target.map(str::to_string),
        }
    }
}

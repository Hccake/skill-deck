use std::collections::BTreeMap;

mod election;

use crate::application::agent_selection::{
    AgentInstallOptionKind, AgentSelectionCatalog, DirectoryPlacementId,
};
use crate::application::installed_skill_resolver::SkillDirectoryName;
use crate::application::library_agent_placements::{
    LibraryAgentPlacementError, LibraryAgentPlacementMap,
};
use crate::application::library_candidates::{LibraryCandidateError, LibraryCandidateSet};
use crate::application::mutation::plan::{
    stable_digest, ExpectedTargetEntry, PreparedEntryAction, PreparedEntryMutation,
};
use crate::application::mutation::planning::PreparedMutationEntries;
use crate::application::skill_entry_projection::{
    observed_entry_kind, ObservedEntryReader, ObservedPhysicalEntry, ObservedPlannedEntry,
};
use crate::core::agent_definition::AgentId;
use crate::core::skill_payload::PayloadId;
use crate::environment::planning::{ResolvedTargetFact, TargetEntryKind};
use crate::environment::runtime::{observed_entry_id, PhysicalTargetKey};
use crate::environment::types::{display_locator, SkillLocationRef};
use crate::error::{AppError, SkillPlacementTargetKind};
use election::{
    plan_physical_skill_directories, DirectVersionCandidate, PhysicalSkillDirectoryConflict,
    PhysicalSkillDirectoryRequest, PlacementVersionDemand, SkillDirectoryInputError,
    SkillDirectoryPlacementInput,
};
pub(crate) use election::{
    DirectoryPlacementRef, DirectoryUpdate, ElectedVersion, ObservedVersion,
};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum DirectContentIdentity {
    Existing(PhysicalTargetKey),
    Payload(PayloadId),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PreparedDirectVersion {
    content: DirectContentIdentity,
    action: PreparedEntryAction,
}

#[derive(Debug, Clone)]
struct CurrentDirectVersion {
    content: DirectContentIdentity,
    candidate: DirectVersionCandidate,
}

impl PreparedDirectVersion {
    pub(crate) fn new(content: DirectContentIdentity, action: PreparedEntryAction) -> Self {
        Self { content, action }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DirectPlacementChange {
    Preserve,
    Set(PreparedDirectVersion),
    Clear,
}

#[derive(Debug, Clone)]
pub(crate) struct ScopeSkillPlacementSet {
    context: SkillLocationRef,
    resolved: BTreeMap<DirectoryPlacementId, ResolvedTargetFact>,
}

impl ScopeSkillPlacementSet {
    pub(crate) fn new(
        context: SkillLocationRef,
        resolved: BTreeMap<DirectoryPlacementId, ResolvedTargetFact>,
    ) -> Self {
        Self { context, resolved }
    }

    pub(crate) fn facts(&self) -> &BTreeMap<DirectoryPlacementId, ResolvedTargetFact> {
        &self.resolved
    }
}

#[derive(Debug, Clone)]
pub(crate) struct LegacyLibraryPlacement {
    pub(crate) fact: ResolvedTargetFact,
    pub(crate) reader_agent_ids: Vec<AgentId>,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct LibraryElectionState<'a> {
    pub(crate) candidates: &'a LibraryCandidateSet,
    pub(crate) selected_agent_ids: &'a [AgentId],
}

pub(crate) struct DirectSkillChangeRequest<'a> {
    pub(crate) skill: SkillDirectoryName,
    pub(crate) catalog: &'a AgentSelectionCatalog,
    pub(crate) placements: ScopeSkillPlacementSet,
    pub(crate) libraries: LibraryElectionState<'a>,
    pub(crate) direct_changes: BTreeMap<DirectoryPlacementId, DirectPlacementChange>,
}

pub(crate) struct LibrarySkillChangeRequest<'a> {
    pub(crate) skill: SkillDirectoryName,
    pub(crate) catalog: &'a AgentSelectionCatalog,
    pub(crate) placements: ScopeSkillPlacementSet,
    pub(crate) before: LibraryElectionState<'a>,
    pub(crate) after: LibraryElectionState<'a>,
    pub(crate) legacy: Vec<LegacyLibraryPlacement>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ScopeSkillPlanningError {
    ScopeMismatch,
    MissingStandard,
    MissingPlacement(DirectoryPlacementId),
    CatalogPlacementMismatch(DirectoryPlacementId),
    LibraryCandidate(LibraryCandidateError),
    InvalidInput(SkillDirectoryInputError),
    LibraryAgentPlacement(LibraryAgentPlacementError),
    ConflictingDirectContent {
        target_path: String,
    },
    ConflictingDirectMaterialization {
        target_path: String,
    },
    ExternalTarget {
        skill_name: String,
        target_path: String,
        target_kind: SkillPlacementTargetKind,
        agent_ids: Vec<AgentId>,
    },
    Physical(PhysicalSkillDirectoryConflict),
}

impl ScopeSkillPlanningError {
    pub(crate) fn into_app_error(self) -> AppError {
        match self {
            Self::ScopeMismatch => AppError::StaleContext,
            Self::MissingStandard => AppError::ConfigurationCorrupted {
                message: "Scope Skill planning is missing the Standard placement".to_string(),
            },
            Self::MissingPlacement(placement) => AppError::ConfigurationCorrupted {
                message: format!("Scope Skill planning is missing placement {placement:?}"),
            },
            Self::CatalogPlacementMismatch(placement) => AppError::ConfigurationCorrupted {
                message: format!(
                    "Scope Skill planning facts do not match catalog placement {placement:?}"
                ),
            },
            Self::LibraryCandidate(error) => AppError::ConfigurationCorrupted {
                message: format!("Scope Skill Library candidates are inconsistent: {error:?}"),
            },
            Self::InvalidInput(error) => AppError::ConfigurationCorrupted {
                message: format!("Scope Skill planning input is inconsistent: {error:?}"),
            },
            Self::LibraryAgentPlacement(LibraryAgentPlacementError::UnknownAgent(agent)) => {
                AppError::InvalidAgent {
                    agent: agent.as_str().to_string(),
                }
            }
            Self::LibraryAgentPlacement(LibraryAgentPlacementError::PartialSelection(_)) => {
                AppError::AgentSelectionInvalid {
                    reason: crate::error::AgentSelectionInvalidReason::OptionUnavailable,
                }
            }
            Self::ConflictingDirectContent { target_path }
            | Self::ConflictingDirectMaterialization { target_path } => AppError::Validation {
                field: Some("skillName".to_string()),
                message: format!("direct Skill content conflicts at {target_path}"),
            },
            Self::ExternalTarget {
                skill_name,
                target_path,
                target_kind,
                agent_ids,
            } => AppError::SkillPlacementTargetConflict {
                skill_name,
                agent_ids,
                target_path,
                target_kind,
            },
            Self::Physical(PhysicalSkillDirectoryConflict::ExternalTarget {
                key: _,
                skill_name,
                target_path,
                target_kind,
            }) => AppError::SkillPlacementTargetConflict {
                skill_name,
                agent_ids: Vec::new(),
                target_path,
                target_kind,
            },
            Self::Physical(PhysicalSkillDirectoryConflict::ConflictingDirectVersions {
                target_path,
                ..
            }) => AppError::Validation {
                field: Some("skillName".to_string()),
                message: format!("direct Skill versions conflict at {target_path}"),
            },
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ScopePlannedDirectory {
    fact: ResolvedTargetFact,
    placements: Vec<DirectoryPlacementRef>,
    observed: ObservedVersion,
    elected: ElectedVersion,
    action: PreparedEntryAction,
    readers: Vec<ObservedEntryReader>,
}

impl ScopePlannedDirectory {
    pub(crate) fn fact(&self) -> &ResolvedTargetFact {
        &self.fact
    }

    pub(crate) fn placements(&self) -> &[DirectoryPlacementRef] {
        &self.placements
    }

    pub(crate) fn observed(&self) -> &ObservedVersion {
        &self.observed
    }

    pub(crate) fn elected(&self) -> &ElectedVersion {
        &self.elected
    }

    pub(crate) fn action(&self) -> &PreparedEntryAction {
        &self.action
    }

    pub(crate) fn update(&self) -> DirectoryUpdate {
        match self.action {
            PreparedEntryAction::Keep => DirectoryUpdate::Unchanged,
            PreparedEntryAction::Remove => DirectoryUpdate::Remove,
            _ if matches!(self.elected, ElectedVersion::Library(_)) => DirectoryUpdate::UseLibrary,
            _ => DirectoryUpdate::UseDirect,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ScopeSkillPlan {
    directories: Vec<ScopePlannedDirectory>,
}

impl ScopeSkillPlan {
    pub(crate) fn directories(&self) -> &[ScopePlannedDirectory] {
        &self.directories
    }

    pub(crate) fn standard_fact(&self) -> Result<&ResolvedTargetFact, ScopeSkillPlanningError> {
        self.directories
            .iter()
            .find(|directory| {
                directory
                    .placements
                    .contains(&DirectoryPlacementRef::Catalog(
                        DirectoryPlacementId::Standard,
                    ))
            })
            .map(|directory| &directory.fact)
            .ok_or(ScopeSkillPlanningError::MissingStandard)
    }

    pub(crate) fn project_observed_entries(
        &self,
    ) -> Result<Vec<ObservedPlannedEntry>, ScopeSkillPlanningError> {
        let standard = self.standard_fact()?;
        Ok(self
            .directories
            .iter()
            .filter(|directory| {
                directory.fact.entry_kind != TargetEntryKind::Missing
                    && !directory
                        .placements
                        .contains(&DirectoryPlacementRef::Catalog(
                            DirectoryPlacementId::Standard,
                        ))
            })
            .map(|directory| {
                let will_break_if_standard_removed = matches!(
                    directory.fact.entry_kind,
                    TargetEntryKind::Symlink | TargetEntryKind::Junction
                ) && directory
                    .fact
                    .link_target_identity
                    .as_ref()
                    .is_some_and(|identity| identity.matches(&standard.destination));
                ObservedPlannedEntry {
                    public: ObservedPhysicalEntry {
                        entry_id: observed_entry_id(
                            &directory.fact.key,
                            &directory.fact.fingerprint,
                        )
                        .expect("validated physical facts produce observed IDs"),
                        display_path: display_locator(&directory.fact.destination),
                        kind: observed_entry_kind(directory.fact.entry_kind),
                        physical_target_key: stable_digest(&directory.fact.key)
                            .expect("validated physical keys are serializable"),
                        readers: directory.readers.clone(),
                        will_break_if_standard_removed,
                    },
                    fact: directory.fact.clone(),
                }
            })
            .collect())
    }

    pub(crate) fn compile_entries(&self) -> PreparedMutationEntries {
        let mut primary = None;
        let mut additional = Vec::new();
        let mut expected_targets = Vec::new();
        for directory in &self.directories {
            let mut reader_agent_ids = directory
                .readers
                .iter()
                .map(|reader| reader.agent_id.clone())
                .collect::<Vec<_>>();
            reader_agent_ids.sort();
            reader_agent_ids.dedup();
            let mutation = PreparedEntryMutation {
                key: directory.fact.key.clone(),
                destination: directory.fact.destination.clone(),
                action: directory.action.clone(),
                reader_agent_ids,
            };
            expected_targets.push(ExpectedTargetEntry {
                key: directory.fact.key.clone(),
                fingerprint: directory.fact.fingerprint.clone(),
                expected_content_manifest_hash: None,
            });
            if directory
                .placements
                .contains(&DirectoryPlacementRef::Catalog(
                    DirectoryPlacementId::Standard,
                ))
            {
                primary = Some(mutation);
            } else {
                additional.push(mutation);
            }
        }
        PreparedMutationEntries {
            primary,
            additional,
            expected_targets,
        }
    }
}

pub(crate) struct ScopeSkillPlanner;

impl ScopeSkillPlanner {
    pub(crate) fn plan_direct_change(
        request: DirectSkillChangeRequest<'_>,
    ) -> Result<ScopeSkillPlan, ScopeSkillPlanningError> {
        plan(
            request.skill,
            request.catalog,
            request.placements,
            request.libraries,
            request.libraries,
            request.direct_changes,
            Vec::new(),
        )
    }

    pub(crate) fn plan_library_change(
        request: LibrarySkillChangeRequest<'_>,
    ) -> Result<ScopeSkillPlan, ScopeSkillPlanningError> {
        plan(
            request.skill,
            request.catalog,
            request.placements,
            request.before,
            request.after,
            BTreeMap::new(),
            request.legacy,
        )
    }
}

fn plan(
    skill: SkillDirectoryName,
    catalog: &AgentSelectionCatalog,
    placements: ScopeSkillPlacementSet,
    before: LibraryElectionState<'_>,
    after: LibraryElectionState<'_>,
    direct_changes: BTreeMap<DirectoryPlacementId, DirectPlacementChange>,
    legacy: Vec<LegacyLibraryPlacement>,
) -> Result<ScopeSkillPlan, ScopeSkillPlanningError> {
    if catalog.context() != &placements.context {
        return Err(ScopeSkillPlanningError::ScopeMismatch);
    }
    if catalog.standard().id != DirectoryPlacementId::Standard {
        return Err(ScopeSkillPlanningError::MissingStandard);
    }
    let standard = placements
        .resolved
        .get(&DirectoryPlacementId::Standard)
        .ok_or(ScopeSkillPlanningError::MissingStandard)?;
    for option in catalog.options() {
        let placement_id = DirectoryPlacementId::Option(option.public.id.clone());
        if !placements.resolved.contains_key(&placement_id) {
            return Err(ScopeSkillPlanningError::CatalogPlacementMismatch(
                placement_id,
            ));
        }
    }
    let library_placements = LibraryAgentPlacementMap::from_catalog(catalog);
    let before_library = library_placements
        .placements_for(before.selected_agent_ids)
        .map_err(ScopeSkillPlanningError::LibraryAgentPlacement)?;
    let after_library = library_placements
        .placements_for(after.selected_agent_ids)
        .map_err(ScopeSkillPlanningError::LibraryAgentPlacement)?;
    validate_prepared_direct_changes(&placements.resolved, &direct_changes)?;

    let mut recognized = before.candidates.recognized().to_vec();
    for candidate in after.candidates.recognized() {
        if !recognized.contains(candidate) {
            recognized.push(candidate.clone());
        }
    }
    let candidates = LibraryCandidateSet::new(recognized, after.candidates.ordered().to_vec())
        .map_err(ScopeSkillPlanningError::LibraryCandidate)?;
    let before_has_library = !before.candidates.ordered().is_empty();
    let after_has_library = !after.candidates.ordered().is_empty();
    let prepared_keys = direct_changes
        .iter()
        .filter_map(|(placement_id, change)| {
            matches!(change, DirectPlacementChange::Set(_))
                .then(|| {
                    placements
                        .resolved
                        .get(placement_id)
                        .map(|fact| fact.key.clone())
                })
                .flatten()
        })
        .collect::<std::collections::BTreeSet<_>>();
    let display_names = catalog
        .snapshot()
        .agents
        .iter()
        .map(|agent| (agent.id.clone(), agent.display_name.clone()))
        .collect::<BTreeMap<_, _>>();

    let mut inputs = Vec::new();
    let mut readers_by_key = BTreeMap::<PhysicalTargetKey, Vec<ObservedEntryReader>>::new();
    let mut preserved_content = BTreeMap::<PhysicalTargetKey, DirectContentIdentity>::new();
    for (placement_id, fact) in &placements.resolved {
        if catalog.placement(placement_id).is_none() {
            return Err(ScopeSkillPlanningError::MissingPlacement(
                placement_id.clone(),
            ));
        }
        let current_direct = current_direct_version(fact, placement_id, standard, &candidates);
        if !prepared_keys.contains(&fact.key) {
            if let Some(current) = &current_direct {
                if let Some(existing) = preserved_content.get(&fact.key) {
                    if existing != &current.content {
                        return Err(ScopeSkillPlanningError::ConflictingDirectContent {
                            target_path: fact.destination.native_path.clone(),
                        });
                    }
                } else {
                    preserved_content.insert(fact.key.clone(), current.content.clone());
                }
            }
        }
        let current_candidate = current_direct.map(|current| current.candidate);
        let target_direct = match direct_changes
            .get(placement_id)
            .unwrap_or(&DirectPlacementChange::Preserve)
        {
            DirectPlacementChange::Preserve => current_candidate.clone(),
            DirectPlacementChange::Set(prepared) => {
                Some(DirectVersionCandidate::prepared(prepared.action.clone()))
            }
            DirectPlacementChange::Clear => None,
        };
        let before_library_here = before_has_library
            && (placement_id == &DirectoryPlacementId::Standard
                || before_library.contains(placement_id));
        let after_library_here = after_has_library
            && (placement_id == &DirectoryPlacementId::Standard
                || after_library.contains(placement_id));
        inputs.push(
            SkillDirectoryPlacementInput::new(
                DirectoryPlacementRef::Catalog(placement_id.clone()),
                fact.clone(),
                demand(current_candidate, before_library_here),
                demand(target_direct, after_library_here),
            )
            .map_err(ScopeSkillPlanningError::InvalidInput)?,
        );
        let readers = readers_for_placement(catalog, placement_id, &display_names);
        readers_by_key
            .entry(fact.key.clone())
            .or_default()
            .extend(readers);
    }
    for legacy in legacy {
        readers_by_key
            .entry(legacy.fact.key.clone())
            .or_default()
            .extend(legacy.reader_agent_ids.iter().map(|agent_id| {
                ObservedEntryReader {
                    agent_id: agent_id.clone(),
                    display_name: display_names
                        .get(agent_id)
                        .cloned()
                        .unwrap_or_else(|| agent_id.as_str().to_string()),
                    logical_target_id: format!("agent:{}:private", agent_id.as_str()),
                }
            }));
        inputs.push(
            SkillDirectoryPlacementInput::new(
                DirectoryPlacementRef::Legacy,
                legacy.fact,
                if before_has_library {
                    PlacementVersionDemand::library()
                } else {
                    PlacementVersionDemand::default()
                },
                PlacementVersionDemand::default(),
            )
            .map_err(ScopeSkillPlanningError::InvalidInput)?,
        );
    }

    let physical = plan_physical_skill_directories(
        PhysicalSkillDirectoryRequest::new(
            skill,
            inputs,
            candidates,
            before.candidates.recognized().to_vec(),
        )
        .map_err(ScopeSkillPlanningError::InvalidInput)?,
    )
    .map_err(|conflict| match conflict {
        PhysicalSkillDirectoryConflict::ExternalTarget {
            key,
            skill_name,
            target_path,
            target_kind,
        } => {
            let mut agent_ids = readers_by_key
                .get(key.as_ref())
                .into_iter()
                .flatten()
                .map(|reader| reader.agent_id.clone())
                .collect::<Vec<_>>();
            agent_ids.sort();
            agent_ids.dedup();
            ScopeSkillPlanningError::ExternalTarget {
                skill_name,
                target_path,
                target_kind,
                agent_ids,
            }
        }
        other => ScopeSkillPlanningError::Physical(other),
    })?;
    let mut directories = Vec::new();
    for directory in physical.directories() {
        let mut readers = readers_by_key
            .remove(&directory.fact().key)
            .unwrap_or_default();
        readers.sort_by(|left, right| {
            (&left.agent_id, &left.logical_target_id)
                .cmp(&(&right.agent_id, &right.logical_target_id))
        });
        readers.dedup_by(|left, right| {
            left.agent_id == right.agent_id && left.logical_target_id == right.logical_target_id
        });
        directories.push(ScopePlannedDirectory {
            fact: directory.fact().clone(),
            placements: directory.placements().to_vec(),
            observed: directory.observed().clone(),
            elected: directory.elected().clone(),
            action: directory.action(),
            readers,
        });
    }
    Ok(ScopeSkillPlan { directories })
}

fn readers_for_placement(
    catalog: &AgentSelectionCatalog,
    placement_id: &DirectoryPlacementId,
    display_names: &BTreeMap<AgentId, String>,
) -> Vec<ObservedEntryReader> {
    match placement_id {
        DirectoryPlacementId::Standard => catalog
            .readers(placement_id)
            .into_iter()
            .map(|agent_id| ObservedEntryReader {
                display_name: display_names
                    .get(&agent_id)
                    .cloned()
                    .unwrap_or_else(|| agent_id.as_str().to_string()),
                logical_target_id: format!("agent:{}:standard", agent_id.as_str()),
                agent_id,
            })
            .collect(),
        DirectoryPlacementId::Option(option_id) => catalog
            .option(option_id)
            .into_iter()
            .flat_map(|option| {
                option.public.agent_ids.iter().cloned().map(|agent_id| {
                    let logical_target_id =
                        if option.public.kind == AgentInstallOptionKind::GroupLocation {
                            option.target_id()
                        } else {
                            format!("agent:{}:private", agent_id.as_str())
                        };
                    ObservedEntryReader {
                        display_name: display_names
                            .get(&agent_id)
                            .cloned()
                            .unwrap_or_else(|| option.public.display_name.clone()),
                        logical_target_id,
                        agent_id,
                    }
                })
            })
            .collect(),
    }
}

fn demand(direct: Option<DirectVersionCandidate>, library: bool) -> PlacementVersionDemand {
    match (direct, library) {
        (Some(direct), true) => PlacementVersionDemand::direct(direct).with_library(),
        (Some(direct), false) => PlacementVersionDemand::direct(direct),
        (None, true) => PlacementVersionDemand::library(),
        (None, false) => PlacementVersionDemand::default(),
    }
}

fn current_direct_version(
    fact: &ResolvedTargetFact,
    placement: &DirectoryPlacementId,
    standard: &ResolvedTargetFact,
    libraries: &LibraryCandidateSet,
) -> Option<CurrentDirectVersion> {
    let link_is_library = fact.link_target_identity.as_ref().is_some_and(|identity| {
        libraries
            .recognized()
            .iter()
            .any(|candidate| identity.matches(candidate.locator()))
    });
    let source =
        (placement != &DirectoryPlacementId::Standard).then_some(standard.destination.clone());
    let direct = match fact.entry_kind {
        TargetEntryKind::Directory => Some(DirectVersionCandidate::existing(source)),
        TargetEntryKind::Symlink | TargetEntryKind::Junction if !link_is_library => {
            Some(DirectVersionCandidate::existing(source))
        }
        TargetEntryKind::BrokenLink
            if source.as_ref().is_some_and(|source| {
                fact.link_target_identity
                    .as_ref()
                    .is_some_and(|identity| identity.matches(source))
            }) =>
        {
            Some(DirectVersionCandidate::existing(source))
        }
        _ => None,
    }?;
    let content = if placement == &DirectoryPlacementId::Standard
        || matches!(
            fact.entry_kind,
            TargetEntryKind::Symlink | TargetEntryKind::Junction | TargetEntryKind::BrokenLink
        ) {
        DirectContentIdentity::Existing(standard.key.clone())
    } else {
        DirectContentIdentity::Existing(fact.key.clone())
    };
    Some(CurrentDirectVersion {
        content,
        candidate: direct,
    })
}

fn validate_prepared_direct_changes(
    placements: &BTreeMap<DirectoryPlacementId, ResolvedTargetFact>,
    changes: &BTreeMap<DirectoryPlacementId, DirectPlacementChange>,
) -> Result<(), ScopeSkillPlanningError> {
    let mut prepared =
        BTreeMap::<PhysicalTargetKey, (&DirectContentIdentity, &PreparedEntryAction, &str)>::new();
    for (placement_id, change) in changes {
        let DirectPlacementChange::Set(version) = change else {
            continue;
        };
        let fact = placements
            .get(placement_id)
            .ok_or_else(|| ScopeSkillPlanningError::MissingPlacement(placement_id.clone()))?;
        match prepared.get(&fact.key) {
            Some((content, _, _)) if *content != &version.content => {
                return Err(ScopeSkillPlanningError::ConflictingDirectContent {
                    target_path: fact.destination.native_path.clone(),
                });
            }
            Some((_, action, _)) if *action != &version.action => {
                return Err(ScopeSkillPlanningError::ConflictingDirectMaterialization {
                    target_path: fact.destination.native_path.clone(),
                });
            }
            Some(_) => {}
            None => {
                prepared.insert(
                    fact.key.clone(),
                    (
                        &version.content,
                        &version.action,
                        &fact.destination.native_path,
                    ),
                );
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::agent_selection::{
        AgentInstallOption, AgentInstallOptionId, AgentInstallOptionKind, AgentSelectionAgent,
        AgentSelectionAgentKind, AgentSelectionModeConstraint, AgentSelectionRevision,
        AgentSelectionSnapshot, ResolvedAgentInstallOption, ResolvedDirectoryPlacement,
        SkillDirectoryAccess,
    };
    use crate::application::library_candidates::LibraryVersionCandidate;
    use crate::application::skill_libraries::LibraryId;
    use crate::environment::agent_environment::DetectionState;
    use crate::environment::planning::TargetEntryKind;
    use crate::environment::runtime::{EntryFingerprint, ExecutionBackend, PhysicalParentIdentity};
    use crate::environment::types::{
        EnvironmentRef, ResourceLocator, SkillLocation, StorageAccess,
    };

    fn key(name: &str) -> PhysicalTargetKey {
        PhysicalTargetKey {
            backend: if cfg!(windows) {
                ExecutionBackend::NativeWindows
            } else {
                ExecutionBackend::NativeUnix
            },
            physical_parent: if cfg!(windows) {
                PhysicalParentIdentity::Windows {
                    volume_serial: 1,
                    file_id: 2,
                }
            } else {
                PhysicalParentIdentity::Unix {
                    device: 1,
                    inode: 2,
                }
            },
            normalized_final_child_name: name.to_string(),
        }
    }

    fn native_path(path: &str) -> String {
        let mut result = if cfg!(windows) {
            std::path::PathBuf::from(r"C:\")
        } else {
            std::path::PathBuf::from("/")
        };
        for component in path.trim_start_matches('/').split('/') {
            result.push(component);
        }
        result.to_string_lossy().into_owned()
    }

    fn locator(path: &str) -> ResourceLocator {
        ResourceLocator {
            environment: EnvironmentRef::Native,
            native_path: native_path(path),
        }
    }

    fn fact(name: &str, kind: TargetEntryKind, link_target: Option<&str>) -> ResolvedTargetFact {
        let destination = locator(&format!("/scope/{name}"));
        let link_target = link_target.map(native_path);
        ResolvedTargetFact {
            key: key(name),
            destination: destination.clone(),
            storage_access: StorageAccess::Native,
            fingerprint: EntryFingerprint(format!("entry-v1-{name}")),
            entry_kind: kind,
            link_target_identity: link_target.as_deref().and_then(|raw| {
                crate::environment::planning::resolve_link_target_identity(&destination, raw)
            }),
            link_target,
        }
    }

    fn context() -> SkillLocationRef {
        SkillLocationRef {
            environment: EnvironmentRef::Native,
            scope: SkillLocation::Global,
        }
    }

    fn catalog() -> AgentSelectionCatalog {
        AgentSelectionCatalog::from_parts_for_test(
            context(),
            AgentSelectionSnapshot {
                agents: Vec::new(),
                install_options: Vec::new(),
                groups: Vec::new(),
                initial_selected_option_ids: Vec::new(),
                unavailable_explicit_agents: Vec::new(),
                user_mode_option_ids: Vec::new(),
                revision: AgentSelectionRevision("revision".to_string()),
            },
            BTreeMap::new(),
            ResolvedDirectoryPlacement {
                id: DirectoryPlacementId::Standard,
                root: locator("/scope"),
                physical_key: key("root"),
                storage_access: StorageAccess::Native,
                content: crate::application::agent_selection::DirectoryContentKind::Original,
            },
        )
    }

    fn catalog_with_option() -> AgentSelectionCatalog {
        let option_id = AgentInstallOptionId("private".to_string());
        let agent_id = AgentId::parse("private-agent").unwrap();
        let public = AgentInstallOption {
            id: option_id.clone(),
            kind: AgentInstallOptionKind::StandardDirectory,
            agent_ids: vec![agent_id.clone()],
            display_name: "Private Agent".to_string(),
            path: native_path("/agent/skills"),
            group_id: None,
            selectable: true,
            mode_constraint: AgentSelectionModeConstraint::UserSelectable,
            disabled_reason: None,
        };
        AgentSelectionCatalog::from_parts_for_test(
            context(),
            AgentSelectionSnapshot {
                agents: vec![AgentSelectionAgent {
                    kind: AgentSelectionAgentKind::Standard,
                    id: agent_id.clone(),
                    display_name: "Private Agent".to_string(),
                    detection: DetectionState::Detected,
                    directory_access: Some(SkillDirectoryAccess::PrivateOnly),
                    install_option_id: Some(option_id.clone()),
                    group_id: None,
                }],
                install_options: vec![public.clone()],
                groups: Vec::new(),
                initial_selected_option_ids: Vec::new(),
                unavailable_explicit_agents: Vec::new(),
                user_mode_option_ids: Vec::new(),
                revision: AgentSelectionRevision("revision".to_string()),
            },
            BTreeMap::from([(
                option_id.clone(),
                ResolvedAgentInstallOption {
                    public,
                    adapter_target_ids: Vec::new(),
                    placement: ResolvedDirectoryPlacement {
                        id: DirectoryPlacementId::Option(option_id),
                        root: locator("/agent/skills"),
                        physical_key: key("agent-root"),
                        storage_access: StorageAccess::Native,
                        content:
                            crate::application::agent_selection::DirectoryContentKind::Original,
                    },
                },
            )]),
            ResolvedDirectoryPlacement {
                id: DirectoryPlacementId::Standard,
                root: locator("/scope"),
                physical_key: key("root"),
                storage_access: StorageAccess::Native,
                content: crate::application::agent_selection::DirectoryContentKind::Original,
            },
        )
    }

    fn catalog_with_shared_group_options() -> AgentSelectionCatalog {
        let agent_id = AgentId::parse("group-agent").unwrap();
        let first_id = AgentInstallOptionId("group-first".to_string());
        let second_id = AgentInstallOptionId("group-second".to_string());
        let option = |id: AgentInstallOptionId, path: &str| AgentInstallOption {
            id,
            kind: AgentInstallOptionKind::GroupLocation,
            agent_ids: vec![agent_id.clone()],
            display_name: "Group Agent".to_string(),
            path: native_path(path),
            group_id: Some("group-agent".to_string()),
            selectable: true,
            mode_constraint: AgentSelectionModeConstraint::CopyOnly,
            disabled_reason: None,
        };
        let first = option(first_id.clone(), "/group/first/skills");
        let second = option(second_id.clone(), "/group/second/skills");
        let resolved_option =
            |public: AgentInstallOption,
             id: AgentInstallOptionId,
             root: &str,
             logical_target: &str| ResolvedAgentInstallOption {
                public,
                adapter_target_ids: vec![logical_target.to_string()],
                placement: ResolvedDirectoryPlacement {
                    id: DirectoryPlacementId::Option(id),
                    root: locator(root),
                    physical_key: key("shared-group-root"),
                    storage_access: StorageAccess::Native,
                    content: crate::application::agent_selection::DirectoryContentKind::Original,
                },
            };
        AgentSelectionCatalog::from_parts_for_test(
            context(),
            AgentSelectionSnapshot {
                agents: vec![AgentSelectionAgent {
                    kind: AgentSelectionAgentKind::Grouped,
                    id: agent_id.clone(),
                    display_name: "Group Agent".to_string(),
                    detection: DetectionState::Detected,
                    directory_access: None,
                    install_option_id: Some(first_id.clone()),
                    group_id: Some("group-agent".to_string()),
                }],
                install_options: vec![first.clone(), second.clone()],
                groups: Vec::new(),
                initial_selected_option_ids: Vec::new(),
                unavailable_explicit_agents: Vec::new(),
                user_mode_option_ids: Vec::new(),
                revision: AgentSelectionRevision("revision".to_string()),
            },
            BTreeMap::from([
                (
                    first_id.clone(),
                    resolved_option(first, first_id, "/group/first/skills", "group:first"),
                ),
                (
                    second_id.clone(),
                    resolved_option(second, second_id, "/group/second/skills", "group:second"),
                ),
            ]),
            ResolvedDirectoryPlacement {
                id: DirectoryPlacementId::Standard,
                root: locator("/scope"),
                physical_key: key("root"),
                storage_access: StorageAccess::Native,
                content: crate::application::agent_selection::DirectoryContentKind::Original,
            },
        )
    }

    #[test]
    fn planner_rejects_catalog_placement_missing_from_observed_facts() {
        let catalog = catalog_with_option();
        let error = ScopeSkillPlanner::plan_direct_change(DirectSkillChangeRequest {
            skill: SkillDirectoryName::try_from("demo").unwrap(),
            catalog: &catalog,
            placements: ScopeSkillPlacementSet::new(
                context(),
                BTreeMap::from([(
                    DirectoryPlacementId::Standard,
                    fact("demo", TargetEntryKind::Missing, None),
                )]),
            ),
            libraries: LibraryElectionState {
                candidates: &LibraryCandidateSet::empty(),
                selected_agent_ids: &[],
            },
            direct_changes: BTreeMap::new(),
        })
        .unwrap_err();

        assert_eq!(
            error,
            ScopeSkillPlanningError::CatalogPlacementMismatch(DirectoryPlacementId::Option(
                AgentInstallOptionId("private".to_string())
            ))
        );
        assert!(matches!(
            error.into_app_error(),
            AppError::ConfigurationCorrupted { .. }
        ));
    }

    #[test]
    fn physical_conflict_reports_readers_from_all_paths_of_shared_directory() {
        let catalog = catalog_with_option();
        let option_id = AgentInstallOptionId("private".to_string());
        let standard = fact("shared-demo", TargetEntryKind::File, None);
        let mut option = standard.clone();
        option.destination = locator("/agent/skills/shared-demo");
        let error = ScopeSkillPlanner::plan_direct_change(DirectSkillChangeRequest {
            skill: SkillDirectoryName::try_from("demo").unwrap(),
            catalog: &catalog,
            placements: ScopeSkillPlacementSet::new(
                context(),
                BTreeMap::from([
                    (DirectoryPlacementId::Standard, standard),
                    (DirectoryPlacementId::Option(option_id), option),
                ]),
            ),
            libraries: LibraryElectionState {
                candidates: &LibraryCandidateSet::empty(),
                selected_agent_ids: &[],
            },
            direct_changes: BTreeMap::from([(
                DirectoryPlacementId::Standard,
                DirectPlacementChange::Set(PreparedDirectVersion::new(
                    DirectContentIdentity::Existing(key("replacement")),
                    PreparedEntryAction::Link {
                        target: locator("/content/demo"),
                    },
                )),
            )]),
        })
        .unwrap_err();

        assert!(matches!(
            error,
            ScopeSkillPlanningError::ExternalTarget { agent_ids, .. }
                if agent_ids == vec![AgentId::parse("private-agent").unwrap()]
        ));
    }

    #[test]
    fn plan_projects_observed_entries_from_the_same_reader_facts_used_for_execution() {
        let catalog = catalog_with_option();
        let option_id = AgentInstallOptionId("private".to_string());
        let plan = ScopeSkillPlanner::plan_direct_change(DirectSkillChangeRequest {
            skill: SkillDirectoryName::try_from("demo").unwrap(),
            catalog: &catalog,
            placements: ScopeSkillPlacementSet::new(
                context(),
                BTreeMap::from([
                    (
                        DirectoryPlacementId::Standard,
                        fact("standard-demo", TargetEntryKind::Directory, None),
                    ),
                    (
                        DirectoryPlacementId::Option(option_id),
                        fact("private-demo", TargetEntryKind::Directory, None),
                    ),
                ]),
            ),
            libraries: LibraryElectionState {
                candidates: &LibraryCandidateSet::empty(),
                selected_agent_ids: &[],
            },
            direct_changes: BTreeMap::new(),
        })
        .unwrap();

        let entries = plan.project_observed_entries().unwrap();

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].public.readers.len(), 1);
        assert_eq!(
            entries[0].public.readers[0].agent_id.as_str(),
            "private-agent"
        );
        assert_eq!(
            entries[0].public.readers[0].logical_target_id,
            "agent:private-agent:private"
        );
        assert_eq!(
            plan.compile_entries().additional[0].reader_agent_ids,
            vec![AgentId::parse("private-agent").unwrap()]
        );
    }

    #[test]
    fn plan_preserves_multiple_logical_targets_for_one_agent_in_a_shared_directory() {
        let catalog = catalog_with_shared_group_options();
        let shared = fact("shared-group-demo", TargetEntryKind::Directory, None);
        let plan = ScopeSkillPlanner::plan_direct_change(DirectSkillChangeRequest {
            skill: SkillDirectoryName::try_from("demo").unwrap(),
            catalog: &catalog,
            placements: ScopeSkillPlacementSet::new(
                context(),
                BTreeMap::from([
                    (
                        DirectoryPlacementId::Standard,
                        fact("standard-demo", TargetEntryKind::Directory, None),
                    ),
                    (
                        DirectoryPlacementId::Option(AgentInstallOptionId(
                            "group-first".to_string(),
                        )),
                        shared.clone(),
                    ),
                    (
                        DirectoryPlacementId::Option(AgentInstallOptionId(
                            "group-second".to_string(),
                        )),
                        shared,
                    ),
                ]),
            ),
            libraries: LibraryElectionState {
                candidates: &LibraryCandidateSet::empty(),
                selected_agent_ids: &[],
            },
            direct_changes: BTreeMap::new(),
        })
        .unwrap();

        let entries = plan.project_observed_entries().unwrap();
        let logical_targets = entries[0]
            .public
            .readers
            .iter()
            .map(|reader| reader.logical_target_id.as_str())
            .collect::<Vec<_>>();

        assert_eq!(logical_targets, vec!["group:first", "group:second"]);
        assert_eq!(
            plan.compile_entries().additional[0].reader_agent_ids,
            vec![AgentId::parse("group-agent").unwrap()]
        );
    }

    #[test]
    fn direct_change_uses_prepared_content_over_preserved_existing_content() {
        let catalog = catalog();
        let standard = fact("demo", TargetEntryKind::Directory, None);
        let replacement = PreparedEntryAction::Link {
            target: locator("/content/new-demo"),
        };
        let plan = ScopeSkillPlanner::plan_direct_change(DirectSkillChangeRequest {
            skill: SkillDirectoryName::try_from("demo").unwrap(),
            catalog: &catalog,
            placements: ScopeSkillPlacementSet::new(
                context(),
                BTreeMap::from([(DirectoryPlacementId::Standard, standard)]),
            ),
            libraries: LibraryElectionState {
                candidates: &LibraryCandidateSet::empty(),
                selected_agent_ids: &[],
            },
            direct_changes: BTreeMap::from([(
                DirectoryPlacementId::Standard,
                DirectPlacementChange::Set(PreparedDirectVersion::new(
                    DirectContentIdentity::Existing(key("new-content")),
                    replacement.clone(),
                )),
            )]),
        })
        .unwrap();

        assert_eq!(plan.directories()[0].action(), &replacement);
    }

    #[test]
    fn library_link_without_previous_application_is_not_removed() {
        let catalog = catalog();
        let candidate = LibraryVersionCandidate::new(
            LibraryId::parse("library-one"),
            "demo",
            locator("/libraries/library-one/skills/demo"),
        );
        let candidates = LibraryCandidateSet::for_skill(
            &EnvironmentRef::Native,
            &SkillDirectoryName::try_from("demo").unwrap(),
            vec![candidate],
            Vec::new(),
        )
        .unwrap();
        let plan = ScopeSkillPlanner::plan_direct_change(DirectSkillChangeRequest {
            skill: SkillDirectoryName::try_from("demo").unwrap(),
            catalog: &catalog,
            placements: ScopeSkillPlacementSet::new(
                context(),
                BTreeMap::from([(
                    DirectoryPlacementId::Standard,
                    fact(
                        "demo",
                        TargetEntryKind::Symlink,
                        Some("/libraries/library-one/skills/demo"),
                    ),
                )]),
            ),
            libraries: LibraryElectionState {
                candidates: &candidates,
                selected_agent_ids: &[],
            },
            direct_changes: BTreeMap::new(),
        })
        .unwrap();

        assert_eq!(plan.directories()[0].action(), &PreparedEntryAction::Keep);
    }

    #[test]
    fn library_change_does_not_remove_link_only_recognized_by_target_state() {
        let catalog = catalog();
        let skill = SkillDirectoryName::try_from("demo").unwrap();
        let before_candidate = LibraryVersionCandidate::new(
            LibraryId::parse("library-before"),
            "demo",
            locator("/libraries/library-before/skills/demo"),
        );
        let target_candidate = LibraryVersionCandidate::new(
            LibraryId::parse("library-target"),
            "demo",
            locator("/libraries/library-target/skills/demo"),
        );
        let before_candidates = LibraryCandidateSet::for_skill(
            &EnvironmentRef::Native,
            &skill,
            vec![before_candidate.clone()],
            vec![before_candidate],
        )
        .unwrap();
        let target_candidates = LibraryCandidateSet::for_skill(
            &EnvironmentRef::Native,
            &skill,
            vec![target_candidate.clone()],
            vec![target_candidate],
        )
        .unwrap();

        let plan = ScopeSkillPlanner::plan_library_change(LibrarySkillChangeRequest {
            skill,
            catalog: &catalog,
            placements: ScopeSkillPlacementSet::new(
                context(),
                BTreeMap::from([(
                    DirectoryPlacementId::Standard,
                    fact("demo", TargetEntryKind::Missing, None),
                )]),
            ),
            before: LibraryElectionState {
                candidates: &before_candidates,
                selected_agent_ids: &[],
            },
            after: LibraryElectionState {
                candidates: &target_candidates,
                selected_agent_ids: &[],
            },
            legacy: vec![LegacyLibraryPlacement {
                fact: fact(
                    "legacy-demo",
                    TargetEntryKind::Symlink,
                    Some("/libraries/library-target/skills/demo"),
                ),
                reader_agent_ids: vec![AgentId::parse("legacy-agent").unwrap()],
            }],
        })
        .unwrap();

        let legacy = plan
            .directories()
            .iter()
            .find(|directory| directory.placements() == [DirectoryPlacementRef::Legacy])
            .expect("legacy directory remains in the plan");
        assert_eq!(legacy.action(), &PreparedEntryAction::Keep);
    }
}

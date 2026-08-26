//! Scope 内同名 Skill 的目录版本选举。
//!
//! 调用方提交已解析的目录位置、当前和目标版本需求，以及按库顺序排列的候选。
//! 该 Module 只按物理目录完成版本选举，并将结果投影为目录动作与执行观察。

use std::collections::{BTreeMap, BTreeSet};

use crate::application::agent_selection::DirectoryPlacementId;
use crate::application::installed_skill_resolver::SkillDirectoryName;
use crate::application::library_candidates::{LibraryCandidateSet, LibraryVersionCandidate};
use crate::application::mutation::plan::PreparedEntryAction;
use crate::environment::planning::ResolvedTargetFact;
use crate::environment::runtime::PhysicalTargetKey;
use crate::environment::types::ResourceLocator;
use crate::error::SkillPlacementTargetKind;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum DirectoryPlacementRef {
    Catalog(DirectoryPlacementId),
    Legacy,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DirectVersionCandidate {
    Existing { source: Option<ResourceLocator> },
    Prepared { action: PreparedEntryAction },
}

impl DirectVersionCandidate {
    pub(crate) fn existing(source: Option<ResourceLocator>) -> Self {
        Self::Existing { source }
    }

    pub(crate) fn prepared(action: PreparedEntryAction) -> Self {
        Self::Prepared { action }
    }

    fn source(&self) -> Option<&ResourceLocator> {
        match self {
            Self::Existing { source } => source.as_ref(),
            Self::Prepared { .. } => None,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct PlacementVersionDemand {
    direct: Option<DirectVersionCandidate>,
    library: bool,
}

impl PlacementVersionDemand {
    #[cfg(test)]
    pub(crate) fn empty() -> Self {
        Self::default()
    }

    pub(crate) fn direct(direct: DirectVersionCandidate) -> Self {
        Self {
            direct: Some(direct),
            library: false,
        }
    }

    pub(crate) fn library() -> Self {
        Self {
            direct: None,
            library: true,
        }
    }

    pub(crate) fn with_library(mut self) -> Self {
        self.library = true;
        self
    }

    fn is_empty(&self) -> bool {
        self.direct.is_none() && !self.library
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SkillDirectoryPlacementInput {
    placement: DirectoryPlacementRef,
    fact: ResolvedTargetFact,
    current: PlacementVersionDemand,
    target: PlacementVersionDemand,
}

impl SkillDirectoryPlacementInput {
    pub(crate) fn new(
        placement: DirectoryPlacementRef,
        fact: ResolvedTargetFact,
        current: PlacementVersionDemand,
        target: PlacementVersionDemand,
    ) -> Result<Self, SkillDirectoryInputError> {
        if placement == DirectoryPlacementRef::Legacy && !target.is_empty() {
            return Err(SkillDirectoryInputError::LegacyTargetDemand);
        }
        Ok(Self {
            placement,
            fact,
            current,
            target,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SkillDirectoryInputError {
    MissingPlacement,
    MissingStandardPlacement,
    DuplicateStandardPlacement,
    DuplicateActivePlacement,
    LegacyTargetDemand,
    InconsistentPhysicalFact,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PhysicalSkillDirectoryRequest {
    skill: SkillDirectoryName,
    placements: Vec<SkillDirectoryPlacementInput>,
    libraries: LibraryCandidateSet,
    before_library_candidates: Vec<LibraryVersionCandidate>,
}

impl PhysicalSkillDirectoryRequest {
    pub(crate) fn new(
        skill: SkillDirectoryName,
        placements: Vec<SkillDirectoryPlacementInput>,
        libraries: LibraryCandidateSet,
        before_library_candidates: Vec<LibraryVersionCandidate>,
    ) -> Result<Self, SkillDirectoryInputError> {
        if placements.is_empty() {
            return Err(SkillDirectoryInputError::MissingPlacement);
        }
        let standard_count = placements
            .iter()
            .filter(|input| {
                input.placement == DirectoryPlacementRef::Catalog(DirectoryPlacementId::Standard)
            })
            .count();
        match standard_count {
            0 => return Err(SkillDirectoryInputError::MissingStandardPlacement),
            1 => {}
            _ => return Err(SkillDirectoryInputError::DuplicateStandardPlacement),
        }
        let mut active = BTreeSet::new();
        for input in &placements {
            if let DirectoryPlacementRef::Catalog(id) = &input.placement {
                if !active.insert(id.clone()) {
                    return Err(SkillDirectoryInputError::DuplicateActivePlacement);
                }
            }
        }
        let mut facts = BTreeMap::<PhysicalTargetKey, &ResolvedTargetFact>::new();
        for input in &placements {
            if let Some(existing) = facts.get(&input.fact.key) {
                if existing.fingerprint != input.fact.fingerprint
                    || existing.entry_kind != input.fact.entry_kind
                    || existing.link_target_identity != input.fact.link_target_identity
                    || existing.storage_access != input.fact.storage_access
                {
                    return Err(SkillDirectoryInputError::InconsistentPhysicalFact);
                }
            } else {
                facts.insert(input.fact.key.clone(), &input.fact);
            }
        }
        Ok(Self {
            skill,
            placements,
            libraries,
            before_library_candidates,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ObservedVersion {
    Direct,
    Library(LibraryVersionCandidate),
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ElectedVersion {
    Direct(DirectVersionCandidate),
    Library(LibraryVersionCandidate),
    Absent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DirectoryUpdate {
    Unchanged,
    UseDirect,
    UseLibrary,
    Remove,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PlannedPhysicalDirectory {
    fact: ResolvedTargetFact,
    placements: Vec<DirectoryPlacementRef>,
    observed: ObservedVersion,
    elected: ElectedVersion,
    removable_by_current: bool,
}

impl PlannedPhysicalDirectory {
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

    pub(crate) fn action(&self) -> PreparedEntryAction {
        use crate::environment::planning::TargetEntryKind;

        match &self.elected {
            ElectedVersion::Direct(DirectVersionCandidate::Prepared { action }) => action.clone(),
            ElectedVersion::Direct(DirectVersionCandidate::Existing { source })
                if self.fact.entry_kind == TargetEntryKind::BrokenLink
                    && self.observed == ObservedVersion::Direct
                    && source.is_some() =>
            {
                PreparedEntryAction::Link {
                    target: source.clone().expect("guarded direct source"),
                }
            }
            ElectedVersion::Direct(_) => PreparedEntryAction::Keep,
            ElectedVersion::Library(candidate)
                if self.fact.entry_kind != TargetEntryKind::BrokenLink
                    && self.observed == ObservedVersion::Library(candidate.clone()) =>
            {
                PreparedEntryAction::Keep
            }
            ElectedVersion::Library(candidate) => PreparedEntryAction::Link {
                target: candidate.locator().clone(),
            },
            ElectedVersion::Absent if self.removable_by_current => PreparedEntryAction::Remove,
            ElectedVersion::Absent => PreparedEntryAction::Keep,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PhysicalSkillDirectoryPlan {
    directories: Vec<PlannedPhysicalDirectory>,
}

impl PhysicalSkillDirectoryPlan {
    pub(crate) fn directories(&self) -> &[PlannedPhysicalDirectory] {
        &self.directories
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PhysicalSkillDirectoryConflict {
    ExternalTarget {
        key: Box<PhysicalTargetKey>,
        skill_name: String,
        target_path: String,
        target_kind: SkillPlacementTargetKind,
    },
    ConflictingDirectVersions {
        skill_name: String,
        target_path: String,
    },
}

pub(crate) fn plan_physical_skill_directories(
    request: PhysicalSkillDirectoryRequest,
) -> Result<PhysicalSkillDirectoryPlan, PhysicalSkillDirectoryConflict> {
    let mut grouped = BTreeMap::<PhysicalTargetKey, Vec<SkillDirectoryPlacementInput>>::new();
    for input in request.placements {
        grouped
            .entry(input.fact.key.clone())
            .or_default()
            .push(input);
    }
    let mut directories = Vec::with_capacity(grouped.len());
    for (_, group) in grouped {
        let representative = group
            .iter()
            .find(|input| {
                input.placement == DirectoryPlacementRef::Catalog(DirectoryPlacementId::Standard)
            })
            .unwrap_or(&group[0]);
        let mut placements = group
            .iter()
            .map(|input| input.placement.clone())
            .collect::<Vec<_>>();
        placements.sort();
        placements.dedup();
        let mut direct = group
            .iter()
            .filter_map(|input| input.target.direct.clone())
            .collect::<Vec<_>>();
        if direct
            .iter()
            .any(|candidate| matches!(candidate, DirectVersionCandidate::Prepared { .. }))
        {
            direct.retain(|candidate| matches!(candidate, DirectVersionCandidate::Prepared { .. }));
        }
        direct.dedup();
        if direct.len() > 1 {
            return Err(PhysicalSkillDirectoryConflict::ConflictingDirectVersions {
                skill_name: request.skill.as_ref().to_string(),
                target_path: representative.fact.destination.native_path.clone(),
            });
        }
        let elected = if let Some(direct) = direct.pop() {
            ElectedVersion::Direct(direct)
        } else if group.iter().any(|input| input.target.library) {
            request
                .libraries
                .ordered()
                .first()
                .cloned()
                .map(ElectedVersion::Library)
                .unwrap_or(ElectedVersion::Absent)
        } else {
            ElectedVersion::Absent
        };
        if !matches!(elected, ElectedVersion::Absent)
            && matches!(
                representative.fact.entry_kind,
                crate::environment::planning::TargetEntryKind::File
                    | crate::environment::planning::TargetEntryKind::Other
            )
        {
            return Err(PhysicalSkillDirectoryConflict::ExternalTarget {
                key: Box::new(representative.fact.key.clone()),
                skill_name: request.skill.as_ref().to_string(),
                target_path: representative.fact.destination.native_path.clone(),
                target_kind: conflict_target_kind(representative.fact.entry_kind),
            });
        }
        let observed = observe_physical_directory(&group, &request.libraries);
        let removable_by_current = group.iter().any(|input| match &observed {
            ObservedVersion::Direct => input.current.direct.is_some(),
            ObservedVersion::Library(candidate) => {
                input.current.library && request.before_library_candidates.contains(candidate)
            }
            ObservedVersion::Unknown => false,
        });
        directories.push(PlannedPhysicalDirectory {
            fact: representative.fact.clone(),
            placements,
            observed,
            elected,
            removable_by_current,
        });
    }
    Ok(PhysicalSkillDirectoryPlan { directories })
}

fn observe_physical_directory(
    group: &[SkillDirectoryPlacementInput],
    libraries: &LibraryCandidateSet,
) -> ObservedVersion {
    use crate::environment::planning::TargetEntryKind;

    let fact = &group[0].fact;
    match fact.entry_kind {
        TargetEntryKind::Directory => ObservedVersion::Direct,
        TargetEntryKind::Symlink | TargetEntryKind::Junction | TargetEntryKind::BrokenLink => {
            if let Some(identity) = fact.link_target_identity.as_ref() {
                if let Some(candidate) = libraries
                    .recognized()
                    .iter()
                    .find(|candidate| identity.matches(candidate.locator()))
                {
                    return ObservedVersion::Library(candidate.clone());
                }
                if group
                    .iter()
                    .flat_map(|input| {
                        input
                            .current
                            .direct
                            .iter()
                            .chain(input.target.direct.iter())
                    })
                    .filter_map(DirectVersionCandidate::source)
                    .any(|source| identity.matches(source))
                {
                    return ObservedVersion::Direct;
                }
            }
            if fact.entry_kind == TargetEntryKind::BrokenLink {
                ObservedVersion::Unknown
            } else {
                ObservedVersion::Direct
            }
        }
        TargetEntryKind::Missing | TargetEntryKind::File | TargetEntryKind::Other => {
            ObservedVersion::Unknown
        }
    }
}

fn conflict_target_kind(
    kind: crate::environment::planning::TargetEntryKind,
) -> SkillPlacementTargetKind {
    match kind {
        crate::environment::planning::TargetEntryKind::File => SkillPlacementTargetKind::File,
        crate::environment::planning::TargetEntryKind::Other => SkillPlacementTargetKind::Other,
        _ => unreachable!("only external entries produce placement conflicts"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::skill_libraries::LibraryId;
    use crate::environment::planning::TargetEntryKind;
    use crate::environment::runtime::{
        EntryFingerprint, ExecutionBackend, PhysicalParentIdentity, PhysicalTargetKey,
    };
    use crate::environment::types::{EnvironmentRef, StorageAccess};

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

    fn fact(
        name: &str,
        path: &str,
        kind: TargetEntryKind,
        link: Option<&str>,
    ) -> ResolvedTargetFact {
        let destination = locator(path);
        let link_target = link.map(native_path);
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

    #[test]
    fn placement_request_rejects_missing_duplicate_standard_and_legacy_target_demand() {
        let skill = SkillDirectoryName::try_from("demo").unwrap();
        assert_eq!(
            PhysicalSkillDirectoryRequest::new(
                skill.clone(),
                Vec::new(),
                LibraryCandidateSet::empty(),
                Vec::new(),
            ),
            Err(SkillDirectoryInputError::MissingPlacement)
        );
        let standard = SkillDirectoryPlacementInput::new(
            DirectoryPlacementRef::Catalog(DirectoryPlacementId::Standard),
            fact(
                "canonical",
                "/scope/.agents/skills/demo",
                TargetEntryKind::Directory,
                None,
            ),
            PlacementVersionDemand::empty(),
            PlacementVersionDemand::empty(),
        )
        .unwrap();
        assert_eq!(
            PhysicalSkillDirectoryRequest::new(
                skill,
                vec![standard.clone(), standard],
                LibraryCandidateSet::empty(),
                Vec::new(),
            ),
            Err(SkillDirectoryInputError::DuplicateStandardPlacement)
        );
        assert_eq!(
            SkillDirectoryPlacementInput::new(
                DirectoryPlacementRef::Legacy,
                fact(
                    "legacy",
                    "/legacy/skills/demo",
                    TargetEntryKind::Directory,
                    None
                ),
                PlacementVersionDemand::direct(DirectVersionCandidate::existing(None)),
                PlacementVersionDemand::library(),
            ),
            Err(SkillDirectoryInputError::LegacyTargetDemand)
        );
    }

    #[test]
    fn pure_planner_merges_shared_placements_and_projects_keep_observation() {
        let shared = fact(
            "shared",
            "/scope/.agents/skills/demo",
            TargetEntryKind::Directory,
            None,
        );
        let direct = DirectVersionCandidate::existing(Some(locator("/scope/.agents/skills/demo")));
        let standard = SkillDirectoryPlacementInput::new(
            DirectoryPlacementRef::Catalog(DirectoryPlacementId::Standard),
            shared.clone(),
            PlacementVersionDemand::direct(direct.clone()),
            PlacementVersionDemand::direct(direct),
        )
        .unwrap();
        let option = SkillDirectoryPlacementInput::new(
            DirectoryPlacementRef::Catalog(DirectoryPlacementId::Option(
                crate::application::agent_selection::AgentInstallOptionId(
                    "shared-option".to_string(),
                ),
            )),
            shared,
            PlacementVersionDemand::empty(),
            PlacementVersionDemand::library(),
        )
        .unwrap();
        let request = PhysicalSkillDirectoryRequest::new(
            SkillDirectoryName::try_from("demo").unwrap(),
            vec![standard, option],
            LibraryCandidateSet::empty(),
            Vec::new(),
        )
        .unwrap();
        let plan = plan_physical_skill_directories(request).unwrap();
        assert_eq!(plan.directories().len(), 1);
        let directory = &plan.directories()[0];
        assert_eq!(directory.placements().len(), 2);
        assert!(matches!(directory.elected(), ElectedVersion::Direct(_)));
        assert_eq!(directory.action(), PreparedEntryAction::Keep);
    }

    #[test]
    fn shared_physical_fact_accepts_equivalent_link_text_with_the_same_identity() {
        let candidate = LibraryVersionCandidate::new(
            LibraryId::parse("library-one"),
            "demo",
            locator("/libraries/library-one/skills/demo"),
        );
        let candidates =
            LibraryCandidateSet::new(vec![candidate.clone()], vec![candidate.clone()]).unwrap();
        let standard_fact = fact(
            "shared-library",
            "/scope/.agents/skills/demo",
            TargetEntryKind::Symlink,
            Some("/libraries/library-one/skills/demo"),
        );
        let option_fact = fact(
            "shared-library",
            "/agent/skills/demo",
            TargetEntryKind::Symlink,
            Some("/libraries/library-one/skills/./demo"),
        );
        let standard = SkillDirectoryPlacementInput::new(
            DirectoryPlacementRef::Catalog(DirectoryPlacementId::Standard),
            standard_fact,
            PlacementVersionDemand::library(),
            PlacementVersionDemand::library(),
        )
        .unwrap();
        let option = SkillDirectoryPlacementInput::new(
            DirectoryPlacementRef::Catalog(DirectoryPlacementId::Option(
                crate::application::agent_selection::AgentInstallOptionId(
                    "shared-option".to_string(),
                ),
            )),
            option_fact,
            PlacementVersionDemand::library(),
            PlacementVersionDemand::library(),
        )
        .unwrap();

        let request = PhysicalSkillDirectoryRequest::new(
            SkillDirectoryName::try_from("demo").unwrap(),
            vec![standard, option],
            candidates,
            vec![candidate],
        )
        .unwrap();
        let plan = plan_physical_skill_directories(request).unwrap();

        assert_eq!(plan.directories().len(), 1);
        assert_eq!(plan.directories()[0].action(), PreparedEntryAction::Keep);
    }
}

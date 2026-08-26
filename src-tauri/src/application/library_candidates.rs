use std::collections::{BTreeMap, BTreeSet};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use crate::application::installed_skill_resolver::SkillDirectoryName;
use crate::application::library_application::LibraryApplicationRepository;
use crate::application::skill_libraries::{LibraryCatalog, LibraryId};
use crate::core::agent_definition::AgentId;
use crate::environment::types::{
    same_environment_identity, EnvironmentRef, ResourceLocator, SkillLocationRef,
};
use crate::error::AppError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LibraryVersionCandidate {
    library_id: LibraryId,
    member_name: String,
    locator: ResourceLocator,
}

impl LibraryVersionCandidate {
    pub(crate) fn new(
        library_id: LibraryId,
        member_name: impl Into<String>,
        locator: ResourceLocator,
    ) -> Self {
        Self {
            library_id,
            member_name: member_name.into(),
            locator,
        }
    }

    pub(crate) fn library_id(&self) -> &LibraryId {
        &self.library_id
    }

    pub(crate) fn member_name(&self) -> &str {
        &self.member_name
    }

    pub(crate) fn locator(&self) -> &ResourceLocator {
        &self.locator
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum LibraryCandidateError {
    CandidateSkillMismatch {
        member_name: String,
    },
    CandidateEnvironmentMismatch {
        member_name: String,
    },
    DuplicateLibraryCandidate {
        library_id: LibraryId,
    },
    InvalidCatalogMemberName {
        library_id: LibraryId,
        member_name: String,
    },
    DuplicateCatalogMemberDirectory {
        library_id: LibraryId,
        first_member_name: String,
        duplicate_member_name: String,
    },
    UnknownLibrary {
        library_id: LibraryId,
    },
    DuplicateOrderedCandidate,
    OrderedCandidateNotRecognized,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LibraryCandidateSet {
    recognized: Vec<LibraryVersionCandidate>,
    ordered: Vec<LibraryVersionCandidate>,
}

impl LibraryCandidateSet {
    pub(crate) fn for_skill(
        environment: &EnvironmentRef,
        skill: &SkillDirectoryName,
        recognized: Vec<LibraryVersionCandidate>,
        ordered: Vec<LibraryVersionCandidate>,
    ) -> Result<Self, LibraryCandidateError> {
        let mut candidate_libraries = BTreeSet::new();
        for candidate in &recognized {
            if SkillDirectoryName::try_from(candidate.member_name.as_str())
                .ok()
                .as_ref()
                != Some(skill)
            {
                return Err(LibraryCandidateError::CandidateSkillMismatch {
                    member_name: candidate.member_name.clone(),
                });
            }
            if !same_environment_identity(environment, &candidate.locator.environment) {
                return Err(LibraryCandidateError::CandidateEnvironmentMismatch {
                    member_name: candidate.member_name.clone(),
                });
            }
            if !candidate_libraries.insert(candidate.library_id.clone()) {
                return Err(LibraryCandidateError::DuplicateLibraryCandidate {
                    library_id: candidate.library_id.clone(),
                });
            }
        }
        Self::new(recognized, ordered)
    }

    pub(crate) fn new(
        recognized: Vec<LibraryVersionCandidate>,
        ordered: Vec<LibraryVersionCandidate>,
    ) -> Result<Self, LibraryCandidateError> {
        let mut seen = Vec::new();
        for candidate in &ordered {
            if seen.contains(candidate) {
                return Err(LibraryCandidateError::DuplicateOrderedCandidate);
            }
            seen.push(candidate.clone());
            if !recognized.contains(candidate) {
                return Err(LibraryCandidateError::OrderedCandidateNotRecognized);
            }
        }
        Ok(Self {
            recognized,
            ordered,
        })
    }

    #[cfg(test)]
    pub(crate) fn empty() -> Self {
        Self {
            recognized: Vec::new(),
            ordered: Vec::new(),
        }
    }

    pub(crate) fn recognized(&self) -> &[LibraryVersionCandidate] {
        &self.recognized
    }

    pub(crate) fn ordered(&self) -> &[LibraryVersionCandidate] {
        &self.ordered
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum LibraryCandidateSnapshotError {
    EmptyEvidenceDigest,
    DuplicateSelectedAgent,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LibraryCandidateSnapshot {
    evidence_digest: String,
    selected_agent_ids: Vec<AgentId>,
    candidates: LibraryCandidateSet,
}

impl LibraryCandidateSnapshot {
    pub(crate) fn new(
        evidence_digest: impl Into<String>,
        mut selected_agent_ids: Vec<AgentId>,
        candidates: LibraryCandidateSet,
    ) -> Result<Self, LibraryCandidateSnapshotError> {
        let evidence_digest = evidence_digest.into();
        if evidence_digest.trim().is_empty() {
            return Err(LibraryCandidateSnapshotError::EmptyEvidenceDigest);
        }
        selected_agent_ids.sort();
        if selected_agent_ids.windows(2).any(|pair| pair[0] == pair[1]) {
            return Err(LibraryCandidateSnapshotError::DuplicateSelectedAgent);
        }
        Ok(Self {
            evidence_digest,
            selected_agent_ids,
            candidates,
        })
    }

    pub(crate) fn evidence_digest(&self) -> &str {
        &self.evidence_digest
    }

    pub(crate) fn selected_agent_ids(&self) -> &[AgentId] {
        &self.selected_agent_ids
    }

    pub(crate) fn candidates(&self) -> &LibraryCandidateSet {
        &self.candidates
    }
}

pub(crate) type LibraryCandidateFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

pub(crate) trait LibraryCandidateSource: Send + Sync {
    fn load_candidates<'a>(
        &'a self,
        context: &'a SkillLocationRef,
        skill: &'a SkillDirectoryName,
    ) -> LibraryCandidateFuture<'a, Result<LibraryCandidateSnapshot, AppError>>;
}

#[cfg(test)]
pub(crate) struct EmptyLibraryCandidateSource;

#[cfg(test)]
impl LibraryCandidateSource for EmptyLibraryCandidateSource {
    fn load_candidates<'a>(
        &'a self,
        _context: &'a SkillLocationRef,
        _skill: &'a SkillDirectoryName,
    ) -> LibraryCandidateFuture<'a, Result<LibraryCandidateSnapshot, AppError>> {
        Box::pin(async {
            LibraryCandidateSnapshot::new(
                "digest-v1-empty-library-candidates",
                Vec::new(),
                LibraryCandidateSet::empty(),
            )
            .map_err(candidate_configuration_error)
        })
    }
}


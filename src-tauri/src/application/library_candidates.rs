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


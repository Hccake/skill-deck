use std::collections::{BTreeMap, BTreeSet};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use crate::application::installed_skill_resolver::SkillDirectoryName;
use crate::application::library_application::LibraryApplicationRepository;
use crate::application::skill_libraries::{LibraryCatalog, LibraryId};
use crate::core::agent_definition::AgentId;
use crate::environment::planning::{ResolvedTargetFact, TargetFactResolver};
use crate::environment::types::{
    same_environment_identity, EnvironmentRef, ResourceLocator, SkillLocationRef,
};
use crate::error::AppError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LibraryVersionCandidate {
    library_id: LibraryId,
    member_name: String,
    physical_locator: ResourceLocator,
}

impl LibraryVersionCandidate {
    #[cfg(test)]
    pub(crate) fn new(
        library_id: LibraryId,
        member_name: impl Into<String>,
        locator: ResourceLocator,
    ) -> Self {
        Self {
            library_id,
            member_name: member_name.into(),
            physical_locator: locator,
        }
    }

    fn from_resolved_fact(member: &LibraryCatalogMember, fact: ResolvedTargetFact) -> Self {
        Self {
            library_id: member.library_id.clone(),
            member_name: member.member_name.clone(),
            physical_locator: fact.destination,
        }
    }

    pub(crate) fn library_id(&self) -> &LibraryId {
        &self.library_id
    }

    pub(crate) fn member_name(&self) -> &str {
        &self.member_name
    }

    pub(crate) fn locator(&self) -> &ResourceLocator {
        &self.physical_locator
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

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct LibraryCatalogMember {
    pub(crate) library_id: LibraryId,
    pub(crate) member_name: String,
}

#[derive(Debug, Clone)]
pub(crate) struct LibraryCatalogMemberIndex {
    library_ids: BTreeSet<LibraryId>,
    by_directory_name: BTreeMap<SkillDirectoryName, Vec<LibraryCatalogMember>>,
}

impl LibraryCatalogMemberIndex {
    pub(crate) fn build(catalog: &LibraryCatalog) -> Result<Self, LibraryCandidateError> {
        let mut library_ids = BTreeSet::new();
        let mut by_directory_name =
            BTreeMap::<SkillDirectoryName, Vec<LibraryCatalogMember>>::new();
        for library in &catalog.libraries {
            library_ids.insert(library.id.clone());
            let mut members_in_library = BTreeMap::<SkillDirectoryName, String>::new();
            for member in &library.skills {
                let directory_name =
                    SkillDirectoryName::try_from(member.name.as_str()).map_err(|_| {
                        LibraryCandidateError::InvalidCatalogMemberName {
                            library_id: library.id.clone(),
                            member_name: member.name.clone(),
                        }
                    })?;
                if let Some(first_member_name) =
                    members_in_library.insert(directory_name.clone(), member.name.clone())
                {
                    return Err(LibraryCandidateError::DuplicateCatalogMemberDirectory {
                        library_id: library.id.clone(),
                        first_member_name,
                        duplicate_member_name: member.name.clone(),
                    });
                }
                by_directory_name
                    .entry(directory_name)
                    .or_default()
                    .push(LibraryCatalogMember {
                        library_id: library.id.clone(),
                        member_name: member.name.clone(),
                    });
            }
        }
        Ok(Self {
            library_ids,
            by_directory_name,
        })
    }

    pub(crate) fn members_for(
        &self,
        ordered_library_ids: &[LibraryId],
    ) -> Result<BTreeMap<SkillDirectoryName, Vec<LibraryCatalogMember>>, LibraryCandidateError>
    {
        let mut grouped = BTreeMap::<SkillDirectoryName, Vec<LibraryCatalogMember>>::new();
        for library_id in ordered_library_ids {
            if !self.library_ids.contains(library_id) {
                return Err(LibraryCandidateError::UnknownLibrary {
                    library_id: library_id.clone(),
                });
            }
            for (directory_name, members) in &self.by_directory_name {
                if let Some(member) = members
                    .iter()
                    .find(|member| &member.library_id == library_id)
                {
                    grouped
                        .entry(directory_name.clone())
                        .or_default()
                        .push(member.clone());
                }
            }
        }
        Ok(grouped)
    }
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
            if !same_environment_identity(environment, &candidate.locator().environment) {
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

    fn load_candidate_sets<'a>(
        &'a self,
        context: &'a SkillLocationRef,
        skills: &'a [SkillDirectoryName],
    ) -> LibraryCandidateFuture<'a, Result<Vec<LibraryCandidateSnapshot>, AppError>> {
        Box::pin(async move {
            let mut snapshots = Vec::with_capacity(skills.len());
            for skill in skills {
                snapshots.push(self.load_candidates(context, skill).await?);
            }
            Ok(snapshots)
        })
    }
}

pub(crate) struct RepositoryLibraryCandidateSource {
    repository: Arc<dyn LibraryApplicationRepository>,
    targets: Arc<dyn TargetFactResolver>,
}

impl RepositoryLibraryCandidateSource {
    pub(crate) fn new<T>(repository: Arc<dyn LibraryApplicationRepository>, targets: T) -> Self
    where
        T: TargetFactResolver + 'static,
    {
        Self {
            repository,
            targets: Arc::new(targets),
        }
    }
}

impl LibraryCandidateSource for RepositoryLibraryCandidateSource {
    fn load_candidates<'a>(
        &'a self,
        context: &'a SkillLocationRef,
        skill: &'a SkillDirectoryName,
    ) -> LibraryCandidateFuture<'a, Result<LibraryCandidateSnapshot, AppError>> {
        Box::pin(async move {
            self.load_candidate_sets(context, std::slice::from_ref(skill))
                .await?
                .pop()
                .ok_or(AppError::StaleTarget)
        })
    }

    fn load_candidate_sets<'a>(
        &'a self,
        context: &'a SkillLocationRef,
        skills: &'a [SkillDirectoryName],
    ) -> LibraryCandidateFuture<'a, Result<Vec<LibraryCandidateSnapshot>, AppError>> {
        Box::pin(async move {
            if skills.is_empty() {
                return Ok(Vec::new());
            }
            let record = self.repository.load_application(context).await?;
            if record.pending_operation.is_some() {
                return Err(AppError::MutationBusy);
            }
            let catalog = self.repository.load_catalog(context).await?;
            let evidence_digest =
                crate::application::mutation::plan::stable_digest(&(&record.current, &catalog))?;
            let index = LibraryCatalogMemberIndex::build(&catalog)
                .map_err(candidate_configuration_error)?;
            let grouped = index
                .members_for(&record.current.ordered_library_ids)
                .map_err(candidate_configuration_error)?;
            let member_groups = skills
                .iter()
                .map(|skill| grouped.get(skill).cloned().unwrap_or_default())
                .collect::<Vec<_>>();
            let members = member_groups.iter().flatten().cloned().collect::<Vec<_>>();
            let candidate_index = ResolvedLibraryCandidateIndex::load(
                self.repository.as_ref(),
                self.targets.as_ref(),
                context,
                &members,
            )
            .await?;
            skills
                .iter()
                .zip(member_groups)
                .map(|(skill, members)| {
                    let candidates = candidate_index.candidates_for(&members)?;
                    let candidates = LibraryCandidateSet::for_skill(
                        &context.environment,
                        skill,
                        candidates.clone(),
                        candidates,
                    )
                    .map_err(candidate_configuration_error)?;
                    LibraryCandidateSnapshot::new(
                        evidence_digest.clone(),
                        record.current.selected_agent_ids.clone(),
                        candidates,
                    )
                    .map_err(candidate_configuration_error)
                })
                .collect()
        })
    }
}

pub(crate) struct ResolvedLibraryCandidateIndex {
    by_member: BTreeMap<LibraryCatalogMember, LibraryVersionCandidate>,
}

impl ResolvedLibraryCandidateIndex {
    pub(crate) async fn load<T: TargetFactResolver + ?Sized>(
        repository: &dyn LibraryApplicationRepository,
        targets: &T,
        context: &SkillLocationRef,
        members: &[LibraryCatalogMember],
    ) -> Result<Self, AppError> {
        let members = members.iter().cloned().collect::<BTreeSet<_>>();
        let mut unresolved = Vec::with_capacity(members.len());
        for member in &members {
            unresolved.push(
                repository
                    .library_skill_locator(context, &member.library_id, &member.member_name)
                    .await?,
            );
        }
        let resolved = if unresolved.is_empty() {
            Vec::new()
        } else {
            targets.resolve(context, &unresolved, None).await?
        };
        if resolved.len() != members.len() {
            return Err(AppError::StaleTarget);
        }
        Ok(Self {
            by_member: members
                .into_iter()
                .zip(resolved)
                .map(|(member, fact)| {
                    let candidate = LibraryVersionCandidate::from_resolved_fact(&member, fact);
                    (member, candidate)
                })
                .collect(),
        })
    }

    pub(crate) fn candidates_for(
        &self,
        members: &[LibraryCatalogMember],
    ) -> Result<Vec<LibraryVersionCandidate>, AppError> {
        members
            .iter()
            .map(|member| {
                self.by_member
                    .get(member)
                    .cloned()
                    .ok_or(AppError::StaleTarget)
            })
            .collect()
    }
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

fn candidate_configuration_error(error: impl std::fmt::Debug) -> AppError {
    AppError::ConfigurationCorrupted {
        message: format!("invalid Skill Library candidate snapshot: {error:?}"),
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    use super::*;
    use crate::application::installed_skill_resolver::SkillDirectoryName;
    use crate::application::library_application::{
        LibraryApplicationFuture, LibraryApplicationRecord, LibraryApplicationRepository,
        LibraryApplicationState, PendingLibraryApplication,
    };
    use crate::application::skill_libraries::{
        LibraryCatalog, LibraryId, LibrarySkillRecord, SkillLibraryRecord, LIBRARY_SCHEMA_VERSION,
    };
    use crate::core::agent_definition::AgentId;
    use crate::environment::planning::{RuntimeTargetFactResolver, TargetFactFuture};
    use crate::environment::types::{
        EnvironmentRef, ResourceLocator, SkillLocation, SkillLocationRef,
    };
    use crate::environment::wsl::WslRuntime;

    fn targets() -> RuntimeTargetFactResolver {
        RuntimeTargetFactResolver::new(Arc::new(WslRuntime::default()))
    }

    #[derive(Clone)]
    struct CountingTargets {
        inner: RuntimeTargetFactResolver,
        calls: Arc<AtomicUsize>,
    }

    impl TargetFactResolver for CountingTargets {
        fn resolve<'a>(
            &'a self,
            context: &'a SkillLocationRef,
            logical_destinations: &'a [ResourceLocator],
            cancellation: Option<crate::core::mutation::CancellationSignal>,
        ) -> TargetFactFuture<'a, Result<Vec<ResolvedTargetFact>, AppError>> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.inner
                .resolve(context, logical_destinations, cancellation)
        }
    }

    #[test]
    fn candidate_set_rejects_another_skill_or_environment() {
        let skill = SkillDirectoryName::try_from("demo").unwrap();
        let native = EnvironmentRef::Native;
        let wrong_skill = LibraryVersionCandidate::new(
            LibraryId::parse("library-one"),
            "other",
            ResourceLocator {
                environment: native.clone(),
                native_path: "/libraries/library-one/skills/other".to_string(),
            },
        );
        assert!(matches!(
            LibraryCandidateSet::for_skill(&native, &skill, vec![wrong_skill], Vec::new(),),
            Err(LibraryCandidateError::CandidateSkillMismatch { .. })
        ));

        let wrong_environment = LibraryVersionCandidate::new(
            LibraryId::parse("library-one"),
            "demo",
            ResourceLocator {
                environment: EnvironmentRef::Wsl {
                    distro_name: "Ubuntu".to_string(),
                },
                native_path: "/libraries/library-one/skills/demo".to_string(),
            },
        );
        assert!(matches!(
            LibraryCandidateSet::for_skill(&native, &skill, vec![wrong_environment], Vec::new(),),
            Err(LibraryCandidateError::CandidateEnvironmentMismatch { .. })
        ));
    }

    #[test]
    fn candidate_set_rejects_unrecognized_and_duplicate_ordered_candidates() {
        let first = LibraryVersionCandidate::new(
            LibraryId::parse("first"),
            "demo",
            ResourceLocator {
                environment: EnvironmentRef::Native,
                native_path: "/libraries/first/skills/demo".to_string(),
            },
        );
        let second = LibraryVersionCandidate::new(
            LibraryId::parse("second"),
            "demo",
            ResourceLocator {
                environment: EnvironmentRef::Native,
                native_path: "/libraries/second/skills/demo".to_string(),
            },
        );

        assert_eq!(
            LibraryCandidateSet::new(vec![first.clone()], vec![second]),
            Err(LibraryCandidateError::OrderedCandidateNotRecognized)
        );
        assert_eq!(
            LibraryCandidateSet::new(vec![first.clone()], vec![first.clone(), first]),
            Err(LibraryCandidateError::DuplicateOrderedCandidate)
        );
    }

    #[test]
    fn candidate_set_rejects_two_candidates_from_one_library_for_the_same_skill() {
        let skill = SkillDirectoryName::try_from("ce-review").unwrap();
        let library_id = LibraryId::parse("library-one");
        let first = LibraryVersionCandidate::new(
            library_id.clone(),
            "CE:Review",
            ResourceLocator {
                environment: EnvironmentRef::Native,
                native_path: "/libraries/library-one/skills/CE:Review".to_string(),
            },
        );
        let second = LibraryVersionCandidate::new(
            library_id,
            "ce-review",
            ResourceLocator {
                environment: EnvironmentRef::Native,
                native_path: "/libraries/library-one/skills/ce-review".to_string(),
            },
        );

        assert_eq!(
            LibraryCandidateSet::for_skill(
                &EnvironmentRef::Native,
                &skill,
                vec![first.clone(), second],
                vec![first],
            ),
            Err(LibraryCandidateError::DuplicateLibraryCandidate {
                library_id: LibraryId::parse("library-one"),
            })
        );
    }

    #[test]
    fn catalog_member_index_reports_the_conflicting_names_in_one_library() {
        let library_id = LibraryId::parse("library-one");
        let skill_record = |name: &str| LibrarySkillRecord {
            name: name.to_string(),
            description: String::new(),
            source_record: serde_json::json!({}),
            content_manifest_hash: "manifest".to_string(),
            updated_at: None,
            extra: serde_json::Map::new(),
        };
        let catalog = LibraryCatalog {
            schema_version: LIBRARY_SCHEMA_VERSION,
            libraries: vec![SkillLibraryRecord {
                id: library_id.clone(),
                name: "Library One".to_string(),
                skills: vec![skill_record("CE:Review"), skill_record("ce-review")],
                extra: serde_json::Map::new(),
            }],
            extra: serde_json::Map::new(),
        };

        assert!(matches!(
            LibraryCatalogMemberIndex::build(&catalog),
            Err(LibraryCandidateError::DuplicateCatalogMemberDirectory {
                library_id: actual_library_id,
                first_member_name,
                duplicate_member_name,
            }) if actual_library_id == library_id
                && first_member_name == "CE:Review"
                && duplicate_member_name == "ce-review"
        ));
    }

    struct MemoryRepository {
        record: LibraryApplicationRecord,
        catalog: LibraryCatalog,
        locator_root: Option<PathBuf>,
        locator_requests: Mutex<Vec<(LibraryId, String)>>,
    }

    impl LibraryApplicationRepository for MemoryRepository {
        fn load_application<'a>(
            &'a self,
            _context: &'a SkillLocationRef,
        ) -> LibraryApplicationFuture<'a, Result<LibraryApplicationRecord, AppError>> {
            Box::pin(async move { Ok(self.record.clone()) })
        }

        fn save_application<'a>(
            &'a self,
            _context: &'a SkillLocationRef,
            _record: &'a LibraryApplicationRecord,
        ) -> LibraryApplicationFuture<'a, Result<(), AppError>> {
            Box::pin(async { Ok(()) })
        }

        fn library_skill_locator<'a>(
            &'a self,
            context: &'a SkillLocationRef,
            library_id: &'a LibraryId,
            skill_name: &'a str,
        ) -> LibraryApplicationFuture<'a, Result<ResourceLocator, AppError>> {
            Box::pin(async move {
                let install_dir_name =
                    crate::application::installed_skill_resolver::InstalledSkillResolver::install_dir_name(
                        skill_name,
                    )?;
                self.locator_requests
                    .lock()
                    .unwrap()
                    .push((library_id.clone(), skill_name.to_string()));
                Ok(ResourceLocator {
                    environment: context.environment.clone(),
                    native_path: self
                        .locator_root
                        .as_ref()
                        .expect("test must fail before resolving a Library member locator")
                        .join(library_id.as_str())
                        .join("skills")
                        .join(install_dir_name)
                        .to_string_lossy()
                        .into_owned(),
                })
            })
        }

        fn load_catalog<'a>(
            &'a self,
            _context: &'a SkillLocationRef,
        ) -> LibraryApplicationFuture<'a, Result<LibraryCatalog, AppError>> {
            Box::pin(async move { Ok(self.catalog.clone()) })
        }

        fn remove_application<'a>(
            &'a self,
            _context: &'a SkillLocationRef,
        ) -> LibraryApplicationFuture<'a, Result<(), AppError>> {
            Box::pin(async { Ok(()) })
        }
    }

    fn agent(id: &str) -> AgentId {
        AgentId::parse(id).unwrap()
    }

    #[test]
    fn snapshot_validates_evidence_and_selected_agents() {
        assert_eq!(
            LibraryCandidateSnapshot::new("", Vec::new(), LibraryCandidateSet::empty(),),
            Err(LibraryCandidateSnapshotError::EmptyEvidenceDigest)
        );
        assert_eq!(
            LibraryCandidateSnapshot::new(
                "digest-v1-library",
                vec![agent("codex"), agent("codex")],
                LibraryCandidateSet::empty(),
            ),
            Err(LibraryCandidateSnapshotError::DuplicateSelectedAgent)
        );

        let snapshot = LibraryCandidateSnapshot::new(
            "digest-v1-library",
            vec![agent("cursor"), agent("codex")],
            LibraryCandidateSet::empty(),
        )
        .unwrap();
        assert_eq!(snapshot.evidence_digest(), "digest-v1-library");
        assert_eq!(
            snapshot.selected_agent_ids(),
            &[agent("codex"), agent("cursor")]
        );
        assert!(snapshot.candidates().ordered().is_empty());
    }

    struct FixedSource(LibraryCandidateSnapshot);

    impl LibraryCandidateSource for FixedSource {
        fn load_candidates<'a>(
            &'a self,
            _context: &'a SkillLocationRef,
            _skill: &'a SkillDirectoryName,
        ) -> LibraryCandidateFuture<'a, Result<LibraryCandidateSnapshot, crate::error::AppError>>
        {
            Box::pin(async move { Ok(self.0.clone()) })
        }
    }

    #[tokio::test]
    async fn source_returns_the_validated_snapshot_contract() {
        let expected = LibraryCandidateSnapshot::new(
            "digest-v1-library",
            vec![agent("codex")],
            LibraryCandidateSet::empty(),
        )
        .unwrap();
        let source = FixedSource(expected.clone());
        let context = SkillLocationRef {
            environment: EnvironmentRef::Native,
            scope: SkillLocation::Global,
        };
        let skill = SkillDirectoryName::try_from("demo").unwrap();

        let actual = source.load_candidates(&context, &skill).await.unwrap();

        assert_eq!(actual, expected);
    }

    #[tokio::test]
    async fn repository_source_projects_members_and_preserves_order_and_real_names() {
        let temp = tempfile::tempdir().unwrap();
        let locator_root = temp.path().join("libraries");
        std::fs::create_dir_all(locator_root.join("first/skills/ce-review")).unwrap();
        std::fs::create_dir_all(locator_root.join("second/skills/ce-review")).unwrap();
        let physical_root = std::fs::canonicalize(&locator_root).unwrap();
        let context = SkillLocationRef {
            environment: EnvironmentRef::Native,
            scope: SkillLocation::Global,
        };
        let first_id = LibraryId::parse("first");
        let second_id = LibraryId::parse("second");
        let skill_record = |name: &str| LibrarySkillRecord {
            name: name.to_string(),
            description: String::new(),
            source_record: serde_json::json!({}),
            content_manifest_hash: "manifest".to_string(),
            updated_at: None,
            extra: serde_json::Map::new(),
        };
        let library_record = |id: LibraryId, name: &str| SkillLibraryRecord {
            id,
            name: name.to_string(),
            skills: vec![skill_record(name)],
            extra: serde_json::Map::new(),
        };
        let repository = Arc::new(MemoryRepository {
            record: LibraryApplicationRecord {
                schema_version:
                    crate::application::library_application::LIBRARY_APPLICATION_SCHEMA_VERSION,
                current: LibraryApplicationState {
                    ordered_library_ids: vec![first_id.clone(), second_id.clone()],
                    selected_agent_ids: vec![agent("cursor"), agent("codex")],
                },
                pending_operation: None,
            },
            catalog: LibraryCatalog {
                schema_version: LIBRARY_SCHEMA_VERSION,
                libraries: vec![
                    library_record(first_id.clone(), "CE:Review"),
                    library_record(second_id.clone(), "ce-review"),
                ],
                extra: serde_json::Map::new(),
            },
            locator_root: Some(locator_root),
            locator_requests: Mutex::new(Vec::new()),
        });
        let source = RepositoryLibraryCandidateSource::new(repository.clone(), targets());
        let skill = SkillDirectoryName::try_from("ce-review").unwrap();

        let snapshot = source.load_candidates(&context, &skill).await.unwrap();

        assert_eq!(
            snapshot.selected_agent_ids(),
            &[agent("codex"), agent("cursor")]
        );
        let candidates = snapshot.candidates().ordered();
        assert_eq!(
            candidates
                .iter()
                .map(|candidate| (candidate.library_id().as_str(), candidate.member_name()))
                .collect::<Vec<_>>(),
            vec![("first", "CE:Review"), ("second", "ce-review")]
        );
        assert_eq!(
            candidates[0].locator().native_path,
            physical_root
                .join("first/skills/ce-review")
                .to_string_lossy()
        );
        assert_eq!(
            candidates[1].locator().native_path,
            physical_root
                .join("second/skills/ce-review")
                .to_string_lossy()
        );
        assert_eq!(
            snapshot.candidates().recognized(),
            snapshot.candidates().ordered()
        );
        assert_eq!(
            *repository.locator_requests.lock().unwrap(),
            vec![
                (first_id, "CE:Review".to_string()),
                (second_id, "ce-review".to_string()),
            ]
        );
        assert!(snapshot.evidence_digest().starts_with("digest-v1-"));
    }

    #[tokio::test]
    async fn repository_source_loads_multiple_skill_candidates_with_one_projection() {
        let temp = tempfile::tempdir().unwrap();
        let locator_root = temp.path().join("libraries");
        for skill in ["alpha", "beta"] {
            std::fs::create_dir_all(locator_root.join("library-one/skills").join(skill)).unwrap();
        }
        let context = SkillLocationRef {
            environment: EnvironmentRef::Native,
            scope: SkillLocation::Global,
        };
        let library_id = LibraryId::parse("library-one");
        let repository = Arc::new(MemoryRepository {
            record: LibraryApplicationRecord {
                schema_version:
                    crate::application::library_application::LIBRARY_APPLICATION_SCHEMA_VERSION,
                current: LibraryApplicationState {
                    ordered_library_ids: vec![library_id.clone()],
                    selected_agent_ids: Vec::new(),
                },
                pending_operation: None,
            },
            catalog: LibraryCatalog {
                schema_version: LIBRARY_SCHEMA_VERSION,
                libraries: vec![SkillLibraryRecord {
                    id: library_id,
                    name: "Library One".to_string(),
                    skills: ["alpha", "beta"]
                        .into_iter()
                        .map(|name| LibrarySkillRecord {
                            name: name.to_string(),
                            description: String::new(),
                            source_record: serde_json::json!({}),
                            content_manifest_hash: format!("manifest-{name}"),
                            updated_at: None,
                            extra: serde_json::Map::new(),
                        })
                        .collect(),
                    extra: serde_json::Map::new(),
                }],
                extra: serde_json::Map::new(),
            },
            locator_root: Some(locator_root),
            locator_requests: Mutex::new(Vec::new()),
        });
        let calls = Arc::new(AtomicUsize::new(0));
        let source = RepositoryLibraryCandidateSource::new(
            repository,
            CountingTargets {
                inner: targets(),
                calls: Arc::clone(&calls),
            },
        );
        let skills = [
            SkillDirectoryName::try_from("alpha").unwrap(),
            SkillDirectoryName::try_from("beta").unwrap(),
        ];

        let snapshots = source.load_candidate_sets(&context, &skills).await.unwrap();

        assert_eq!(snapshots.len(), 2);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn repository_source_rejects_one_library_with_two_names_for_the_same_directory() {
        let context = SkillLocationRef {
            environment: EnvironmentRef::Native,
            scope: SkillLocation::Global,
        };
        let library_id = LibraryId::parse("library-one");
        let skill_record = |name: &str| LibrarySkillRecord {
            name: name.to_string(),
            description: String::new(),
            source_record: serde_json::json!({}),
            content_manifest_hash: "manifest".to_string(),
            updated_at: None,
            extra: serde_json::Map::new(),
        };
        let repository = Arc::new(MemoryRepository {
            record: LibraryApplicationRecord {
                schema_version:
                    crate::application::library_application::LIBRARY_APPLICATION_SCHEMA_VERSION,
                current: LibraryApplicationState {
                    ordered_library_ids: vec![library_id.clone()],
                    selected_agent_ids: Vec::new(),
                },
                pending_operation: None,
            },
            catalog: LibraryCatalog {
                schema_version: LIBRARY_SCHEMA_VERSION,
                libraries: vec![SkillLibraryRecord {
                    id: library_id,
                    name: "Library One".to_string(),
                    skills: vec![skill_record("CE:Review"), skill_record("ce-review")],
                    extra: serde_json::Map::new(),
                }],
                extra: serde_json::Map::new(),
            },
            locator_root: None,
            locator_requests: Mutex::new(Vec::new()),
        });
        let source = RepositoryLibraryCandidateSource::new(repository, targets());

        let result = source
            .load_candidates(
                &context,
                &SkillDirectoryName::try_from("ce-review").unwrap(),
            )
            .await;

        assert!(matches!(
            result,
            Err(AppError::ConfigurationCorrupted { .. })
        ));
    }

    #[tokio::test]
    async fn repository_source_rejects_a_pending_application() {
        let context = SkillLocationRef {
            environment: EnvironmentRef::Native,
            scope: SkillLocation::Global,
        };
        let current = LibraryApplicationState::default();
        let repository = Arc::new(MemoryRepository {
            record: LibraryApplicationRecord {
                schema_version:
                    crate::application::library_application::LIBRARY_APPLICATION_SCHEMA_VERSION,
                current: current.clone(),
                pending_operation: Some(PendingLibraryApplication {
                    operation_id: "operation-pending".to_string(),
                    before_application: current.clone(),
                    target_application: current,
                    preview_fingerprint: "preview-pending".to_string(),
                }),
            },
            catalog: LibraryCatalog {
                schema_version: LIBRARY_SCHEMA_VERSION,
                libraries: Vec::new(),
                extra: serde_json::Map::new(),
            },
            locator_root: None,
            locator_requests: Mutex::new(Vec::new()),
        });
        let source = RepositoryLibraryCandidateSource::new(repository, targets());

        let result = source
            .load_candidates(&context, &SkillDirectoryName::try_from("demo").unwrap())
            .await;

        assert!(matches!(result, Err(AppError::MutationBusy)));
    }

    #[tokio::test]
    async fn empty_source_returns_deterministic_empty_evidence() {
        let source = EmptyLibraryCandidateSource;
        let global = SkillLocationRef {
            environment: EnvironmentRef::Native,
            scope: SkillLocation::Global,
        };
        let project = SkillLocationRef {
            environment: EnvironmentRef::Native,
            scope: SkillLocation::Project {
                project_id: "project-one".to_string(),
            },
        };

        let first = source
            .load_candidates(&global, &SkillDirectoryName::try_from("first").unwrap())
            .await
            .unwrap();
        let second = source
            .load_candidates(&project, &SkillDirectoryName::try_from("second").unwrap())
            .await
            .unwrap();

        assert_eq!(first, second);
        assert_eq!(
            first.evidence_digest(),
            "digest-v1-empty-library-candidates"
        );
        assert!(first.selected_agent_ids().is_empty());
        assert!(first.candidates().recognized().is_empty());
        assert!(first.candidates().ordered().is_empty());
    }
}

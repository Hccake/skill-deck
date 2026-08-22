use std::collections::BTreeSet;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use crate::application::collection_records::{
    CollectionRecordReader, CollectionRecordSnapshot, DocumentRevision, LibraryCatalogRecordReader,
    LockCollectionRecordReader, RecordProjection, SkillSelection, SourceRecordRevision,
};
use crate::application::planning_facts::ScopePlanningSnapshotSource;
use crate::application::skill_libraries::{LibraryId, SkillLibraryRepository};
use crate::application::skill_paths::{
    ContentRevision, ResolvedSkillRoot, ResolvedSkillTarget, RootResolutionRevision,
    SkillPathObserver, SkillTargetRequest, TargetRevision,
};
use crate::environment::content_manifest::ContentManifestReader;
use crate::environment::planning::TargetFactResolver;
use crate::environment::types::{EnvironmentRef, ResourceLocator, SkillLocationRef};
use crate::error::AppError;

#[derive(Debug, Clone)]
pub struct UpdateSubject {
    pub skill_name: String,
    pub source_record_revision: SourceRecordRevision,
    pub target_revision: TargetRevision,
    pub content_revision: ContentRevision,
    pub projection: RecordProjection,
}

#[derive(Debug, Clone)]
pub struct UpdateSubjectSnapshot {
    pub environment: crate::environment::types::EnvironmentRef,
    pub resolution_revision: RootResolutionRevision,
    pub document_revision: DocumentRevision,
    pub subjects: Vec<UpdateSubject>,
}

pub type UpdateSubjectFuture<'a> =
    Pin<Box<dyn Future<Output = Result<UpdateSubjectSnapshot, AppError>> + Send + 'a>>;

pub trait InstalledUpdateSubjectSnapshots: Send + Sync {
    fn snapshot_installed<'a>(
        &'a self,
        context: &'a SkillLocationRef,
        names: SkillSelection,
    ) -> UpdateSubjectFuture<'a>;
}

pub trait LibraryUpdateSubjectSnapshots: Send + Sync {
    fn snapshot_library<'a>(
        &'a self,
        environment: &'a EnvironmentRef,
        library_id: &'a LibraryId,
        names: SkillSelection,
    ) -> UpdateSubjectFuture<'a>;
}

pub trait UpdateSubjectSource: Send + Sync {
    fn environment(&self) -> &crate::environment::types::EnvironmentRef;

    fn snapshot<'a>(&'a self, names: BTreeSet<String>) -> UpdateSubjectFuture<'a>;
}

pub struct BoundInstalledUpdateSubjectSource<'a, P> {
    provider: &'a P,
    context: SkillLocationRef,
}

impl<'a, P> BoundInstalledUpdateSubjectSource<'a, P> {
    pub fn new(provider: &'a P, context: SkillLocationRef) -> Self {
        Self { provider, context }
    }
}

impl<P> UpdateSubjectSource for BoundInstalledUpdateSubjectSource<'_, P>
where
    P: InstalledUpdateSubjectSnapshots,
{
    fn environment(&self) -> &EnvironmentRef {
        &self.context.environment
    }

    fn snapshot<'a>(&'a self, names: BTreeSet<String>) -> UpdateSubjectFuture<'a> {
        self.provider.snapshot_installed(&self.context, names)
    }
}

pub struct BoundLibraryUpdateSubjectSource<'a, P> {
    provider: &'a P,
    environment: EnvironmentRef,
    library_id: LibraryId,
}

impl<'a, P> BoundLibraryUpdateSubjectSource<'a, P> {
    pub fn new(provider: &'a P, environment: EnvironmentRef, library_id: LibraryId) -> Self {
        Self {
            provider,
            environment,
            library_id,
        }
    }
}

impl<P> UpdateSubjectSource for BoundLibraryUpdateSubjectSource<'_, P>
where
    P: LibraryUpdateSubjectSnapshots,
{
    fn environment(&self) -> &EnvironmentRef {
        &self.environment
    }

    fn snapshot<'a>(&'a self, names: BTreeSet<String>) -> UpdateSubjectFuture<'a> {
        self.provider
            .snapshot_library(&self.environment, &self.library_id, names)
    }
}

pub struct InstalledUpdateSubjectProvider<F, T> {
    facts: F,
    targets: T,
}

pub struct LibraryUpdateSubjectProvider<T> {
    repository: Arc<dyn SkillLibraryRepository>,
    targets: T,
}

impl<T> LibraryUpdateSubjectProvider<T> {
    pub fn new(repository: Arc<dyn SkillLibraryRepository>, targets: T) -> Self {
        Self {
            repository,
            targets,
        }
    }
}

impl<F, T> InstalledUpdateSubjectProvider<F, T> {
    pub fn new(facts: F, targets: T) -> Self {
        Self { facts, targets }
    }
}

impl<F, T> InstalledUpdateSubjectSnapshots for InstalledUpdateSubjectProvider<F, T>
where
    F: ScopePlanningSnapshotSource,
    T: TargetFactResolver + ContentManifestReader,
{
    fn snapshot_installed<'a>(
        &'a self,
        context: &'a SkillLocationRef,
        names: SkillSelection,
    ) -> UpdateSubjectFuture<'a> {
        Box::pin(async move {
            let facts = self.facts.snapshot(context).await?;
            let root = SkillPathObserver::resolve_installed_collection(
                &facts.resolved_context,
                &facts.revisions.environment,
            )?;
            if root.environment != context.environment {
                return Err(AppError::StaleContext);
            }
            let project_root =
                facts
                    .resolved_context
                    .project
                    .as_ref()
                    .map(|project| ResourceLocator {
                        environment: root.environment.clone(),
                        native_path: project.native_path.clone(),
                    });
            let records = LockCollectionRecordReader::new(
                &root.environment,
                facts.lock_schema,
                &facts.lock_document,
                project_root.as_ref(),
            )
            .load_snapshot(names)?;
            build_update_subject_snapshot(&self.targets, root, records).await
        })
    }
}

impl<T> LibraryUpdateSubjectSnapshots for LibraryUpdateSubjectProvider<T>
where
    T: TargetFactResolver + ContentManifestReader + Send + Sync,
{
    fn snapshot_library<'a>(
        &'a self,
        environment: &'a EnvironmentRef,
        library_id: &'a LibraryId,
        names: SkillSelection,
    ) -> UpdateSubjectFuture<'a> {
        Box::pin(async move {
            let root = self
                .repository
                .resolve_collection(environment, library_id)
                .await?;
            let catalog = self.repository.load(environment).await?;
            let records =
                LibraryCatalogRecordReader::new(&catalog, library_id).load_snapshot(names)?;
            build_update_subject_snapshot(&self.targets, root, records).await
        })
    }
}

async fn build_update_subject_snapshot<T>(
    targets: &T,
    root: ResolvedSkillRoot,
    records: CollectionRecordSnapshot,
) -> Result<UpdateSubjectSnapshot, AppError>
where
    T: TargetFactResolver + ContentManifestReader,
{
    if records.records.is_empty() {
        return build_update_subject_snapshot_from_targets(root, records, Vec::new());
    }
    let observed_targets = SkillPathObserver::resolve_skill_targets(
        targets,
        &root,
        records
            .records
            .iter()
            .map(|record| SkillTargetRequest {
                skill_name: record.skill_name.clone(),
            })
            .collect(),
        None,
    )
    .await?;
    build_update_subject_snapshot_from_targets(root, records, observed_targets)
}

pub fn build_update_subject_snapshot_from_targets(
    root: ResolvedSkillRoot,
    records: CollectionRecordSnapshot,
    observed_targets: Vec<ResolvedSkillTarget>,
) -> Result<UpdateSubjectSnapshot, AppError> {
    let CollectionRecordSnapshot {
        document_revision,
        records,
    } = records;
    if observed_targets.len() != records.len() {
        return Err(AppError::StaleTarget);
    }
    let subjects = records
        .into_iter()
        .zip(observed_targets)
        .map(|(record, target)| {
            if record.skill_name != target.skill_name {
                return Err(AppError::StaleTarget);
            }
            Ok(UpdateSubject {
                skill_name: target.skill_name,
                source_record_revision: record.source_record_revision,
                target_revision: target.target_revision,
                content_revision: target.content_revision,
                projection: record.projection,
            })
        })
        .collect::<Result<Vec<_>, AppError>>()?;
    Ok(UpdateSubjectSnapshot {
        environment: root.environment,
        resolution_revision: root.resolution_revision,
        document_revision,
        subjects,
    })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::collections::BTreeSet;
    use std::fs;
    use std::sync::Arc;

    use crate::application::install::InstallFuture;
    use crate::application::mutation::plan::RuntimeRevisions;
    use crate::application::planning_facts::{ScopePlanningSnapshot, ScopePlanningSnapshotSource};
    use crate::application::skill_libraries::{
        LibraryCatalog, LibraryId, LibrarySkillRecord, LibrarySkillSourceRecord,
        SkillLibraryRecord, SkillLibraryRepository, LIBRARY_SCHEMA_VERSION,
    };
    use crate::core::lossless_lock::{LockSchema, LosslessLockDocument};
    use crate::core::projects::{ProjectMigrationRegistry, ProjectMigrationState};
    use crate::environment::agent_environment::AgentRuntimeSnapshot;
    use crate::environment::context_resolver::ResolvedContext;
    use crate::environment::planning::RuntimeTargetFactResolver;
    use crate::environment::runtime::ContextSnapshotRevision;
    use crate::environment::types::{
        EnvironmentRef, EnvironmentStatus, ResourceLocator, SkillLocation, SkillLocationRef,
    };
    use crate::environment::wsl::WslRuntime;
    use crate::error::AppError;

    use super::{
        InstalledUpdateSubjectProvider, InstalledUpdateSubjectSnapshots,
        LibraryUpdateSubjectProvider, LibraryUpdateSubjectSnapshots,
    };

    struct Facts(ScopePlanningSnapshot);

    impl ScopePlanningSnapshotSource for Facts {
        fn snapshot<'a>(
            &'a self,
            _context: &'a SkillLocationRef,
        ) -> InstallFuture<'a, Result<ScopePlanningSnapshot, AppError>> {
            Box::pin(async { Ok(self.0.clone()) })
        }
    }

    #[tokio::test]
    async fn installed_provider_combines_lock_target_and_content_revisions() {
        let root = tempfile::tempdir().unwrap();
        let skill_root = root.path().join("skills");
        let skill_dir = skill_root.join("demo");
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: demo\ndescription: Demo\n---\nbody",
        )
        .unwrap();
        let context = SkillLocationRef {
            environment: EnvironmentRef::Native,
            scope: SkillLocation::Global,
        };
        let facts = ScopePlanningSnapshot {
            resolved_context: ResolvedContext {
                context: context.clone(),
                project: None,
                home: ResourceLocator {
                    environment: EnvironmentRef::Native,
                    native_path: root.path().to_string_lossy().into_owned(),
                },
                skill_root: ResourceLocator {
                    environment: EnvironmentRef::Native,
                    native_path: skill_root.to_string_lossy().into_owned(),
                },
                lock: ResourceLocator {
                    environment: EnvironmentRef::Native,
                    native_path: root.path().join("skills-lock.json").to_string_lossy().into_owned(),
                },
            },
            agent_runtime: AgentRuntimeSnapshot {
                registry_revision: "registry-v1".to_string(),
                environment_revision: "environment-v1".to_string(),
                environment: EnvironmentRef::Native,
                availability: EnvironmentStatus::Available,
                project_path: None,
                agents: BTreeMap::new(),
            },
            revisions: RuntimeRevisions {
                registry: "registry-v1".to_string(),
                environment: "environment-v1".to_string(),
                context: ContextSnapshotRevision::parse("context-v1").unwrap(),
            },
            lock_schema: LockSchema::Global,
            lock_document: LosslessLockDocument::parse(
                br#"{"version":3,"skills":{"demo":{"source":"owner/repo","sourceType":"github","sourceUrl":"https://github.com/owner/repo","ref":"main","skillPath":"skills/demo","skillFolderHash":"tree-old"}}}"#,
            )
            .unwrap(),
            eve_targets: Vec::new(),
        };
        let provider = InstalledUpdateSubjectProvider::new(
            Facts(facts),
            RuntimeTargetFactResolver::new(Arc::new(WslRuntime::default())),
        );

        let snapshot = provider
            .snapshot_installed(&context, BTreeSet::from(["demo".to_string()]))
            .await
            .unwrap();

        assert_eq!(snapshot.subjects.len(), 1);
        assert_eq!(snapshot.subjects[0].skill_name, "demo");
        assert_eq!(
            snapshot.subjects[0].projection.metadata().unwrap().source,
            "owner/repo"
        );
        assert!(snapshot.subjects[0]
            .content_revision
            .manifest_hash()
            .is_some());
    }

    #[tokio::test]
    async fn library_provider_combines_catalog_target_and_content_revisions() {
        let root = tempfile::tempdir().unwrap();
        let library_root = root.path().join("library-state");
        let library_id = LibraryId::parse("library-1");
        let skill_dir = library_root
            .join("libraries")
            .join(library_id.as_str())
            .join("skills/demo");
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: demo\ndescription: Demo\n---\nbody",
        )
        .unwrap();
        let repository = Arc::new(
            crate::runtime::skill_libraries::RuntimeSkillLibraryRepository::new(
                library_root,
                Arc::new(WslRuntime::new_with_support(false, false)),
                Arc::new(ProjectMigrationRegistry::new(
                    ProjectMigrationState::NotNeeded,
                )),
            ),
        );
        repository
            .save(
                &EnvironmentRef::Native,
                &LibraryCatalog {
                    schema_version: LIBRARY_SCHEMA_VERSION,
                    libraries: vec![SkillLibraryRecord {
                        id: library_id.clone(),
                        name: "Library".to_string(),
                        skills: vec![LibrarySkillRecord {
                            name: "demo".to_string(),
                            description: "Demo".to_string(),
                            source_record: serde_json::to_value(LibrarySkillSourceRecord {
                                source_type: "github".to_string(),
                                source: "owner/repo".to_string(),
                                reacquisition_url: Some(
                                    "https://github.com/owner/repo".to_string(),
                                ),
                                ref_name: Some("main".to_string()),
                                skill_path: Some("skills/demo".to_string()),
                                installed_revision: Some("tree-old".to_string()),
                                computed_hash: None,
                                artifact_url: None,
                                plugin_name: None,
                                well_known: None,
                                extra: serde_json::Map::new(),
                            })
                            .unwrap(),
                            content_manifest_hash: "manifest-old".to_string(),
                            updated_at: None,
                            extra: serde_json::Map::new(),
                        }],
                        extra: serde_json::Map::new(),
                    }],
                    extra: serde_json::Map::new(),
                },
            )
            .await
            .unwrap();
        let provider = LibraryUpdateSubjectProvider::new(
            repository,
            RuntimeTargetFactResolver::new(Arc::new(WslRuntime::default())),
        );
        let snapshot = provider
            .snapshot_library(
                &EnvironmentRef::Native,
                &library_id,
                BTreeSet::from(["demo".to_string()]),
            )
            .await
            .unwrap();

        assert_eq!(snapshot.subjects.len(), 1);
        assert_eq!(
            snapshot.subjects[0].projection.metadata().unwrap().source,
            "owner/repo"
        );
        assert!(snapshot.subjects[0]
            .content_revision
            .manifest_hash()
            .is_some());
    }
}

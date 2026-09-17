use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use crate::application::collection_records::{
    CollectionRecordReader, CollectionRecordSnapshot, DocumentRevision, LibraryCatalogRecordReader,
    RecordProjection, SkillSelection, SourceRecordRevision,
};
use crate::application::skill_libraries::{LibraryId, SkillLibraryRepository};
use crate::application::skill_paths::{
    ContentRevision, ResolvedSkillRoot, ResolvedSkillTarget, RootResolutionRevision,
    SkillPathObserver, SkillTargetRequest, TargetRevision,
};
use crate::environment::content_manifest::ContentManifestReader;
use crate::environment::planning::TargetFactResolver;
use crate::environment::types::EnvironmentRef;
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

pub trait LibraryUpdateSubjectSnapshots: Send + Sync {
    fn snapshot_library<'a>(
        &'a self,
        environment: &'a EnvironmentRef,
        library_id: &'a LibraryId,
        names: SkillSelection,
    ) -> UpdateSubjectFuture<'a>;
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
    use std::collections::BTreeSet;
    use std::fs;
    use std::sync::Arc;

    use crate::application::skill_libraries::{
        LibraryCatalog, LibraryId, LibrarySkillRecord, LibrarySkillSourceRecord,
        SkillLibraryRecord, SkillLibraryRepository, LIBRARY_SCHEMA_VERSION,
    };
    use crate::core::projects::{ProjectMigrationRegistry, ProjectMigrationState};
    use crate::environment::planning::RuntimeTargetFactResolver;
    use crate::environment::types::EnvironmentRef;
    use crate::environment::wsl::WslRuntime;

    use super::{LibraryUpdateSubjectProvider, LibraryUpdateSubjectSnapshots};

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
                        retired_skills: Vec::new(),
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

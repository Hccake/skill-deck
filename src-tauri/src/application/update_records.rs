use std::collections::BTreeSet;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use crate::application::agent_selection::build_agent_selection_catalog;
use crate::application::collection_records::{
    CollectionRecordReader, CollectionSkillRecord, LibraryCatalogRecordReader,
    LockCollectionRecordReader, RecordProjection,
};
use crate::application::installed_skill_resolver::SkillDirectoryName;
use crate::application::library_candidates::ResolvedLibraryCandidateIndex;
use crate::application::mutation::plan::stable_digest;
use crate::application::planning_facts::ScopePlanningSnapshotSource;
use crate::application::scope_skill_placements::observe_scope_skill_placements;
use crate::application::skill_libraries::{LibraryId, SkillLibraryRepository};
use crate::environment::planning::TargetFactResolver;
use crate::environment::types::{EnvironmentRef, ResourceLocator, SkillLocationRef};
use crate::error::AppError;

#[derive(Debug, Clone)]
pub enum UpdateRecordState {
    Source(RecordProjection),
    Excluded,
    Invalid(AppError),
}

#[derive(Debug, Clone)]
pub struct UpdateRecord {
    pub skill_name: String,
    pub revision: String,
    pub comparison_fingerprint: Option<String>,
    pub state: UpdateRecordState,
}

pub type UpdateRecordFuture<'a> =
    Pin<Box<dyn Future<Output = Result<Vec<UpdateRecord>, AppError>> + Send + 'a>>;

pub trait InstalledUpdateRecordSnapshots: Send + Sync {
    fn snapshot_installed_records<'a>(
        &'a self,
        context: &'a SkillLocationRef,
        names: BTreeSet<String>,
    ) -> UpdateRecordFuture<'a>;
}

pub trait LibraryUpdateRecordSnapshots: Send + Sync {
    fn snapshot_library_records<'a>(
        &'a self,
        environment: &'a EnvironmentRef,
        library_id: &'a LibraryId,
        names: BTreeSet<String>,
    ) -> UpdateRecordFuture<'a>;
}

pub trait UpdateRecordSource: Send + Sync {
    fn environment(&self) -> &EnvironmentRef;
    fn snapshot(&self, names: BTreeSet<String>) -> UpdateRecordFuture<'_>;
}

pub struct BoundInstalledUpdateRecords<'a, P> {
    pub provider: &'a P,
    pub context: SkillLocationRef,
}

impl<P: InstalledUpdateRecordSnapshots> UpdateRecordSource for BoundInstalledUpdateRecords<'_, P> {
    fn environment(&self) -> &EnvironmentRef {
        &self.context.environment
    }

    fn snapshot(&self, names: BTreeSet<String>) -> UpdateRecordFuture<'_> {
        self.provider
            .snapshot_installed_records(&self.context, names)
    }
}

pub struct BoundLibraryUpdateRecords<'a, P> {
    pub provider: &'a P,
    pub environment: EnvironmentRef,
    pub library_id: LibraryId,
}

impl<P: LibraryUpdateRecordSnapshots> UpdateRecordSource for BoundLibraryUpdateRecords<'_, P> {
    fn environment(&self) -> &EnvironmentRef {
        &self.environment
    }

    fn snapshot(&self, names: BTreeSet<String>) -> UpdateRecordFuture<'_> {
        self.provider
            .snapshot_library_records(&self.environment, &self.library_id, names)
    }
}

pub struct InstalledUpdateRecordProvider<F, T> {
    facts: F,
    targets: T,
    libraries: Arc<dyn SkillLibraryRepository>,
}

impl<F, T> InstalledUpdateRecordProvider<F, T> {
    pub fn new(facts: F, targets: T, libraries: Arc<dyn SkillLibraryRepository>) -> Self {
        Self {
            facts,
            targets,
            libraries,
        }
    }
}

impl<F: ScopePlanningSnapshotSource, T: TargetFactResolver> InstalledUpdateRecordSnapshots
    for InstalledUpdateRecordProvider<F, T>
{
    fn snapshot_installed_records<'a>(
        &'a self,
        context: &'a SkillLocationRef,
        names: BTreeSet<String>,
    ) -> UpdateRecordFuture<'a> {
        Box::pin(async move {
            let facts = self.facts.snapshot(context).await?;
            let project = facts
                .resolved_context
                .project
                .as_ref()
                .map(|project| ResourceLocator {
                    environment: context.environment.clone(),
                    native_path: project.native_path.clone(),
                });
            let records = LockCollectionRecordReader::new(
                &context.environment,
                facts.lock_schema,
                &facts.lock_document,
                project.as_ref(),
            )
            .load_snapshot(names)?;
            let observed_names = records
                .records
                .iter()
                .filter_map(|record| SkillDirectoryName::try_from(record.skill_name.as_str()).ok())
                .collect();
            let scope = async {
                let catalog = build_agent_selection_catalog(
                    context,
                    &facts.agent_runtime,
                    &facts.eve_targets,
                    &facts.resolved_context.skill_root,
                    &self.targets,
                )
                .await?;
                let libraries = ResolvedLibraryCandidateIndex::load_known(
                    self.libraries.as_ref(),
                    &self.targets,
                    &context.environment,
                    &observed_names,
                )
                .await?;
                Ok::<_, AppError>((catalog, libraries))
            }
            .await;
            let mut result = Vec::with_capacity(records.records.len());
            for record in records.records {
                let observation = async {
                    let (catalog, libraries) = scope.as_ref().map_err(Clone::clone)?;
                    let observed = observe_scope_skill_placements(
                        &self.targets,
                        context,
                        &record.skill_name,
                        &facts,
                        catalog,
                    )
                    .await?;
                    let placements = observed.describe(catalog, libraries)?;
                    let eligible = placements
                        .iter()
                        .any(|placement| placement.is_direct_directory());
                    let ownership = stable_digest(&(
                        &facts.revisions,
                        placements
                            .iter()
                            .map(|placement| {
                                (
                                    &placement.entry.fact.key,
                                    &placement.entry.fact.fingerprint,
                                    placement.entry.fact.entry_kind as u8,
                                    &placement.entry.fact.link_target,
                                    placement.library.as_ref().map(|library| {
                                        (library.library_id().as_str(), library.member_name())
                                    }),
                                )
                            })
                            .collect::<Vec<_>>(),
                    ))?;
                    Ok::<_, AppError>((eligible, ownership))
                }
                .await;
                let (state, ownership) = match observation {
                    Ok((true, revision)) => (
                        UpdateRecordState::Source(record.projection.clone()),
                        revision,
                    ),
                    Ok((false, revision)) => (UpdateRecordState::Excluded, revision),
                    Err(error) => (UpdateRecordState::Invalid(error), String::new()),
                };
                result.push(project_record(record, state, &ownership)?);
            }
            Ok(result)
        })
    }
}

pub struct LibraryUpdateRecordProvider {
    repository: Arc<dyn SkillLibraryRepository>,
}

impl LibraryUpdateRecordProvider {
    pub fn new(repository: Arc<dyn SkillLibraryRepository>) -> Self {
        Self { repository }
    }
}

impl LibraryUpdateRecordSnapshots for LibraryUpdateRecordProvider {
    fn snapshot_library_records<'a>(
        &'a self,
        environment: &'a EnvironmentRef,
        library_id: &'a LibraryId,
        names: BTreeSet<String>,
    ) -> UpdateRecordFuture<'a> {
        Box::pin(async move {
            let root = self
                .repository
                .resolve_collection(environment, library_id)
                .await?;
            let catalog = self.repository.load(environment).await?;
            let records =
                LibraryCatalogRecordReader::new(&catalog, library_id).load_snapshot(names)?;
            records
                .records
                .into_iter()
                .map(|record| {
                    let state = UpdateRecordState::Source(record.projection.clone());
                    project_record(record, state, root.resolution_revision.as_str())
                })
                .collect()
        })
    }
}

fn project_record(
    record: CollectionSkillRecord,
    state: UpdateRecordState,
    ownership: &str,
) -> Result<UpdateRecord, AppError> {
    let state = match &record.projection {
        RecordProjection::Missing => UpdateRecordState::Excluded,
        RecordProjection::Available(metadata)
            if matches!(metadata.source_type.as_str(), "local" | "download") =>
        {
            UpdateRecordState::Excluded
        }
        _ => state,
    };
    Ok(UpdateRecord {
        comparison_fingerprint: record
            .projection
            .metadata()
            .map(|metadata| metadata.comparison_fingerprint()),
        revision: stable_digest(&(
            "update-record-v1",
            &record.source_record_revision,
            ownership,
        ))?,
        skill_name: record.skill_name,
        state,
    })
}

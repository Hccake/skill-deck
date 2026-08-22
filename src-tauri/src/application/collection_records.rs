use std::collections::BTreeSet;

use serde::Serialize;

use crate::application::installed_skill_resolver::InstalledSkillResolver;
use crate::application::mutation::plan::stable_digest;
use crate::application::skill_libraries::{
    validate_catalog, LibraryCatalog, LibraryId, LibrarySkillRecord,
};
use crate::core::local_lock::LocalSkillLockEntry;
use crate::core::lossless_lock::{LockSchema, LosslessLockDocument};
use crate::core::skill_lock::SkillLockEntry;
use crate::core::{
    normalize_global_lock_entry, normalize_local_lock_entry, NormalizedUpdateMetadata,
};
use crate::environment::types::{same_environment_identity, EnvironmentRef, ResourceLocator};
use crate::error::AppError;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DocumentRevision(String);

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SourceRecordRevision(String);

impl SourceRecordRevision {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
impl DocumentRevision {
    pub(crate) fn for_test(value: &str) -> Self {
        Self(value.to_string())
    }
}

#[cfg(test)]
impl SourceRecordRevision {
    pub(crate) fn for_test(value: &str) -> Self {
        Self(value.to_string())
    }
}

pub type SkillSelection = BTreeSet<String>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecordProjection {
    Missing,
    Uninterpretable,
    Available(NormalizedUpdateMetadata),
}

impl RecordProjection {
    pub fn metadata(&self) -> Option<&NormalizedUpdateMetadata> {
        match self {
            Self::Available(metadata) => Some(metadata),
            Self::Missing | Self::Uninterpretable => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CollectionSkillRecord {
    pub skill_name: String,
    pub projection: RecordProjection,
    pub source_record_revision: SourceRecordRevision,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CollectionRecordSnapshot {
    pub document_revision: DocumentRevision,
    pub records: Vec<CollectionSkillRecord>,
}

pub trait CollectionRecordReader {
    fn load_snapshot(
        &self,
        selection: SkillSelection,
    ) -> Result<CollectionRecordSnapshot, AppError>;
}

pub struct LockCollectionRecordReader<'a> {
    environment: &'a EnvironmentRef,
    schema: LockSchema,
    document: &'a LosslessLockDocument,
    project_root: Option<&'a ResourceLocator>,
}

impl<'a> LockCollectionRecordReader<'a> {
    pub fn new(
        environment: &'a EnvironmentRef,
        schema: LockSchema,
        document: &'a LosslessLockDocument,
        project_root: Option<&'a ResourceLocator>,
    ) -> Self {
        Self {
            environment,
            schema,
            document,
            project_root,
        }
    }
}

impl CollectionRecordReader for LockCollectionRecordReader<'_> {
    fn load_snapshot(
        &self,
        selection: SkillSelection,
    ) -> Result<CollectionRecordSnapshot, AppError> {
        if self
            .project_root
            .is_some_and(|root| !same_environment_identity(self.environment, &root.environment))
        {
            return Err(AppError::StaleEnvironment);
        }
        let document = self.document.clone().into_value();
        let skills = document
            .get("skills")
            .and_then(serde_json::Value::as_object)
            .ok_or_else(|| AppError::ConfigurationCorrupted {
                message: "lock skills must be an object".to_string(),
            })?;
        let names = selection;
        let mut records = Vec::new();
        for name in names {
            let legacy_key = InstalledSkillResolver::install_dir_name(&name)?;
            let raw_record = skills.get(&name);
            let legacy_record = (legacy_key != name)
                .then(|| skills.get(&legacy_key))
                .flatten();
            let projection = match raw_record.or(legacy_record).cloned() {
                Some(record) => match normalize_record(self.schema, record, self.project_root) {
                    Ok(metadata) => RecordProjection::Available(metadata),
                    Err(_) => RecordProjection::Uninterpretable,
                },
                None => RecordProjection::Missing,
            };
            let source_record_revision = SourceRecordRevision(stable_digest(&(
                "lock-source-record-v1",
                &name,
                raw_record,
                &legacy_key,
                legacy_record,
            ))?);
            records.push(CollectionSkillRecord {
                skill_name: name,
                projection,
                source_record_revision,
            });
        }
        records.sort_by(|left, right| left.skill_name.cmp(&right.skill_name));
        Ok(CollectionRecordSnapshot {
            document_revision: DocumentRevision(stable_digest(&("lock-document-v1", &document))?),
            records,
        })
    }
}

pub struct LibraryCatalogRecordReader<'a> {
    catalog: &'a LibraryCatalog,
    library_id: &'a LibraryId,
}

impl<'a> LibraryCatalogRecordReader<'a> {
    pub fn new(catalog: &'a LibraryCatalog, library_id: &'a LibraryId) -> Self {
        Self {
            catalog,
            library_id,
        }
    }
}

impl CollectionRecordReader for LibraryCatalogRecordReader<'_> {
    fn load_snapshot(
        &self,
        selection: SkillSelection,
    ) -> Result<CollectionRecordSnapshot, AppError> {
        validate_catalog(self.catalog)?;
        let library = self
            .catalog
            .libraries
            .iter()
            .find(|library| &library.id == self.library_id)
            .ok_or_else(|| AppError::PathNotFound {
                path: self.library_id.as_str().to_string(),
            })?;
        let selected_names = selection;
        let mut records = selected_names
            .into_iter()
            .map(|name| {
                let record = library.skills.iter().find(|record| record.name == name);
                Ok(CollectionSkillRecord {
                    skill_name: name.clone(),
                    projection: match record {
                        Some(record) => match library_update_metadata(record) {
                            Ok(metadata) => RecordProjection::Available(metadata),
                            Err(_) => RecordProjection::Uninterpretable,
                        },
                        None => RecordProjection::Missing,
                    },
                    source_record_revision: SourceRecordRevision(stable_digest(&(
                        "library-source-record-v2",
                        &name,
                        record.map(|record| &record.source_record),
                    ))?),
                })
            })
            .collect::<Result<Vec<_>, AppError>>()?;
        records.sort_by(|left, right| left.skill_name.cmp(&right.skill_name));
        Ok(CollectionRecordSnapshot {
            document_revision: DocumentRevision(stable_digest(&(
                "library-catalog-document-v2",
                self.catalog,
            ))?),
            records,
        })
    }
}

fn library_update_metadata(
    record: &LibrarySkillRecord,
) -> Result<NormalizedUpdateMetadata, AppError> {
    let source: crate::application::skill_libraries::LibrarySkillSourceRecord =
        serde_json::from_value(record.source_record.clone())?;
    if source.source_type == "well-known"
        && source.well_known.as_ref().is_none_or(|well_known| {
            source.reacquisition_url.as_deref() != Some(&well_known.index_url)
        })
    {
        return Err(AppError::ConfigurationCorrupted {
            message: "Well-known Library source addresses do not match".to_string(),
        });
    }
    Ok(NormalizedUpdateMetadata {
        source: source.source.clone(),
        source_type: source.source_type.clone(),
        source_url: source.reacquisition_url.clone(),
        ref_name: source.ref_name.clone(),
        skill_path: source.skill_path.clone(),
        remote_hash: source.installed_revision.clone(),
        computed_hash: source.computed_hash.clone(),
        well_known_digest: source
            .well_known
            .as_ref()
            .and_then(|value| value.digest.clone()),
    })
}

fn normalize_record(
    schema: LockSchema,
    record: serde_json::Value,
    project_root: Option<&ResourceLocator>,
) -> Result<NormalizedUpdateMetadata, AppError> {
    match schema {
        LockSchema::Global => Ok(normalize_global_lock_entry(&serde_json::from_value::<
            SkillLockEntry,
        >(record)?)),
        LockSchema::Project => {
            let mut entry = serde_json::from_value::<LocalSkillLockEntry>(record)?;
            if entry.source_type == "local" {
                if let Some(project_root) = project_root {
                    entry.source = crate::core::portable_project_path::resolve_project_source(
                        &project_root.native_path,
                        &entry.source,
                    );
                }
            }
            Ok(normalize_local_lock_entry(&entry))
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use crate::application::skill_libraries::{
        LibraryCatalog, LibraryId, LibrarySkillRecord, LibrarySkillSourceRecord,
        SkillLibraryRecord, LIBRARY_SCHEMA_VERSION,
    };
    use crate::core::lossless_lock::{LockSchema, LosslessLockDocument};
    use crate::environment::types::{
        EnvironmentRef, ResourceLocator, SkillLocation, SkillLocationRef,
    };

    use super::{
        CollectionRecordReader, LibraryCatalogRecordReader, LockCollectionRecordReader,
        RecordProjection,
    };

    fn document(beta_source: &str) -> LosslessLockDocument {
        LosslessLockDocument::parse(
            format!(
                r#"{{"version":3,"skills":{{"alpha":{{"source":"owner/alpha","sourceType":"github","sourceUrl":"https://github.com/owner/alpha","ref":"main","skillPath":"skills/alpha","skillFolderHash":"tree-alpha"}},"beta":{{"source":"{beta_source}","sourceType":"github","sourceUrl":"https://github.com/owner/beta","ref":"main","skillPath":"skills/beta","skillFolderHash":"tree-beta"}}}}}}"#,
            )
            .as_bytes(),
        )
        .unwrap()
    }

    #[test]
    fn unrelated_lock_entries_change_the_document_but_not_the_selected_skill_revision() {
        let selection = BTreeSet::from(["alpha".to_string()]);
        let initial = LockCollectionRecordReader::new(
            &EnvironmentRef::Native,
            LockSchema::Global,
            &document("owner/beta"),
            None,
        )
        .load_snapshot(selection.clone())
        .unwrap();
        let changed = LockCollectionRecordReader::new(
            &EnvironmentRef::Native,
            LockSchema::Global,
            &document("other/beta"),
            None,
        )
        .load_snapshot(selection)
        .unwrap();

        assert_ne!(initial.document_revision, changed.document_revision);
        assert_eq!(initial.records.len(), 1);
        assert_eq!(
            initial.records[0].source_record_revision,
            changed.records[0].source_record_revision,
        );
    }

    #[test]
    fn raw_skill_identity_reads_the_legacy_safe_key_and_tracks_both_key_states() {
        let legacy = LosslessLockDocument::parse(
            br#"{"version":3,"skills":{"ce-review":{"source":"owner/repo","sourceType":"github","sourceUrl":"https://github.com/owner/repo","ref":"main","skillPath":"skills/ce-review","skillFolderHash":"tree-old"}}}"#,
        )
        .unwrap();
        let migrated = LosslessLockDocument::parse(
            br#"{"version":3,"skills":{"ce:review":{"source":"owner/repo","sourceType":"github","sourceUrl":"https://github.com/owner/repo","ref":"main","skillPath":"skills/ce-review","skillFolderHash":"tree-old"},"ce-review":{"source":"owner/repo","sourceType":"github","sourceUrl":"https://github.com/owner/repo","ref":"main","skillPath":"skills/ce-review","skillFolderHash":"tree-old"}}}"#,
        )
        .unwrap();
        let selection = BTreeSet::from(["ce:review".to_string()]);

        let legacy = LockCollectionRecordReader::new(
            &EnvironmentRef::Native,
            LockSchema::Global,
            &legacy,
            None,
        )
        .load_snapshot(selection.clone())
        .unwrap();
        let migrated = LockCollectionRecordReader::new(
            &EnvironmentRef::Native,
            LockSchema::Global,
            &migrated,
            None,
        )
        .load_snapshot(selection)
        .unwrap();

        assert_eq!(legacy.records[0].skill_name, "ce:review");
        assert_eq!(
            legacy.records[0].projection.metadata().unwrap().source,
            "owner/repo",
        );
        assert_ne!(
            legacy.records[0].source_record_revision,
            migrated.records[0].source_record_revision,
        );
    }

    #[test]
    fn all_selection_preserves_raw_and_safe_keys_for_conflict_detection() {
        let document = LosslessLockDocument::parse(
            br#"{"version":3,"skills":{"ce:review":{"source":"owner/repo","sourceType":"github","sourceUrl":"https://github.com/owner/repo","ref":"main","skillPath":"skills/ce-review","skillFolderHash":"tree-old"},"ce-review":{"source":"owner/repo","sourceType":"github","sourceUrl":"https://github.com/owner/repo","ref":"main","skillPath":"skills/ce-review","skillFolderHash":"tree-old"}}}"#,
        )
        .unwrap();

        let snapshot = LockCollectionRecordReader::new(
            &EnvironmentRef::Native,
            LockSchema::Global,
            &document,
            None,
        )
        .load_snapshot(BTreeSet::from([
            "ce-review".to_string(),
            "ce:review".to_string(),
        ]))
        .unwrap();

        assert_eq!(snapshot.records.len(), 2);
        assert_eq!(snapshot.records[0].skill_name, "ce-review");
        assert_eq!(snapshot.records[1].skill_name, "ce:review");
    }

    #[test]
    fn selected_missing_record_has_a_revision_that_changes_when_the_record_appears() {
        let missing = LosslessLockDocument::parse(br#"{"version":3,"skills":{}}"#).unwrap();
        let present = LosslessLockDocument::parse(
            br#"{"version":3,"skills":{"demo":{"source":"owner/repo","sourceType":"github","sourceUrl":"https://github.com/owner/repo","ref":"main","skillPath":"skills/demo","skillFolderHash":"tree-old"}}}"#,
        )
        .unwrap();
        let selection = BTreeSet::from(["demo".to_string()]);

        let missing = LockCollectionRecordReader::new(
            &EnvironmentRef::Native,
            LockSchema::Global,
            &missing,
            None,
        )
        .load_snapshot(selection.clone())
        .unwrap();
        let present = LockCollectionRecordReader::new(
            &EnvironmentRef::Native,
            LockSchema::Global,
            &present,
            None,
        )
        .load_snapshot(selection)
        .unwrap();

        assert_eq!(missing.records.len(), 1);
        assert!(matches!(
            missing.records[0].projection,
            RecordProjection::Missing
        ));
        assert_ne!(
            missing.records[0].source_record_revision,
            present.records[0].source_record_revision,
        );
    }

    #[test]
    fn incomplete_source_record_remains_observable_without_update_metadata() {
        let document = LosslessLockDocument::parse(
            br#"{"version":1,"skills":{"demo":{"source":"legacy/source","futureEntry":42}}}"#,
        )
        .unwrap();

        let snapshot = LockCollectionRecordReader::new(
            &EnvironmentRef::Native,
            LockSchema::Global,
            &document,
            None,
        )
        .load_snapshot(BTreeSet::from(["demo".to_string()]))
        .unwrap();

        assert_eq!(snapshot.records.len(), 1);
        assert_eq!(snapshot.records[0].skill_name, "demo");
        assert!(matches!(
            snapshot.records[0].projection,
            RecordProjection::Uninterpretable
        ));
    }

    #[test]
    fn selected_names_left_join_missing_uninterpretable_and_available_records() {
        let document = LosslessLockDocument::parse(
            br#"{"version":3,"skills":{"available":{"source":"owner/repo","sourceType":"github","sourceUrl":"https://github.com/owner/repo","ref":"main","skillPath":"skills/available","skillFolderHash":"tree-available"},"broken":{"source":42}}}"#,
        )
        .unwrap();
        let selection = BTreeSet::from([
            "available".to_string(),
            "broken".to_string(),
            "missing".to_string(),
        ]);

        let snapshot = LockCollectionRecordReader::new(
            &EnvironmentRef::Native,
            LockSchema::Global,
            &document,
            None,
        )
        .load_snapshot(selection)
        .unwrap();

        assert_eq!(
            snapshot
                .records
                .iter()
                .map(|record| record.skill_name.as_str())
                .collect::<Vec<_>>(),
            vec!["available", "broken", "missing"]
        );
        assert!(matches!(
            snapshot.records[0].projection,
            RecordProjection::Available(_)
        ));
        assert!(matches!(
            snapshot.records[1].projection,
            RecordProjection::Uninterpretable
        ));
        assert!(matches!(
            snapshot.records[2].projection,
            RecordProjection::Missing
        ));
    }

    #[test]
    fn project_local_source_is_resolved_from_the_registered_project_root() {
        let context = SkillLocationRef {
            environment: EnvironmentRef::Native,
            scope: SkillLocation::Project {
                project_id: "project-1".to_string(),
            },
        };
        let document = LosslessLockDocument::parse(
            br#"{"version":1,"skills":{"demo":{"source":"../shared/demo","sourceType":"local","sourceUrl":null,"ref":null,"skillPath":"demo","computedHash":"hash-old"}}}"#,
        )
        .unwrap();

        let project_root = ResourceLocator {
            environment: EnvironmentRef::Native,
            native_path: "/work/project".to_string(),
        };
        let snapshot = LockCollectionRecordReader::new(
            &context.environment,
            LockSchema::Project,
            &document,
            Some(&project_root),
        )
        .load_snapshot(BTreeSet::from(["demo".to_string()]))
        .unwrap();

        assert_eq!(snapshot.records.len(), 1);
        assert_eq!(
            snapshot.records[0].projection.metadata().unwrap().source,
            "/work/shared/demo",
        );
    }

    #[test]
    fn library_catalog_records_use_reacquisition_evidence_not_artifact_urls() {
        let library_id = LibraryId::parse("library-1");
        let catalog = library_catalog(
            &library_id,
            vec![library_skill("demo", "well-known", "sha256:index-v2")],
        );

        let snapshot = LibraryCatalogRecordReader::new(&catalog, &library_id)
            .load_snapshot(BTreeSet::from(["demo".to_string()]))
            .expect("Library catalog snapshot");

        let metadata = snapshot.records[0]
            .projection
            .metadata()
            .expect("complete source metadata");
        assert_eq!(metadata.source_type, "well-known");
        assert_eq!(
            metadata.source_url.as_deref(),
            Some("https://example.com/.well-known/skills/index.json")
        );
        assert_eq!(
            metadata.well_known_digest.as_deref(),
            Some("sha256:index-v2")
        );
        assert_ne!(
            metadata.source_url.as_deref(),
            Some("https://cdn.example.com/demo.tar.gz")
        );
    }

    #[test]
    fn changing_another_library_skill_preserves_the_selected_source_revision() {
        let library_id = LibraryId::parse("library-1");
        let initial = library_catalog(
            &library_id,
            vec![
                library_skill("alpha", "git", "alpha-v1"),
                library_skill("beta", "git", "beta-v1"),
            ],
        );
        let changed = library_catalog(
            &library_id,
            vec![
                library_skill("alpha", "git", "alpha-v1"),
                library_skill("beta", "git", "beta-v2"),
            ],
        );
        let selection = BTreeSet::from(["alpha".to_string()]);

        let initial = LibraryCatalogRecordReader::new(&initial, &library_id)
            .load_snapshot(selection.clone())
            .unwrap();
        let changed = LibraryCatalogRecordReader::new(&changed, &library_id)
            .load_snapshot(selection)
            .unwrap();

        assert_eq!(
            initial.records[0].source_record_revision,
            changed.records[0].source_record_revision
        );
        assert_ne!(initial.document_revision, changed.document_revision);
    }

    #[test]
    fn malformed_library_source_only_marks_that_member_uninterpretable() {
        let library_id = LibraryId::parse("library-1");
        let catalog: LibraryCatalog = serde_json::from_value(serde_json::json!({
            "schemaVersion": LIBRARY_SCHEMA_VERSION,
            "libraries": [{
                "id": "library-1",
                "name": "Library",
                "skills": [{
                    "name": "available",
                    "description": "Available",
                    "sourceRecord": {
                        "sourceType": "github",
                        "source": "owner/repo",
                        "reacquisitionUrl": "https://github.com/owner/repo",
                        "refName": "main",
                        "skillPath": "skills/available",
                        "installedRevision": "tree-available",
                        "computedHash": null,
                        "pluginName": null,
                        "artifactUrl": null,
                        "wellKnown": null
                    },
                    "contentManifestHash": "manifest-available"
                }, {
                    "name": "broken",
                    "description": "Broken",
                    "sourceRecord": { "sourceType": 42 },
                    "contentManifestHash": "manifest-broken"
                }]
            }]
        }))
        .expect("catalog envelope remains readable");

        let snapshot = LibraryCatalogRecordReader::new(&catalog, &library_id)
            .load_snapshot(BTreeSet::from([
                "available".to_string(),
                "broken".to_string(),
            ]))
            .unwrap();

        assert!(matches!(
            snapshot.records[0].projection,
            RecordProjection::Available(_)
        ));
        assert!(matches!(
            snapshot.records[1].projection,
            RecordProjection::Uninterpretable
        ));
    }

    fn library_catalog(library_id: &LibraryId, skills: Vec<LibrarySkillRecord>) -> LibraryCatalog {
        LibraryCatalog {
            schema_version: LIBRARY_SCHEMA_VERSION,
            libraries: vec![SkillLibraryRecord {
                id: library_id.clone(),
                name: "Library".to_string(),
                skills,
                extra: serde_json::Map::new(),
            }],
            extra: serde_json::Map::new(),
        }
    }

    fn library_skill(name: &str, source_type: &str, revision: &str) -> LibrarySkillRecord {
        LibrarySkillRecord {
            name: name.to_string(),
            description: name.to_string(),
            source_record: serde_json::to_value(LibrarySkillSourceRecord {
                source_type: source_type.to_string(),
                source: "example.com".to_string(),
                reacquisition_url: Some(
                    "https://example.com/.well-known/skills/index.json".to_string(),
                ),
                ref_name: Some("main".to_string()),
                skill_path: Some(format!("skills/{name}")),
                installed_revision: Some(revision.to_string()),
                computed_hash: Some(revision.to_string()),
                artifact_url: Some("https://cdn.example.com/demo.tar.gz".to_string()),
                plugin_name: None,
                well_known: (source_type == "well-known").then(|| {
                    crate::application::skill_libraries::LibraryWellKnownSourceRecord {
                        index_url: "https://example.com/.well-known/skills/index.json".to_string(),
                        digest: Some(revision.to_string()),
                        extra: serde_json::Map::new(),
                    }
                }),
                extra: serde_json::Map::new(),
            })
            .unwrap(),
            content_manifest_hash: format!("manifest-{name}"),
            updated_at: None,
            extra: serde_json::Map::new(),
        }
    }
}

use crate::application::collection_records::{DocumentRevision, SourceRecordRevision};
use crate::application::installed_skill_resolver::InstalledSkillResolver;
use crate::application::payload_session::{
    AcquiredPayloadHandle, DiscoverySessionHandle, PayloadPlanningMetadata, PinnedPayloadLease,
};
use crate::application::skill_paths::{ContentRevision, RootResolutionRevision, TargetRevision};
use crate::application::update_subjects::UpdateSubjectSnapshot;
use crate::core::skill::{parse_skill_md_content, SkillFrontmatter};
use crate::core::skill_payload::{PayloadEntryKind, SkillPayload, SkillPayloadManifest};
use crate::core::NormalizedUpdateMetadata;
use crate::environment::types::{same_environment_identity, EnvironmentRef};
use crate::error::AppError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NormalizedSkillSource {
    pub update: NormalizedUpdateMetadata,
    pub artifact_url: Option<String>,
    pub plugin_name: Option<String>,
}

pub struct ValidatedSkillPayload {
    handle: AcquiredPayloadHandle,
    lease: PinnedPayloadLease,
    payload: SkillPayload,
    name: String,
    install_dir_name: String,
    content_manifest: String,
    source: NormalizedSkillSource,
}

pub struct UpdateDriftComparison {
    pub ready: Vec<ReadyUpdatePayload>,
    pub stale_skill_names: Vec<String>,
}

pub struct ReadyUpdatePayload {
    pub payload: ValidatedSkillPayload,
    pub expected_resolution_revision: RootResolutionRevision,
    pub expected_target_revision: TargetRevision,
    pub expected_content_revision: ContentRevision,
    pub expected_source_record_revision: SourceRecordRevision,
    pub document_revision: DocumentRevision,
}

pub fn compare_update_subjects(
    initial: &UpdateSubjectSnapshot,
    latest: &UpdateSubjectSnapshot,
    payloads: Vec<ValidatedSkillPayload>,
) -> Result<UpdateDriftComparison, AppError> {
    if initial.environment != latest.environment {
        return Err(AppError::StaleContext);
    }
    let resolution_changed = initial.resolution_revision != latest.resolution_revision;
    let initial_by_name = initial
        .subjects
        .iter()
        .map(|subject| (subject.skill_name.as_str(), subject))
        .collect::<std::collections::BTreeMap<_, _>>();
    let latest_by_name = latest
        .subjects
        .iter()
        .map(|subject| (subject.skill_name.as_str(), subject))
        .collect::<std::collections::BTreeMap<_, _>>();
    let mut names = std::collections::BTreeSet::new();
    let mut ready = Vec::new();
    let mut stale_skill_names = Vec::new();
    for payload in payloads {
        if !names.insert(payload.name().to_string()) {
            return Err(AppError::StalePayload);
        }
        let latest_subject = initial_by_name
            .get(payload.name())
            .zip(latest_by_name.get(payload.name()))
            .and_then(|(initial, latest)| {
                (!resolution_changed
                    && initial.target_revision == latest.target_revision
                    && initial.content_revision == latest.content_revision
                    && initial.source_record_revision == latest.source_record_revision)
                    .then_some(*latest)
            });
        if let Some(latest_subject) = latest_subject {
            ready.push(ReadyUpdatePayload {
                payload,
                expected_resolution_revision: latest.resolution_revision.clone(),
                expected_target_revision: latest_subject.target_revision.clone(),
                expected_content_revision: latest_subject.content_revision.clone(),
                expected_source_record_revision: latest_subject.source_record_revision.clone(),
                document_revision: latest.document_revision.clone(),
            });
        } else {
            stale_skill_names.push(payload.name().to_string());
        }
    }
    Ok(UpdateDriftComparison {
        ready,
        stale_skill_names,
    })
}

impl ValidatedSkillPayload {
    pub async fn validate(
        handle: AcquiredPayloadHandle,
        discovery: &DiscoverySessionHandle,
        environment: &EnvironmentRef,
        expected_skill_name: &str,
        lease: PinnedPayloadLease,
    ) -> Result<Self, AppError> {
        let metadata = lease.planning_metadata();
        if expected_skill_name.trim().is_empty()
            || handle.session_id != discovery.session_id
            || handle.source_fingerprint != discovery.source_fingerprint
            || handle.skill_path != metadata.skill_path
            || handle.manifest_hash != lease.manifest().payload_root_hash
            || !same_environment_identity(&handle.environment, environment)
            || !same_environment_identity(&discovery.environment, environment)
            || metadata.validate().is_err()
            || metadata.skill_name != expected_skill_name
            || metadata.install_dir_name
                != InstalledSkillResolver::install_dir_name(&metadata.skill_name)?
        {
            return Err(AppError::StalePayload);
        }
        let payload = lease.load_payload().await?;
        if payload.payload_root_hash != handle.manifest_hash {
            return Err(AppError::StalePayload);
        }
        let frontmatter = payload_frontmatter(&payload)?;
        if frontmatter.name != metadata.skill_name {
            return Err(AppError::StalePayload);
        }
        let source = normalized_source(metadata);
        let name = frontmatter.name;
        let install_dir_name = metadata.install_dir_name.clone();
        let content_manifest = lease.manifest().payload_root_hash.clone();
        Ok(Self {
            handle,
            lease,
            payload,
            name,
            install_dir_name,
            content_manifest,
            source,
        })
    }

    pub fn handle(&self) -> &AcquiredPayloadHandle {
        &self.handle
    }

    pub fn lease(&self) -> &PinnedPayloadLease {
        &self.lease
    }

    pub fn payload(&self) -> &SkillPayload {
        &self.payload
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn install_dir_name(&self) -> &str {
        &self.install_dir_name
    }

    pub fn content_manifest(&self) -> &str {
        &self.content_manifest
    }

    pub fn source(&self) -> &NormalizedSkillSource {
        &self.source
    }

    pub fn planning_metadata(&self) -> &PayloadPlanningMetadata {
        self.lease.planning_metadata()
    }

    pub fn manifest(&self) -> &SkillPayloadManifest {
        self.lease.manifest()
    }

    pub fn into_lease(self) -> PinnedPayloadLease {
        self.lease
    }
}

fn normalized_source(metadata: &PayloadPlanningMetadata) -> NormalizedSkillSource {
    let well_known_digest = metadata
        .well_known
        .as_ref()
        .map(|well_known| well_known.digest.clone());
    let artifact_url = metadata
        .well_known
        .as_ref()
        .map(|well_known| well_known.artifact_url.clone())
        .or_else(|| {
            (metadata.source_type == "download")
                .then(|| metadata.source_url.clone())
                .flatten()
        });
    NormalizedSkillSource {
        update: NormalizedUpdateMetadata {
            source: metadata.source.clone(),
            source_type: metadata.source_type.clone(),
            source_url: metadata.source_url.clone(),
            ref_name: metadata.ref_name.clone(),
            skill_path: Some(metadata.skill_path.clone()),
            remote_hash: metadata.upstream_revision.clone(),
            computed_hash: Some(metadata.computed_hash.clone()),
            well_known_digest,
        },
        artifact_url,
        plugin_name: metadata.plugin_name.clone(),
    }
}

pub(crate) fn payload_frontmatter(payload: &SkillPayload) -> Result<SkillFrontmatter, AppError> {
    let entry = payload
        .entries
        .iter()
        .find(|entry| {
            entry.kind == PayloadEntryKind::File
                && entry.relative_path.eq_ignore_ascii_case("SKILL.md")
        })
        .ok_or_else(|| AppError::InvalidSkillMd {
            message: "Skill payload is missing SKILL.md".to_string(),
        })?;
    let blob_id = entry.blob_id.as_deref().ok_or(AppError::StalePayload)?;
    let content = payload.blobs.get(blob_id).ok_or(AppError::StalePayload)?;
    let content = std::str::from_utf8(content).map_err(|error| AppError::InvalidSkillMd {
        message: error.to_string(),
    })?;
    parse_skill_md_content(content)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::Arc;

    use tempfile::tempdir;

    use super::*;
    use crate::application::collection_records::{DocumentRevision, SourceRecordRevision};
    use crate::application::payload_session::{
        PayloadPlanningMetadata, PayloadSessionLimits, PayloadSessionManager,
        WellKnownPlanningMetadata,
    };
    use crate::application::update_subjects::{UpdateSubject, UpdateSubjectSnapshot};
    use crate::core::skill_payload::build_skill_payload;

    fn metadata(skill_name: &str) -> PayloadPlanningMetadata {
        PayloadPlanningMetadata {
            skill_name: skill_name.to_string(),
            install_dir_name: InstalledSkillResolver::install_dir_name(skill_name).unwrap(),
            source: "skills.example.com".to_string(),
            source_type: "well-known".to_string(),
            source_url: Some("https://skills.example.com/catalog/index.json".to_string()),
            ref_name: None,
            skill_path: "skills/demo".to_string(),
            plugin_name: Some("tools".to_string()),
            computed_hash: "computed-v1".to_string(),
            upstream_revision: None,
            well_known: Some(WellKnownPlanningMetadata {
                artifact_url: "https://cdn.example.com/demo.tar.gz".to_string(),
                digest: "sha256:demo-v1".to_string(),
            }),
        }
    }

    async fn acquired(
        payload_name: &str,
        metadata_name: &str,
    ) -> (
        Arc<PayloadSessionManager>,
        DiscoverySessionHandle,
        AcquiredPayloadHandle,
    ) {
        let temp = tempdir().unwrap();
        fs::write(
            temp.path().join("SKILL.md"),
            format!("---\nname: {payload_name}\ndescription: Demo\n---\nbody"),
        )
        .unwrap();
        let payload = build_skill_payload(temp.path()).unwrap();
        let manager = Arc::new(PayloadSessionManager::in_memory(
            PayloadSessionLimits {
                ttl_ms: 60_000,
                max_sessions: 4,
                max_bytes: 1_000_000,
            },
            || 1_000,
        ));
        let discovery = manager
            .discover(EnvironmentRef::Native, "source-v1")
            .await
            .unwrap();
        let handle = manager
            .acquire_payload_with_metadata(
                &discovery,
                "skills/demo",
                payload,
                metadata(metadata_name),
            )
            .await
            .unwrap();
        (manager, discovery, handle)
    }

    #[tokio::test]
    async fn validated_payload_binds_content_name_manifest_and_well_known_source() {
        let (manager, discovery, handle) = acquired("demo", "demo").await;
        let expected_manifest = handle.manifest_hash.clone();
        let lease = manager.pin_verified(&handle).await.unwrap();

        let validated = ValidatedSkillPayload::validate(
            handle.clone(),
            &discovery,
            &EnvironmentRef::Native,
            "demo",
            lease,
        )
        .await
        .unwrap();

        assert_eq!(validated.handle.session_id, handle.session_id);
        assert_eq!(validated.handle.payload_id, handle.payload_id);
        assert_eq!(validated.handle.manifest_hash, handle.manifest_hash);
        assert_eq!(validated.name, "demo");
        assert_eq!(validated.install_dir_name, "demo");
        assert_eq!(validated.content_manifest, expected_manifest);
        assert_eq!(validated.payload.payload_root_hash, expected_manifest);
        assert_eq!(validated.source.update.source_type, "well-known");
        assert_eq!(
            validated.source.update.source_url.as_deref(),
            Some("https://skills.example.com/catalog/index.json")
        );
        assert_eq!(
            validated.source.artifact_url.as_deref(),
            Some("https://cdn.example.com/demo.tar.gz")
        );
        assert_eq!(
            validated.source.update.well_known_digest.as_deref(),
            Some("sha256:demo-v1")
        );
        assert_eq!(validated.source.plugin_name.as_deref(), Some("tools"));
    }

    #[tokio::test]
    async fn validated_payload_rejects_a_skill_md_name_that_differs_from_planning_metadata() {
        let (manager, discovery, handle) = acquired("demo", "other").await;
        let lease = manager.pin_verified(&handle).await.unwrap();

        assert!(matches!(
            ValidatedSkillPayload::validate(
                handle,
                &discovery,
                &EnvironmentRef::Native,
                "other",
                lease,
            )
            .await,
            Err(AppError::StalePayload)
        ));
    }

    #[tokio::test]
    async fn canonical_update_stops_only_payloads_with_changed_subject_facts() {
        let (manager, discovery, handle) = acquired("demo", "demo").await;
        let payload = ValidatedSkillPayload::validate(
            handle.clone(),
            &discovery,
            &EnvironmentRef::Native,
            "demo",
            manager.pin_verified(&handle).await.unwrap(),
        )
        .await
        .unwrap();
        let initial = update_snapshot("target-v1", "document-v1");
        let changed = update_snapshot("target-v2", "document-v2");

        let prepared = compare_update_subjects(&initial, &changed, vec![payload]).unwrap();

        assert!(prepared.ready.is_empty());
        assert_eq!(prepared.stale_skill_names, vec!["demo"]);

        let (manager, discovery, handle) = acquired("demo", "demo").await;
        let payload = ValidatedSkillPayload::validate(
            handle.clone(),
            &discovery,
            &EnvironmentRef::Native,
            "demo",
            manager.pin_verified(&handle).await.unwrap(),
        )
        .await
        .unwrap();
        let unchanged_subject = update_snapshot("target-v1", "document-v2");
        let prepared =
            compare_update_subjects(&initial, &unchanged_subject, vec![payload]).unwrap();

        assert_eq!(prepared.ready.len(), 1);
        assert!(prepared.stale_skill_names.is_empty());
    }

    fn update_snapshot(target_revision: &str, document_revision: &str) -> UpdateSubjectSnapshot {
        UpdateSubjectSnapshot {
            environment: EnvironmentRef::Native,
            resolution_revision: crate::application::skill_paths::RootResolutionRevision::for_test(
                "collection-v1",
            ),
            document_revision: DocumentRevision::for_test(document_revision),
            subjects: vec![UpdateSubject {
                skill_name: "demo".to_string(),
                source_record_revision: SourceRecordRevision::for_test("source-v1"),
                target_revision: crate::application::skill_paths::TargetRevision::for_test(
                    target_revision,
                ),
                content_revision:
                    crate::application::skill_paths::ContentRevision::missing_for_test(),
                projection: crate::application::collection_records::RecordProjection::Available(
                    NormalizedUpdateMetadata {
                        source: "skills.example.com".to_string(),
                        source_type: "well-known".to_string(),
                        source_url: Some(
                            "https://skills.example.com/catalog/index.json".to_string(),
                        ),
                        ref_name: None,
                        skill_path: Some("skills/demo".to_string()),
                        remote_hash: None,
                        computed_hash: Some("computed-v1".to_string()),
                        well_known_digest: Some("sha256:demo-v1".to_string()),
                    },
                ),
            }],
        }
    }
}

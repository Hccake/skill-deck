use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use crate::application::payload_session::{
    AcquiredPayloadHandle, DiscoverySessionHandle, PayloadSessionManager,
};
use crate::application::skill_changes::ValidatedSkillPayload;
use crate::application::source_evidence::{RemoteEvidenceKey, SourceSnapshotFacts};
use crate::application::source_snapshot_reuse::PayloadAcquisitionKey;
use crate::core::mutation::CancellationSignal;
use crate::core::{NormalizedUpdateMetadata, SourceIdentity};
use crate::environment::types::EnvironmentRef;
use crate::error::AppError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SavedSkillSource {
    pub name: String,
    pub metadata: NormalizedUpdateMetadata,
}

impl SavedSkillSource {
    pub fn skill_path(&self) -> &str {
        self.metadata.skill_path.as_deref().unwrap_or_default()
    }
}

pub struct SavedSkillSourceGroup {
    pub source_result_id: String,
    pub source: String,
    pub environment: EnvironmentRef,
    pub key: PayloadAcquisitionKey,
    pub evidence_key: RemoteEvidenceKey,
    pub descriptor: Arc<crate::core::source_identity::AcquisitionDescriptor>,
    pub skills: Vec<SavedSkillSource>,
}

pub struct AcquiredSavedSkillSource {
    pub facts: SourceSnapshotFacts,
    pub payloads: Vec<(String, AcquiredPayloadHandle)>,
    pub skill_errors: Vec<(String, AppError)>,
    pub redirected_download_host: Option<String>,
}

pub struct SavedSkillSourceAcquisition {
    pub source_result_id: String,
    pub source: String,
    pub skill_names: Vec<String>,
    pub result: Result<AcquiredSavedSkillSource, AppError>,
}

pub struct SavedPayloadCandidate {
    pub source_result_id: String,
    pub discovery_session: DiscoverySessionHandle,
    pub skill_name: String,
    pub handle: AcquiredPayloadHandle,
}

pub struct ValidatedSavedPayload {
    pub payload: ValidatedSkillPayload,
}

pub struct FailedSavedPayload {
    pub source_result_id: String,
    pub skill_name: String,
    pub error: AppError,
}

pub struct SavedPayloadValidation {
    pub validated: Vec<ValidatedSavedPayload>,
    pub failed: Vec<FailedSavedPayload>,
}

pub type SkillSourceFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

pub trait SkillSourceModule: Send + Sync {
    fn acquire_saved_skills<'a>(
        &'a self,
        environment: &'a EnvironmentRef,
        skills: Vec<SavedSkillSource>,
        cancellation: CancellationSignal,
    ) -> SkillSourceFuture<'a, Result<Vec<SavedSkillSourceAcquisition>, AppError>> {
        Box::pin(async move {
            let groups = group_saved_skills(environment, skills)?;
            let acquisitions = match self.acquire_saved_groups(&groups, cancellation).await {
                Ok(acquisitions) => acquisitions,
                Err(AppError::MutationCancelled) => groups
                    .iter()
                    .map(|group| SavedSkillSourceAcquisition {
                        source_result_id: group.source_result_id.clone(),
                        source: group.source.clone(),
                        skill_names: group
                            .skills
                            .iter()
                            .map(|skill| skill.name.clone())
                            .collect(),
                        result: Err(AppError::MutationCancelled),
                    })
                    .collect(),
                Err(error) => return Err(error),
            };
            Ok(acquisitions)
        })
    }

    fn acquire_saved_groups<'a>(
        &'a self,
        groups: &'a [SavedSkillSourceGroup],
        cancellation: CancellationSignal,
    ) -> SkillSourceFuture<'a, Result<Vec<SavedSkillSourceAcquisition>, AppError>>;
}

pub fn group_saved_skills(
    environment: &EnvironmentRef,
    skills: Vec<SavedSkillSource>,
) -> Result<Vec<SavedSkillSourceGroup>, AppError> {
    let mut groups = Vec::<SavedSkillSourceGroup>::new();
    for skill in skills {
        let identity = SourceIdentity::from_metadata(&skill.metadata)?;
        let key = PayloadAcquisitionKey::from_identity(&identity, environment);
        if let Some(group) = groups.iter_mut().find(|group| {
            group.key == key
                && group
                    .descriptor
                    .acquisition_equivalent(identity.acquisition())
        }) {
            group.skills.push(skill);
            continue;
        }
        groups.push(SavedSkillSourceGroup {
            source_result_id: format!("source-{}", groups.len() + 1),
            source: identity.sanitized_display().to_string(),
            environment: environment.clone(),
            key,
            evidence_key: RemoteEvidenceKey::from_identity(&identity),
            descriptor: Arc::new(identity.acquisition().clone()),
            skills: vec![skill],
        });
    }
    Ok(groups)
}

pub async fn validate_saved_payloads(
    payloads: &PayloadSessionManager,
    environment: &EnvironmentRef,
    candidates: Vec<SavedPayloadCandidate>,
) -> SavedPayloadValidation {
    let mut validated = Vec::new();
    let mut failed = Vec::new();
    for candidate in candidates {
        let result = match payloads.pin_verified(&candidate.handle).await {
            Ok(lease) => {
                ValidatedSkillPayload::validate(
                    candidate.handle,
                    &candidate.discovery_session,
                    environment,
                    &candidate.skill_name,
                    lease,
                )
                .await
            }
            Err(error) => Err(error),
        };
        match result {
            Ok(payload) => validated.push(ValidatedSavedPayload { payload }),
            Err(error) => failed.push(FailedSavedPayload {
                source_result_id: candidate.source_result_id,
                skill_name: candidate.skill_name,
                error,
            }),
        }
    }
    SavedPayloadValidation { validated, failed }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;

    #[test]
    fn saved_skills_share_acquisition_only_for_equivalent_sources() {
        let groups = group_saved_skills(
            &EnvironmentRef::Native,
            vec![
                saved("alpha", "main"),
                saved("beta", "main"),
                saved("next", "next"),
            ],
        )
        .unwrap();

        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].skills.len(), 2);
        assert_eq!(groups[1].skills[0].name, "next");
    }

    struct RecordingSource {
        group_sizes: Arc<Mutex<Vec<usize>>>,
    }

    impl SkillSourceModule for RecordingSource {
        fn acquire_saved_groups<'a>(
            &'a self,
            groups: &'a [SavedSkillSourceGroup],
            _cancellation: CancellationSignal,
        ) -> SkillSourceFuture<'a, Result<Vec<SavedSkillSourceAcquisition>, AppError>> {
            *self.group_sizes.lock().unwrap() =
                groups.iter().map(|group| group.skills.len()).collect();
            Box::pin(async { Ok(Vec::new()) })
        }
    }

    #[tokio::test]
    async fn source_module_groups_unprepared_skills_inside_its_interface() {
        let group_sizes = Arc::new(Mutex::new(Vec::new()));
        let source = RecordingSource {
            group_sizes: group_sizes.clone(),
        };

        let batch = source
            .acquire_saved_skills(
                &EnvironmentRef::Native,
                vec![
                    saved("alpha", "main"),
                    saved("beta", "main"),
                    saved("next", "next"),
                ],
                CancellationSignal::default(),
            )
            .await
            .unwrap();

        assert_eq!(*group_sizes.lock().unwrap(), vec![2, 1]);
        assert!(batch.is_empty());
    }

    fn saved(name: &str, ref_name: &str) -> SavedSkillSource {
        SavedSkillSource {
            name: name.to_string(),
            metadata: NormalizedUpdateMetadata {
                source: "acme/tools".to_string(),
                source_type: "github".to_string(),
                source_url: Some("https://github.com/acme/tools.git".to_string()),
                ref_name: Some(ref_name.to_string()),
                skill_path: Some(format!("skills/{name}")),
                remote_hash: Some(format!("tree-{name}")),
                computed_hash: None,
                well_known_digest: None,
            },
        }
    }
}

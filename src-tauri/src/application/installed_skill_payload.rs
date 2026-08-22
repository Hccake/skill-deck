use std::path::Path;
use std::sync::Arc;

use crate::application::installed_skill_resolver::InstalledSkillResolver;
use crate::application::mutation::plan::stable_digest;
use crate::application::payload_session::{
    AcquiredPayloadHandle, DiscoverySourceDescriptor, DiscoverySourceLocation,
    PayloadPlanningMetadata, PayloadSessionManager, PayloadSessionStorage, PayloadStorageKey,
    RetainedDiscoverySource,
};
use crate::environment::planning::{ResolvedTargetFact, TargetEntryKind};
use crate::environment::types::{same_environment_identity, EnvironmentRef, SkillLocationRef};
use crate::environment::wsl::operations::acquire::WslPayloadSessionStorage;
use crate::environment::wsl::WslRuntime;
use crate::error::AppError;

pub struct InstalledSkillPayloadAcquirer {
    payloads: Arc<PayloadSessionManager>,
    environments: Arc<WslRuntime>,
}

impl InstalledSkillPayloadAcquirer {
    pub fn new(payloads: Arc<PayloadSessionManager>, environments: Arc<WslRuntime>) -> Self {
        Self {
            payloads,
            environments,
        }
    }

    pub async fn acquire(
        &self,
        context: &SkillLocationRef,
        skill_name: &str,
        standard: &ResolvedTargetFact,
    ) -> Result<AcquiredPayloadHandle, AppError> {
        validate_standard(context, standard)?;
        let source_fingerprint = stable_digest(&(&standard.key, &standard.fingerprint))?;
        match &context.environment {
            EnvironmentRef::Native => {
                let payload = crate::core::skill_payload::build_skill_payload(Path::new(
                    &standard.destination.native_path,
                ))?;
                let computed_hash =
                    crate::core::skill_payload::compute_cli_project_hash_from_payload(&payload)?;
                let discovery = self
                    .payloads
                    .discover(EnvironmentRef::Native, source_fingerprint)
                    .await?;
                self.payloads
                    .acquire_payload_with_metadata(
                        &discovery,
                        skill_name,
                        payload,
                        installed_metadata(skill_name, computed_hash)?,
                    )
                    .await
            }
            EnvironmentRef::Wsl { distro_name } => {
                let workspace = self.environments.workspace(distro_name)?;
                let standard_path = standard.destination.native_path.clone();
                let skill_name = skill_name.to_string();
                let storage = Arc::new(WslPayloadSessionStorage::new(workspace));
                let retained = RetainedDiscoverySource::new(
                    DiscoverySourceLocation::WslNative {
                        distro_name: distro_name.clone(),
                        linux_root: standard_path.clone(),
                        ref_revision: None,
                    },
                    DiscoverySourceDescriptor {
                        source: "installed-canonical".to_string(),
                        source_type: "installed".to_string(),
                        source_url: None,
                        ref_name: None,
                        redirected_download_host: None,
                    },
                    Default::default(),
                    (),
                );
                let discovery = self
                    .payloads
                    .discover_with_source(
                        context.environment.clone(),
                        source_fingerprint,
                        storage.clone(),
                        retained,
                    )
                    .await?;
                let key = PayloadStorageKey::new(&discovery.session_id, skill_name.clone());
                let acquired = storage
                    .acquire_from_path(&key, &standard_path, None)
                    .await?;
                self.payloads
                    .register_existing_payload_with_metadata(
                        &discovery,
                        skill_name.clone(),
                        acquired.manifest,
                        acquired.total_bytes,
                        installed_metadata(&skill_name, acquired.computed_hash)?,
                    )
                    .await
            }
        }
    }

    pub async fn current_manifest_hash(
        &self,
        context: &SkillLocationRef,
        skill_name: &str,
        standard: &ResolvedTargetFact,
    ) -> Result<String, AppError> {
        validate_standard(context, standard)?;
        match &context.environment {
            EnvironmentRef::Native => Ok(crate::core::skill_payload::build_skill_payload(
                Path::new(&standard.destination.native_path),
            )?
            .manifest()
            .payload_root_hash),
            EnvironmentRef::Wsl { distro_name } => {
                let workspace = self.environments.workspace(distro_name)?;
                let storage = Arc::new(WslPayloadSessionStorage::new(workspace));
                let session_id = format!("copy-source-check-{}", uuid::Uuid::new_v4().simple());
                let key = PayloadStorageKey::new(&session_id, skill_name);
                let acquired = storage
                    .acquire_from_path(&key, &standard.destination.native_path, None)
                    .await;
                let cleanup = storage.remove_session(&session_id).await;
                match (acquired, cleanup) {
                    (Ok(acquired), Ok(())) => Ok(acquired.manifest.payload_root_hash),
                    (Err(error), _) | (Ok(_), Err(error)) => Err(error),
                }
            }
        }
    }
}

fn validate_standard(
    context: &SkillLocationRef,
    standard: &ResolvedTargetFact,
) -> Result<(), AppError> {
    if standard.entry_kind != TargetEntryKind::Directory
        || !same_environment_identity(&standard.destination.environment, &context.environment)
    {
        return Err(AppError::StaleTarget);
    }
    Ok(())
}

fn installed_metadata(
    skill_name: &str,
    computed_hash: String,
) -> Result<PayloadPlanningMetadata, AppError> {
    Ok(PayloadPlanningMetadata {
        skill_name: skill_name.to_string(),
        install_dir_name: InstalledSkillResolver::install_dir_name(skill_name)?,
        source: "installed-canonical".to_string(),
        source_type: "installed".to_string(),
        source_url: None,
        ref_name: None,
        skill_path: skill_name.to_string(),
        plugin_name: None,
        computed_hash,
        upstream_revision: None,
        well_known: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn installed_payload_metadata_uses_the_resolved_install_directory() {
        let metadata = installed_metadata("ce:review", "computed".to_string()).unwrap();

        assert_eq!(metadata.install_dir_name, "ce-review");
    }
}

use sha2::{Digest, Sha256};
use std::path::Path;
use std::sync::Arc;

use crate::application::installed_skill_resolver::InstalledSkillResolver;
use crate::application::mutation::plan::stable_digest;
use crate::application::payload_session::{
    AcquiredPayloadHandle, DiscoverySourceDescriptor, DiscoverySourceLocation,
    PayloadPlanningMetadata, PayloadSessionManager, PayloadStorageKey, RetainedDiscoverySource,
    StoredPayload,
};
use crate::environment::planning::{ResolvedTargetFact, TargetEntryKind};
use crate::environment::types::{same_environment_identity, EnvironmentRef, SkillLocationRef};
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

    pub(crate) async fn select_source(
        &self,
        context: &SkillLocationRef,
        skill_name: &str,
        candidates: &[ResolvedTargetFact],
    ) -> Result<(ResolvedTargetFact, String), AppError> {
        let source = candidates.first().ok_or(AppError::StaleTarget)?;
        let hash = self
            .current_manifest_hash(context, skill_name, source)
            .await?;
        for candidate in &candidates[1..] {
            if self
                .current_manifest_hash(context, skill_name, candidate)
                .await?
                != hash
            {
                return Err(
                    crate::application::scope_skill_placements::ambiguous_installed_source(
                        skill_name,
                    ),
                );
            }
        }
        Ok((source.clone(), hash))
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
                validate_format(
                    skill_name,
                    crate::application::skill_changes::payload_frontmatter(&payload),
                    &standard.destination.native_path,
                )?;
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
                let storage = workspace.payload_storage();
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
                self.payloads
                    .prepare_payload(
                        &discovery,
                        skill_name.clone(),
                        move |storage, key| async move {
                            let acquired = storage
                                .acquire_from_path(&key, &standard_path, None)
                                .await?;
                            let entry = acquired
                                .manifest
                                .entries
                                .iter()
                                .find(|entry| {
                                    entry.kind == crate::core::skill_payload::PayloadEntryKind::File
                                        && entry.relative_path.eq_ignore_ascii_case("SKILL.md")
                                })
                                .ok_or_else(|| format_unavailable(&standard_path))?;
                            let blob_id = entry.blob_id.as_deref().ok_or(AppError::StalePayload)?;
                            let bytes = storage
                                .read_blob(&key, blob_id)
                                .await?
                                .ok_or(AppError::StalePayload)?;
                            if format!("{:x}", Sha256::digest(&bytes)) != blob_id {
                                return Err(AppError::StalePayload);
                            }
                            let frontmatter = std::str::from_utf8(&bytes)
                                .map_err(|error| AppError::InvalidSkillMd {
                                    message: error.to_string(),
                                })
                                .and_then(crate::core::skill::parse_skill_md_content);
                            validate_format(&skill_name, frontmatter, &standard_path)?;
                            let planning_metadata =
                                installed_metadata(&skill_name, acquired.computed_hash)?;
                            planning_metadata.validate()?;
                            Ok(StoredPayload {
                                manifest: acquired.manifest,
                                total_bytes: acquired.total_bytes,
                                planning_metadata,
                            })
                        },
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
                let storage = workspace.payload_storage();
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

fn format_unavailable(path: &str) -> AppError {
    AppError::CapabilityUnavailable {
        capability: "installedSkillFormat".into(),
        path: Some(path.into()),
    }
}

fn validate_format(
    name: &str,
    frontmatter: Result<crate::core::skill::SkillFrontmatter, AppError>,
    path: &str,
) -> Result<(), AppError> {
    if frontmatter.is_ok_and(|metadata| metadata.name == name) {
        Ok(())
    } else {
        Err(format_unavailable(path))
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

    #[tokio::test]
    async fn installed_payload_rejects_eve_content_without_ordinary_metadata() {
        use crate::environment::planning::{RuntimeTargetFactResolver, TargetFactResolver};
        let root = tempfile::tempdir().unwrap();
        std::fs::write(
            root.path().join("SKILL.md"),
            b"---\ndescription: Eve content\n---\nbody",
        )
        .unwrap();
        let environments = Arc::new(WslRuntime::default());
        let context = SkillLocationRef {
            environment: EnvironmentRef::Native,
            scope: crate::environment::types::SkillLocation::Global,
        };
        let fact = RuntimeTargetFactResolver::new(environments.clone())
            .resolve(
                &context,
                &[crate::environment::types::ResourceLocator {
                    environment: EnvironmentRef::Native,
                    native_path: root.path().to_string_lossy().into_owned(),
                }],
                None,
            )
            .await
            .unwrap()
            .remove(0);
        let manager = Arc::new(PayloadSessionManager::in_memory(
            crate::application::payload_session::PayloadSessionLimits {
                ttl_ms: 60_000,
                max_sessions: 4,
                max_bytes: 1_000_000,
            },
            || 1_000,
        ));
        let acquirer = InstalledSkillPayloadAcquirer::new(manager, environments);
        assert!(acquirer.acquire(&context, "demo", &fact).await.is_err());
        assert_eq!(
            std::fs::read(root.path().join("SKILL.md")).unwrap(),
            b"---\ndescription: Eve content\n---\nbody"
        );
    }

    #[test]
    fn installed_payload_metadata_uses_the_resolved_install_directory() {
        let metadata = installed_metadata("ce:review", "computed".to_string()).unwrap();

        assert_eq!(metadata.install_dir_name, "ce-review");
    }
}

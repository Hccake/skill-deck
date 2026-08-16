use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use specta::Type;
use walkdir::WalkDir;

use crate::application::git_transport::GitSourceTransport;
use crate::application::payload_session::{
    AcquiredPayloadHandle, DiscoverySessionHandle, DiscoverySkillSnapshot,
    DiscoverySourceDescriptor, DiscoverySourceLocation, PayloadPlanningMetadata,
    PayloadSessionManager, PayloadSessionStorage, PayloadStorageKey, RetainedDiscoverySource,
};
use crate::application::source_clone_gate::shared_source_clone_gate;
use crate::application::wellknown_access::{extract_hostname, WellKnownTrustMetadata};
use crate::application::wsl_source_access::WslSourceAccess;
use crate::core::mutation::CancellationSignal;
use crate::core::skill_paths::normalize_skill_folder_path;
use crate::core::skill_payload::{build_skill_payload, compute_cli_project_hash_from_payload};
use crate::core::{
    compute_local_tree_sha, discover_skills, get_owner_repo, CloneProgress, DiscoverOptions,
};
use crate::environment::types::{EnvironmentRef, SkillLocationRef};
use crate::error::{AppError, SourceAcquisitionFailureReason};
use crate::models::{AvailableSkill, FetchResult, ParsedSource, SourceType};

const EXCLUDED_SOURCE_FILES: &[&str] = &["metadata.json"];
const EXCLUDED_SOURCE_DIRS: &[&str] = &[".git", "__pycache__", "__pypackages__"];

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AcquireSelectedPayloadsRequest {
    pub discovery_session: DiscoverySessionHandle,
    pub skill_paths: Vec<String>,
}

#[derive(Clone)]
pub(crate) struct GitSourceDiscovery {
    sessions: Arc<PayloadSessionManager>,
    git_transport: Arc<dyn GitSourceTransport>,
    wsl_source: Arc<dyn WslSourceAccess>,
}

impl GitSourceDiscovery {
    pub(crate) fn new(
        sessions: Arc<PayloadSessionManager>,
        git_transport: Arc<dyn GitSourceTransport>,
        wsl_source: Arc<dyn WslSourceAccess>,
    ) -> Self {
        Self {
            sessions,
            git_transport,
            wsl_source,
        }
    }

    pub(crate) async fn discover<P>(
        &self,
        context: SkillLocationRef,
        parsed: ParsedSource,
        requested_source: String,
        full_depth: bool,
        on_progress: P,
        cancellation: CancellationSignal,
    ) -> Result<FetchResult, AppError>
    where
        P: Fn(CloneProgress) + Clone + Send + Sync + 'static,
    {
        if matches!(
            parsed.source_type,
            SourceType::Local | SourceType::WellKnown | SourceType::Download
        ) {
            return Err(invalid_source("Git source discovery requires a Git source"));
        }
        match &context.environment {
            EnvironmentRef::Native => {
                let clone_url = parsed.url.clone();
                let clone_ref = parsed.git_ref.clone();
                let clone_cancellation = cancellation.clone();
                let git_transport = Arc::clone(&self.git_transport);
                let clone_progress = on_progress.clone();
                let _clone_permit = shared_source_clone_gate().acquire(&cancellation).await?;
                let cloned = tokio::task::spawn_blocking(move || {
                    git_transport.clone_source(
                        &clone_url,
                        clone_ref.as_deref(),
                        &clone_progress,
                        clone_cancellation,
                    )
                })
                .await
                .map_err(|error| AppError::ExecutionFailed {
                    message: format!("native Git clone task failed: {error}"),
                })??;
                let root = cloned.repo_path.clone();
                let ref_revision = cloned.ref_revision.clone();
                retain_discovered_source(
                    self.sessions.clone(),
                    context.environment.clone(),
                    parsed.clone(),
                    requested_source.clone(),
                    DiscoverySourceLocation::Native {
                        root: root.clone(),
                        ref_revision,
                    },
                    root,
                    cloned,
                    RetainedSourceOptions {
                        full_depth,
                        ..Default::default()
                    },
                )
                .await
            }
            EnvironmentRef::Wsl { distro_name } => {
                self.wsl_source
                    .discover(
                        distro_name,
                        parsed,
                        requested_source,
                        full_depth,
                        cancellation,
                    )
                    .await
            }
        }
    }
}

pub(crate) async fn attempt_wellknown_then_download<T, WellKnownFuture, DownloadFuture>(
    well_known: WellKnownFuture,
    download: impl FnOnce() -> DownloadFuture,
    environment_label: &str,
) -> Result<T, AppError>
where
    WellKnownFuture: Future<Output = Result<T, AppError>>,
    DownloadFuture: Future<Output = Result<T, AppError>>,
{
    let well_known_error = match well_known.await {
        Ok(result) => return Ok(result),
        Err(AppError::MutationCancelled) => return Err(AppError::MutationCancelled),
        Err(error) => error,
    };
    let well_known_reason = source_acquisition_failure_reason(&well_known_error);
    match download().await {
        Ok(result) => Ok(result),
        Err(AppError::MutationCancelled) => Err(AppError::MutationCancelled),
        Err(download_error) => {
            log::warn!(
                "{environment_label} Well-known and direct download acquisition failed: well-known={well_known_error}; download={download_error}"
            );
            Err(AppError::SourceAcquisitionFailed {
                well_known_reason,
                download_reason: source_acquisition_failure_reason(&download_error),
            })
        }
    }
}

pub(crate) fn source_acquisition_failure_reason(
    error: &AppError,
) -> SourceAcquisitionFailureReason {
    match error {
        AppError::GitRepoNotFound { .. } | AppError::PathNotFound { .. } => {
            SourceAcquisitionFailureReason::NotFound
        }
        AppError::GitAuthFailed { .. } => SourceAcquisitionFailureReason::AuthenticationRequired,
        AppError::GitTimeout { .. } | AppError::WslCommandTimedOut => {
            SourceAcquisitionFailureReason::Timeout
        }
        AppError::GitNetworkError { .. } | AppError::DiscoveryRequestFailed { .. } => {
            SourceAcquisitionFailureReason::Network
        }
        AppError::InvalidSkillMd { .. } | AppError::NoSkillsFound => {
            SourceAcquisitionFailureReason::InvalidContent
        }
        AppError::DirectDownloadFailed { reason } => match reason {
            crate::error::DirectDownloadFailureReason::NotFound => {
                SourceAcquisitionFailureReason::NotFound
            }
            crate::error::DirectDownloadFailureReason::AuthenticationRequired => {
                SourceAcquisitionFailureReason::AuthenticationRequired
            }
            crate::error::DirectDownloadFailureReason::Timeout => {
                SourceAcquisitionFailureReason::Timeout
            }
            crate::error::DirectDownloadFailureReason::Network => {
                SourceAcquisitionFailureReason::Network
            }
            crate::error::DirectDownloadFailureReason::DownloadTooLarge
            | crate::error::DirectDownloadFailureReason::ArchiveTooLarge
            | crate::error::DirectDownloadFailureReason::TooManyEntries => {
                SourceAcquisitionFailureReason::LimitExceeded
            }
            crate::error::DirectDownloadFailureReason::UnsafeArchive
            | crate::error::DirectDownloadFailureReason::InvalidContent
            | crate::error::DirectDownloadFailureReason::EmptyContent => {
                SourceAcquisitionFailureReason::InvalidContent
            }
        },
        AppError::WellKnownSourceFailed { reason } => *reason,
        AppError::InvalidSource { .. } => SourceAcquisitionFailureReason::InvalidContent,
        _ => SourceAcquisitionFailureReason::Unavailable,
    }
}

#[derive(Default)]
pub(crate) struct RetainedSourceOptions {
    pub(crate) storage: Option<Arc<dyn PayloadSessionStorage>>,
    pub(crate) trust_metadata: Option<std::collections::HashMap<String, WellKnownTrustMetadata>>,
    pub(crate) redirected_download_host: Option<String>,
    pub(crate) full_depth: bool,
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn retain_discovered_source<O>(
    sessions: Arc<PayloadSessionManager>,
    environment: EnvironmentRef,
    parsed: ParsedSource,
    requested_source: String,
    location: DiscoverySourceLocation,
    native_root: PathBuf,
    owner: O,
    options: RetainedSourceOptions,
) -> Result<FetchResult, AppError>
where
    O: Send + Sync + 'static,
{
    let RetainedSourceOptions {
        storage,
        trust_metadata,
        redirected_download_host,
        full_depth,
    } = options;
    let scan_root = native_root.clone();
    let scan_subpath = parsed.subpath.clone();
    let scan_include_internal = parsed.skill_filter.is_some();
    let (discovered, catalog) = tokio::task::spawn_blocking(move || {
        build_discovery_catalog(
            &scan_root,
            scan_subpath.as_deref(),
            scan_include_internal,
            full_depth,
        )
    })
    .await
    .map_err(|error| AppError::ExecutionFailed {
        message: format!("native source discovery task failed: {error}"),
    })??;
    if discovered.is_empty() {
        return Err(AppError::NoSkillsFound);
    }
    let descriptor = DiscoverySourceDescriptor {
        source: source_identifier(&parsed, &requested_source),
        source_type: parsed.source_type.to_string(),
        source_url: (!parsed.url.is_empty()).then(|| parsed.url.clone()),
        ref_name: parsed.git_ref.clone(),
        redirected_download_host: redirected_download_host.clone(),
    };
    let source_fingerprint = snapshot_fingerprint(&descriptor, &catalog);
    let retained = RetainedDiscoverySource::new(location, descriptor, catalog, owner)
        .with_well_known_metadata(trust_metadata.clone().unwrap_or_default());
    let discovery_session = match storage {
        Some(storage) => {
            sessions
                .discover_with_source(environment, source_fingerprint, storage, retained)
                .await?
        }
        None => {
            sessions
                .discover_with_retained_source(environment, source_fingerprint, retained)
                .await?
        }
    };
    let mut skills = discovered
        .into_iter()
        .map(AvailableSkill::from)
        .collect::<Vec<_>>();
    if let Some(trust_metadata) = trust_metadata {
        apply_trust_metadata(&mut skills, &trust_metadata);
    }
    Ok(FetchResult {
        discovery_session,
        source_type: parsed.source_type.to_string(),
        source_url: parsed.url.clone(),
        redirected_download_host,
        git_ref: parsed.git_ref.clone(),
        skill_filter: parsed.skill_filter.clone(),
        skills,
    })
}

pub(crate) fn redirected_host(requested: &str, final_url: &str) -> Option<String> {
    let requested = url::Url::parse(requested).ok()?;
    let final_url = url::Url::parse(final_url).ok()?;
    (requested.host_str()? != final_url.host_str()?)
        .then(|| final_url.host_str().unwrap().to_string())
}

fn build_discovery_catalog(
    native_root: &Path,
    subpath: Option<&str>,
    include_internal: bool,
    full_depth: bool,
) -> Result<
    (
        Vec<crate::core::DiscoveredSkill>,
        std::collections::BTreeMap<String, DiscoverySkillSnapshot>,
    ),
    AppError,
> {
    let discovered = discover_skills(
        native_root,
        subpath,
        DiscoverOptions {
            include_internal,
            full_depth,
        },
    )?;
    let physical_root = fs::canonicalize(native_root)?;
    let mut catalog = std::collections::BTreeMap::new();
    for skill in &discovered {
        let source_root = resolve_skill_root(&physical_root, &skill.relative_path)?;
        catalog.insert(
            skill.relative_path.clone(),
            DiscoverySkillSnapshot {
                skill_name: skill.name.clone(),
                install_dir_name: skill.install_dir_name.clone(),
                relative_path: skill.relative_path.clone(),
                plugin_name: skill.plugin_name.clone(),
                source_metadata_fingerprint: compute_source_metadata_fingerprint(&source_root)?,
            },
        );
    }
    Ok((discovered, catalog))
}

pub(crate) fn source_identifier(parsed: &ParsedSource, requested_source: &str) -> String {
    if parsed.source_type == SourceType::WellKnown {
        return extract_hostname(&parsed.url).unwrap_or_else(|| requested_source.to_string());
    }
    if parsed.url.starts_with("git@") || parsed.url.starts_with("ssh://") {
        return parsed.url.clone();
    }
    get_owner_repo(parsed).unwrap_or_else(|| requested_source.to_string())
}

pub(crate) fn snapshot_fingerprint(
    descriptor: &DiscoverySourceDescriptor,
    catalog: &std::collections::BTreeMap<String, DiscoverySkillSnapshot>,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"skill-deck-discovery-source-v1\0");
    for value in [
        descriptor.source.as_str(),
        descriptor.source_type.as_str(),
        descriptor.source_url.as_deref().unwrap_or_default(),
        descriptor.ref_name.as_deref().unwrap_or_default(),
        descriptor
            .redirected_download_host
            .as_deref()
            .unwrap_or_default(),
    ] {
        hasher.update(value.as_bytes());
        hasher.update([0]);
    }
    for (path, skill) in catalog {
        for value in [
            path.as_str(),
            skill.skill_name.as_str(),
            skill.install_dir_name.as_str(),
            skill.plugin_name.as_deref().unwrap_or_default(),
            skill.source_metadata_fingerprint.as_str(),
        ] {
            hasher.update(value.as_bytes());
            hasher.update([0]);
        }
    }
    format!("{:x}", hasher.finalize())
}

fn apply_trust_metadata(
    skills: &mut [AvailableSkill],
    metadata: &std::collections::HashMap<String, WellKnownTrustMetadata>,
) {
    for skill in skills {
        if let Some(metadata) = metadata.get(&skill.name) {
            skill.well_known_version = metadata.well_known_version.clone();
            skill.well_known_entry_type = metadata.well_known_entry_type.clone();
            skill.artifact_url_host = metadata.artifact_url_host.clone();
            skill.digest_verified = metadata.digest_verified;
            skill.trust_reason = metadata.trust_reason.clone();
        }
    }
}

pub(crate) fn invalid_source(message: &str) -> AppError {
    AppError::InvalidSource {
        value: message.to_string(),
    }
}

#[derive(Clone)]
pub(crate) struct ManagedDownloadedDirectory {
    _owner: Arc<ManagedDownloadedDirectoryOwner>,
}

impl ManagedDownloadedDirectory {
    pub(crate) fn new(path: PathBuf) -> Self {
        Self {
            _owner: Arc::new(ManagedDownloadedDirectoryOwner(path)),
        }
    }
}

struct ManagedDownloadedDirectoryOwner(PathBuf);

impl Drop for ManagedDownloadedDirectoryOwner {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

pub struct SelectedPayloadAcquisitionService {
    sessions: Arc<PayloadSessionManager>,
}

enum PreparedNativeSelection {
    Existing(AcquiredPayloadHandle),
    Acquired(Box<PreparedNativePayload>),
}

struct PreparedNativePayload {
    relative_path: String,
    payload: crate::core::skill_payload::SkillPayload,
    metadata: PayloadPlanningMetadata,
}

impl SelectedPayloadAcquisitionService {
    pub fn new(sessions: Arc<PayloadSessionManager>) -> Self {
        Self { sessions }
    }

    pub async fn acquire(
        &self,
        request: AcquireSelectedPayloadsRequest,
    ) -> Result<Vec<AcquiredPayloadHandle>, AppError> {
        let source = self.sessions.source_snapshot(&request.discovery_session)?;
        validate_selected_paths(&request.skill_paths)?;
        let mut handles = Vec::with_capacity(request.skill_paths.len());

        match source.location() {
            DiscoverySourceLocation::Native { root, .. } => {
                let mut existing = BTreeMap::new();
                for skill_path in &request.skill_paths {
                    let skill = source.skill(skill_path).ok_or(AppError::StalePayload)?;
                    if let Some(handle) = self
                        .sessions
                        .existing_payload_handle(&request.discovery_session, &skill.relative_path)?
                    {
                        existing.insert(skill.relative_path.clone(), handle);
                    }
                }
                let selected_paths = request.skill_paths.clone();
                let source_for_task = source.clone();
                let root_for_task = root.clone();
                let prepared = tokio::task::spawn_blocking(move || {
                    prepare_native_selections(
                        &source_for_task,
                        &root_for_task,
                        &selected_paths,
                        &existing,
                    )
                })
                .await
                .map_err(|error| AppError::ExecutionFailed {
                    message: format!("native selected payload task failed: {error}"),
                })??;
                for prepared in prepared {
                    match prepared {
                        PreparedNativeSelection::Existing(handle) => handles.push(handle),
                        PreparedNativeSelection::Acquired(prepared) => {
                            let PreparedNativePayload {
                                relative_path,
                                payload,
                                metadata,
                            } = *prepared;
                            handles.push(
                                self.sessions
                                    .acquire_payload_with_metadata(
                                        &request.discovery_session,
                                        relative_path,
                                        payload,
                                        metadata,
                                    )
                                    .await?,
                            )
                        }
                    }
                }
            }
            DiscoverySourceLocation::WslNative {
                distro_name,
                linux_root,
                ..
            } => {
                let expected_distro = match &request.discovery_session.environment {
                    crate::environment::types::EnvironmentRef::Wsl { distro_name } => distro_name,
                    crate::environment::types::EnvironmentRef::Native => {
                        return Err(AppError::StaleEnvironment)
                    }
                };
                if crate::environment::types::EnvironmentKey::wsl(distro_name)
                    != crate::environment::types::EnvironmentKey::wsl(expected_distro)
                {
                    return Err(AppError::StaleEnvironment);
                }
                let storage = self
                    .sessions
                    .storage_for_discovery(&request.discovery_session)?;
                for skill_path in &request.skill_paths {
                    let skill = source.skill(skill_path).ok_or(AppError::StalePayload)?;
                    let key = PayloadStorageKey::new(
                        &request.discovery_session.session_id,
                        &skill.relative_path,
                    );
                    let linux_skill_root = format!(
                        "{}/{}",
                        linux_root.trim_end_matches('/'),
                        normalize_skill_folder_path(&skill.relative_path)
                    );
                    if storage
                        .source_metadata_fingerprint(&linux_skill_root)
                        .await?
                        != skill.source_metadata_fingerprint
                    {
                        return Err(AppError::StalePayload);
                    }
                    if let Some(existing) = self
                        .sessions
                        .existing_payload_handle(&request.discovery_session, &skill.relative_path)?
                    {
                        handles.push(existing);
                        continue;
                    }
                    let upstream_revision =
                        if requires_git_tree_revision(&source.descriptor().source_type) {
                            storage
                                .source_upstream_revision(linux_root, &skill.relative_path)
                                .await?
                        } else {
                            None
                        };
                    let acquired = storage
                        .acquire_from_source_path(
                            &key,
                            &linux_skill_root,
                            Some(CancellationSignal::default()),
                        )
                        .await?;
                    if storage
                        .source_metadata_fingerprint(&linux_skill_root)
                        .await?
                        != skill.source_metadata_fingerprint
                    {
                        let _ = storage.remove(&key).await;
                        return Err(AppError::StalePayload);
                    }
                    let metadata = planning_metadata(
                        source.descriptor(),
                        skill,
                        acquired.computed_hash,
                        upstream_revision,
                        source.well_known_metadata(&skill.skill_name),
                    );
                    handles.push(
                        self.sessions
                            .register_existing_payload_with_metadata(
                                &request.discovery_session,
                                skill.relative_path.clone(),
                                acquired.manifest,
                                acquired.total_bytes,
                                metadata,
                            )
                            .await?,
                    );
                }
            }
        }

        Ok(handles)
    }
}

fn prepare_native_selections(
    source: &RetainedDiscoverySource,
    root: &Path,
    selected_paths: &[String],
    existing: &BTreeMap<String, AcquiredPayloadHandle>,
) -> Result<Vec<PreparedNativeSelection>, AppError> {
    let physical_root = fs::canonicalize(root)?;
    let mut prepared = Vec::with_capacity(selected_paths.len());
    for skill_path in selected_paths {
        let skill = source.skill(skill_path).ok_or(AppError::StalePayload)?;
        let source_root = resolve_skill_root(&physical_root, &skill.relative_path)?;
        verify_source_fingerprint(&source_root, skill)?;
        if let Some(handle) = existing.get(&skill.relative_path) {
            prepared.push(PreparedNativeSelection::Existing(handle.clone()));
            continue;
        }
        let payload = build_skill_payload(&source_root)?;
        verify_source_fingerprint(&source_root, skill)?;
        let upstream_revision = requires_git_tree_revision(&source.descriptor().source_type)
            .then(|| compute_local_tree_sha(&physical_root, &skill.relative_path))
            .flatten();
        let metadata = planning_metadata(
            source.descriptor(),
            skill,
            compute_cli_project_hash_from_payload(&payload)?,
            upstream_revision,
            source.well_known_metadata(&skill.skill_name),
        );
        prepared.push(PreparedNativeSelection::Acquired(Box::new(
            PreparedNativePayload {
                relative_path: skill.relative_path.clone(),
                payload,
                metadata,
            },
        )));
    }
    Ok(prepared)
}

fn planning_metadata(
    source: &DiscoverySourceDescriptor,
    skill: &DiscoverySkillSnapshot,
    computed_hash: String,
    upstream_revision: Option<String>,
    well_known: Option<&WellKnownTrustMetadata>,
) -> PayloadPlanningMetadata {
    let well_known = well_known.and_then(|metadata| {
        Some(
            crate::application::payload_session::WellKnownPlanningMetadata {
                artifact_url: metadata.artifact_url.clone()?,
                digest: metadata.digest.clone()?,
            },
        )
    });
    PayloadPlanningMetadata {
        skill_name: skill.skill_name.clone(),
        install_dir_name: skill.install_dir_name.clone(),
        source: source.source.clone(),
        source_type: source.source_type.clone(),
        source_url: source.source_url.clone(),
        ref_name: source.ref_name.clone(),
        skill_path: skill.relative_path.clone(),
        plugin_name: skill.plugin_name.clone(),
        computed_hash,
        upstream_revision,
        well_known,
    }
}

fn requires_git_tree_revision(source_type: &str) -> bool {
    source_type == "github"
}

fn verify_source_fingerprint(
    source_root: &Path,
    skill: &DiscoverySkillSnapshot,
) -> Result<(), AppError> {
    if compute_source_metadata_fingerprint(source_root)? != skill.source_metadata_fingerprint {
        return Err(AppError::StalePayload);
    }
    Ok(())
}

pub fn compute_source_metadata_fingerprint(root: &Path) -> Result<String, AppError> {
    let physical_root = fs::canonicalize(root)?;
    let mut records = Vec::new();
    for entry in WalkDir::new(&physical_root)
        .follow_links(false)
        .sort_by_file_name()
        .into_iter()
        .filter_entry(|entry| !excluded_directory(entry.path(), &physical_root))
    {
        let entry = entry.map_err(|error| AppError::Io {
            message: error.to_string(),
        })?;
        if entry.path() == physical_root || excluded_file(entry.path()) {
            continue;
        }
        let relative =
            entry
                .path()
                .strip_prefix(&physical_root)
                .map_err(|error| AppError::UnsafePath {
                    path: entry.path().to_string_lossy().into_owned(),
                    reason: error.to_string(),
                })?;
        let metadata = fs::symlink_metadata(entry.path())?;
        let kind = if metadata.file_type().is_symlink() {
            "link"
        } else if metadata.is_dir() {
            "directory"
        } else if metadata.is_file() {
            "file"
        } else {
            "other"
        };
        let modified = metadata
            .modified()
            .ok()
            .and_then(|value| value.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|value| value.as_nanos())
            .unwrap_or_default();
        let link_target = metadata
            .file_type()
            .is_symlink()
            .then(|| fs::read_link(entry.path()))
            .transpose()?
            .unwrap_or_default();
        records.push((
            normalized_path(relative),
            kind,
            metadata.len(),
            modified,
            executable_mode(&metadata),
            normalized_path(&link_target),
        ));
    }
    records.sort();

    let mut hasher = Sha256::new();
    hasher.update(b"skill-deck-source-metadata-v1\0");
    for (path, kind, len, modified, mode, link_target) in records {
        hasher.update(path.as_bytes());
        hasher.update([0]);
        hasher.update(kind.as_bytes());
        hasher.update([0]);
        hasher.update(len.to_le_bytes());
        hasher.update(modified.to_le_bytes());
        hasher.update(mode.to_le_bytes());
        hasher.update(link_target.as_bytes());
        hasher.update([0]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn validate_selected_paths(skill_paths: &[String]) -> Result<(), AppError> {
    let mut unique = BTreeSet::new();
    for skill_path in skill_paths {
        if skill_path.is_empty() || !unique.insert(skill_path) {
            return Err(AppError::Validation {
                field: Some("skillPaths".to_string()),
                message: "selected Skill paths must be non-empty and unique".to_string(),
            });
        }
    }
    Ok(())
}

fn resolve_skill_root(root: &Path, relative_path: &str) -> Result<PathBuf, AppError> {
    if relative_path.is_empty()
        || relative_path.starts_with('/')
        || relative_path.starts_with('\\')
        || relative_path.contains('\\')
        || relative_path
            .split('/')
            .any(|part| part.is_empty() || matches!(part, "." | ".."))
    {
        return Err(AppError::StalePayload);
    }
    let directory = normalize_skill_folder_path(relative_path);
    let candidate = if directory.is_empty() {
        fs::canonicalize(root)?
    } else {
        fs::canonicalize(root.join(directory))?
    };
    if !candidate.starts_with(root) || !candidate.is_dir() {
        return Err(AppError::StalePayload);
    }
    Ok(candidate)
}

fn excluded_directory(path: &Path, root: &Path) -> bool {
    path != root
        && path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| EXCLUDED_SOURCE_DIRS.contains(&name))
}

fn excluded_file(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| EXCLUDED_SOURCE_FILES.contains(&name))
}

fn normalized_path(path: &Path) -> String {
    path.components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

#[cfg(unix)]
fn executable_mode(metadata: &fs::Metadata) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    metadata.permissions().mode() & 0o111
}

#[cfg(not(unix))]
fn executable_mode(_metadata: &fs::Metadata) -> u32 {
    0
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, HashMap};
    use std::fs;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    use std::io::Write;
    use tempfile::tempdir;

    use super::*;
    use crate::application::download_source::materialize_download;
    use crate::application::payload_session::{
        BackendAcquiredPayload, DiscoverySkillSnapshot, DiscoverySourceDescriptor,
        DiscoverySourceLocation, InMemoryPayloadSessionStorage, PayloadSessionLimits,
        PayloadSessionManager, PayloadSessionStorage, PayloadStorageFuture, PayloadStorageKey,
        RetainedDiscoverySource,
    };
    use crate::core::mutation::CancellationSignal;
    use crate::core::parse_source;
    use crate::core::skill_payload::{build_skill_payload, SkillPayload};
    use crate::environment::types::EnvironmentRef;
    use crate::environment::types::{SkillLocation, SkillLocationRef};
    use crate::environment::wsl::operations::source_acquisition::WslAcquisitionSource;
    use crate::models::SourceType;
    use crate::runtime::source_acquisition::SourceDiscoveryService;
    use crate::runtime::wsl_source::{build_wsl_discovery_catalog, wsl_acquisition_source};

    fn valid_skill(name: &str) -> Vec<u8> {
        format!("---\nname: {name}\ndescription: Demo\n---\n").into_bytes()
    }

    fn zip_entries(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut bytes = std::io::Cursor::new(Vec::new());
        {
            let mut writer = zip::ZipWriter::new(&mut bytes);
            for (path, content) in entries {
                writer
                    .start_file(*path, zip::write::SimpleFileOptions::default())
                    .expect("zip entry");
                writer.write_all(content).expect("zip content");
            }
            writer.finish().expect("finish zip");
        }
        bytes.into_inner()
    }

    fn tar_entries(entries: &[(&str, &[u8])], gzip: bool) -> Vec<u8> {
        let mut tar_bytes = Vec::new();
        {
            let mut builder = tar::Builder::new(&mut tar_bytes);
            for (path, content) in entries {
                let mut header = tar::Header::new_gnu();
                header.set_size(content.len() as u64);
                header.set_mode(0o644);
                header.set_cksum();
                builder
                    .append_data(&mut header, *path, *content)
                    .expect("tar entry");
            }
            builder.finish().expect("finish tar");
        }
        if !gzip {
            return tar_bytes;
        }
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(&tar_bytes).expect("gzip tar");
        encoder.finish().expect("finish gzip")
    }

    #[test]
    fn direct_download_materializes_skill_markdown_and_supported_archives() {
        let cases = [
            ("skill-md", valid_skill("single")),
            (
                "zip",
                zip_entries(&[
                    ("alpha/SKILL.md", &valid_skill("alpha")),
                    ("beta/SKILL.md", &valid_skill("beta")),
                ]),
            ),
            (
                "tar",
                tar_entries(&[("demo/SKILL.md", &valid_skill("demo"))], false),
            ),
            (
                "tar-gz",
                tar_entries(&[("demo/SKILL.md", &valid_skill("demo"))], true),
            ),
        ];

        for (case, bytes) in cases {
            let materialized =
                materialize_download(&bytes).unwrap_or_else(|error| panic!("{case}: {error}"));
            let discovered = discover_skills(materialized.path(), None, DiscoverOptions::default())
                .unwrap_or_else(|error| panic!("{case}: {error}"));
            assert!(!discovered.is_empty(), "{case}");
        }
    }

    #[test]
    fn direct_download_rejects_archive_path_collisions() {
        for bytes in [
            zip_entries(&[
                ("demo/SKILL.md", &valid_skill("demo")),
                ("demo/./SKILL.md", &valid_skill("other")),
            ]),
            zip_entries(&[("demo", b"file"), ("demo/SKILL.md", &valid_skill("demo"))]),
            zip_entries(&[
                ("Demo/SKILL.md", &valid_skill("demo")),
                ("demo/skill.md", &valid_skill("other")),
            ]),
        ] {
            assert!(matches!(
                materialize_download(&bytes),
                Err(AppError::DirectDownloadFailed { .. })
            ));
        }
    }

    #[test]
    fn direct_download_rejects_unsafe_paths_and_archive_links() {
        let mut tar_bytes = Vec::new();
        {
            let mut builder = tar::Builder::new(&mut tar_bytes);
            let mut header = tar::Header::new_gnu();
            header.set_entry_type(tar::EntryType::Symlink);
            header.set_size(0);
            header.set_mode(0o777);
            header.set_link_name("../outside").unwrap();
            header.set_cksum();
            builder
                .append_data(&mut header, "demo/SKILL.md", std::io::empty())
                .unwrap();
            builder.finish().unwrap();
        }

        for bytes in [
            zip_entries(&[("../SKILL.md", &valid_skill("demo"))]),
            zip_entries(&[("C:\\SKILL.md", &valid_skill("demo"))]),
            tar_bytes,
        ] {
            assert!(matches!(
                materialize_download(&bytes),
                Err(AppError::DirectDownloadFailed { .. })
            ));
        }

        let mut hard_link = Vec::new();
        {
            let mut builder = tar::Builder::new(&mut hard_link);
            let mut header = tar::Header::new_gnu();
            header.set_entry_type(tar::EntryType::Link);
            header.set_size(0);
            header.set_mode(0o644);
            header.set_link_name("other/SKILL.md").unwrap();
            header.set_cksum();
            builder
                .append_data(&mut header, "demo/SKILL.md", std::io::empty())
                .unwrap();
            builder.finish().unwrap();
        }
        assert!(matches!(
            materialize_download(&hard_link),
            Err(AppError::DirectDownloadFailed { .. })
        ));
    }

    #[tokio::test]
    async fn shared_http_attempt_state_machine_covers_wsl_success_cancel_and_double_failure() {
        let download_calls = AtomicUsize::new(0);
        let success = attempt_wellknown_then_download(
            async { Err::<&str, _>(AppError::NoSkillsFound) },
            || {
                download_calls.fetch_add(1, Ordering::SeqCst);
                async { Ok("downloaded") }
            },
            "WSL",
        )
        .await
        .unwrap();
        assert_eq!(success, "downloaded");
        assert_eq!(download_calls.load(Ordering::SeqCst), 1);

        let cancelled_calls = AtomicUsize::new(0);
        let cancelled = attempt_wellknown_then_download(
            async { Err::<(), _>(AppError::MutationCancelled) },
            || {
                cancelled_calls.fetch_add(1, Ordering::SeqCst);
                async { Ok(()) }
            },
            "WSL",
        )
        .await;
        assert_eq!(cancelled, Err(AppError::MutationCancelled));
        assert_eq!(cancelled_calls.load(Ordering::SeqCst), 0);

        let failure = attempt_wellknown_then_download(
            async {
                Err::<(), _>(AppError::DirectDownloadFailed {
                    reason: crate::error::DirectDownloadFailureReason::NotFound,
                })
            },
            || async {
                Err::<(), _>(AppError::DirectDownloadFailed {
                    reason: crate::error::DirectDownloadFailureReason::ArchiveTooLarge,
                })
            },
            "WSL",
        )
        .await;
        assert!(matches!(
            failure,
            Err(AppError::SourceAcquisitionFailed {
                well_known_reason: SourceAcquisitionFailureReason::NotFound,
                download_reason: SourceAcquisitionFailureReason::LimitExceeded,
            })
        ));
    }

    #[derive(Default)]
    struct SourceAcquiringStorage {
        payloads: Mutex<HashMap<PayloadStorageKey, SkillPayload>>,
        acquisitions: AtomicUsize,
        stores: AtomicUsize,
        upstream_revision: Option<String>,
    }

    impl PayloadSessionStorage for SourceAcquiringStorage {
        fn acquire_from_source_path<'a>(
            &'a self,
            key: &'a PayloadStorageKey,
            source_root: &'a str,
            _cancellation: Option<CancellationSignal>,
        ) -> PayloadStorageFuture<'a, Result<BackendAcquiredPayload, AppError>> {
            Box::pin(async move {
                self.acquisitions.fetch_add(1, Ordering::SeqCst);
                let payload = build_skill_payload(Path::new(source_root))?;
                let acquired = BackendAcquiredPayload {
                    manifest: payload.manifest(),
                    total_bytes: payload.blobs.values().map(|blob| blob.len() as u64).sum(),
                    computed_hash: compute_cli_project_hash_from_payload(&payload)?,
                };
                self.payloads
                    .lock()
                    .expect("payloads")
                    .insert(key.clone(), payload);
                Ok(acquired)
            })
        }

        fn store<'a>(
            &'a self,
            key: &'a PayloadStorageKey,
            payload: SkillPayload,
        ) -> PayloadStorageFuture<'a, Result<u64, AppError>> {
            Box::pin(async move {
                self.stores.fetch_add(1, Ordering::SeqCst);
                let bytes = payload.blobs.values().map(|blob| blob.len() as u64).sum();
                self.payloads
                    .lock()
                    .expect("payloads")
                    .insert(key.clone(), payload);
                Ok(bytes)
            })
        }

        fn source_metadata_fingerprint<'a>(
            &'a self,
            source_root: &'a str,
        ) -> PayloadStorageFuture<'a, Result<String, AppError>> {
            Box::pin(async move { compute_source_metadata_fingerprint(Path::new(source_root)) })
        }

        fn source_upstream_revision<'a>(
            &'a self,
            _repository_root: &'a str,
            _skill_path: &'a str,
        ) -> PayloadStorageFuture<'a, Result<Option<String>, AppError>> {
            Box::pin(async move { Ok(self.upstream_revision.clone()) })
        }

        fn verify<'a>(
            &'a self,
            key: &'a PayloadStorageKey,
        ) -> PayloadStorageFuture<
            'a,
            Result<Option<crate::core::skill_payload::SkillPayloadManifest>, AppError>,
        > {
            Box::pin(async move {
                Ok(self
                    .payloads
                    .lock()
                    .expect("payloads")
                    .get(key)
                    .map(SkillPayload::manifest))
            })
        }

        fn read_blob<'a>(
            &'a self,
            key: &'a PayloadStorageKey,
            blob_id: &'a str,
        ) -> PayloadStorageFuture<'a, Result<Option<Vec<u8>>, AppError>> {
            Box::pin(async move {
                Ok(self
                    .payloads
                    .lock()
                    .expect("payloads")
                    .get(key)
                    .and_then(|payload| payload.blobs.get(blob_id))
                    .cloned())
            })
        }

        fn remove<'a>(
            &'a self,
            key: &'a PayloadStorageKey,
        ) -> PayloadStorageFuture<'a, Result<(), AppError>> {
            Box::pin(async move {
                self.payloads.lock().expect("payloads").remove(key);
                Ok(())
            })
        }

        fn remove_session<'a>(
            &'a self,
            session_id: &'a str,
        ) -> PayloadStorageFuture<'a, Result<(), AppError>> {
            Box::pin(async move {
                self.payloads
                    .lock()
                    .expect("payloads")
                    .retain(|key, _| key.session_id() != session_id);
                Ok(())
            })
        }
    }

    #[test]
    fn source_environment_routing_keeps_well_known_on_native_and_git_local_in_wsl() {
        let git = parse_source("owner/repo#main").expect("git");
        let local = parse_source("/home/alice/code/skills").expect("local");
        let well_known = parse_source("https://skills.example.com").expect("well-known");

        assert_eq!(git.source_type, SourceType::GitHub);
        assert_eq!(
            wsl_acquisition_source(&git).expect("WSL git"),
            WslAcquisitionSource::Git {
                url: git.url.clone(),
                git_ref: Some("main".to_string()),
            }
        );
        assert_eq!(
            wsl_acquisition_source(&local).expect("WSL local"),
            WslAcquisitionSource::Local {
                native_path: "/home/alice/code/skills".to_string(),
            }
        );
        assert!(matches!(
            wsl_acquisition_source(&well_known),
            Err(AppError::InvalidSource { .. })
        ));
    }

    #[test]
    fn planning_metadata_does_not_treat_payload_hash_as_github_revision() {
        let metadata = planning_metadata(
            &DiscoverySourceDescriptor {
                source: "owner/repo".to_string(),
                source_type: "github".to_string(),
                source_url: Some("https://github.com/owner/repo.git".to_string()),
                ref_name: None,
                redirected_download_host: None,
            },
            &DiscoverySkillSnapshot {
                skill_name: "demo".to_string(),
                install_dir_name: "demo".to_string(),
                relative_path: "skills/demo".to_string(),
                plugin_name: None,
                source_metadata_fingerprint: "fingerprint".to_string(),
            },
            "payload-sha256".to_string(),
            None,
            None,
        );

        assert_eq!(metadata.computed_hash, "payload-sha256");
        assert_eq!(metadata.upstream_revision, None);
    }

    #[test]
    fn planning_metadata_keeps_generic_git_computed_hash_out_of_upstream_revision() {
        let metadata = planning_metadata(
            &DiscoverySourceDescriptor {
                source: "example.com/owner/repo".to_string(),
                source_type: "git".to_string(),
                source_url: Some("https://example.com/owner/repo.git".to_string()),
                ref_name: Some("main".to_string()),
                redirected_download_host: None,
            },
            &DiscoverySkillSnapshot {
                skill_name: "demo".to_string(),
                install_dir_name: "demo".to_string(),
                relative_path: "skills/demo".to_string(),
                plugin_name: None,
                source_metadata_fingerprint: "fingerprint".to_string(),
            },
            "cli-computed-hash".to_string(),
            None,
            None,
        );

        assert_eq!(metadata.computed_hash, "cli-computed-hash");
        assert_eq!(metadata.upstream_revision, None);
        assert_eq!(metadata.global_skill_folder_hash(), "cli-computed-hash");
    }

    #[test]
    fn wsl_discovery_matches_native_paths_internal_filter_and_plugin_metadata() {
        use crate::environment::wsl::operations::scan::{
            ScanResponse, ScannedEntry, ScannedEntryKind,
        };

        let file = |relative_path: &str, content: &[u8]| ScannedEntry {
            root_index: 0,
            relative_path: relative_path.to_string(),
            kind: ScannedEntryKind::File,
            resolved_target: None,
            size: content.len() as u64,
            mode: 0o644,
            modified_seconds: 10,
            content_bytes: content.to_vec(),
            truncated: false,
            error_code: None,
        };
        let response = ScanResponse {
            entries: vec![
                file(
                    ".claude-plugin/plugin.json",
                    br#"{"name":"toolkit","skills":["./skills/demo"]}"#,
                ),
                file(
                    "skills/demo/SKILL.md",
                    b"---\nname: demo\ndescription: Demo\n---\n",
                ),
                file(
                    "skills/private/SKILL.md",
                    b"---\nname: private\ndescription: Private\nmetadata:\n  internal: true\n---\n",
                ),
                file("skills/broken/SKILL.md", b"not frontmatter"),
            ],
            root_count: 1,
            total_content_bytes: 0,
        };

        let (discovered, catalog) =
            build_wsl_discovery_catalog(&response, None, false, false).expect("catalog");

        assert_eq!(discovered.len(), 1);
        assert_eq!(discovered[0].relative_path, "skills/demo/SKILL.md");
        assert_eq!(discovered[0].plugin_name.as_deref(), Some("toolkit"));
        assert_eq!(
            catalog
                .get("skills/demo/SKILL.md")
                .and_then(|skill| skill.plugin_name.as_deref()),
            Some("toolkit")
        );
    }

    #[test]
    fn wsl_discovery_supports_a_source_root_that_is_itself_a_skill() {
        use crate::environment::wsl::operations::scan::{
            ScanResponse, ScannedEntry, ScannedEntryKind,
        };

        let response = ScanResponse {
            entries: vec![ScannedEntry {
                root_index: 0,
                relative_path: "SKILL.md".to_string(),
                kind: ScannedEntryKind::File,
                resolved_target: None,
                size: 42,
                mode: 0o644,
                modified_seconds: 10,
                content_bytes: b"---\nname: demo\ndescription: Demo\n---\n".to_vec(),
                truncated: false,
                error_code: None,
            }],
            root_count: 1,
            total_content_bytes: 42,
        };

        let (discovered, _) =
            build_wsl_discovery_catalog(&response, None, false, false).expect("catalog");

        assert_eq!(discovered.len(), 1);
        assert_eq!(discovered[0].relative_path, "SKILL.md");
    }

    #[test]
    fn wsl_discovery_uses_priority_results_without_unrelated_recursive_matches() {
        use crate::environment::wsl::operations::scan::{
            ScanResponse, ScannedEntry, ScannedEntryKind,
        };

        let file = |relative_path: &str, content: &[u8]| ScannedEntry {
            root_index: 0,
            relative_path: relative_path.to_string(),
            kind: ScannedEntryKind::File,
            resolved_target: None,
            size: content.len() as u64,
            mode: 0o644,
            modified_seconds: 10,
            content_bytes: content.to_vec(),
            truncated: false,
            error_code: None,
        };
        let response = ScanResponse {
            entries: vec![
                file(
                    "skills/demo/SKILL.md",
                    b"---\nname: demo\ndescription: Demo\n---\n",
                ),
                file(
                    "examples/unrelated/SKILL.md",
                    b"---\nname: unrelated\ndescription: Unrelated\n---\n",
                ),
            ],
            root_count: 1,
            total_content_bytes: 0,
        };

        let (discovered, _) =
            build_wsl_discovery_catalog(&response, None, false, false).expect("catalog");

        assert_eq!(
            discovered
                .iter()
                .map(|skill| skill.name.as_str())
                .collect::<Vec<_>>(),
            vec!["demo"]
        );
    }

    #[test]
    fn wsl_valid_root_skill_shadows_nested_candidates() {
        use crate::environment::wsl::operations::scan::{
            ScanResponse, ScannedEntry, ScannedEntryKind,
        };

        let file = |relative_path: &str, content: &[u8]| ScannedEntry {
            root_index: 0,
            relative_path: relative_path.to_string(),
            kind: ScannedEntryKind::File,
            resolved_target: None,
            size: content.len() as u64,
            mode: 0o644,
            modified_seconds: 10,
            content_bytes: content.to_vec(),
            truncated: false,
            error_code: None,
        };
        let response = ScanResponse {
            entries: vec![
                file("SKILL.md", b"---\nname: root\ndescription: Root\n---\n"),
                file(
                    "skills/nested/SKILL.md",
                    b"---\nname: nested\ndescription: Nested\n---\n",
                ),
            ],
            root_count: 1,
            total_content_bytes: 0,
        };

        let (discovered, _) =
            build_wsl_discovery_catalog(&response, None, false, false).expect("catalog");

        assert_eq!(
            discovered
                .iter()
                .map(|skill| skill.name.as_str())
                .collect::<Vec<_>>(),
            vec!["root"]
        );
    }

    #[test]
    fn wsl_full_depth_keeps_nested_candidates_beside_root_skill() {
        use crate::environment::wsl::operations::scan::{
            ScanResponse, ScannedEntry, ScannedEntryKind,
        };

        let file = |relative_path: &str, content: &[u8]| ScannedEntry {
            root_index: 0,
            relative_path: relative_path.to_string(),
            kind: ScannedEntryKind::File,
            resolved_target: None,
            size: content.len() as u64,
            mode: 0o644,
            modified_seconds: 10,
            content_bytes: content.to_vec(),
            truncated: false,
            error_code: None,
        };
        let response = ScanResponse {
            entries: vec![
                file("SKILL.md", b"---\nname: root\ndescription: Root\n---\n"),
                file(
                    "plugins/example/skills/deep/SKILL.md",
                    b"---\nname: deep\ndescription: Deep\n---\n",
                ),
            ],
            root_count: 1,
            total_content_bytes: 0,
        };

        let (discovered, _) =
            build_wsl_discovery_catalog(&response, None, false, true).expect("catalog");

        assert_eq!(
            discovered
                .iter()
                .map(|skill| skill.name.as_str())
                .collect::<Vec<_>>(),
            vec!["root", "deep"]
        );
    }

    #[test]
    fn wsl_locked_root_skill_continues_discovery_inside_the_selected_subpath() {
        use crate::environment::wsl::operations::scan::{
            ScanResponse, ScannedEntry, ScannedEntryKind,
        };

        let file = |root_index: u32, relative_path: &str, content: &[u8]| ScannedEntry {
            root_index,
            relative_path: relative_path.to_string(),
            kind: ScannedEntryKind::File,
            resolved_target: None,
            size: content.len() as u64,
            mode: 0o644,
            modified_seconds: 10,
            content_bytes: content.to_vec(),
            truncated: false,
            error_code: None,
        };
        let response = ScanResponse {
            entries: vec![
                file(
                    0,
                    "SKILL.md",
                    b"---\nname: demo\ndescription: Installed\n---\n",
                ),
                file(
                    0,
                    "nested/SKILL.md",
                    b"---\nname: source\ndescription: Source\n---\n",
                ),
                file(
                    1,
                    "",
                    br#"{"version":1,"skills":{"demo":{"source":"owner/repo"}}}"#,
                ),
            ],
            root_count: 2,
            total_content_bytes: 0,
        };

        let (discovered, _) =
            build_wsl_discovery_catalog(&response, Some(".agents/skills/demo"), false, false)
                .expect("catalog");

        assert_eq!(
            discovered
                .iter()
                .map(|skill| skill.name.as_str())
                .collect::<Vec<_>>(),
            vec!["source"]
        );
        assert_eq!(
            discovered[0].relative_path,
            ".agents/skills/demo/nested/SKILL.md"
        );
    }

    #[tokio::test]
    async fn native_selected_acquisition_returns_immutable_handles_with_complete_metadata() {
        let source = tempdir().expect("source");
        let skill_root = source.path().join("skills/demo");
        fs::create_dir_all(skill_root.join("scripts")).expect("scripts");
        fs::write(skill_root.join("SKILL.md"), b"---\nname: demo\n---\n").expect("skill");
        fs::write(skill_root.join("scripts/run.sh"), b"#!/bin/sh\n").expect("script");
        let fingerprint = compute_source_metadata_fingerprint(&skill_root).expect("fingerprint");
        let mut skills = BTreeMap::new();
        skills.insert(
            "skills/demo".to_string(),
            DiscoverySkillSnapshot {
                skill_name: "demo".to_string(),
                install_dir_name: "demo".to_string(),
                relative_path: "skills/demo".to_string(),
                plugin_name: Some("examples".to_string()),
                source_metadata_fingerprint: fingerprint,
            },
        );
        let manager = Arc::new(PayloadSessionManager::new(
            Arc::new(InMemoryPayloadSessionStorage::default()),
            PayloadSessionLimits {
                ttl_ms: 60_000,
                max_sessions: 4,
                max_bytes: 1024 * 1024,
            },
            || 1_000,
        ));
        let discovery = manager
            .discover_with_source(
                EnvironmentRef::Native,
                "source-fingerprint",
                Arc::new(InMemoryPayloadSessionStorage::default()),
                RetainedDiscoverySource::new(
                    DiscoverySourceLocation::Native {
                        root: source.path().to_path_buf(),
                        ref_revision: None,
                    },
                    DiscoverySourceDescriptor {
                        source: "owner/repo".to_string(),
                        source_type: "github".to_string(),
                        source_url: Some("https://github.com/owner/repo.git".to_string()),
                        ref_name: Some("main".to_string()),
                        redirected_download_host: None,
                    },
                    skills,
                    source,
                ),
            )
            .await
            .expect("discovery");
        let service = SelectedPayloadAcquisitionService::new(manager.clone());
        let request = AcquireSelectedPayloadsRequest {
            discovery_session: discovery,
            skill_paths: vec!["skills/demo".to_string()],
        };

        let first = service.acquire(request.clone()).await.expect("acquire");
        let second = service.acquire(request).await.expect("reacquire");

        assert_eq!(first[0].session_id, second[0].session_id);
        assert_eq!(first[0].payload_id, second[0].payload_id);
        assert_eq!(first.len(), 1);
        let lease = manager.pin_verified(&first[0]).await.expect("pin");
        let metadata = lease.planning_metadata();
        assert_eq!(metadata.skill_name, "demo");
        assert_eq!(metadata.install_dir_name, "demo");
        assert_eq!(metadata.source, "owner/repo");
        assert_eq!(metadata.source_type, "github");
        assert_eq!(
            metadata.source_url.as_deref(),
            Some("https://github.com/owner/repo.git")
        );
        assert_eq!(metadata.ref_name.as_deref(), Some("main"));
        assert_eq!(metadata.skill_path, "skills/demo");
        assert_eq!(metadata.plugin_name.as_deref(), Some("examples"));
        assert_eq!(metadata.upstream_revision, None);
        assert_eq!(metadata.computed_hash.len(), 64);
        assert_eq!(lease.manifest().entries.len(), 3);
    }

    #[tokio::test]
    async fn wsl_selected_acquisition_stays_in_the_source_backend() {
        let source = tempdir().expect("source");
        let skill_root = source.path().join("skills/demo");
        fs::create_dir_all(skill_root.join("assets")).expect("assets");
        fs::write(skill_root.join("SKILL.md"), b"skill").expect("skill");
        fs::write(skill_root.join("assets/data.bin"), [0, 159, 146, 150]).expect("asset");
        let fingerprint = compute_source_metadata_fingerprint(&skill_root).expect("fingerprint");
        let mut skills = BTreeMap::new();
        skills.insert(
            "skills/demo".to_string(),
            DiscoverySkillSnapshot {
                skill_name: "demo".to_string(),
                install_dir_name: "demo".to_string(),
                relative_path: "skills/demo".to_string(),
                plugin_name: None,
                source_metadata_fingerprint: fingerprint,
            },
        );
        let default_storage = Arc::new(InMemoryPayloadSessionStorage::default());
        let wsl_storage = Arc::new(SourceAcquiringStorage {
            upstream_revision: Some("a".repeat(40)),
            ..SourceAcquiringStorage::default()
        });
        let manager = Arc::new(PayloadSessionManager::new(
            default_storage,
            PayloadSessionLimits {
                ttl_ms: 60_000,
                max_sessions: 4,
                max_bytes: 1024 * 1024,
            },
            || 2_000,
        ));
        let linux_root = source.path().to_string_lossy().to_string();
        let discovery = manager
            .discover_with_source(
                EnvironmentRef::Wsl {
                    distro_name: "Ubuntu-24.04".to_string(),
                },
                "wsl-source-fingerprint",
                wsl_storage.clone(),
                RetainedDiscoverySource::new(
                    DiscoverySourceLocation::WslNative {
                        distro_name: "Ubuntu-24.04".to_string(),
                        linux_root,
                        ref_revision: None,
                    },
                    DiscoverySourceDescriptor {
                        source: "owner/repo".to_string(),
                        source_type: "github".to_string(),
                        source_url: Some("git@github.com:owner/repo.git".to_string()),
                        ref_name: None,
                        redirected_download_host: None,
                    },
                    skills,
                    source,
                ),
            )
            .await
            .expect("discovery");
        let service = SelectedPayloadAcquisitionService::new(manager.clone());

        let request = AcquireSelectedPayloadsRequest {
            discovery_session: discovery,
            skill_paths: vec!["skills/demo".to_string()],
        };
        let handles = service.acquire(request.clone()).await.expect("acquire");
        let repeated = service.acquire(request.clone()).await.expect("reacquire");

        assert_eq!(wsl_storage.acquisitions.load(Ordering::SeqCst), 1);
        assert_eq!(handles[0].payload_id, repeated[0].payload_id);
        let lease = manager.pin_verified(&handles[0]).await.expect("pin");
        assert_eq!(lease.planning_metadata().computed_hash.len(), 64);
        assert_eq!(
            lease.planning_metadata().upstream_revision.as_deref(),
            Some("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
        );
        assert!(lease
            .manifest()
            .entries
            .iter()
            .any(|entry| entry.relative_path == "assets/data.bin"));

        fs::write(skill_root.join("assets/data.bin"), b"changed").expect("mutate Source");
        assert!(matches!(
            service.acquire(request).await,
            Err(AppError::StalePayload)
        ));
    }

    #[tokio::test]
    async fn wsl_direct_download_bridges_the_materialized_payload_into_wsl_storage() {
        let source = tempdir().expect("download source");
        fs::write(
            source.path().join("SKILL.md"),
            b"---\nname: demo\ndescription: Demo\n---\n",
        )
        .expect("skill");
        let fingerprint = compute_source_metadata_fingerprint(source.path()).expect("fingerprint");
        let storage = Arc::new(SourceAcquiringStorage::default());
        let manager = Arc::new(PayloadSessionManager::new(
            Arc::new(InMemoryPayloadSessionStorage::default()),
            PayloadSessionLimits {
                ttl_ms: 60_000,
                max_sessions: 4,
                max_bytes: 1024 * 1024,
            },
            || 2_500,
        ));
        let discovery = manager
            .discover_with_source(
                EnvironmentRef::Wsl {
                    distro_name: "Ubuntu-24.04".to_string(),
                },
                "wsl-download-fingerprint",
                storage.clone(),
                RetainedDiscoverySource::new(
                    DiscoverySourceLocation::Native {
                        root: source.path().to_path_buf(),
                        ref_revision: None,
                    },
                    DiscoverySourceDescriptor {
                        source: "https://example.com/SKILL.md".to_string(),
                        source_type: "download".to_string(),
                        source_url: Some("https://example.com/SKILL.md".to_string()),
                        ref_name: None,
                        redirected_download_host: None,
                    },
                    BTreeMap::from([(
                        "SKILL.md".to_string(),
                        DiscoverySkillSnapshot {
                            skill_name: "demo".to_string(),
                            install_dir_name: "demo".to_string(),
                            relative_path: "SKILL.md".to_string(),
                            plugin_name: None,
                            source_metadata_fingerprint: fingerprint,
                        },
                    )]),
                    source,
                ),
            )
            .await
            .expect("discovery");

        let handles = SelectedPayloadAcquisitionService::new(manager.clone())
            .acquire(AcquireSelectedPayloadsRequest {
                discovery_session: discovery,
                skill_paths: vec!["SKILL.md".to_string()],
            })
            .await
            .expect("bridge payload");

        assert_eq!(storage.acquisitions.load(Ordering::SeqCst), 0);
        assert_eq!(storage.stores.load(Ordering::SeqCst), 1);
        let lease = manager.pin_verified(&handles[0]).await.expect("pin");
        assert_eq!(lease.planning_metadata().source_type, "download");
        assert_eq!(lease.planning_metadata().upstream_revision, None);
    }

    #[tokio::test]
    async fn native_root_skill_discovery_acquires_complete_directory_payload() {
        let source = tempdir().expect("source");
        let skill_root = source.path();
        fs::create_dir_all(skill_root.join("scripts")).expect("scripts");
        fs::create_dir_all(skill_root.join("assets")).expect("assets");
        fs::write(
            skill_root.join("SKILL.md"),
            b"---\nname: demo\ndescription: Demo skill\n---\n",
        )
        .expect("skill");
        fs::write(skill_root.join("scripts/run.sh"), b"#!/bin/sh\necho demo\n").expect("script");
        fs::write(skill_root.join("assets/data.bin"), [0, 159, 146, 150]).expect("asset");
        let manager = Arc::new(PayloadSessionManager::new(
            Arc::new(InMemoryPayloadSessionStorage::default()),
            PayloadSessionLimits {
                ttl_ms: 60_000,
                max_sessions: 4,
                max_bytes: 1024 * 1024,
            },
            || 3_000,
        ));
        let discovery_service = SourceDiscoveryService::with_git_transport(
            manager.clone(),
            Arc::new(crate::application::git_transport::UnavailableGitSourceTransport),
        );

        let fetched = discovery_service
            .discover(
                SkillLocationRef {
                    environment: EnvironmentRef::Native,
                    scope: SkillLocation::Global,
                },
                source.path().to_string_lossy().to_string(),
                |_| {},
            )
            .await
            .expect("discover");

        assert_eq!(fetched.skills.len(), 1);
        assert_eq!(fetched.skills[0].relative_path, "SKILL.md");
        assert_eq!(
            fetched.discovery_session.environment,
            EnvironmentRef::Native
        );
        let selected_path = fetched.skills[0].relative_path.clone();
        let handles = SelectedPayloadAcquisitionService::new(manager.clone())
            .acquire(AcquireSelectedPayloadsRequest {
                discovery_session: fetched.discovery_session,
                skill_paths: vec![selected_path],
            })
            .await
            .expect("selected payload");
        assert_eq!(handles.len(), 1);
        let lease = manager.pin_verified(&handles[0]).await.expect("pin");
        assert_eq!(lease.planning_metadata().upstream_revision, None);
        assert!(lease
            .manifest()
            .entries
            .iter()
            .any(|entry| entry.relative_path == "scripts/run.sh"));
        assert!(lease
            .manifest()
            .entries
            .iter()
            .any(|entry| entry.relative_path == "assets/data.bin"));
    }
}

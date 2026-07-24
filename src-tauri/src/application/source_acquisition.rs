use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use specta::Type;
use walkdir::WalkDir;

use crate::application::git_transport::{GitSourceTransport, ProcessGitTransport};
use crate::application::payload_session::{
    AcquiredPayloadHandle, DiscoverySessionHandle, DiscoverySkillSnapshot,
    DiscoverySourceDescriptor, DiscoverySourceLocation, PayloadPlanningMetadata,
    PayloadSessionManager, PayloadSessionStorage, PayloadStorageKey, RetainedDiscoverySource,
};
use crate::application::source_clone_gate::shared_source_clone_gate;
use crate::core::mutation::CancellationSignal;
use crate::core::plugin_manifest::get_relative_plugin_search_dirs;
use crate::core::skill_paths::normalize_skill_folder_path;
use crate::core::skill_payload::{build_skill_payload, compute_cli_project_hash_from_payload};
use crate::core::wellknown::{extract_hostname, fetch_wellknown_skills, WellKnownTrustMetadata};
use crate::core::{
    compute_local_tree_sha, discover_skills, get_owner_repo, parse_source,
    select_discovered_skills, source_risk_policy, CloneProgress, DiscoverOptions,
    DiscoveryDocument, DiscoveryInventory,
};
use crate::environment::acquisition::{
    acquire_wsl_source_native, WslAcquisitionSource, WslNativeSource,
};
use crate::environment::types::{ContextRef, EnvironmentRef};
use crate::environment::wsl::operations::acquire::WslPayloadSessionStorage;
use crate::environment::wsl::operations::scan::{
    scan, scan_priority_directories, ScanRequest, ScanResponse, ScannedEntryKind,
};
use crate::environment::wsl::{EnvironmentRegistry, WslSession};
use crate::error::AppError;
use crate::models::{AvailableSkill, FetchResult, ParsedSource, SourceType};

const EXCLUDED_SOURCE_FILES: &[&str] = &["metadata.json"];
const EXCLUDED_SOURCE_DIRS: &[&str] = &[".git", "__pycache__", "__pypackages__"];

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AcquireSelectedPayloadsRequest {
    pub discovery_session: DiscoverySessionHandle,
    pub skill_paths: Vec<String>,
}

pub struct SourceDiscoveryService<'a> {
    sessions: Arc<PayloadSessionManager>,
    environments: &'a EnvironmentRegistry,
    git_transport: Arc<dyn GitSourceTransport>,
}

impl<'a> SourceDiscoveryService<'a> {
    pub fn new(
        sessions: Arc<PayloadSessionManager>,
        environments: &'a EnvironmentRegistry,
    ) -> Self {
        Self {
            sessions,
            environments,
            git_transport: Arc::new(ProcessGitTransport),
        }
    }

    pub(crate) fn with_git_transport(
        sessions: Arc<PayloadSessionManager>,
        environments: &'a EnvironmentRegistry,
        git_transport: Arc<dyn GitSourceTransport>,
    ) -> Self {
        Self {
            sessions,
            environments,
            git_transport,
        }
    }

    pub async fn discover<P>(
        &self,
        context: ContextRef,
        requested_source: String,
        on_progress: P,
    ) -> Result<FetchResult, AppError>
    where
        P: Fn(CloneProgress) + Clone + Send + Sync + 'static,
    {
        self.discover_with_cancellation(
            context,
            requested_source,
            on_progress,
            CancellationSignal::default(),
        )
        .await
    }

    pub async fn discover_with_cancellation<P>(
        &self,
        context: ContextRef,
        requested_source: String,
        on_progress: P,
        cancellation: CancellationSignal,
    ) -> Result<FetchResult, AppError>
    where
        P: Fn(CloneProgress) + Clone + Send + Sync + 'static,
    {
        let parsed = parse_source(&requested_source)?;
        self.discover_parsed_with_cancellation(
            context,
            parsed,
            requested_source,
            on_progress,
            cancellation,
        )
        .await
    }

    pub(crate) async fn discover_parsed_with_cancellation<P>(
        &self,
        context: ContextRef,
        parsed: ParsedSource,
        requested_source: String,
        on_progress: P,
        cancellation: CancellationSignal,
    ) -> Result<FetchResult, AppError>
    where
        P: Fn(CloneProgress) + Clone + Send + Sync + 'static,
    {
        match (&context.environment, parsed.source_type.clone()) {
            (EnvironmentRef::Host, SourceType::Local) => {
                let root = parsed
                    .local_path
                    .clone()
                    .ok_or_else(|| invalid_source("Missing local path"))?;
                retain_discovered_source(
                    self.sessions.clone(),
                    context.environment,
                    parsed,
                    requested_source,
                    DiscoverySourceLocation::Native { root: root.clone() },
                    root,
                    (),
                    None,
                    None,
                )
                .await
            }
            (EnvironmentRef::Host, SourceType::WellKnown) => {
                let fetched = fetch_wellknown_skills(&parsed.url).await?;
                let root = fetched.repo_path.clone();
                retain_discovered_source(
                    self.sessions.clone(),
                    context.environment,
                    parsed,
                    requested_source,
                    DiscoverySourceLocation::Native { root: root.clone() },
                    root.clone(),
                    ManagedDownloadedDirectory::new(root),
                    None,
                    Some(fetched.trust_metadata),
                )
                .await
            }
            (EnvironmentRef::Host, _) => {
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
                retain_discovered_source(
                    self.sessions.clone(),
                    context.environment,
                    parsed,
                    requested_source,
                    DiscoverySourceLocation::Native { root: root.clone() },
                    root,
                    cloned,
                    None,
                    None,
                )
                .await
            }
            (EnvironmentRef::Wsl { distro_name }, SourceType::WellKnown) => {
                let fetched = fetch_wellknown_skills(&parsed.url).await?;
                let root = fetched.repo_path.clone();
                let owner = ManagedDownloadedDirectory::new(root.clone());
                let sessions = self.sessions.clone();
                let environment = context.environment.clone();
                let requested_source = requested_source.clone();
                self.environments
                    .with_session_retry(distro_name, move |session| {
                        let sessions = sessions.clone();
                        let parsed = parsed.clone();
                        let requested_source = requested_source.clone();
                        let root = root.clone();
                        let environment = environment.clone();
                        let storage = Arc::new(WslPayloadSessionStorage::new(session));
                        let owner = owner.clone();
                        let trust_metadata = fetched.trust_metadata.clone();
                        async move {
                            retain_discovered_source(
                                sessions,
                                environment,
                                parsed,
                                requested_source,
                                DiscoverySourceLocation::Native { root: root.clone() },
                                root,
                                owner,
                                Some(storage),
                                Some(trust_metadata),
                            )
                            .await
                        }
                    })
                    .await
            }
            (EnvironmentRef::Wsl { distro_name }, _) => {
                let acquisition = wsl_acquisition_source(&parsed)?;
                let sessions = self.sessions.clone();
                let environment = context.environment.clone();
                let requested_source = requested_source.clone();
                self.environments
                    .with_session_retry(distro_name, move |session| {
                        let sessions = sessions.clone();
                        let parsed = parsed.clone();
                        let requested_source = requested_source.clone();
                        let acquisition = acquisition.clone();
                        let environment = environment.clone();
                        let cancellation = cancellation.clone();
                        async move {
                            let _clone_permit = match &acquisition {
                                WslAcquisitionSource::Git { .. } => {
                                    Some(shared_source_clone_gate().acquire(&cancellation).await?)
                                }
                                WslAcquisitionSource::Local { .. } => None,
                            };
                            let native =
                                acquire_wsl_source_native(&session, acquisition, cancellation)
                                    .await?;
                            retain_native_wsl_source(
                                sessions,
                                environment,
                                parsed,
                                requested_source,
                                session,
                                native,
                            )
                            .await
                        }
                    })
                    .await
            }
        }
    }
}

async fn retain_native_wsl_source(
    sessions: Arc<PayloadSessionManager>,
    environment: EnvironmentRef,
    parsed: ParsedSource,
    requested_source: String,
    session: WslSession,
    native: WslNativeSource,
) -> Result<FetchResult, AppError> {
    let root = native_root_with_subpath(native.native_root(), parsed.subpath.as_deref())?;
    let mut roots = vec![root.clone()];
    let mut stat_only_root_indexes = BTreeSet::new();
    if parsed.subpath.is_some() {
        roots.push(format!(
            "{}/skills-lock.json",
            native.native_root().trim_end_matches('/')
        ));
        stat_only_root_indexes.insert(1);
    }
    let mut response = scan(
        &session,
        ScanRequest {
            roots,
            stat_only_root_indexes,
            recursive: true,
            per_file_limit: 256 * 1024,
            aggregate_limit: 16 * 1024 * 1024,
        },
        Some(CancellationSignal::default()),
    )
    .await?;
    let plugin_search_dirs = get_relative_plugin_search_dirs(
        wsl_document(&response, ".claude-plugin/marketplace.json"),
        wsl_document(&response, ".claude-plugin/plugin.json"),
    );
    if !plugin_search_dirs.is_empty() {
        let priority = scan_priority_directories(
            &session,
            ScanRequest {
                roots: plugin_search_dirs
                    .iter()
                    .map(|relative| {
                        format!(
                            "{}/{}",
                            root.trim_end_matches('/'),
                            relative.to_string_lossy().replace('\\', "/")
                        )
                    })
                    .collect(),
                stat_only_root_indexes: BTreeSet::new(),
                recursive: false,
                per_file_limit: 256 * 1024,
                aggregate_limit: 16 * 1024 * 1024,
            },
            Some(CancellationSignal::default()),
        )
        .await?;
        merge_wsl_priority_documents(&mut response, priority, &plugin_search_dirs);
    }
    let (discovered, mut catalog) = build_wsl_discovery_catalog(
        &response,
        parsed.subpath.as_deref(),
        parsed.skill_filter.is_some(),
    )?;
    let storage = Arc::new(WslPayloadSessionStorage::new(session.clone()));
    for skill in catalog.values_mut() {
        let source_root = format!(
            "{}/{}",
            native.native_root().trim_end_matches('/'),
            normalize_skill_folder_path(&skill.relative_path)
        );
        skill.source_metadata_fingerprint =
            storage.source_metadata_fingerprint(&source_root).await?;
    }
    let descriptor = DiscoverySourceDescriptor {
        source: source_identifier(&parsed, &requested_source),
        source_type: parsed.source_type.to_string(),
        source_url: (!parsed.url.is_empty()).then(|| parsed.url.clone()),
        ref_name: parsed.git_ref.clone(),
    };
    let source_fingerprint = snapshot_fingerprint(&descriptor, &catalog);
    let risk_policy = source_risk_policy(&parsed);
    let retained = RetainedDiscoverySource::new(
        DiscoverySourceLocation::WslNative {
            distro_name: session.distro_name.clone(),
            linux_root: native.native_root().to_string(),
            ref_revision: native.ref_revision().map(str::to_string),
        },
        descriptor,
        catalog,
        native,
    );
    let storage: Arc<dyn PayloadSessionStorage> = storage;
    let discovery_session = sessions
        .discover_with_source(environment, source_fingerprint, storage, retained)
        .await?;
    Ok(FetchResult {
        discovery_session,
        source_type: parsed.source_type.to_string(),
        source_url: parsed.url,
        git_ref: parsed.git_ref,
        skill_filter: parsed.skill_filter,
        risk_policy,
        skills: discovered.into_iter().map(AvailableSkill::from).collect(),
    })
}

fn wsl_document<'a>(response: &'a ScanResponse, relative_path: &str) -> Option<&'a str> {
    response
        .entries
        .iter()
        .find(|entry| {
            entry.root_index == 0
                && matches!(
                    entry.kind,
                    ScannedEntryKind::File | ScannedEntryKind::Symlink
                )
                && entry.relative_path == relative_path
                && !entry.truncated
        })
        .and_then(|entry| std::str::from_utf8(&entry.content_bytes).ok())
}

fn merge_wsl_priority_documents(
    inventory: &mut ScanResponse,
    priority: ScanResponse,
    relative_roots: &[PathBuf],
) {
    for mut entry in priority.entries {
        if entry.relative_path.is_empty() {
            continue;
        }
        let Some(prefix) = relative_roots.get(entry.root_index as usize) else {
            continue;
        };
        entry.relative_path = prefix
            .join(&entry.relative_path)
            .to_string_lossy()
            .replace('\\', "/");
        entry.root_index = 0;
        if !inventory.entries.iter().any(|existing| {
            existing.root_index == 0 && existing.relative_path == entry.relative_path
        }) {
            inventory.total_content_bytes = inventory
                .total_content_bytes
                .saturating_add(entry.content_bytes.len() as u32);
            inventory.entries.push(entry);
        }
    }
}

fn native_root_with_subpath(root: &str, subpath: Option<&str>) -> Result<String, AppError> {
    let Some(subpath) = subpath else {
        return Ok(root.to_string());
    };
    let path = std::path::Path::new(subpath);
    if path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                std::path::Component::ParentDir | std::path::Component::RootDir
            )
        })
    {
        return Err(AppError::UnsafePath {
            path: subpath.to_string(),
            reason: "WSL source subpath escapes its native root".to_string(),
        });
    }
    Ok(format!("{}/{}", root.trim_end_matches('/'), subpath))
}

fn build_wsl_discovery_catalog(
    response: &crate::environment::wsl::operations::scan::ScanResponse,
    subpath: Option<&str>,
    include_internal: bool,
) -> Result<
    (
        Vec<crate::core::DiscoveredSkill>,
        BTreeMap<String, DiscoverySkillSnapshot>,
    ),
    AppError,
> {
    let skill_documents = response
        .entries
        .iter()
        .filter(|entry| {
            entry.root_index == 0
                && matches!(
                    entry.kind,
                    ScannedEntryKind::File | ScannedEntryKind::Symlink
                )
                && entry
                    .relative_path
                    .rsplit('/')
                    .next()
                    .is_some_and(|name| name.eq_ignore_ascii_case("SKILL.md"))
                && !entry.truncated
                && entry.error_code.is_none()
        })
        .map(|entry| DiscoveryDocument {
            relative_path: entry.relative_path.clone(),
            content: entry.content_bytes.clone(),
        })
        .collect();
    let local_lock_document = response
        .entries
        .iter()
        .find(|entry| {
            matches!(
                entry.kind,
                ScannedEntryKind::File | ScannedEntryKind::Symlink
            ) && !entry.truncated
                && ((entry.root_index == 0 && entry.relative_path == "skills-lock.json")
                    || (entry.root_index == 1 && entry.relative_path.is_empty()))
        })
        .and_then(|entry| std::str::from_utf8(&entry.content_bytes).ok())
        .map(str::to_owned);
    let inventory = DiscoveryInventory {
        search_prefix: subpath.map(PathBuf::from).unwrap_or_default(),
        skill_documents,
        marketplace_document: wsl_document(response, ".claude-plugin/marketplace.json")
            .map(str::to_owned),
        plugin_document: wsl_document(response, ".claude-plugin/plugin.json").map(str::to_owned),
        local_lock_document,
    };
    let discovered = select_discovered_skills(
        &inventory,
        DiscoverOptions {
            include_internal,
            full_depth: false,
        },
    )?;
    let mut catalog = BTreeMap::new();
    for skill in &discovered {
        catalog.insert(
            skill.relative_path.clone(),
            DiscoverySkillSnapshot {
                skill_name: skill.name.clone(),
                install_dir_name: skill.install_dir_name.clone(),
                relative_path: skill.relative_path.clone(),
                plugin_name: skill.plugin_name.clone(),
                source_metadata_fingerprint: String::new(),
            },
        );
    }
    Ok((discovered, catalog))
}

#[allow(clippy::too_many_arguments)]
async fn retain_discovered_source<O>(
    sessions: Arc<PayloadSessionManager>,
    environment: EnvironmentRef,
    parsed: ParsedSource,
    requested_source: String,
    location: DiscoverySourceLocation,
    host_root: PathBuf,
    owner: O,
    storage: Option<Arc<dyn PayloadSessionStorage>>,
    trust_metadata: Option<std::collections::HashMap<String, WellKnownTrustMetadata>>,
) -> Result<FetchResult, AppError>
where
    O: Send + Sync + 'static,
{
    let scan_root = host_root.clone();
    let scan_subpath = parsed.subpath.clone();
    let scan_include_internal = parsed.skill_filter.is_some();
    let (discovered, catalog) = tokio::task::spawn_blocking(move || {
        build_discovery_catalog(&scan_root, scan_subpath.as_deref(), scan_include_internal)
    })
    .await
    .map_err(|error| AppError::ExecutionFailed {
        message: format!("native source discovery task failed: {error}"),
    })??;
    let descriptor = DiscoverySourceDescriptor {
        source: source_identifier(&parsed, &requested_source),
        source_type: parsed.source_type.to_string(),
        source_url: (!parsed.url.is_empty()).then(|| parsed.url.clone()),
        ref_name: parsed.git_ref.clone(),
    };
    let source_fingerprint = snapshot_fingerprint(&descriptor, &catalog);
    let retained = RetainedDiscoverySource::new(location, descriptor, catalog, owner);
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
        git_ref: parsed.git_ref.clone(),
        skill_filter: parsed.skill_filter.clone(),
        risk_policy: source_risk_policy(&parsed),
        skills,
    })
}

fn build_discovery_catalog(
    host_root: &Path,
    subpath: Option<&str>,
    include_internal: bool,
) -> Result<
    (
        Vec<crate::core::DiscoveredSkill>,
        std::collections::BTreeMap<String, DiscoverySkillSnapshot>,
    ),
    AppError,
> {
    let discovered = discover_skills(
        host_root,
        subpath,
        DiscoverOptions {
            include_internal,
            full_depth: false,
        },
    )?;
    let physical_root = fs::canonicalize(host_root)?;
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

fn wsl_acquisition_source(parsed: &ParsedSource) -> Result<WslAcquisitionSource, AppError> {
    match parsed.source_type {
        SourceType::Local => Ok(WslAcquisitionSource::Local {
            native_path: parsed
                .local_path
                .as_ref()
                .ok_or_else(|| invalid_source("Missing local path"))?
                .to_string_lossy()
                .to_string(),
        }),
        SourceType::GitHub | SourceType::GitLab | SourceType::Git => {
            Ok(WslAcquisitionSource::Git {
                url: parsed.url.clone(),
                git_ref: parsed.git_ref.clone(),
            })
        }
        SourceType::WellKnown => Err(invalid_source("Well-known sources use Host HTTP")),
    }
}

fn source_identifier(parsed: &ParsedSource, requested_source: &str) -> String {
    if parsed.source_type == SourceType::WellKnown {
        return extract_hostname(&parsed.url).unwrap_or_else(|| requested_source.to_string());
    }
    if parsed.url.starts_with("git@") || parsed.url.starts_with("ssh://") {
        return parsed.url.clone();
    }
    get_owner_repo(parsed).unwrap_or_else(|| requested_source.to_string())
}

fn snapshot_fingerprint(
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

fn invalid_source(message: &str) -> AppError {
    AppError::InvalidSource {
        value: message.to_string(),
    }
}

#[derive(Clone)]
struct ManagedDownloadedDirectory {
    _owner: Arc<ManagedDownloadedDirectoryOwner>,
}

impl ManagedDownloadedDirectory {
    fn new(path: PathBuf) -> Self {
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
            DiscoverySourceLocation::Native { root } => {
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
                    crate::environment::types::EnvironmentRef::Host => {
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
) -> PayloadPlanningMetadata {
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

    use tempfile::tempdir;

    use super::*;
    use crate::application::payload_session::{
        BackendAcquiredPayload, DiscoverySkillSnapshot, DiscoverySourceDescriptor,
        DiscoverySourceLocation, InMemoryPayloadSessionStorage, PayloadSessionLimits,
        PayloadSessionManager, PayloadSessionStorage, PayloadStorageFuture, PayloadStorageKey,
        RetainedDiscoverySource,
    };
    use crate::core::mutation::CancellationSignal;
    use crate::core::skill_payload::{build_skill_payload, SkillPayload};
    use crate::environment::types::EnvironmentRef;
    use crate::environment::types::{ContextRef, ContextScope};
    use crate::environment::wsl::EnvironmentRegistry;
    use crate::models::SourceType;

    #[derive(Default)]
    struct SourceAcquiringStorage {
        payloads: Mutex<HashMap<PayloadStorageKey, SkillPayload>>,
        acquisitions: AtomicUsize,
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
    fn source_environment_routing_keeps_well_known_on_host_and_git_local_in_wsl() {
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
        );

        assert_eq!(metadata.computed_hash, "cli-computed-hash");
        assert_eq!(metadata.upstream_revision, None);
        assert_eq!(metadata.global_skill_folder_hash(), "cli-computed-hash");
    }

    #[test]
    fn wsl_discovery_matches_host_paths_internal_filter_and_plugin_metadata() {
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
            build_wsl_discovery_catalog(&response, None, false).expect("catalog");

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

        let (discovered, _) = build_wsl_discovery_catalog(&response, None, false).expect("catalog");

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

        let (discovered, _) = build_wsl_discovery_catalog(&response, None, false).expect("catalog");

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

        let (discovered, _) = build_wsl_discovery_catalog(&response, None, false).expect("catalog");

        assert_eq!(
            discovered
                .iter()
                .map(|skill| skill.name.as_str())
                .collect::<Vec<_>>(),
            vec!["root"]
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
            build_wsl_discovery_catalog(&response, Some(".agents/skills/demo"), false)
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
                EnvironmentRef::Host,
                "source-fingerprint",
                Arc::new(InMemoryPayloadSessionStorage::default()),
                RetainedDiscoverySource::new(
                    DiscoverySourceLocation::Native {
                        root: source.path().to_path_buf(),
                    },
                    DiscoverySourceDescriptor {
                        source: "owner/repo".to_string(),
                        source_type: "github".to_string(),
                        source_url: Some("https://github.com/owner/repo.git".to_string()),
                        ref_name: Some("main".to_string()),
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
    async fn host_local_discovery_and_selected_acquisition_share_one_source_snapshot() {
        let source = tempdir().expect("source");
        let skill_root = source.path().join("skills/demo");
        fs::create_dir_all(&skill_root).expect("skill root");
        fs::write(
            skill_root.join("SKILL.md"),
            b"---\nname: demo\ndescription: Demo skill\n---\n",
        )
        .expect("skill");
        let manager = Arc::new(PayloadSessionManager::new(
            Arc::new(InMemoryPayloadSessionStorage::default()),
            PayloadSessionLimits {
                ttl_ms: 60_000,
                max_sessions: 4,
                max_bytes: 1024 * 1024,
            },
            || 3_000,
        ));
        let registry = EnvironmentRegistry::default();
        let discovery_service = SourceDiscoveryService::new(manager.clone(), &registry);

        let fetched = discovery_service
            .discover(
                ContextRef {
                    environment: EnvironmentRef::Host,
                    scope: ContextScope::Global,
                },
                source.path().to_string_lossy().to_string(),
                |_| {},
            )
            .await
            .expect("discover");

        assert_eq!(fetched.skills.len(), 1);
        assert_eq!(fetched.skills[0].relative_path, "skills/demo/SKILL.md");
        assert_eq!(fetched.discovery_session.environment, EnvironmentRef::Host);
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
    }
}

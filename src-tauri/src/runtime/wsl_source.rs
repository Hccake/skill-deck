use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::application::download_source::materialize_download;
use crate::application::payload_session::{
    DiscoverySkillSnapshot, DiscoverySourceDescriptor, DiscoverySourceLocation,
    PayloadSessionManager, PayloadSessionStorage, RetainedDiscoverySource,
};
use crate::application::source_acquisition::{
    attempt_wellknown_then_download, invalid_source, redirected_host, retain_discovered_source,
    snapshot_fingerprint, source_identifier, InternalSkillVisibility, ManagedDownloadedDirectory,
    RetainedSourceOptions, SourceDiscoveryPolicy,
};
use crate::application::source_clone_gate::shared_source_clone_gate;
use crate::application::wellknown_access::{WellKnownAccess, WellKnownFetchError};
use crate::application::wsl_source_access::{WslSourceAccess, WslSourceFuture};
use crate::core::mutation::CancellationSignal;
use crate::core::plugin_manifest::get_relative_plugin_search_dirs;
use crate::core::skill_paths::normalize_skill_folder_path;
use crate::core::{
    resolve_clone_timeout_secs, select_discovered_skills, DiscoverOptions, DiscoveryDocument,
    DiscoveryInventory,
};
use crate::environment::types::EnvironmentRef;
use crate::environment::wsl::operations::acquire::WslPayloadSessionStorage;
use crate::environment::wsl::operations::scan::{
    scan, scan_priority_directories, ScanRequest, ScanResponse, ScannedEntryKind,
};
use crate::environment::wsl::operations::source_acquisition::{
    acquire_wsl_source_native, WslAcquisitionSource, WslNativeSource,
};
use crate::environment::wsl::{WslRuntime, WslSession, WslWorkspace};
use crate::error::AppError;
use crate::models::{AvailableSkill, FetchResult, ParsedSource, SourceType};
use crate::runtime::download::RuntimeDownloadAccess;
use crate::runtime::proxy_settings::ProxySettingsStore;

pub(crate) struct RuntimeWslSourceAccess {
    sessions: Arc<PayloadSessionManager>,
    environments: Arc<WslRuntime>,
    settings: Arc<ProxySettingsStore>,
    wellknown: Arc<dyn WellKnownAccess>,
    download: RuntimeDownloadAccess,
}

impl RuntimeWslSourceAccess {
    pub(crate) fn new(
        sessions: Arc<PayloadSessionManager>,
        environments: Arc<WslRuntime>,
        settings: Arc<ProxySettingsStore>,
        wellknown: Arc<dyn WellKnownAccess>,
        download: RuntimeDownloadAccess,
    ) -> Self {
        Self {
            sessions,
            environments,
            settings,
            wellknown,
            download,
        }
    }
}

impl WslSourceAccess for RuntimeWslSourceAccess {
    fn discover<'a>(
        &'a self,
        distro_name: &'a str,
        parsed: ParsedSource,
        requested_source: String,
        policy: SourceDiscoveryPolicy,
        cancellation: CancellationSignal,
    ) -> WslSourceFuture<'a> {
        Box::pin(async move {
            match parsed.source_type {
                SourceType::WellKnown => {
                    attempt_wellknown_then_download(
                        self.discover_wellknown(
                            distro_name,
                            parsed.clone(),
                            requested_source.clone(),
                            policy.clone(),
                            cancellation.clone(),
                        ),
                        || {
                            self.discover_download(
                                distro_name,
                                parsed,
                                requested_source,
                                policy,
                                cancellation,
                            )
                        },
                        "WSL",
                    )
                    .await
                }
                SourceType::Download => {
                    self.discover_download(
                        distro_name,
                        parsed,
                        requested_source,
                        policy,
                        cancellation,
                    )
                    .await
                }
                _ => {
                    self.discover_native(
                        distro_name,
                        parsed,
                        requested_source,
                        policy,
                        cancellation,
                    )
                    .await
                }
            }
        })
    }
}

impl RuntimeWslSourceAccess {
    async fn discover_wellknown(
        &self,
        distro_name: &str,
        parsed: ParsedSource,
        requested_source: String,
        policy: SourceDiscoveryPolicy,
        cancellation: CancellationSignal,
    ) -> Result<FetchResult, WellKnownFetchError> {
        let workspace = self
            .environments
            .workspace(distro_name)
            .map_err(WellKnownFetchError::unproven)?;
        let fetched = self.wellknown.fetch(&parsed.url, &cancellation).await?;
        let root = fetched.repo_path.clone();
        let owner = ManagedDownloadedDirectory::new(root.clone());
        let environment = EnvironmentRef::Wsl {
            distro_name: distro_name.to_string(),
        };
        let sessions = self.sessions.clone();
        workspace
            .clone()
            .with_access(move || {
                let workspace = workspace.clone();
                async move {
                    let storage = Arc::new(WslPayloadSessionStorage::new(workspace));
                    retain_discovered_source(
                        sessions,
                        environment,
                        parsed,
                        requested_source,
                        DiscoverySourceLocation::Native {
                            root: root.clone(),
                            ref_revision: None,
                        },
                        root,
                        owner,
                        RetainedSourceOptions {
                            storage: Some(storage),
                            trust_metadata: Some(fetched.trust_metadata),
                            full_depth: policy.full_depth,
                            internal_skill_visibility: policy.internal_skill_visibility,
                            ..Default::default()
                        },
                    )
                    .await
                }
            })
            .await
            .map_err(WellKnownFetchError::catalog_established)
    }

    async fn discover_download(
        &self,
        distro_name: &str,
        mut parsed: ParsedSource,
        requested_source: String,
        policy: SourceDiscoveryPolicy,
        cancellation: CancellationSignal,
    ) -> Result<FetchResult, AppError> {
        let workspace = self.environments.workspace(distro_name)?;
        let fetched = self.download.fetch(&parsed.url, &cancellation).await?;
        let materialized = materialize_download(&fetched.bytes)?;
        let root = materialized.keep();
        let owner = ManagedDownloadedDirectory::new(root.clone());
        let redirected_download_host = redirected_host(&parsed.url, &fetched.final_url);
        parsed.source_type = SourceType::Download;
        let environment = EnvironmentRef::Wsl {
            distro_name: distro_name.to_string(),
        };
        let sessions = self.sessions.clone();
        workspace
            .clone()
            .with_access(move || {
                let workspace = workspace.clone();
                async move {
                    let storage = Arc::new(WslPayloadSessionStorage::new(workspace));
                    retain_discovered_source(
                        sessions,
                        environment,
                        parsed,
                        requested_source,
                        DiscoverySourceLocation::Native {
                            root: root.clone(),
                            ref_revision: None,
                        },
                        root,
                        owner,
                        RetainedSourceOptions {
                            storage: Some(storage),
                            redirected_download_host,
                            full_depth: policy.full_depth,
                            internal_skill_visibility: policy.internal_skill_visibility,
                            ..Default::default()
                        },
                    )
                    .await
                }
            })
            .await
    }

    async fn discover_native(
        &self,
        distro_name: &str,
        parsed: ParsedSource,
        requested_source: String,
        policy: SourceDiscoveryPolicy,
        cancellation: CancellationSignal,
    ) -> Result<FetchResult, AppError> {
        let workspace = self.environments.workspace(distro_name)?;
        let acquisition = wsl_acquisition_source(&parsed)?;
        let environment = EnvironmentRef::Wsl {
            distro_name: distro_name.to_string(),
        };
        let sessions = self.sessions.clone();
        let settings = self.settings.clone();
        let proxy_distro = distro_name.to_string();
        let git_timeout = Duration::from_secs(resolve_clone_timeout_secs());
        self.environments
            .with_session_retry(distro_name, move |session| {
                let workspace = workspace.clone();
                let parsed = parsed.clone();
                let requested_source = requested_source.clone();
                let acquisition = acquisition.clone();
                let cancellation = cancellation.clone();
                let sessions = sessions.clone();
                let environment = environment.clone();
                let settings = settings.clone();
                let policy = policy.clone();
                let proxy_distro = proxy_distro.clone();
                async move {
                    let _clone_permit = match &acquisition {
                        WslAcquisitionSource::Git { .. } => {
                            Some(shared_source_clone_gate().acquire(&cancellation).await?)
                        }
                        WslAcquisitionSource::Local { .. } => None,
                    };
                    let proxy = match &acquisition {
                        WslAcquisitionSource::Git { url, .. } => {
                            settings.wsl_git_proxy(&proxy_distro, url)
                        }
                        WslAcquisitionSource::Local { .. } => None,
                    };
                    let started_at = Instant::now();
                    let remaining = match &acquisition {
                        WslAcquisitionSource::Git { .. } => {
                            let remaining = git_timeout.saturating_sub(started_at.elapsed());
                            if remaining.is_zero() {
                                return Err(AppError::GitTimeout {
                                    timeout_secs: u32::try_from(git_timeout.as_secs())
                                        .unwrap_or(u32::MAX),
                                });
                            }
                            remaining
                        }
                        WslAcquisitionSource::Local { .. } => git_timeout,
                    };
                    let native = acquire_wsl_source_native(
                        workspace.clone(),
                        &session,
                        acquisition,
                        remaining,
                        proxy,
                        cancellation.clone(),
                    )
                    .await?;
                    let prepared = prepare_native_wsl_source(
                        parsed,
                        requested_source,
                        session,
                        workspace,
                        native,
                        policy,
                        cancellation,
                    )
                    .await?;
                    let discovery_session = sessions
                        .discover_with_source(
                            environment,
                            prepared.source_fingerprint,
                            prepared.storage,
                            prepared.retained,
                        )
                        .await?;
                    Ok(FetchResult {
                        discovery_session,
                        source_type: prepared.source_type,
                        source_url: prepared.source_url,
                        redirected_download_host: None,
                        git_ref: prepared.git_ref,
                        skill_filter: prepared.skill_filter,
                        skills: prepared.skills,
                    })
                }
            })
            .await
    }
}

struct PreparedWslDiscovery {
    source_fingerprint: String,
    storage: Arc<dyn PayloadSessionStorage>,
    retained: RetainedDiscoverySource,
    source_type: String,
    source_url: String,
    git_ref: Option<String>,
    skill_filter: Option<String>,
    skills: Vec<AvailableSkill>,
}

async fn prepare_native_wsl_source(
    parsed: ParsedSource,
    requested_source: String,
    session: WslSession,
    workspace: WslWorkspace,
    native: WslNativeSource,
    policy: SourceDiscoveryPolicy,
    cancellation: CancellationSignal,
) -> Result<PreparedWslDiscovery, AppError> {
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
        Some(cancellation.clone()),
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
            Some(cancellation),
        )
        .await?;
        merge_wsl_priority_documents(&mut response, priority, &plugin_search_dirs);
    }
    let (discovered, mut catalog) = build_wsl_discovery_catalog(
        &response,
        parsed.subpath.as_deref(),
        &policy.internal_skill_visibility,
        policy.full_depth,
    )?;
    let storage = Arc::new(WslPayloadSessionStorage::new(workspace));
    for skill in catalog.values_mut() {
        let source_root = format!(
            "{}/{}",
            native.native_root().trim_end_matches('/'),
            normalize_skill_folder_path(&skill.relative_path)
        );
        skill.source_metadata_fingerprint = storage
            .source_metadata_fingerprint_in_active_session(&session, &source_root)
            .await?;
    }
    let descriptor = DiscoverySourceDescriptor {
        source: source_identifier(&parsed, &requested_source),
        source_type: parsed.source_type.to_string(),
        source_url: (!parsed.url.is_empty()).then(|| parsed.url.clone()),
        ref_name: parsed.git_ref.clone(),
        redirected_download_host: None,
    };
    let source_fingerprint = snapshot_fingerprint(&descriptor, &catalog);
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
    Ok(PreparedWslDiscovery {
        source_fingerprint,
        storage,
        retained,
        source_type: parsed.source_type.to_string(),
        source_url: parsed.url,
        git_ref: parsed.git_ref,
        skill_filter: parsed.skill_filter,
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

pub(crate) fn build_wsl_discovery_catalog(
    response: &ScanResponse,
    subpath: Option<&str>,
    internal_skill_visibility: &InternalSkillVisibility,
    full_depth: bool,
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
            include_internal: internal_skill_visibility.scans_internal(),
            full_depth,
        },
    )?
    .into_iter()
    .filter(|skill| internal_skill_visibility.allows(&skill.name, skill.internal))
    .collect::<Vec<_>>();
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

pub(crate) fn wsl_acquisition_source(
    parsed: &ParsedSource,
) -> Result<WslAcquisitionSource, AppError> {
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
        SourceType::WellKnown | SourceType::Download => {
            Err(invalid_source("HTTP sources use Native HTTP"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wsl_proxy_selection_is_isolated_by_distro_and_transport() {
        let policy = ProxySettingsStore::new(crate::models::NetworkProxySettings {
            wsl_git: [
                (
                    "Ubuntu".to_string(),
                    crate::models::WslGitProxySettings::UseProxy {
                        proxy_url: "http://ubuntu.proxy:7890".to_string(),
                        scope: crate::models::GitProxyScope::AllHttpHttps,
                    },
                ),
                (
                    "Debian".to_string(),
                    crate::models::WslGitProxySettings::UseProxy {
                        proxy_url: "https://debian.proxy:7890".to_string(),
                        scope: crate::models::GitProxyScope::AllHttpHttps,
                    },
                ),
            ]
            .into_iter()
            .collect(),
            ..crate::models::NetworkProxySettings::default()
        });

        assert_eq!(
            policy.wsl_git_proxy("Ubuntu", "https://github.com/owner/repo.git"),
            Some("http://ubuntu.proxy:7890".to_string())
        );
        assert_eq!(
            policy.wsl_git_proxy("Debian", "http://github.com/owner/repo.git"),
            Some("https://debian.proxy:7890".to_string())
        );
        assert_eq!(
            policy.wsl_git_proxy("Ubuntu", "git@github.com:owner/repo.git"),
            None
        );
        assert_eq!(
            policy.wsl_git_proxy("Fedora", "https://github.com/owner/repo.git"),
            None
        );
    }
}

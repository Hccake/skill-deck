use std::future::Future;
use std::path::{Component, Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;

use crate::core::mutation::CancellationSignal;
use crate::environment::content_manifest::{
    ContentManifest, ContentManifestReader, ContentManifestTarget,
};
use crate::environment::native::content_manifest::NativeContentManifestReader;
use crate::environment::native::tree::{inspect_entry_no_follow, project_target, NativeEntryKind};
use crate::environment::path_mapping::{windows_storage_owner, WindowsStorageOwner};
use crate::environment::runtime::{
    projected_physical_target_key, EntryFingerprint, ExecutionBackend, PhysicalParentIdentity,
    PhysicalTargetKey,
};
use crate::environment::types::{
    normalized_wsl_distro_name, same_environment_identity, EnvironmentRef, ResourceLocator,
    SkillLocationRef, StorageAccess,
};
use crate::environment::wsl::operations::content_manifest as wsl_content_manifest;
use crate::environment::wsl::operations::entry::inspect_entries;
use crate::environment::wsl::operations::projection::{project_targets, ProjectedPosixTarget};
use crate::environment::wsl::{WslRuntime, WslSession};
use crate::error::AppError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedTargetFact {
    pub key: PhysicalTargetKey,
    pub destination: ResourceLocator,
    pub storage_access: StorageAccess,
    pub fingerprint: EntryFingerprint,
    pub entry_kind: TargetEntryKind,
    pub link_target: Option<String>,
    pub link_target_identity: Option<ResolvedLinkTargetIdentity>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetEntryKind {
    Missing,
    File,
    Directory,
    Symlink,
    Junction,
    BrokenLink,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedLinkTargetIdentity {
    environment: EnvironmentRef,
    comparison_path: String,
}

impl ResolvedLinkTargetIdentity {
    pub(crate) fn matches(&self, target: &ResourceLocator) -> bool {
        same_environment_identity(&self.environment, &target.environment)
            && normalized_comparison_path(&target.environment, &target.native_path)
                .is_some_and(|candidate| candidate == self.comparison_path)
    }
}

pub(crate) fn resolve_link_target_identity(
    destination: &ResourceLocator,
    raw_target: &str,
) -> Option<ResolvedLinkTargetIdentity> {
    let native_path = match &destination.environment {
        EnvironmentRef::Native => {
            let raw = Path::new(raw_target);
            let joined = if raw.is_absolute() {
                raw.to_path_buf()
            } else {
                Path::new(&destination.native_path).parent()?.join(raw)
            };
            lexical_normalize_native(&joined)
                .to_string_lossy()
                .into_owned()
        }
        EnvironmentRef::Wsl { .. } => {
            let joined = if raw_target.starts_with('/') {
                raw_target.to_string()
            } else {
                let parent = destination.native_path.rsplit_once('/')?.0;
                format!("{parent}/{raw_target}")
            };
            lexical_normalize_posix(&joined)
        }
    };
    let comparison_path = normalized_comparison_path(&destination.environment, &native_path)?;
    Some(ResolvedLinkTargetIdentity {
        environment: destination.environment.clone(),
        comparison_path,
    })
}

fn lexical_normalize_native(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            other => normalized.push(other.as_os_str()),
        }
    }
    normalized
}

fn lexical_normalize_posix(path: &str) -> String {
    let absolute = path.starts_with('/');
    let mut components = Vec::new();
    for component in path.split('/') {
        match component {
            "" | "." => {}
            ".." => {
                if components
                    .last()
                    .is_some_and(|component| *component != "..")
                {
                    components.pop();
                } else if !absolute {
                    components.push("..");
                }
            }
            value => components.push(value),
        }
    }
    let normalized = components.join("/");
    if absolute {
        format!("/{normalized}")
    } else {
        normalized
    }
}

fn normalized_comparison_path(environment: &EnvironmentRef, path: &str) -> Option<String> {
    match environment {
        EnvironmentRef::Native => {
            let normalized = lexical_normalize_native(Path::new(path));
            Some(native_comparison_path(&normalized))
        }
        EnvironmentRef::Wsl { .. } => Some(lexical_normalize_posix(path)),
    }
}

#[cfg(not(windows))]
fn native_comparison_path(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

#[cfg(windows)]
fn native_comparison_path(path: &Path) -> String {
    let normalized = path.to_string_lossy().replace('/', "\\");
    let without_verbatim = normalized
        .strip_prefix(r"\\?\UNC\")
        .map(|suffix| format!(r"\\{suffix}"))
        .or_else(|| {
            normalized
                .strip_prefix(r"\\?\")
                .filter(|suffix| suffix.as_bytes().get(1) == Some(&b':'))
                .map(str::to_string)
        })
        .unwrap_or(normalized);
    without_verbatim.to_lowercase()
}

pub type TargetFactFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

pub trait TargetFactResolver: Send + Sync {
    fn resolve<'a>(
        &'a self,
        context: &'a SkillLocationRef,
        logical_destinations: &'a [ResourceLocator],
        cancellation: Option<CancellationSignal>,
    ) -> TargetFactFuture<'a, Result<Vec<ResolvedTargetFact>, AppError>>;

    fn resolve_environment<'a>(
        &'a self,
        environment: &'a EnvironmentRef,
        logical_destinations: &'a [ResourceLocator],
        cancellation: Option<CancellationSignal>,
    ) -> TargetFactFuture<'a, Result<Vec<ResolvedTargetFact>, AppError>> {
        Box::pin(async move {
            let context = SkillLocationRef {
                environment: environment.clone(),
                scope: crate::environment::types::SkillLocation::Global,
            };
            self.resolve(&context, logical_destinations, cancellation)
                .await
        })
    }
}

#[derive(Clone)]
pub struct RuntimeTargetFactResolver {
    environments: Arc<WslRuntime>,
}

impl RuntimeTargetFactResolver {
    pub fn new(environments: Arc<WslRuntime>) -> Self {
        Self { environments }
    }
}

impl TargetFactResolver for RuntimeTargetFactResolver {
    fn resolve<'a>(
        &'a self,
        context: &'a SkillLocationRef,
        logical_destinations: &'a [ResourceLocator],
        cancellation: Option<CancellationSignal>,
    ) -> TargetFactFuture<'a, Result<Vec<ResolvedTargetFact>, AppError>> {
        self.resolve_environment(&context.environment, logical_destinations, cancellation)
    }

    fn resolve_environment<'a>(
        &'a self,
        environment: &'a EnvironmentRef,
        logical_destinations: &'a [ResourceLocator],
        cancellation: Option<CancellationSignal>,
    ) -> TargetFactFuture<'a, Result<Vec<ResolvedTargetFact>, AppError>> {
        Box::pin(async move {
            validate_owners(environment, logical_destinations)?;
            match environment {
                EnvironmentRef::Native => {
                    let destinations = logical_destinations.to_vec();
                    tokio::task::spawn_blocking(move || resolve_native(&destinations))
                        .await
                        .map_err(|error| AppError::ExecutionFailed {
                            message: format!("native target inspection task failed: {error}"),
                        })?
                }
                EnvironmentRef::Wsl { distro_name } => {
                    let destinations = logical_destinations
                        .iter()
                        .map(|target| target.native_path.clone())
                        .collect::<Vec<_>>();
                    let cancellation_for_retry = cancellation.clone();
                    self.environments
                        .with_session_retry(distro_name, move |session| {
                            let destinations = destinations.clone();
                            let cancellation = cancellation_for_retry.clone();
                            async move { resolve_wsl(&session, &destinations, cancellation).await }
                        })
                        .await
                }
            }
        })
    }
}

impl ContentManifestReader for RuntimeTargetFactResolver {
    fn read<'a>(
        &'a self,
        target: &'a ContentManifestTarget,
    ) -> Pin<Box<dyn Future<Output = Result<ContentManifest, AppError>> + Send + 'a>> {
        Box::pin(async move {
            match &target.location.environment {
                EnvironmentRef::Native => NativeContentManifestReader.read(target).await,
                EnvironmentRef::Wsl { distro_name } => {
                    let target = target.clone();
                    self.environments
                        .with_session_retry(distro_name, move |session| {
                            let target = target.clone();
                            async move {
                                wsl_content_manifest::inspect(&session, &target, None).await
                            }
                        })
                        .await
                }
            }
        })
    }
}

fn resolve_native(
    logical_destinations: &[ResourceLocator],
) -> Result<Vec<ResolvedTargetFact>, AppError> {
    let backend = native_backend();
    logical_destinations
        .iter()
        .map(|logical| {
            let projected =
                project_target(std::path::Path::new(&logical.native_path), backend.clone())?;
            let inspection = inspect_entry_no_follow(&projected.physical_destination)?;
            let entry_kind = native_entry_kind(inspection.kind, &projected.physical_destination);
            let storage_access =
                native_storage_access(projected.physical_destination.to_string_lossy().as_ref());
            let destination = ResourceLocator {
                environment: EnvironmentRef::Native,
                native_path: projected
                    .physical_destination
                    .to_string_lossy()
                    .into_owned(),
            };
            let link_target = inspection
                .link_target
                .map(|target| target.to_string_lossy().into_owned());
            let link_target_identity = link_target
                .as_deref()
                .and_then(|raw| resolve_link_target_identity(&destination, raw));
            Ok(ResolvedTargetFact {
                key: projected.key,
                destination,
                storage_access,
                fingerprint: projected.fingerprint,
                entry_kind,
                link_target,
                link_target_identity,
            })
        })
        .collect()
}

pub(crate) fn resolve_native_targets(
    logical_destinations: &[ResourceLocator],
) -> Result<Vec<ResolvedTargetFact>, AppError> {
    resolve_native(logical_destinations)
}

async fn resolve_wsl(
    session: &WslSession,
    logical_destinations: &[String],
    cancellation: Option<CancellationSignal>,
) -> Result<Vec<ResolvedTargetFact>, AppError> {
    let projected = project_targets(session, logical_destinations, cancellation.clone()).await?;
    let physical_paths = projected
        .iter()
        .map(|target| target.physical_destination.clone())
        .collect::<Vec<_>>();
    let entries = inspect_entries(session, &physical_paths, cancellation).await?;
    if entries.len() != projected.len() {
        return Err(protocol_mismatch());
    }
    projected
        .into_iter()
        .zip(entries)
        .map(|(projected, entry)| {
            wsl_fact(
                session,
                projected,
                entry.fingerprint,
                entry.link_target,
                match entry.kind {
                    crate::environment::wsl::operations::entry::PosixEntryKind::Missing => {
                        TargetEntryKind::Missing
                    }
                    crate::environment::wsl::operations::entry::PosixEntryKind::File => {
                        TargetEntryKind::File
                    }
                    crate::environment::wsl::operations::entry::PosixEntryKind::Directory => {
                        TargetEntryKind::Directory
                    }
                    crate::environment::wsl::operations::entry::PosixEntryKind::Symlink => {
                        TargetEntryKind::Symlink
                    }
                    crate::environment::wsl::operations::entry::PosixEntryKind::BrokenLink => {
                        TargetEntryKind::BrokenLink
                    }
                    crate::environment::wsl::operations::entry::PosixEntryKind::Other => {
                        TargetEntryKind::Other
                    }
                },
            )
        })
        .collect()
}

pub(crate) async fn resolve_wsl_targets(
    session: &WslSession,
    logical_destinations: &[String],
    cancellation: Option<CancellationSignal>,
) -> Result<Vec<ResolvedTargetFact>, AppError> {
    resolve_wsl(session, logical_destinations, cancellation).await
}

fn wsl_fact(
    session: &WslSession,
    projected: ProjectedPosixTarget,
    fingerprint: EntryFingerprint,
    link_target: Option<String>,
    entry_kind: TargetEntryKind,
) -> Result<ResolvedTargetFact, AppError> {
    let normalized_distro_name = normalized_wsl_distro_name(&session.distro_name);
    let (case_sensitive, storage_access) =
        match windows_storage_owner(&projected.storage_projection) {
            WindowsStorageOwner::Windows => (false, StorageAccess::CrossStorage),
            WindowsStorageOwner::Wsl { distro_name }
                if normalized_wsl_distro_name(&distro_name) == normalized_distro_name =>
            {
                (true, StorageAccess::Native)
            }
            WindowsStorageOwner::Wsl { .. } | WindowsStorageOwner::Unknown => {
                return Err(AppError::StorageMappingUnsupported {
                    path: projected.physical_destination,
                    environment: EnvironmentRef::Wsl {
                        distro_name: session.distro_name.clone(),
                    },
                });
            }
        };
    let backend = ExecutionBackend::WslPosix {
        distro_name: normalized_distro_name.clone(),
    };
    let key = projected_physical_target_key(
        backend,
        PhysicalParentIdentity::Wsl {
            distro_name: normalized_distro_name,
            device: projected.anchor_device,
            inode: projected.anchor_inode,
        },
        projected.relative_components.iter().map(String::as_str),
        case_sensitive,
    )?;
    let destination = ResourceLocator {
        environment: EnvironmentRef::Wsl {
            distro_name: session.distro_name.clone(),
        },
        native_path: projected.physical_destination,
    };
    let link_target_identity = link_target
        .as_deref()
        .and_then(|raw| resolve_link_target_identity(&destination, raw));
    Ok(ResolvedTargetFact {
        key,
        destination,
        storage_access,
        fingerprint,
        entry_kind,
        link_target,
        link_target_identity,
    })
}

fn native_storage_access(path: &str) -> StorageAccess {
    if !cfg!(target_os = "windows") {
        return StorageAccess::Native;
    }
    match windows_storage_owner(path) {
        WindowsStorageOwner::Windows if is_windows_network_unc(path) => StorageAccess::Unsupported,
        WindowsStorageOwner::Windows => StorageAccess::Native,
        WindowsStorageOwner::Wsl { .. } => StorageAccess::CrossStorage,
        WindowsStorageOwner::Unknown => StorageAccess::Unknown,
    }
}

fn is_windows_network_unc(path: &str) -> bool {
    let normalized = path.trim().replace('/', "\\");
    let lower = normalized.to_ascii_lowercase();
    if lower.starts_with("\\\\?\\unc\\") {
        return true;
    }
    normalized.starts_with("\\\\")
        && !normalized.starts_with("\\\\?\\")
        && !normalized.starts_with("\\\\.\\")
}

fn native_entry_kind(kind: NativeEntryKind, path: &std::path::Path) -> TargetEntryKind {
    if matches!(
        kind,
        NativeEntryKind::Symlink | NativeEntryKind::ReparsePoint
    ) && std::fs::metadata(path).is_err_and(|error| error.kind() == std::io::ErrorKind::NotFound)
    {
        return TargetEntryKind::BrokenLink;
    }
    match kind {
        NativeEntryKind::Missing => TargetEntryKind::Missing,
        NativeEntryKind::File => TargetEntryKind::File,
        NativeEntryKind::Directory => TargetEntryKind::Directory,
        NativeEntryKind::Symlink => TargetEntryKind::Symlink,
        NativeEntryKind::ReparsePoint => TargetEntryKind::Junction,
        NativeEntryKind::Other => TargetEntryKind::Other,
    }
}

fn validate_owners(
    environment: &EnvironmentRef,
    targets: &[ResourceLocator],
) -> Result<(), AppError> {
    if targets.is_empty()
        || targets
            .iter()
            .any(|target| !same_environment_identity(environment, &target.environment))
    {
        return Err(AppError::StaleEnvironment);
    }
    Ok(())
}

fn native_backend() -> ExecutionBackend {
    if cfg!(windows) {
        ExecutionBackend::NativeWindows
    } else {
        ExecutionBackend::NativeUnix
    }
}

fn protocol_mismatch() -> AppError {
    AppError::ConfigurationCorrupted {
        message: "target projection and entry inspection counts differ".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::Arc;

    use tempfile::tempdir;

    use super::*;
    use crate::environment::types::{
        EnvironmentRef, ResourceLocator, SkillLocation, SkillLocationRef,
    };
    use crate::environment::wsl::WslRuntime;

    fn wsl_session() -> WslSession {
        WslSession {
            distro_name: "Ubuntu-24.04".to_string(),
            user: "alice".to_string(),
            uid: 1000,
            home: "/home/alice".to_string(),
            xdg_state_home: None,
            config_home: "/home/alice/.config".to_string(),
            environment: BTreeMap::new(),
            runtime_generation: 0,
        }
    }

    #[test]
    fn resolved_link_target_identity_normalizes_both_native_paths() {
        let root = std::env::temp_dir().join("skill-deck-link-identity");
        let destination_path = root.join("skills").join("demo");
        let destination = ResourceLocator {
            environment: EnvironmentRef::Native,
            native_path: destination_path.to_string_lossy().into_owned(),
        };
        let target = ResourceLocator {
            environment: EnvironmentRef::Native,
            native_path: root
                .join("shared")
                .join(".")
                .join("toolkit")
                .to_string_lossy()
                .into_owned(),
        };

        let identity = resolve_link_target_identity(&destination, "../shared/toolkit").unwrap();

        assert!(identity.matches(&target));
    }

    #[test]
    fn resolved_link_target_identity_uses_posix_rules_for_wsl() {
        let wsl_destination = ResourceLocator {
            environment: EnvironmentRef::Wsl {
                distro_name: "Ubuntu".to_string(),
            },
            native_path: "/home/alice/.agents/skills/demo".to_string(),
        };
        let identity = resolve_link_target_identity(&wsl_destination, "../shared/./demo").unwrap();

        assert!(identity.matches(&ResourceLocator {
            environment: wsl_destination.environment.clone(),
            native_path: "/home/alice/.agents/shared/other/../demo".to_string(),
        }));
        assert!(!identity.matches(&ResourceLocator {
            environment: wsl_destination.environment,
            native_path: "/home/alice/.agents/shared/Demo".to_string(),
        }));
    }

    #[cfg(windows)]
    #[test]
    fn resolved_native_link_identity_matches_windows_case_verbatim_and_unc_forms() {
        let drive_destination = ResourceLocator {
            environment: EnvironmentRef::Native,
            native_path: r"C:\scope\skills\demo".to_string(),
        };
        let drive =
            resolve_link_target_identity(&drive_destination, r"\\?\C:\Scope\Shared\Toolkit")
                .unwrap();
        assert!(drive.matches(&ResourceLocator {
            environment: EnvironmentRef::Native,
            native_path: "c:/scope/shared/./toolkit".to_string(),
        }));

        let unc_destination = ResourceLocator {
            environment: EnvironmentRef::Native,
            native_path: r"\\server\share\skills\demo".to_string(),
        };
        let unc =
            resolve_link_target_identity(&unc_destination, r"\\?\UNC\SERVER\Share\Library\Toolkit")
                .unwrap();
        assert!(unc.matches(&ResourceLocator {
            environment: EnvironmentRef::Native,
            native_path: r"\\server\share\library\toolkit".to_string(),
        }));
    }

    fn projected(child: &str, storage_projection: &str) -> ProjectedPosixTarget {
        ProjectedPosixTarget {
            index: 0,
            anchor_device: 7,
            anchor_inode: 11,
            physical_destination: format!("/target/{child}"),
            relative_components: vec![child.to_string()],
            storage_projection: storage_projection.to_string(),
        }
    }

    fn projected_fact(target: ProjectedPosixTarget) -> Result<ResolvedTargetFact, AppError> {
        wsl_fact(
            &wsl_session(),
            target,
            EntryFingerprint("entry-v1-missing".to_string()),
            None,
            TargetEntryKind::Missing,
        )
    }

    #[test]
    fn wsl_target_identity_uses_the_projected_storage_case_semantics() {
        let windows_upper = projected_fact(projected("Foo", r"C:\work\skills")).unwrap();
        let windows_lower = projected_fact(projected("foo", r"C:\work\skills")).unwrap();
        assert_eq!(windows_upper.key, windows_lower.key);

        let wsl_upper = projected_fact(projected(
            "Foo",
            r"\\wsl.localhost\Ubuntu-24.04\home\alice\skills",
        ))
        .unwrap();
        let wsl_lower = projected_fact(projected(
            "foo",
            r"\\wsl.localhost\ubuntu-24.04\home\alice\skills",
        ))
        .unwrap();
        assert_ne!(wsl_upper.key, wsl_lower.key);
    }

    #[test]
    fn wsl_fact_carries_the_resolved_link_target_identity() {
        let fact = wsl_fact(
            &wsl_session(),
            projected("demo", r"\\wsl.localhost\Ubuntu-24.04\home\alice\skills"),
            EntryFingerprint("entry-v1-link".to_string()),
            Some("library/demo".to_string()),
            TargetEntryKind::Symlink,
        )
        .unwrap();

        assert!(fact
            .link_target_identity
            .as_ref()
            .is_some_and(|identity| identity.matches(&ResourceLocator {
                environment: EnvironmentRef::Wsl {
                    distro_name: "ubuntu-24.04".to_string(),
                },
                native_path: "/target/library/demo".to_string(),
            })));
    }

    #[test]
    fn distinguishes_verbatim_drive_paths_from_network_unc_paths() {
        assert!(!is_windows_network_unc(r"\\?\C:\Users\alice\project"));
        assert!(is_windows_network_unc(r"\\?\UNC\server\share\project"));
        assert!(is_windows_network_unc(r"\\server\share\project"));
        assert!(!is_windows_network_unc(r"C:\Users\alice\project"));
    }

    #[test]
    fn wsl_target_identity_rejects_unknown_or_foreign_storage_owner() {
        assert!(matches!(
            projected_fact(projected("demo", "not-a-storage-path")),
            Err(AppError::StorageMappingUnsupported { .. })
        ));
        assert!(matches!(
            projected_fact(projected(
                "demo",
                r"\\wsl.localhost\Debian\home\alice\skills"
            )),
            Err(AppError::StorageMappingUnsupported { .. })
        ));
    }

    #[tokio::test]
    async fn native_facts_resolve_physical_destinations_and_missing_entry_fingerprints() {
        let temp = tempdir().unwrap();
        let destination = std::fs::canonicalize(temp.path())
            .unwrap()
            .join(".custom/skills/demo");
        let context = SkillLocationRef {
            environment: EnvironmentRef::Native,
            scope: SkillLocation::Global,
        };
        let resolver = RuntimeTargetFactResolver::new(Arc::new(WslRuntime::default()));

        let facts = resolver
            .resolve(
                &context,
                &[ResourceLocator {
                    environment: EnvironmentRef::Native,
                    native_path: destination.to_string_lossy().into_owned(),
                }],
                None,
            )
            .await
            .unwrap();

        assert_eq!(facts.len(), 1);
        assert_eq!(facts[0].entry_kind, TargetEntryKind::Missing);
        assert_eq!(
            facts[0].destination.native_path,
            destination.to_string_lossy()
        );
        assert_eq!(facts[0].fingerprint.0, "entry-v1-missing");
        assert_eq!(
            facts[0].key.normalized_final_child_name,
            ".custom/skills/demo"
        );
        assert!(!destination.parent().unwrap().exists());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn native_facts_keep_final_directory_and_symlink_kinds_distinct() {
        use std::os::unix::fs::symlink;

        let temp = tempdir().unwrap();
        let directory = temp.path().join("directory");
        let link = temp.path().join("link");
        std::fs::create_dir(&directory).unwrap();
        symlink(&directory, &link).unwrap();
        let context = SkillLocationRef {
            environment: EnvironmentRef::Native,
            scope: SkillLocation::Global,
        };
        let resolver = RuntimeTargetFactResolver::new(Arc::new(WslRuntime::default()));

        let facts = resolver
            .resolve(
                &context,
                &[
                    ResourceLocator {
                        environment: EnvironmentRef::Native,
                        native_path: directory.to_string_lossy().into_owned(),
                    },
                    ResourceLocator {
                        environment: EnvironmentRef::Native,
                        native_path: link.to_string_lossy().into_owned(),
                    },
                ],
                None,
            )
            .await
            .unwrap();

        assert_eq!(facts[0].entry_kind, TargetEntryKind::Directory);
        assert_eq!(facts[1].entry_kind, TargetEntryKind::Symlink);
        assert_eq!(
            facts[1].link_target.as_deref(),
            Some(directory.to_string_lossy().as_ref())
        );

        let broken = temp.path().join("broken");
        symlink(temp.path().join("missing"), &broken).unwrap();
        let broken_fact = resolver
            .resolve(
                &context,
                &[ResourceLocator {
                    environment: EnvironmentRef::Native,
                    native_path: broken.to_string_lossy().into_owned(),
                }],
                None,
            )
            .await
            .unwrap();
        assert_eq!(broken_fact[0].entry_kind, TargetEntryKind::BrokenLink);
    }

    #[tokio::test]
    async fn resolver_rejects_locators_owned_by_another_environment() {
        let context = SkillLocationRef {
            environment: EnvironmentRef::Native,
            scope: SkillLocation::Global,
        };
        let resolver = RuntimeTargetFactResolver::new(Arc::new(WslRuntime::default()));

        assert!(resolver
            .resolve(
                &context,
                &[ResourceLocator {
                    environment: EnvironmentRef::Wsl {
                        distro_name: "Ubuntu".to_string(),
                    },
                    native_path: "/tmp/demo".to_string(),
                }],
                None,
            )
            .await
            .is_err());
    }
}

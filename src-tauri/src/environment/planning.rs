use std::future::Future;
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
    normalized_wsl_distro_name, same_environment_identity, ContextRef, EnvironmentRef,
    ResourceLocator,
};
use crate::environment::wsl::operations::content_manifest as wsl_content_manifest;
use crate::environment::wsl::operations::entry::inspect_entries;
use crate::environment::wsl::operations::projection::{project_targets, ProjectedPosixTarget};
use crate::environment::wsl::{EnvironmentRegistry, WslSession};
use crate::error::AppError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedTargetFact {
    pub key: PhysicalTargetKey,
    pub destination: ResourceLocator,
    pub fingerprint: EntryFingerprint,
    pub entry_kind: TargetEntryKind,
    pub link_target: Option<String>,
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

pub type TargetFactFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

pub trait TargetFactResolver: Send + Sync {
    fn resolve<'a>(
        &'a self,
        context: &'a ContextRef,
        logical_destinations: &'a [ResourceLocator],
        cancellation: Option<CancellationSignal>,
    ) -> TargetFactFuture<'a, Result<Vec<ResolvedTargetFact>, AppError>>;
}

#[derive(Clone)]
pub struct RuntimeTargetFactResolver {
    environments: Arc<EnvironmentRegistry>,
}

impl RuntimeTargetFactResolver {
    pub fn new(environments: Arc<EnvironmentRegistry>) -> Self {
        Self { environments }
    }
}

impl TargetFactResolver for RuntimeTargetFactResolver {
    fn resolve<'a>(
        &'a self,
        context: &'a ContextRef,
        logical_destinations: &'a [ResourceLocator],
        cancellation: Option<CancellationSignal>,
    ) -> TargetFactFuture<'a, Result<Vec<ResolvedTargetFact>, AppError>> {
        Box::pin(async move {
            validate_owners(&context.environment, logical_destinations)?;
            match &context.environment {
                EnvironmentRef::Host => {
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
                EnvironmentRef::Host => NativeContentManifestReader.read(target).await,
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
            Ok(ResolvedTargetFact {
                key: projected.key,
                destination: ResourceLocator {
                    environment: EnvironmentRef::Host,
                    native_path: projected
                        .physical_destination
                        .to_string_lossy()
                        .into_owned(),
                },
                fingerprint: projected.fingerprint,
                entry_kind,
                link_target: inspection
                    .link_target
                    .map(|target| target.to_string_lossy().into_owned()),
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
    let case_sensitive = match windows_storage_owner(&projected.storage_projection) {
        WindowsStorageOwner::Host => false,
        WindowsStorageOwner::Wsl { distro_name }
            if normalized_wsl_distro_name(&distro_name) == normalized_distro_name =>
        {
            true
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
    Ok(ResolvedTargetFact {
        key,
        destination: ResourceLocator {
            environment: EnvironmentRef::Wsl {
                distro_name: session.distro_name.clone(),
            },
            native_path: projected.physical_destination,
        },
        fingerprint,
        entry_kind,
        link_target,
    })
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
    use crate::environment::types::{ContextRef, ContextScope, EnvironmentRef, ResourceLocator};
    use crate::environment::wsl::EnvironmentRegistry;

    fn wsl_session() -> WslSession {
        WslSession {
            distro_name: "Ubuntu-24.04".to_string(),
            user: "alice".to_string(),
            uid: 1000,
            home: "/home/alice".to_string(),
            xdg_state_home: None,
            config_home: "/home/alice/.config".to_string(),
            environment: BTreeMap::new(),
            git_available: true,
            execution_profile: crate::environment::wsl_protocol::WslExecutionProfile::all_supported(
            ),
            runtime_generation: 0,
        }
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
    async fn host_facts_resolve_physical_destinations_and_missing_entry_fingerprints() {
        let temp = tempdir().unwrap();
        let destination = std::fs::canonicalize(temp.path())
            .unwrap()
            .join(".custom/skills/demo");
        let context = ContextRef {
            environment: EnvironmentRef::Host,
            scope: ContextScope::Global,
        };
        let resolver = RuntimeTargetFactResolver::new(Arc::new(EnvironmentRegistry::default()));

        let facts = resolver
            .resolve(
                &context,
                &[ResourceLocator {
                    environment: EnvironmentRef::Host,
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
    async fn host_facts_keep_final_directory_and_symlink_kinds_distinct() {
        use std::os::unix::fs::symlink;

        let temp = tempdir().unwrap();
        let directory = temp.path().join("directory");
        let link = temp.path().join("link");
        std::fs::create_dir(&directory).unwrap();
        symlink(&directory, &link).unwrap();
        let context = ContextRef {
            environment: EnvironmentRef::Host,
            scope: ContextScope::Global,
        };
        let resolver = RuntimeTargetFactResolver::new(Arc::new(EnvironmentRegistry::default()));

        let facts = resolver
            .resolve(
                &context,
                &[
                    ResourceLocator {
                        environment: EnvironmentRef::Host,
                        native_path: directory.to_string_lossy().into_owned(),
                    },
                    ResourceLocator {
                        environment: EnvironmentRef::Host,
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
                    environment: EnvironmentRef::Host,
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
        let context = ContextRef {
            environment: EnvironmentRef::Host,
            scope: ContextScope::Global,
        };
        let resolver = RuntimeTargetFactResolver::new(Arc::new(EnvironmentRegistry::default()));

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

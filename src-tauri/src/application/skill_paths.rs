use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;

use crate::application::installed_skill_resolver::InstalledSkillResolver;
use crate::application::mutation::plan::stable_digest;
use crate::core::mutation::CancellationSignal;
use crate::environment::content_manifest::{
    ContentManifestHash, ContentManifestReader, ContentManifestTarget,
};
use crate::environment::context_resolver::ResolvedContext;
use crate::environment::planning::{ResolvedTargetFact, TargetEntryKind, TargetFactResolver};
use crate::environment::types::{
    same_environment_identity, EnvironmentKey, EnvironmentRef, ResourceLocator, SkillLocation,
};
use crate::error::AppError;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RootResolutionRevision(String);

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TargetRevision(String);

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ContentRevision(Option<ContentManifestHash>);

impl RootResolutionRevision {
    pub fn as_str(&self) -> &str {
        &self.0
    }

    #[cfg(test)]
    pub(crate) fn for_test(value: &str) -> Self {
        Self(value.to_string())
    }
}

#[cfg(test)]
impl TargetRevision {
    pub(crate) fn for_test(value: &str) -> Self {
        Self(value.to_string())
    }
}

impl ContentRevision {
    pub fn manifest_hash(&self) -> Option<&ContentManifestHash> {
        self.0.as_ref()
    }

    #[cfg(test)]
    pub(crate) fn missing_for_test() -> Self {
        Self(None)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedSkillRoot {
    pub environment: EnvironmentRef,
    pub root: ResourceLocator,
    pub resolution_revision: RootResolutionRevision,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedSkillTarget {
    pub skill_name: String,
    pub root: ResolvedSkillRoot,
    pub install_dir_name: String,
    pub target: ResolvedTargetFact,
    pub target_revision: TargetRevision,
    pub content_revision: ContentRevision,
}

pub struct SkillTargetRequest {
    pub skill_name: String,
}

pub struct SkillPathObserver;

impl SkillPathObserver {
    pub fn resolve_collection(
        environment: EnvironmentRef,
        root: ResourceLocator,
        environment_revision: &str,
    ) -> Result<ResolvedSkillRoot, AppError> {
        if environment_revision.trim().is_empty()
            || !same_environment_identity(&environment, &root.environment)
        {
            return Err(AppError::StaleEnvironment);
        }
        let environment_key = EnvironmentKey::from_ref(&environment);
        let resolution_revision = RootResolutionRevision(stable_digest(&(
            "skill-collection-resolution-v1",
            environment_key,
            &root.native_path,
            environment_revision,
        ))?);
        Ok(ResolvedSkillRoot {
            environment,
            root,
            resolution_revision,
        })
    }

    pub fn resolve_installed_collection(
        resolved: &ResolvedContext,
        environment_revision: &str,
    ) -> Result<ResolvedSkillRoot, AppError> {
        if environment_revision.trim().is_empty()
            || !same_environment_identity(
                &resolved.context.environment,
                &resolved.skill_root.environment,
            )
        {
            return Err(AppError::StaleEnvironment);
        }
        match (&resolved.context.scope, resolved.project.as_ref()) {
            (SkillLocation::Global, None) => {}
            (SkillLocation::Project { project_id }, Some(project)) if project_id == &project.id => {
            }
            _ => return Err(AppError::StaleContext),
        }
        Self::resolve_collection(
            resolved.context.environment.clone(),
            resolved.skill_root.clone(),
            environment_revision,
        )
    }

    pub async fn resolve_skill_targets<T>(
        targets: &T,
        root: &ResolvedSkillRoot,
        requests: Vec<SkillTargetRequest>,
        cancellation: Option<CancellationSignal>,
    ) -> Result<Vec<ResolvedSkillTarget>, AppError>
    where
        T: TargetFactResolver + ContentManifestReader,
    {
        if requests.is_empty() {
            return Err(AppError::Validation {
                field: Some("skills".to_string()),
                message: "at least one Skill is required".to_string(),
            });
        }
        let mut logical_names = BTreeSet::new();
        let mut install_dir_names = Vec::with_capacity(requests.len());
        let mut destinations = Vec::with_capacity(requests.len());
        for request in &requests {
            if !logical_names.insert(request.skill_name.clone()) {
                return Err(skill_name_conflict(&request.skill_name));
            }
            let install_dir_name = InstalledSkillResolver::install_dir_name(&request.skill_name)?;
            destinations.push(root.root.join_child(&install_dir_name));
            install_dir_names.push(install_dir_name);
        }

        let facts = targets
            .resolve_environment(&root.environment, &destinations, cancellation)
            .await?;
        if facts.len() != destinations.len() {
            return Err(AppError::StaleTarget);
        }

        let mut physical_skills = BTreeMap::new();
        let mut resolved = Vec::with_capacity(requests.len());
        for ((request, install_dir_name), canonical) in
            requests.into_iter().zip(install_dir_names).zip(facts)
        {
            if let Some(existing) =
                physical_skills.insert(canonical.key.clone(), request.skill_name.clone())
            {
                if existing != request.skill_name {
                    return Err(skill_name_conflict(&request.skill_name));
                }
            }
            let target_revision = TargetRevision(stable_digest(&(
                "skill-target-revision-v1",
                &canonical.key,
                &canonical.fingerprint,
                canonical.entry_kind as u8,
                &canonical.link_target,
                canonical.storage_access,
            ))?);
            let content_revision = if canonical.entry_kind == TargetEntryKind::Directory {
                let manifest = targets
                    .read(&ContentManifestTarget {
                        key: canonical.key.clone(),
                        location: canonical.destination.clone(),
                    })
                    .await?;
                ContentRevision(Some(manifest.hash().clone()))
            } else {
                ContentRevision(None)
            };
            resolved.push(ResolvedSkillTarget {
                skill_name: request.skill_name,
                root: root.clone(),
                install_dir_name,
                target: canonical,
                target_revision,
                content_revision,
            });
        }
        Ok(resolved)
    }

    pub async fn resolve_install_targets<T>(
        targets: &T,
        root: &ResolvedSkillRoot,
        requested_names: Vec<String>,
        existing_names: impl IntoIterator<Item = String>,
        cancellation: Option<CancellationSignal>,
    ) -> Result<Vec<ResolvedSkillTarget>, AppError>
    where
        T: TargetFactResolver + ContentManifestReader,
    {
        if requested_names.is_empty() {
            return Err(AppError::Validation {
                field: Some("skills".to_string()),
                message: "at least one Skill is required".to_string(),
            });
        }
        let requested_count = requested_names.len();
        let requested = requested_names.iter().cloned().collect::<BTreeSet<_>>();
        let mut names = requested_names;
        names.extend(
            existing_names
                .into_iter()
                .filter(|name| !requested.contains(name)),
        );
        let mut resolved = Self::resolve_skill_targets(
            targets,
            root,
            names
                .into_iter()
                .map(|skill_name| SkillTargetRequest { skill_name })
                .collect(),
            cancellation,
        )
        .await?;
        resolved.truncate(requested_count);
        Ok(resolved)
    }

    pub(crate) fn revisions_for_observation(
        target: &ResolvedTargetFact,
        content_manifest: Option<ContentManifestHash>,
    ) -> Result<(TargetRevision, ContentRevision), AppError> {
        let target_revision = TargetRevision(stable_digest(&(
            "skill-target-revision-v1",
            &target.key,
            &target.fingerprint,
            target.entry_kind as u8,
            &target.link_target,
            target.storage_access,
        ))?);
        Ok((target_revision, ContentRevision(content_manifest)))
    }
}

fn skill_name_conflict(skill_name: &str) -> AppError {
    AppError::Validation {
        field: Some("skillName".to_string()),
        message: format!("Skill '{skill_name}' conflicts with another Skill directory"),
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::{Arc, Mutex};

    use super::*;
    use crate::environment::planning::RuntimeTargetFactResolver;
    use crate::environment::planning::{TargetEntryKind, TargetFactFuture};
    use crate::environment::runtime::{
        EntryFingerprint, ExecutionBackend, PhysicalParentIdentity, PhysicalTargetKey,
    };
    use crate::environment::types::SkillLocationRef;
    use crate::environment::types::{RegisteredProject, StorageAccess};

    #[derive(Default)]
    struct RecordingTargets {
        destinations: Arc<Mutex<Vec<ResourceLocator>>>,
    }

    impl TargetFactResolver for RecordingTargets {
        fn resolve<'a>(
            &'a self,
            _context: &'a SkillLocationRef,
            logical_destinations: &'a [ResourceLocator],
            _cancellation: Option<CancellationSignal>,
        ) -> TargetFactFuture<'a, Result<Vec<ResolvedTargetFact>, AppError>> {
            let destinations = logical_destinations.to_vec();
            *self.destinations.lock().unwrap() = destinations.clone();
            Box::pin(async move {
                destinations
                    .into_iter()
                    .enumerate()
                    .map(|(index, destination)| {
                        let (backend, physical_parent, child_name) = match &destination.environment
                        {
                            EnvironmentRef::Native if cfg!(windows) => (
                                ExecutionBackend::NativeWindows,
                                PhysicalParentIdentity::Windows {
                                    volume_serial: 1,
                                    file_id: 1,
                                },
                                std::path::Path::new(&destination.native_path)
                                    .file_name()
                                    .unwrap()
                                    .to_string_lossy()
                                    .to_ascii_lowercase(),
                            ),
                            EnvironmentRef::Native => (
                                ExecutionBackend::NativeUnix,
                                PhysicalParentIdentity::Unix {
                                    device: 1,
                                    inode: 1,
                                },
                                std::path::Path::new(&destination.native_path)
                                    .file_name()
                                    .unwrap()
                                    .to_string_lossy()
                                    .into_owned(),
                            ),
                            EnvironmentRef::Wsl { distro_name } => (
                                ExecutionBackend::WslPosix {
                                    distro_name: distro_name.to_ascii_lowercase(),
                                },
                                PhysicalParentIdentity::Wsl {
                                    distro_name: distro_name.to_ascii_lowercase(),
                                    device: 1,
                                    inode: 1,
                                },
                                destination
                                    .native_path
                                    .rsplit('/')
                                    .next()
                                    .unwrap()
                                    .to_string(),
                            ),
                        };
                        Ok(ResolvedTargetFact {
                            key: PhysicalTargetKey {
                                backend,
                                physical_parent,
                                normalized_final_child_name: child_name,
                            },
                            destination,
                            storage_access: StorageAccess::Native,
                            fingerprint: EntryFingerprint(format!("entry-v1-{index}")),
                            entry_kind: TargetEntryKind::Missing,
                            link_target: None,
                            link_target_identity: None,
                        })
                    })
                    .collect()
            })
        }
    }

    impl ContentManifestReader for RecordingTargets {
        fn read<'a>(
            &'a self,
            _target: &'a ContentManifestTarget,
        ) -> std::pin::Pin<
            Box<
                dyn std::future::Future<
                        Output = Result<
                            crate::environment::content_manifest::ContentManifest,
                            AppError,
                        >,
                    > + Send
                    + 'a,
            >,
        > {
            Box::pin(async { Err(AppError::StaleTarget) })
        }
    }

    fn context(
        environment: EnvironmentRef,
        scope: SkillLocation,
        root: &str,
        project: Option<RegisteredProject>,
    ) -> ResolvedContext {
        ResolvedContext {
            context: SkillLocationRef {
                environment: environment.clone(),
                scope,
            },
            project,
            home: ResourceLocator {
                environment: environment.clone(),
                native_path: "/home/alice".to_string(),
            },
            skill_root: ResourceLocator {
                environment: environment.clone(),
                native_path: root.to_string(),
            },
            lock: ResourceLocator {
                environment,
                native_path: "/ignored/skills-lock.json".to_string(),
            },
        }
    }

    #[tokio::test]
    async fn content_revision_changes_without_changing_the_skill_target() {
        let root = tempfile::tempdir().unwrap();
        let skill_root = root.path().join("skills");
        let skill_dir = skill_root.join("demo");
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: demo\ndescription: Demo\n---\nfirst",
        )
        .unwrap();
        let resolved = context(
            EnvironmentRef::Native,
            SkillLocation::Global,
            &skill_root.to_string_lossy(),
            None,
        );
        let collection =
            SkillPathObserver::resolve_installed_collection(&resolved, "environment-v1").unwrap();
        let targets = RuntimeTargetFactResolver::new(Arc::new(
            crate::environment::wsl::WslRuntime::default(),
        ));
        let request = || SkillTargetRequest {
            skill_name: "demo".to_string(),
        };

        let first =
            SkillPathObserver::resolve_skill_targets(&targets, &collection, vec![request()], None)
                .await
                .unwrap();
        fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: demo\ndescription: Demo\n---\nsecond",
        )
        .unwrap();
        let second =
            SkillPathObserver::resolve_skill_targets(&targets, &collection, vec![request()], None)
                .await
                .unwrap();

        assert_eq!(first[0].target_revision, second[0].target_revision);
        assert_ne!(first[0].content_revision, second[0].content_revision);
    }

    #[test]
    fn installed_collection_keeps_environment_scope_root_and_path_revision() {
        let project = RegisteredProject {
            id: "project-1".to_string(),
            native_path: "/work/app".to_string(),
            display_name: None,
            order: None,
            suppress_cross_storage_warning: false,
        };
        let resolved = context(
            EnvironmentRef::Wsl {
                distro_name: "Ubuntu".to_string(),
            },
            SkillLocation::Project {
                project_id: project.id.clone(),
            },
            "/work/app/.agents/skills",
            Some(project),
        );

        let first =
            SkillPathObserver::resolve_installed_collection(&resolved, "environment-v1").unwrap();
        let second =
            SkillPathObserver::resolve_installed_collection(&resolved, "environment-v2").unwrap();
        let mut display_only = resolved.clone();
        display_only.project.as_mut().unwrap().display_name = Some("Renamed".to_string());
        display_only.project.as_mut().unwrap().order = Some(9);
        let display_only =
            SkillPathObserver::resolve_installed_collection(&display_only, "environment-v1")
                .unwrap();
        let mut moved = resolved.clone();
        moved.project.as_mut().unwrap().native_path = "/work/moved".to_string();
        moved.skill_root.native_path = "/work/moved/.agents/skills".to_string();
        let moved =
            SkillPathObserver::resolve_installed_collection(&moved, "environment-v1").unwrap();
        let mut equivalent_environment = resolved.clone();
        equivalent_environment.context.environment = EnvironmentRef::Wsl {
            distro_name: "UBUNTU".to_string(),
        };
        equivalent_environment.home.environment =
            equivalent_environment.context.environment.clone();
        equivalent_environment.skill_root.environment =
            equivalent_environment.context.environment.clone();
        equivalent_environment.lock.environment =
            equivalent_environment.context.environment.clone();
        let equivalent_environment = SkillPathObserver::resolve_installed_collection(
            &equivalent_environment,
            "environment-v1",
        )
        .unwrap();

        assert_eq!(first.environment, resolved.context.environment);
        assert_eq!(first.root, resolved.skill_root);
        assert_eq!(
            first.resolution_revision.0,
            display_only.resolution_revision.0
        );
        assert_ne!(
            first.resolution_revision.0.as_str(),
            second.resolution_revision.0.as_str()
        );
        assert_ne!(first.resolution_revision.0, moved.resolution_revision.0);
        assert_eq!(
            first.resolution_revision.0,
            equivalent_environment.resolution_revision.0
        );
    }

    #[tokio::test]
    async fn skill_targets_use_safe_directory_names() {
        let resolved = context(
            EnvironmentRef::Wsl {
                distro_name: "Ubuntu".to_string(),
            },
            SkillLocation::Global,
            "/home/alice/.agents/skills",
            None,
        );
        let collection =
            SkillPathObserver::resolve_installed_collection(&resolved, "environment-v1").unwrap();
        let targets = RecordingTargets::default();
        let observed = SkillPathObserver::resolve_skill_targets(
            &targets,
            &collection,
            vec![SkillTargetRequest {
                skill_name: "ce:review".to_string(),
            }],
            None,
        )
        .await
        .unwrap();

        assert_eq!(observed[0].install_dir_name, "ce-review");
        assert_eq!(
            observed[0].target.destination.native_path,
            "/home/alice/.agents/skills/ce-review"
        );
        assert!(observed[0]
            .target_revision
            .0
            .as_str()
            .starts_with("digest-v1-"));
    }

    #[tokio::test]
    async fn different_skill_names_cannot_resolve_to_one_physical_directory() {
        let resolved = context(
            EnvironmentRef::Native,
            SkillLocation::Global,
            "/home/alice/.agents/skills",
            None,
        );
        let collection =
            SkillPathObserver::resolve_installed_collection(&resolved, "environment-v1").unwrap();
        let targets = RecordingTargets::default();
        let requests = ["ce:review", "ce review"]
            .into_iter()
            .map(|skill_name| SkillTargetRequest {
                skill_name: skill_name.to_string(),
            })
            .collect();

        assert!(matches!(
            SkillPathObserver::resolve_skill_targets(
                &targets,
                &collection,
                requests,
                None,
            )
            .await,
            Err(AppError::Validation { field: Some(field), .. }) if field == "skillName"
        ));
    }
}

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::environment::planning::TargetEntryKind;
use crate::environment::planning::TargetFactResolver;
use crate::environment::types::{ContextRef, ResourceLocator};
use crate::error::AppError;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct SkillIdentity {
    pub context: ContextRef,
    pub skill_name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub enum ConfigResourceKind {
    ContextRoot,
    CanonicalSkillsRoot,
}

#[derive(Debug, Clone)]
pub struct ResolvedResourceContext {
    pub context_root: ResourceLocator,
    pub canonical_skills_root: ResourceLocator,
}

pub type ResourceFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

pub trait ResourceContextSource: Send + Sync {
    fn resolve<'a>(
        &'a self,
        context: &'a ContextRef,
    ) -> ResourceFuture<'a, Result<ResolvedResourceContext, AppError>>;
}

pub trait AuthorizedResourceOpener: Send + Sync {
    fn open<'a>(&'a self, target: ResourceLocator) -> ResourceFuture<'a, Result<(), AppError>>;
}

pub trait AuthorizedResourceReader: Send + Sync {
    fn read_skill<'a>(
        &'a self,
        target: ResourceLocator,
    ) -> ResourceFuture<'a, Result<String, AppError>>;
}

pub struct ResourceService<S, T, O, R> {
    source: S,
    targets: T,
    opener: O,
    reader: R,
}

impl<S, T, O, R> ResourceService<S, T, O, R>
where
    S: ResourceContextSource,
    T: TargetFactResolver,
    O: AuthorizedResourceOpener,
    R: AuthorizedResourceReader,
{
    pub fn new(source: S, targets: T, opener: O, reader: R) -> Self {
        Self {
            source,
            targets,
            opener,
            reader,
        }
    }

    pub async fn open_skill(&self, identity: &SkillIdentity) -> Result<(), AppError> {
        validate_skill_name(&identity.skill_name)?;
        let resolved = self.source.resolve(&identity.context).await?;
        let target = crate::application::skill_entries::join_entry(
            &resolved.canonical_skills_root,
            &identity.skill_name,
        );
        let target = self.resolve_directory(&identity.context, target).await?;
        self.opener.open(target).await
    }

    pub async fn read_skill(&self, identity: &SkillIdentity) -> Result<String, AppError> {
        validate_skill_name(&identity.skill_name)?;
        let resolved = self.source.resolve(&identity.context).await?;
        let target = crate::application::skill_entries::join_entry(
            &resolved.canonical_skills_root,
            &identity.skill_name,
        );
        let target = self.resolve_directory(&identity.context, target).await?;
        self.reader.read_skill(target).await
    }

    pub async fn open_config(
        &self,
        context: &ContextRef,
        kind: ConfigResourceKind,
    ) -> Result<(), AppError> {
        let resolved = self.source.resolve(context).await?;
        let target = match kind {
            ConfigResourceKind::ContextRoot => resolved.context_root,
            ConfigResourceKind::CanonicalSkillsRoot => resolved.canonical_skills_root,
        };
        let target = self.resolve_directory(context, target).await?;
        self.opener.open(target).await
    }

    async fn resolve_directory(
        &self,
        context: &ContextRef,
        target: ResourceLocator,
    ) -> Result<ResourceLocator, AppError> {
        let mut facts = self
            .targets
            .resolve(context, std::slice::from_ref(&target), None)
            .await?;
        if facts.len() != 1 {
            return Err(AppError::StaleTarget);
        }
        let fact = facts.pop().expect("one resolved resource");
        if fact.entry_kind != TargetEntryKind::Directory {
            return Err(AppError::PathNotFound {
                path: target.native_path,
            });
        }
        Ok(fact.destination)
    }
}

fn validate_skill_name(name: &str) -> Result<(), AppError> {
    if name.is_empty()
        || name.len() > 255
        || name == "."
        || name == ".."
        || name.contains(['/', '\\', '\0'])
        || crate::core::skill::sanitize_name(name) != name
    {
        return Err(AppError::UnsafePath {
            path: name.to_string(),
            reason: "Skill identity must contain one normalized entry name".to_string(),
        });
    }
    Ok(())
}

#[derive(Clone)]
pub struct RuntimeResourceContextSource {
    facts: crate::application::runtime_facts::RuntimePlanningFactSource,
}

impl RuntimeResourceContextSource {
    pub fn new(facts: crate::application::runtime_facts::RuntimePlanningFactSource) -> Self {
        Self { facts }
    }
}

impl ResourceContextSource for RuntimeResourceContextSource {
    fn resolve<'a>(
        &'a self,
        context: &'a ContextRef,
    ) -> ResourceFuture<'a, Result<ResolvedResourceContext, AppError>> {
        use crate::application::install_planner::InstallPlanningFactSource;

        Box::pin(async move {
            let facts = self.facts.current(context).await?;
            let context_root = facts
                .resolved_context
                .project
                .as_ref()
                .map(|project| ResourceLocator {
                    environment: context.environment.clone(),
                    native_path: project.native_path.clone(),
                })
                .unwrap_or_else(|| facts.resolved_context.home.clone());
            Ok(ResolvedResourceContext {
                context_root,
                canonical_skills_root: facts.resolved_context.skill_root,
            })
        })
    }
}

impl AuthorizedResourceOpener for crate::environment::opener::SystemResourceOpener {
    fn open<'a>(&'a self, target: ResourceLocator) -> ResourceFuture<'a, Result<(), AppError>> {
        Box::pin(async move { crate::environment::opener::open_authorized_resource(&target) })
    }
}

#[derive(Clone)]
pub struct RuntimeResourceReader {
    environments: Arc<crate::environment::wsl::EnvironmentRegistry>,
}

impl RuntimeResourceReader {
    pub fn new(environments: Arc<crate::environment::wsl::EnvironmentRegistry>) -> Self {
        Self { environments }
    }
}

impl AuthorizedResourceReader for RuntimeResourceReader {
    fn read_skill<'a>(
        &'a self,
        target: ResourceLocator,
    ) -> ResourceFuture<'a, Result<String, AppError>> {
        Box::pin(async move {
            match &target.environment {
                crate::environment::types::EnvironmentRef::Host => {
                    crate::core::skill::read_skill_content(&target.native_path)
                }
                crate::environment::types::EnvironmentRef::Wsl { distro_name } => {
                    let path = target.native_path;
                    self.environments
                        .with_session_retry(distro_name, move |session| {
                            let path = path.clone();
                            async move {
                                let markdown = crate::environment::wsl::operations::skill_content::read_skill_markdown(
                                    &session,
                                    &path,
                                )
                                .await?;
                                Ok(crate::core::skill::skill_content_from_markdown(&markdown))
                            }
                        })
                        .await
                }
            }
        })
    }
}

pub type RuntimeResourceService = ResourceService<
    RuntimeResourceContextSource,
    crate::environment::planning::RuntimeTargetFactResolver,
    crate::environment::opener::SystemResourceOpener,
    RuntimeResourceReader,
>;

pub fn build_runtime_resource_service(
    environments: Arc<crate::environment::wsl::EnvironmentRegistry>,
    registry: Arc<dyn crate::application::runtime_facts::AgentRegistrySnapshotSource>,
) -> RuntimeResourceService {
    let facts = crate::application::runtime_facts::RuntimePlanningFactSource::for_current_user(
        registry,
        environments.clone(),
    );
    ResourceService::new(
        RuntimeResourceContextSource::new(facts),
        crate::environment::planning::RuntimeTargetFactResolver::new(environments.clone()),
        crate::environment::opener::SystemResourceOpener,
        RuntimeResourceReader::new(environments),
    )
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;
    use crate::environment::planning::{ResolvedTargetFact, TargetEntryKind, TargetFactFuture};
    use crate::environment::runtime::{
        EntryFingerprint, ExecutionBackend, PhysicalParentIdentity, PhysicalTargetKey,
    };
    use crate::environment::types::{ContextScope, EnvironmentRef};

    #[derive(Clone)]
    struct StaticSource;

    impl ResourceContextSource for StaticSource {
        fn resolve<'a>(
            &'a self,
            context: &'a ContextRef,
        ) -> ResourceFuture<'a, Result<ResolvedResourceContext, AppError>> {
            let environment = context.environment.clone();
            Box::pin(async move {
                Ok(ResolvedResourceContext {
                    context_root: locator(environment.clone(), "/work/project"),
                    canonical_skills_root: locator(environment, "/work/project/.agents/skills"),
                })
            })
        }
    }

    #[derive(Clone)]
    struct DirectoryTargets;

    impl TargetFactResolver for DirectoryTargets {
        fn resolve<'a>(
            &'a self,
            _context: &'a ContextRef,
            destinations: &'a [ResourceLocator],
            _cancellation: Option<crate::core::mutation::CancellationSignal>,
        ) -> TargetFactFuture<'a, Result<Vec<ResolvedTargetFact>, AppError>> {
            Box::pin(async move {
                Ok(destinations
                    .iter()
                    .map(|destination| ResolvedTargetFact {
                        key: PhysicalTargetKey {
                            backend: ExecutionBackend::NativeUnix,
                            physical_parent: PhysicalParentIdentity::Unix {
                                device: 1,
                                inode: 2,
                            },
                            normalized_final_child_name: destination
                                .native_path
                                .rsplit('/')
                                .next()
                                .unwrap_or_default()
                                .to_string(),
                        },
                        destination: destination.clone(),
                        fingerprint: EntryFingerprint("entry-v1-directory".to_string()),
                        entry_kind: TargetEntryKind::Directory,
                        link_target: None,
                    })
                    .collect())
            })
        }
    }

    #[derive(Clone, Default)]
    struct RecordingOpener(Arc<Mutex<Vec<ResourceLocator>>>);

    impl AuthorizedResourceOpener for RecordingOpener {
        fn open<'a>(&'a self, target: ResourceLocator) -> ResourceFuture<'a, Result<(), AppError>> {
            self.0.lock().unwrap().push(target);
            Box::pin(async { Ok(()) })
        }
    }

    #[derive(Clone, Default)]
    struct RecordingReader(Arc<Mutex<Vec<ResourceLocator>>>);

    impl AuthorizedResourceReader for RecordingReader {
        fn read_skill<'a>(
            &'a self,
            target: ResourceLocator,
        ) -> ResourceFuture<'a, Result<String, AppError>> {
            self.0.lock().unwrap().push(target);
            Box::pin(async { Ok("skill body".to_string()) })
        }
    }

    fn context() -> ContextRef {
        ContextRef {
            environment: EnvironmentRef::Host,
            scope: ContextScope::Project {
                project_id: "project-1".to_string(),
            },
        }
    }

    fn locator(environment: EnvironmentRef, path: &str) -> ResourceLocator {
        ResourceLocator {
            environment,
            native_path: path.to_string(),
        }
    }

    #[tokio::test]
    async fn skill_identity_is_resolved_under_the_backend_canonical_root() {
        let opener = RecordingOpener::default();
        let opened = opener.0.clone();
        let service = ResourceService::new(
            StaticSource,
            DirectoryTargets,
            opener,
            RecordingReader::default(),
        );

        service
            .open_skill(&SkillIdentity {
                context: context(),
                skill_name: "demo".to_string(),
            })
            .await
            .unwrap();

        assert_eq!(
            *opened.lock().unwrap(),
            vec![locator(
                EnvironmentRef::Host,
                &std::path::Path::new("/work/project/.agents/skills")
                    .join("demo")
                    .to_string_lossy()
            )]
        );
    }

    #[tokio::test]
    async fn traversal_identity_is_rejected_before_the_opener() {
        let opener = RecordingOpener::default();
        let opened = opener.0.clone();
        let service = ResourceService::new(
            StaticSource,
            DirectoryTargets,
            opener,
            RecordingReader::default(),
        );

        assert!(service
            .open_skill(&SkillIdentity {
                context: context(),
                skill_name: "../outside".to_string(),
            })
            .await
            .is_err());
        assert!(opened.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn config_kind_selects_only_a_backend_resolved_root() {
        let opener = RecordingOpener::default();
        let opened = opener.0.clone();
        let service = ResourceService::new(
            StaticSource,
            DirectoryTargets,
            opener,
            RecordingReader::default(),
        );

        service
            .open_config(&context(), ConfigResourceKind::ContextRoot)
            .await
            .unwrap();

        assert_eq!(
            *opened.lock().unwrap(),
            vec![locator(EnvironmentRef::Host, "/work/project")]
        );
    }

    #[tokio::test]
    async fn content_read_uses_the_same_backend_authorized_skill_identity() {
        let reader = RecordingReader::default();
        let read = reader.0.clone();
        let service = ResourceService::new(
            StaticSource,
            DirectoryTargets,
            RecordingOpener::default(),
            reader,
        );

        let content = service
            .read_skill(&SkillIdentity {
                context: context(),
                skill_name: "demo".to_string(),
            })
            .await
            .unwrap();

        assert_eq!(content, "skill body");
        assert_eq!(
            *read.lock().unwrap(),
            vec![locator(
                EnvironmentRef::Host,
                &std::path::Path::new("/work/project/.agents/skills")
                    .join("demo")
                    .to_string_lossy()
            )]
        );
    }
}

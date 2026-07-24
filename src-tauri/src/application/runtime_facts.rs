use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::application::install::InstallFuture;
use crate::application::install_planner::{InstallPlanningFactSource, InstallPlanningFacts};
use crate::application::mutation::coordinator::{
    BoxFuture, RuntimeAuthorityRevisions, RuntimeRevisionSnapshot, RuntimeRevisionSource,
};
use crate::application::mutation::plan::{stable_digest, RuntimeRevisions};
use crate::core::agent_registry::AgentRegistrySnapshot;
use crate::core::projects::{ProjectPathSemantics, ProjectsFile};
use crate::core::skill_lock;
use crate::core::{get_config_path, paths::PATHS};
use crate::environment::agent_environment::{AgentEnvironmentResolver, EnvironmentContext};
use crate::environment::context_resolver::ContextResolver;
use crate::environment::context_resolver::ResolvedContext;
use crate::environment::native::atomic_file::NativeAtomicDocumentIo;
use crate::environment::planning::{resolve_native_targets, resolve_wsl_targets};
use crate::environment::runtime::{
    context_snapshot_revision, ContextOwnedRootFields, ContextRevisionInput,
    PhysicalProjectIdentity,
};
use crate::environment::types::{
    ContextRef, ContextScope, EnvironmentRef, EnvironmentStatus, ResourceLocator,
};
use crate::environment::wsl::operations::atomic_file::WslAtomicDocumentIo;
use crate::environment::wsl::{EnvironmentRegistry, WslSession};
use crate::error::AppError;
use crate::storage::atomic_document::AtomicDocumentIo;
use crate::storage::lock_plan::load_lock_document;
use crate::{core::lossless_lock::LockSchema, core::lossless_lock::LosslessLockDocument};

pub trait AgentRegistrySnapshotSource: Send + Sync {
    fn snapshot(&self) -> Arc<AgentRegistrySnapshot>;
}

#[derive(Debug, Clone)]
pub struct HostRuntimeSnapshot {
    pub home: PathBuf,
    pub config_home: PathBuf,
    pub projects_path: PathBuf,
    pub global_lock_path: PathBuf,
    pub environment_variables: BTreeMap<String, String>,
}

#[derive(Clone)]
pub struct RuntimePlanningFactSource {
    registry: Arc<dyn AgentRegistrySnapshotSource>,
    environments: Arc<EnvironmentRegistry>,
    host: Arc<dyn HostRuntimeSource>,
}

impl RuntimePlanningFactSource {
    pub fn for_current_user(
        registry: Arc<dyn AgentRegistrySnapshotSource>,
        environments: Arc<EnvironmentRegistry>,
    ) -> Self {
        Self {
            registry,
            environments,
            host: Arc::new(SystemHostRuntimeSource),
        }
    }

    #[cfg(any(test, all(target_os = "windows", feature = "wsl-integration-tests")))]
    pub fn with_host_snapshot(
        registry: Arc<dyn AgentRegistrySnapshotSource>,
        environments: Arc<EnvironmentRegistry>,
        host: HostRuntimeSnapshot,
    ) -> Self {
        Self {
            registry,
            environments,
            host: Arc::new(StaticHostRuntimeSource(host)),
        }
    }

    async fn capture_install(
        &self,
        context: &ContextRef,
    ) -> Result<InstallPlanningFacts, AppError> {
        install_facts_from_base(self.capture_context_base(context).await?).await
    }

    async fn capture_context_base(&self, context: &ContextRef) -> Result<CapturedBase, AppError> {
        let registry = self.registry.snapshot();
        match &context.environment {
            EnvironmentRef::Host => self.capture_host_base(context, registry).await,
            EnvironmentRef::Wsl { distro_name } => {
                let context = context.clone();
                let registry = Arc::clone(&registry);
                self.environments
                    .with_session_retry(distro_name, move |session| {
                        let context = context.clone();
                        let registry = Arc::clone(&registry);
                        async move { capture_wsl_base(&context, registry, session).await }
                    })
                    .await
            }
        }
    }

    async fn capture_revisions(&self, context: &ContextRef) -> Result<RuntimeRevisions, AppError> {
        Ok(self.capture_context_base(context).await?.revisions)
    }

    async fn capture_host_base(
        &self,
        context: &ContextRef,
        registry: Arc<AgentRegistrySnapshot>,
    ) -> Result<CapturedBase, AppError> {
        let host = self.host.snapshot()?;
        let io = NativeAtomicDocumentIo;
        let projects = load_projects(
            &io,
            &ResourceLocator {
                environment: EnvironmentRef::Host,
                native_path: host.projects_path.to_string_lossy().into_owned(),
            },
            ProjectPathSemantics::host(),
        )
        .await?;
        let resolved = ContextResolver::resolve_host_from(
            context.clone(),
            host.home.clone(),
            host.global_lock_path.clone(),
            projects.projects.clone(),
        )?;
        let environment = host_environment_context(&resolved, &host);
        let targets =
            resolve_native_targets(&[resolved.skill_root.clone(), resolved.lock.clone()])?;
        build_base(
            resolved,
            environment,
            registry,
            projects.schema_version,
            targets,
        )
    }
}

impl InstallPlanningFactSource for RuntimePlanningFactSource {
    fn current<'a>(
        &'a self,
        context: &'a ContextRef,
    ) -> InstallFuture<'a, Result<InstallPlanningFacts, AppError>> {
        Box::pin(async move { self.capture_install(context).await })
    }
}

impl RuntimeRevisionSource for RuntimePlanningFactSource {
    fn current<'a>(
        &'a self,
        context: &'a ContextRef,
    ) -> BoxFuture<'a, Result<RuntimeRevisions, AppError>> {
        Box::pin(async move { self.capture_revisions(context).await })
    }

    fn snapshot<'a>(
        &'a self,
        context: &'a ContextRef,
    ) -> BoxFuture<'a, Result<RuntimeRevisionSnapshot, AppError>> {
        Box::pin(async move {
            let base = self.capture_context_base(context).await?;
            Ok(RuntimeRevisionSnapshot {
                revisions: base.revisions,
                authority: base.authority_revisions,
            })
        })
    }
}

trait HostRuntimeSource: Send + Sync {
    fn snapshot(&self) -> Result<HostRuntimeSnapshot, AppError>;
}

struct SystemHostRuntimeSource;

impl HostRuntimeSource for SystemHostRuntimeSource {
    fn snapshot(&self) -> Result<HostRuntimeSnapshot, AppError> {
        let home = dirs::home_dir().ok_or_else(|| AppError::Path {
            message: "cannot resolve home directory".to_string(),
        })?;
        Ok(HostRuntimeSnapshot {
            home,
            config_home: PATHS.config_home.clone(),
            projects_path: get_config_path()?.with_file_name("projects.json"),
            global_lock_path: skill_lock::get_skill_lock_path(),
            environment_variables: std::env::vars().collect(),
        })
    }
}

#[cfg(any(test, all(target_os = "windows", feature = "wsl-integration-tests")))]
struct StaticHostRuntimeSource(HostRuntimeSnapshot);

#[cfg(any(test, all(target_os = "windows", feature = "wsl-integration-tests")))]
impl HostRuntimeSource for StaticHostRuntimeSource {
    fn snapshot(&self) -> Result<HostRuntimeSnapshot, AppError> {
        Ok(self.0.clone())
    }
}

struct CapturedBase {
    resolved_context: ResolvedContext,
    environment_context: EnvironmentContext,
    registry: Arc<AgentRegistrySnapshot>,
    revisions: RuntimeRevisions,
    authority_revisions: RuntimeAuthorityRevisions,
    lock_schema: LockSchema,
}

async fn capture_wsl_base(
    context: &ContextRef,
    registry: Arc<AgentRegistrySnapshot>,
    session: WslSession,
) -> Result<CapturedBase, AppError> {
    let io = WslAtomicDocumentIo::new(session.clone());
    let (resolved, project_schema_version) =
        resolve_wsl_context_from_io(&io, context, &session).await?;
    let environment = wsl_environment_context(&resolved, session.clone());
    let targets = resolve_wsl_targets(
        &session,
        &[
            resolved.skill_root.native_path.clone(),
            resolved.lock.native_path.clone(),
        ],
        None,
    )
    .await?;
    build_base(
        resolved,
        environment,
        registry,
        project_schema_version,
        targets,
    )
}

async fn resolve_wsl_context_from_io<I>(
    io: &I,
    context: &ContextRef,
    session: &WslSession,
) -> Result<(ResolvedContext, u32), AppError>
where
    I: AtomicDocumentIo + ?Sized,
{
    let projects = load_projects(
        io,
        &ResourceLocator {
            environment: context.environment.clone(),
            native_path: format!(
                "{}/.skill-deck/projects.json",
                session.home.trim_end_matches('/')
            ),
        },
        ProjectPathSemantics::Posix,
    )
    .await?;
    let resolved = ContextResolver::resolve_wsl_from_projects(
        context.clone(),
        session,
        projects.projects.clone(),
    )?;
    Ok((resolved, projects.schema_version))
}

fn build_base(
    resolved_context: ResolvedContext,
    environment_context: EnvironmentContext,
    registry: Arc<AgentRegistrySnapshot>,
    project_schema_version: u32,
    target_facts: Vec<crate::environment::planning::ResolvedTargetFact>,
) -> Result<CapturedBase, AppError> {
    if target_facts.len() != 2 {
        return Err(AppError::StaleContext);
    }
    let storage_mapping_identity = stable_digest(&(&target_facts[0].key, &target_facts[1].key))?;
    let resolved_project_identity = match resolved_context.project.as_ref() {
        Some(project) => Some(PhysicalProjectIdentity {
            owner: resolved_context.context.environment.clone(),
            stable_id: stable_digest(&(&project.id, &target_facts[0].key))?,
        }),
        None => None,
    };
    let context_revision = context_snapshot_revision(&ContextRevisionInput {
        context: resolved_context.context.clone(),
        selected_project: resolved_context.project.clone(),
        owned_root_fields: ContextOwnedRootFields {
            schema_version: project_schema_version,
            storage_mapping_fields: BTreeMap::from([(
                "pathSemantics".to_string(),
                path_semantics(&resolved_context.context.environment).to_string(),
            )]),
        },
        resolved_project_identity: resolved_project_identity.clone(),
        canonical_root_identity: target_facts[0].key.physical_parent.clone(),
        lock_parent_identity: target_facts[1].key.physical_parent.clone(),
        storage_mapping_identity,
    })?;
    let revisions = RuntimeRevisions {
        registry: registry.revision.clone(),
        environment: environment_context.revision.clone(),
        context: context_revision,
    };
    let authority_revisions = RuntimeAuthorityRevisions {
        registry: registry.revision.clone(),
        environment: environment_context.revision.clone(),
        context: context_authority_revision(&resolved_context, project_schema_version)?,
    };
    let lock_schema = match resolved_context.context.scope {
        ContextScope::Global => LockSchema::Global,
        ContextScope::Project { .. } => LockSchema::Project,
    };
    Ok(CapturedBase {
        resolved_context,
        environment_context,
        registry,
        revisions,
        authority_revisions,
        lock_schema,
    })
}

fn context_authority_revision(
    resolved: &ResolvedContext,
    project_schema_version: u32,
) -> Result<String, AppError> {
    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct SelectedProject<'a> {
        id: &'a str,
        native_path: &'a str,
    }

    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct AuthorityProjection<'a> {
        format_version: u32,
        context: &'a ContextRef,
        selected_project: Option<SelectedProject<'a>>,
        project_schema_version: u32,
        path_semantics: &'static str,
        home: &'a str,
        canonical_root: &'a str,
        lock: &'a str,
    }

    let projection = AuthorityProjection {
        format_version: 1,
        context: &resolved.context,
        selected_project: resolved.project.as_ref().map(|project| SelectedProject {
            id: &project.id,
            native_path: &project.native_path,
        }),
        project_schema_version,
        path_semantics: path_semantics(&resolved.context.environment),
        home: &resolved.home.native_path,
        canonical_root: &resolved.skill_root.native_path,
        lock: &resolved.lock.native_path,
    };
    stable_digest(&projection)
}

async fn install_facts_from_base(base: CapturedBase) -> Result<InstallPlanningFacts, AppError> {
    let project_path = base
        .resolved_context
        .project
        .as_ref()
        .map(|project| project.native_path.as_str());
    let agent_runtime =
        AgentEnvironmentResolver::from_environment(base.environment_context.clone())
            .resolve_registry(&base.registry, project_path)
            .await?;
    let lock_document = load_current_lock(&base).await?;
    Ok(InstallPlanningFacts {
        resolved_context: base.resolved_context,
        agent_runtime,
        revisions: base.revisions,
        lock_schema: base.lock_schema,
        lock_document,
    })
}

async fn load_current_lock(base: &CapturedBase) -> Result<LosslessLockDocument, AppError> {
    match &base.resolved_context.context.environment {
        EnvironmentRef::Host => {
            load_lock_document(
                &NativeAtomicDocumentIo,
                &base.resolved_context.lock,
                None,
                base.lock_schema,
            )
            .await
        }
        EnvironmentRef::Wsl { .. } => {
            let session = base
                .environment_context
                .wsl_session
                .clone()
                .ok_or(AppError::StaleEnvironment)?;
            load_lock_document(
                &WslAtomicDocumentIo::new(session),
                &base.resolved_context.lock,
                None,
                base.lock_schema,
            )
            .await
        }
    }
}

async fn load_projects<I>(
    io: &I,
    target: &ResourceLocator,
    semantics: ProjectPathSemantics,
) -> Result<ProjectsFile, AppError>
where
    I: AtomicDocumentIo + ?Sized,
{
    let current = ProjectsFile::new(Vec::new(), semantics);
    let Some(bytes) = io.read_optional(target).await? else {
        return Ok(current);
    };
    let parsed: ProjectsFile = serde_json::from_slice(&bytes)?;
    if parsed.schema_version > current.schema_version {
        return Err(AppError::ConfigurationCorrupted {
            message: format!(
                "projects schema version {} is newer than {}",
                parsed.schema_version, current.schema_version
            ),
        });
    }
    Ok(parsed)
}

fn host_environment_context(
    resolved: &ResolvedContext,
    host: &HostRuntimeSnapshot,
) -> EnvironmentContext {
    let revision = environment_revision(
        "host",
        &(
            &resolved.home.native_path,
            host.config_home.to_string_lossy(),
            &host.environment_variables,
        ),
    );
    EnvironmentContext {
        environment: EnvironmentRef::Host,
        home: resolved.home.native_path.clone(),
        config_home: host.config_home.to_string_lossy().into_owned(),
        environment_variables: host.environment_variables.clone(),
        availability: EnvironmentStatus::Available,
        revision,
        wsl_session: None,
    }
}

fn wsl_environment_context(resolved: &ResolvedContext, session: WslSession) -> EnvironmentContext {
    let revision = environment_revision("wsl", &(session.runtime_generation, &session));
    EnvironmentContext {
        environment: resolved.context.environment.clone(),
        home: session.home.clone(),
        config_home: session.config_home.clone(),
        environment_variables: session.environment.clone(),
        availability: EnvironmentStatus::Available,
        revision,
        wsl_session: Some(session),
    }
}

fn environment_revision(value_kind: &str, value: &impl Serialize) -> String {
    let mut hasher = Sha256::new();
    hasher.update(value_kind.as_bytes());
    hasher.update(
        serde_json::to_vec(value)
            .expect("environment revision inputs must serialize deterministically"),
    );
    format!("{:x}", hasher.finalize())
}

fn path_semantics(environment: &EnvironmentRef) -> &'static str {
    match environment {
        EnvironmentRef::Host if cfg!(windows) => "native-windows-v1",
        EnvironmentRef::Host => "native-unix-v1",
        EnvironmentRef::Wsl { .. } => "wsl-posix-v1",
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::fs;
    use std::sync::{Arc, Mutex};

    use tempfile::tempdir;

    use super::*;
    use crate::application::install_planner::InstallPlanningFactSource;
    use crate::application::mutation::coordinator::RuntimeRevisionSource;
    use crate::core::agent_definition::{
        AgentAdapter, AgentDefinition, AgentId, AgentSource, DetectionSpec, PathSpec,
        ScopeDefinition,
    };
    use crate::core::agent_registry::AgentRegistrySnapshot;
    use crate::environment::types::{ContextRef, ContextScope, EnvironmentRef};
    use crate::environment::wsl::{EnvironmentRegistry, WslSession};
    use crate::storage::atomic_document::{AtomicDocumentIo, IoFuture};

    struct StaticRegistry(Arc<AgentRegistrySnapshot>);

    impl AgentRegistrySnapshotSource for StaticRegistry {
        fn snapshot(&self) -> Arc<AgentRegistrySnapshot> {
            Arc::clone(&self.0)
        }
    }

    struct RecordingDocumentIo {
        bytes: Vec<u8>,
        reads: Mutex<Vec<String>>,
    }

    impl AtomicDocumentIo for RecordingDocumentIo {
        fn read_optional<'a>(
            &'a self,
            target: &'a crate::environment::types::ResourceLocator,
        ) -> IoFuture<'a, Result<Option<Vec<u8>>, AppError>> {
            Box::pin(async move {
                self.reads.lock().unwrap().push(target.native_path.clone());
                Ok(Some(self.bytes.clone()))
            })
        }

        fn write_atomic<'a>(
            &'a self,
            _target: &'a crate::environment::types::ResourceLocator,
            _bytes: Vec<u8>,
        ) -> IoFuture<'a, Result<(), AppError>> {
            Box::pin(async { panic!("context capture is read-only") })
        }
    }

    fn registry_snapshot() -> AgentRegistrySnapshot {
        let id = AgentId::parse("demo-agent").unwrap();
        AgentRegistrySnapshot {
            revision: "registry-1".to_string(),
            active_definitions: BTreeMap::from([(
                id.clone(),
                AgentDefinition {
                    id,
                    display_name: "Demo Agent".to_string(),
                    source: AgentSource::Builtin,
                    aliases: Vec::new(),
                    global: ScopeDefinition {
                        enabled: true,
                        reads_shared: true,
                        private_path: None,
                    },
                    project: ScopeDefinition {
                        enabled: true,
                        reads_shared: true,
                        private_path: None,
                    },
                    detection: DetectionSpec::AnyPathExists {
                        paths: vec![PathSpec::home(".demo-agent")],
                    },
                    legacy_paths: Vec::new(),
                    adapter: AgentAdapter::Standard,
                },
            )]),
        }
    }

    fn projects_json(selected_path: &str, unrelated_path: &str) -> String {
        serde_json::json!({
            "schemaVersion": 1,
            "projects": [
                {
                    "id": "selected",
                    "nativePath": selected_path,
                    "displayName": "Selected",
                    "order": 0,
                    "suppressCrossStorageWarning": false
                },
                {
                    "id": "unrelated",
                    "nativePath": unrelated_path,
                    "displayName": "Unrelated",
                    "order": 1,
                    "suppressCrossStorageWarning": false
                }
            ]
        })
        .to_string()
    }

    #[tokio::test]
    async fn host_planning_and_execute_revisions_share_one_scoped_fact_model() {
        let temp = tempdir().unwrap();
        let home = temp.path().join("home");
        let selected = temp.path().join("selected");
        let changed = temp.path().join("changed");
        let unrelated = temp.path().join("unrelated");
        let projects_path = temp.path().join("state/projects.json");
        let global_lock_path = temp.path().join("state/global-lock.json");
        for path in [&home, &selected, &changed, &unrelated] {
            fs::create_dir_all(path).unwrap();
        }
        fs::create_dir_all(projects_path.parent().unwrap()).unwrap();
        fs::write(
            &projects_path,
            projects_json(&selected.to_string_lossy(), &unrelated.to_string_lossy()),
        )
        .unwrap();
        fs::write(
            selected.join("skills-lock.json"),
            br#"{"version":1,"skills":{"demo":{"source":"owner/repo","unknown":"kept"}}}"#,
        )
        .unwrap();

        let registry = Arc::new(StaticRegistry(Arc::new(registry_snapshot())));
        let source = RuntimePlanningFactSource::with_host_snapshot(
            registry,
            Arc::new(EnvironmentRegistry::default()),
            HostRuntimeSnapshot {
                home: home.clone(),
                config_home: temp.path().join("config"),
                projects_path: projects_path.clone(),
                global_lock_path,
                environment_variables: BTreeMap::new(),
            },
        );
        let context = ContextRef {
            environment: EnvironmentRef::Host,
            scope: ContextScope::Project {
                project_id: "selected".to_string(),
            },
        };

        let facts = InstallPlanningFactSource::current(&source, &context)
            .await
            .unwrap();
        let execute_revisions = RuntimeRevisionSource::current(&source, &context)
            .await
            .unwrap();

        assert_eq!(facts.revisions, execute_revisions);
        assert_eq!(facts.agent_runtime.registry_revision, "registry-1");
        assert_eq!(
            facts.resolved_context.skill_root.native_path,
            selected.join(".agents").join("skills").to_string_lossy()
        );
        assert_eq!(
            facts
                .lock_document
                .entry_snapshot("demo")
                .value()
                .and_then(|entry| entry.get("unknown"))
                .and_then(serde_json::Value::as_str),
            Some("kept")
        );

        fs::write(
            &projects_path,
            projects_json(
                &selected.to_string_lossy(),
                &temp.path().join("unrelated-renamed").to_string_lossy(),
            ),
        )
        .unwrap();
        let after_unrelated_change = RuntimeRevisionSource::current(&source, &context)
            .await
            .unwrap();
        assert_eq!(execute_revisions, after_unrelated_change);

        fs::write(
            &projects_path,
            projects_json(&changed.to_string_lossy(), &unrelated.to_string_lossy()),
        )
        .unwrap();
        let after_selected_change = RuntimeRevisionSource::current(&source, &context)
            .await
            .unwrap();
        assert_ne!(execute_revisions.context, after_selected_change.context);
    }

    #[tokio::test]
    async fn wsl_context_reads_projects_through_typed_document_io() {
        let io = RecordingDocumentIo {
            bytes: projects_json("/work/app", "/work/other").into_bytes(),
            reads: Mutex::new(Vec::new()),
        };
        let context = ContextRef {
            environment: EnvironmentRef::Wsl {
                distro_name: "Ubuntu".to_string(),
            },
            scope: ContextScope::Project {
                project_id: "selected".to_string(),
            },
        };
        let session = WslSession {
            distro_name: "Ubuntu".to_string(),
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
        };

        let (resolved, schema_version) = resolve_wsl_context_from_io(&io, &context, &session)
            .await
            .unwrap();

        assert_eq!(schema_version, 1);
        assert_eq!(resolved.skill_root.native_path, "/work/app/.agents/skills");
        assert_eq!(
            *io.reads.lock().unwrap(),
            vec!["/home/alice/.skill-deck/projects.json"]
        );

        let first = wsl_environment_context(
            &resolved,
            WslSession {
                runtime_generation: 1,
                ..session.clone()
            },
        );
        let second = wsl_environment_context(
            &resolved,
            WslSession {
                runtime_generation: 2,
                ..session
            },
        );
        assert_ne!(first.revision, second.revision);
    }
}

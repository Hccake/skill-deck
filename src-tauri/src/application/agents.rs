// list_agents command
// 对应 CLI: detectInstalledAgents + getAgentConfig

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, RwLock};

use crate::application::agent_registry_source::AgentRegistrySnapshotSource;
use crate::application::runtime_admission::RuntimeAdmissionCoordinator;
use crate::application::skill_libraries::LibraryUsage;
use crate::core::agent_definition::{
    AgentFieldError, AgentId, AgentSource, CustomAgentDefinition, PathSpec, ScopeDefinition,
};
use crate::core::agent_registry::{AgentRegistry, AgentRegistrySnapshot};
use crate::core::agent_settings::{AgentSettingsSnapshot, AgentStorageIssue, CustomAgentRecord};
use crate::core::custom_agent_repository::CustomAgentRepository;
use crate::core::mutation::MutationKind;
use crate::core::paths::PATHS;
use crate::environment::agent_environment::{
    AgentEnvironmentResolver, AgentRuntimeSnapshot, DetectionReason, DirectoryPresenceState,
    EnvironmentContext, ResolvedAgent, ResolvedAgentScope,
};
use crate::environment::context_resolver::{ContextResolver, ResolvedContext};
use crate::environment::directory_inspection::{inspect_native, inspect_wsl, DirectoryInspection};
use crate::environment::types::{EnvironmentRef, EnvironmentStatus, SkillLocationRef};
use crate::environment::wsl::{WslRuntime, WslSession};
use crate::error::AppError;
use crate::models::Scope;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use specta::Type;

const CUSTOM_AGENT_STORAGE_UNAVAILABLE_CODE: &str = "customAgentStorageUnavailable";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
#[specta(tag = "kind", rename_all = "camelCase")]
pub enum AgentCommandError {
    Application { error: AppError },
    InvalidDraft { errors: Vec<AgentFieldError> },
    StaleRegistryRevision { expected: String, actual: String },
}

impl From<AppError> for AgentCommandError {
    fn from(error: AppError) -> Self {
        Self::Application { error }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct CustomAgentDraftValidation {
    pub registry_revision: String,
    pub environment_revision: String,
    pub environment: EnvironmentRef,
    pub resolved: ResolvedAgent,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct AgentDeleteResult {
    pub settings: AgentSettingsSnapshot,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct AgentDeleteScopeImpact {
    pub scope: Scope,
    pub paths: Vec<AgentDeletePathImpact>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub enum AgentDeletePathKind {
    Standard,
    Private,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct AgentDeletePathImpact {
    pub kind: AgentDeletePathKind,
    pub logical_path: PathSpec,
    pub resolved_path: Option<String>,
    pub presence: DirectoryPresenceState,
    pub observed_skill_count: Option<u32>,
    pub observed_skill_count_truncated: bool,
    pub unavailable_reason: Option<DetectionReason>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct AgentDeleteImpact {
    pub agent_id: AgentId,
    pub display_name: String,
    pub registry_revision: String,
    pub environment_revision: String,
    pub scopes: Vec<AgentDeleteScopeImpact>,
    pub loses_management_capability: bool,
    pub files_will_be_deleted: bool,
    pub library_usages: Vec<LibraryUsage>,
}

#[derive(Clone)]
pub struct ManagedAgentRegistry {
    repository: Option<CustomAgentRepository>,
    builtin_registry: Arc<AgentRegistry>,
    registry: Arc<RwLock<Arc<AgentRegistry>>>,
    custom_storage_error: Arc<RwLock<Option<AppError>>>,
    mutation_lock: Arc<Mutex<()>>,
}

impl AgentRegistrySnapshotSource for ManagedAgentRegistry {
    fn snapshot(&self) -> Arc<AgentRegistrySnapshot> {
        self.registry_snapshot(true)
    }
}

struct CapturedCustomDelete {
    registry: Arc<AgentRegistry>,
    definition: CustomAgentDefinition,
}

impl ManagedAgentRegistry {
    pub fn for_current_user() -> Self {
        Self::from_repository_initializer(CustomAgentRepository::for_current_user)
    }

    fn from_repository_initializer(
        initialize: impl FnOnce() -> Result<CustomAgentRepository, AppError>,
    ) -> Self {
        match initialize() {
            Ok(repository) => Self::from_repository(repository),
            Err(error) => Self::from_unavailable_repository(error),
        }
    }

    fn from_unavailable_repository(error: AppError) -> Self {
        let registry = Arc::new(AgentRegistry::empty_unavailable());
        Self {
            repository: None,
            builtin_registry: Arc::clone(&registry),
            registry: Arc::new(RwLock::new(registry)),
            custom_storage_error: Arc::new(RwLock::new(Some(error))),
            mutation_lock: Arc::new(Mutex::new(())),
        }
    }

    fn from_repository(repository: CustomAgentRepository) -> Self {
        match repository.load() {
            Ok(file) => Self::from_loaded_state(Some(repository), file.records, None),
            Err(error) => Self::from_loaded_state(Some(repository), Vec::new(), Some(error)),
        }
    }

    fn from_loaded_state(
        repository: Option<CustomAgentRepository>,
        records: Vec<CustomAgentRecord>,
        custom_storage_error: Option<AppError>,
    ) -> Self {
        Self {
            repository,
            builtin_registry: Arc::new(AgentRegistry::new(Vec::new())),
            registry: Arc::new(RwLock::new(Arc::new(AgentRegistry::new(records)))),
            custom_storage_error: Arc::new(RwLock::new(custom_storage_error)),
            mutation_lock: Arc::new(Mutex::new(())),
        }
    }

    fn registry(&self) -> Arc<AgentRegistry> {
        Arc::clone(
            &self
                .registry
                .read()
                .expect("managed agent registry read lock poisoned"),
        )
    }

    fn registry_snapshot(&self, include_custom: bool) -> Arc<AgentRegistrySnapshot> {
        let registry = if include_custom {
            self.registry()
        } else {
            Arc::clone(&self.builtin_registry)
        };
        Arc::new(registry.snapshot().clone())
    }

    fn settings_snapshot(&self, environment: EnvironmentRef) -> AgentSettingsSnapshot {
        let registry = self.registry();
        let mut snapshot = registry
            .settings_records()
            .snapshot(registry.snapshot().revision.clone(), environment);
        snapshot.custom_storage_issue =
            self.custom_storage_error().map(|error| AgentStorageIssue {
                code: CUSTOM_AGENT_STORAGE_UNAVAILABLE_CODE.to_string(),
                message: error.to_string(),
                read_only: true,
            });
        snapshot
    }

    pub(crate) fn active_custom_definition(&self, id: &AgentId) -> Option<CustomAgentDefinition> {
        self.registry()
            .settings_records()
            .active_custom
            .iter()
            .find(|agent| &agent.definition.id == id)
            .map(|agent| agent.definition.clone())
    }

    fn preview_registry(
        &self,
        definition: CustomAgentDefinition,
    ) -> Result<AgentRegistry, AgentCommandError> {
        if self.repository.is_none() {
            self.repository()?;
        }
        validate_draft(&definition)?;
        self.reject_builtin_collision(&definition.id)?;
        let mut records = if self.custom_storage_error().is_some() {
            Vec::new()
        } else {
            self.repository()?.load()?.records
        };
        let raw = serde_json::to_value(&definition).map_err(AppError::from)?;
        if let Some(record) = records.iter_mut().find(|record| {
            matches!(
                record,
                CustomAgentRecord::Valid {
                    definition: existing,
                    ..
                } if existing.id == definition.id
            )
        }) {
            *record = CustomAgentRecord::Valid { definition, raw };
        } else {
            records.push(CustomAgentRecord::Valid { definition, raw });
        }
        Ok(AgentRegistry::new(records))
    }

    fn custom_storage_error(&self) -> Option<AppError> {
        self.custom_storage_error
            .read()
            .expect("managed agent storage error read lock poisoned")
            .clone()
    }

    fn repository(&self) -> Result<&CustomAgentRepository, AgentCommandError> {
        if let Some(error) = self.custom_storage_error() {
            return Err(AgentCommandError::Application { error });
        }
        self.repository
            .as_ref()
            .ok_or_else(|| AgentCommandError::Application {
                error: AppError::Path {
                    message: "custom agent repository path is unavailable".to_string(),
                },
            })
    }

    fn reject_builtin_collision(&self, id: &AgentId) -> Result<(), AgentCommandError> {
        if self
            .builtin_registry
            .snapshot()
            .get(id)
            .is_some_and(|definition| definition.source == AgentSource::Builtin)
        {
            return Err(AgentCommandError::InvalidDraft {
                errors: vec![AgentFieldError::new("id", "duplicateAgentId")],
            });
        }
        Ok(())
    }

    fn reject_custom_collision(
        &self,
        id: &AgentId,
        original_id: Option<&AgentId>,
    ) -> Result<(), AgentCommandError> {
        if original_id != Some(id) && self.registry().snapshot().get(id).is_some() {
            return Err(AgentCommandError::InvalidDraft {
                errors: vec![AgentFieldError::new("id", "duplicateAgentId")],
            });
        }
        Ok(())
    }

    fn assert_revision(&self, expected: &str) -> Result<(), AgentCommandError> {
        let actual = self.registry().snapshot().revision.clone();
        if expected != actual {
            return Err(AgentCommandError::StaleRegistryRevision {
                expected: expected.to_string(),
                actual,
            });
        }
        Ok(())
    }

    fn preflight_save(
        &self,
        definition: &CustomAgentDefinition,
        original_id: Option<&AgentId>,
        expected_registry_revision: &str,
    ) -> Result<(), AgentCommandError> {
        self.repository()?;
        validate_draft(definition)?;
        self.reject_builtin_collision(&definition.id)?;
        self.reject_custom_collision(&definition.id, original_id)?;
        self.assert_revision(expected_registry_revision)
    }

    fn preflight_delete(
        &self,
        id: &AgentId,
        expected_registry_revision: &str,
    ) -> Result<CapturedCustomDelete, AgentCommandError> {
        self.repository()?;
        self.capture_custom_delete(id, expected_registry_revision)
    }

    fn preview_delete_definition(
        &self,
        id: &AgentId,
        expected_registry_revision: &str,
    ) -> Result<CapturedCustomDelete, AgentCommandError> {
        self.preflight_delete(id, expected_registry_revision)
    }

    fn capture_custom_delete(
        &self,
        id: &AgentId,
        expected_registry_revision: &str,
    ) -> Result<CapturedCustomDelete, AgentCommandError> {
        let registry = self.registry();
        let actual = registry.snapshot().revision.clone();
        if expected_registry_revision != actual {
            return Err(AgentCommandError::StaleRegistryRevision {
                expected: expected_registry_revision.to_string(),
                actual,
            });
        }
        let definition = registry
            .settings_records()
            .active_custom
            .iter()
            .map(|record| &record.definition)
            .chain(
                registry
                    .settings_records()
                    .disabled_conflicts
                    .iter()
                    .map(|record| &record.definition),
            )
            .find(|definition| definition.id == *id)
            .cloned()
            .ok_or_else(|| AgentCommandError::Application {
                error: AppError::InvalidAgent {
                    agent: id.to_string(),
                },
            })?;
        Ok(CapturedCustomDelete {
            registry,
            definition,
        })
    }

    fn save(
        &self,
        definition: CustomAgentDefinition,
        original_id: Option<&AgentId>,
        expected_registry_revision: &str,
    ) -> Result<(), AgentCommandError> {
        let _mutation = self
            .mutation_lock
            .lock()
            .expect("managed agent registry mutation lock poisoned");
        let repository = self.repository()?;
        validate_draft(&definition)?;
        self.reject_builtin_collision(&definition.id)?;
        self.reject_custom_collision(&definition.id, original_id)?;
        self.assert_revision(expected_registry_revision)?;
        let file = repository.upsert(definition)?;
        *self
            .registry
            .write()
            .expect("managed agent registry write lock poisoned") =
            Arc::new(AgentRegistry::new(file.records));
        Ok(())
    }

    fn delete(
        &self,
        id: &AgentId,
        expected_registry_revision: &str,
    ) -> Result<(), AgentCommandError> {
        let _mutation = self
            .mutation_lock
            .lock()
            .expect("managed agent registry mutation lock poisoned");
        let repository = self.repository()?;
        self.capture_custom_delete(id, expected_registry_revision)?;
        let file = repository.delete(id)?;
        *self
            .registry
            .write()
            .expect("managed agent registry write lock poisoned") =
            Arc::new(AgentRegistry::new(file.records));
        Ok(())
    }

    fn delete_invalid(
        &self,
        index: usize,
        expected_registry_revision: &str,
    ) -> Result<(), AgentCommandError> {
        let _mutation = self
            .mutation_lock
            .lock()
            .expect("managed agent registry mutation lock poisoned");
        let repository = self.repository()?;
        self.assert_revision(expected_registry_revision)?;
        let file = repository.delete_invalid(index)?;
        *self
            .registry
            .write()
            .expect("managed agent registry write lock poisoned") =
            Arc::new(AgentRegistry::new(file.records));
        Ok(())
    }
}

fn validate_draft(definition: &CustomAgentDefinition) -> Result<(), AgentCommandError> {
    definition
        .validate()
        .map_err(|error| AgentCommandError::InvalidDraft {
            errors: vec![error],
        })
}

pub async fn list_agents(
    context: SkillLocationRef,
    environment_registry: &WslRuntime,
    agent_registry: &ManagedAgentRegistry,
) -> Result<AgentRuntimeSnapshot, AgentCommandError> {
    match &context.environment {
        EnvironmentRef::Native => {
            let resolved = ContextResolver::resolve_native(context)?;
            let project_path = resolved
                .project
                .as_ref()
                .map(|project| project.native_path.clone());
            let environment = native_environment_context(&resolved);
            list_agents_dynamic(agent_registry, environment, project_path.as_deref())
                .await
                .map_err(AgentCommandError::from)
        }
        EnvironmentRef::Wsl { distro_name } => {
            let distro_name = distro_name.clone();
            let workspace = environment_registry.workspace(&distro_name)?;
            let retry_context = context.clone();
            let service = agent_registry;
            environment_registry
                .with_session_retry(&distro_name, move |session| {
                    let workspace = workspace.clone();
                    let context = retry_context.clone();
                    async move {
                        let resolved =
                            ContextResolver::resolve_wsl(context.clone(), &session).await?;
                        let project_path = resolved
                            .project
                            .as_ref()
                            .map(|project| project.native_path.clone());
                        let environment =
                            wsl_environment_context(&resolved, session.clone(), workspace);
                        let snapshot = service.registry_snapshot(true);
                        AgentEnvironmentResolver::from_active_wsl_session(environment, session)
                            .resolve_registry(&snapshot, project_path.as_deref())
                            .await
                    }
                })
                .await
                .map_err(AgentCommandError::from)
        }
    }
}

#[cfg(test)]
pub(crate) fn assert_runtime_revisions_match(
    captured: &AgentRuntimeSnapshot,
    current: &AgentRuntimeSnapshot,
) -> Result<(), AppError> {
    if captured.registry_revision == current.registry_revision
        && captured.environment_revision == current.environment_revision
    {
        return Ok(());
    }
    Err(AppError::StaleAgentRuntime {
        expected_registry_revision: captured.registry_revision.clone(),
        actual_registry_revision: current.registry_revision.clone(),
        expected_environment_revision: captured.environment_revision.clone(),
        actual_environment_revision: current.environment_revision.clone(),
    })
}

#[cfg(test)]
async fn assert_runtime_snapshot_current_with<C, Fut>(
    captured: &AgentRuntimeSnapshot,
    capture: C,
) -> Result<(), AppError>
where
    C: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Result<AgentRuntimeSnapshot, AppError>>,
{
    let current = capture().await?;
    assert_runtime_revisions_match(captured, &current)
}

pub fn get_agent_settings_snapshot(
    context: SkillLocationRef,
    registry: &ManagedAgentRegistry,
) -> AgentSettingsSnapshot {
    settings_snapshot(registry, context.environment)
}

pub async fn validate_custom_agent_draft(
    context: SkillLocationRef,
    draft: CustomAgentDefinition,
    environment_registry: &WslRuntime,
    agent_registry: &ManagedAgentRegistry,
) -> Result<CustomAgentDraftValidation, AgentCommandError> {
    let draft_id = draft.id.clone();
    let preview_snapshot = agent_registry.preview_registry(draft)?.snapshot().clone();
    match &context.environment {
        EnvironmentRef::Native => {
            let resolved = ContextResolver::resolve_native(context)?;
            let project_path = resolved
                .project
                .as_ref()
                .map(|project| project.native_path.clone());
            resolve_custom_agent_preview(
                &preview_snapshot,
                &draft_id,
                native_environment_context(&resolved),
                project_path.as_deref(),
            )
            .await
            .map_err(AgentCommandError::from)
        }
        EnvironmentRef::Wsl { distro_name } => {
            let distro_name = distro_name.clone();
            let workspace = environment_registry.workspace(&distro_name)?;
            let retry_context = context.clone();
            environment_registry
                .with_session_retry(&distro_name, move |session| {
                    let workspace = workspace.clone();
                    let context = retry_context.clone();
                    let preview_snapshot = preview_snapshot.clone();
                    let draft_id = draft_id.clone();
                    async move {
                        let resolved = ContextResolver::resolve_wsl(context, &session).await?;
                        let project_path = resolved
                            .project
                            .as_ref()
                            .map(|project| project.native_path.clone());
                        resolve_custom_agent_preview_with_resolver(
                            &preview_snapshot,
                            &draft_id,
                            AgentEnvironmentResolver::from_active_wsl_session(
                                wsl_environment_context(&resolved, session.clone(), workspace),
                                session,
                            ),
                            project_path.as_deref(),
                        )
                        .await
                    }
                })
                .await
                .map_err(AgentCommandError::from)
        }
    }
}

pub fn save_custom_agent(
    context: SkillLocationRef,
    draft: CustomAgentDefinition,
    original_id: Option<AgentId>,
    expected_registry_revision: String,
    registry: &ManagedAgentRegistry,
    controller: &RuntimeAdmissionCoordinator,
) -> Result<AgentSettingsSnapshot, AgentCommandError> {
    save_custom_agent_with_original_id_controller(
        registry,
        controller,
        context,
        draft,
        original_id,
        expected_registry_revision,
    )
}

pub fn delete_custom_agent(
    context: SkillLocationRef,
    id: AgentId,
    expected_registry_revision: String,
    registry: &ManagedAgentRegistry,
    controller: &RuntimeAdmissionCoordinator,
) -> Result<AgentDeleteResult, AgentCommandError> {
    delete_custom_agent_with_controller_result(
        registry,
        controller,
        context,
        id,
        expected_registry_revision,
    )
}

pub async fn delete_invalid_custom_agent(
    context: SkillLocationRef,
    index: u32,
    expected_registry_revision: String,
    registry: &ManagedAgentRegistry,
    controller: &RuntimeAdmissionCoordinator,
) -> Result<AgentDeleteResult, AgentCommandError> {
    delete_invalid_custom_agent_with_controller(
        registry,
        controller,
        context,
        index,
        expected_registry_revision,
    )
}

fn delete_invalid_custom_agent_with_controller(
    registry: &ManagedAgentRegistry,
    controller: &RuntimeAdmissionCoordinator,
    context: SkillLocationRef,
    index: u32,
    expected_registry_revision: String,
) -> Result<AgentDeleteResult, AgentCommandError> {
    registry.repository()?;
    registry.assert_revision(&expected_registry_revision)?;
    let _guard =
        controller.begin_mutation(MutationKind::ManageAgentDefinitions, context.clone())?;
    registry.delete_invalid(
        usize::try_from(index).expect("custom Agent record index must fit in usize"),
        &expected_registry_revision,
    )?;
    Ok(AgentDeleteResult {
        settings: registry.settings_snapshot(context.environment),
    })
}

pub async fn preview_custom_agent_delete(
    context: SkillLocationRef,
    id: AgentId,
    expected_registry_revision: String,
    registry: &ManagedAgentRegistry,
    environment_registry: &WslRuntime,
) -> Result<AgentDeleteImpact, AgentCommandError> {
    let captured = registry.preview_delete_definition(&id, &expected_registry_revision)?;
    let definition = captured.definition;
    let snapshot = delete_preview_snapshot(&definition, &captured.registry.snapshot().revision)?;
    match &context.environment {
        EnvironmentRef::Native => {
            let resolved = ContextResolver::resolve_native(context)?;
            let project_path = resolved
                .project
                .as_ref()
                .map(|project| project.native_path.as_str());
            let runtime =
                AgentEnvironmentResolver::from_environment(native_environment_context(&resolved))
                    .resolve_registry(&snapshot, project_path)
                    .await?;
            let inspections =
                inspect_native(&delete_impact_resolved_paths(&runtime, &definition.id)).await;
            Ok(build_delete_impact(
                &runtime,
                definition.id,
                definition.display_name,
                &inspections,
            ))
        }
        EnvironmentRef::Wsl { distro_name } => {
            let distro_name = distro_name.clone();
            let workspace = environment_registry.workspace(&distro_name)?;
            environment_registry
                .with_session_retry(&distro_name, move |session| {
                    let workspace = workspace.clone();
                    let context = context.clone();
                    let definition = definition.clone();
                    let snapshot = snapshot.clone();
                    async move {
                        let agent_id = definition.id.clone();
                        let display_name = definition.display_name.clone();
                        let resolved = ContextResolver::resolve_wsl(context, &session).await?;
                        let project_path = resolved
                            .project
                            .as_ref()
                            .map(|project| project.native_path.clone());
                        let runtime = AgentEnvironmentResolver::from_active_wsl_session(
                            wsl_environment_context(&resolved, session.clone(), workspace),
                            session.clone(),
                        )
                        .resolve_registry(&snapshot, project_path.as_deref())
                        .await?;
                        let paths = delete_impact_resolved_paths(&runtime, &agent_id);
                        let inspections = inspect_wsl(&session, &paths).await?;
                        Ok(build_delete_impact(
                            &runtime,
                            agent_id,
                            display_name,
                            &inspections,
                        ))
                    }
                })
                .await
                .map_err(AgentCommandError::from)
        }
    }
}

fn settings_snapshot(
    registry: &ManagedAgentRegistry,
    environment: EnvironmentRef,
) -> AgentSettingsSnapshot {
    registry.settings_snapshot(environment)
}

async fn list_agents_dynamic(
    registry: &ManagedAgentRegistry,
    environment: EnvironmentContext,
    project_path: Option<&str>,
) -> Result<AgentRuntimeSnapshot, AppError> {
    let snapshot = registry.registry_snapshot(true);
    AgentEnvironmentResolver::from_environment(environment)
        .resolve_registry(&snapshot, project_path)
        .await
}

#[cfg(test)]
async fn validate_custom_agent_draft_inner(
    registry: &ManagedAgentRegistry,
    environment: EnvironmentContext,
    project_path: Option<&str>,
    draft: CustomAgentDefinition,
) -> Result<CustomAgentDraftValidation, AgentCommandError> {
    let draft_id = draft.id.clone();
    let preview_registry = registry.preview_registry(draft)?;
    resolve_custom_agent_preview(
        preview_registry.snapshot(),
        &draft_id,
        environment,
        project_path,
    )
    .await
    .map_err(AgentCommandError::from)
}

async fn resolve_custom_agent_preview(
    preview_snapshot: &AgentRegistrySnapshot,
    draft_id: &AgentId,
    environment: EnvironmentContext,
    project_path: Option<&str>,
) -> Result<CustomAgentDraftValidation, AppError> {
    resolve_custom_agent_preview_with_resolver(
        preview_snapshot,
        draft_id,
        AgentEnvironmentResolver::from_environment(environment),
        project_path,
    )
    .await
}

async fn resolve_custom_agent_preview_with_resolver(
    preview_snapshot: &AgentRegistrySnapshot,
    draft_id: &AgentId,
    resolver: AgentEnvironmentResolver,
    project_path: Option<&str>,
) -> Result<CustomAgentDraftValidation, AppError> {
    let runtime = resolver
        .resolve_registry(preview_snapshot, project_path)
        .await?;
    let resolved = runtime
        .agents
        .get(draft_id)
        .cloned()
        .ok_or_else(|| AppError::InvalidAgent {
            agent: draft_id.to_string(),
        })?;
    Ok(CustomAgentDraftValidation {
        registry_revision: runtime.registry_revision,
        environment_revision: runtime.environment_revision,
        environment: runtime.environment,
        resolved,
    })
}

#[cfg(test)]
fn save_custom_agent_inner(
    registry: &ManagedAgentRegistry,
    environment: EnvironmentRef,
    draft: CustomAgentDefinition,
    expected_registry_revision: String,
) -> Result<AgentSettingsSnapshot, AgentCommandError> {
    save_custom_agent_inner_with_original_id(
        registry,
        environment,
        draft,
        None,
        expected_registry_revision,
    )
}

fn save_custom_agent_inner_with_original_id(
    registry: &ManagedAgentRegistry,
    environment: EnvironmentRef,
    draft: CustomAgentDefinition,
    original_id: Option<AgentId>,
    expected_registry_revision: String,
) -> Result<AgentSettingsSnapshot, AgentCommandError> {
    registry.save(draft, original_id.as_ref(), &expected_registry_revision)?;
    Ok(registry.settings_snapshot(environment))
}

#[cfg(test)]
fn save_custom_agent_with_controller(
    registry: &ManagedAgentRegistry,
    controller: &RuntimeAdmissionCoordinator,
    context: SkillLocationRef,
    draft: CustomAgentDefinition,
    expected_registry_revision: String,
) -> Result<AgentSettingsSnapshot, AgentCommandError> {
    save_custom_agent_with_original_id_controller(
        registry,
        controller,
        context,
        draft,
        None,
        expected_registry_revision,
    )
}

fn save_custom_agent_with_original_id_controller(
    registry: &ManagedAgentRegistry,
    controller: &RuntimeAdmissionCoordinator,
    context: SkillLocationRef,
    draft: CustomAgentDefinition,
    original_id: Option<AgentId>,
    expected_registry_revision: String,
) -> Result<AgentSettingsSnapshot, AgentCommandError> {
    registry.preflight_save(&draft, original_id.as_ref(), &expected_registry_revision)?;
    let _guard =
        controller.begin_mutation(MutationKind::ManageAgentDefinitions, context.clone())?;
    save_custom_agent_inner_with_original_id(
        registry,
        context.environment,
        draft,
        original_id,
        expected_registry_revision,
    )
}

fn delete_custom_agent_inner(
    registry: &ManagedAgentRegistry,
    environment: EnvironmentRef,
    id: AgentId,
    expected_registry_revision: String,
) -> Result<AgentSettingsSnapshot, AgentCommandError> {
    registry.delete(&id, &expected_registry_revision)?;
    Ok(registry.settings_snapshot(environment))
}

fn build_delete_impact(
    runtime: &AgentRuntimeSnapshot,
    agent_id: AgentId,
    display_name: String,
    inspections: &BTreeMap<String, DirectoryInspection>,
) -> AgentDeleteImpact {
    let resolved = runtime
        .agents
        .get(&agent_id)
        .expect("delete impact runtime must contain the requested Custom Agent");
    let scopes = [
        (Scope::Global, &resolved.global),
        (Scope::Project, &resolved.project),
    ]
    .into_iter()
    .map(|(scope, resolved_scope)| {
        let definition_scope = match scope {
            Scope::Global => &resolved.definition.global,
            Scope::Project => &resolved.definition.project,
        };
        let paths =
            delete_scope_path_impacts(definition_scope, resolved_scope, scope.clone(), inspections);
        AgentDeleteScopeImpact { scope, paths }
    })
    .collect();

    AgentDeleteImpact {
        agent_id,
        display_name,
        registry_revision: runtime.registry_revision.clone(),
        environment_revision: runtime.environment_revision.clone(),
        scopes,
        loses_management_capability: true,
        files_will_be_deleted: false,
        library_usages: Vec::new(),
    }
}

fn delete_impact_resolved_paths(runtime: &AgentRuntimeSnapshot, id: &AgentId) -> Vec<String> {
    let resolved = runtime
        .agents
        .get(id)
        .expect("delete impact runtime must contain the requested Custom Agent");
    [&resolved.global, &resolved.project]
        .into_iter()
        .flat_map(|scope| {
            let mut paths = Vec::new();
            if scope.reads_standard {
                paths.extend(scope.standard_path.clone());
            }
            paths.extend(scope.private_path.clone());
            paths
        })
        .collect()
}

fn delete_scope_path_impacts(
    definition_scope: &ScopeDefinition,
    resolved_scope: &ResolvedAgentScope,
    scope: Scope,
    inspections: &BTreeMap<String, DirectoryInspection>,
) -> Vec<AgentDeletePathImpact> {
    if !definition_scope.enabled {
        return Vec::new();
    }
    let mut paths = Vec::new();
    if definition_scope.reads_standard {
        paths.push(delete_path_impact(
            AgentDeletePathKind::Standard,
            standard_logical_path(&scope),
            resolved_scope.standard_path.clone(),
            resolved_scope
                .standard_presence
                .unwrap_or(DirectoryPresenceState::EnvironmentUnavailable),
            inspections,
        ));
    }
    if let Some(logical_path) = definition_scope.private_path.clone() {
        paths.push(delete_path_impact(
            AgentDeletePathKind::Private,
            logical_path,
            resolved_scope.private_path.clone(),
            resolved_scope
                .private_presence
                .unwrap_or(DirectoryPresenceState::EnvironmentUnavailable),
            inspections,
        ));
    }
    paths
}

fn standard_logical_path(scope: &Scope) -> PathSpec {
    match scope {
        Scope::Global => PathSpec::home(".agents/skills"),
        Scope::Project => PathSpec::project(".agents/skills"),
    }
}

fn delete_path_impact(
    kind: AgentDeletePathKind,
    logical_path: PathSpec,
    resolved_path: Option<String>,
    presence: DirectoryPresenceState,
    inspections: &BTreeMap<String, DirectoryInspection>,
) -> AgentDeletePathImpact {
    let inspection = resolved_path
        .as_ref()
        .and_then(|path| inspections.get(path));
    let (observed_skill_count, observed_skill_count_truncated, unavailable_reason) = match presence
    {
        DirectoryPresenceState::Present | DirectoryPresenceState::LegacyPath => inspection
            .map(|inspection| {
                (
                    inspection.observed_skill_count,
                    inspection.observed_skill_count_truncated,
                    inspection
                        .observed_skill_count
                        .is_none()
                        .then_some(DetectionReason::EnvironmentUnavailable),
                )
            })
            .unwrap_or((None, false, Some(DetectionReason::EnvironmentUnavailable))),
        DirectoryPresenceState::Missing
        | DirectoryPresenceState::BrokenLink
        | DirectoryPresenceState::ConflictingEntry => (Some(0), false, None),
        DirectoryPresenceState::ProjectNotSelected => {
            (None, false, Some(DetectionReason::ProjectContextRequired))
        }
        DirectoryPresenceState::EnvironmentUnavailable => {
            (None, false, Some(DetectionReason::EnvironmentUnavailable))
        }
        DirectoryPresenceState::UnsafePath => (
            inspection.and_then(|inspection| inspection.observed_skill_count),
            inspection.is_some_and(|inspection| inspection.observed_skill_count_truncated),
            None,
        ),
    };
    AgentDeletePathImpact {
        kind,
        logical_path,
        resolved_path,
        presence,
        observed_skill_count,
        observed_skill_count_truncated,
        unavailable_reason,
    }
}

#[cfg(test)]
fn delete_custom_agent_with_controller(
    registry: &ManagedAgentRegistry,
    controller: &RuntimeAdmissionCoordinator,
    context: SkillLocationRef,
    id: AgentId,
    expected_registry_revision: String,
) -> Result<AgentSettingsSnapshot, AgentCommandError> {
    registry.preflight_delete(&id, &expected_registry_revision)?;
    let _guard =
        controller.begin_mutation(MutationKind::ManageAgentDefinitions, context.clone())?;
    delete_custom_agent_inner(
        registry,
        context.environment,
        id,
        expected_registry_revision,
    )
}

fn delete_custom_agent_with_controller_result(
    registry: &ManagedAgentRegistry,
    controller: &RuntimeAdmissionCoordinator,
    context: SkillLocationRef,
    id: AgentId,
    expected_registry_revision: String,
) -> Result<AgentDeleteResult, AgentCommandError> {
    registry.preflight_delete(&id, &expected_registry_revision)?;
    let _guard =
        controller.begin_mutation(MutationKind::ManageAgentDefinitions, context.clone())?;
    let settings = delete_custom_agent_inner(
        registry,
        context.environment,
        id,
        expected_registry_revision,
    )?;
    Ok(AgentDeleteResult { settings })
}

fn delete_preview_snapshot(
    definition: &CustomAgentDefinition,
    revision: &str,
) -> Result<AgentRegistrySnapshot, AgentCommandError> {
    let normalized = definition
        .normalize()
        .map_err(|error| AgentCommandError::InvalidDraft {
            errors: vec![error],
        })?;
    Ok(AgentRegistrySnapshot {
        revision: revision.to_string(),
        active_definitions: BTreeMap::from([(normalized.id.clone(), normalized)]),
    })
}

fn native_environment_context(resolved: &ResolvedContext) -> EnvironmentContext {
    let environment_variables = std::env::vars().collect::<BTreeMap<_, _>>();
    let home = resolved.home.native_path.clone();
    let config_home = PATHS.config_home.to_string_lossy().to_string();
    let revision = environment_revision(
        "native",
        &(home.clone(), config_home.clone(), &environment_variables),
    );
    EnvironmentContext {
        environment: EnvironmentRef::Native,
        home,
        config_home,
        environment_variables,
        availability: EnvironmentStatus::Available,
        revision,
        wsl_workspace: None,
    }
}

fn wsl_environment_context(
    resolved: &ResolvedContext,
    session: WslSession,
    workspace: crate::environment::wsl::WslWorkspace,
) -> EnvironmentContext {
    let revision = environment_revision("wsl", &session);
    EnvironmentContext {
        environment: resolved.context.environment.clone(),
        home: session.home.clone(),
        config_home: session.config_home.clone(),
        environment_variables: session.environment.clone(),
        availability: EnvironmentStatus::Available,
        revision,
        wsl_workspace: Some(workspace),
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

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::{Arc, Barrier};

    use super::*;
    use crate::application::runtime_admission::RuntimeAdmissionCoordinator;
    use crate::core::agent_definition::{
        AgentFieldError, AgentId, CustomAgentDefinition, CustomPathBase, CustomPathSpec,
        CustomScopeDefinition, ScopeLocation,
    };
    use crate::core::agent_settings::{AgentStorageIssue, CustomAgentRecord};
    use crate::core::custom_agent_repository::{CustomAgentFile, CustomAgentRepository};
    use crate::core::mutation::MutationKind;
    use crate::environment::agent_environment::{
        AgentEnvironmentResolver, DetectionState, EnvironmentContext,
    };
    use crate::environment::types::{EnvironmentStatus, ResourceLocator, SkillLocation};
    use serde_json::json;

    #[test]
    fn cloned_managed_registry_handles_share_runtime_state() {
        let temp = tempfile::tempdir().expect("temp");
        let registry = ManagedAgentRegistry::from_repository(CustomAgentRepository::new(
            temp.path().join("custom-agents.json"),
        ));
        let cloned = registry.clone();

        assert!(Arc::ptr_eq(&registry.registry, &cloned.registry));
        assert!(Arc::ptr_eq(
            &registry.custom_storage_error,
            &cloned.custom_storage_error
        ));
        assert!(Arc::ptr_eq(&registry.mutation_lock, &cloned.mutation_lock));
    }

    fn custom_definition(id: &str, relative_path: &str) -> CustomAgentDefinition {
        CustomAgentDefinition {
            id: AgentId::parse(id).expect("agent ID"),
            display_name: format!("{id} display"),
            global: CustomScopeDefinition {
                enabled: true,
                location: ScopeLocation::Private,
                private_path: Some(CustomPathSpec::Based {
                    base: CustomPathBase::Home,
                    relative_path: format!("{relative_path}/skills"),
                }),
            },
            project: CustomScopeDefinition {
                enabled: false,
                location: ScopeLocation::Standard,
                private_path: None,
            },
            detection_paths: vec![CustomPathSpec::Based {
                base: CustomPathBase::Home,
                relative_path: relative_path.to_string(),
            }],
        }
    }

    fn native_environment(home: &std::path::Path) -> EnvironmentContext {
        EnvironmentContext {
            environment: EnvironmentRef::Native,
            home: home.to_string_lossy().to_string(),
            config_home: home.join(".config").to_string_lossy().to_string(),
            environment_variables: BTreeMap::new(),
            availability: EnvironmentStatus::Available,
            revision: "native-test-revision".to_string(),
            wsl_workspace: None,
        }
    }

    fn repository_with_records(
        records: Vec<CustomAgentRecord>,
    ) -> (tempfile::TempDir, CustomAgentRepository) {
        let temp = tempfile::tempdir().expect("tempdir");
        let repository = CustomAgentRepository::new(temp.path().join("custom-agents.json"));
        repository
            .save(&CustomAgentFile {
                schema_version: 1,
                records,
                root_extensions: Default::default(),
            })
            .expect("seed custom agents");
        (temp, repository)
    }

    #[test]
    fn create_reports_an_id_collision_when_another_window_claims_the_id() {
        let (_temp, repository) = repository_with_records(Vec::new());
        let service = ManagedAgentRegistry::from_repository(repository);
        let initial_revision = service.registry_snapshot(true).revision.clone();
        let draft = custom_definition("new-agent", ".new-agent");

        save_custom_agent_inner(
            &service,
            EnvironmentRef::Native,
            draft.clone(),
            initial_revision.clone(),
        )
        .expect("other window claims Agent ID");

        let error = save_custom_agent_inner(
            &service,
            EnvironmentRef::Native,
            draft.clone(),
            initial_revision.clone(),
        )
        .expect_err("duplicate ID must be reported before stale revision");

        assert_eq!(
            error,
            AgentCommandError::InvalidDraft {
                errors: vec![AgentFieldError::new("id", "duplicateAgentId")],
            }
        );

        let edit_error = save_custom_agent_inner_with_original_id(
            &service,
            EnvironmentRef::Native,
            draft.clone(),
            Some(draft.id),
            initial_revision,
        )
        .expect_err("editing an existing ID must preserve stale revision semantics");
        assert!(matches!(
            edit_error,
            AgentCommandError::StaleRegistryRevision { .. }
        ));
    }

    #[tokio::test]
    async fn settings_keeps_all_records_while_runtime_resolves_only_active_open_ids() {
        let active = custom_definition("my-custom-agent", ".my-custom-agent");
        let conflict = custom_definition("codex", ".old-codex");
        let invalid_raw = json!({ "id": "Broken ID", "displayName": "Broken" });
        let invalid = CustomAgentRecord::Invalid {
            index: 2,
            raw: invalid_raw.clone(),
            errors: vec![AgentFieldError::new("id", "invalidAgentId")],
        };
        let (temp, repository) = repository_with_records(vec![
            CustomAgentRecord::valid(active),
            CustomAgentRecord::valid(conflict),
            invalid,
        ]);
        std::fs::create_dir_all(temp.path().join(".my-custom-agent/skills"))
            .expect("custom agent directory");
        let service = ManagedAgentRegistry::from_repository(repository);

        let settings = settings_snapshot(&service, EnvironmentRef::Native);
        let runtime = list_agents_dynamic(&service, native_environment(temp.path()), None)
            .await
            .expect("runtime snapshot");

        assert_eq!(settings.active_custom.len(), 1);
        assert_eq!(settings.disabled_conflicts.len(), 1);
        assert_eq!(settings.invalid_custom_records.len(), 1);
        assert_eq!(settings.invalid_custom_records[0].raw.0, invalid_raw);
        assert!(runtime
            .agents
            .contains_key(&AgentId::parse("my-custom-agent").unwrap()));
        assert_eq!(
            runtime.agents[&AgentId::parse("my-custom-agent").unwrap()].detection,
            DetectionState::Detected
        );
        assert!(!runtime
            .agents
            .contains_key(&AgentId::parse("codex").unwrap()));
        assert!(!runtime
            .agents
            .contains_key(&AgentId::parse("broken-id").unwrap()));
        assert_eq!(runtime.environment, EnvironmentRef::Native);
    }

    #[tokio::test]
    async fn future_schema_initialization_keeps_builtins_visible_and_storage_read_only() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("custom-agents.json");
        let original = br#"{"schemaVersion":99,"agents":[]}"#;
        std::fs::write(&path, original).expect("future schema fixture");
        let service =
            ManagedAgentRegistry::from_repository(CustomAgentRepository::new(path.clone()));
        let settings = settings_snapshot(&service, EnvironmentRef::Native);
        let runtime = list_agents_dynamic(&service, native_environment(temp.path()), None)
            .await
            .expect("built-in runtime remains available");
        let validation = validate_custom_agent_draft_inner(
            &service,
            native_environment(temp.path()),
            None,
            custom_definition("preview-agent", ".preview-agent"),
        )
        .await
        .expect("read-only validation remains available");
        let expected_error = AppError::ConfigurationReadOnly;
        let expected_command_error = AgentCommandError::Application {
            error: expected_error.clone(),
        };
        let revision = settings.registry_revision.clone();
        let mut invalid_draft = custom_definition("invalid-agent", ".invalid-agent");
        invalid_draft.display_name = "   ".to_string();
        let invalid_save_error = save_custom_agent_inner(
            &service,
            EnvironmentRef::Native,
            invalid_draft,
            revision.clone(),
        )
        .expect_err("storage error takes precedence over draft validation");
        let collision_save_error = save_custom_agent_inner(
            &service,
            EnvironmentRef::Native,
            custom_definition("codex", ".old-codex"),
            revision.clone(),
        )
        .expect_err("storage error takes precedence over Built-in collision");
        let save_error = save_custom_agent_inner(
            &service,
            EnvironmentRef::Native,
            custom_definition("blocked-agent", ".blocked-agent"),
            revision.clone(),
        )
        .expect_err("unavailable storage blocks save");
        let delete_error = delete_custom_agent_inner(
            &service,
            EnvironmentRef::Native,
            AgentId::parse("blocked-agent").unwrap(),
            revision,
        )
        .expect_err("unavailable storage blocks delete");

        assert!(!settings.active_builtin.is_empty());
        assert!(settings.active_custom.is_empty());
        assert_eq!(
            settings.custom_storage_issue,
            Some(AgentStorageIssue {
                code: "customAgentStorageUnavailable".to_string(),
                message: expected_error.to_string(),
                read_only: true,
            })
        );
        assert!(!runtime.agents.is_empty());
        assert!(runtime
            .agents
            .values()
            .all(|agent| agent.definition.source == AgentSource::Builtin));
        assert_eq!(validation.resolved.definition.id.as_str(), "preview-agent");
        assert_eq!(invalid_save_error, expected_command_error);
        assert_eq!(collision_save_error, expected_command_error);
        assert_eq!(save_error, expected_command_error);
        assert_eq!(delete_error, expected_command_error);
        assert_eq!(std::fs::read(path).unwrap(), original);
    }

    #[tokio::test]
    async fn repository_path_failure_starts_with_empty_read_only_snapshots() {
        let expected_error = AppError::Path {
            message: "home directory is unavailable".to_string(),
        };
        let service =
            ManagedAgentRegistry::from_repository_initializer(|| Err(expected_error.clone()));
        let settings = settings_snapshot(&service, EnvironmentRef::Native);
        let first_runtime = list_agents_dynamic(
            &service,
            native_environment(std::path::Path::new("/unavailable-home")),
            None,
        )
        .await
        .expect("empty runtime snapshot");
        let second_runtime = list_agents_dynamic(
            &service,
            native_environment(std::path::Path::new("/unavailable-home")),
            None,
        )
        .await
        .expect("stable empty runtime snapshot");
        let expected_command_error = AgentCommandError::Application {
            error: expected_error.clone(),
        };
        let validation_error = validate_custom_agent_draft_inner(
            &service,
            native_environment(std::path::Path::new("/unavailable-home")),
            None,
            custom_definition("preview-agent", ".preview-agent"),
        )
        .await
        .expect_err("path failure blocks validation");
        let save_error = save_custom_agent_inner(
            &service,
            EnvironmentRef::Native,
            custom_definition("new-agent", ".new-agent"),
            settings.registry_revision.clone(),
        )
        .expect_err("path failure blocks save");
        let delete_error = delete_custom_agent_inner(
            &service,
            EnvironmentRef::Native,
            AgentId::parse("missing-agent").unwrap(),
            settings.registry_revision.clone(),
        )
        .expect_err("path failure blocks delete");

        assert!(settings.active_builtin.is_empty());
        assert!(settings.active_custom.is_empty());
        assert!(settings.disabled_conflicts.is_empty());
        assert!(settings.invalid_custom_records.is_empty());
        assert_eq!(
            settings.custom_storage_issue,
            Some(AgentStorageIssue {
                code: "customAgentStorageUnavailable".to_string(),
                message: expected_error.to_string(),
                read_only: true,
            })
        );
        assert!(first_runtime.agents.is_empty());
        assert!(second_runtime.agents.is_empty());
        assert_eq!(first_runtime.registry_revision, settings.registry_revision);
        assert_eq!(second_runtime.registry_revision, settings.registry_revision);
        assert_eq!(validation_error, expected_command_error);
        assert_eq!(save_error, expected_command_error);
        assert_eq!(delete_error, expected_command_error);
    }

    #[test]
    fn corrupt_unrecoverable_initialization_reports_issue_and_never_overwrites_primary() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("custom-agents.json");
        let original = b"{broken";
        std::fs::write(&path, original).expect("corrupt primary fixture");
        let service =
            ManagedAgentRegistry::from_repository(CustomAgentRepository::new(path.clone()));
        let settings = settings_snapshot(&service, EnvironmentRef::Native);
        let issue = settings
            .custom_storage_issue
            .expect("Settings exposes storage failure");

        let save_error = save_custom_agent_inner(
            &service,
            EnvironmentRef::Native,
            custom_definition("blocked-agent", ".blocked-agent"),
            settings.registry_revision,
        )
        .expect_err("corrupt storage blocks save");

        assert!(!settings.active_builtin.is_empty());
        assert_eq!(issue.code, "customAgentStorageUnavailable");
        assert!(issue.read_only);
        assert!(issue.message.starts_with("JSON error:"));
        assert!(matches!(
            save_error,
            AgentCommandError::Application { error }
                if error.to_string() == issue.message
        ));
        assert_eq!(std::fs::read(path).unwrap(), original);
    }

    #[test]
    fn successful_mutations_rebuild_the_owned_snapshot_revision() {
        let source = custom_definition("source-agent", ".source-agent");
        let (_temp, repository) = repository_with_records(vec![CustomAgentRecord::valid(source)]);
        let service = ManagedAgentRegistry::from_repository(repository);
        let initial_revision = service.registry_snapshot(true).revision.clone();

        let saved = save_custom_agent_inner(
            &service,
            EnvironmentRef::Native,
            custom_definition("new-agent", ".new-agent"),
            initial_revision.clone(),
        )
        .expect("save custom agent");
        assert_ne!(saved.registry_revision, initial_revision);
        assert!(service
            .registry_snapshot(true)
            .active_definitions
            .contains_key(&AgentId::parse("new-agent").unwrap()));

        let deleted = delete_custom_agent_inner(
            &service,
            EnvironmentRef::Native,
            AgentId::parse("new-agent").unwrap(),
            saved.registry_revision.clone(),
        )
        .expect("delete custom agent");
        assert_eq!(deleted.registry_revision, initial_revision);
        assert!(!service
            .registry_snapshot(true)
            .active_definitions
            .contains_key(&AgentId::parse("new-agent").unwrap()));
    }

    #[tokio::test]
    async fn delete_impact_reports_paths_and_counts() {
        let temp = tempfile::tempdir().expect("tempdir");
        let project = temp.path().join("project");
        std::fs::create_dir_all(temp.path().join(".agents/skills")).unwrap();
        std::fs::write(temp.path().join(".agents/skills/one.md"), "one").unwrap();
        std::fs::write(temp.path().join(".agents/skills/two.md"), "two").unwrap();
        std::fs::create_dir_all(project.join(".my-agent/skills")).unwrap();
        std::fs::write(project.join(".my-agent/skills/project.md"), "project").unwrap();
        let mut definition = custom_definition_with_project_scope("my-agent", ".my-agent");
        definition.global = CustomScopeDefinition {
            enabled: true,
            location: ScopeLocation::Standard,
            private_path: None,
        };
        let snapshot = AgentRegistrySnapshot {
            revision: "registry-revision".to_string(),
            active_definitions: BTreeMap::from([(
                definition.id.clone(),
                definition.normalize().expect("normalize definition"),
            )]),
        };
        let runtime = AgentEnvironmentResolver::from_environment(native_environment(temp.path()))
            .resolve_registry(&snapshot, Some(project.to_str().unwrap()))
            .await
            .expect("resolve one current-environment snapshot");

        let inspections = crate::environment::directory_inspection::inspect_native(
            &delete_impact_resolved_paths(&runtime, &definition.id),
        )
        .await;
        let impact = build_delete_impact(
            &runtime,
            definition.id.clone(),
            definition.display_name.clone(),
            &inspections,
        );

        assert_eq!(impact.agent_id.as_str(), "my-agent");
        assert_eq!(impact.scopes[0].scope, Scope::Global);
        assert_eq!(
            impact.scopes[0].paths[0].kind,
            AgentDeletePathKind::Standard
        );
        assert_eq!(
            impact.scopes[0].paths[0].logical_path,
            PathSpec::home(".agents/skills")
        );
        assert_eq!(impact.scopes[0].paths[0].observed_skill_count, Some(2));
        assert_eq!(impact.scopes[1].scope, Scope::Project);
        assert_eq!(impact.scopes[1].paths[0].kind, AgentDeletePathKind::Private);
        assert_eq!(impact.scopes[1].paths[0].observed_skill_count, Some(1));
        assert!(impact.loses_management_capability);
        assert!(!impact.files_will_be_deleted);
    }

    #[tokio::test]
    async fn global_context_marks_project_based_impact_as_needing_project_context() {
        let temp = tempfile::tempdir().expect("tempdir");
        let definition = custom_definition_with_project_scope("my-agent", ".my-agent");
        let snapshot = AgentRegistrySnapshot {
            revision: "registry-revision".to_string(),
            active_definitions: BTreeMap::from([(
                definition.id.clone(),
                definition.normalize().expect("normalize definition"),
            )]),
        };
        let runtime = AgentEnvironmentResolver::from_environment(native_environment(temp.path()))
            .resolve_registry(&snapshot, None)
            .await
            .expect("resolve one current-environment snapshot");

        let impact = build_delete_impact(
            &runtime,
            definition.id,
            definition.display_name,
            &std::collections::BTreeMap::new(),
        );

        assert_eq!(
            impact.scopes[1].paths[0].unavailable_reason,
            Some(DetectionReason::ProjectContextRequired)
        );
        assert_eq!(impact.scopes[1].paths[0].observed_skill_count, None);
    }

    #[tokio::test]
    async fn delete_impact_emits_shared_and_private_paths_for_a_both_scope() {
        let temp = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir_all(temp.path().join(".agents/skills")).expect("create skills");
        std::fs::write(temp.path().join(".agents/skills/one.md"), "one").expect("write skill");
        let definition = custom_definition("both-agent", ".both-agent");
        let mut definition = definition;
        definition.global = CustomScopeDefinition {
            enabled: true,
            location: ScopeLocation::Both,
            private_path: Some(CustomPathSpec::Based {
                base: CustomPathBase::Home,
                relative_path: ".agents/skills".to_string(),
            }),
        };
        let snapshot = AgentRegistrySnapshot {
            revision: "registry-revision".to_string(),
            active_definitions: BTreeMap::from([(
                definition.id.clone(),
                definition.normalize().expect("normalize definition"),
            )]),
        };
        let runtime = AgentEnvironmentResolver::from_environment(native_environment(temp.path()))
            .resolve_registry(&snapshot, None)
            .await
            .expect("resolve runtime");
        let inspected = crate::environment::directory_inspection::inspect_native(
            &delete_impact_resolved_paths(&runtime, &definition.id),
        )
        .await;

        let impact =
            build_delete_impact(&runtime, definition.id, definition.display_name, &inspected);

        assert_eq!(inspected.len(), 1);
        assert_eq!(impact.scopes[0].paths.len(), 2);
        assert_eq!(
            impact.scopes[0].paths[0].kind,
            AgentDeletePathKind::Standard
        );
        assert_eq!(impact.scopes[0].paths[1].kind, AgentDeletePathKind::Private);
        assert_eq!(
            impact.scopes[0].paths[0].logical_path,
            PathSpec::home(".agents/skills")
        );
        assert_eq!(
            impact.scopes[0].paths[1].logical_path,
            PathSpec::home(".agents/skills")
        );
        assert_eq!(impact.scopes[0].paths[0].observed_skill_count, Some(1));
        assert_eq!(impact.scopes[0].paths[1].observed_skill_count, Some(1));
    }

    #[test]
    fn delete_path_impact_projects_unavailable_and_absent_paths_differently() {
        let inspections = BTreeMap::new();
        let logical_path = PathSpec::home(".agent/skills");

        for presence in [
            DirectoryPresenceState::Present,
            DirectoryPresenceState::LegacyPath,
        ] {
            let impact = delete_path_impact(
                AgentDeletePathKind::Private,
                logical_path.clone(),
                Some("/home/alice/.agent/skills".to_string()),
                presence,
                &inspections,
            );
            assert_eq!(impact.observed_skill_count, None);
            assert_eq!(
                impact.unavailable_reason,
                Some(DetectionReason::EnvironmentUnavailable)
            );
        }

        let inspections = BTreeMap::from([(
            "/home/alice/.agent/skills".to_string(),
            DirectoryInspection {
                observed_skill_count: None,
                observed_skill_count_truncated: false,
            },
        )]);
        for presence in [
            DirectoryPresenceState::Present,
            DirectoryPresenceState::LegacyPath,
        ] {
            let impact = delete_path_impact(
                AgentDeletePathKind::Private,
                logical_path.clone(),
                Some("/home/alice/.agent/skills".to_string()),
                presence,
                &inspections,
            );
            assert_eq!(impact.observed_skill_count, None);
            assert_eq!(
                impact.unavailable_reason,
                Some(DetectionReason::EnvironmentUnavailable)
            );
        }

        for presence in [
            DirectoryPresenceState::Missing,
            DirectoryPresenceState::BrokenLink,
            DirectoryPresenceState::ConflictingEntry,
        ] {
            let impact = delete_path_impact(
                AgentDeletePathKind::Private,
                logical_path.clone(),
                Some("/home/alice/.agent/skills".to_string()),
                presence,
                &inspections,
            );
            assert_eq!(impact.observed_skill_count, Some(0));
            assert_eq!(impact.unavailable_reason, None);
        }

        for (presence, reason) in [
            (
                DirectoryPresenceState::ProjectNotSelected,
                DetectionReason::ProjectContextRequired,
            ),
            (
                DirectoryPresenceState::EnvironmentUnavailable,
                DetectionReason::EnvironmentUnavailable,
            ),
        ] {
            let impact = delete_path_impact(
                AgentDeletePathKind::Private,
                logical_path.clone(),
                None,
                presence,
                &inspections,
            );
            assert_eq!(impact.observed_skill_count, None);
            assert_eq!(impact.unavailable_reason, Some(reason));
        }
    }

    #[tokio::test]
    async fn project_shared_is_unavailable_without_a_project_but_home_and_config_private_paths_are_inspectable(
    ) {
        for (id, base, private_path) in [
            ("project-home", CustomPathBase::Home, ".project-private"),
            (
                "project-config",
                CustomPathBase::ConfigHome,
                ".project-private",
            ),
        ] {
            let temp = tempfile::tempdir().expect("tempdir");
            let private_root = match base {
                CustomPathBase::Home => temp.path().join(private_path),
                CustomPathBase::ConfigHome => temp.path().join(".config").join(private_path),
                CustomPathBase::Project => unreachable!("test covers non-project bases"),
            };
            std::fs::create_dir_all(&private_root).expect("create private path");
            std::fs::write(private_root.join("one.md"), "one").expect("write skill");
            let mut definition = custom_definition(id, ".detection");
            definition.global = CustomScopeDefinition {
                enabled: false,
                location: ScopeLocation::Standard,
                private_path: None,
            };
            definition.project = CustomScopeDefinition {
                enabled: true,
                location: ScopeLocation::Both,
                private_path: Some(CustomPathSpec::Based {
                    base,
                    relative_path: private_path.to_string(),
                }),
            };
            let snapshot = AgentRegistrySnapshot {
                revision: "registry-revision".to_string(),
                active_definitions: BTreeMap::from([(
                    definition.id.clone(),
                    definition.normalize().expect("normalize definition"),
                )]),
            };
            let runtime =
                AgentEnvironmentResolver::from_environment(native_environment(temp.path()))
                    .resolve_registry(&snapshot, None)
                    .await
                    .expect("resolve runtime");
            let inspected = crate::environment::directory_inspection::inspect_native(
                &delete_impact_resolved_paths(&runtime, &definition.id),
            )
            .await;
            let impact =
                build_delete_impact(&runtime, definition.id, definition.display_name, &inspected);
            let paths = &impact.scopes[1].paths;

            assert_eq!(paths[0].kind, AgentDeletePathKind::Standard);
            assert_eq!(
                paths[0].unavailable_reason,
                Some(DetectionReason::ProjectContextRequired)
            );
            assert_eq!(paths[1].kind, AgentDeletePathKind::Private);
            assert_eq!(paths[1].observed_skill_count, Some(1));
            assert_eq!(paths[1].unavailable_reason, None);
        }
    }

    fn custom_definition_with_project_scope(
        id: &str,
        relative_path: &str,
    ) -> CustomAgentDefinition {
        let mut definition = custom_definition(id, relative_path);
        definition.project = CustomScopeDefinition {
            enabled: true,
            location: ScopeLocation::Private,
            private_path: Some(CustomPathSpec::Based {
                base: CustomPathBase::Project,
                relative_path: format!("{relative_path}/skills"),
            }),
        };
        definition
    }

    #[tokio::test]
    async fn invalid_draft_returns_stable_field_errors_before_environment_io() {
        let (_temp, repository) = repository_with_records(Vec::new());
        let service = ManagedAgentRegistry::from_repository(repository);
        let mut draft = custom_definition("draft-agent", ".draft-agent");
        draft.display_name = "   ".to_string();

        let error = validate_custom_agent_draft_inner(
            &service,
            native_environment(std::path::Path::new("/definitely/not/read")),
            None,
            draft,
        )
        .await
        .expect_err("invalid draft");

        assert_eq!(
            error,
            AgentCommandError::InvalidDraft {
                errors: vec![AgentFieldError::new("displayName", "required")]
            }
        );
    }

    #[test]
    fn stale_save_and_delete_leave_repository_bytes_unchanged() {
        let source = custom_definition("source-agent", ".source-agent");
        let (_temp, repository) = repository_with_records(vec![CustomAgentRecord::valid(source)]);
        let path = repository.path().to_path_buf();
        let service = ManagedAgentRegistry::from_repository(repository);
        let actual_revision = service.registry_snapshot(true).revision.clone();
        let before = std::fs::read(&path).expect("original bytes");

        let save_error = save_custom_agent_inner(
            &service,
            EnvironmentRef::Native,
            custom_definition("new-agent", ".new-agent"),
            "stale-revision".to_string(),
        )
        .expect_err("stale save");
        let delete_error = delete_custom_agent_inner(
            &service,
            EnvironmentRef::Native,
            AgentId::parse("source-agent").unwrap(),
            "stale-revision".to_string(),
        )
        .expect_err("stale delete");

        let expected = AgentCommandError::StaleRegistryRevision {
            expected: "stale-revision".to_string(),
            actual: actual_revision,
        };
        assert_eq!(save_error, expected);
        assert_eq!(delete_error, expected);
        assert_eq!(std::fs::read(path).unwrap(), before);
    }

    #[test]
    fn delete_rejects_builtin_and_unknown_ids_without_changing_the_repository() {
        let (_temp, repository) = repository_with_records(vec![CustomAgentRecord::valid(
            custom_definition("kept-agent", ".kept-agent"),
        )]);
        let path = repository.path().to_path_buf();
        let service = ManagedAgentRegistry::from_repository(repository);
        let revision = service.registry_snapshot(true).revision.clone();
        let before = std::fs::read(&path).expect("repository bytes");

        for id in ["codex", "missing-agent"] {
            let error = service
                .delete(&AgentId::parse(id).unwrap(), &revision)
                .expect_err("non-Custom target must be rejected");
            assert_eq!(
                error,
                AgentCommandError::Application {
                    error: AppError::InvalidAgent {
                        agent: id.to_string(),
                    },
                }
            );
        }

        assert_eq!(std::fs::read(path).unwrap(), before);
    }

    #[test]
    fn delete_preview_stamps_the_registry_revision_from_its_captured_registry() {
        let definition = custom_definition("captured-agent", ".captured-agent");
        let (_temp, repository) =
            repository_with_records(vec![CustomAgentRecord::valid(definition.clone())]);
        let service = ManagedAgentRegistry::from_repository(repository);
        let actual_revision = service.registry_snapshot(true).revision.clone();

        let captured = service
            .preview_delete_definition(&definition.id, &actual_revision)
            .expect("capture active Custom Agent");
        let preview =
            delete_preview_snapshot(&captured.definition, &captured.registry.snapshot().revision)
                .expect("build delete preview registry");

        assert_eq!(captured.registry.snapshot().revision, actual_revision);
        assert_eq!(preview.revision, actual_revision);
    }

    #[test]
    fn invalid_delete_does_not_publish_mutation_state() {
        let (_temp, repository) = repository_with_records(vec![CustomAgentRecord::valid(
            custom_definition("kept-agent", ".kept-agent"),
        )]);
        let service = ManagedAgentRegistry::from_repository(repository);
        let controller = RuntimeAdmissionCoordinator::default();
        let revision = service.registry_snapshot(true).revision.clone();

        let error = delete_custom_agent_with_controller_result(
            &service,
            &controller,
            SkillLocationRef {
                environment: EnvironmentRef::Native,
                scope: SkillLocation::Global,
            },
            AgentId::parse("codex").unwrap(),
            revision,
        )
        .expect_err("invalid definition must fail before mutation admission");

        assert_eq!(
            error,
            AgentCommandError::Application {
                error: AppError::InvalidAgent {
                    agent: "codex".to_string(),
                },
            }
        );
        assert_eq!(controller.snapshot().revision, 0);
    }

    #[test]
    fn invalid_save_preflight_does_not_publish_mutation_state() {
        let (_temp, repository) = repository_with_records(Vec::new());
        let service = ManagedAgentRegistry::from_repository(repository);
        let controller = RuntimeAdmissionCoordinator::default();
        let mut invalid = custom_definition("invalid-agent", ".invalid-agent");
        invalid.display_name = "   ".to_string();

        let error = save_custom_agent_with_controller(
            &service,
            &controller,
            SkillLocationRef {
                environment: EnvironmentRef::Native,
                scope: SkillLocation::Global,
            },
            invalid,
            service.registry_snapshot(true).revision.clone(),
        )
        .expect_err("invalid draft");

        assert_eq!(
            error,
            AgentCommandError::InvalidDraft {
                errors: vec![AgentFieldError::new("displayName", "required")],
            }
        );
        assert_eq!(controller.snapshot().revision, 0);
        assert!(controller.active().is_none());
    }

    #[test]
    fn busy_controller_preserves_save_validation_errors_without_writes() {
        let (_temp, repository) = repository_with_records(Vec::new());
        let repository_path = repository.path().to_path_buf();
        let service = ManagedAgentRegistry::from_repository(repository);
        let controller = RuntimeAdmissionCoordinator::default();
        let context = SkillLocationRef {
            environment: EnvironmentRef::Native,
            scope: SkillLocation::Global,
        };
        let _guard = controller
            .begin_mutation(MutationKind::Install, context.clone())
            .expect("occupy controller");
        let controller_revision = controller.snapshot().revision;
        let repository_before = std::fs::read(&repository_path).expect("repository bytes");
        let registry_revision = service.registry_snapshot(true).revision.clone();
        let mut invalid = custom_definition("invalid-agent", ".invalid-agent");
        invalid.display_name = "   ".to_string();

        let invalid_error = save_custom_agent_with_controller(
            &service,
            &controller,
            context.clone(),
            invalid,
            registry_revision.clone(),
        )
        .expect_err("invalid draft before busy");
        let collision_error = save_custom_agent_with_controller(
            &service,
            &controller,
            context,
            custom_definition("codex", ".codex-custom"),
            registry_revision,
        )
        .expect_err("Built-in collision before busy");

        assert_eq!(
            invalid_error,
            AgentCommandError::InvalidDraft {
                errors: vec![AgentFieldError::new("displayName", "required")],
            }
        );
        assert_eq!(
            collision_error,
            AgentCommandError::InvalidDraft {
                errors: vec![AgentFieldError::new("id", "duplicateAgentId")],
            }
        );
        assert_eq!(controller.snapshot().revision, controller_revision);
        assert_eq!(std::fs::read(repository_path).unwrap(), repository_before);
    }

    #[test]
    fn busy_controller_preserves_stale_save_and_delete_without_writes() {
        let source = custom_definition("source-agent", ".source-agent");
        let (_temp, repository) = repository_with_records(vec![CustomAgentRecord::valid(source)]);
        let repository_path = repository.path().to_path_buf();
        let service = ManagedAgentRegistry::from_repository(repository);
        let controller = RuntimeAdmissionCoordinator::default();
        let context = SkillLocationRef {
            environment: EnvironmentRef::Native,
            scope: SkillLocation::Global,
        };
        let _guard = controller
            .begin_mutation(MutationKind::Install, context.clone())
            .expect("occupy controller");
        let controller_revision = controller.snapshot().revision;
        let repository_before = std::fs::read(&repository_path).expect("repository bytes");
        let actual_revision = service.registry_snapshot(true).revision.clone();

        let save_error = save_custom_agent_with_controller(
            &service,
            &controller,
            context.clone(),
            custom_definition("new-agent", ".new-agent"),
            "stale-revision".to_string(),
        )
        .expect_err("stale save before busy");
        let delete_error = delete_custom_agent_with_controller(
            &service,
            &controller,
            context,
            AgentId::parse("source-agent").unwrap(),
            "stale-revision".to_string(),
        )
        .expect_err("stale delete before busy");
        let expected = AgentCommandError::StaleRegistryRevision {
            expected: "stale-revision".to_string(),
            actual: actual_revision,
        };

        assert_eq!(save_error, expected);
        assert_eq!(delete_error, expected);
        assert_eq!(controller.snapshot().revision, controller_revision);
        assert_eq!(std::fs::read(repository_path).unwrap(), repository_before);
    }

    #[test]
    fn invalid_record_delete_uses_definition_mutation_admission_before_writing() {
        let invalid = CustomAgentRecord::Invalid {
            index: 0,
            raw: json!({ "broken": true }),
            errors: vec![AgentFieldError::new("record", "invalidDefinition")],
        };
        let (_temp, repository) = repository_with_records(vec![invalid]);
        let path = repository.path().to_path_buf();
        let service = ManagedAgentRegistry::from_repository(repository);
        let controller = RuntimeAdmissionCoordinator::default();
        let context = SkillLocationRef {
            environment: EnvironmentRef::Native,
            scope: SkillLocation::Global,
        };
        let _guard = controller
            .begin_mutation(MutationKind::Install, context.clone())
            .expect("occupy controller");
        let before = std::fs::read(&path).expect("repository bytes");

        let error = delete_invalid_custom_agent_with_controller(
            &service,
            &controller,
            context,
            0,
            service.registry_snapshot(true).revision.clone(),
        )
        .expect_err("busy admission blocks invalid record deletion");

        assert_eq!(
            error,
            AgentCommandError::Application {
                error: AppError::MutationBusy,
            }
        );
        assert_eq!(std::fs::read(path).unwrap(), before);
    }

    #[test]
    fn busy_controller_preserves_unavailable_storage_error() {
        let expected_error = AppError::Path {
            message: "custom agent storage unavailable".to_string(),
        };
        let service =
            ManagedAgentRegistry::from_repository_initializer(|| Err(expected_error.clone()));
        let controller = RuntimeAdmissionCoordinator::default();
        let context = SkillLocationRef {
            environment: EnvironmentRef::Native,
            scope: SkillLocation::Global,
        };
        let _guard = controller
            .begin_mutation(MutationKind::Install, context.clone())
            .expect("occupy controller");
        let controller_revision = controller.snapshot().revision;

        let save_error = save_custom_agent_with_controller(
            &service,
            &controller,
            context.clone(),
            custom_definition("new-agent", ".new-agent"),
            service.registry_snapshot(true).revision.clone(),
        )
        .expect_err("storage error before busy");
        let delete_error = delete_custom_agent_with_controller(
            &service,
            &controller,
            context,
            AgentId::parse("missing-agent").unwrap(),
            service.registry_snapshot(true).revision.clone(),
        )
        .expect_err("storage error before busy");
        let expected = AgentCommandError::Application {
            error: expected_error,
        };

        assert_eq!(save_error, expected);
        assert_eq!(delete_error, expected);
        assert_eq!(controller.snapshot().revision, controller_revision);
    }

    #[test]
    fn settings_guard_blocks_definition_save_and_delete() {
        let source = custom_definition("source-agent", ".source-agent");
        let (_temp, repository) = repository_with_records(vec![CustomAgentRecord::valid(source)]);
        let repository_path = repository.path().to_path_buf();
        let service = ManagedAgentRegistry::from_repository(repository);
        let controller = RuntimeAdmissionCoordinator::default();
        let context = SkillLocationRef {
            environment: EnvironmentRef::Native,
            scope: SkillLocation::Global,
        };
        let r1 = service.registry_snapshot(true);
        let repository_before = std::fs::read(&repository_path).expect("repository bytes");
        let barrier = Arc::new(Barrier::new(2));
        let guard = controller
            .begin_mutation(MutationKind::UpdateSettings, context.clone())
            .expect("settings guard");

        let save_error = std::thread::scope(|scope| {
            let barrier_for_save = Arc::clone(&barrier);
            let service = &service;
            let controller = &controller;
            let context = context.clone();
            let revision = r1.revision.clone();
            let handle = scope.spawn(move || {
                barrier_for_save.wait();
                save_custom_agent_with_controller(
                    service,
                    controller,
                    context,
                    custom_definition("new-agent", ".new-agent"),
                    revision,
                )
                .expect_err("overlapping definition save")
            });
            barrier.wait();
            handle.join().expect("save thread")
        });
        let delete_error = delete_custom_agent_with_controller(
            &service,
            &controller,
            context.clone(),
            AgentId::parse("source-agent").unwrap(),
            r1.revision.clone(),
        )
        .expect_err("overlapping definition delete");
        let expected_busy = AgentCommandError::Application {
            error: AppError::MutationBusy,
        };

        assert_eq!(save_error, expected_busy);
        assert_eq!(delete_error, expected_busy);
        assert_eq!(service.registry_snapshot(true).revision, r1.revision);
        assert_eq!(std::fs::read(&repository_path).unwrap(), repository_before);

        drop(guard);
    }

    #[test]
    fn selected_wsl_workspace_builds_the_current_environment_context() {
        let session = WslSession {
            distro_name: "Ubuntu-24.04".to_string(),
            user: "alice".to_string(),
            uid: 1000,
            home: "/home/alice".to_string(),
            xdg_state_home: Some("/home/alice/.local/state".to_string()),
            config_home: "/home/alice/.config".to_string(),
            environment: BTreeMap::from([
                ("CODEX_HOME".to_string(), "/opt/codex".to_string()),
                ("GROK_HOME".to_string(), "/opt/grok".to_string()),
            ]),
            runtime_generation: 0,
        };
        let resolved = ResolvedContext {
            context: SkillLocationRef {
                environment: EnvironmentRef::Wsl {
                    distro_name: session.distro_name.clone(),
                },
                scope: SkillLocation::Global,
            },
            project: None,
            home: ResourceLocator {
                environment: EnvironmentRef::Wsl {
                    distro_name: session.distro_name.clone(),
                },
                native_path: session.home.clone(),
            },
            skill_root: ResourceLocator {
                environment: EnvironmentRef::Wsl {
                    distro_name: session.distro_name.clone(),
                },
                native_path: "/home/alice/.agents/skills".to_string(),
            },
            lock: ResourceLocator {
                environment: EnvironmentRef::Wsl {
                    distro_name: session.distro_name.clone(),
                },
                native_path: "/home/alice/.agents/.skill-lock.json".to_string(),
            },
        };

        let runtime = WslRuntime::default();
        let workspace = runtime.workspace(&session.distro_name).unwrap();
        let context = wsl_environment_context(&resolved, session.clone(), workspace.clone());

        assert_eq!(
            context.environment,
            EnvironmentRef::Wsl {
                distro_name: "Ubuntu-24.04".to_string()
            }
        );
        assert_eq!(context.home, "/home/alice");
        assert_eq!(context.config_home, "/home/alice/.config");
        assert_eq!(context.environment_variables["CODEX_HOME"], "/opt/codex");
        assert_eq!(context.environment_variables["GROK_HOME"], "/opt/grok");
        assert_eq!(context.wsl_workspace, Some(workspace));
        assert_ne!(context.revision, "compatibility-native");
    }

    fn revision_only_runtime(registry: &str, environment: &str) -> AgentRuntimeSnapshot {
        AgentRuntimeSnapshot {
            registry_revision: registry.to_string(),
            environment_revision: environment.to_string(),
            environment: EnvironmentRef::Native,
            availability: EnvironmentStatus::Available,
            project_path: None,
            agents: BTreeMap::new(),
        }
    }

    #[test]
    fn runtime_revision_guard_rejects_registry_or_environment_drift() {
        let captured = revision_only_runtime("registry-1", "environment-1");

        assert_runtime_revisions_match(
            &captured,
            &revision_only_runtime("registry-1", "environment-1"),
        )
        .expect("matching revisions");

        let error = assert_runtime_revisions_match(
            &captured,
            &revision_only_runtime("registry-2", "environment-3"),
        )
        .expect_err("stale runtime");
        assert_eq!(
            error,
            AppError::StaleAgentRuntime {
                expected_registry_revision: "registry-1".to_string(),
                actual_registry_revision: "registry-2".to_string(),
                expected_environment_revision: "environment-1".to_string(),
                actual_environment_revision: "environment-3".to_string(),
            }
        );
    }

    #[tokio::test]
    async fn runtime_revision_guard_recaptures_once_before_comparing() {
        let captures = std::sync::atomic::AtomicUsize::new(0);
        let captured = revision_only_runtime("registry-1", "environment-1");

        assert_runtime_snapshot_current_with(&captured, || {
            captures.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            std::future::ready(Ok(revision_only_runtime("registry-1", "environment-1")))
        })
        .await
        .expect("current runtime");

        assert_eq!(captures.load(std::sync::atomic::Ordering::SeqCst), 1);
    }
}

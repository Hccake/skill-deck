use std::collections::{BTreeMap, BTreeSet};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use specta::Type;

use crate::application::agent_intent::{
    validate_agent_intents, AgentTargetFallbackPreview, AgentWriteIntent, PrivateEntryIntent,
};
use crate::application::install::InstallPlanExecutor;
use crate::application::install_planner::{InstallPlanningFactSource, InstallPlanningFacts};
use crate::application::mutation::plan::PreviewToken;
use crate::application::mutation::plan::{
    group_physical_mutations, preview_token, stable_digest, ExecutionUnit, ExpectedTargetEntry,
    MutationPlan, PreparedEntryAction, PreparedEntryMutation, PreviewFingerprint, RuntimeRevisions,
};
use crate::application::mutation::result::{MutationUnitResult, OperationErrorCode};
use crate::application::payload_session::{
    AcquiredPayloadHandle, CopySourceSnapshot, PayloadSessionManager, PinnedPayloadLease,
};
use crate::application::skill_entries::{
    join_entry, InstalledSkillPayloadAcquirer, SkillEntryObserver,
};
use crate::application::workflow_planner::{resolve_agent_entry_plan, AgentEntryPlan};
use crate::core::agent_definition::AgentId;
use crate::core::mutation::{CancellationSignal, MutationKind};
use crate::core::skill_payload::{validate_manifest_for_target, PayloadId, TargetPathProfile};
use crate::environment::agent_environment::DetectionState;
use crate::environment::planning::{ResolvedTargetFact, TargetFactResolver};
use crate::environment::runtime::{
    ContextSnapshotRevision, ExecutionBackend, PhysicalIdentityComparison,
};
use crate::environment::types::{
    same_environment_identity, ContextRef, EnvironmentRef, ResourceLocator, StorageAccess,
};
use crate::error::AppError;
use crate::models::InstallMode;
use crate::storage::lock_plan::{LockExpectedState, PreparedLockMutation};
use uuid::Uuid;

pub type CopyAgentIntent = AgentWriteIntent;

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct CopyRequest {
    pub skill_name: String,
    pub source: ContextRef,
    pub target_environment: EnvironmentRef,
    pub target_project_ids: Vec<String>,
    pub requested_mode: InstallMode,
    pub agent_intents: Vec<CopyAgentIntent>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct CopyExecutionRequest {
    pub request: CopyRequest,
    pub token: PreviewToken,
    pub payload: AcquiredPayloadHandle,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct CopyPreview {
    pub token: PreviewToken,
    pub payload: AcquiredPayloadHandle,
    pub source: ContextRef,
    pub target_environment: EnvironmentRef,
    pub targets: Vec<CopyTargetPreview>,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(tag = "status", rename_all = "camelCase")]
#[specta(tag = "status", rename_all = "camelCase")]
#[allow(
    clippy::large_enum_variant,
    reason = "IPC 结果保持直接 DTO，避免只为内存布局增加 Box 和调用侧解包"
)]
pub enum CopyPreviewOutcome {
    Ready { preview: CopyPreview },
    SourceRepairRequired { reason: CopySourceRepairReason },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub enum CopySourceRepairReason {
    MissingMetadata,
    InvalidMetadata,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct CopyTargetPreview {
    pub project_id: String,
    pub display_name: String,
    pub storage_access: StorageAccess,
    pub physical_identity: PhysicalIdentityComparison,
    pub agent_targets: Vec<AgentTargetPreview>,
    pub fallback_forecasts: Vec<AgentTargetFallbackPreview>,
    pub blocking_reasons: Vec<OperationErrorCode>,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct AgentTargetPreview {
    pub agent_id: AgentId,
    pub target_id: String,
    pub display_path: ResourceLocator,
    pub private_entry: PrivateEntryIntent,
    pub availability: DetectionState,
    pub blocking_reason: Option<OperationErrorCode>,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct CopyResponse {
    pub units: Vec<MutationUnitResult>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectComparison {
    pub physical_identity: PhysicalIdentityComparison,
    pub target_storage_access: StorageAccess,
}

pub type CopyFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

pub trait CopyProjectComparator: Send + Sync {
    fn capture_source<'a>(
        &'a self,
        source: &'a InstallPlanningFacts,
    ) -> CopyFuture<'a, Result<ResolvedTargetFact, AppError>>;

    fn compare<'a>(
        &'a self,
        source: &'a ResolvedTargetFact,
        target: &'a InstallPlanningFacts,
    ) -> CopyFuture<'a, Result<ProjectComparison, AppError>>;
}

pub struct CopyService<F, T, E, C> {
    facts: F,
    targets: T,
    observer: SkillEntryObserver<F, T>,
    payloads: Arc<PayloadSessionManager>,
    acquirer: InstalledSkillPayloadAcquirer,
    executor: E,
    comparator: C,
}

impl<F, T, E, C> CopyService<F, T, E, C>
where
    F: InstallPlanningFactSource + Clone,
    T: TargetFactResolver + Clone,
    E: InstallPlanExecutor,
    C: CopyProjectComparator,
{
    pub fn new(
        facts: F,
        targets: T,
        payloads: Arc<PayloadSessionManager>,
        acquirer: InstalledSkillPayloadAcquirer,
        executor: E,
        comparator: C,
    ) -> Self {
        Self {
            observer: SkillEntryObserver::new(facts.clone(), targets.clone()),
            facts,
            targets,
            payloads,
            acquirer,
            executor,
            comparator,
        }
    }

    pub async fn preview(&self, request: &CopyRequest) -> Result<CopyPreviewOutcome, AppError> {
        validate_copy_request(request)?;
        let source = self
            .observer
            .observe(&request.source, &request.skill_name)
            .await?;
        let source_lock_entry = source
            .facts
            .lock_document
            .entry_snapshot(&request.skill_name)
            .value()
            .cloned();
        if let Err(error) = normalize_copy_metadata(source_lock_entry.as_ref()) {
            return Ok(CopyPreviewOutcome::SourceRepairRequired {
                reason: error.repair_reason(),
            });
        }
        let handle = self
            .acquirer
            .acquire(&request.source, &request.skill_name, &source.canonical)
            .await?;
        let payload = self.payloads.pin_verified(&handle).await?;
        let source_snapshot = CopySourceSnapshot {
            source_context: request.source.clone(),
            skill_name: request.skill_name.clone(),
            revisions: source.facts.revisions.clone(),
            lock_entry: source_lock_entry,
            project_identity: self.comparator.capture_source(&source.facts).await?,
        };
        self.payloads
            .bind_copy_source_snapshot(&handle, source_snapshot.clone())?;
        let built = self
            .build(request, &source_snapshot, payload, false)
            .await?;
        Ok(CopyPreviewOutcome::Ready {
            preview: CopyPreview {
                token: built.token,
                payload: handle,
                source: request.source.clone(),
                target_environment: request.target_environment.clone(),
                targets: built.previews,
            },
        })
    }

    pub async fn execute(
        &self,
        execution: &CopyExecutionRequest,
        cancellation: CancellationSignal,
    ) -> Result<CopyResponse, AppError> {
        validate_copy_request(&execution.request)?;
        let payload = self.payloads.pin_verified(&execution.payload).await?;
        let source_snapshot = self.payloads.copy_source_snapshot(&execution.payload)?;
        validate_copy_source_snapshot(&source_snapshot, &execution.request)?;
        let built = self
            .build(&execution.request, &source_snapshot, payload, true)
            .await?;
        validate_copy_token(&execution.token, &built.token)?;
        if built.previews.iter().any(|target| {
            target
                .blocking_reasons
                .contains(&OperationErrorCode::SelfCopy)
        }) {
            return Err(AppError::SelfCopy);
        }
        Ok(CopyResponse {
            units: self
                .executor
                .execute(built.plan.expect("execute build has a plan"), cancellation)
                .await,
        })
    }

    async fn build(
        &self,
        request: &CopyRequest,
        source: &CopySourceSnapshot,
        canonical_payload: PinnedPayloadLease,
        include_plan: bool,
    ) -> Result<BuiltCopy, AppError> {
        let source_lock_entry = source.lock_entry.as_ref();
        let mut targets = Vec::with_capacity(request.target_project_ids.len());
        let canonical = canonical_payload.load_payload().await?;
        let mut needs_eve = false;
        for project_id in &request.target_project_ids {
            let context = project_context(&request.target_environment, project_id);
            let facts = self.facts.current(&context).await?;
            let agent_plan =
                resolve_agent_entry_plan(&context, &facts.agent_runtime, &request.agent_intents)?;
            needs_eve |= agent_plan
                .required_agent_roots
                .iter()
                .any(|target| target.target_id.starts_with("eve:"));
            let destinations = std::iter::once(join_entry(
                &facts.resolved_context.skill_root,
                &request.skill_name,
            ))
            .chain(
                agent_plan
                    .required_agent_roots
                    .iter()
                    .map(|target| join_entry(&target.root, &request.skill_name)),
            )
            .collect::<Vec<_>>();
            let target_facts = self.targets.resolve(&context, &destinations, None).await?;
            if target_facts.len() != destinations.len() {
                return Err(AppError::StaleTarget);
            }
            let comparison = self
                .comparator
                .compare(&source.project_identity, &facts)
                .await?;
            targets.push(BuiltCopyTarget {
                context,
                facts,
                agent_plan,
                target_facts,
                comparison,
            });
        }
        let eve_payload = if needs_eve {
            let derived = crate::core::eve::derive_eve_skill_payload(&canonical)?;
            Some(
                self.payloads
                    .pin_derived_payload(&canonical_payload, "eve-copy", derived)
                    .await?,
            )
        } else {
            None
        };
        for target in &targets {
            validate_copy_payload_targets(
                &canonical,
                eve_payload.as_ref(),
                &target.agent_plan,
                &target.target_facts,
            )
            .await?;
        }
        let revisions = aggregate_copy_revisions(&source.revisions, &targets)?;
        let observed_state_digest = stable_digest(&(
            &request.skill_name,
            &canonical_payload.manifest().payload_id(),
            &canonical_payload.manifest().payload_root_hash,
            source_lock_entry,
            (
                &source.project_identity.key,
                &source.project_identity.fingerprint,
            ),
            targets
                .iter()
                .map(|target| {
                    (
                        &target.context,
                        target
                            .target_facts
                            .iter()
                            .map(|fact| (&fact.key, &fact.fingerprint))
                            .collect::<Vec<_>>(),
                        target.comparison.physical_identity,
                    )
                })
                .collect::<Vec<_>>(),
        ))?;
        let token = preview_token(&PreviewFingerprint {
            kind: MutationKind::Copy,
            request_digest: stable_digest(request)?,
            revisions,
            observed_state_digest,
            planner_contract_version: 1,
        })?;
        let previews = targets
            .iter()
            .map(|target| copy_target_preview(request, target))
            .collect::<Result<Vec<_>, _>>()?;
        let plan = if include_plan {
            let computed_hash = canonical_payload.planning_metadata().computed_hash.clone();
            let units = targets
                .iter()
                .map(|target| {
                    build_copy_unit(
                        request,
                        target,
                        &canonical_payload,
                        eve_payload.as_ref(),
                        source_lock_entry,
                        &computed_hash,
                    )
                })
                .collect::<Result<Vec<_>, _>>()?;
            let mut payloads = vec![canonical_payload];
            if let Some(eve) = eve_payload {
                payloads.push(eve);
            }
            Some(MutationPlan {
                operation_id: Uuid::new_v4().simple().to_string(),
                payloads: payloads
                    .into_iter()
                    .map(|payload| (payload.manifest().payload_id().clone(), payload))
                    .collect::<BTreeMap<PayloadId, PinnedPayloadLease>>(),
                units,
            })
        } else {
            None
        };
        Ok(BuiltCopy {
            token,
            previews,
            plan,
        })
    }
}

struct BuiltCopy {
    token: PreviewToken,
    previews: Vec<CopyTargetPreview>,
    plan: Option<MutationPlan>,
}

struct BuiltCopyTarget {
    context: ContextRef,
    facts: InstallPlanningFacts,
    agent_plan: AgentEntryPlan,
    target_facts: Vec<ResolvedTargetFact>,
    comparison: ProjectComparison,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NormalizedCopyMetadata {
    pub source: String,
    pub source_type: String,
    pub source_url: Option<String>,
    pub ref_name: Option<String>,
    pub skill_path: Option<String>,
    pub upstream_revision: Option<String>,
    pub plugin_name: Option<String>,
}

pub fn validate_copy_request(request: &CopyRequest) -> Result<(), AppError> {
    if request.skill_name.trim().is_empty()
        || request.target_project_ids.is_empty()
        || !matches!(
            request.source.scope,
            crate::environment::types::ContextScope::Project { .. }
        )
    {
        return Err(validation("Skill and target projects are required"));
    }
    let mut projects = BTreeSet::new();
    if request
        .target_project_ids
        .iter()
        .any(|project| project.trim().is_empty() || !projects.insert(project))
    {
        return Err(validation("invalid or duplicate target project"));
    }
    validate_agent_intents(&request.agent_intents)
}

fn project_context(environment: &EnvironmentRef, project_id: &str) -> ContextRef {
    ContextRef {
        environment: environment.clone(),
        scope: crate::environment::types::ContextScope::Project {
            project_id: project_id.to_string(),
        },
    }
}

fn aggregate_copy_revisions(
    source: &RuntimeRevisions,
    targets: &[BuiltCopyTarget],
) -> Result<RuntimeRevisions, AppError> {
    let all = std::iter::once(source)
        .chain(targets.iter().map(|target| &target.facts.revisions))
        .collect::<Vec<_>>();
    let registry = stable_digest(
        &all.iter()
            .map(|revision| &revision.registry)
            .collect::<Vec<_>>(),
    )?;
    let environment = stable_digest(
        &all.iter()
            .map(|revision| &revision.environment)
            .collect::<Vec<_>>(),
    )?;
    let context_digest = stable_digest(
        &all.iter()
            .map(|revision| revision.context.as_str())
            .collect::<Vec<_>>(),
    )?;
    Ok(RuntimeRevisions {
        registry,
        environment,
        context: ContextSnapshotRevision::parse(format!(
            "context-copy-{}",
            context_digest.trim_start_matches("digest-v1-")
        ))?,
    })
}

fn validate_copy_source_snapshot(
    snapshot: &CopySourceSnapshot,
    request: &CopyRequest,
) -> Result<(), AppError> {
    if snapshot.skill_name != request.skill_name
        || snapshot.source_context.scope != request.source.scope
        || !same_environment_identity(
            &snapshot.source_context.environment,
            &request.source.environment,
        )
    {
        return Err(AppError::StalePayload);
    }
    Ok(())
}

fn validate_copy_token(expected: &PreviewToken, actual: &PreviewToken) -> Result<(), AppError> {
    if expected.registry_revision != actual.registry_revision {
        return Err(AppError::StaleRegistry);
    }
    if expected.environment_revision != actual.environment_revision {
        return Err(AppError::StaleEnvironment);
    }
    if expected.context_revision != actual.context_revision
        || expected.generation != actual.generation
    {
        return Err(AppError::StaleContext);
    }
    Ok(())
}

async fn validate_copy_payload_targets(
    canonical: &crate::core::skill_payload::SkillPayload,
    eve_payload: Option<&PinnedPayloadLease>,
    agent_plan: &AgentEntryPlan,
    facts: &[ResolvedTargetFact],
) -> Result<(), AppError> {
    if facts.len() != 1 + agent_plan.required_agent_roots.len() {
        return Err(AppError::StaleTarget);
    }
    let eve = match eve_payload {
        Some(payload) => Some(payload.load_payload().await?),
        None => None,
    };
    for (index, fact) in facts.iter().enumerate() {
        let payload = if index > 0
            && agent_plan.required_agent_roots[index - 1]
                .target_id
                .starts_with("eve:")
        {
            eve.as_ref().ok_or(AppError::StalePayload)?
        } else {
            canonical
        };
        let profile = match fact.key.backend {
            ExecutionBackend::NativeWindows => TargetPathProfile::native_windows(),
            ExecutionBackend::NativeUnix | ExecutionBackend::WslPosix { .. } => {
                TargetPathProfile::native_unix()
            }
        };
        validate_manifest_for_target(payload, &fact.destination, &profile)?;
    }
    Ok(())
}

fn copy_target_preview(
    request: &CopyRequest,
    target: &BuiltCopyTarget,
) -> Result<CopyTargetPreview, AppError> {
    let project = target
        .facts
        .resolved_context
        .project
        .as_ref()
        .ok_or(AppError::StaleContext)?;
    let mut agent_targets = Vec::new();
    for agent_id in &target.agent_plan.canonical_owner_agent_ids {
        let agent = target
            .facts
            .agent_runtime
            .agents
            .get(agent_id)
            .ok_or(AppError::StaleRegistry)?;
        agent_targets.push(AgentTargetPreview {
            agent_id: agent_id.clone(),
            target_id: "canonical".to_string(),
            display_path: target.target_facts[0].destination.clone(),
            private_entry: private_intent(request, agent_id),
            availability: agent.detection,
            blocking_reason: None,
        });
    }
    for (logical, fact) in target
        .agent_plan
        .required_agent_roots
        .iter()
        .zip(&target.target_facts[1..])
    {
        for agent_id in &logical.owner_agent_ids {
            let agent = target
                .facts
                .agent_runtime
                .agents
                .get(agent_id)
                .ok_or(AppError::StaleRegistry)?;
            agent_targets.push(AgentTargetPreview {
                agent_id: agent_id.clone(),
                target_id: logical.target_id.clone(),
                display_path: fact.destination.clone(),
                private_entry: private_intent(request, agent_id),
                availability: agent.detection,
                blocking_reason: None,
            });
        }
    }
    let blocking_reasons = (target.comparison.physical_identity
        == PhysicalIdentityComparison::Same)
        .then_some(OperationErrorCode::SelfCopy)
        .into_iter()
        .collect();
    Ok(CopyTargetPreview {
        project_id: project.id.clone(),
        display_name: project
            .display_name
            .clone()
            .unwrap_or_else(|| project.native_path.clone()),
        storage_access: target.comparison.target_storage_access,
        physical_identity: target.comparison.physical_identity,
        agent_targets,
        fallback_forecasts: Vec::new(),
        blocking_reasons,
    })
}

fn private_intent(request: &CopyRequest, agent_id: &AgentId) -> PrivateEntryIntent {
    request
        .agent_intents
        .iter()
        .find(|intent| &intent.agent_id == agent_id)
        .map(|intent| intent.private_entry)
        .unwrap_or(PrivateEntryIntent::None)
}

fn build_copy_unit(
    request: &CopyRequest,
    target: &BuiltCopyTarget,
    canonical_payload: &PinnedPayloadLease,
    eve_payload: Option<&PinnedPayloadLease>,
    source_lock_entry: Option<&Value>,
    computed_hash: &str,
) -> Result<ExecutionUnit, AppError> {
    let canonical_id = canonical_payload.manifest().payload_id().clone();
    let canonical = PreparedEntryMutation {
        key: target.target_facts[0].key.clone(),
        destination: target.target_facts[0].destination.clone(),
        action: PreparedEntryAction::Replace {
            payload_id: canonical_id.clone(),
            requested_mode: InstallMode::Copy,
        },
        owner_agent_ids: target.agent_plan.canonical_owner_agent_ids.clone(),
    };
    let required = target
        .agent_plan
        .required_agent_roots
        .iter()
        .zip(&target.target_facts[1..])
        .map(|(logical, fact)| {
            let is_eve = logical.target_id.starts_with("eve:");
            Ok(PreparedEntryMutation {
                key: fact.key.clone(),
                destination: fact.destination.clone(),
                action: PreparedEntryAction::Replace {
                    payload_id: if is_eve {
                        eve_payload
                            .ok_or(AppError::StalePayload)?
                            .manifest()
                            .payload_id()
                            .clone()
                    } else {
                        canonical_id.clone()
                    },
                    requested_mode: if is_eve {
                        InstallMode::Copy
                    } else {
                        request.requested_mode.clone()
                    },
                },
                owner_agent_ids: logical.owner_agent_ids.clone(),
            })
        })
        .collect::<Result<Vec<_>, AppError>>()?;
    let grouped =
        group_physical_mutations(std::iter::once(canonical.clone()).chain(required).collect())?;
    let canonical_entry = grouped
        .iter()
        .find(|entry| entry.key == canonical.key)
        .cloned();
    let required_agent_entries = grouped
        .into_iter()
        .filter(|entry| entry.key != canonical.key)
        .collect();
    let lock_mutation = PreparedLockMutation {
        target: target.facts.resolved_context.lock.clone(),
        legacy_target: None,
        schema: target.facts.lock_schema,
        skill_name: request.skill_name.clone(),
        replacement: Some(project_lock_replacement(source_lock_entry, computed_hash)?),
        root_replacements: BTreeMap::new(),
        expected: LockExpectedState::capture(
            &target.facts.lock_document,
            [&request.skill_name],
            std::iter::empty::<&str>(),
        ),
    };
    Ok(ExecutionUnit {
        id: format!(
            "copy:{}:{}",
            request.skill_name,
            project_id(&target.context)?
        ),
        skill_name: request.skill_name.clone(),
        source: Some(request.source.clone()),
        target: target.context.clone(),
        expected_revisions: target.facts.revisions.clone(),
        canonical_entry,
        required_agent_entries,
        lock_mutation: Some(lock_mutation),
        expected_targets: target
            .target_facts
            .iter()
            .map(|fact| ExpectedTargetEntry {
                key: fact.key.clone(),
                fingerprint: fact.fingerprint.clone(),
                expected_content_manifest_hash: None,
            })
            .collect(),
    })
}

fn project_id(context: &ContextRef) -> Result<&str, AppError> {
    match &context.scope {
        crate::environment::types::ContextScope::Project { project_id } => Ok(project_id),
        crate::environment::types::ContextScope::Global => Err(AppError::StaleContext),
    }
}

fn validation(message: &str) -> AppError {
    AppError::Validation {
        field: Some("request".to_string()),
        message: message.to_string(),
    }
}

pub(crate) fn compare_resolved_projects(
    left: &crate::environment::planning::ResolvedTargetFact,
    right: &crate::environment::planning::ResolvedTargetFact,
) -> Result<PhysicalIdentityComparison, AppError> {
    if left.key.backend != right.key.backend
        || !crate::environment::types::same_environment_identity(
            &left.destination.environment,
            &right.destination.environment,
        )
    {
        return Ok(PhysicalIdentityComparison::Unknown);
    }
    if left.key == right.key {
        return Ok(PhysicalIdentityComparison::Same);
    }
    let case_sensitive = !matches!(left.key.backend, ExecutionBackend::NativeWindows);
    if crate::environment::runtime::physical_paths_overlap(
        &left.destination,
        &right.destination,
        case_sensitive,
    )? {
        Ok(PhysicalIdentityComparison::Same)
    } else {
        Ok(PhysicalIdentityComparison::Different)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CopyMetadataError {
    Missing,
    Invalid(&'static str),
}

impl CopyMetadataError {
    fn repair_reason(self) -> CopySourceRepairReason {
        match self {
            Self::Missing => CopySourceRepairReason::MissingMetadata,
            Self::Invalid(_) => CopySourceRepairReason::InvalidMetadata,
        }
    }

    fn into_app_error(self) -> AppError {
        match self {
            Self::Missing => AppError::InvalidSource {
                value: "source Skill has no lock metadata; repair its source before copying"
                    .to_string(),
            },
            Self::Invalid(message) => AppError::ConfigurationCorrupted {
                message: message.to_string(),
            },
        }
    }
}

fn normalize_copy_metadata(
    source: Option<&Value>,
) -> Result<NormalizedCopyMetadata, CopyMetadataError> {
    let source = source.ok_or(CopyMetadataError::Missing)?;
    if !source.is_object() {
        return Err(CopyMetadataError::Invalid(
            "source lock entry must be an object",
        ));
    }
    let text = |field: &str| {
        source
            .get(field)
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    };
    let source_value = text("source").ok_or(CopyMetadataError::Invalid(
        "source lock entry is missing source identity",
    ))?;
    let source_type = text("sourceType").ok_or(CopyMetadataError::Invalid(
        "source lock entry is missing source type",
    ))?;
    Ok(NormalizedCopyMetadata {
        source: source_value,
        source_type,
        source_url: text("sourceUrl"),
        ref_name: text("ref"),
        skill_path: text("skillPath"),
        upstream_revision: text("remoteHash").or_else(|| {
            matches!(
                text("sourceType").as_deref(),
                Some("github" | "gitlab" | "git")
            )
            .then(|| text("skillFolderHash"))
            .flatten()
        }),
        plugin_name: text("pluginName"),
    })
}

fn project_lock_replacement(
    source: Option<&Value>,
    computed_hash: &str,
) -> Result<Value, AppError> {
    let metadata = normalize_copy_metadata(source).map_err(CopyMetadataError::into_app_error)?;
    let mut entry = Map::new();
    entry.insert("source".to_string(), Value::String(metadata.source));
    entry.insert(
        "sourceType".to_string(),
        Value::String(metadata.source_type),
    );
    entry.insert(
        "sourceUrl".to_string(),
        serde_json::json!(metadata.source_url),
    );
    entry.insert("ref".to_string(), serde_json::json!(metadata.ref_name));
    entry.insert(
        "skillPath".to_string(),
        serde_json::json!(metadata.skill_path),
    );
    entry.insert(
        "computedHash".to_string(),
        Value::String(computed_hash.to_string()),
    );
    entry.insert(
        "pluginName".to_string(),
        serde_json::json!(metadata.plugin_name),
    );
    if let Some(remote_hash) = metadata.upstream_revision {
        entry.insert("remoteHash".to_string(), Value::String(remote_hash));
    }
    Ok(Value::Object(entry))
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::fs;
    use std::sync::Mutex;

    use tempfile::tempdir;

    use super::*;
    use crate::application::install::InstallFuture;
    use crate::application::payload_session::PayloadSessionLimits;
    use crate::core::lossless_lock::{LockSchema, LosslessLockDocument};
    use crate::environment::agent_environment::AgentRuntimeSnapshot;
    use crate::environment::context_resolver::ResolvedContext;
    use crate::environment::native::acquire::NativePayloadSessionStorage;
    use crate::environment::planning::{ResolvedTargetFact, TargetEntryKind};
    use crate::environment::runtime::{
        EntryFingerprint, ExecutionBackend, PhysicalParentIdentity, PhysicalTargetKey,
    };
    use crate::environment::types::{
        ContextRef, ContextScope, EnvironmentRef, EnvironmentStatus, ProjectBinding,
    };
    use crate::environment::wsl::EnvironmentRegistry;
    use crate::models::InstallMode;

    #[test]
    fn copy_request_has_one_target_environment_and_unique_projects() {
        let request = CopyRequest {
            skill_name: "demo".to_string(),
            source: ContextRef {
                environment: EnvironmentRef::Host,
                scope: ContextScope::Global,
            },
            target_environment: EnvironmentRef::Wsl {
                distro_name: "Ubuntu".to_string(),
            },
            target_project_ids: vec!["project-1".to_string(), "project-1".to_string()],
            requested_mode: InstallMode::Copy,
            agent_intents: Vec::new(),
        };
        assert!(validate_copy_request(&request).is_err());
        let value = serde_json::to_value(request).unwrap();
        assert!(value.get("targetEnvironments").is_none());
        assert_eq!(value["targetEnvironment"]["kind"], "wsl");
    }

    #[test]
    fn copy_source_must_be_a_project_context() {
        let request = CopyRequest {
            skill_name: "demo".to_string(),
            source: ContextRef {
                environment: EnvironmentRef::Host,
                scope: ContextScope::Global,
            },
            target_environment: EnvironmentRef::Host,
            target_project_ids: vec!["project-1".to_string()],
            requested_mode: InstallMode::Copy,
            agent_intents: Vec::new(),
        };

        assert!(validate_copy_request(&request).is_err());
    }

    fn project_fact(path: &str, inode: u64, child: &str) -> ResolvedTargetFact {
        ResolvedTargetFact {
            key: PhysicalTargetKey {
                backend: ExecutionBackend::NativeUnix,
                physical_parent: PhysicalParentIdentity::Unix { device: 1, inode },
                normalized_final_child_name: child.to_string(),
            },
            destination: ResourceLocator {
                environment: EnvironmentRef::Host,
                native_path: path.to_string(),
            },
            fingerprint: EntryFingerprint("entry-v1-project".to_string()),
            entry_kind: TargetEntryKind::Directory,
            link_target: None,
        }
    }

    #[test]
    fn physical_project_comparison_blocks_equal_and_nested_but_allows_similar_paths() {
        let source = project_fact("/work/app", 1, "app");
        let same = project_fact("/work/app", 1, "app");
        let nested = project_fact("/work/app/examples/demo", 1, "app/examples/demo");
        let similar = project_fact("/work/application", 1, "application");

        assert_eq!(
            compare_resolved_projects(&source, &same).unwrap(),
            PhysicalIdentityComparison::Same
        );
        assert_eq!(
            compare_resolved_projects(&source, &nested).unwrap(),
            PhysicalIdentityComparison::Same
        );
        assert_eq!(
            compare_resolved_projects(&source, &similar).unwrap(),
            PhysicalIdentityComparison::Different
        );
    }

    #[test]
    fn target_lock_projects_source_metadata_without_inheriting_adapter_state() {
        let source = serde_json::json!({
            "source": "owner/repo",
            "sourceType": "github",
            "sourceUrl": "https://github.com/owner/repo.git",
            "ref": "main",
            "skillPath": "skills/demo",
            "computedHash": "source-local",
            "remoteHash": "upstream-version",
            "pluginName": "toolkit",
            "subagents": ["researcher"],
            "futureField": { "keepAtSourceOnly": true }
        });

        let target = project_lock_replacement(Some(&source), "target-computed").unwrap();

        assert_eq!(target["computedHash"], "target-computed");
        assert_eq!(target["remoteHash"], "upstream-version");
        assert_eq!(target["sourceUrl"], source["sourceUrl"]);
        assert_eq!(target["skillPath"], source["skillPath"]);
        assert_eq!(target["pluginName"], source["pluginName"]);
        assert!(target.get("subagents").is_none());
        assert!(target.get("futureField").is_none());
    }

    #[test]
    fn copying_a_global_local_source_does_not_invent_project_remote_hash() {
        let source = serde_json::json!({
            "source": "/home/alice/skills",
            "sourceType": "local",
            "skillPath": "skills/demo",
            "skillFolderHash": "local-content-sha256"
        });

        let target = project_lock_replacement(Some(&source), "target-computed").unwrap();

        assert_eq!(target["computedHash"], "target-computed");
        assert!(target.get("remoteHash").is_none());
    }

    #[test]
    fn copying_without_source_lock_metadata_requires_source_repair() {
        assert!(matches!(
            project_lock_replacement(None, "target-computed"),
            Err(AppError::InvalidSource { .. })
        ));
    }

    #[derive(Clone)]
    struct Facts(Arc<Mutex<HashMap<ContextRef, InstallPlanningFacts>>>);

    impl InstallPlanningFactSource for Facts {
        fn current<'a>(
            &'a self,
            context: &'a ContextRef,
        ) -> InstallFuture<'a, Result<InstallPlanningFacts, AppError>> {
            Box::pin(async move {
                self.0
                    .lock()
                    .unwrap()
                    .get(context)
                    .cloned()
                    .ok_or(AppError::StaleContext)
            })
        }
    }

    struct DifferentProjects;

    impl CopyProjectComparator for DifferentProjects {
        fn capture_source<'a>(
            &'a self,
            source: &'a InstallPlanningFacts,
        ) -> CopyFuture<'a, Result<ResolvedTargetFact, AppError>> {
            Box::pin(async move {
                let project = source
                    .resolved_context
                    .project
                    .as_ref()
                    .ok_or(AppError::StaleContext)?;
                Ok(project_fact(&project.native_path, 1, "source"))
            })
        }

        fn compare<'a>(
            &'a self,
            _source: &'a ResolvedTargetFact,
            _target: &'a InstallPlanningFacts,
        ) -> CopyFuture<'a, Result<ProjectComparison, AppError>> {
            Box::pin(async {
                Ok(ProjectComparison {
                    physical_identity: PhysicalIdentityComparison::Different,
                    target_storage_access: StorageAccess::Native,
                })
            })
        }
    }

    #[derive(Debug)]
    struct CapturedCopyPlan {
        unit_count: usize,
        payload_paths: Vec<String>,
        remote_hashes: Vec<Option<String>>,
    }

    struct CapturingExecutor(Arc<Mutex<Option<CapturedCopyPlan>>>);

    impl InstallPlanExecutor for CapturingExecutor {
        fn execute<'a>(
            &'a self,
            plan: MutationPlan,
            _cancellation: CancellationSignal,
        ) -> InstallFuture<'a, Vec<MutationUnitResult>> {
            Box::pin(async move {
                let payload = plan
                    .payloads
                    .values()
                    .next()
                    .expect("copy payload")
                    .load_payload()
                    .await
                    .expect("load payload");
                let payload_paths = payload
                    .manifest()
                    .entries
                    .iter()
                    .map(|entry| entry.relative_path.clone())
                    .collect();
                let remote_hashes = plan
                    .units
                    .iter()
                    .map(|unit| {
                        unit.lock_mutation
                            .as_ref()
                            .and_then(|lock| lock.replacement.as_ref())
                            .and_then(|entry| entry.get("remoteHash"))
                            .and_then(Value::as_str)
                            .map(str::to_string)
                    })
                    .collect();
                *self.0.lock().unwrap() = Some(CapturedCopyPlan {
                    unit_count: plan.units.len(),
                    payload_paths,
                    remote_hashes,
                });
                Vec::new()
            })
        }
    }

    fn context(environment: EnvironmentRef, project_id: &str) -> ContextRef {
        ContextRef {
            environment,
            scope: ContextScope::Project {
                project_id: project_id.to_string(),
            },
        }
    }

    fn planning_facts(
        context: ContextRef,
        root: &std::path::Path,
        source_lock: bool,
    ) -> InstallPlanningFacts {
        let project_id = project_id(&context).unwrap().to_string();
        let lock_document = if source_lock {
            LosslessLockDocument::parse(
                br#"{"version":1,"skills":{"demo":{"source":"owner/repo","sourceType":"github","sourceUrl":"https://github.com/owner/repo.git","ref":"main","skillPath":"skills/demo","computedHash":"old-local","remoteHash":"remote-v1","pluginName":"toolkit"}}}"#,
            )
            .unwrap()
        } else {
            LosslessLockDocument::empty(LockSchema::Project)
        };
        let project = ProjectBinding {
            id: project_id.clone(),
            native_path: root.to_string_lossy().into_owned(),
            display_name: Some(project_id.clone()),
            order: None,
            suppress_cross_storage_warning: false,
        };
        InstallPlanningFacts {
            resolved_context: ResolvedContext {
                context: context.clone(),
                project: Some(project),
                home: ResourceLocator {
                    environment: context.environment.clone(),
                    native_path: root.to_string_lossy().into_owned(),
                },
                skill_root: ResourceLocator {
                    environment: context.environment.clone(),
                    native_path: root.join(".agents/skills").to_string_lossy().into_owned(),
                },
                lock: ResourceLocator {
                    environment: context.environment.clone(),
                    native_path: root.join("skills-lock.json").to_string_lossy().into_owned(),
                },
            },
            agent_runtime: AgentRuntimeSnapshot {
                registry_revision: "registry-1".to_string(),
                environment_revision: "environment-1".to_string(),
                environment: context.environment.clone(),
                availability: EnvironmentStatus::Available,
                project_path: Some(root.to_string_lossy().into_owned()),
                agents: BTreeMap::new(),
            },
            revisions: RuntimeRevisions {
                registry: "registry-1".to_string(),
                environment: "environment-1".to_string(),
                context: ContextSnapshotRevision::parse(format!("context-{project_id}")).unwrap(),
            },
            lock_schema: LockSchema::Project,
            lock_document,
        }
    }

    async fn preview_with_source_lock(
        source_lock: LosslessLockDocument,
        target_project_ids: Vec<String>,
    ) -> Result<CopyPreviewOutcome, AppError> {
        let temp = tempdir().unwrap();
        let source_root = temp.path().join("source");
        let target_root = temp.path().join("target");
        let source_skill = source_root.join(".agents/skills/demo");
        fs::create_dir_all(&source_skill).unwrap();
        fs::write(source_skill.join("SKILL.md"), b"---\nname: demo\n---\nbody").unwrap();
        fs::create_dir_all(&target_root).unwrap();

        let source = context(EnvironmentRef::Host, "source");
        let target = context(EnvironmentRef::Host, "target");
        let mut source_facts = planning_facts(source.clone(), &source_root, false);
        source_facts.lock_document = source_lock;
        let facts = Arc::new(Mutex::new(HashMap::from([
            (source.clone(), source_facts),
            (target.clone(), planning_facts(target, &target_root, false)),
        ])));
        let environments = Arc::new(EnvironmentRegistry::default());
        let storage =
            Arc::new(NativePayloadSessionStorage::new(temp.path().join("payloads")).unwrap());
        let payloads = Arc::new(PayloadSessionManager::new(
            storage,
            PayloadSessionLimits {
                ttl_ms: 60_000,
                max_sessions: 8,
                max_bytes: 1024 * 1024,
            },
            || 1_000,
        ));
        let service = CopyService::new(
            Facts(facts),
            crate::environment::planning::RuntimeTargetFactResolver::new(environments.clone()),
            payloads.clone(),
            InstalledSkillPayloadAcquirer::new(payloads, environments),
            CapturingExecutor(Arc::new(Mutex::new(None))),
            DifferentProjects,
        );
        let request = CopyRequest {
            skill_name: "demo".to_string(),
            source,
            target_environment: EnvironmentRef::Host,
            target_project_ids,
            requested_mode: InstallMode::Copy,
            agent_intents: Vec::new(),
        };

        service.preview(&request).await
    }

    #[tokio::test]
    async fn copy_preview_requests_source_repair_when_lock_entry_is_missing() {
        let outcome = preview_with_source_lock(
            LosslessLockDocument::empty(LockSchema::Project),
            vec!["target".to_string()],
        )
        .await
        .unwrap();

        assert!(matches!(
            outcome,
            CopyPreviewOutcome::SourceRepairRequired {
                reason: CopySourceRepairReason::MissingMetadata
            }
        ));
    }

    #[tokio::test]
    async fn copy_preview_requests_source_repair_when_lock_entry_is_invalid() {
        let outcome = preview_with_source_lock(
            LosslessLockDocument::parse(
                br#"{"version":1,"skills":{"demo":{"source":42,"sourceType":"github"}}}"#,
            )
            .unwrap(),
            vec!["target".to_string()],
        )
        .await
        .unwrap();

        assert!(matches!(
            outcome,
            CopyPreviewOutcome::SourceRepairRequired {
                reason: CopySourceRepairReason::InvalidMetadata
            }
        ));
    }

    #[tokio::test]
    async fn copy_preview_keeps_non_source_failures_as_app_errors() {
        let error = preview_with_source_lock(
            LosslessLockDocument::parse(
                br#"{"version":1,"skills":{"demo":{"source":"owner/repo","sourceType":"github"}}}"#,
            )
            .unwrap(),
            Vec::new(),
        )
        .await
        .unwrap_err();

        assert!(matches!(error, AppError::Validation { .. }));
    }

    #[tokio::test]
    async fn copy_preview_allows_source_metadata_without_a_remote_hash() {
        let outcome = preview_with_source_lock(
            LosslessLockDocument::parse(
                br#"{"version":1,"skills":{"demo":{"source":"owner/repo","sourceType":"github","skillPath":"skills/demo"}}}"#,
            )
            .unwrap(),
            vec!["target".to_string()],
        )
        .await
        .unwrap();

        assert!(matches!(outcome, CopyPreviewOutcome::Ready { .. }));
    }

    #[tokio::test]
    async fn copy_service_builds_two_atomic_units_from_one_complete_source_payload() {
        let temp = tempdir().unwrap();
        let source_root = temp.path().join("source");
        let first_root = temp.path().join("first");
        let second_root = temp.path().join("second");
        let source_skill = source_root.join(".agents/skills/demo");
        fs::create_dir_all(source_skill.join("scripts")).unwrap();
        fs::write(source_skill.join("SKILL.md"), b"---\nname: demo\n---\nbody").unwrap();
        fs::write(source_skill.join("scripts/run.sh"), b"#!/bin/sh\n").unwrap();
        fs::create_dir_all(&first_root).unwrap();
        fs::create_dir_all(&second_root).unwrap();

        let source = context(EnvironmentRef::Host, "source");
        let first = context(EnvironmentRef::Host, "first");
        let second = context(EnvironmentRef::Host, "second");
        let facts = Arc::new(Mutex::new(HashMap::from([
            (
                source.clone(),
                planning_facts(source.clone(), &source_root, true),
            ),
            (
                first.clone(),
                planning_facts(first.clone(), &first_root, false),
            ),
            (second.clone(), planning_facts(second, &second_root, false)),
        ])));
        let environments = Arc::new(EnvironmentRegistry::default());
        let storage =
            Arc::new(NativePayloadSessionStorage::new(temp.path().join("payloads")).unwrap());
        let payloads = Arc::new(PayloadSessionManager::new(
            storage,
            PayloadSessionLimits {
                ttl_ms: 60_000,
                max_sessions: 8,
                max_bytes: 1024 * 1024,
            },
            || 1_000,
        ));
        let captured = Arc::new(Mutex::new(None));
        let service = CopyService::new(
            Facts(facts.clone()),
            crate::environment::planning::RuntimeTargetFactResolver::new(environments.clone()),
            payloads.clone(),
            InstalledSkillPayloadAcquirer::new(payloads, environments),
            CapturingExecutor(captured.clone()),
            DifferentProjects,
        );
        let request = CopyRequest {
            skill_name: "demo".to_string(),
            source,
            target_environment: EnvironmentRef::Host,
            target_project_ids: vec!["first".to_string(), "second".to_string()],
            requested_mode: InstallMode::Symlink,
            agent_intents: Vec::new(),
        };
        let preview = match service.preview(&request).await.unwrap() {
            CopyPreviewOutcome::Ready { preview } => preview,
            CopyPreviewOutcome::SourceRepairRequired { reason } => {
                panic!("complete source metadata unexpectedly required repair: {reason:?}")
            }
        };
        assert_eq!(preview.targets.len(), 2);
        facts.lock().unwrap().remove(&request.source);

        facts
            .lock()
            .unwrap()
            .get_mut(&first)
            .unwrap()
            .revisions
            .environment = "environment-2".to_string();
        let stale_error = service
            .execute(
                &CopyExecutionRequest {
                    request: request.clone(),
                    token: preview.token.clone(),
                    payload: preview.payload.clone(),
                },
                CancellationSignal::default(),
            )
            .await
            .unwrap_err();
        assert!(matches!(stale_error, AppError::StaleEnvironment));
        facts
            .lock()
            .unwrap()
            .get_mut(&first)
            .unwrap()
            .revisions
            .environment = "environment-1".to_string();

        service
            .execute(
                &CopyExecutionRequest {
                    request,
                    token: preview.token,
                    payload: preview.payload,
                },
                CancellationSignal::default(),
            )
            .await
            .unwrap();

        let captured = captured.lock().unwrap();
        let captured = captured.as_ref().unwrap();
        assert_eq!(captured.unit_count, 2);
        assert!(captured
            .payload_paths
            .iter()
            .any(|path| path == "scripts/run.sh"));
        assert_eq!(
            captured.remote_hashes,
            vec![Some("remote-v1".to_string()), Some("remote-v1".to_string())]
        );
    }
}

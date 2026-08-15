use std::collections::BTreeSet;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::application::agent_intent::AgentTargetFallbackPreview;
use crate::application::agent_selection::{
    AgentSelectionSubmission, InstallAgentSelectionSnapshot,
};
use crate::application::mutation::executor::MutationPlanExecutor;
use crate::application::mutation::plan::{MutationPlan, PreviewToken};
use crate::application::mutation::planning::validate_exact_preview;
use crate::application::mutation::result::{MutationUnitResult, OperationErrorCode};
use crate::application::payload_session::{
    AcquiredPayloadHandle, DiscoverySessionHandle, PayloadSessionManager, PinnedPayloadLease,
};
use crate::application::source_evidence::{RemoteEvidenceKey, SourceSuppressionWarningCode};
use crate::core::mutation::CancellationSignal;
use crate::core::{parse_source, NormalizedUpdateMetadata, SourceIdentity};
use crate::environment::types::{same_environment_identity, EnvironmentRef, SkillLocationRef};
use crate::error::AppError;
use crate::models::SourceType;

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct InstallRequest {
    pub context: SkillLocationRef,
    pub source: String,
    pub discovery_session: DiscoverySessionHandle,
    pub payloads: Vec<AcquiredPayloadHandle>,
    pub skills: Vec<String>,
    pub agent_selection: AgentSelectionSubmission,
    pub acknowledge_redirect: bool,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct InstallPreview {
    pub token: PreviewToken,
    pub skills: Vec<InstallSkillPreview>,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(tag = "status", rename_all = "camelCase")]
#[specta(tag = "status", rename_all = "camelCase")]
#[allow(
    clippy::large_enum_variant,
    reason = "IPC 结果直接携带最新选择快照，调用侧无需额外读取"
)]
pub enum InstallPreviewOutcome {
    Ready {
        preview: InstallPreview,
    },
    SelectionStale {
        snapshot: InstallAgentSelectionSnapshot,
    },
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct InstallSkillPreview {
    pub skill_name: String,
    pub payload: AcquiredPayloadHandle,
    pub overwrite_targets: Vec<String>,
    pub blocking_reasons: Vec<OperationErrorCode>,
    pub fallback_forecasts: Vec<AgentTargetFallbackPreview>,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct InstallResponse {
    pub units: Vec<MutationUnitResult>,
    pub warnings: Vec<SourceSuppressionWarningCode>,
}

pub type InstallFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallOperation {
    Install,
    Repair,
}

impl InstallOperation {
    pub fn mutation_kind(self) -> crate::core::mutation::MutationKind {
        match self {
            Self::Install => crate::core::mutation::MutationKind::Install,
            Self::Repair => crate::core::mutation::MutationKind::Repair,
        }
    }
}

pub trait InstallPlanner: Send + Sync {
    fn preview<'a>(
        &'a self,
        operation: InstallOperation,
        request: &'a InstallRequest,
        payloads: Vec<PinnedPayloadLease>,
    ) -> InstallFuture<'a, Result<InstallPreviewOutcome, AppError>>;

    fn rebuild<'a>(
        &'a self,
        operation: InstallOperation,
        request: &'a InstallRequest,
        payloads: Vec<PinnedPayloadLease>,
    ) -> InstallFuture<'a, Result<(PreviewToken, MutationPlan), AppError>>;
}

type SourceSuppressionClearer =
    dyn Fn(&EnvironmentRef, &RemoteEvidenceKey) -> Result<(), AppError> + Send + Sync;

pub struct InstallService<P, E> {
    payloads: Arc<PayloadSessionManager>,
    planner: P,
    executor: E,
    source_suppression_clearer: Option<Arc<SourceSuppressionClearer>>,
}

impl<P, E> InstallService<P, E>
where
    P: InstallPlanner,
    E: MutationPlanExecutor,
{
    pub fn new(payloads: Arc<PayloadSessionManager>, planner: P, executor: E) -> Self {
        Self {
            payloads,
            planner,
            executor,
            source_suppression_clearer: None,
        }
    }

    pub fn with_source_suppression_clearer(
        mut self,
        clearer: Arc<SourceSuppressionClearer>,
    ) -> Self {
        self.source_suppression_clearer = Some(clearer);
        self
    }

    pub async fn preview(
        &self,
        operation: InstallOperation,
        request: &InstallRequest,
    ) -> Result<InstallPreviewOutcome, AppError> {
        validate_install_request(request)?;
        let payloads = self.pin_request_payloads(request, None).await?;
        self.planner.preview(operation, request, payloads).await
    }

    pub async fn execute(
        &self,
        operation: InstallOperation,
        request: &InstallRequest,
        expected_token: PreviewToken,
        cancellation: CancellationSignal,
    ) -> Result<InstallResponse, AppError> {
        validate_install_request(request)?;
        let payloads = self
            .pin_request_payloads(request, Some(cancellation.clone()))
            .await?;
        let (actual_token, plan) = self.planner.rebuild(operation, request, payloads).await?;
        validate_exact_preview(&expected_token, &actual_token)?;
        let mut response = InstallResponse {
            units: self.executor.execute(plan, cancellation).await,
            warnings: Vec::new(),
        };
        if self.clear_source_suppression_after_success(request, &response) {
            response
                .warnings
                .push(SourceSuppressionWarningCode::SuppressionCleanupFailed);
        }
        Ok(response)
    }

    fn clear_source_suppression_after_success(
        &self,
        request: &InstallRequest,
        response: &InstallResponse,
    ) -> bool {
        if response.units.is_empty()
            || response.units.iter().any(|unit| {
                unit.status != crate::application::mutation::result::MutationUnitStatus::Succeeded
            })
        {
            return false;
        }
        let Some(clearer) = &self.source_suppression_clearer else {
            return false;
        };
        let result = source_evidence_key(&request.source)
            .and_then(|key| key.map_or(Ok(()), |key| clearer(&request.context.environment, &key)));
        result.is_err()
    }

    async fn pin_request_payloads(
        &self,
        request: &InstallRequest,
        cancellation: Option<CancellationSignal>,
    ) -> Result<Vec<PinnedPayloadLease>, AppError> {
        validate_handles(request)?;
        let mut payloads = Vec::with_capacity(request.payloads.len());
        for handle in &request.payloads {
            if cancellation
                .as_ref()
                .is_some_and(|signal| signal.is_cancelled())
            {
                return Err(AppError::MutationCancelled);
            }
            let lease = self.payloads.pin_verified(handle).await?;
            payloads.push(lease);
        }
        Ok(payloads)
    }
}

fn source_evidence_key(source: &str) -> Result<Option<RemoteEvidenceKey>, AppError> {
    let parsed = parse_source(source)?;
    if matches!(
        parsed.source_type,
        SourceType::Local | SourceType::WellKnown | SourceType::Download
    ) {
        return Ok(None);
    }
    let metadata = NormalizedUpdateMetadata {
        source: parsed.url.clone(),
        source_type: parsed.source_type.to_string(),
        source_url: Some(parsed.url),
        ref_name: parsed.git_ref,
        skill_path: None,
        remote_hash: None,
        computed_hash: None,
        well_known_digest: None,
    };
    let identity = SourceIdentity::from_metadata(&metadata)?;
    Ok(Some(RemoteEvidenceKey::from_identity(&identity)))
}

fn validate_handles(request: &InstallRequest) -> Result<(), AppError> {
    if request.payloads.len() != request.skills.len()
        || request.payloads.iter().any(|handle| {
            handle.session_id != request.discovery_session.session_id
                || handle.source_fingerprint != request.discovery_session.source_fingerprint
                || !same_environment_identity(&handle.environment, &request.context.environment)
        })
    {
        return Err(AppError::StalePayload);
    }
    Ok(())
}

pub fn validate_install_request(request: &InstallRequest) -> Result<(), AppError> {
    if request.source.trim().is_empty()
        || request.discovery_session.session_id.is_empty()
        || request.skills.is_empty()
        || !same_environment_identity(
            &request.context.environment,
            &request.discovery_session.environment,
        )
    {
        return Err(validation("invalid source, session, or Environment"));
    }
    let mut skills = BTreeSet::new();
    if request
        .skills
        .iter()
        .any(|skill| skill.trim().is_empty() || !skills.insert(skill))
    {
        return Err(validation("invalid or duplicate Skill selection"));
    }
    Ok(())
}

fn validation(message: &str) -> AppError {
    AppError::Validation {
        field: Some("request".to_string()),
        message: message.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    use tempfile::tempdir;

    use super::*;
    use crate::application::mutation::plan::MutationPlan;
    use crate::application::payload_session::{
        DiscoverySessionHandle, PayloadSessionLimits, PayloadSessionManager,
    };
    use crate::core::mutation::CancellationSignal;
    use crate::core::skill_payload::build_skill_payload;
    use crate::core::{NormalizedRef, SourceProvider};
    use crate::environment::runtime::ContextSnapshotRevision;
    use crate::environment::types::{EnvironmentRef, SkillLocation, SkillLocationRef};
    use crate::models::InstallMode;

    fn selection(mode: InstallMode) -> AgentSelectionSubmission {
        AgentSelectionSubmission {
            revision: crate::application::agent_selection::AgentSelectionRevision(
                "selection-v1-test".to_string(),
            ),
            selected_option_ids: Vec::new(),
            requested_mode: mode,
        }
    }

    #[test]
    fn request_requires_one_environment_and_unique_skills() {
        let request = InstallRequest {
            context: SkillLocationRef {
                environment: EnvironmentRef::Native,
                scope: SkillLocation::Global,
            },
            source: "owner/repo".to_string(),
            discovery_session: DiscoverySessionHandle {
                session_id: "session-1".to_string(),
                environment: EnvironmentRef::Native,
                source_fingerprint: "source-1".to_string(),
                expires_at_epoch_ms: 10_000,
            },
            payloads: Vec::new(),
            skills: vec!["demo".to_string(), "demo".to_string()],
            agent_selection: selection(InstallMode::Copy),
            acknowledge_redirect: true,
        };
        let serialized = serde_json::to_value(&request).expect("serialize install request");
        assert_eq!(serialized["acknowledgeRedirect"], true);
        assert!(serialized.get("acknowledgeRisk").is_none());
        assert!(validate_install_request(&request).is_err());

        let mut mismatched = request;
        mismatched.skills = vec!["demo".to_string()];
        mismatched.discovery_session.environment = EnvironmentRef::Wsl {
            distro_name: "Ubuntu".to_string(),
        };
        assert!(validate_install_request(&mismatched).is_err());
    }

    #[test]
    fn openclaw_source_uses_the_standard_install_policy() {
        let request = InstallRequest {
            context: SkillLocationRef {
                environment: EnvironmentRef::Native,
                scope: SkillLocation::Global,
            },
            source: "openclaw/community-skills".to_string(),
            discovery_session: DiscoverySessionHandle {
                session_id: "session-1".to_string(),
                environment: EnvironmentRef::Native,
                source_fingerprint: "source-1".to_string(),
                expires_at_epoch_ms: 10_000,
            },
            payloads: Vec::new(),
            skills: vec!["demo".to_string()],
            agent_selection: selection(InstallMode::Copy),
            acknowledge_redirect: false,
        };

        assert!(validate_install_request(&request).is_ok());
    }

    struct Planner {
        preview: InstallPreview,
        rebuilds: Arc<AtomicUsize>,
    }

    impl InstallPlanner for Planner {
        fn preview<'a>(
            &'a self,
            _operation: InstallOperation,
            _request: &'a InstallRequest,
            _payloads: Vec<PinnedPayloadLease>,
        ) -> InstallFuture<'a, Result<InstallPreviewOutcome, AppError>> {
            Box::pin(async move {
                Ok(InstallPreviewOutcome::Ready {
                    preview: self.preview.clone(),
                })
            })
        }

        fn rebuild<'a>(
            &'a self,
            operation: InstallOperation,
            _request: &'a InstallRequest,
            payloads: Vec<PinnedPayloadLease>,
        ) -> InstallFuture<'a, Result<(PreviewToken, MutationPlan), AppError>> {
            self.rebuilds.fetch_add(1, Ordering::SeqCst);
            Box::pin(async move {
                Ok((
                    self.preview.token.clone(),
                    MutationPlan {
                        kind: operation.mutation_kind(),
                        operation_id: "operation-1".to_string(),
                        payloads: payloads
                            .into_iter()
                            .map(|lease| (lease.manifest().payload_id().clone(), lease))
                            .collect(),
                        units: Vec::new(),
                    },
                ))
            })
        }
    }

    struct Executor(Arc<AtomicUsize>);

    impl MutationPlanExecutor for Executor {
        fn execute<'a>(
            &'a self,
            plan: MutationPlan,
            _cancellation: CancellationSignal,
        ) -> InstallFuture<'a, Vec<MutationUnitResult>> {
            assert_eq!(plan.payloads.len(), 1);
            self.0.fetch_add(1, Ordering::SeqCst);
            Box::pin(async {
                vec![MutationUnitResult {
                    unit_id: "demo".to_string(),
                    skill_name: "demo".to_string(),
                    source: None,
                    target: SkillLocationRef {
                        environment: EnvironmentRef::Native,
                        scope: SkillLocation::Global,
                    },
                    status: crate::application::mutation::result::MutationUnitStatus::Succeeded,
                    retryable: false,
                    lock_committed: true,
                    actual_mode: Some(InstallMode::Copy),
                    fallback_reason: None,
                    agent_targets: Vec::new(),
                    warnings: Vec::new(),
                    error: None,
                    recovery: None,
                }]
            })
        }
    }

    #[test]
    fn source_evidence_key_matches_remote_sources_and_ignores_local_sources() {
        let key = source_evidence_key("owner/repo#release")
            .unwrap()
            .expect("remote source");

        assert_eq!(key.remote.provider(), &SourceProvider::Github);
        assert_eq!(key.remote.authority(), "github.com");
        assert_eq!(key.remote.repository(), "owner/repo");
        assert_eq!(key.normalized_ref, NormalizedRef::Named("release".into()));
        assert!(source_evidence_key("/tmp/local-skill").unwrap().is_none());
        assert!(source_evidence_key("https://skills.example.com")
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn execute_pins_exact_preview_handle_and_rebuilds_without_reacquiring() {
        let temp = tempdir().unwrap();
        let source = temp.path().join("demo");
        fs::create_dir(&source).unwrap();
        fs::write(source.join("SKILL.md"), b"demo").unwrap();
        let payload = build_skill_payload(&source).unwrap();
        let manager = Arc::new(PayloadSessionManager::in_memory(
            PayloadSessionLimits {
                ttl_ms: 60_000,
                max_sessions: 4,
                max_bytes: 1_000_000,
            },
            || 1_000,
        ));
        let discovery = manager
            .discover(EnvironmentRef::Native, "source-1")
            .await
            .unwrap();
        let handle = manager
            .acquire_payload(&discovery, "demo", payload)
            .await
            .unwrap();
        let token = PreviewToken {
            generation: "preview-v1-demo".to_string(),
            registry_revision: "registry-1".to_string(),
            environment_revision: "environment-1".to_string(),
            context_revision: ContextSnapshotRevision::parse("context-v1-demo").unwrap(),
        };
        let request = InstallRequest {
            context: SkillLocationRef {
                environment: EnvironmentRef::Native,
                scope: SkillLocation::Global,
            },
            source: "owner/repo".to_string(),
            discovery_session: discovery,
            payloads: vec![handle.clone()],
            skills: vec!["demo".to_string()],
            agent_selection: selection(InstallMode::Copy),
            acknowledge_redirect: true,
        };
        let rebuilds = Arc::new(AtomicUsize::new(0));
        let executions = Arc::new(AtomicUsize::new(0));
        let suppression_attempts = Arc::new(AtomicUsize::new(0));
        let suppression_attempts_for_clearer = Arc::clone(&suppression_attempts);
        let service = InstallService::new(
            Arc::clone(&manager),
            Planner {
                preview: InstallPreview {
                    token: token.clone(),
                    skills: vec![InstallSkillPreview {
                        skill_name: "demo".to_string(),
                        payload: handle.clone(),
                        overwrite_targets: Vec::new(),
                        blocking_reasons: Vec::new(),
                        fallback_forecasts: Vec::new(),
                    }],
                },
                rebuilds: Arc::clone(&rebuilds),
            },
            Executor(Arc::clone(&executions)),
        )
        .with_source_suppression_clearer(Arc::new(move |_, _| {
            suppression_attempts_for_clearer.fetch_add(1, Ordering::SeqCst);
            Err(AppError::Io {
                message: "update-check state is read-only".to_string(),
            })
        }));

        let InstallPreviewOutcome::Ready { preview } = service
            .preview(InstallOperation::Install, &request)
            .await
            .unwrap()
        else {
            panic!("expected ready preview");
        };
        assert_eq!(preview.token, token);
        let response = service
            .execute(
                InstallOperation::Install,
                &request,
                token,
                CancellationSignal::default(),
            )
            .await
            .unwrap();

        assert_eq!(response.units.len(), 1);
        assert_eq!(
            response.warnings,
            vec![SourceSuppressionWarningCode::SuppressionCleanupFailed]
        );
        assert_eq!(
            response.units[0].status,
            crate::application::mutation::result::MutationUnitStatus::Succeeded,
        );
        assert_eq!(rebuilds.load(Ordering::SeqCst), 1);
        assert_eq!(executions.load(Ordering::SeqCst), 1);
        assert_eq!(suppression_attempts.load(Ordering::SeqCst), 1);
    }
}

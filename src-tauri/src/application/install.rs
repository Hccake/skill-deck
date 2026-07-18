use std::collections::BTreeSet;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::application::agent_intent::{
    validate_agent_intents, AgentTargetFallbackPreview, AgentWriteIntent,
};
use crate::application::mutation::plan::{MutationPlan, PreviewToken};
use crate::application::mutation::result::{MutationUnitResult, OperationErrorCode};
use crate::application::payload_session::{
    AcquiredPayloadHandle, DiscoverySessionHandle, PayloadSessionManager, PinnedPayloadLease,
};
use crate::core::mutation::CancellationSignal;
use crate::core::{ensure_install_risk_acknowledged, parse_source, source_risk_policy};
use crate::environment::types::{same_environment_identity, ContextRef};
use crate::error::AppError;
use crate::models::InstallMode;

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct InstallRequest {
    pub context: ContextRef,
    pub source: String,
    pub discovery_session: DiscoverySessionHandle,
    pub payloads: Vec<AcquiredPayloadHandle>,
    pub skills: Vec<String>,
    pub agent_intents: Vec<AgentWriteIntent>,
    pub requested_mode: InstallMode,
    pub acknowledge_risk: bool,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct InstallPreview {
    pub token: PreviewToken,
    pub skills: Vec<InstallSkillPreview>,
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
}

pub type InstallFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

pub trait InstallPlanner: Send + Sync {
    fn preview<'a>(
        &'a self,
        request: &'a InstallRequest,
        payloads: Vec<PinnedPayloadLease>,
    ) -> InstallFuture<'a, Result<InstallPreview, AppError>>;

    fn rebuild<'a>(
        &'a self,
        request: &'a InstallRequest,
        payloads: Vec<PinnedPayloadLease>,
    ) -> InstallFuture<'a, Result<(PreviewToken, MutationPlan), AppError>>;
}

pub trait InstallPlanExecutor: Send + Sync {
    fn execute<'a>(
        &'a self,
        plan: MutationPlan,
        cancellation: CancellationSignal,
    ) -> InstallFuture<'a, Vec<MutationUnitResult>>;
}

pub struct InstallService<P, E> {
    payloads: Arc<PayloadSessionManager>,
    planner: P,
    executor: E,
}

impl<P, E> InstallService<P, E>
where
    P: InstallPlanner,
    E: InstallPlanExecutor,
{
    pub fn new(payloads: Arc<PayloadSessionManager>, planner: P, executor: E) -> Self {
        Self {
            payloads,
            planner,
            executor,
        }
    }

    pub async fn preview(&self, request: &InstallRequest) -> Result<InstallPreview, AppError> {
        validate_install_request(request)?;
        let payloads = self.pin_request_payloads(request, None).await?;
        self.planner.preview(request, payloads).await
    }

    pub async fn execute(
        &self,
        request: &InstallRequest,
        expected_token: PreviewToken,
        cancellation: CancellationSignal,
    ) -> Result<InstallResponse, AppError> {
        validate_install_request(request)?;
        let payloads = self
            .pin_request_payloads(request, Some(cancellation.clone()))
            .await?;
        let (actual_token, plan) = self.planner.rebuild(request, payloads).await?;
        validate_preview_token(&expected_token, &actual_token)?;
        Ok(InstallResponse {
            units: self.executor.execute(plan, cancellation).await,
        })
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

fn validate_preview_token(expected: &PreviewToken, actual: &PreviewToken) -> Result<(), AppError> {
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
    let parsed = parse_source(&request.source)?;
    ensure_install_risk_acknowledged(&source_risk_policy(&parsed), request.acknowledge_risk)?;
    let mut skills = BTreeSet::new();
    if request
        .skills
        .iter()
        .any(|skill| skill.trim().is_empty() || !skills.insert(skill))
    {
        return Err(validation("invalid or duplicate Skill selection"));
    }
    validate_agent_intents(&request.agent_intents)
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
    use crate::environment::runtime::ContextSnapshotRevision;
    use crate::environment::types::{ContextRef, ContextScope, EnvironmentRef};
    use crate::models::InstallMode;

    #[test]
    fn request_requires_one_environment_and_unique_skills() {
        let request = InstallRequest {
            context: ContextRef {
                environment: EnvironmentRef::Host,
                scope: ContextScope::Global,
            },
            source: "owner/repo".to_string(),
            discovery_session: DiscoverySessionHandle {
                session_id: "session-1".to_string(),
                environment: EnvironmentRef::Host,
                source_fingerprint: "source-1".to_string(),
                expires_at_epoch_ms: 10_000,
            },
            payloads: Vec::new(),
            skills: vec!["demo".to_string(), "demo".to_string()],
            agent_intents: Vec::new(),
            requested_mode: InstallMode::Copy,
            acknowledge_risk: true,
        };
        assert!(validate_install_request(&request).is_err());

        let mut mismatched = request;
        mismatched.skills = vec!["demo".to_string()];
        mismatched.discovery_session.environment = EnvironmentRef::Wsl {
            distro_name: "Ubuntu".to_string(),
        };
        assert!(validate_install_request(&mismatched).is_err());
    }

    #[test]
    fn guarded_source_requires_explicit_risk_acknowledgement() {
        let request = InstallRequest {
            context: ContextRef {
                environment: EnvironmentRef::Host,
                scope: ContextScope::Global,
            },
            source: "openclaw/community-skills".to_string(),
            discovery_session: DiscoverySessionHandle {
                session_id: "session-1".to_string(),
                environment: EnvironmentRef::Host,
                source_fingerprint: "source-1".to_string(),
                expires_at_epoch_ms: 10_000,
            },
            payloads: Vec::new(),
            skills: vec!["demo".to_string()],
            agent_intents: Vec::new(),
            requested_mode: InstallMode::Copy,
            acknowledge_risk: false,
        };

        assert!(matches!(
            validate_install_request(&request),
            Err(AppError::InstallRiskConfirmationRequired { .. })
        ));
        let acknowledged = InstallRequest {
            acknowledge_risk: true,
            ..request
        };
        assert!(validate_install_request(&acknowledged).is_ok());
    }

    struct Planner {
        preview: InstallPreview,
        rebuilds: Arc<AtomicUsize>,
    }

    impl InstallPlanner for Planner {
        fn preview<'a>(
            &'a self,
            _request: &'a InstallRequest,
            _payloads: Vec<PinnedPayloadLease>,
        ) -> InstallFuture<'a, Result<InstallPreview, AppError>> {
            Box::pin(async move { Ok(self.preview.clone()) })
        }

        fn rebuild<'a>(
            &'a self,
            _request: &'a InstallRequest,
            payloads: Vec<PinnedPayloadLease>,
        ) -> InstallFuture<'a, Result<(PreviewToken, MutationPlan), AppError>> {
            self.rebuilds.fetch_add(1, Ordering::SeqCst);
            Box::pin(async move {
                Ok((
                    self.preview.token.clone(),
                    MutationPlan {
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

    impl InstallPlanExecutor for Executor {
        fn execute<'a>(
            &'a self,
            plan: MutationPlan,
            _cancellation: CancellationSignal,
        ) -> InstallFuture<'a, Vec<MutationUnitResult>> {
            assert_eq!(plan.payloads.len(), 1);
            self.0.fetch_add(1, Ordering::SeqCst);
            Box::pin(async { Vec::new() })
        }
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
            .discover(EnvironmentRef::Host, "source-1")
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
            context: ContextRef {
                environment: EnvironmentRef::Host,
                scope: ContextScope::Global,
            },
            source: "owner/repo".to_string(),
            discovery_session: discovery,
            payloads: vec![handle.clone()],
            skills: vec!["demo".to_string()],
            agent_intents: Vec::new(),
            requested_mode: InstallMode::Copy,
            acknowledge_risk: true,
        };
        let rebuilds = Arc::new(AtomicUsize::new(0));
        let executions = Arc::new(AtomicUsize::new(0));
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
        );

        assert_eq!(service.preview(&request).await.unwrap().token, token);
        let response = service
            .execute(&request, token, CancellationSignal::default())
            .await
            .unwrap();

        assert!(response.units.is_empty());
        assert_eq!(rebuilds.load(Ordering::SeqCst), 1);
        assert_eq!(executions.load(Ordering::SeqCst), 1);
    }
}

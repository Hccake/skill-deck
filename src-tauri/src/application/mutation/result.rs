use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::core::agent_definition::AgentId;
use crate::environment::types::{EnvironmentRef, ResourceLocator, SkillLocationRef};
use crate::error::AppError;
use crate::models::InstallMode;

pub use crate::error::RecoveryResourceId;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub enum MutationUnitStatus {
    Succeeded,
    Failed,
    Skipped,
    Cancelled,
    NotRun,
    RecoveryRequired,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub enum OperationErrorCode {
    Validation,
    EnvironmentUnavailable,
    EnvironmentChanged,
    ContextChanged,
    StorageUnsupported,
    CapabilityUnavailable,
    UnsafePath,
    UnsafeSourceLink,
    SelfCopy,
    PayloadSessionExpired,
    StaleContext,
    StaleRegistry,
    StaleEnvironment,
    StalePayload,
    StaleTarget,
    ExternalLockChanged,
    MutationCancelled,
    ExecutionFailed,
    RestoreFailed,
    RecoveryRequired,
    ConfigurationReadOnly,
    ConfigurationCorrupted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub enum FallbackReasonCode {
    SymlinkUnavailable,
    CrossStorageCopyRequired,
    TargetCapabilityFallback,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub enum MutationWarningCode {
    DefaultTargetCleanupFailed,
    BackupCleanupFailed,
    RemoteHashRefreshFailed,
    CleanupMarkerRetained,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub enum SuggestedActionCode {
    ReviewChanges,
    Refresh,
    OpenRecoveryResource,
    SaveDefaultsLater,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub enum ErrorSeverity {
    Info,
    Warning,
    Error,
    Critical,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct ErrorReport {
    pub code: OperationErrorCode,
    pub parameters: BTreeMap<String, String>,
    pub field: Option<String>,
    pub severity: ErrorSeverity,
    pub retryable: bool,
    pub technical_details: Option<String>,
    pub environment: Option<EnvironmentRef>,
    pub context: Option<SkillLocationRef>,
    pub unit_id: Option<String>,
    pub recovery_resource_id: Option<RecoveryResourceId>,
    pub display_paths: Vec<ResourceLocator>,
}

impl ErrorReport {
    pub fn new(code: OperationErrorCode) -> Self {
        Self {
            code,
            parameters: BTreeMap::new(),
            field: None,
            severity: ErrorSeverity::Error,
            retryable: false,
            technical_details: None,
            environment: None,
            context: None,
            unit_id: None,
            recovery_resource_id: None,
            display_paths: Vec::new(),
        }
    }

    pub fn recovery_required(
        resource_id: RecoveryResourceId,
        technical_details: impl Into<String>,
    ) -> Self {
        Self {
            code: OperationErrorCode::RecoveryRequired,
            severity: ErrorSeverity::Critical,
            recovery_resource_id: Some(resource_id),
            technical_details: Some(technical_details.into()),
            ..Self::new(OperationErrorCode::RecoveryRequired)
        }
    }

    pub fn from_app_error(error: AppError, context: Option<SkillLocationRef>) -> Self {
        let mut report = match error {
            AppError::Validation { field, message } => {
                let mut report = Self::with_details(OperationErrorCode::Validation, false, message);
                report.field = field;
                report
            }
            AppError::AgentSelectionInvalid { reason } => {
                let mut report = Self::new(OperationErrorCode::Validation);
                report.field = Some("agentSelection".to_string());
                report
                    .parameters
                    .insert("reason".to_string(), reason.code().to_string());
                report
            }
            AppError::InvalidProxySettings { code } => {
                let mut report = Self::new(OperationErrorCode::Validation);
                report.field = Some("networkProxy".to_string());
                report.parameters.insert("reason".to_string(), code);
                report
            }
            AppError::InvalidSkillMd { message }
            | AppError::InvalidSource { value: message }
            | AppError::InvalidAgent { agent: message } => {
                Self::with_details(OperationErrorCode::Validation, false, message)
            }
            AppError::DirectDownloadRedirectConfirmationRequired { host } => {
                let mut report = Self::new(OperationErrorCode::Validation);
                report.field = Some("acknowledgeRedirect".to_string());
                report.parameters.insert("host".to_string(), host);
                report
            }
            AppError::SourceAcquisitionFailed {
                well_known_reason,
                download_reason,
            } => {
                let mut report = Self::new(OperationErrorCode::ExecutionFailed);
                report.parameters.insert(
                    "wellKnownReason".to_string(),
                    well_known_reason.code().to_string(),
                );
                report.parameters.insert(
                    "downloadReason".to_string(),
                    download_reason.code().to_string(),
                );
                report
            }
            AppError::WellKnownSourceFailed { reason } => {
                let mut report = Self::new(OperationErrorCode::ExecutionFailed);
                report
                    .parameters
                    .insert("reason".to_string(), reason.code().to_string());
                report
            }
            AppError::DirectDownloadFailed { reason } => {
                let mut report = Self::new(OperationErrorCode::ExecutionFailed);
                report
                    .parameters
                    .insert("reason".to_string(), reason.code().to_string());
                report
            }
            AppError::DirectDownloadUnsupportedOperation => {
                let mut report = Self::new(OperationErrorCode::Validation);
                report.parameters.insert(
                    "reason".to_string(),
                    "direct-download-unsupported-operation".to_string(),
                );
                report
            }
            AppError::DirectDownloadConflict { target } => {
                let mut report = Self::new(OperationErrorCode::Validation);
                report
                    .parameters
                    .insert("reason".to_string(), "direct-download-conflict".to_string());
                report.parameters.insert("target".to_string(), target);
                report
            }
            AppError::NoSkillsFound => Self::new(OperationErrorCode::Validation),
            AppError::EnvironmentUnavailable {
                environment,
                message,
            } => {
                let mut report =
                    Self::with_details(OperationErrorCode::EnvironmentUnavailable, true, message);
                report.environment = Some(environment);
                report
            }
            AppError::EnvironmentDiscoveryFailed { message }
            | AppError::WslCommandFailed {
                stderr: message, ..
            } => Self::with_details(OperationErrorCode::EnvironmentUnavailable, true, message),
            AppError::WslCommandTimedOut | AppError::WslOutputLimitExceeded { .. } => {
                Self::new(OperationErrorCode::EnvironmentUnavailable).with_retryable(true)
            }
            AppError::EnvironmentChanged { .. } => {
                Self::new(OperationErrorCode::EnvironmentChanged).with_retryable(true)
            }
            AppError::ContextChanged { .. } => {
                Self::new(OperationErrorCode::ContextChanged).with_retryable(true)
            }
            AppError::StorageUnsupported { path }
            | AppError::StorageMappingUnsupported { path, .. } => {
                let mut report = Self::new(OperationErrorCode::StorageUnsupported);
                report.parameters.insert("path".to_string(), path);
                report
            }
            AppError::CapabilityUnavailable { capability, path } => {
                let mut report = Self::new(OperationErrorCode::CapabilityUnavailable);
                report
                    .parameters
                    .insert("capability".to_string(), capability);
                if let Some(path) = path {
                    report.parameters.insert("path".to_string(), path);
                }
                report
            }
            AppError::UnsafePath { path, reason } => {
                let mut report = Self::with_details(OperationErrorCode::UnsafePath, false, reason);
                report.parameters.insert("path".to_string(), path);
                report
            }
            AppError::UnsafeSourceLink { path } => {
                let mut report = Self::new(OperationErrorCode::UnsafeSourceLink);
                report.parameters.insert("path".to_string(), path);
                report
            }
            AppError::SelfCopy => Self::new(OperationErrorCode::SelfCopy),
            AppError::PayloadSessionExpired { session_id } => {
                let mut report =
                    Self::new(OperationErrorCode::PayloadSessionExpired).with_retryable(true);
                report
                    .parameters
                    .insert("sessionId".to_string(), session_id);
                report
            }
            AppError::PayloadStorageRequiresCleanup { environment } => {
                let mut report =
                    Self::new(OperationErrorCode::CapabilityUnavailable).with_retryable(true);
                report.parameters.insert(
                    "capability".to_string(),
                    "payloadStorageCleanup".to_string(),
                );
                report.environment = Some(environment);
                report
            }
            AppError::StaleContext => {
                Self::new(OperationErrorCode::StaleContext).with_retryable(true)
            }
            AppError::StaleRegistry => {
                Self::new(OperationErrorCode::StaleRegistry).with_retryable(true)
            }
            AppError::StaleEnvironment => {
                Self::new(OperationErrorCode::StaleEnvironment).with_retryable(true)
            }
            AppError::StalePayload => {
                Self::new(OperationErrorCode::StalePayload).with_retryable(true)
            }
            AppError::StaleTarget => {
                Self::new(OperationErrorCode::StaleTarget).with_retryable(true)
            }
            AppError::StaleAgentRuntime {
                expected_registry_revision,
                actual_registry_revision,
                expected_environment_revision,
                actual_environment_revision,
            } => {
                let code = if expected_registry_revision != actual_registry_revision {
                    OperationErrorCode::StaleRegistry
                } else if expected_environment_revision != actual_environment_revision {
                    OperationErrorCode::StaleEnvironment
                } else {
                    OperationErrorCode::StaleTarget
                };
                Self::new(code).with_retryable(true)
            }
            AppError::ExternalLockChanged { target } | AppError::LockConflict { target } => {
                let mut report = Self::new(OperationErrorCode::ExternalLockChanged);
                report
                    .parameters
                    .insert("target".to_string(), target.to_string());
                report
            }
            AppError::MutationCancelled => {
                Self::new(OperationErrorCode::MutationCancelled).with_retryable(true)
            }
            AppError::RecoveryRequired {
                recovery_resource_id,
                message,
            } => Self::recovery_required(recovery_resource_id, redact_public_text(message)),
            AppError::RestoreFailed { message } => {
                Self::with_details(OperationErrorCode::RestoreFailed, false, message)
            }
            AppError::ConfigurationReadOnly => Self::new(OperationErrorCode::ConfigurationReadOnly),
            AppError::ConfigurationCorrupted { message }
            | AppError::Yaml { message }
            | AppError::Json { message } => {
                Self::with_details(OperationErrorCode::ConfigurationCorrupted, false, message)
            }
            AppError::GitTimeout { .. } | AppError::GitNetworkError { .. } => {
                Self::new(OperationErrorCode::ExecutionFailed).with_retryable(true)
            }
            AppError::DiscoveryRequestFailed { reason } => {
                let mut report =
                    Self::new(OperationErrorCode::ExecutionFailed).with_retryable(true);
                report.parameters.insert("reason".to_string(), reason);
                report
            }
            AppError::Io { message }
            | AppError::Path { message }
            | AppError::GitCloneFailed { message }
            | AppError::GitAuthFailed { message }
            | AppError::GitHubApiError { message, .. }
            | AppError::ProjectMigrationFailed { message }
            | AppError::ExecutionFailed { message }
            | AppError::Custom { message } => {
                Self::with_details(OperationErrorCode::ExecutionFailed, false, message)
            }
            AppError::GitRepoNotFound { repo } => {
                Self::with_details(OperationErrorCode::ExecutionFailed, false, repo)
            }
            AppError::GitRefNotFound { ref_name } => {
                Self::with_details(OperationErrorCode::ExecutionFailed, false, ref_name)
            }
            AppError::PathNotFound { path } => {
                Self::with_details(OperationErrorCode::ExecutionFailed, false, path)
            }
            AppError::MutationBusy
            | AppError::InstallWizardActive
            | AppError::InstallWizardSessionUnavailable
            | AppError::ApplicationTerminating
            | AppError::WslIntegrationBusy { .. } => {
                Self::new(OperationErrorCode::ExecutionFailed).with_retryable(true)
            }
        };

        if let Some(context) = context {
            report.environment = Some(context.environment.clone());
            report.context = Some(context);
        }
        report
    }

    fn with_details(
        code: OperationErrorCode,
        retryable: bool,
        technical_details: impl Into<String>,
    ) -> Self {
        let mut report = Self::new(code).with_retryable(retryable);
        report.technical_details = Some(redact_public_text(technical_details.into()));
        report
    }

    fn with_retryable(mut self, retryable: bool) -> Self {
        self.retryable = retryable;
        self
    }
}

fn redact_public_text(value: String) -> String {
    let lowercase = value.to_ascii_lowercase();
    if [
        "authorization:",
        "bearer ",
        "ssh_secret",
        "private_key",
        "password=",
        "token=",
    ]
    .iter()
    .any(|marker| lowercase.contains(marker))
    {
        return "[redacted]".to_string();
    }

    value.chars().take(4096).collect()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct MutationWarning {
    pub code: MutationWarningCode,
    pub parameters: BTreeMap<String, String>,
    pub technical_details: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct RecoveryAction {
    pub resource_id: RecoveryResourceId,
    pub suggested_action_code: SuggestedActionCode,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct AgentTargetMutationResult {
    pub target_id: String,
    pub agent_id: AgentId,
    pub status: MutationUnitStatus,
    pub actual_mode: Option<InstallMode>,
    pub fallback_reason: Option<FallbackReasonCode>,
    pub error: Option<ErrorReport>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct MutationUnitResult {
    pub unit_id: String,
    pub skill_name: String,
    pub source: Option<SkillLocationRef>,
    pub target: SkillLocationRef,
    pub status: MutationUnitStatus,
    pub retryable: bool,
    pub lock_committed: bool,
    pub actual_mode: Option<InstallMode>,
    pub fallback_reason: Option<FallbackReasonCode>,
    pub agent_targets: Vec<AgentTargetMutationResult>,
    pub warnings: Vec<MutationWarning>,
    pub error: Option<ErrorReport>,
    pub recovery: Option<RecoveryAction>,
}

impl MutationUnitResult {
    pub fn recovery_required(
        unit_id: impl Into<String>,
        skill_name: impl Into<String>,
        target: SkillLocationRef,
        mut error: ErrorReport,
    ) -> Self {
        let resource_id = error
            .recovery_resource_id
            .clone()
            .expect("RecoveryRequired report must contain a recovery resource ID");
        let unit_id = unit_id.into();
        error.unit_id = Some(unit_id.clone());
        error.context = Some(target.clone());
        error.environment = Some(target.environment.clone());

        Self {
            unit_id,
            skill_name: skill_name.into(),
            source: None,
            target,
            status: MutationUnitStatus::RecoveryRequired,
            retryable: false,
            lock_committed: false,
            actual_mode: None,
            fallback_reason: None,
            agent_targets: Vec::new(),
            warnings: Vec::new(),
            error: Some(error),
            recovery: Some(RecoveryAction {
                resource_id,
                suggested_action_code: SuggestedActionCode::OpenRecoveryResource,
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::environment::types::{EnvironmentRef, SkillLocation, SkillLocationRef};
    use crate::error::AppError;

    fn context() -> SkillLocationRef {
        SkillLocationRef {
            environment: EnvironmentRef::Native,
            scope: SkillLocation::Global,
        }
    }

    #[test]
    fn recovery_required_is_not_retryable_by_default() {
        let resource_id = RecoveryResourceId::parse("recovery-1").expect("valid recovery ID");
        let report = ErrorReport::recovery_required(resource_id.clone(), "restore failed");
        let result = MutationUnitResult::recovery_required("unit-1", "demo", context(), report);

        assert_eq!(result.status, MutationUnitStatus::RecoveryRequired);
        assert!(!result.retryable);
        assert!(!result.lock_committed);
        assert_eq!(
            result.recovery.expect("recovery action").resource_id,
            resource_id
        );
    }

    #[test]
    fn stable_codes_serialize_as_lower_camel_values_without_summary() {
        let values = [
            serde_json::to_value(MutationUnitStatus::NotRun).expect("status"),
            serde_json::to_value(OperationErrorCode::PayloadSessionExpired).expect("error code"),
            serde_json::to_value(FallbackReasonCode::CrossStorageCopyRequired)
                .expect("fallback code"),
            serde_json::to_value(MutationWarningCode::BackupCleanupFailed).expect("warning code"),
            serde_json::to_value(SuggestedActionCode::OpenRecoveryResource).expect("action code"),
        ];

        assert_eq!(
            values,
            [
                serde_json::json!("notRun"),
                serde_json::json!("payloadSessionExpired"),
                serde_json::json!("crossStorageCopyRequired"),
                serde_json::json!("backupCleanupFailed"),
                serde_json::json!("openRecoveryResource"),
            ]
        );

        let report = ErrorReport::new(OperationErrorCode::Validation);
        let serialized = serde_json::to_value(report).expect("serialize report");
        assert!(serialized.get("summary").is_none());
    }

    #[test]
    fn recovery_resource_id_is_opaque_and_validated() {
        assert!(RecoveryResourceId::parse("").is_err());
        assert!(RecoveryResourceId::parse("../backup").is_err());
        assert!(RecoveryResourceId::parse("contains spaces").is_err());

        let id = RecoveryResourceId::parse("recovery-01J123ABC_xyz").expect("valid ID");
        let json = serde_json::to_string(&id).expect("serialize ID");
        let decoded: RecoveryResourceId = serde_json::from_str(&json).expect("deserialize ID");
        assert_eq!(decoded, id);
    }

    #[test]
    fn app_errors_map_to_stable_codes_and_context() {
        let context = context();
        let resource_id = RecoveryResourceId::parse("recovery-2").expect("valid recovery ID");
        let cases = [
            (
                AppError::Validation {
                    field: Some("agentId".to_string()),
                    message: "invalid".to_string(),
                },
                OperationErrorCode::Validation,
            ),
            (
                AppError::PayloadSessionExpired {
                    session_id: "session-1".to_string(),
                },
                OperationErrorCode::PayloadSessionExpired,
            ),
            (AppError::SelfCopy, OperationErrorCode::SelfCopy),
            (
                AppError::ExternalLockChanged {
                    target: crate::error::LockConflictTarget::Skill {
                        skill_name: "demo".to_string(),
                    },
                },
                OperationErrorCode::ExternalLockChanged,
            ),
            (
                AppError::RecoveryRequired {
                    recovery_resource_id: resource_id,
                    message: "restore failed".to_string(),
                },
                OperationErrorCode::RecoveryRequired,
            ),
        ];

        for (error, expected_code) in cases {
            let report = ErrorReport::from_app_error(error, Some(context.clone()));
            assert_eq!(report.code, expected_code);
            assert_eq!(report.context.as_ref(), Some(&context));
            assert_eq!(report.environment.as_ref(), Some(&context.environment));
        }
    }

    #[test]
    fn public_error_report_redacts_sensitive_technical_details() {
        let report = ErrorReport::from_app_error(
            AppError::ExecutionFailed {
                message: "Authorization: Bearer super-secret".to_string(),
            },
            None,
        );
        let serialized = serde_json::to_string(&report).expect("serialize report");

        assert!(!serialized.contains("super-secret"));
        assert!(serialized.contains("[redacted]"));
    }
}

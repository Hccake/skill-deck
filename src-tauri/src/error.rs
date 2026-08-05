// src-tauri/src/error.rs
use std::fmt;

use serde::{Deserialize, Serialize};
use specta::Type;
use thiserror::Error;

use crate::environment::types::EnvironmentRef;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub enum WslIntegrationBusyReason {
    Mutation,
    Lifecycle,
    InstallWizard,
    WslOperation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub enum AgentSelectionInvalidReason {
    DuplicateOption,
    OptionUnavailable,
    PlacementConflict,
    OptionMissing,
    ResultNotAllowed,
}

impl AgentSelectionInvalidReason {
    pub fn code(self) -> &'static str {
        match self {
            Self::DuplicateOption => "duplicateOption",
            Self::OptionUnavailable => "optionUnavailable",
            Self::PlacementConflict => "placementConflict",
            Self::OptionMissing => "optionMissing",
            Self::ResultNotAllowed => "resultNotAllowed",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(transparent)]
pub struct RecoveryResourceId(String);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoveryResourceIdError;

impl RecoveryResourceId {
    pub fn parse(value: impl Into<String>) -> Result<Self, RecoveryResourceIdError> {
        let value = value.into();
        let valid = !value.is_empty()
            && value.len() <= 128
            && value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_');
        valid.then_some(Self(value)).ok_or(RecoveryResourceIdError)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for RecoveryResourceIdError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("invalid recovery resource ID")
    }
}

impl std::error::Error for RecoveryResourceIdError {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
#[specta(tag = "kind", rename_all = "camelCase")]
pub enum LockConflictTarget {
    Skill {
        #[serde(rename = "skillName")]
        skill_name: String,
    },
    RootField {
        field: String,
    },
}

impl std::fmt::Display for LockConflictTarget {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Skill { skill_name } => {
                write!(
                    formatter,
                    "Skill lock entry changed externally: {skill_name}"
                )
            }
            Self::RootField { field } => {
                write!(
                    formatter,
                    "Skill lock root field changed externally: {field}"
                )
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error, Serialize, Type)]
#[serde(tag = "kind", content = "data", rename_all = "camelCase")]
#[specta(tag = "kind", content = "data", rename_all = "camelCase")]
pub enum AppError {
    #[error("IO error: {message}")]
    Io { message: String },

    #[error("YAML error: {message}")]
    Yaml { message: String },

    #[error("JSON error: {message}")]
    Json { message: String },

    #[error("Invalid SKILL.md: {message}")]
    InvalidSkillMd { message: String },

    #[error("Path error: {message}")]
    Path { message: String },

    #[error("Invalid source: {value}")]
    InvalidSource { value: String },

    #[error("Git clone failed: {message}")]
    GitCloneFailed { message: String },

    #[error("Git authentication failed: {message}")]
    GitAuthFailed { message: String },

    #[error("Git repository not found: {repo}")]
    GitRepoNotFound { repo: String },

    #[error("Git ref not found: {ref_name}")]
    GitRefNotFound {
        #[serde(rename = "refName")]
        ref_name: String,
    },

    #[error("Git operation timed out after {timeout_secs} seconds")]
    GitTimeout {
        #[serde(rename = "timeoutSecs")]
        timeout_secs: u32,
    },

    #[error("Git network error: {message}")]
    GitNetworkError { message: String },

    /// GitHub API 调用失败,带机器可读的 reason 让前端可以区分文案。
    /// reason 当前取值: `rate-limited` / `network-error` / `auth` / `http-<code>`。
    #[expect(
        dead_code,
        reason = "retained for backward-compatible IPC error decoding"
    )]
    #[error("GitHub API error ({reason}): {message}")]
    GitHubApiError { reason: String, message: String },

    #[error("Path not found: {path}")]
    PathNotFound { path: String },

    #[error("Installation requires explicit risk confirmation: {code}")]
    InstallRiskConfirmationRequired { code: String },

    #[error("No skills found")]
    NoSkillsFound,

    #[error("Another Skill operation is already running")]
    MutationBusy,

    #[error("The install wizard is active")]
    InstallWizardActive,

    #[error("The install wizard session is unavailable")]
    InstallWizardSessionUnavailable,

    #[error("The application is terminating")]
    ApplicationTerminating,

    #[error("WSL integration cannot change while {reason:?} is active")]
    WslIntegrationBusy { reason: WslIntegrationBusyReason },

    #[error("Skill operation was cancelled")]
    MutationCancelled,

    #[error("WSL environment discovery failed: {message}")]
    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    EnvironmentDiscoveryFailed { message: String },

    #[error("WSL command timed out")]
    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    WslCommandTimedOut,

    #[error("WSL {stream} exceeded the {limit}-byte limit")]
    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    WslOutputLimitExceeded { stream: String, limit: u32 },

    #[error("WSL command failed with exit code {exit_code:?}: {stderr}")]
    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    WslCommandFailed {
        #[serde(rename = "exitCode")]
        exit_code: Option<i32>,
        stderr: String,
    },

    #[error("Environment is unavailable: {message}")]
    EnvironmentUnavailable {
        environment: EnvironmentRef,
        message: String,
    },

    #[error("Path cannot be mapped into the target environment: {path}")]
    StorageMappingUnsupported {
        path: String,
        environment: EnvironmentRef,
    },

    #[error("Host project migration failed: {message}")]
    ProjectMigrationFailed { message: String },

    #[error("{target}")]
    LockConflict { target: LockConflictTarget },

    #[error("Invalid agent: {agent}")]
    InvalidAgent { agent: String },

    #[error("Invalid Agent selection: {reason:?}")]
    AgentSelectionInvalid { reason: AgentSelectionInvalidReason },

    #[error("Configuration is read-only because it uses an unsupported schema")]
    ConfigurationReadOnly,

    #[error("Validation failed: {message}")]
    Validation {
        field: Option<String>,
        message: String,
    },

    #[error("Environment changed ({expected_revision} -> {actual_revision})")]
    #[allow(dead_code)]
    EnvironmentChanged {
        expected_revision: String,
        actual_revision: String,
    },

    #[error("Context changed ({expected_revision} -> {actual_revision})")]
    #[allow(dead_code)]
    ContextChanged {
        expected_revision: String,
        actual_revision: String,
    },

    #[error("Storage is unsupported: {path}")]
    StorageUnsupported { path: String },

    #[error("Required capability is unavailable: {capability}")]
    CapabilityUnavailable {
        capability: String,
        path: Option<String>,
    },

    #[error("Unsafe path {path}: {reason}")]
    UnsafePath { path: String, reason: String },

    #[error("Source link escapes or cannot be resolved safely: {path}")]
    UnsafeSourceLink { path: String },

    #[error("Source and target resolve to the same physical project")]
    SelfCopy,

    #[error("Payload session expired: {session_id}")]
    PayloadSessionExpired { session_id: String },

    #[error("Payload storage requires cleanup before new acquisition")]
    PayloadStorageRequiresCleanup { environment: EnvironmentRef },

    #[error("Context preview is stale")]
    StaleContext,

    #[error("Agent Registry preview is stale")]
    StaleRegistry,

    #[error("Environment preview is stale")]
    StaleEnvironment,

    #[error("Payload preview is stale")]
    StalePayload,

    #[error("Target preview is stale")]
    StaleTarget,

    #[error("Skill lock changed externally: {target}")]
    #[allow(dead_code)]
    ExternalLockChanged { target: LockConflictTarget },

    #[error("Execution failed: {message}")]
    ExecutionFailed { message: String },

    #[error("Restore failed: {message}")]
    RestoreFailed { message: String },

    #[error("Recovery is required: {message}")]
    RecoveryRequired {
        recovery_resource_id: RecoveryResourceId,
        message: String,
    },

    #[error("Configuration is corrupted: {message}")]
    ConfigurationCorrupted { message: String },

    #[error(
        "Agent runtime changed before mutation (registry {expected_registry_revision} -> {actual_registry_revision}, environment {expected_environment_revision} -> {actual_environment_revision})"
    )]
    StaleAgentRuntime {
        expected_registry_revision: String,
        actual_registry_revision: String,
        expected_environment_revision: String,
        actual_environment_revision: String,
    },

    #[error("{message}")]
    Custom { message: String },
}

// From 实现
impl From<std::io::Error> for AppError {
    fn from(e: std::io::Error) -> Self {
        Self::Io {
            message: e.to_string(),
        }
    }
}

impl From<serde_yaml::Error> for AppError {
    fn from(e: serde_yaml::Error) -> Self {
        Self::Yaml {
            message: e.to_string(),
        }
    }
}

impl From<serde_json::Error> for AppError {
    fn from(e: serde_json::Error) -> Self {
        Self::Json {
            message: e.to_string(),
        }
    }
}

impl From<String> for AppError {
    fn from(message: String) -> Self {
        Self::Custom { message }
    }
}

impl From<&str> for AppError {
    fn from(message: &str) -> Self {
        Self::Custom {
            message: message.to_string(),
        }
    }
}

impl From<reqwest::Error> for AppError {
    fn from(e: reqwest::Error) -> Self {
        Self::GitNetworkError {
            message: e.to_string(),
        }
    }
}

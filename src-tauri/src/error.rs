// src-tauri/src/error.rs
use serde::Serialize;
use specta::Type;
use thiserror::Error;

use crate::environment::types::EnvironmentRef;

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
    #[error("GitHub API error ({reason}): {message}")]
    GitHubApiError { reason: String, message: String },

    #[error("Path not found: {path}")]
    PathNotFound { path: String },

    #[error("Install failed: {message}")]
    InstallFailed { message: String },

    #[error("Installation requires explicit risk confirmation: {code}")]
    InstallRiskConfirmationRequired { code: String },

    #[error("No skills found")]
    NoSkillsFound,

    #[error("Another Skill operation is already running")]
    MutationBusy,

    #[error("The application is terminating")]
    ApplicationTerminating,

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

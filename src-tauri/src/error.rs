// src-tauri/src/error.rs
use serde::Serialize;
use specta::Type;
use thiserror::Error;

#[derive(Debug, Error, Serialize, Type)]
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

    #[error("Git operation timed out")]
    GitTimeout,

    #[error("Git network error: {message}")]
    GitNetworkError { message: String },

    #[error("Path not found: {path}")]
    PathNotFound { path: String },

    #[error("Install failed: {message}")]
    InstallFailed { message: String },

    #[error("No skills found")]
    NoSkillsFound,

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

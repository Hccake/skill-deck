// src-tauri/src/error.rs
use serde::Serialize;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("YAML parse error: {0}")]
    Yaml(#[from] serde_yaml::Error),

    #[error("JSON parse error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Invalid SKILL.md: {0}")]
    InvalidSkillMd(String),

    #[error("Path error: {0}")]
    Path(String),

    #[error("Invalid source: {0}")]
    InvalidSource(String),

    // Git 相关错误
    #[error("Git clone failed: {0}")]
    GitCloneFailed(String),

    #[error("Git authentication failed: {0}")]
    GitAuthFailed(String),

    #[error("Git repository not found: {0}")]
    GitRepoNotFound(String),

    #[error("Git ref not found: {0}")]
    GitRefNotFound(String),

    #[error("Git operation timed out")]
    GitTimeout,

    #[error("Git network error: {0}")]
    GitNetworkError(String),

    #[error("Path not found: {0}")]
    PathNotFound(String),

    #[error("Install failed: {0}")]
    InstallFailed(String),

    #[error("No skills found")]
    NoSkillsFound,

    #[error("Invalid agent: {0}")]
    InvalidAgent(String),
}

// Tauri Command 要求 Result 的 Err 类型实现 Serialize
impl Serialize for AppError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

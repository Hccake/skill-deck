use crate::environment::wsl::WslWorkspace;
use crate::error::AppError;

const DIRECTORY_COUNT_DEADLINE_MILLIS: u64 = 20_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectoryCountFact {
    pub path: String,
    pub observed_count: Option<u32>,
    pub truncated: bool,
}

impl WslWorkspace {
    pub(crate) async fn count_directory_entries(
        &self,
        paths: Vec<String>,
        limit: u32,
    ) -> Result<Vec<DirectoryCountFact>, AppError> {
        if paths.is_empty() || limit == 0 || paths.iter().any(|path| !path.starts_with('/')) {
            return Err(AppError::Validation {
                field: Some("directoryCount".to_string()),
                message: "WSL directory count requires absolute paths and a positive limit"
                    .to_string(),
            });
        }
        let response: environment_protocol::DirectoryCountResponse = self
            .request_worker_payload(environment_protocol::Message::CountDirectoryEntries {
                request: environment_protocol::DirectoryCountRequest {
                    paths: paths.clone(),
                    limit,
                    deadline_millis: DIRECTORY_COUNT_DEADLINE_MILLIS,
                },
            })
            .await?;
        if response.facts.len() != paths.len()
            || response
                .facts
                .iter()
                .zip(&paths)
                .any(|(fact, path)| fact.path != *path)
        {
            return Err(AppError::ConfigurationCorrupted {
                message: "invalid WSL Worker directory count response".to_string(),
            });
        }
        Ok(response
            .facts
            .into_iter()
            .map(|fact| DirectoryCountFact {
                path: fact.path,
                observed_count: fact.observed_count,
                truncated: fact.truncated,
            })
            .collect())
    }

    pub(crate) async fn list_child_directories(
        &self,
        path: String,
        limit: u32,
    ) -> Result<Vec<String>, AppError> {
        let response: environment_protocol::DirectoryListResponse = self
            .request_worker_payload(environment_protocol::Message::ListChildDirectories {
                request: environment_protocol::DirectoryListRequest {
                    path,
                    limit,
                    deadline_millis: DIRECTORY_COUNT_DEADLINE_MILLIS,
                },
            })
            .await?;
        if response.truncated {
            return Err(AppError::ExecutionFailed {
                message: "WSL child directory list exceeds its boundary".to_string(),
            });
        }
        response
            .names
            .into_iter()
            .map(|name| {
                let name =
                    String::from_utf8(name).map_err(|_| AppError::ConfigurationCorrupted {
                        message: "WSL child directory name is not UTF-8".to_string(),
                    })?;
                if name.is_empty() || matches!(name.as_str(), "." | "..") || name.contains('/') {
                    return Err(AppError::ConfigurationCorrupted {
                        message: "WSL Worker returned an unsafe directory name".to_string(),
                    });
                }
                Ok(name)
            })
            .collect()
    }
}

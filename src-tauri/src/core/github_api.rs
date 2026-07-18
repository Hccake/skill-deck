//! GitHub API 模块
//!
//! 功能：
//! - 获取 GitHub token
//! - 调用 GitHub Trees API 获取完整、可验证的 source evidence

use reqwest::header::{ETAG, IF_NONE_MATCH, RETRY_AFTER};
use reqwest::Client;
use serde::Deserialize;

/// GitHub Trees API 响应
#[derive(Debug, Deserialize)]
struct TreesResponse {
    sha: String,
    #[serde(default)]
    truncated: bool,
    tree: Vec<TreeEntry>,
}

#[derive(Debug, Deserialize)]
struct CommitResponse {
    sha: String,
}

#[derive(Debug, Deserialize)]
struct TreeEntry {
    path: String,
    #[serde(rename = "type")]
    entry_type: String,
    sha: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GithubTreeFailure {
    AuthenticationRequired,
    NotFoundOrUnauthorized,
    Network,
    SourceUnavailable,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GithubTreeSnapshot {
    pub ref_revision: String,
    pub root_tree_revision: String,
    pub validation: Option<String>,
    pub entries: Vec<GithubTreeSnapshotEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GithubTreeSnapshotEntry {
    pub path: String,
    pub entry_type: String,
    pub revision: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GithubTreeFetchOutcome {
    Modified(GithubTreeSnapshot),
    NotModified { ref_revision: String },
    RateLimited { retry_at_epoch_ms: Option<u64> },
    Incomplete,
    Failed(GithubTreeFailure),
}

#[derive(Clone)]
pub struct GithubApiClient {
    client: Client,
    api_base: String,
    token: Option<String>,
}

impl Default for GithubApiClient {
    fn default() -> Self {
        Self::new()
    }
}

impl GithubApiClient {
    pub fn new() -> Self {
        Self {
            client: Client::new(),
            api_base: "https://api.github.com".to_string(),
            token: get_github_env_token(),
        }
    }

    #[cfg(test)]
    pub(crate) fn with_base_url(api_base: String) -> Self {
        Self {
            client: Client::new(),
            api_base,
            token: None,
        }
    }

    pub async fn fetch_tree(
        &self,
        repository: &str,
        git_ref: &str,
        validation: Option<&str>,
    ) -> GithubTreeFetchOutcome {
        let commit_url = format!(
            "{}/repos/{}/commits/{}",
            self.api_base.trim_end_matches('/'),
            repository,
            urlencoding::encode(git_ref)
        );
        let mut commit_request = self
            .client
            .get(commit_url)
            .header("Accept", "application/vnd.github.v3+json")
            .header("User-Agent", "skill-deck");
        if let Some(token) = &self.token {
            commit_request = commit_request.header("Authorization", format!("Bearer {token}"));
        }
        let commit_response = match commit_request.send().await {
            Ok(response) => response,
            Err(_) => return GithubTreeFetchOutcome::Failed(GithubTreeFailure::Network),
        };
        if let Some(failure) = response_failure(&commit_response) {
            return failure;
        }
        let ref_revision = match commit_response.json::<CommitResponse>().await {
            Ok(body) if !body.sha.is_empty() => body.sha,
            _ => return GithubTreeFetchOutcome::Failed(GithubTreeFailure::Network),
        };
        let url = format!(
            "{}/repos/{}/git/trees/{}?recursive=1",
            self.api_base.trim_end_matches('/'),
            repository,
            urlencoding::encode(&ref_revision)
        );
        let mut request = self
            .client
            .get(url)
            .header("Accept", "application/vnd.github.v3+json")
            .header("User-Agent", "skill-deck");
        if let Some(token) = &self.token {
            request = request.header("Authorization", format!("Bearer {token}"));
        }
        if let Some(validation) = validation {
            request = request.header(IF_NONE_MATCH, validation);
        }
        let response = match request.send().await {
            Ok(response) => response,
            Err(_) => return GithubTreeFetchOutcome::Failed(GithubTreeFailure::Network),
        };
        let status = response.status();
        if status == reqwest::StatusCode::NOT_MODIFIED {
            return GithubTreeFetchOutcome::NotModified { ref_revision };
        }
        if let Some(failure) = response_failure(&response) {
            return failure;
        }
        let validation = response
            .headers()
            .get(ETAG)
            .and_then(|value| value.to_str().ok())
            .map(str::to_string);
        let body = match response.json::<TreesResponse>().await {
            Ok(body) => body,
            Err(_) => return GithubTreeFetchOutcome::Failed(GithubTreeFailure::Network),
        };
        if body.truncated {
            return GithubTreeFetchOutcome::Incomplete;
        }
        GithubTreeFetchOutcome::Modified(GithubTreeSnapshot {
            ref_revision,
            root_tree_revision: body.sha,
            validation,
            entries: body
                .tree
                .into_iter()
                .map(|entry| GithubTreeSnapshotEntry {
                    path: entry.path,
                    entry_type: entry.entry_type,
                    revision: entry.sha,
                })
                .collect(),
        })
    }
}

fn response_failure(response: &reqwest::Response) -> Option<GithubTreeFetchOutcome> {
    let status = response.status();
    if status.is_success() {
        return None;
    }
    if status == reqwest::StatusCode::UNAUTHORIZED {
        return Some(GithubTreeFetchOutcome::Failed(
            GithubTreeFailure::AuthenticationRequired,
        ));
    }
    if status == reqwest::StatusCode::NOT_FOUND {
        return Some(GithubTreeFetchOutcome::Failed(
            GithubTreeFailure::NotFoundOrUnauthorized,
        ));
    }
    if status == reqwest::StatusCode::FORBIDDEN
        && response
            .headers()
            .get("X-RateLimit-Remaining")
            .and_then(|value| value.to_str().ok())
            == Some("0")
    {
        let retry_at_epoch_ms = response
            .headers()
            .get(RETRY_AFTER)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse::<u64>().ok())
            .map(|seconds| {
                chrono::Utc::now().timestamp_millis().max(0) as u64 + seconds.saturating_mul(1_000)
            })
            .or_else(|| {
                response
                    .headers()
                    .get("X-RateLimit-Reset")
                    .and_then(|value| value.to_str().ok())
                    .and_then(|value| value.parse::<u64>().ok())
                    .map(|seconds| seconds.saturating_mul(1_000))
            });
        return Some(GithubTreeFetchOutcome::RateLimited { retry_at_epoch_ms });
    }
    Some(GithubTreeFetchOutcome::Failed(
        GithubTreeFailure::SourceUnavailable,
    ))
}

fn get_github_env_token() -> Option<String> {
    if let Ok(token) = std::env::var("GITHUB_TOKEN") {
        if !token.trim().is_empty() {
            return Some(token);
        }
    }

    if let Ok(token) = std::env::var("GH_TOKEN") {
        if !token.trim().is_empty() {
            return Some(token);
        }
    }

    None
}

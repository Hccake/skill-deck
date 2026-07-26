//! GitHub API 模块
//!
//! 功能：
//! - 获取 GitHub token
//! - 调用 GitHub Trees API 获取完整、可验证的 source evidence

use reqwest::header::{ETAG, IF_NONE_MATCH, RETRY_AFTER};
use reqwest::Client;
use serde::Deserialize;
use std::sync::Arc;

/// GitHub Trees API 响应
#[derive(Debug, Deserialize)]
struct TreesResponse {
    sha: String,
    #[serde(default)]
    truncated: bool,
    tree: Vec<TreeEntry>,
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
    NotModified,
    RateLimited { retry_at_epoch_ms: Option<u64> },
    Incomplete,
    Failed(GithubTreeFailure),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GithubTokenValidation {
    Verified {
        login: String,
        rate_limit_remaining: Option<u64>,
        rate_limit_limit: Option<u64>,
        rate_limit_reset_at_epoch_ms: Option<u64>,
    },
    Invalid,
    RateLimited {
        retry_at_epoch_ms: Option<u64>,
    },
    Unavailable,
}

pub trait GithubTokenProvider: Send + Sync {
    fn token(&self) -> Option<String>;
}

struct EnvironmentGithubTokenProvider;

impl GithubTokenProvider for EnvironmentGithubTokenProvider {
    fn token(&self) -> Option<String> {
        get_github_env_token()
    }
}

#[cfg(test)]
struct EmptyGithubTokenProvider;

#[cfg(test)]
impl GithubTokenProvider for EmptyGithubTokenProvider {
    fn token(&self) -> Option<String> {
        None
    }
}

#[derive(Clone)]
pub struct GithubApiClient {
    client: Client,
    api_base: String,
    token_provider: Arc<dyn GithubTokenProvider>,
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
            token_provider: Arc::new(EnvironmentGithubTokenProvider),
        }
    }

    pub fn with_token_provider(token_provider: Arc<dyn GithubTokenProvider>) -> Self {
        Self {
            client: Client::new(),
            api_base: "https://api.github.com".to_string(),
            token_provider,
        }
    }

    #[cfg(test)]
    pub(crate) fn with_base_url(api_base: String) -> Self {
        Self {
            client: Client::new(),
            api_base,
            token_provider: Arc::new(EmptyGithubTokenProvider),
        }
    }

    #[cfg(test)]
    pub(crate) fn with_base_url_and_provider(
        api_base: String,
        token_provider: Arc<dyn GithubTokenProvider>,
    ) -> Self {
        Self {
            client: Client::new(),
            api_base,
            token_provider,
        }
    }

    pub async fn fetch_tree(
        &self,
        repository: &str,
        git_ref: &str,
        validation: Option<&str>,
    ) -> GithubTreeFetchOutcome {
        let url = format!(
            "{}/repos/{}/git/trees/{}?recursive=1",
            self.api_base.trim_end_matches('/'),
            repository,
            urlencoding::encode(git_ref)
        );
        let token = self.token_provider.token();
        let mut response = match self.send_request(&url, validation, token.as_deref()).await {
            Ok(response) => response,
            Err(_) => return GithubTreeFetchOutcome::Failed(GithubTreeFailure::Network),
        };
        if response.status() == reqwest::StatusCode::UNAUTHORIZED && token.is_some() {
            response = match self.send_request(&url, validation, None).await {
                Ok(response) => response,
                Err(_) => return GithubTreeFetchOutcome::Failed(GithubTreeFailure::Network),
            };
        }
        let status = response.status();
        if status == reqwest::StatusCode::NOT_MODIFIED {
            return GithubTreeFetchOutcome::NotModified;
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
        if body.sha.is_empty() {
            return GithubTreeFetchOutcome::Failed(GithubTreeFailure::SourceUnavailable);
        }
        let source_revision = body.sha.clone();
        GithubTreeFetchOutcome::Modified(GithubTreeSnapshot {
            ref_revision: source_revision,
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

    pub async fn validate_token(&self, token: &str) -> GithubTokenValidation {
        let url = format!("{}/user", self.api_base.trim_end_matches('/'));
        let response = match self
            .client
            .get(url)
            .header("Accept", "application/vnd.github.v3+json")
            .header("User-Agent", "skill-deck")
            .header("Authorization", format!("Bearer {token}"))
            .send()
            .await
        {
            Ok(response) => response,
            Err(_) => return GithubTokenValidation::Unavailable,
        };

        if response.status() == reqwest::StatusCode::UNAUTHORIZED {
            return GithubTokenValidation::Invalid;
        }
        if is_rate_limited(&response) {
            return GithubTokenValidation::RateLimited {
                retry_at_epoch_ms: retry_at_epoch_ms(&response),
            };
        }
        if !response.status().is_success() {
            return GithubTokenValidation::Unavailable;
        }

        let rate_limit_remaining = response_header_u64(&response, "X-RateLimit-Remaining");
        let rate_limit_limit = response_header_u64(&response, "X-RateLimit-Limit");
        let rate_limit_reset_at_epoch_ms = response_header_u64(&response, "X-RateLimit-Reset")
            .map(|seconds| seconds.saturating_mul(1_000));
        let account = match response.json::<GithubUserResponse>().await {
            Ok(account) if !account.login.trim().is_empty() => account,
            _ => return GithubTokenValidation::Unavailable,
        };
        GithubTokenValidation::Verified {
            login: account.login,
            rate_limit_remaining,
            rate_limit_limit,
            rate_limit_reset_at_epoch_ms,
        }
    }

    async fn send_request(
        &self,
        url: &str,
        validation: Option<&str>,
        token: Option<&str>,
    ) -> Result<reqwest::Response, reqwest::Error> {
        let mut request = self
            .client
            .get(url)
            .header("Accept", "application/vnd.github.v3+json")
            .header("User-Agent", "skill-deck");
        if let Some(token) = token {
            request = request.header("Authorization", format!("Bearer {token}"));
        }
        if let Some(validation) = validation {
            request = request.header(IF_NONE_MATCH, validation);
        }
        request.send().await
    }
}

#[derive(Deserialize)]
struct GithubUserResponse {
    login: String,
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
    if is_rate_limited(response) {
        return Some(GithubTreeFetchOutcome::RateLimited {
            retry_at_epoch_ms: retry_at_epoch_ms(response),
        });
    }
    Some(GithubTreeFetchOutcome::Failed(
        GithubTreeFailure::SourceUnavailable,
    ))
}

fn is_rate_limited(response: &reqwest::Response) -> bool {
    response.status() == reqwest::StatusCode::FORBIDDEN
        && response_header_u64(response, "X-RateLimit-Remaining") == Some(0)
}

fn retry_at_epoch_ms(response: &reqwest::Response) -> Option<u64> {
    response_header_u64(response, RETRY_AFTER.as_str())
        .map(|seconds| {
            chrono::Utc::now().timestamp_millis().max(0) as u64 + seconds.saturating_mul(1_000)
        })
        .or_else(|| {
            response_header_u64(response, "X-RateLimit-Reset")
                .map(|seconds| seconds.saturating_mul(1_000))
        })
}

fn response_header_u64(response: &reqwest::Response, name: &str) -> Option<u64> {
    response
        .headers()
        .get(name)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
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

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::net::SocketAddr;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};
    use std::thread;

    use super::*;

    struct TokenProvider(Mutex<Option<String>>);

    impl GithubTokenProvider for TokenProvider {
        fn token(&self) -> Option<String> {
            self.0.lock().unwrap().clone()
        }
    }

    struct HttpResponse {
        status: u16,
        headers: Vec<(&'static str, &'static str)>,
        body: &'static str,
    }

    type RecordedHeaders = Vec<(String, String)>;

    struct HttpFixture {
        addr: SocketAddr,
        requests: Arc<Mutex<Vec<RecordedHeaders>>>,
        stopped: Arc<AtomicBool>,
        worker: Option<thread::JoinHandle<()>>,
    }

    impl HttpFixture {
        fn new(responses: Vec<HttpResponse>) -> Self {
            let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
            let addr = server.server_addr().to_ip().unwrap();
            let requests = Arc::new(Mutex::new(Vec::new()));
            let stopped = Arc::new(AtomicBool::new(false));
            let worker = {
                let requests = requests.clone();
                let stopped = stopped.clone();
                thread::spawn(move || {
                    let mut responses = VecDeque::from(responses);
                    while !stopped.load(Ordering::SeqCst) {
                        let Some(request) = server
                            .recv_timeout(std::time::Duration::from_millis(10))
                            .unwrap()
                        else {
                            continue;
                        };
                        requests.lock().unwrap().push(
                            request
                                .headers()
                                .iter()
                                .map(|header| {
                                    (header.field.to_string(), header.value.as_str().to_string())
                                })
                                .collect(),
                        );
                        let response = responses.pop_front().unwrap();
                        let mut reply = tiny_http::Response::from_string(response.body)
                            .with_status_code(response.status);
                        for (name, value) in response.headers {
                            reply.add_header(tiny_http::Header::from_bytes(name, value).unwrap());
                        }
                        request.respond(reply).unwrap();
                    }
                })
            };
            Self {
                addr,
                requests,
                stopped,
                worker: Some(worker),
            }
        }

        fn base_url(&self) -> String {
            format!("http://{}", self.addr)
        }
    }

    impl Drop for HttpFixture {
        fn drop(&mut self) {
            self.stopped.store(true, Ordering::SeqCst);
            if let Some(worker) = self.worker.take() {
                let _ = worker.join();
            }
        }
    }

    #[tokio::test]
    async fn invalid_configured_token_retries_public_tree_anonymously_once() {
        let fixture = HttpFixture::new(vec![
            HttpResponse {
                status: 401,
                headers: vec![],
                body: "{}",
            },
            HttpResponse {
                status: 200,
                headers: vec![("Content-Type", "application/json")],
                body: r#"{"sha":"tree-1","truncated":false,"tree":[]}"#,
            },
        ]);
        let provider = Arc::new(TokenProvider(Mutex::new(Some("secret-token".to_string()))));
        let client = GithubApiClient::with_base_url_and_provider(fixture.base_url(), provider);

        let outcome = client.fetch_tree("owner/repo", "main", None).await;

        assert!(matches!(outcome, GithubTreeFetchOutcome::Modified(_)));
        let requests = fixture.requests.lock().unwrap();
        assert_eq!(requests.len(), 2);
        assert_eq!(
            header_value(&requests[0], "authorization"),
            Some("Bearer secret-token")
        );
        assert_eq!(header_value(&requests[1], "authorization"), None);
    }

    #[tokio::test]
    async fn token_validation_returns_account_and_rate_limit_without_echoing_token() {
        let fixture = HttpFixture::new(vec![HttpResponse {
            status: 200,
            headers: vec![
                ("Content-Type", "application/json"),
                ("X-RateLimit-Remaining", "4999"),
                ("X-RateLimit-Limit", "5000"),
                ("X-RateLimit-Reset", "2"),
            ],
            body: r#"{"login":"octocat"}"#,
        }]);
        let client = GithubApiClient::with_base_url(fixture.base_url());

        let validation = client.validate_token("secret-token").await;

        assert_eq!(
            validation,
            GithubTokenValidation::Verified {
                login: "octocat".to_string(),
                rate_limit_remaining: Some(4_999),
                rate_limit_limit: Some(5_000),
                rate_limit_reset_at_epoch_ms: Some(2_000),
            }
        );
        assert!(!format!("{validation:?}").contains("secret-token"));
    }

    fn header_value<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
        headers
            .iter()
            .find(|(header_name, _)| header_name.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }
}

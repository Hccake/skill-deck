//! GitHub API 模块
//!
//! 功能：
//! - 获取 GitHub token
//! - 调用 GitHub Trees API 获取完整、可验证的 source evidence

use reqwest::header::{HeaderMap, HeaderValue, ETAG, IF_NONE_MATCH, RETRY_AFTER};
use serde::Deserialize;
use std::sync::Arc;
use std::time::Duration;

use crate::core::{
    GithubTokenProvider, GithubTokenValidation, GithubTreeFailure, GithubTreeFetchOutcome,
    GithubTreeSnapshot, GithubTreeSnapshotEntry,
};
use crate::models::{NetworkProxySettings, ProxyMode};
use crate::runtime::http_transport::{HttpGetRequest, HttpResponse, HttpTransport};
use crate::runtime::proxy_settings::ProxySettingsStore;

const RATE_LIMIT_FALLBACK_MS: u64 = 5 * 60 * 1_000;

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
    http: HttpTransport,
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
        Self::with_network(default_direct_http())
    }

    pub(crate) fn with_network(http: HttpTransport) -> Self {
        Self {
            http,
            api_base: "https://api.github.com".to_string(),
            token_provider: Arc::new(EnvironmentGithubTokenProvider),
        }
    }

    pub(crate) fn with_token_provider_and_network(
        token_provider: Arc<dyn GithubTokenProvider>,
        http: HttpTransport,
    ) -> Self {
        Self {
            http,
            api_base: "https://api.github.com".to_string(),
            token_provider,
        }
    }

    #[cfg(test)]
    pub(crate) fn with_base_url(api_base: String) -> Self {
        Self {
            http: default_direct_http(),
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
            http: default_direct_http(),
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
        if response.status == reqwest::StatusCode::UNAUTHORIZED && token.is_some() {
            response = match self.send_request(&url, validation, None).await {
                Ok(response) => response,
                Err(_) => return GithubTreeFetchOutcome::Failed(GithubTreeFailure::Network),
            };
        }
        let status = response.status;
        if status == reqwest::StatusCode::NOT_MODIFIED {
            return GithubTreeFetchOutcome::NotModified;
        }
        if is_rate_limited(status, &response.headers, &response.body) {
            let retry_at_epoch_ms = retry_at_epoch_ms(&response.headers);
            return GithubTreeFetchOutcome::RateLimited { retry_at_epoch_ms };
        }
        if let Some(failure) = response_failure(status) {
            return failure;
        }
        let validation = response
            .headers
            .get(ETAG)
            .and_then(|value| value.to_str().ok())
            .map(str::to_string);
        let body = match serde_json::from_slice::<TreesResponse>(&response.body) {
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
        let mut headers = github_headers();
        let Ok(value) = HeaderValue::from_str(&format!("Bearer {token}")) else {
            return GithubTokenValidation::Invalid;
        };
        headers.insert("Authorization", value);
        let request =
            HttpGetRequest::new(url, Duration::from_secs(30), 1024 * 1024).headers(headers);
        let response = match self.http.get(request).await {
            Ok(response) => response,
            Err(_) => return GithubTokenValidation::Unavailable,
        };

        if response.status == reqwest::StatusCode::UNAUTHORIZED {
            return GithubTokenValidation::Invalid;
        }
        if is_rate_limited(response.status, &response.headers, &response.body) {
            let retry_at_epoch_ms = retry_at_epoch_ms(&response.headers);
            return GithubTokenValidation::RateLimited { retry_at_epoch_ms };
        }
        if !response.status.is_success() {
            return GithubTokenValidation::Unavailable;
        }

        let rate_limit_remaining = response_header_u64(&response.headers, "X-RateLimit-Remaining");
        let rate_limit_limit = response_header_u64(&response.headers, "X-RateLimit-Limit");
        let rate_limit_reset_at_epoch_ms =
            response_header_u64(&response.headers, "X-RateLimit-Reset")
                .map(|seconds| seconds.saturating_mul(1_000));
        let account = match serde_json::from_slice::<GithubUserResponse>(&response.body) {
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
    ) -> Result<HttpResponse, crate::runtime::http_transport::HttpTransportError> {
        let mut headers = github_headers();
        if let Some(token) = token {
            if let Ok(value) = HeaderValue::from_str(&format!("Bearer {token}")) {
                headers.insert("Authorization", value);
            }
        }
        if let Some(validation) = validation {
            if let Ok(value) = HeaderValue::from_str(validation) {
                headers.insert(IF_NONE_MATCH, value);
            }
        }
        let request =
            HttpGetRequest::new(url, Duration::from_secs(30), 10 * 1024 * 1024).headers(headers);
        self.http.get(request).await
    }
}

fn github_headers() -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(
        "Accept",
        HeaderValue::from_static("application/vnd.github+json"),
    );
    headers.insert(
        "X-GitHub-Api-Version",
        HeaderValue::from_static("2022-11-28"),
    );
    headers.insert("User-Agent", HeaderValue::from_static("skill-deck"));
    headers
}

fn default_direct_http() -> HttpTransport {
    let settings = NetworkProxySettings {
        mode: ProxyMode::Direct,
        ..NetworkProxySettings::default()
    };
    HttpTransport::new(Arc::new(ProxySettingsStore::new(settings)))
}

#[derive(Deserialize)]
struct GithubUserResponse {
    login: String,
}

fn response_failure(status: reqwest::StatusCode) -> Option<GithubTreeFetchOutcome> {
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
    Some(GithubTreeFetchOutcome::Failed(
        GithubTreeFailure::SourceUnavailable,
    ))
}

fn is_rate_limited(status: reqwest::StatusCode, headers: &HeaderMap, body: &[u8]) -> bool {
    if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
        return true;
    }
    if status != reqwest::StatusCode::FORBIDDEN {
        return false;
    }
    if response_header_u64(headers, "X-RateLimit-Remaining") == Some(0)
        || headers.contains_key(RETRY_AFTER)
    {
        return true;
    }
    let body = String::from_utf8_lossy(body).to_ascii_lowercase();
    body.contains("secondary rate limit") || body.contains("abuse detection")
}

fn retry_at_epoch_ms(headers: &HeaderMap) -> Option<u64> {
    let now_epoch_ms = now_epoch_ms();
    Some(rate_limit_retry_deadline(
        now_epoch_ms,
        response_header_u64(headers, RETRY_AFTER.as_str()),
        response_header_u64(headers, "X-RateLimit-Reset"),
    ))
}

fn now_epoch_ms() -> u64 {
    chrono::Utc::now().timestamp_millis().max(0) as u64
}

fn rate_limit_retry_deadline(
    now_epoch_ms: u64,
    retry_after_seconds: Option<u64>,
    rate_limit_reset_seconds: Option<u64>,
) -> u64 {
    retry_after_seconds
        .map(|seconds| now_epoch_ms.saturating_add(seconds.saturating_mul(1_000)))
        .or_else(|| rate_limit_reset_seconds.map(|seconds| seconds.saturating_mul(1_000)))
        .unwrap_or_else(|| now_epoch_ms.saturating_add(RATE_LIMIT_FALLBACK_MS))
}

fn response_header_u64(headers: &HeaderMap, name: &str) -> Option<u64> {
    headers
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

    #[tokio::test]
    async fn recognizes_429_and_secondary_403_as_rate_limited() {
        for status in [429, 403] {
            let headers = if status == 403 {
                vec![("Retry-After", "2")]
            } else {
                vec![]
            };
            let fixture = HttpFixture::new(vec![HttpResponse {
                status,
                headers,
                body: "{}",
            }]);
            let client = GithubApiClient::with_base_url(fixture.base_url());
            let outcome = client.fetch_tree("owner/repo", "main", None).await;
            assert!(matches!(
                outcome,
                GithubTreeFetchOutcome::RateLimited {
                    retry_at_epoch_ms: Some(_)
                }
            ));
        }
    }

    #[tokio::test]
    async fn recognizes_secondary_rate_limit_from_the_response_body() {
        let fixture = HttpFixture::new(vec![HttpResponse {
            status: 403,
            headers: vec![],
            body: r#"{"message":"You have exceeded a secondary rate limit."}"#,
        }]);
        let client = GithubApiClient::with_base_url(fixture.base_url());

        let outcome = client.fetch_tree("owner/repo", "main", None).await;

        assert!(matches!(
            outcome,
            GithubTreeFetchOutcome::RateLimited { .. }
        ));
    }

    #[tokio::test]
    async fn github_requests_use_the_versioned_json_media_type() {
        let fixture = HttpFixture::new(vec![HttpResponse {
            status: 200,
            headers: vec![("Content-Type", "application/json")],
            body: r#"{"sha":"tree-1","truncated":false,"tree":[]}"#,
        }]);
        let client = GithubApiClient::with_base_url(fixture.base_url());

        client.fetch_tree("owner/repo", "main", None).await;

        let requests = fixture.requests.lock().unwrap();
        assert_eq!(
            header_value(&requests[0], "accept"),
            Some("application/vnd.github+json")
        );
        assert_eq!(
            header_value(&requests[0], "x-github-api-version"),
            Some("2022-11-28")
        );
    }

    #[tokio::test]
    async fn tree_fetch_uses_custom_proxy_without_putting_the_token_in_the_url() {
        let proxy = tiny_http::Server::http("127.0.0.1:0").expect("GitHub proxy");
        let proxy_url = format!("http://{}", proxy.server_addr());
        let recorded: Arc<Mutex<Option<(String, RecordedHeaders)>>> = Arc::new(Mutex::new(None));
        let recorded_for_worker = recorded.clone();
        let worker = thread::spawn(move || {
            let request = proxy
                .recv_timeout(Duration::from_secs(2))
                .expect("GitHub proxy receive")
                .expect("GitHub proxied request");
            let url = request.url().to_string();
            let headers = request
                .headers()
                .iter()
                .map(|header| (header.field.to_string(), header.value.as_str().to_string()))
                .collect();
            *recorded_for_worker.lock().expect("recorded request lock") = Some((url, headers));
            request
                .respond(
                    tiny_http::Response::from_string(
                        r#"{"sha":"tree-proxied","truncated":false,"tree":[]}"#,
                    )
                    .with_header(
                        tiny_http::Header::from_bytes("Content-Type", "application/json")
                            .expect("Content-Type header"),
                    ),
                )
                .expect("GitHub proxy response");
        });
        let http = HttpTransport::new(Arc::new(ProxySettingsStore::new(NetworkProxySettings {
            mode: ProxyMode::Custom,
            custom_proxy_url: Some(proxy_url),
            ..NetworkProxySettings::default()
        })));
        let client = GithubApiClient {
            http,
            api_base: "http://127.0.0.1:45678".to_string(),
            token_provider: Arc::new(TokenProvider(Mutex::new(Some("secret-token".to_string())))),
        };

        let outcome = client.fetch_tree("owner/repo", "main", None).await;

        worker.join().expect("GitHub proxy worker");
        assert!(matches!(
            outcome,
            GithubTreeFetchOutcome::Modified(GithubTreeSnapshot { ref_revision, .. })
                if ref_revision == "tree-proxied"
        ));
        let recorded = recorded.lock().expect("recorded request lock");
        let (url, headers) = recorded.as_ref().expect("recorded GitHub request");
        assert_eq!(
            url,
            "http://127.0.0.1:45678/repos/owner/repo/git/trees/main?recursive=1"
        );
        assert!(!url.contains("secret-token"));
        assert_eq!(
            header_value(headers, "authorization"),
            Some("Bearer secret-token")
        );
    }

    #[tokio::test]
    async fn primary_rate_limit_uses_the_reset_deadline() {
        let fixture = HttpFixture::new(vec![HttpResponse {
            status: 403,
            headers: vec![
                ("X-RateLimit-Remaining", "0"),
                ("X-RateLimit-Reset", "4102444800"),
            ],
            body: "{}",
        }]);
        let client = GithubApiClient::with_base_url(fixture.base_url());

        let outcome = client.fetch_tree("owner/repo", "main", None).await;

        assert_eq!(
            outcome,
            GithubTreeFetchOutcome::RateLimited {
                retry_at_epoch_ms: Some(4_102_444_800_000),
            }
        );
    }

    #[tokio::test]
    async fn github_requests_follow_the_http_clients_default_redirect_policy() {
        let redirected = HttpFixture::new(vec![HttpResponse {
            status: 200,
            headers: vec![("Content-Type", "application/json")],
            body: r#"{"sha":"tree-1","truncated":false,"tree":[]}"#,
        }]);
        let redirected_url = redirected.base_url().replacen("127.0.0.1", "localhost", 1);
        let origin = HttpFixture::new(vec![HttpResponse {
            status: 302,
            headers: vec![("Location", Box::leak(redirected_url.into_boxed_str()))],
            body: "{}",
        }]);
        let client = GithubApiClient::with_base_url(origin.base_url());

        let outcome = client.fetch_tree("owner/repo", "main", None).await;

        assert!(matches!(outcome, GithubTreeFetchOutcome::Modified(_)));
        assert_eq!(redirected.requests.lock().unwrap().len(), 1);
    }

    #[test]
    fn rate_limit_without_deadline_uses_exactly_five_minutes_from_detection() {
        assert_eq!(
            rate_limit_retry_deadline(1_000_000, None, None),
            1_000_000 + RATE_LIMIT_FALLBACK_MS,
        );
    }

    #[test]
    fn rate_limit_retry_after_wins_over_reset_deadline() {
        assert_eq!(
            rate_limit_retry_deadline(1_000_000, Some(2), Some(4_102_444_800)),
            1_002_000,
        );
    }

    #[test]
    fn retry_after_overflow_saturates_instead_of_panicking() {
        assert_eq!(
            rate_limit_retry_deadline(1_000_000, Some(u64::MAX), None),
            u64::MAX,
        );
    }

    fn header_value<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
        headers
            .iter()
            .find(|(header_name, _)| header_name.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }
}

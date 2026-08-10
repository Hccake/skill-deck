use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, CONTENT_TYPE, USER_AGENT};
use scraper::{Html, Selector};

use crate::application::discovery::{
    DiscoverLeaderboardPayload, DiscoverLeaderboardTab, DiscoverSearchPayload,
};
use crate::error::AppError;
use crate::runtime::http_transport::{
    HttpGetRequest, HttpResponse, HttpTransport, HttpTransportError,
};

const DISCOVERY_TIMEOUT: Duration = Duration::from_secs(30);
const DISCOVERY_ORIGIN: &str = "https://www.skills.sh/";
pub(crate) const DISCOVERY_CONNECTION_TEST_TARGET: &str =
    "https://www.skills.sh/api/search?q=skill&limit=1";
const SEARCH_LIMIT: &str = "100";
const MAX_SEARCH_BYTES: usize = 2 * 1024 * 1024;
const MAX_HTML_BYTES: usize = 8 * 1024 * 1024;

#[derive(Clone, Copy)]
enum DiscoveryResponseKind {
    Json,
    Html,
}

#[derive(Clone)]
pub(crate) struct DiscoveryGateway {
    client: HttpTransport,
    origin: url::Url,
    official_creators: Arc<tokio::sync::OnceCell<Vec<String>>>,
    official_creators_loading: Arc<AtomicBool>,
}

impl DiscoveryGateway {
    pub(crate) fn new(client: HttpTransport) -> Self {
        Self::from_origin(
            client,
            url::Url::parse(DISCOVERY_ORIGIN).expect("fixed Discovery origin must be valid"),
        )
    }

    fn from_origin(client: HttpTransport, origin: url::Url) -> Self {
        Self {
            client,
            origin,
            official_creators: Arc::new(tokio::sync::OnceCell::new()),
            official_creators_loading: Arc::new(AtomicBool::new(false)),
        }
    }

    pub(crate) async fn search(&self, query: &str) -> Result<DiscoverSearchPayload, AppError> {
        let search_url = build_search_url(&self.origin, query)?;
        self.start_official_creators_load();
        let search_json = self
            .fetch(search_url, DiscoveryResponseKind::Json, MAX_SEARCH_BYTES)
            .await?;
        Ok(DiscoverSearchPayload {
            search_json,
            official_creators: self.official_creators.get().cloned(),
        })
    }

    pub(crate) async fn leaderboard(
        &self,
        tab: DiscoverLeaderboardTab,
    ) -> Result<DiscoverLeaderboardPayload, AppError> {
        let path = match tab {
            DiscoverLeaderboardTab::Popular => "",
            DiscoverLeaderboardTab::Trending => "trending",
            DiscoverLeaderboardTab::Hot => "hot",
        };
        let leaderboard_url = self
            .origin
            .join(path)
            .expect("fixed Discovery leaderboard path must be valid");
        self.start_official_creators_load();
        let leaderboard_html = self
            .fetch(leaderboard_url, DiscoveryResponseKind::Html, MAX_HTML_BYTES)
            .await?;
        Ok(DiscoverLeaderboardPayload {
            leaderboard_html,
            official_creators: self.official_creators.get().cloned(),
        })
    }

    pub(crate) async fn detail(&self, source: &str, skill: &str) -> Result<String, AppError> {
        self.fetch(
            build_detail_url(&self.origin, source, skill)?,
            DiscoveryResponseKind::Html,
            MAX_HTML_BYTES,
        )
        .await
    }

    fn start_official_creators_load(&self) {
        if self.official_creators.get().is_some()
            || self.official_creators_loading.swap(true, Ordering::AcqRel)
        {
            return;
        }

        let gateway = self.clone();
        tokio::spawn(async move {
            if let Err(error) = gateway.load_official_creators().await {
                log::warn!("Discovery official metadata is unavailable: {error}");
            }
            gateway
                .official_creators_loading
                .store(false, Ordering::Release);
        });
    }

    async fn load_official_creators(&self) -> Result<Vec<String>, AppError> {
        self.official_creators
            .get_or_try_init(|| async {
                let official_url = self
                    .origin
                    .join("official")
                    .expect("fixed Discovery official path must be valid");
                let html = self
                    .fetch(official_url, DiscoveryResponseKind::Html, MAX_HTML_BYTES)
                    .await?;
                parse_official_creators(&html)
            })
            .await
            .cloned()
    }

    async fn fetch(
        &self,
        url: url::Url,
        response_kind: DiscoveryResponseKind,
        max_body_bytes: usize,
    ) -> Result<String, AppError> {
        fetch_discovery(&self.client, url, response_kind, max_body_bytes).await
    }
}

fn parse_official_creators(html: &str) -> Result<Vec<String>, AppError> {
    let document = Html::parse_document(html);
    let anchors = Selector::parse("a[href]").expect("fixed selector must be valid");
    let mut creators = document
        .select(&anchors)
        .filter_map(|anchor| anchor.value().attr("href"))
        .filter_map(|href| {
            let slug = href.trim().strip_prefix('/')?;
            (!slug.is_empty() && !slug.contains('/')).then(|| slug.to_string())
        })
        .collect::<Vec<_>>();
    creators.sort_unstable();
    creators.dedup();
    if creators.is_empty() {
        return Err(discovery_error("officialMetadata"));
    }
    Ok(creators)
}

fn build_search_url(origin: &url::Url, query: &str) -> Result<url::Url, AppError> {
    let query = query.trim();
    if query.is_empty() || query.chars().count() > 200 {
        return Err(validation_error("query"));
    }
    let mut url = origin
        .join("api/search")
        .expect("fixed Discovery search path must be valid");
    url.query_pairs_mut()
        .append_pair("q", query)
        .append_pair("limit", SEARCH_LIMIT);
    Ok(url)
}

fn build_detail_url(origin: &url::Url, source: &str, skill: &str) -> Result<url::Url, AppError> {
    let source_segments = source.split('/').collect::<Vec<_>>();
    let repository_source = source_segments.len() == 2
        && source_segments
            .iter()
            .all(|segment| valid_segment(segment, false));
    let site_source =
        source_segments.len() == 1 && source.contains('.') && valid_segment(source, false);
    if (!repository_source && !site_source) || !valid_segment(skill, true) {
        return Err(validation_error("source"));
    }

    let mut url = origin.clone();
    {
        let mut path = url
            .path_segments_mut()
            .expect("HTTP Discovery URL can hold path segments");
        path.clear();
        if site_source {
            path.push("site");
        }
        for segment in source_segments {
            path.push(segment);
        }
        path.push(skill);
    }
    Ok(url)
}

fn valid_segment(value: &str, allow_colon: bool) -> bool {
    !value.is_empty()
        && value != "."
        && value != ".."
        && value.chars().count() <= 128
        && value.chars().all(|character| {
            character.is_ascii_alphanumeric()
                || matches!(character, '-' | '_' | '.')
                || (allow_colon && character == ':')
        })
}

fn validation_error(field: &str) -> AppError {
    AppError::Validation {
        field: Some(field.to_string()),
        message: "invalid Discovery request".to_string(),
    }
}

async fn fetch_discovery(
    client: &HttpTransport,
    url: url::Url,
    response_kind: DiscoveryResponseKind,
    max_body_bytes: usize,
) -> Result<String, AppError> {
    let started_at = std::time::Instant::now();
    let mut headers = HeaderMap::new();
    headers.insert(
        USER_AGENT,
        HeaderValue::from_str(concat!("SkillDeck/", env!("CARGO_PKG_VERSION")))
            .expect("application version must form a valid User-Agent"),
    );
    headers.insert(
        ACCEPT,
        HeaderValue::from_static(match response_kind {
            DiscoveryResponseKind::Json => "application/json",
            DiscoveryResponseKind::Html => "text/html,application/xhtml+xml",
        }),
    );
    let response = client
        .get(HttpGetRequest::new(url.as_str(), DISCOVERY_TIMEOUT, max_body_bytes).headers(headers))
        .await;
    let response = match response {
        Ok(response) => response,
        Err(error) => {
            log::warn!(
                "Discovery request failed: target={}, elapsed_ms={}, reason={}",
                url,
                started_at.elapsed().as_millis(),
                map_network_error_code(&error),
            );
            return Err(map_network_error(error));
        }
    };
    log::info!(
        "Discovery response: final_url={}, status={}, elapsed_ms={}, server={}, x_vercel_id={}, cf_mitigated={}",
        response.final_url,
        response.status.as_u16(),
        started_at.elapsed().as_millis(),
        diagnostic_header(&response.headers, "server"),
        diagnostic_header(&response.headers, "x-vercel-id"),
        diagnostic_header(&response.headers, "cf-mitigated"),
    );
    validate_discovery_response(response, response_kind)
}

fn diagnostic_header(headers: &HeaderMap, name: &str) -> String {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("-")
        .chars()
        .take(256)
        .collect()
}

fn validate_discovery_response(
    response: HttpResponse,
    response_kind: DiscoveryResponseKind,
) -> Result<String, AppError> {
    let cloudflare_challenge = response
        .headers
        .get("cf-mitigated")
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.eq_ignore_ascii_case("challenge"))
        || contains_ascii_case_insensitive(&response.body, b"cf-chl")
        || contains_ascii_case_insensitive(&response.body, b"just a moment...");
    if cloudflare_challenge {
        return Err(discovery_error("challenge"));
    }
    if response.status == reqwest::StatusCode::FORBIDDEN {
        return Err(discovery_error("forbidden"));
    }
    if response.status == reqwest::StatusCode::TOO_MANY_REQUESTS {
        return Err(discovery_error("rateLimited"));
    }
    if !response.status.is_success() {
        return Err(discovery_error("httpStatus"));
    }
    let content_type = response
        .headers
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let valid_content_type = match response_kind {
        DiscoveryResponseKind::Json => content_type.starts_with("application/json"),
        DiscoveryResponseKind::Html => {
            content_type.starts_with("text/html")
                || content_type.starts_with("application/xhtml+xml")
        }
    };
    if !valid_content_type {
        return Err(discovery_error("contentType"));
    }
    let body = String::from_utf8(response.body).map_err(|_| discovery_error("encoding"))?;
    if matches!(response_kind, DiscoveryResponseKind::Json)
        && serde_json::from_str::<serde_json::Value>(&body).is_err()
    {
        return Err(discovery_error("parse"));
    }
    Ok(body)
}

fn contains_ascii_case_insensitive(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|window| {
        window
            .iter()
            .zip(needle)
            .all(|(left, right)| left.eq_ignore_ascii_case(right))
    })
}

fn map_network_error(error: HttpTransportError) -> AppError {
    discovery_error(map_network_error_code(&error))
}

fn map_network_error_code(error: &HttpTransportError) -> &'static str {
    match error {
        HttpTransportError::ResponseTooLarge => "responseTooLarge",
        HttpTransportError::Settings(_) => "proxyUnavailable",
        HttpTransportError::Request { .. } => "network",
    }
}

fn discovery_error(reason: &str) -> AppError {
    AppError::DiscoveryRequestFailed {
        reason: reason.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::Ordering;
    use std::sync::Arc;
    use std::thread;
    use std::time::Duration;

    use super::*;
    use crate::models::{NetworkProxySettings, ProxyMode};
    use crate::runtime::proxy_settings::ProxySettingsStore;

    fn local_gateway(origin: url::Url) -> DiscoveryGateway {
        let http = HttpTransport::new(Arc::new(ProxySettingsStore::new(
            NetworkProxySettings::default(),
        )));
        DiscoveryGateway::from_origin(http, origin)
    }

    fn local_origin(server: &tiny_http::Server) -> url::Url {
        url::Url::parse(&format!("http://{}/", server.server_addr()))
            .expect("local Discovery origin")
    }

    fn content_type(value: &str) -> tiny_http::Header {
        tiny_http::Header::from_bytes("Content-Type", value).expect("Content-Type header")
    }

    fn respond_search(request: tiny_http::Request) {
        request
            .respond(
                tiny_http::Response::from_string(r#"{"skills":[]}"#)
                    .with_header(content_type("application/json")),
            )
            .expect("Discovery search response");
    }

    fn respond_official(request: tiny_http::Request) {
        request
            .respond(
                tiny_http::Response::from_string(
                    r#"<html><a href="/official">Official</a></html>"#,
                )
                .with_header(content_type("text/html")),
            )
            .expect("Discovery official response");
    }

    async fn wait_for_official_load_to_finish(gateway: &DiscoveryGateway) {
        tokio::time::timeout(Duration::from_secs(1), async {
            while gateway.official_creators_loading.load(Ordering::Acquire) {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("official metadata load must finish");
    }

    #[tokio::test]
    async fn concurrent_searches_do_not_wait_for_or_duplicate_official_metadata() {
        let server = tiny_http::Server::http("127.0.0.1:0").expect("Discovery server");
        let origin = local_origin(&server);
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let server_thread = thread::spawn(move || {
            let mut started_tx = Some(started_tx);
            let mut release_rx = Some(release_rx);
            let mut official_handler = None;
            let mut official_calls = 0;
            for _ in 0..4 {
                let request = server
                    .recv_timeout(Duration::from_secs(2))
                    .expect("Discovery receive")
                    .expect("Discovery request before deadline");
                if request.url() == "/official" {
                    official_calls += 1;
                    let started = started_tx.take().expect("one official request");
                    let release = release_rx.take().expect("one official request release");
                    official_handler = Some(thread::spawn(move || {
                        started.send(()).expect("official request start");
                        release
                            .recv_timeout(Duration::from_secs(2))
                            .expect("official request release");
                        respond_official(request);
                    }));
                } else {
                    respond_search(request);
                }
            }
            official_handler
                .expect("official request handler")
                .join()
                .expect("official request thread");
            official_calls
        });
        let gateway = local_gateway(origin);

        let first_gateway = gateway.clone();
        let first = tokio::spawn(async move { first_gateway.search("rust").await });
        started_rx.await.expect("official request started");
        let first = tokio::time::timeout(Duration::from_secs(1), first)
            .await
            .expect("official metadata must not delay search")
            .expect("first search task")
            .expect("first search");
        assert_eq!(first.search_json, r#"{"skills":[]}"#);
        assert_eq!(first.official_creators, None);

        let second = gateway.search("rust").await.expect("second search");
        assert_eq!(second.official_creators, None);

        release_tx.send(()).expect("release official request");
        wait_for_official_load_to_finish(&gateway).await;
        let cached = gateway.search("rust").await.expect("cached search");

        let official_calls = server_thread.join().expect("Discovery server thread");
        assert_eq!(cached.official_creators, Some(vec!["official".to_string()]));
        assert_eq!(official_calls, 1);
    }

    #[tokio::test]
    async fn failed_official_metadata_is_retried_by_a_later_search() {
        let server = tiny_http::Server::http("127.0.0.1:0").expect("Discovery server");
        let origin = local_origin(&server);
        let server_thread = thread::spawn(move || {
            let mut official_calls = 0;
            for _ in 0..5 {
                let request = server
                    .recv_timeout(Duration::from_secs(2))
                    .expect("Discovery receive")
                    .expect("Discovery request before deadline");
                if request.url() == "/official" {
                    official_calls += 1;
                    if official_calls == 1 {
                        request
                            .respond(tiny_http::Response::empty(500))
                            .expect("failed official response");
                    } else {
                        respond_official(request);
                    }
                } else {
                    respond_search(request);
                }
            }
            official_calls
        });
        let gateway = local_gateway(origin);

        let degraded = gateway.search("rust").await.expect("degraded search");
        wait_for_official_load_to_finish(&gateway).await;
        let _retrying = gateway.search("rust").await.expect("retrying search");
        wait_for_official_load_to_finish(&gateway).await;
        let recovered = gateway.search("rust").await.expect("recovered search");

        let official_calls = server_thread.join().expect("Discovery server thread");
        assert_eq!(degraded.official_creators, None);
        assert_eq!(
            recovered.official_creators,
            Some(vec!["official".to_string()])
        );
        assert_eq!(official_calls, 2);
    }

    #[tokio::test]
    async fn invalid_search_does_not_start_network_io() {
        let server = tiny_http::Server::http("127.0.0.1:0").expect("Discovery server");
        let gateway = local_gateway(local_origin(&server));

        let error = gateway.search("   ").await.expect_err("invalid search");

        assert!(matches!(error, AppError::Validation { .. }));
        assert!(server
            .try_recv()
            .expect("inspect Discovery server")
            .is_none());
    }

    #[tokio::test]
    async fn detail_uses_the_configured_custom_proxy() {
        let proxy = tiny_http::Server::http("127.0.0.1:0").expect("Discovery proxy");
        let proxy_url = format!("http://{}", proxy.server_addr());
        let requested_url = Arc::new(std::sync::Mutex::new(None));
        let requested_url_for_worker = requested_url.clone();
        let worker = thread::spawn(move || {
            let request = proxy
                .recv_timeout(Duration::from_secs(2))
                .expect("Discovery proxy receive")
                .expect("Discovery proxied request");
            *requested_url_for_worker.lock().expect("request URL lock") =
                Some(request.url().to_string());
            request
                .respond(
                    tiny_http::Response::from_string("<html>proxied detail</html>")
                        .with_header(content_type("text/html")),
                )
                .expect("Discovery proxy response");
        });
        let http = HttpTransport::new(Arc::new(ProxySettingsStore::new(NetworkProxySettings {
            mode: ProxyMode::Custom,
            custom_proxy_url: Some(proxy_url),
            ..NetworkProxySettings::default()
        })));
        let origin = url::Url::parse("http://127.0.0.1:45678/").expect("Discovery target");
        let gateway = DiscoveryGateway::from_origin(http, origin);

        let detail = gateway
            .detail("owner/repo", "demo")
            .await
            .expect("proxied Discovery detail");

        worker.join().expect("Discovery proxy worker");
        assert_eq!(detail, "<html>proxied detail</html>");
        assert_eq!(
            requested_url.lock().expect("request URL lock").as_deref(),
            Some("http://127.0.0.1:45678/owner/repo/demo")
        );
    }

    #[test]
    fn discovery_urls_are_built_from_structured_inputs() {
        let origin = url::Url::parse(DISCOVERY_ORIGIN).expect("Discovery origin");
        assert_eq!(
            build_search_url(&origin, "react hooks")
                .expect("search URL")
                .as_str(),
            "https://www.skills.sh/api/search?q=react+hooks&limit=100"
        );
        assert_eq!(
            build_detail_url(&origin, "vercel-labs/skills", "find-skills")
                .expect("repository detail URL")
                .as_str(),
            "https://www.skills.sh/vercel-labs/skills/find-skills"
        );
        assert_eq!(
            build_detail_url(&origin, "docs.stripe.com", "stripe-best-practices")
                .expect("site detail URL")
                .as_str(),
            "https://www.skills.sh/site/docs.stripe.com/stripe-best-practices"
        );

        assert!(build_detail_url(&origin, "../admin", "secret").is_err());
        assert!(build_detail_url(&origin, "https://example.com", "skill").is_err());
        assert!(build_detail_url(&origin, "owner/repo", "../secret").is_err());
    }

    #[test]
    fn cloudflare_challenge_is_recognized_even_on_a_success_status() {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert("cf-mitigated", "challenge".parse().unwrap());
        headers.insert(reqwest::header::CONTENT_TYPE, "text/html".parse().unwrap());
        let error = validate_discovery_response(
            HttpResponse {
                status: reqwest::StatusCode::OK,
                headers,
                final_url: url::Url::parse("https://skills.sh/").unwrap(),
                body: b"<html>Just a moment...</html>".to_vec(),
            },
            DiscoveryResponseKind::Html,
        )
        .expect_err("challenge response");

        assert!(matches!(
            error,
            AppError::DiscoveryRequestFailed { reason } if reason == "challenge"
        ));
    }
}

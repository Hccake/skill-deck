use std::sync::{Arc, Mutex};
use std::time::Duration;

use reqwest::header::HeaderMap;

use crate::core::mutation::CancellationSignal;
use crate::runtime::proxy_settings::{ProxySettingsError, ProxySettingsStore};

pub(crate) struct HttpGetRequest {
    operation_id: String,
    target: String,
    timeout: Duration,
    headers: HeaderMap,
    max_body_bytes: Option<usize>,
    cancellation: Option<CancellationSignal>,
}

impl HttpGetRequest {
    pub(crate) fn new(target: impl Into<String>, timeout: Duration, max_body_bytes: usize) -> Self {
        Self::with_body_limit(target, timeout, Some(max_body_bytes))
    }

    pub(crate) fn status_only(target: impl Into<String>, timeout: Duration) -> Self {
        Self::with_body_limit(target, timeout, None)
    }

    fn with_body_limit(
        target: impl Into<String>,
        timeout: Duration,
        max_body_bytes: Option<usize>,
    ) -> Self {
        Self {
            operation_id: uuid::Uuid::new_v4().simple().to_string(),
            target: target.into(),
            timeout,
            headers: HeaderMap::new(),
            max_body_bytes,
            cancellation: None,
        }
    }

    pub(crate) fn operation_id(mut self, operation_id: impl Into<String>) -> Self {
        self.operation_id = operation_id.into();
        self
    }

    pub(crate) fn headers(mut self, headers: HeaderMap) -> Self {
        self.headers = headers;
        self
    }

    pub(crate) fn cancellation(mut self, cancellation: CancellationSignal) -> Self {
        self.cancellation = Some(cancellation);
        self
    }
}

pub(crate) struct HttpResponse {
    pub(crate) status: reqwest::StatusCode,
    pub(crate) headers: HeaderMap,
    pub(crate) final_url: url::Url,
    pub(crate) body: Vec<u8>,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum HttpTransportError {
    #[error("network proxy settings are invalid")]
    Settings(#[from] ProxySettingsError),
    #[error("network request failed during {stage}: {reason}")]
    Request {
        stage: &'static str,
        reason: &'static str,
    },
    #[error("network response exceeded the configured size limit")]
    ResponseTooLarge,
}

#[derive(Clone)]
pub(crate) struct HttpTransport {
    settings: Arc<ProxySettingsStore>,
    clients: Arc<Mutex<HttpClientPool>>,
}

#[derive(Default)]
struct HttpClientPool {
    entry: Option<(Option<String>, reqwest::Client)>,
}

impl HttpClientPool {
    fn get(&self, proxy_url: &Option<String>) -> Option<reqwest::Client> {
        self.entry
            .as_ref()
            .filter(|(cached_proxy, _)| cached_proxy == proxy_url)
            .map(|(_, client)| client.clone())
    }

    fn insert(&mut self, proxy_url: Option<String>, client: reqwest::Client) -> reqwest::Client {
        self.entry = Some((proxy_url, client.clone()));
        client
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        usize::from(self.entry.is_some())
    }
}

impl HttpTransport {
    pub(crate) fn new(settings: Arc<ProxySettingsStore>) -> Self {
        Self {
            settings,
            clients: Arc::new(Mutex::new(HttpClientPool::default())),
        }
    }

    pub(crate) async fn get(
        &self,
        request: HttpGetRequest,
    ) -> Result<HttpResponse, HttpTransportError> {
        log::debug!(
            "Network request started: operation_id={}",
            request.operation_id,
        );
        if request.timeout.is_zero() {
            return self.fail_before_request(&request.operation_id, request_timeout());
        }
        if request
            .cancellation
            .as_ref()
            .is_some_and(CancellationSignal::is_cancelled)
        {
            return self.fail_before_request(
                &request.operation_id,
                HttpTransportError::Request {
                    stage: "request",
                    reason: "cancelled",
                },
            );
        }

        let proxy_url = match self.settings.proxy_url() {
            Ok(proxy_url) => proxy_url,
            Err(error) => return self.fail_before_request(&request.operation_id, error.into()),
        };
        log::debug!(
            "Network settings read: operation_id={}, proxy={}",
            request.operation_id,
            if proxy_url.is_some() {
                "custom"
            } else {
                "direct"
            },
        );

        let operation_id = request.operation_id.clone();
        let result = self.get_with_proxy(request, proxy_url).await;
        match &result {
            Ok(response) => log::debug!(
                "Network request completed: operation_id={}, status={}",
                operation_id,
                response.status.as_u16(),
            ),
            Err(error) => log::warn!(
                "Network request failed: operation_id={}, error={}",
                operation_id,
                error,
            ),
        }
        result
    }

    fn fail_before_request<T>(
        &self,
        operation_id: &str,
        error: HttpTransportError,
    ) -> Result<T, HttpTransportError> {
        log::warn!(
            "Network request failed: operation_id={}, error={}",
            operation_id,
            error,
        );
        Err(error)
    }

    async fn get_with_proxy(
        &self,
        request: HttpGetRequest,
        proxy_url: Option<String>,
    ) -> Result<HttpResponse, HttpTransportError> {
        let request_once =
            tokio::time::timeout(request.timeout, self.get_once(&request, proxy_url));
        let result = if let Some(cancellation) = request.cancellation.clone() {
            tokio::select! {
                result = request_once => result,
                () = cancellation.cancelled() => {
                    return Err(HttpTransportError::Request {
                        stage: "request",
                        reason: "cancelled",
                    });
                }
            }
        } else {
            request_once.await
        };
        result.map_err(|_| request_timeout())?
    }

    async fn get_once(
        &self,
        request: &HttpGetRequest,
        proxy_url: Option<String>,
    ) -> Result<HttpResponse, HttpTransportError> {
        let target = url::Url::parse(&request.target).map_err(|_| invalid_target())?;
        if !matches!(target.scheme(), "http" | "https") || target.host_str().is_none() {
            return Err(invalid_target());
        }
        let client = self.client_for(proxy_url)?;
        let mut response = client
            .get(target)
            .headers(request.headers.clone())
            .send()
            .await
            .map_err(|error| HttpTransportError::Request {
                stage: if error.is_redirect() {
                    "redirect"
                } else {
                    "request"
                },
                reason: if error.is_timeout() {
                    "timeout"
                } else {
                    "transport"
                },
            })?;
        let status = response.status();
        let headers = response.headers().clone();
        let final_url = response.url().clone();
        let mut body = Vec::new();
        if let Some(max_body_bytes) = request.max_body_bytes {
            if response
                .content_length()
                .is_some_and(|length| length > max_body_bytes as u64)
            {
                return Err(HttpTransportError::ResponseTooLarge);
            }
            while let Some(chunk) =
                response
                    .chunk()
                    .await
                    .map_err(|_| HttpTransportError::Request {
                        stage: "response_body",
                        reason: "transport",
                    })?
            {
                if body.len().saturating_add(chunk.len()) > max_body_bytes {
                    return Err(HttpTransportError::ResponseTooLarge);
                }
                body.extend_from_slice(&chunk);
            }
        }
        Ok(HttpResponse {
            status,
            headers,
            final_url,
            body,
        })
    }

    fn client_for(&self, proxy_url: Option<String>) -> Result<reqwest::Client, HttpTransportError> {
        if let Some(client) = self
            .clients
            .lock()
            .expect("HTTP client pool lock")
            .get(&proxy_url)
        {
            return Ok(client);
        }

        let builder =
            match proxy_url.as_deref() {
                None => reqwest::Client::builder().no_proxy(),
                Some(endpoint) => reqwest::Client::builder().proxy(
                    reqwest::Proxy::all(endpoint).map_err(|_| HttpTransportError::Request {
                        stage: "prepare",
                        reason: "invalid_proxy",
                    })?,
                ),
            };
        let client = builder.build().map_err(|_| HttpTransportError::Request {
            stage: "prepare",
            reason: "client_build",
        })?;
        let mut clients = self.clients.lock().expect("HTTP client pool lock");
        Ok(clients.insert(proxy_url, client))
    }

    #[cfg(test)]
    fn cached_client_count(&self) -> usize {
        self.clients.lock().expect("HTTP client pool lock").len()
    }

    #[cfg(test)]
    pub(crate) fn shares_client_pool_with(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.clients, &other.clients)
    }
}

fn invalid_target() -> HttpTransportError {
    HttpTransportError::Request {
        stage: "prepare",
        reason: "invalid_target",
    }
}

fn request_timeout() -> HttpTransportError {
    HttpTransportError::Request {
        stage: "request",
        reason: "timeout",
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;
    use std::net::TcpListener;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex, Once};
    use std::thread;
    use std::time::Duration;

    use crate::models::{NetworkProxySettings, ProxyMode};
    use crate::runtime::proxy_settings::ProxySettingsStore;

    use super::{HttpGetRequest, HttpTransport};

    const PRE_CANCEL_OPERATION_ID: &str = "network-http-pre-cancel-log";
    static PRE_CANCEL_LOG_OBSERVED: AtomicBool = AtomicBool::new(false);
    static TEST_LOGGER_AVAILABLE: AtomicBool = AtomicBool::new(false);
    static TEST_LOGGER_INIT: Once = Once::new();
    static TEST_LOGGER: PreCancelLogger = PreCancelLogger;

    struct PreCancelLogger;

    impl log::Log for PreCancelLogger {
        fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
            metadata.level() <= log::Level::Warn
        }

        fn log(&self, record: &log::Record<'_>) {
            if self.enabled(record.metadata()) {
                let message = record.args().to_string();
                if record.level() == log::Level::Warn
                    && message.contains(PRE_CANCEL_OPERATION_ID)
                    && message.contains("network request failed during request: cancelled")
                {
                    PRE_CANCEL_LOG_OBSERVED.store(true, Ordering::Release);
                }
            }
        }

        fn flush(&self) {}
    }

    fn enable_pre_cancel_log_capture() {
        TEST_LOGGER_INIT.call_once(|| {
            if log::set_logger(&TEST_LOGGER).is_ok() {
                log::set_max_level(log::LevelFilter::Warn);
                TEST_LOGGER_AVAILABLE.store(true, Ordering::Release);
            }
        });
        assert!(
            TEST_LOGGER_AVAILABLE.load(Ordering::Acquire),
            "test logger was already initialized by another test"
        );
        PRE_CANCEL_LOG_OBSERVED.store(false, Ordering::Release);
    }

    fn direct_client() -> HttpTransport {
        HttpTransport::new(Arc::new(ProxySettingsStore::new(
            NetworkProxySettings::default(),
        )))
    }

    #[test]
    fn requests_receive_unique_operation_ids_and_allow_workflow_correlation() {
        let first = HttpGetRequest::new("https://example.com/first", Duration::from_secs(1), 1024);
        let second =
            HttpGetRequest::new("https://example.com/second", Duration::from_secs(1), 1024);
        assert_ne!(first.operation_id, second.operation_id);

        let correlated = second.operation_id("well-known-operation");
        assert_eq!(correlated.operation_id, "well-known-operation");
    }

    #[tokio::test]
    async fn custom_proxy_routes_the_request_through_the_recording_proxy() {
        let server = tiny_http::Server::http("127.0.0.1:0").expect("proxy server");
        let proxy_url = format!(
            "http://{}",
            server.server_addr().to_ip().expect("proxy addr")
        );
        let requested_url = Arc::new(Mutex::new(None));
        let requested_url_for_worker = requested_url.clone();
        let worker = thread::spawn(move || {
            let request = server
                .recv_timeout(Duration::from_secs(2))
                .expect("proxy receive")
                .expect("proxied request");
            *requested_url_for_worker.lock().expect("request URL lock") =
                Some(request.url().to_string());
            request
                .respond(tiny_http::Response::from_string("proxied"))
                .expect("proxy response");
        });
        let client = HttpTransport::new(Arc::new(ProxySettingsStore::new(NetworkProxySettings {
            mode: ProxyMode::Custom,
            custom_proxy_url: Some(proxy_url),
            ..NetworkProxySettings::default()
        })));

        let response = client
            .get(HttpGetRequest::new(
                "http://127.0.0.1:45678/skills?q=rust",
                Duration::from_secs(2),
                1024,
            ))
            .await
            .expect("proxied response");

        worker.join().expect("proxy worker");
        assert_eq!(response.body, b"proxied");
        assert_eq!(
            requested_url.lock().expect("request URL lock").as_deref(),
            Some("http://127.0.0.1:45678/skills?q=rust")
        );
    }

    #[tokio::test]
    async fn request_after_settings_change_uses_the_current_connection_mode() {
        let proxy = tiny_http::Server::http("127.0.0.1:0").expect("proxy server");
        let proxy_url = format!(
            "http://{}",
            proxy.server_addr().to_ip().expect("proxy addr")
        );
        let proxy_worker = thread::spawn(move || {
            proxy
                .recv_timeout(Duration::from_secs(2))
                .expect("proxy receive")
                .expect("proxied request")
                .respond(tiny_http::Response::from_string("proxied"))
                .expect("proxy response");
        });
        let origin = tiny_http::Server::http("127.0.0.1:0").expect("origin server");
        let target = format!(
            "http://{}/skill.md",
            origin.server_addr().to_ip().expect("origin addr")
        );
        let origin_worker = thread::spawn(move || {
            origin
                .recv_timeout(Duration::from_secs(2))
                .expect("origin receive")
                .expect("direct request")
                .respond(tiny_http::Response::from_string("direct"))
                .expect("origin response");
        });
        let settings = Arc::new(ProxySettingsStore::new(NetworkProxySettings {
            mode: ProxyMode::Custom,
            custom_proxy_url: Some(proxy_url),
            ..NetworkProxySettings::default()
        }));
        let client = HttpTransport::new(settings.clone());

        let proxied = client
            .get(HttpGetRequest::new(&target, Duration::from_secs(2), 1024))
            .await
            .expect("proxied response");
        settings.replace_settings(NetworkProxySettings::default());
        let direct = client
            .get(HttpGetRequest::new(&target, Duration::from_secs(2), 1024))
            .await
            .expect("direct response");

        proxy_worker.join().expect("proxy worker");
        origin_worker.join().expect("origin worker");
        assert_eq!(proxied.body, b"proxied");
        assert_eq!(direct.body, b"direct");
        assert_eq!(client.cached_client_count(), 1);
    }

    #[tokio::test]
    async fn transport_follows_client_default_cross_origin_redirects() {
        let redirect = tiny_http::Server::http("127.0.0.1:0").expect("redirect server");
        let destination = tiny_http::Server::http("127.0.0.1:0").expect("destination server");
        let target = format!(
            "http://{}/index.json",
            redirect.server_addr().to_ip().expect("redirect addr")
        );
        let destination_url = format!(
            "http://{}/artifact.zip",
            destination.server_addr().to_ip().expect("destination addr")
        );
        let redirect_worker = thread::spawn(move || {
            let request = redirect
                .recv_timeout(Duration::from_secs(2))
                .expect("redirect receive")
                .expect("redirect request");
            let location = tiny_http::Header::from_bytes("Location", destination_url)
                .expect("location header");
            request
                .respond(tiny_http::Response::empty(302).with_header(location))
                .expect("redirect response");
        });
        let destination_worker = thread::spawn(move || {
            destination
                .recv_timeout(Duration::from_secs(2))
                .expect("destination receive")
                .expect("redirected request")
                .respond(tiny_http::Response::from_string("redirected"))
                .expect("destination response");
        });

        let response = direct_client()
            .get(HttpGetRequest::new(target, Duration::from_secs(2), 1024))
            .await
            .expect("redirected response");

        redirect_worker.join().expect("redirect worker");
        destination_worker.join().expect("destination worker");
        assert_eq!(response.body, b"redirected");
    }

    #[tokio::test]
    async fn caller_controls_the_response_body_limit() {
        let server = tiny_http::Server::http("127.0.0.1:0").expect("origin server");
        let target = format!(
            "http://{}/index.json",
            server.server_addr().to_ip().expect("origin addr")
        );
        let worker = thread::spawn(move || {
            server
                .recv_timeout(Duration::from_secs(2))
                .expect("origin receive")
                .expect("origin request")
                .respond(tiny_http::Response::from_data(vec![b'x'; 33]))
                .expect("origin response");
        });

        let result = direct_client()
            .get(HttpGetRequest::new(target, Duration::from_secs(2), 32))
            .await;

        worker.join().expect("origin worker");
        assert!(matches!(
            result,
            Err(super::HttpTransportError::ResponseTooLarge)
        ));
    }

    #[tokio::test]
    async fn total_timeout_includes_response_body_reading() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("origin listener");
        let target = format!(
            "http://{}/slow",
            listener.local_addr().expect("origin addr")
        );
        let worker = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("origin request");
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\n\r\no")
                .expect("partial response");
            thread::sleep(Duration::from_millis(250));
            let _ = stream.write_all(b"kay");
        });
        let started = std::time::Instant::now();

        let result = direct_client()
            .get(HttpGetRequest::new(target, Duration::from_millis(40), 1024))
            .await;
        let elapsed = started.elapsed();

        worker.join().expect("origin worker");
        assert!(elapsed < Duration::from_millis(200));
        assert!(matches!(
            result,
            Err(super::HttpTransportError::Request {
                reason: "timeout",
                ..
            })
        ));
    }

    #[tokio::test]
    async fn cancellation_aborts_an_in_flight_request() {
        let server = tiny_http::Server::http("127.0.0.1:0").expect("origin server");
        let target = format!(
            "http://{}/artifact.zip",
            server.server_addr().to_ip().expect("origin addr")
        );
        let (request_started_tx, request_started_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let worker = thread::spawn(move || {
            let request = server
                .recv_timeout(Duration::from_secs(2))
                .expect("origin receive")
                .expect("origin request");
            let _ = request_started_tx.send(());
            let _ = release_rx.recv_timeout(Duration::from_secs(2));
            let _ = request.respond(tiny_http::Response::from_data(b"late"));
        });
        let cancellation = crate::core::mutation::CancellationSignal::default();
        let request_cancellation = cancellation.clone();
        let client = direct_client();

        let task = tokio::spawn(async move {
            client
                .get(
                    HttpGetRequest::new(target, Duration::from_secs(10), 1024)
                        .cancellation(request_cancellation),
                )
                .await
        });
        request_started_rx.await.expect("request started");
        cancellation.cancel();

        let result = task.await.expect("request task");
        let _ = release_tx.send(());
        worker.join().expect("origin worker");
        assert!(matches!(
            result,
            Err(super::HttpTransportError::Request {
                stage: "request",
                reason: "cancelled",
            })
        ));
    }

    #[tokio::test]
    async fn cancellation_before_request_is_logged_and_stops_before_io() {
        enable_pre_cancel_log_capture();
        let cancellation = crate::core::mutation::CancellationSignal::default();
        cancellation.cancel();

        let result = direct_client()
            .get(
                HttpGetRequest::new(
                    "https://example.com/artifact.zip",
                    Duration::from_secs(10),
                    1024,
                )
                .operation_id(PRE_CANCEL_OPERATION_ID)
                .cancellation(cancellation),
            )
            .await;

        assert!(matches!(
            result,
            Err(super::HttpTransportError::Request {
                stage: "request",
                reason: "cancelled",
            })
        ));
        assert!(PRE_CANCEL_LOG_OBSERVED.load(Ordering::Acquire));
    }

    #[tokio::test]
    async fn requests_with_unchanged_settings_reuse_one_client() {
        let server = tiny_http::Server::http("127.0.0.1:0").expect("origin server");
        let target = format!(
            "http://{}/artifact.zip",
            server.server_addr().to_ip().expect("origin addr")
        );
        let worker = thread::spawn(move || {
            for _ in 0..2 {
                server
                    .recv_timeout(Duration::from_secs(2))
                    .expect("origin receive")
                    .expect("origin request")
                    .respond(tiny_http::Response::from_data(b"ok"))
                    .expect("origin response");
            }
        });
        let client = direct_client();

        for _ in 0..2 {
            client
                .get(HttpGetRequest::new(&target, Duration::from_secs(2), 1024))
                .await
                .expect("request");
        }

        worker.join().expect("origin worker");
        assert_eq!(client.cached_client_count(), 1);
    }
}

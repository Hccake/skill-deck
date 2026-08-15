use std::time::Duration;

use crate::core::mutation::CancellationSignal;
use crate::error::{AppError, DirectDownloadFailureReason};
use crate::runtime::http_transport::{HttpGetRequest, HttpTransport, HttpTransportError};

const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_DOWNLOAD_BYTES: usize = 10 * 1024 * 1024;

#[derive(Clone)]
pub(crate) struct RuntimeDownloadAccess {
    http: HttpTransport,
    timeout: Duration,
    max_download_bytes: usize,
}

impl RuntimeDownloadAccess {
    pub(crate) fn new(http: HttpTransport) -> Self {
        Self {
            http,
            timeout: DOWNLOAD_TIMEOUT,
            max_download_bytes: MAX_DOWNLOAD_BYTES,
        }
    }

    #[cfg(test)]
    fn with_limits(http: HttpTransport, timeout: Duration, max_download_bytes: usize) -> Self {
        Self {
            http,
            timeout,
            max_download_bytes,
        }
    }

    pub(crate) async fn fetch(
        &self,
        url: &str,
        cancellation: &CancellationSignal,
    ) -> Result<DownloadFetchResult, AppError> {
        let response = self
            .http
            .get(
                HttpGetRequest::new(url, self.timeout, self.max_download_bytes)
                    .cancellation(cancellation.clone()),
            )
            .await
            .map_err(map_network_error)?;
        if !response.status.is_success() {
            let reason = match response.status.as_u16() {
                401 | 403 => DirectDownloadFailureReason::AuthenticationRequired,
                404 => DirectDownloadFailureReason::NotFound,
                _ => DirectDownloadFailureReason::Network,
            };
            return Err(AppError::DirectDownloadFailed { reason });
        }
        Ok(DownloadFetchResult {
            bytes: response.body,
            final_url: response.final_url.into(),
        })
    }
}

#[derive(Debug, Clone)]
pub(crate) struct DownloadFetchResult {
    pub(crate) bytes: Vec<u8>,
    pub(crate) final_url: String,
}

fn map_network_error(error: HttpTransportError) -> AppError {
    match error {
        HttpTransportError::Request {
            reason: "cancelled",
            ..
        } => AppError::MutationCancelled,
        HttpTransportError::ResponseTooLarge => AppError::DirectDownloadFailed {
            reason: DirectDownloadFailureReason::DownloadTooLarge,
        },
        HttpTransportError::Request {
            reason: "timeout", ..
        } => AppError::DirectDownloadFailed {
            reason: DirectDownloadFailureReason::Timeout,
        },
        error => {
            log::warn!("Direct download request failed: {error}");
            AppError::DirectDownloadFailed {
                reason: DirectDownloadFailureReason::Network,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;
    use std::sync::Arc;
    use std::thread;
    use std::time::Duration;

    use crate::application::download_source::materialize_download;
    use crate::core::mutation::CancellationSignal;
    use crate::models::NetworkProxySettings;
    use crate::runtime::http_transport::HttpTransport;
    use crate::runtime::proxy_settings::ProxySettingsStore;

    use super::{RuntimeDownloadAccess, MAX_DOWNLOAD_BYTES};

    fn access() -> RuntimeDownloadAccess {
        RuntimeDownloadAccess::new(HttpTransport::new(Arc::new(ProxySettingsStore::new(
            NetworkProxySettings::default(),
        ))))
    }

    #[tokio::test]
    async fn follows_redirect_and_reports_the_final_url() {
        let server = tiny_http::Server::http("127.0.0.1:0").expect("server");
        let base = format!(
            "http://{}",
            server.server_addr().to_ip().expect("server address")
        );
        let initial = format!("{base}/initial");
        let final_url = format!("{base}/SKILL.md");
        let worker =
            thread::spawn(move || {
                let first = server
                    .recv_timeout(Duration::from_secs(2))
                    .expect("receive redirect")
                    .expect("redirect request");
                first
                    .respond(tiny_http::Response::empty(302).with_header(
                        tiny_http::Header::from_bytes("Location", "/SKILL.md").unwrap(),
                    ))
                    .expect("redirect response");
                server
                    .recv_timeout(Duration::from_secs(2))
                    .expect("receive content")
                    .expect("content request")
                    .respond(tiny_http::Response::from_string(
                        "---\nname: demo\n---\n# Demo\n",
                    ))
                    .expect("content response");
            });

        let fetched = access()
            .fetch(&initial, &CancellationSignal::default())
            .await
            .expect("download");
        worker.join().expect("server worker");
        assert_eq!(fetched.final_url, final_url);
        assert!(fetched.bytes.starts_with(b"---\nname: demo"));
    }

    #[tokio::test]
    async fn rejects_http_failures_and_pre_cancelled_requests() {
        let server = tiny_http::Server::http("127.0.0.1:0").expect("server");
        let target = format!(
            "http://{}/missing",
            server.server_addr().to_ip().expect("server address")
        );
        let worker = thread::spawn(move || {
            server
                .recv_timeout(Duration::from_secs(2))
                .expect("receive request")
                .expect("request")
                .respond(tiny_http::Response::empty(404))
                .expect("response");
        });
        assert!(matches!(
            access()
                .fetch(&target, &CancellationSignal::default())
                .await,
            Err(crate::error::AppError::DirectDownloadFailed {
                reason: crate::error::DirectDownloadFailureReason::NotFound
            })
        ));
        worker.join().expect("server worker");

        let server = tiny_http::Server::http("127.0.0.1:0").expect("server");
        let target = format!(
            "http://{}/private",
            server.server_addr().to_ip().expect("server address")
        );
        let worker = thread::spawn(move || {
            server
                .recv_timeout(Duration::from_secs(2))
                .expect("receive request")
                .expect("request")
                .respond(tiny_http::Response::empty(401))
                .expect("response");
        });
        assert!(matches!(
            access()
                .fetch(&target, &CancellationSignal::default())
                .await,
            Err(crate::error::AppError::DirectDownloadFailed {
                reason: crate::error::DirectDownloadFailureReason::AuthenticationRequired
            })
        ));
        worker.join().expect("server worker");

        let cancellation = CancellationSignal::default();
        cancellation.cancel();
        assert!(matches!(
            access().fetch("http://127.0.0.1:1", &cancellation).await,
            Err(crate::error::AppError::MutationCancelled)
        ));
    }

    #[tokio::test]
    async fn cancels_an_in_flight_download() {
        let server = tiny_http::Server::http("127.0.0.1:0").expect("server");
        let target = format!(
            "http://{}/blocked",
            server.server_addr().to_ip().expect("server address")
        );
        let (received_tx, received_rx) = std::sync::mpsc::channel();
        let worker = thread::spawn(move || {
            let request = server
                .recv_timeout(Duration::from_secs(2))
                .expect("receive request")
                .expect("request");
            received_tx.send(()).expect("signal request");
            thread::sleep(Duration::from_millis(200));
            let _ = request.respond(tiny_http::Response::from_string("late"));
        });
        let cancellation = CancellationSignal::default();
        let cancel = cancellation.clone();
        let canceller = thread::spawn(move || {
            received_rx
                .recv_timeout(Duration::from_secs(2))
                .expect("request signal");
            cancel.cancel();
        });

        let result = access().fetch(&target, &cancellation).await;
        canceller.join().expect("canceller");
        worker.join().expect("server worker");
        assert!(matches!(
            result,
            Err(crate::error::AppError::MutationCancelled)
        ));
    }

    fn zip_skill() -> Vec<u8> {
        let mut bytes = Vec::new();
        {
            let mut archive = zip::ZipWriter::new(std::io::Cursor::new(&mut bytes));
            archive
                .start_file("demo/SKILL.md", zip::write::SimpleFileOptions::default())
                .unwrap();
            archive
                .write_all(b"---\nname: demo\ndescription: Demo\n---\n# Demo\n")
                .unwrap();
            archive.finish().unwrap();
        }
        bytes
    }

    fn tar_skill(gzip: bool) -> Vec<u8> {
        let mut bytes = Vec::new();
        {
            let mut builder = tar::Builder::new(&mut bytes);
            let content = b"---\nname: demo\ndescription: Demo\n---\n# Demo\n";
            let mut header = tar::Header::new_gnu();
            header.set_size(content.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            builder
                .append_data(&mut header, "demo/SKILL.md", content.as_slice())
                .unwrap();
            builder.finish().unwrap();
        }
        if !gzip {
            return bytes;
        }
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(&bytes).unwrap();
        encoder.finish().unwrap()
    }

    #[tokio::test]
    async fn downloads_each_supported_content_format_from_local_http() {
        for body in [
            b"---\nname: demo\ndescription: Demo\n---\n# Demo\n".to_vec(),
            zip_skill(),
            tar_skill(false),
            tar_skill(true),
        ] {
            let server = tiny_http::Server::http("127.0.0.1:0").expect("server");
            let target = format!(
                "http://{}/artifact",
                server.server_addr().to_ip().expect("server address")
            );
            let worker = thread::spawn(move || {
                server
                    .recv_timeout(Duration::from_secs(2))
                    .expect("receive request")
                    .expect("request")
                    .respond(tiny_http::Response::from_data(body))
                    .expect("response");
            });
            let fetched = access()
                .fetch(&target, &CancellationSignal::default())
                .await
                .expect("download");
            worker.join().expect("server worker");
            let materialized = materialize_download(&fetched.bytes).expect("materialize");
            assert!(materialized.path().exists());
        }
    }

    #[tokio::test]
    async fn rejects_a_response_over_the_download_limit() {
        let server = tiny_http::Server::http("127.0.0.1:0").expect("server");
        let target = format!(
            "http://{}/large",
            server.server_addr().to_ip().expect("server address")
        );
        let worker = thread::spawn(move || {
            server
                .recv_timeout(Duration::from_secs(2))
                .expect("receive request")
                .expect("request")
                .respond(tiny_http::Response::from_data(vec![
                    b'x';
                    MAX_DOWNLOAD_BYTES + 1
                ]))
                .expect("response");
        });
        let result = access()
            .fetch(&target, &CancellationSignal::default())
            .await;
        worker.join().expect("server worker");
        assert!(matches!(
            result,
            Err(crate::error::AppError::DirectDownloadFailed {
                reason: crate::error::DirectDownloadFailureReason::DownloadTooLarge
            })
        ));
    }

    #[tokio::test]
    async fn applies_one_total_download_timeout() {
        let server = tiny_http::Server::http("127.0.0.1:0").expect("server");
        let target = format!(
            "http://{}/slow",
            server.server_addr().to_ip().expect("server address")
        );
        let worker = thread::spawn(move || {
            if let Some(request) = server
                .recv_timeout(Duration::from_secs(2))
                .expect("receive request")
            {
                thread::sleep(Duration::from_millis(200));
                let _ = request.respond(tiny_http::Response::from_string("late"));
            }
        });
        let download = RuntimeDownloadAccess::with_limits(
            HttpTransport::new(Arc::new(ProxySettingsStore::new(
                NetworkProxySettings::default(),
            ))),
            Duration::from_millis(50),
            1024,
        );
        let result = download
            .fetch(&target, &CancellationSignal::default())
            .await;
        worker.join().expect("server worker");
        assert!(matches!(
            result,
            Err(crate::error::AppError::DirectDownloadFailed {
                reason: crate::error::DirectDownloadFailureReason::Timeout
            })
        ));
    }
}

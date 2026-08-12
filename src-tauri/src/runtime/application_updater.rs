use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use tauri::{AppHandle, Runtime};
use tauri_plugin_updater::UpdaterExt;

use crate::application::application_update::{
    ApplicationUpdateInfo, ApplicationUpdateLimits, ApplicationUpdateProgress, ApplicationUpdater,
    UpdaterFuture,
};
use crate::error::AppError;
use crate::runtime::proxy_settings::ProxySettingsStore;

pub(crate) struct TauriApplicationUpdater<R: Runtime> {
    app: AppHandle<R>,
    settings: Arc<ProxySettingsStore>,
}

impl<R: Runtime> TauriApplicationUpdater<R> {
    pub(crate) fn new(app: AppHandle<R>, settings: Arc<ProxySettingsStore>) -> Self {
        Self { app, settings }
    }

    fn updater(
        &self,
        timeout: Duration,
    ) -> Result<(tauri_plugin_updater::Updater, &'static str), AppError> {
        let proxy_url = self.settings.proxy_url().map_err(updater_error)?;
        let builder = self.app.updater_builder().timeout(timeout);
        let builder = match &proxy_url {
            None => builder.no_proxy(),
            Some(endpoint) => builder.proxy(
                endpoint
                    .parse()
                    .map_err(|_| updater_error("invalid updater proxy"))?,
            ),
        };
        Ok((
            builder.build().map_err(updater_error)?,
            if proxy_url.is_some() {
                "proxy"
            } else {
                "direct"
            },
        ))
    }

    async fn check_update(
        &self,
        timeout: Duration,
        operation_id: &str,
    ) -> Result<Option<tauri_plugin_updater::Update>, AppError> {
        let (updater, route) = self.updater(timeout)?;
        let started_at = Instant::now();
        log::debug!(
            "Updater check started: operation_id={}, route={}",
            operation_id,
            route,
        );
        let result = updater.check().await.map_err(updater_error);
        match &result {
            Ok(_) => log::debug!(
                "Updater check finished: operation_id={}, route={}, elapsed_ms={}",
                operation_id,
                route,
                started_at.elapsed().as_millis(),
            ),
            Err(error) => log::warn!(
                "Updater check failed: operation_id={}, route={}, elapsed_ms={}, error={}",
                operation_id,
                route,
                started_at.elapsed().as_millis(),
                error,
            ),
        }
        result
    }

    async fn download_and_install_update(
        &self,
        mut update: tauri_plugin_updater::Update,
        progress: Arc<dyn Fn(ApplicationUpdateProgress) + Send + Sync>,
        download_timeout: Duration,
    ) -> Result<(), AppError> {
        update.timeout = Some(download_timeout);
        let chunk_started = Arc::new(AtomicBool::new(false));
        let chunk_started_for_progress = Arc::clone(&chunk_started);
        let chunk_progress = Arc::clone(&progress);
        let finish_progress = Arc::clone(&progress);
        let bytes = update
            .download(
                move |chunk_length, content_length| {
                    if !chunk_started_for_progress.swap(true, Ordering::AcqRel) {
                        chunk_progress(ApplicationUpdateProgress::Started { content_length });
                    }
                    chunk_progress(ApplicationUpdateProgress::Progress {
                        chunk_length: chunk_length.try_into().unwrap_or(u64::MAX),
                    });
                },
                move || finish_progress(ApplicationUpdateProgress::Downloaded),
            )
            .await
            .map_err(updater_error)?;
        progress(ApplicationUpdateProgress::Installing);
        update.install(bytes).map_err(updater_error)?;
        progress(ApplicationUpdateProgress::Finished);
        Ok(())
    }
}

impl<R: Runtime> ApplicationUpdater for TauriApplicationUpdater<R> {
    fn check<'a>(
        &'a self,
        limits: ApplicationUpdateLimits,
    ) -> UpdaterFuture<'a, Result<Option<ApplicationUpdateInfo>, AppError>> {
        Box::pin(async move {
            let operation_id = uuid::Uuid::new_v4().simple().to_string();
            Ok(self
                .check_update(limits.check_timeout, &operation_id)
                .await?
                .map(|update| ApplicationUpdateInfo {
                    version: update.version,
                    body: update.body,
                }))
        })
    }

    fn download_and_install<'a>(
        &'a self,
        expected_version: &'a str,
        progress: Arc<dyn Fn(ApplicationUpdateProgress) + Send + Sync>,
        limits: ApplicationUpdateLimits,
    ) -> UpdaterFuture<'a, Result<ApplicationUpdateInfo, AppError>> {
        Box::pin(async move {
            let operation_id = uuid::Uuid::new_v4().simple().to_string();
            let update = self
                .check_update(limits.check_timeout, &operation_id)
                .await?
                .ok_or_else(no_update)?;
            if update.version != expected_version {
                return Err(AppError::Validation {
                    field: Some("expectedVersion".to_string()),
                    message: "available application update changed".to_string(),
                });
            }
            let installed_update = ApplicationUpdateInfo {
                version: update.version.clone(),
                body: update.body.clone(),
            };
            let started_at = Instant::now();
            log::debug!("Updater download started: operation_id={}", operation_id,);
            self.download_and_install_update(update, progress, limits.download_timeout)
                .await?;
            log::debug!(
                "Updater download finished: operation_id={}, elapsed_ms={}",
                operation_id,
                started_at.elapsed().as_millis(),
            );
            Ok(installed_update)
        })
    }
}

fn no_update() -> AppError {
    AppError::Validation {
        field: Some("expectedVersion".to_string()),
        message: "no application update is currently available".to_string(),
    }
}

fn updater_error(error: impl std::fmt::Display) -> AppError {
    AppError::ExecutionFailed {
        message: error.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;
    use std::thread;

    use tauri::test::{mock_builder, MockRuntime};

    use super::*;
    use crate::models::{NetworkProxySettings, ProxyMode};

    fn test_app(manifest_urls: &[url::Url]) -> tauri::App<MockRuntime> {
        // 测试上下文不嵌入 macOS Info.plist，避免与应用入口重复定义符号。
        let mut context = tauri::generate_context!(test = true);
        let updater_config = context
            .config_mut()
            .plugins
            .0
            .get_mut("updater")
            .expect("updater plugin config");
        updater_config["endpoints"] = serde_json::json!(manifest_urls);
        updater_config["dangerousInsecureTransportProtocol"] = serde_json::json!(true);
        mock_builder()
            .plugin(tauri_plugin_updater::Builder::new().build())
            .build(context)
            .expect("mock Tauri app")
    }

    fn adapter(app: &tauri::App<MockRuntime>) -> TauriApplicationUpdater<MockRuntime> {
        adapter_with_settings(app, NetworkProxySettings::default())
    }

    fn adapter_with_settings(
        app: &tauri::App<MockRuntime>,
        settings: NetworkProxySettings,
    ) -> TauriApplicationUpdater<MockRuntime> {
        TauriApplicationUpdater::new(
            app.handle().clone(),
            Arc::new(ProxySettingsStore::new(settings)),
        )
    }

    fn test_limits() -> ApplicationUpdateLimits {
        ApplicationUpdateLimits {
            check_timeout: Duration::from_secs(2),
            download_timeout: Duration::from_secs(2),
        }
    }

    fn manifest(asset_url: &str) -> String {
        serde_json::json!({
            "version": "99.0.0",
            "notes": "test release",
            "pub_date": "2026-08-10T00:00:00Z",
            "url": asset_url,
            "signature": "aW52YWxpZA=="
        })
        .to_string()
    }

    fn local_url(server: &tiny_http::Server, path: &str) -> url::Url {
        url::Url::parse(&format!("http://{}{path}", server.server_addr()))
            .expect("local updater URL")
    }

    fn receive_request(server: &tiny_http::Server, label: &str) -> tiny_http::Request {
        server
            .recv_timeout(Duration::from_secs(2))
            .unwrap_or_else(|error| panic!("{label} receive failed: {error}"))
            .unwrap_or_else(|| panic!("{label} was not received before the deadline"))
    }

    #[tokio::test]
    async fn official_plugin_reads_a_controlled_manifest_with_direct_settings() {
        let server = tiny_http::Server::http("127.0.0.1:0").expect("manifest server");
        let manifest_url = local_url(&server, "/latest.json");
        let asset_url = local_url(&server, "/asset").to_string();
        let server_thread = thread::spawn(move || {
            let request = receive_request(&server, "manifest request");
            assert_eq!(request.url(), "/latest.json");
            request
                .respond(tiny_http::Response::from_string(manifest(&asset_url)))
                .expect("manifest response");
        });
        let app = test_app(std::slice::from_ref(&manifest_url));
        let updater = adapter(&app);

        let update = ApplicationUpdater::check(&updater, test_limits())
            .await
            .expect("official updater check")
            .expect("new release");

        server_thread.join().expect("manifest server thread");
        assert_eq!(update.version, "99.0.0");
        assert_eq!(update.body.as_deref(), Some("test release"));
    }

    #[tokio::test]
    async fn official_plugin_uses_the_current_custom_proxy_and_verifies_the_asset() {
        let proxy = tiny_http::Server::http("127.0.0.1:0").expect("proxy server");
        let proxy_url = local_url(&proxy, "").to_string();
        let (request_tx, request_rx) = std::sync::mpsc::channel();
        let proxy_thread = thread::spawn(move || {
            let request = receive_request(&proxy, "proxied manifest request");
            request_tx
                .send(request.url().to_string())
                .expect("manifest request URL");
            request
                .respond(tiny_http::Response::from_string(manifest(
                    "http://localhost:9/asset",
                )))
                .expect("proxied manifest response");
            let request = receive_request(&proxy, "proxied asset request");
            request_tx
                .send(request.url().to_string())
                .expect("asset request URL");
            request
                .respond(tiny_http::Response::from_data(b"unsigned updater"))
                .expect("proxied asset response");
        });
        let manifest_url = url::Url::parse("http://localhost:9/latest.json").expect("manifest URL");
        let app = test_app(std::slice::from_ref(&manifest_url));
        let updater = adapter_with_settings(
            &app,
            NetworkProxySettings {
                mode: ProxyMode::Custom,
                custom_proxy_url: Some(proxy_url),
                ..NetworkProxySettings::default()
            }
            .validate_and_normalize()
            .expect("proxy settings"),
        );
        let events = Arc::new(Mutex::new(Vec::new()));
        let observed = Arc::clone(&events);

        let result = ApplicationUpdater::download_and_install(
            &updater,
            "99.0.0",
            Arc::new(move |event| observed.lock().expect("progress events").push(event)),
            test_limits(),
        )
        .await;

        proxy_thread.join().expect("proxy server thread");
        let requests = [
            request_rx
                .recv_timeout(Duration::from_secs(2))
                .expect("proxied manifest URL"),
            request_rx
                .recv_timeout(Duration::from_secs(2))
                .expect("proxied asset URL"),
        ];
        assert!(requests[0].contains("localhost:9/latest.json"));
        assert!(requests[1].contains("localhost:9/asset"));
        match result {
            Err(AppError::ExecutionFailed { message }) => assert!(
                message.to_ascii_lowercase().contains("minisign"),
                "expected signature failure, got: {message}"
            ),
            other => panic!("expected updater signature failure, got: {other:?}"),
        }
        let events = events.lock().expect("progress events");
        assert!(events
            .iter()
            .any(|event| matches!(event, ApplicationUpdateProgress::Downloaded)));
        assert!(!events
            .iter()
            .any(|event| matches!(event, ApplicationUpdateProgress::Installing)));
    }
}

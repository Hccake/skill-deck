use std::collections::{BTreeMap, BTreeSet};
use std::future::Future;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::Serialize;
use specta::Type;

use crate::core::mutation::CancellationSignal;
use crate::environment::wsl::operations::source_acquisition::probe_wsl_git_connection;
use crate::environment::wsl::WslRuntime;
use crate::error::AppError;
use crate::models::NetworkProxySettings;
use crate::runtime::discovery::DISCOVERY_CONNECTION_TEST_TARGET;
use crate::runtime::git_source::ProcessGitTransport;
use crate::runtime::http_transport::{HttpGetRequest, HttpTransport, HttpTransportError};
use crate::runtime::proxy_settings::ProxySettingsStore;

const TEST_TIMEOUT: Duration = Duration::from_secs(10);
const TEST_GIT_URL: &str = "https://github.com/hccake/skill-deck.git";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub enum ProxyConnectionStatus {
    Succeeded,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct ProxyConnectionProbe {
    pub status: ProxyConnectionStatus,
    pub elapsed_ms: u64,
    pub reason_code: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct ProxyConnectionTestResult {
    pub online_services: ProxyConnectionProbe,
    pub native_git: ProxyConnectionProbe,
    pub wsl_git_by_distro: BTreeMap<String, ProxyConnectionProbe>,
}

pub(crate) struct RuntimeNetworkConnectionProbe {
    wsl: Arc<WslRuntime>,
}

impl RuntimeNetworkConnectionProbe {
    pub(crate) fn new(wsl: Arc<WslRuntime>) -> Self {
        Self { wsl }
    }
}

impl RuntimeNetworkConnectionProbe {
    pub(crate) async fn run(
        &self,
        settings: NetworkProxySettings,
        wsl_distros: Vec<String>,
    ) -> Result<ProxyConnectionTestResult, AppError> {
        let settings =
            settings
                .validate_and_normalize()
                .map_err(|error| AppError::InvalidProxySettings {
                    code: error.code().to_string(),
                })?;
        let proxy_settings = Arc::new(ProxySettingsStore::new(settings));
        let http = HttpTransport::new(proxy_settings.clone());
        let native_settings = proxy_settings.clone();
        let wsl_runtime = self.wsl.clone();
        let wsl_settings = proxy_settings.clone();
        let wsl_probes = async move {
            let mut tasks = Vec::new();
            for distro in wsl_distros.into_iter().collect::<BTreeSet<_>>() {
                let task_runtime = wsl_runtime.clone();
                let task_settings = wsl_settings.clone();
                let task_distro = distro.clone();
                tasks.push((
                    distro,
                    tokio::spawn(async move {
                        test_wsl_git(task_runtime.as_ref(), task_settings.as_ref(), task_distro)
                            .await
                    }),
                ));
            }
            let mut probes = BTreeMap::new();
            for (distro, task) in tasks {
                let probe = task
                    .await
                    .unwrap_or_else(|_| failure(Instant::now(), "git_task_failed"));
                probes.insert(distro, probe);
            }
            probes
        };
        let (online_services, native_git, wsl_git_by_distro) = tokio::join!(
            test_http_target(&http, DISCOVERY_CONNECTION_TEST_TARGET),
            async move {
                tokio::task::spawn_blocking(move || test_native_git(native_settings, TEST_GIT_URL))
                    .await
                    .unwrap_or_else(|_| failure(Instant::now(), "git_task_failed"))
            },
            wsl_probes,
        );
        Ok(ProxyConnectionTestResult {
            online_services,
            native_git,
            wsl_git_by_distro,
        })
    }
}

fn test_native_git(settings: Arc<ProxySettingsStore>, target: &str) -> ProxyConnectionProbe {
    let started_at = Instant::now();
    let transport = ProcessGitTransport::new(settings);
    let remaining = TEST_TIMEOUT.saturating_sub(started_at.elapsed());
    let result = if remaining.is_zero() {
        Err(AppError::GitTimeout {
            timeout_secs: TEST_TIMEOUT.as_secs() as u32,
        })
    } else {
        transport.probe_ref_revision_with_timeout(
            target,
            None,
            CancellationSignal::default(),
            remaining,
        )
    };
    git_probe(started_at, result)
}

async fn test_wsl_git(
    wsl: &WslRuntime,
    settings: &ProxySettingsStore,
    distro: String,
) -> ProxyConnectionProbe {
    let started_at = Instant::now();
    let deadline = started_at + TEST_TIMEOUT;
    let workspace = match wsl.workspace(&distro) {
        Ok(workspace) => workspace,
        Err(error) => return git_probe(started_at, Err(error)),
    };
    let probe = wsl.with_session(&distro, |_session| {
        let proxy = settings.wsl_git_proxy(&distro, TEST_GIT_URL);
        let workspace = workspace.clone();
        async move {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(AppError::GitTimeout {
                    timeout_secs: TEST_TIMEOUT.as_secs() as u32,
                });
            }
            probe_wsl_git_connection(
                &workspace,
                TEST_GIT_URL,
                proxy,
                remaining,
                CancellationSignal::default(),
            )
            .await
        }
    });
    finish_wsl_probe(started_at, TEST_TIMEOUT, probe).await
}

async fn finish_wsl_probe<T, F>(
    started_at: Instant,
    timeout: Duration,
    probe: F,
) -> ProxyConnectionProbe
where
    F: Future<Output = Result<T, AppError>>,
{
    let result = tokio::time::timeout(timeout, probe)
        .await
        .unwrap_or_else(|_| {
            Err(AppError::GitTimeout {
                timeout_secs: timeout.as_secs().try_into().unwrap_or(u32::MAX),
            })
        })
        .map(|_| String::new());
    git_probe(started_at, result)
}

fn git_probe(started_at: Instant, result: Result<String, AppError>) -> ProxyConnectionProbe {
    ProxyConnectionProbe {
        status: if result.is_ok() {
            ProxyConnectionStatus::Succeeded
        } else {
            ProxyConnectionStatus::Failed
        },
        elapsed_ms: elapsed_ms(started_at),
        reason_code: result.err().map(|error| match error {
            AppError::GitTimeout { .. } => "git_timeout".to_string(),
            AppError::GitNetworkError { .. } => "git_network".to_string(),
            AppError::GitAuthFailed { .. } => "git_auth".to_string(),
            AppError::EnvironmentUnavailable { .. } => "wsl_unavailable".to_string(),
            AppError::CapabilityUnavailable { capability, .. }
                if capability == "wslIntegration" =>
            {
                "wsl_unavailable".to_string()
            }
            _ => "git_failed".to_string(),
        }),
    }
}

async fn test_http_target(http: &HttpTransport, target: &str) -> ProxyConnectionProbe {
    let started_at = Instant::now();
    let response = http
        .get(HttpGetRequest::status_only(target, TEST_TIMEOUT))
        .await;
    let (status, reason_code) = match response {
        Ok(response) if response.status.is_success() => (ProxyConnectionStatus::Succeeded, None),
        Ok(_) => (
            ProxyConnectionStatus::Failed,
            Some("http_status".to_string()),
        ),
        Err(HttpTransportError::Request { stage, reason }) => (
            ProxyConnectionStatus::Failed,
            Some(http_probe_reason_code(stage, reason).to_string()),
        ),
        Err(_) => (
            ProxyConnectionStatus::Failed,
            Some("request_failed".to_string()),
        ),
    };
    ProxyConnectionProbe {
        status,
        elapsed_ms: elapsed_ms(started_at),
        reason_code,
    }
}

fn failure(started_at: Instant, reason_code: &str) -> ProxyConnectionProbe {
    ProxyConnectionProbe {
        status: ProxyConnectionStatus::Failed,
        elapsed_ms: elapsed_ms(started_at),
        reason_code: Some(reason_code.to_string()),
    }
}

fn elapsed_ms(started_at: Instant) -> u64 {
    started_at
        .elapsed()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

fn http_probe_reason_code(stage: &str, reason: &str) -> &'static str {
    match (stage, reason) {
        (_, "timeout") => "request_timeout",
        ("prepare", _) => "request_prepare_failed",
        _ => "request_failed",
    }
}

#[cfg(test)]
mod tests {
    use std::thread;

    use super::*;
    use crate::git_fixture::BareSkillRepo;
    use crate::models::{NetworkProxySettings, ProxyMode};

    #[test]
    fn native_git_adapter_probes_through_the_real_process_transport() {
        let repository = BareSkillRepo::new(&["demo"]);
        let result = test_native_git(
            Arc::new(ProxySettingsStore::new(NetworkProxySettings::default())),
            &repository.local_source(),
        );

        assert_eq!(result.status, ProxyConnectionStatus::Succeeded);
        assert_eq!(result.reason_code, None);
    }

    #[tokio::test]
    async fn http_adapter_uses_the_draft_proxy_and_returns_a_stable_reason() {
        let proxy = tiny_http::Server::http("127.0.0.1:0").expect("proxy server");
        let proxy_url = format!("http://{}", proxy.server_addr());
        let server_thread = thread::spawn(move || {
            let request = proxy
                .recv_timeout(Duration::from_secs(2))
                .expect("proxy receive")
                .expect("proxy request before deadline");
            assert!(request.url().contains("www.skills.sh"));
            request
                .respond(tiny_http::Response::empty(502))
                .expect("proxy rejection");
        });
        let policy = Arc::new(ProxySettingsStore::new(NetworkProxySettings {
            mode: ProxyMode::Custom,
            custom_proxy_url: Some(proxy_url),
            ..NetworkProxySettings::default()
        }));

        let result = test_http_target(
            &HttpTransport::new(policy),
            DISCOVERY_CONNECTION_TEST_TARGET,
        )
        .await;

        server_thread.join().expect("proxy server thread");
        assert_eq!(result.status, ProxyConnectionStatus::Failed);
        assert_eq!(result.reason_code.as_deref(), Some("request_failed"));
    }

    #[tokio::test]
    async fn wsl_adapter_maps_an_unavailable_distribution_without_running_wsl() {
        let wsl = WslRuntime::new_with_support(false, false);
        let policy = ProxySettingsStore::new(NetworkProxySettings::default());

        let result = test_wsl_git(&wsl, &policy, "Ubuntu".to_string()).await;

        assert_eq!(result.status, ProxyConnectionStatus::Failed);
        assert_eq!(result.reason_code.as_deref(), Some("wsl_unavailable"));
    }

    #[tokio::test]
    async fn wsl_probe_timeout_covers_session_acquisition() {
        let result = finish_wsl_probe(
            Instant::now(),
            Duration::from_millis(10),
            std::future::pending::<Result<(), AppError>>(),
        )
        .await;

        assert_eq!(result.status, ProxyConnectionStatus::Failed);
        assert_eq!(result.reason_code.as_deref(), Some("git_timeout"));
    }

    #[tokio::test]
    async fn runtime_probe_rejects_invalid_draft_before_network_io() {
        let probe = RuntimeNetworkConnectionProbe::new(Arc::new(WslRuntime::new_with_support(
            false, false,
        )));
        let settings = NetworkProxySettings {
            mode: ProxyMode::Custom,
            custom_proxy_url: None,
            ..NetworkProxySettings::default()
        };

        let result = probe.run(settings, Vec::new()).await;

        assert!(matches!(result, Err(AppError::InvalidProxySettings { .. })));
    }

    #[test]
    fn runtime_error_mapping_preserves_git_network_and_timeout_reasons() {
        let timeout = git_probe(
            Instant::now(),
            Err(AppError::GitTimeout { timeout_secs: 10 }),
        );
        let git_network = git_probe(
            Instant::now(),
            Err(AppError::GitNetworkError {
                message: "network unavailable".to_string(),
            }),
        );

        assert_eq!(timeout.reason_code.as_deref(), Some("git_timeout"));
        assert_eq!(git_network.reason_code.as_deref(), Some("git_network"));
    }
}

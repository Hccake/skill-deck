use tokio::time::Duration;

use crate::core::classify_git_failure;
use crate::core::mutation::CancellationSignal;
use crate::environment::wsl::{WslSession, WslWorkspace};
use crate::error::AppError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WslAcquisitionSource {
    Git {
        url: String,
        git_ref: Option<String>,
    },
    Local {
        native_path: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct WorkerSourceHandle {
    pub generation: u64,
    pub id: u64,
}

#[derive(Debug)]
pub struct WslNativeSource {
    workspace: WslWorkspace,
    handle: Option<WorkerSourceHandle>,
    native_root: String,
    managed_owner_registered: bool,
    ref_revision: Option<String>,
}

impl WslNativeSource {
    pub fn native_root(&self) -> &str {
        &self.native_root
    }

    pub fn ref_revision(&self) -> Option<&str> {
        self.ref_revision.as_deref()
    }

    pub(crate) fn handle(&self) -> WorkerSourceHandle {
        self.handle
            .expect("active WSL source must own a Worker handle")
    }
}

impl Drop for WslNativeSource {
    fn drop(&mut self) {
        if let Some(handle) = self.handle.take() {
            self.workspace.defer_worker_source_release(handle);
        }
        if self.managed_owner_registered {
            self.workspace.release_source_owner();
        }
    }
}

pub async fn acquire_wsl_source_native(
    workspace: WslWorkspace,
    _session: &WslSession,
    source: WslAcquisitionSource,
    git_timeout: Duration,
    proxy: Option<String>,
    cancellation: CancellationSignal,
) -> Result<WslNativeSource, AppError> {
    let (message, source_url) = match source {
        WslAcquisitionSource::Git { url, git_ref } => (
            environment_protocol::Message::AcquireGitSource {
                request: environment_protocol::GitSourceRequest {
                    url: url.clone(),
                    git_ref,
                    proxy,
                    deadline_millis: duration_millis(git_timeout),
                },
            },
            Some(url),
        ),
        WslAcquisitionSource::Local { native_path } => {
            if !native_path.starts_with('/') {
                return Err(AppError::UnsafePath {
                    path: native_path,
                    reason: "WSL local Source must use an absolute POSIX path".to_string(),
                });
            }
            (
                environment_protocol::Message::OpenLocalSource {
                    request: environment_protocol::OpenLocalSourceRequest { path: native_path },
                },
                None,
            )
        }
    };
    let (generation, response) = workspace
        .request_worker_control_once(
            message,
            Some(cancellation),
            git_timeout.saturating_add(Duration::from_secs(5)),
        )
        .await
        .map_err(|error| map_transport_timeout(error, git_timeout))?;
    let (id, native_root, ref_revision) = match response {
        environment_protocol::Message::SourceOpened {
            source_id,
            root,
            revision,
        } => (source_id, root, revision),
        environment_protocol::Message::Error {
            code,
            phase,
            parameters,
        } if source_url.is_some() => {
            return Err(map_git_error(
                &code,
                &phase,
                &parameters,
                source_url.as_deref().unwrap_or_default(),
                "clone",
                git_timeout,
            ));
        }
        environment_protocol::Message::Error { code, phase, .. } => {
            return Err(AppError::ExecutionFailed {
                message: format!("WSL Worker source request failed during {phase}: {code}"),
            });
        }
        _ => return Err(protocol_error("invalid WSL Worker SourceOpened response")),
    };
    workspace.register_source_owner()?;
    Ok(WslNativeSource {
        workspace,
        handle: Some(WorkerSourceHandle { generation, id }),
        native_root,
        managed_owner_registered: true,
        ref_revision,
    })
}

pub(crate) async fn probe_wsl_git_connection(
    workspace: &WslWorkspace,
    url: &str,
    proxy: Option<String>,
    timeout: Duration,
    cancellation: CancellationSignal,
) -> Result<(), AppError> {
    let (_, response) = workspace
        .request_worker_control_once(
            environment_protocol::Message::ProbeGit {
                request: environment_protocol::GitSourceRequest {
                    url: url.to_string(),
                    git_ref: None,
                    proxy,
                    deadline_millis: duration_millis(timeout),
                },
            },
            Some(cancellation),
            timeout.saturating_add(Duration::from_secs(5)),
        )
        .await
        .map_err(|error| map_transport_timeout(error, timeout))?;
    match response {
        environment_protocol::Message::GitProbed { .. } => Ok(()),
        environment_protocol::Message::Error {
            code,
            phase,
            parameters,
        } => Err(map_git_error(
            &code,
            &phase,
            &parameters,
            url,
            "ls-remote",
            timeout,
        )),
        _ => Err(protocol_error("invalid WSL Worker GitProbed response")),
    }
}

fn map_git_error(
    code: &str,
    phase: &str,
    parameters: &[(String, String)],
    url: &str,
    operation: &str,
    timeout: Duration,
) -> AppError {
    if code == "deadlineExceeded" {
        return AppError::GitTimeout {
            timeout_secs: u32::try_from(timeout.as_secs()).unwrap_or(u32::MAX),
        };
    }
    let parameter = |name: &str| {
        parameters
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    };
    if code == "gitFailed" {
        let exit_code = parameter("exitCode").and_then(|value| value.parse().ok());
        return classify_git_failure(
            parameter("stderr").unwrap_or_default(),
            url,
            operation,
            exit_code,
        );
    }
    if code == "gitUnavailable" {
        return AppError::GitCloneFailed {
            message: parameter("message")
                .unwrap_or("Git is not available in the selected WSL distribution")
                .to_string(),
        };
    }
    AppError::ExecutionFailed {
        message: format!("WSL Worker Git request failed during {phase}: {code}"),
    }
}

fn duration_millis(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis())
        .unwrap_or(u64::MAX)
        .max(1)
}

fn map_transport_timeout(error: AppError, timeout: Duration) -> AppError {
    if error == AppError::WslCommandTimedOut {
        AppError::GitTimeout {
            timeout_secs: u32::try_from(timeout.as_secs()).unwrap_or(u32::MAX),
        }
    } else {
        error
    }
}

fn protocol_error(message: &str) -> AppError {
    AppError::ConfigurationCorrupted {
        message: message.to_string(),
    }
}

#[cfg(all(test, target_os = "windows"))]
#[allow(
    clippy::disallowed_methods,
    reason = "真实 WSL 2 门禁的 Drop guard 需要同步启动 wsl.exe 清理测试 fixture"
)]
mod windows_wsl2_tests {
    use std::process::Stdio;

    use crate::application::payload_session::{PayloadSessionStorage, PayloadStorageKey};
    use crate::core::mutation::CancellationSignal;
    use crate::environment::wsl::operations::acquire::WslPayloadSessionStorage;
    use crate::environment::wsl::operations::scan::{scan, ScanRequest};
    use crate::environment::wsl::WslRuntime;

    #[tokio::test]
    #[ignore = "requires Windows with an Ubuntu WSL 2 distribution"]
    async fn real_wsl2_worker_completes_git_scan_payload_and_release() {
        let distro =
            std::env::var("SKILL_DECK_TEST_WSL_DISTRO").unwrap_or_else(|_| "Ubuntu".to_string());
        let fixture = format!(
            "/tmp/skill-deck-worker-gate-{}",
            uuid::Uuid::new_v4().simple()
        );
        run_fixture_command(
            &distro,
            r#"set -eu
root=$1
mkdir -p "$root"
git init -b main "$root"
git -C "$root" config user.email test@example.com
git -C "$root" config user.name 'Skill Deck Test'
printf '%s\n' '---' 'name: worker-gate' 'description: Worker gate' '---' > "$root/SKILL.md"
git -C "$root" add SKILL.md
git -C "$root" commit -m fixture
"#,
            &fixture,
        )
        .await;
        let _fixture_cleanup = FixtureCleanup {
            distro: distro.clone(),
            fixture: fixture.clone(),
        };

        let runtime = WslRuntime::for_wsl_test();
        let workspace = runtime.workspace(&distro).unwrap();
        let session = runtime.connect(&distro).await.unwrap();
        let source = super::acquire_wsl_source_native(
            workspace.clone(),
            &session,
            super::WslAcquisitionSource::Git {
                url: fixture.clone(),
                git_ref: None,
            },
            std::time::Duration::from_secs(30),
            None,
            CancellationSignal::default(),
        )
        .await
        .unwrap();
        let inventory = scan(
            &workspace,
            &source,
            ScanRequest {
                roots: vec![source.native_root().to_string()],
                stat_only_root_indexes: Default::default(),
                recursive: true,
                per_file_limit: 256 * 1024,
                aggregate_limit: 1024 * 1024,
            },
            None,
        )
        .await
        .unwrap();
        assert!(inventory
            .entries
            .iter()
            .any(|entry| entry.relative_path == "SKILL.md"));

        let storage = WslPayloadSessionStorage::for_source(workspace, &source);
        let key = PayloadStorageKey::new("worker-gate", "SKILL.md");
        let acquired = storage
            .acquire_from_source_path(&key, source.native_root(), None)
            .await
            .unwrap();
        assert_eq!(
            storage.verify(&key).await.unwrap().unwrap(),
            acquired.manifest
        );
        let blob_id = acquired
            .manifest
            .entries
            .iter()
            .find_map(|entry| entry.blob_id.as_deref())
            .unwrap();
        assert!(!storage
            .read_blob(&key, blob_id)
            .await
            .unwrap()
            .unwrap()
            .is_empty());
        storage.remove(&key).await.unwrap();
        drop(source);
    }

    async fn run_fixture_command(distro: &str, script: &str, fixture: &str) {
        let status = crate::environment::wsl::wsl_command()
            .args([
                "--distribution",
                distro,
                "--exec",
                "/bin/sh",
                "-c",
                script,
                "--",
                fixture,
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .status()
            .await
            .unwrap();
        assert!(status.success());
    }

    struct FixtureCleanup {
        distro: String,
        fixture: String,
    }

    impl Drop for FixtureCleanup {
        fn drop(&mut self) {
            let _ = std::process::Command::new("wsl.exe")
                .args([
                    "--distribution",
                    &self.distro,
                    "--exec",
                    "/bin/rm",
                    "-rf",
                    "--",
                    &self.fixture,
                ])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }
    }
}

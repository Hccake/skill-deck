use tokio::time::Duration;
use uuid::Uuid;

use crate::core::classify_git_failure;
use crate::core::mutation::CancellationSignal;
use crate::environment::wsl::protocol::{
    no_wsl_exit_mapping, WslOperationDescriptor, WslOperationExecutor, WslOperationRequest,
    DEFAULT_WSL_STDERR_LIMIT, DEFAULT_WSL_STDOUT_LIMIT, GIT_OUTPUT_CAPTURE,
};
use crate::environment::wsl::{WslSession, WslWorkspace};
use crate::error::AppError;

const WSL_SOURCE_ACQUISITION_SCRIPT: &str = include_str!("../scripts/source-acquisition.sh");
// 宿主监督额外覆盖发行版启动及超时后的进程清理，不改变 Git 的用户配置时限。
const WSL_GIT_TRANSPORT_GRACE: Duration = Duration::from_secs(30);
const WSL_GIT_TIMEOUT_EXIT_CODE: i32 = 72;

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WslAcquisitionPlan {
    pub script: &'static str,
    pub subcommand: &'static str,
    pub positional_args: Vec<String>,
    pub transport_timeout: Duration,
    pub git_timeout: Duration,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct WslNativeSourcePlan {
    native_root: String,
    operation: Option<WslAcquisitionPlan>,
    cleanup_root: Option<String>,
}

fn build_wsl_native_source_plan(
    session: &WslSession,
    source: WslAcquisitionSource,
    managed_repo_path: &str,
    git_timeout: Duration,
    proxy: Option<&str>,
) -> Result<WslNativeSourcePlan, AppError> {
    match source {
        WslAcquisitionSource::Git { url, git_ref } => Ok(WslNativeSourcePlan {
            native_root: managed_repo_path.to_string(),
            operation: Some(WslAcquisitionPlan {
                script: WSL_SOURCE_ACQUISITION_SCRIPT,
                subcommand: "git",
                positional_args: vec![
                    url,
                    managed_repo_path.to_string(),
                    git_ref.unwrap_or_default(),
                    session.distro_name.clone(),
                    git_timeout.as_secs().to_string(),
                    if proxy.is_some() {
                        "inject"
                    } else {
                        "preserve"
                    }
                    .to_string(),
                    proxy.unwrap_or_default().to_string(),
                ],
                transport_timeout: git_timeout.saturating_add(WSL_GIT_TRANSPORT_GRACE),
                git_timeout,
            }),
            cleanup_root: Some(managed_repo_path.to_string()),
        }),
        WslAcquisitionSource::Local { native_path } => {
            if !native_path.starts_with('/') {
                return Err(AppError::UnsafePath {
                    path: native_path,
                    reason: "WSL local Source must use an absolute POSIX path".to_string(),
                });
            }
            Ok(WslNativeSourcePlan {
                native_root: native_path,
                operation: None,
                cleanup_root: None,
            })
        }
    }
}

fn acquisition_cancelled() -> AppError {
    AppError::MutationCancelled
}

fn map_wsl_git_acquisition_error(error: AppError, source_url: &str, timeout: Duration) -> AppError {
    match error {
        AppError::WslCommandFailed {
            exit_code: Some(WSL_GIT_TIMEOUT_EXIT_CODE),
            ..
        } => AppError::GitTimeout {
            timeout_secs: u32::try_from(timeout.as_secs()).unwrap_or(u32::MAX),
        },
        AppError::WslCommandFailed { exit_code, stderr } => {
            classify_git_failure(&stderr, source_url, "clone", exit_code)
        }
        AppError::WslCommandTimedOut => AppError::GitTimeout {
            timeout_secs: u32::try_from(timeout.as_secs()).unwrap_or(u32::MAX),
        },
        other => other,
    }
}

async fn run_wsl_acquisition_plan_with<F, Fut>(
    session: WslSession,
    plan: WslAcquisitionPlan,
    cancellation: CancellationSignal,
    runner: F,
) -> Result<Vec<u8>, AppError>
where
    F: FnOnce(
        WslSession,
        &'static str,
        &'static str,
        Vec<String>,
        Duration,
        CancellationSignal,
    ) -> Fut,
    Fut: std::future::Future<Output = Result<Vec<u8>, AppError>>,
{
    if cancellation.is_cancelled() {
        return Err(acquisition_cancelled());
    }
    runner(
        session,
        plan.script,
        plan.subcommand,
        plan.positional_args,
        plan.transport_timeout,
        cancellation,
    )
    .await
}

#[derive(Debug)]
pub struct WslNativeSource {
    workspace: WslWorkspace,
    native_root: String,
    cleanup_root: Option<String>,
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
}

impl Drop for WslNativeSource {
    fn drop(&mut self) {
        let Some(native_root) = self.cleanup_root.take() else {
            return;
        };
        self.workspace.defer_source_cleanup(native_root);
        if self.managed_owner_registered {
            self.workspace.release_source_owner();
        }
    }
}

pub(crate) async fn cleanup_wsl_source(
    session: &WslSession,
    native_root: &str,
) -> Result<(), AppError> {
    let descriptor = WslOperationDescriptor {
        subcommand: "cleanup",
        script: WSL_SOURCE_ACQUISITION_SCRIPT,
        map_exit: no_wsl_exit_mapping,
    };
    WslOperationExecutor::execute(
        &descriptor,
        WslOperationRequest {
            session: session.clone(),
            args: vec![native_root.to_string()],
            stdin: Vec::new(),
            timeout: Duration::from_secs(10),
            stdout_limit: 64,
            stderr_limit: DEFAULT_WSL_STDERR_LIMIT,
            cancellation: None,
        },
    )
    .await?;
    Ok(())
}

pub async fn acquire_wsl_source_native(
    workspace: WslWorkspace,
    session: &WslSession,
    source: WslAcquisitionSource,
    git_timeout: Duration,
    proxy: Option<String>,
    cancellation: CancellationSignal,
) -> Result<WslNativeSource, AppError> {
    let managed_root = format!("/tmp/skill-deck-discovery-{}/repo", Uuid::new_v4().simple());
    let source_for_plan = source.clone();
    let initial_plan = build_wsl_native_source_plan(
        session,
        source_for_plan,
        &managed_root,
        git_timeout,
        proxy.as_deref(),
    )?;
    let ref_revision = if let Some(operation) = initial_plan.operation.clone() {
        let source_url = operation
            .positional_args
            .first()
            .cloned()
            .ok_or_else(acquisition_protocol_error)?;
        let response = run_wsl_acquisition_plan_with(
            session.clone(),
            operation,
            cancellation.clone(),
            |session, script, subcommand, positional_args, timeout, cancellation| async move {
                let descriptor = WslOperationDescriptor {
                    subcommand,
                    script,
                    map_exit: no_wsl_exit_mapping,
                };
                WslOperationExecutor::execute_with_output_capture(
                    &descriptor,
                    WslOperationRequest {
                        session,
                        args: positional_args,
                        stdin: Vec::new(),
                        timeout,
                        stdout_limit: DEFAULT_WSL_STDOUT_LIMIT,
                        stderr_limit: DEFAULT_WSL_STDERR_LIMIT,
                        cancellation: Some(cancellation),
                    },
                    GIT_OUTPUT_CAPTURE,
                )
                .await
                .map(|output| output.stdout)
            },
        )
        .await
        .map_err(|error| map_wsl_git_acquisition_error(error, &source_url, git_timeout))?;
        Some(parse_wsl_git_acquisition_response(&response)?)
    } else if cancellation.is_cancelled() {
        return Err(acquisition_cancelled());
    } else {
        None
    };
    let managed_owner_registered = initial_plan.cleanup_root.is_some();
    if managed_owner_registered {
        workspace.register_source_owner()?;
    }
    Ok(WslNativeSource {
        workspace,
        native_root: initial_plan.native_root,
        cleanup_root: initial_plan.cleanup_root,
        managed_owner_registered,
        ref_revision,
    })
}

fn parse_wsl_git_acquisition_response(bytes: &[u8]) -> Result<String, AppError> {
    let fields = bytes.split(|byte| *byte == 0).collect::<Vec<_>>();
    if fields.len() != 3 || fields[0] != b"1" || !fields[2].is_empty() {
        return Err(acquisition_protocol_error());
    }
    let revision = std::str::from_utf8(fields[1]).map_err(|_| acquisition_protocol_error())?;
    if !matches!(revision.len(), 40 | 64) || !revision.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(acquisition_protocol_error());
    }
    Ok(revision.to_ascii_lowercase())
}

fn acquisition_protocol_error() -> AppError {
    AppError::ConfigurationCorrupted {
        message: "invalid WSL source acquisition response".to_string(),
    }
}

pub(crate) async fn probe_wsl_git_connection(
    session: &WslSession,
    url: &str,
    proxy: Option<String>,
    timeout: Duration,
    cancellation: CancellationSignal,
) -> Result<(), AppError> {
    let source_url = url.to_string();
    let descriptor = WslOperationDescriptor {
        subcommand: "git-probe",
        script: WSL_SOURCE_ACQUISITION_SCRIPT,
        map_exit: no_wsl_exit_mapping,
    };
    let response = WslOperationExecutor::execute_with_output_capture(
        &descriptor,
        WslOperationRequest {
            session: session.clone(),
            args: vec![
                source_url.clone(),
                session.distro_name.clone(),
                timeout.as_secs().max(1).to_string(),
                if proxy.is_some() {
                    "inject"
                } else {
                    "preserve"
                }
                .to_string(),
                proxy.clone().unwrap_or_default(),
            ],
            stdin: Vec::new(),
            timeout,
            stdout_limit: 256,
            stderr_limit: DEFAULT_WSL_STDERR_LIMIT,
            cancellation: Some(cancellation),
        },
        GIT_OUTPUT_CAPTURE,
    )
    .await
    .map(|output| output.stdout)
    .map_err(|error| map_wsl_git_acquisition_error(error, &source_url, timeout))?;
    parse_wsl_git_acquisition_response(&response)?;
    Ok(())
}

#[cfg(test)]
#[allow(
    clippy::disallowed_methods,
    reason = "acquisition 流程测试需要直接调用真实 Git 并运行 shell 测试脚本"
)]
mod tests {
    use super::{
        build_wsl_native_source_plan, run_wsl_acquisition_plan_with, WslAcquisitionPlan,
        WslAcquisitionSource, WslNativeSource,
    };
    use crate::core::mutation::CancellationSignal;
    use crate::environment::wsl::{WslRuntime, WslSession};
    use std::collections::BTreeMap;
    #[cfg(unix)]
    use std::fs;
    #[cfg(target_os = "linux")]
    use std::os::unix::fs::PermissionsExt;
    #[cfg(unix)]
    use std::process::Command;

    #[cfg(target_os = "linux")]
    fn git(cwd: &std::path::Path, args: &[&str]) -> String {
        let output = Command::new("git")
            .current_dir(cwd)
            .args(args)
            .output()
            .expect("git command");
        assert!(
            output.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap().trim().to_string()
    }

    #[test]
    fn source_acquisition_response_requires_versioned_git_head() {
        let revision_40 = "a".repeat(40);
        let revision_64 = "b".repeat(64);
        assert_eq!(
            super::parse_wsl_git_acquisition_response(format!("1\0{revision_40}\0").as_bytes())
                .unwrap(),
            revision_40
        );
        assert_eq!(
            super::parse_wsl_git_acquisition_response(format!("1\0{revision_64}\0").as_bytes())
                .unwrap(),
            revision_64
        );
        for invalid in [
            b"2\0aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\0".as_slice(),
            b"1\0short\0".as_slice(),
            b"1\0aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\0extra\0".as_slice(),
            b"1\0aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".as_slice(),
        ] {
            assert!(super::parse_wsl_git_acquisition_response(invalid).is_err());
        }
    }

    #[test]
    fn dropping_native_source_only_defers_cleanup_to_the_runtime() {
        let runtime = WslRuntime::default();
        let workspace = runtime.workspace("Ubuntu").expect("enabled workspace");
        let source = WslNativeSource {
            workspace: workspace.clone(),
            native_root: "/tmp/skill-deck-source/repo".to_string(),
            cleanup_root: Some("/tmp/skill-deck-source".to_string()),
            managed_owner_registered: false,
            ref_revision: None,
        };

        drop(source);

        assert_eq!(workspace.deferred_source_cleanup_count(), 1);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn git_acquisition_reports_cloned_head_even_if_source_advances_after_clone() {
        let temp = tempfile::tempdir().expect("temp");
        let source = temp.path().join("source");
        fs::create_dir(&source).expect("source");
        git(&source, &["init", "-b", "main"]);
        git(&source, &["config", "user.email", "test@example.com"]);
        git(&source, &["config", "user.name", "Skill Deck Test"]);
        fs::write(source.join("SKILL.md"), b"first").expect("first");
        git(&source, &["add", "SKILL.md"]);
        git(&source, &["commit", "-m", "first"]);
        let cloned_revision = git(&source, &["rev-parse", "HEAD"]);
        let managed_root = std::path::PathBuf::from(format!(
            "/tmp/skill-deck-discovery-test-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let destination = managed_root.join("repo");

        let output = Command::new("/bin/sh")
            .arg("-c")
            .arg(super::WSL_SOURCE_ACQUISITION_SCRIPT)
            .arg("--")
            .arg("git")
            .arg(&source)
            .arg(&destination)
            .arg("")
            .arg("Ubuntu")
            .arg("30")
            .arg("preserve")
            .arg("")
            .output()
            .expect("acquisition script");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let reported = super::parse_wsl_git_acquisition_response(&output.stdout).unwrap();

        fs::write(source.join("SKILL.md"), b"second").expect("second");
        git(&source, &["add", "SKILL.md"]);
        git(&source, &["commit", "-m", "second"]);
        let advanced_revision = git(&source, &["rev-parse", "HEAD"]);

        assert_eq!(reported, cloned_revision);
        assert_ne!(reported, advanced_revision);
        fs::remove_dir_all(managed_root).expect("cleanup managed source");
    }

    fn session() -> WslSession {
        WslSession {
            distro_name: "Ubuntu-24.04".to_string(),
            user: "alice".to_string(),
            uid: 1000,
            home: "/home/alice".to_string(),
            xdg_state_home: None,
            config_home: "/home/alice/.config".to_string(),
            environment: BTreeMap::new(),
            runtime_generation: 0,
        }
    }

    fn configured_git_timeout() -> tokio::time::Duration {
        tokio::time::Duration::from_secs(300)
    }

    fn preserve_proxy() -> Option<&'static str> {
        None
    }

    fn git_operation() -> WslAcquisitionPlan {
        build_wsl_native_source_plan(
            &session(),
            WslAcquisitionSource::Git {
                url: "https://github.com/example/repo".to_string(),
                git_ref: None,
            },
            "/tmp/skill-deck-discovery-123/repo",
            configured_git_timeout(),
            preserve_proxy(),
        )
        .expect("build Git source plan")
        .operation
        .expect("Git Source requires acquisition")
    }

    #[test]
    fn wsl_git_plan_keeps_source_and_ref_as_positional_arguments() {
        let plan = build_wsl_native_source_plan(
            &session(),
            WslAcquisitionSource::Git {
                url: "$(touch /tmp/not-shell-source)".to_string(),
                git_ref: Some("feature; echo unsafe".to_string()),
            },
            "/mnt/c/Temp/sd-1/repo",
            configured_git_timeout(),
            preserve_proxy(),
        )
        .expect("build git plan");

        let operation = plan.operation.expect("Git Source requires acquisition");
        assert_eq!(
            operation.positional_args[0],
            "$(touch /tmp/not-shell-source)"
        );
        assert_eq!(operation.positional_args[1], "/mnt/c/Temp/sd-1/repo");
        assert_eq!(operation.positional_args[2], "feature; echo unsafe");
        assert!(!operation.script.contains("$(touch /tmp/not-shell-source)"));
        assert!(!operation.script.contains("feature; echo unsafe"));
    }

    #[test]
    fn wsl_git_plan_uses_the_configured_git_timeout() {
        let plan = build_wsl_native_source_plan(
            &session(),
            WslAcquisitionSource::Git {
                url: "https://github.com/example/repo".to_string(),
                git_ref: None,
            },
            "/mnt/c/Temp/sd-1/repo",
            configured_git_timeout(),
            preserve_proxy(),
        )
        .expect("build Git source plan");

        let operation = plan.operation.expect("Git Source requires acquisition");
        assert_eq!(operation.git_timeout, configured_git_timeout());
        assert_eq!(
            operation.transport_timeout,
            configured_git_timeout() + super::WSL_GIT_TRANSPORT_GRACE
        );
        assert_eq!(operation.positional_args[4], "300");
    }

    #[test]
    fn wsl_git_failures_use_shared_git_error_semantics() {
        let url = "https://alice:secret@github.com/acme/private.git?token=query-secret";
        let git_failure = super::map_wsl_git_acquisition_error(
            crate::error::AppError::WslCommandFailed {
                exit_code: Some(68),
                stderr: format!(
                    "fatal: failed to clone {url}\nAuthorization: Bearer header-secret"
                ),
            },
            url,
            configured_git_timeout(),
        );
        let rendered = git_failure.to_string();
        assert!(matches!(
            git_failure,
            crate::error::AppError::GitCloneFailed { .. }
        ));
        assert!(rendered.contains(url));
        assert!(rendered.contains("Authorization: Bearer header-secret"));

        assert!(matches!(
            super::map_wsl_git_acquisition_error(
                crate::error::AppError::WslCommandFailed {
                    exit_code: Some(68),
                    stderr: "Could not resolve host: github.com".to_string(),
                },
                "https://github.com/acme/private.git",
                configured_git_timeout(),
            ),
            crate::error::AppError::GitNetworkError { .. }
        ));
        assert!(matches!(
            super::map_wsl_git_acquisition_error(
                crate::error::AppError::WslCommandTimedOut,
                "https://github.com/acme/private.git",
                configured_git_timeout(),
            ),
            crate::error::AppError::GitTimeout { timeout_secs: 300 }
        ));
        assert!(matches!(
            super::map_wsl_git_acquisition_error(
                crate::error::AppError::WslCommandFailed {
                    exit_code: Some(72),
                    stderr: String::new(),
                },
                "https://github.com/acme/private.git",
                configured_git_timeout(),
            ),
            crate::error::AppError::GitTimeout { timeout_secs: 300 }
        ));
    }

    #[test]
    fn wsl_git_proxy_failures_use_the_shared_network_error() {
        for stderr in [
            "Could not resolve proxy: proxy.example",
            "Failed to connect to 127.0.0.1 port 7890: Couldn't connect to server",
        ] {
            assert!(matches!(
                super::map_wsl_git_acquisition_error(
                    crate::error::AppError::WslCommandFailed {
                        exit_code: Some(68),
                        stderr: stderr.to_string(),
                    },
                    "https://github.com/acme/private.git",
                    configured_git_timeout(),
                ),
                crate::error::AppError::GitNetworkError { .. }
            ));
        }
    }

    #[test]
    fn wsl_local_source_is_read_directly_without_a_managed_copy() {
        let plan = build_wsl_native_source_plan(
            &session(),
            WslAcquisitionSource::Local {
                native_path: "/home/alice/code/skills".to_string(),
            },
            "/tmp/skill-deck-discovery-123/repo",
            configured_git_timeout(),
            preserve_proxy(),
        )
        .expect("build local plan");

        assert_eq!(plan.native_root, "/home/alice/code/skills");
        assert!(plan.operation.is_none());
        assert!(plan.cleanup_root.is_none());
    }

    #[test]
    fn wsl_local_source_requires_an_absolute_posix_path() {
        let error = build_wsl_native_source_plan(
            &session(),
            WslAcquisitionSource::Local {
                native_path: "relative/skills".to_string(),
            },
            "/tmp/skill-deck-discovery-123/repo",
            configured_git_timeout(),
            preserve_proxy(),
        )
        .expect_err("relative WSL Source must be rejected");

        assert!(matches!(error, crate::error::AppError::UnsafePath { .. }));
    }

    #[test]
    fn wsl_git_plan_injects_proxy_only_for_the_current_command() {
        let plan = build_wsl_native_source_plan(
            &session(),
            WslAcquisitionSource::Git {
                url: "https://github.com/example/repo".to_string(),
                git_ref: None,
            },
            "/tmp/skill-deck-discovery-123/repo",
            configured_git_timeout(),
            Some("http://127.0.0.1:7890"),
        )
        .expect("build proxied Git source plan");

        let operation = plan.operation.expect("Git Source requires acquisition");
        assert_eq!(operation.positional_args[5], "inject");
        assert_eq!(operation.positional_args[6], "http://127.0.0.1:7890");
        assert!(operation
            .script
            .contains("git -c \"http.proxy=$proxy_url\""));
    }

    #[cfg(unix)]
    #[test]
    fn git_acquisition_rejects_an_unmanaged_destination_before_deleting_it() {
        let temp = tempfile::tempdir().expect("temp");
        let destination = temp.path().join("existing/repo");
        fs::create_dir_all(&destination).expect("destination");
        fs::write(destination.join("keep"), b"keep").expect("marker");

        let output = Command::new("/bin/sh")
            .arg("-c")
            .arg(super::WSL_SOURCE_ACQUISITION_SCRIPT)
            .arg("--")
            .arg("git")
            .arg("/missing/source")
            .arg(&destination)
            .arg("")
            .arg("Ubuntu")
            .arg("30")
            .output()
            .expect("acquisition script");

        assert!(!output.status.success());
        assert!(destination.join("keep").is_file());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn failed_git_acquisition_removes_its_managed_temporary_root() {
        let managed_root = std::path::PathBuf::from(format!(
            "/tmp/skill-deck-discovery-test-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let destination = managed_root.join("repo");

        let output = Command::new("/bin/sh")
            .arg("-c")
            .arg(super::WSL_SOURCE_ACQUISITION_SCRIPT)
            .arg("--")
            .arg("git")
            .arg("/missing/source")
            .arg(&destination)
            .arg("")
            .arg("Ubuntu")
            .arg("30")
            .output()
            .expect("acquisition script");

        assert!(!output.status.success());
        assert!(!managed_root.exists());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn git_timeout_starts_when_the_clone_process_starts() {
        let temp = tempfile::tempdir().expect("fake Git temp dir");
        let fake_git = temp.path().join("git");
        fs::write(&fake_git, "#!/bin/sh\nsleep 10\n").expect("fake Git");
        let mut permissions = fs::metadata(&fake_git)
            .expect("fake Git metadata")
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&fake_git, permissions).expect("make fake Git executable");
        let managed_root = std::path::PathBuf::from(format!(
            "/tmp/skill-deck-discovery-test-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let destination = managed_root.join("repo");
        let path = format!(
            "{}:{}",
            temp.path().display(),
            std::env::var("PATH").unwrap()
        );
        let started = std::time::Instant::now();

        let output = Command::new("/bin/sh")
            .arg("-c")
            .arg(super::WSL_SOURCE_ACQUISITION_SCRIPT)
            .arg("--")
            .arg("git")
            .arg("https://github.com/example/repo")
            .arg(&destination)
            .arg("")
            .arg("Ubuntu")
            .arg("1")
            .arg("preserve")
            .arg("")
            .env("PATH", path)
            .output()
            .expect("acquisition script");

        assert_eq!(output.status.code(), Some(72));
        assert!(started.elapsed() < std::time::Duration::from_secs(4));
        assert!(!managed_root.exists());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn git_probe_uses_a_stable_diagnostic_locale() {
        let temp = tempfile::tempdir().expect("fake Git temp dir");
        let fake_git = temp.path().join("git");
        fs::write(
            &fake_git,
            "#!/bin/sh\n[ \"${LC_ALL-}\" = C ] || exit 73\n[ \"${GIT_ALLOW_PROTOCOL-}\" = https:http:ssh:git:file ] || exit 74\nprintf 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\\tHEAD\\n'\n",
        )
        .expect("fake Git");
        let mut permissions = fs::metadata(&fake_git)
            .expect("fake Git metadata")
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&fake_git, permissions).expect("make fake Git executable");
        let path = format!(
            "{}:{}",
            temp.path().display(),
            std::env::var("PATH").expect("PATH")
        );

        let output = Command::new("/bin/sh")
            .arg("-c")
            .arg(super::WSL_SOURCE_ACQUISITION_SCRIPT)
            .arg("--")
            .arg("git-probe")
            .arg("https://github.com/example/repo")
            .arg("Ubuntu")
            .arg("3")
            .arg("preserve")
            .arg("")
            .env("PATH", path)
            .output()
            .expect("acquisition script");

        assert!(
            output.status.success(),
            "git probe failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            output.stdout,
            b"1\0aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\0"
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn git_acquisition_reports_missing_git_before_creating_a_managed_root() {
        let empty_path = tempfile::tempdir().expect("empty PATH");
        let managed_root = std::path::PathBuf::from(format!(
            "/tmp/skill-deck-discovery-test-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let destination = managed_root.join("repo");

        let output = Command::new("/bin/sh")
            .arg("-c")
            .arg(super::WSL_SOURCE_ACQUISITION_SCRIPT)
            .arg("--")
            .arg("git")
            .arg("https://github.com/example/repo.git")
            .arg(&destination)
            .arg("")
            .arg("Ubuntu-24.04")
            .arg("5")
            .arg("preserve")
            .arg("")
            .env("PATH", empty_path.path())
            .output()
            .expect("acquisition script");

        assert_eq!(output.status.code(), Some(127));
        assert!(String::from_utf8_lossy(&output.stderr).contains("install Git"));
        assert!(!managed_root.exists());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn git_acquisition_blocks_ext_protocol_before_running_its_helper() {
        let temp = tempfile::tempdir().expect("temp");
        let marker = temp.path().join("ext-helper-ran");
        let managed_root = std::path::PathBuf::from(format!(
            "/tmp/skill-deck-discovery-test-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let destination = managed_root.join("repo");

        let output = Command::new("/bin/sh")
            .arg("-c")
            .arg(super::WSL_SOURCE_ACQUISITION_SCRIPT)
            .arg("--")
            .arg("git")
            .arg(format!("ext::touch {}", marker.display()))
            .arg(&destination)
            .arg("")
            .arg("Ubuntu")
            .arg("5")
            .arg("preserve")
            .arg("")
            .output()
            .expect("acquisition script");

        assert_eq!(output.status.code(), Some(68));
        assert!(!marker.exists());
        assert!(!managed_root.exists());
    }

    #[tokio::test]
    async fn cancelled_acquisition_does_not_start_wsl_command() {
        let plan = git_operation();
        let cancellation = CancellationSignal::default();
        cancellation.cancel();
        let ran = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let ran_by_command = ran.clone();

        let error = run_wsl_acquisition_plan_with(
            session(),
            plan,
            cancellation,
            move |_, _, _, _, _, _| async move {
                ran_by_command.store(true, std::sync::atomic::Ordering::Release);
                Ok(Vec::new())
            },
        )
        .await
        .expect_err("cancelled acquisition must fail");

        assert!(error.to_string().contains("cancelled"));
        assert!(!ran.load(std::sync::atomic::Ordering::Acquire));
    }

    #[tokio::test]
    async fn cancellation_stops_waiting_for_running_wsl_command() {
        let plan = git_operation();
        let cancellation = CancellationSignal::default();
        let cancellation_request = cancellation.clone();
        let run = run_wsl_acquisition_plan_with(
            session(),
            plan,
            cancellation,
            |_, _, _, _, _, cancellation| async move {
                while !cancellation.is_cancelled() {
                    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
                }
                Err(crate::error::AppError::MutationCancelled)
            },
        );
        let cancel = async move {
            tokio::time::sleep(std::time::Duration::from_millis(30)).await;
            cancellation_request.cancel();
        };

        let (result, ()) = tokio::join!(run, cancel);

        assert!(result
            .expect_err("running acquisition must observe cancellation")
            .to_string()
            .contains("cancelled"));
    }

    #[tokio::test]
    async fn cancellation_waits_for_runner_cleanup_before_returning() {
        let plan = git_operation();
        let cancellation = CancellationSignal::default();
        let cancellation_request = cancellation.clone();
        let cleanup_finished = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let cleanup_from_runner = cleanup_finished.clone();
        let run = run_wsl_acquisition_plan_with(
            session(),
            plan,
            cancellation,
            move |_, _, _, _, _, cancellation| async move {
                while !cancellation.is_cancelled() {
                    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
                }
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                cleanup_from_runner.store(true, std::sync::atomic::Ordering::Release);
                Err(crate::error::AppError::MutationCancelled)
            },
        );
        let cancel = async move {
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            cancellation_request.cancel();
        };

        let (result, ()) = tokio::join!(run, cancel);

        assert_eq!(result, Err(crate::error::AppError::MutationCancelled));
        assert!(cleanup_finished.load(std::sync::atomic::Ordering::Acquire));
    }
}

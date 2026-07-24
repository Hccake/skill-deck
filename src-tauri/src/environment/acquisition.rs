use tokio::time::Duration;
use uuid::Uuid;

use crate::core::mutation::CancellationSignal;
use crate::environment::wsl::WslSession;
use crate::environment::wsl_protocol::{
    no_wsl_exit_mapping, WslOperationDescriptor, WslOperationExecutor, WslOperationRequest,
    DEFAULT_WSL_STDERR_LIMIT, DEFAULT_WSL_STDOUT_LIMIT,
};
use crate::error::AppError;

const WSL_SOURCE_ACQUISITION_SCRIPT: &str = include_str!("wsl/scripts/source-acquisition.sh");

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
    pub timeout: Duration,
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
                ],
                timeout: Duration::from_secs(120),
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
        plan.timeout,
        cancellation,
    )
    .await
}

#[derive(Debug)]
pub struct WslNativeSource {
    session: WslSession,
    native_root: String,
    cleanup_root: Option<String>,
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
        let session = self.session.clone();
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.spawn(async move {
                let descriptor = WslOperationDescriptor {
                    subcommand: "cleanup",
                    script: WSL_SOURCE_ACQUISITION_SCRIPT,
                    required_features: &[],
                    map_exit: no_wsl_exit_mapping,
                };
                let _ = WslOperationExecutor::execute(
                    &descriptor,
                    WslOperationRequest {
                        session,
                        args: vec![native_root],
                        stdin: Vec::new(),
                        timeout: Duration::from_secs(10),
                        stdout_limit: 64,
                        stderr_limit: DEFAULT_WSL_STDERR_LIMIT,
                        cancellation: None,
                    },
                )
                .await;
            });
        }
    }
}

pub async fn acquire_wsl_source_native(
    session: &WslSession,
    source: WslAcquisitionSource,
    cancellation: CancellationSignal,
) -> Result<WslNativeSource, AppError> {
    let managed_root = format!("/tmp/skill-deck-discovery-{}/repo", Uuid::new_v4().simple());
    let plan = build_wsl_native_source_plan(session, source, &managed_root)?;
    let ref_revision = if let Some(operation) = plan.operation {
        let response = run_wsl_acquisition_plan_with(
            session.clone(),
            operation,
            cancellation,
            |session, script, subcommand, positional_args, timeout, cancellation| async move {
                let descriptor = WslOperationDescriptor {
                    subcommand,
                    script,
                    required_features: &[],
                    map_exit: no_wsl_exit_mapping,
                };
                WslOperationExecutor::execute(
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
                )
                .await
                .map(|output| output.stdout)
            },
        )
        .await?;
        Some(parse_wsl_git_acquisition_response(&response)?)
    } else if cancellation.is_cancelled() {
        return Err(acquisition_cancelled());
    } else {
        None
    };
    Ok(WslNativeSource {
        session: session.clone(),
        native_root: plan.native_root,
        cleanup_root: plan.cleanup_root,
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

#[cfg(test)]
#[allow(
    clippy::disallowed_methods,
    reason = "acquisition 测试夹具需要直接启动真实 Git 和 shell"
)]
mod tests {
    use std::collections::BTreeMap;
    #[cfg(unix)]
    use std::fs;
    #[cfg(unix)]
    use std::process::Command;

    use super::{
        build_wsl_native_source_plan, run_wsl_acquisition_plan_with, WslAcquisitionPlan,
        WslAcquisitionSource,
    };
    use crate::core::mutation::CancellationSignal;
    use crate::environment::wsl::WslSession;

    #[cfg(unix)]
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

    #[cfg(unix)]
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

    fn session(git_available: bool) -> WslSession {
        WslSession {
            distro_name: "Ubuntu-24.04".to_string(),
            user: "alice".to_string(),
            uid: 1000,
            home: "/home/alice".to_string(),
            xdg_state_home: None,
            config_home: "/home/alice/.config".to_string(),
            environment: BTreeMap::new(),
            git_available,
            execution_profile: crate::environment::wsl_protocol::WslExecutionProfile::all_supported(
            ),
            runtime_generation: 0,
        }
    }

    fn git_operation(git_available: bool) -> WslAcquisitionPlan {
        build_wsl_native_source_plan(
            &session(git_available),
            WslAcquisitionSource::Git {
                url: "https://github.com/example/repo".to_string(),
                git_ref: None,
            },
            "/tmp/skill-deck-discovery-123/repo",
        )
        .expect("build Git source plan")
        .operation
        .expect("Git Source requires acquisition")
    }

    #[test]
    fn wsl_git_plan_keeps_source_and_ref_as_positional_arguments() {
        let plan = build_wsl_native_source_plan(
            &session(true),
            WslAcquisitionSource::Git {
                url: "$(touch /tmp/not-shell-source)".to_string(),
                git_ref: Some("feature; echo unsafe".to_string()),
            },
            "/mnt/c/Temp/sd-1/repo",
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
    fn wsl_git_plan_rechecks_git_at_operation_time_instead_of_using_cached_flag() {
        let plan = build_wsl_native_source_plan(
            &session(false),
            WslAcquisitionSource::Git {
                url: "https://github.com/example/repo".to_string(),
                git_ref: None,
            },
            "/mnt/c/Temp/sd-1/repo",
        )
        .expect("build plan despite stale cached capability");

        let operation = plan.operation.expect("Git Source requires acquisition");
        assert!(operation.script.contains("command -v git"));
        assert!(operation.script.contains("install Git"));
        assert_eq!(operation.positional_args[3], "Ubuntu-24.04");
    }

    #[test]
    fn wsl_local_source_is_read_directly_without_a_managed_copy() {
        let plan = build_wsl_native_source_plan(
            &session(true),
            WslAcquisitionSource::Local {
                native_path: "/home/alice/code/skills".to_string(),
            },
            "/tmp/skill-deck-discovery-123/repo",
        )
        .expect("build local plan");

        assert_eq!(plan.native_root, "/home/alice/code/skills");
        assert!(plan.operation.is_none());
        assert!(plan.cleanup_root.is_none());
    }

    #[test]
    fn wsl_local_source_requires_an_absolute_posix_path() {
        let error = build_wsl_native_source_plan(
            &session(true),
            WslAcquisitionSource::Local {
                native_path: "relative/skills".to_string(),
            },
            "/tmp/skill-deck-discovery-123/repo",
        )
        .expect_err("relative WSL Source must be rejected");

        assert!(matches!(error, crate::error::AppError::UnsafePath { .. }));
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
            .output()
            .expect("acquisition script");

        assert!(!output.status.success());
        assert!(destination.join("keep").is_file());
    }

    #[cfg(unix)]
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
            .output()
            .expect("acquisition script");

        assert!(!output.status.success());
        assert!(!managed_root.exists());
    }

    #[tokio::test]
    async fn cancelled_acquisition_does_not_start_wsl_command() {
        let plan = git_operation(true);
        let cancellation = CancellationSignal::default();
        cancellation.cancel();
        let ran = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let ran_by_command = ran.clone();

        let error = run_wsl_acquisition_plan_with(
            session(true),
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
        let plan = git_operation(true);
        let cancellation = CancellationSignal::default();
        let cancellation_request = cancellation.clone();
        let run = run_wsl_acquisition_plan_with(
            session(true),
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
        let plan = git_operation(true);
        let cancellation = CancellationSignal::default();
        let cancellation_request = cancellation.clone();
        let cleanup_finished = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let cleanup_from_runner = cleanup_finished.clone();
        let run = run_wsl_acquisition_plan_with(
            session(true),
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

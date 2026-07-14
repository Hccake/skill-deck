use std::path::{Path, PathBuf};

use tempfile::TempDir;
use tokio::time::Duration;

use crate::core::mutation::CancellationSignal;
#[cfg(test)]
use crate::environment::path_mapping::host_path_to_linux_path;
use crate::environment::path_mapping::map_windows_path_with_wslpath;
use crate::environment::wsl::WslSession;
use crate::environment::wsl_protocol::{
    WslCommandRequest, WslCommandRunner, DEFAULT_WSL_STDERR_LIMIT, DEFAULT_WSL_STDOUT_LIMIT,
};
use crate::error::AppError;

const WSL_GIT_CLONE_SCRIPT: &str = r#"
url=$1
dest=$2
git_ref=$3
distro=$4
command -v git >/dev/null 2>&1 || {
  printf "Git is not available in WSL distro '%s'. Please install Git in that distro and try again.\n" "$distro" >&2
  exit 127
}
parent=${dest%/*}
rm -rf -- "$dest"
mkdir -p -- "$parent"
if [ -n "$git_ref" ]; then
  git clone --depth 1 --progress --branch "$git_ref" -- "$url" "$dest"
else
  git clone --depth 1 --progress -- "$url" "$dest"
fi
"#;

const WSL_LOCAL_COPY_SCRIPT: &str = r#"
src=$1
dest=$2
[ -d "$src" ] || { printf 'Local source directory not found: %s\n' "$src" >&2; exit 2; }
parent=${dest%/*}
rm -rf -- "$dest"
mkdir -p -- "$parent" "$dest"
cp -RL -- "$src"/. "$dest"/
"#;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostStagingLayout {
    pub host_repo_path: PathBuf,
    pub linux_repo_path: String,
}

#[derive(Debug)]
pub struct HostStagingDir {
    _temp_dir: TempDir,
    layout: HostStagingLayout,
}

impl HostStagingDir {
    pub async fn new(session: &WslSession) -> Result<Self, AppError> {
        let temp_dir = tempfile::Builder::new()
            .prefix("skill-deck-")
            .tempdir()
            .map_err(|error| AppError::Io {
                message: format!("failed to create Host staging directory: {error}"),
            })?;
        let linux_root =
            map_windows_path_with_wslpath(session, &temp_dir.path().to_string_lossy()).await?;
        let layout = HostStagingLayout {
            host_repo_path: temp_dir.path().join("repo"),
            linux_repo_path: format!("{}/repo", linux_root.trim_end_matches('/')),
        };
        Ok(Self {
            _temp_dir: temp_dir,
            layout,
        })
    }

    #[cfg(test)]
    pub fn layout_for_host_root(
        host_root: impl AsRef<Path>,
    ) -> Result<HostStagingLayout, AppError> {
        let host_root = host_root.as_ref();
        let linux_root =
            host_path_to_linux_path(&host_root.to_string_lossy()).ok_or_else(|| {
                AppError::Path {
                    message: format!(
                        "Host staging directory is not available through standard WSL DrvFS: {}",
                        host_root.display()
                    ),
                }
            })?;
        Ok(HostStagingLayout {
            host_repo_path: host_root.join("repo"),
            linux_repo_path: format!("{}/repo", linux_root.trim_end_matches('/')),
        })
    }

    pub fn host_repo_path(&self) -> &Path {
        &self.layout.host_repo_path
    }

    pub fn linux_repo_path(&self) -> &str {
        &self.layout.linux_repo_path
    }

    #[cfg(test)]
    fn from_temp_dir_for_test(temp_dir: TempDir, linux_root: &str) -> Self {
        let layout = HostStagingLayout {
            host_repo_path: temp_dir.path().join("repo"),
            linux_repo_path: format!("{}/repo", linux_root.trim_end_matches('/')),
        };
        Self {
            _temp_dir: temp_dir,
            layout,
        }
    }
}

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
    pub positional_args: Vec<String>,
    pub timeout: Duration,
}

pub fn build_wsl_acquisition_plan(
    session: &WslSession,
    source: WslAcquisitionSource,
    linux_repo_path: &str,
) -> Result<WslAcquisitionPlan, AppError> {
    match source {
        WslAcquisitionSource::Git { url, git_ref } => Ok(WslAcquisitionPlan {
            script: WSL_GIT_CLONE_SCRIPT,
            positional_args: vec![
                url,
                linux_repo_path.to_string(),
                git_ref.unwrap_or_default(),
                session.distro_name.clone(),
            ],
            timeout: Duration::from_secs(120),
        }),
        WslAcquisitionSource::Local { native_path } => Ok(WslAcquisitionPlan {
            script: WSL_LOCAL_COPY_SCRIPT,
            positional_args: vec![native_path, linux_repo_path.to_string()],
            timeout: Duration::from_secs(120),
        }),
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
) -> Result<(), AppError>
where
    F: FnOnce(WslSession, &'static str, Vec<String>, Duration, CancellationSignal) -> Fut,
    Fut: std::future::Future<Output = Result<(), AppError>>,
{
    if cancellation.is_cancelled() {
        return Err(acquisition_cancelled());
    }
    runner(
        session,
        plan.script,
        plan.positional_args,
        plan.timeout,
        cancellation,
    )
    .await
}

#[derive(Debug)]
pub struct StagedWslSource {
    staging: HostStagingDir,
}

impl StagedWslSource {
    pub fn host_repo_path(&self) -> &Path {
        self.staging.host_repo_path()
    }

    pub fn linux_repo_path(&self) -> &str {
        self.staging.linux_repo_path()
    }

    pub fn linux_path_for_host_path(&self, host_path: &Path) -> Result<String, AppError> {
        let relative =
            host_path
                .strip_prefix(self.host_repo_path())
                .map_err(|_| AppError::Path {
                    message: format!(
                        "discovered path is outside the staged source: {}",
                        host_path.display()
                    ),
                })?;
        let mut segments = Vec::new();
        for component in relative.components() {
            match component {
                std::path::Component::Normal(segment) => {
                    segments.push(segment.to_string_lossy().to_string())
                }
                std::path::Component::CurDir => {}
                _ => {
                    return Err(AppError::Path {
                        message: format!(
                            "discovered path escapes the staged source: {}",
                            host_path.display()
                        ),
                    })
                }
            }
        }
        if segments.is_empty() {
            Ok(self.linux_repo_path().to_string())
        } else {
            Ok(format!(
                "{}/{}",
                self.linux_repo_path().trim_end_matches('/'),
                segments.join("/")
            ))
        }
    }
}

async fn stage_wsl_source_with_staging<F, Fut>(
    session: WslSession,
    source: WslAcquisitionSource,
    cancellation: CancellationSignal,
    staging: HostStagingDir,
    runner: F,
) -> Result<StagedWslSource, AppError>
where
    F: FnOnce(WslSession, &'static str, Vec<String>, Duration, CancellationSignal) -> Fut,
    Fut: std::future::Future<Output = Result<(), AppError>>,
{
    let plan = build_wsl_acquisition_plan(&session, source, staging.linux_repo_path())?;
    run_wsl_acquisition_plan_with(session, plan, cancellation, runner).await?;
    if !staging.host_repo_path().is_dir() {
        return Err(AppError::InstallFailed {
            message: format!(
                "WSL source acquisition did not create the Host staging directory: {}",
                staging.host_repo_path().display()
            ),
        });
    }
    Ok(StagedWslSource { staging })
}

pub async fn stage_wsl_source(
    session: &WslSession,
    source: WslAcquisitionSource,
    cancellation: CancellationSignal,
) -> Result<StagedWslSource, AppError> {
    let staging = HostStagingDir::new(session).await?;
    stage_wsl_source_with_staging(
        session.clone(),
        source,
        cancellation,
        staging,
        |session, script, positional_args, timeout, cancellation| async move {
            WslCommandRunner::run(WslCommandRequest {
                session,
                script,
                args: positional_args,
                stdin: Vec::new(),
                timeout,
                stdout_limit: DEFAULT_WSL_STDOUT_LIMIT,
                stderr_limit: DEFAULT_WSL_STDERR_LIMIT,
                cancellation: Some(cancellation),
            })
            .await?;
            Ok(())
        },
    )
    .await
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::fs;

    use tempfile::tempdir;

    use super::{
        build_wsl_acquisition_plan, run_wsl_acquisition_plan_with, stage_wsl_source_with_staging,
        HostStagingDir, WslAcquisitionSource,
    };
    use crate::core::mutation::CancellationSignal;
    use crate::environment::wsl::WslSession;

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
        }
    }

    #[test]
    fn host_staging_layout_maps_repo_into_drvfs() {
        let layout =
            HostStagingDir::layout_for_host_root(r"C:\Users\alice\AppData\Local\Temp\sd-1")
                .expect("map staging layout");

        assert_eq!(
            layout.linux_repo_path,
            "/mnt/c/Users/alice/AppData/Local/Temp/sd-1/repo"
        );
    }

    #[test]
    fn host_staging_dir_removes_temp_tree_when_dropped() {
        let temp = tempdir().expect("tempdir");
        let root = temp.path().to_path_buf();
        fs::write(root.join("marker"), "staged").expect("write marker");
        let staging = HostStagingDir::from_temp_dir_for_test(temp, "/mnt/c/Temp/sd-1");

        drop(staging);

        assert!(!root.exists());
    }

    #[test]
    fn wsl_git_plan_keeps_source_and_ref_as_positional_arguments() {
        let plan = build_wsl_acquisition_plan(
            &session(true),
            WslAcquisitionSource::Git {
                url: "$(touch /tmp/not-shell-source)".to_string(),
                git_ref: Some("feature; echo unsafe".to_string()),
            },
            "/mnt/c/Temp/sd-1/repo",
        )
        .expect("build git plan");

        assert_eq!(plan.positional_args[0], "$(touch /tmp/not-shell-source)");
        assert_eq!(plan.positional_args[1], "/mnt/c/Temp/sd-1/repo");
        assert_eq!(plan.positional_args[2], "feature; echo unsafe");
        assert!(!plan.script.contains("$(touch /tmp/not-shell-source)"));
        assert!(!plan.script.contains("feature; echo unsafe"));
    }

    #[test]
    fn wsl_git_plan_rechecks_git_at_operation_time_instead_of_using_cached_flag() {
        let plan = build_wsl_acquisition_plan(
            &session(false),
            WslAcquisitionSource::Git {
                url: "https://github.com/example/repo".to_string(),
                git_ref: None,
            },
            "/mnt/c/Temp/sd-1/repo",
        )
        .expect("build plan despite stale cached capability");

        assert!(plan.script.contains("command -v git"));
        assert!(plan.script.contains("install Git"));
        assert_eq!(plan.positional_args[3], "Ubuntu-24.04");
    }

    #[test]
    fn wsl_local_plan_copies_native_source_into_host_staging() {
        let plan = build_wsl_acquisition_plan(
            &session(true),
            WslAcquisitionSource::Local {
                native_path: "/home/alice/code/skills".to_string(),
            },
            "/mnt/c/Temp/sd-1/repo",
        )
        .expect("build local plan");

        assert_eq!(
            plan.positional_args,
            ["/home/alice/code/skills", "/mnt/c/Temp/sd-1/repo"]
        );
        assert!(plan.script.contains("cp"));
        assert!(plan.script.contains("cp -RL"));
    }

    #[tokio::test]
    async fn cancelled_acquisition_does_not_start_wsl_command() {
        let plan = build_wsl_acquisition_plan(
            &session(true),
            WslAcquisitionSource::Local {
                native_path: "/home/alice/code/skills".to_string(),
            },
            "/mnt/c/Temp/sd-1/repo",
        )
        .expect("build plan");
        let cancellation = CancellationSignal::default();
        cancellation.cancel();
        let ran = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let ran_by_command = ran.clone();

        let error = run_wsl_acquisition_plan_with(
            session(true),
            plan,
            cancellation,
            move |_, _, _, _, _| async move {
                ran_by_command.store(true, std::sync::atomic::Ordering::Release);
                Ok(())
            },
        )
        .await
        .expect_err("cancelled acquisition must fail");

        assert!(error.to_string().contains("cancelled"));
        assert!(!ran.load(std::sync::atomic::Ordering::Acquire));
    }

    #[tokio::test]
    async fn cancellation_stops_waiting_for_running_wsl_command() {
        let plan = build_wsl_acquisition_plan(
            &session(true),
            WslAcquisitionSource::Local {
                native_path: "/home/alice/code/skills".to_string(),
            },
            "/mnt/c/Temp/sd-1/repo",
        )
        .expect("build plan");
        let cancellation = CancellationSignal::default();
        let cancellation_request = cancellation.clone();
        let run = run_wsl_acquisition_plan_with(
            session(true),
            plan,
            cancellation,
            |_, _, _, _, cancellation| async move {
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
        let plan = build_wsl_acquisition_plan(
            &session(true),
            WslAcquisitionSource::Local {
                native_path: "/home/alice/code/skills".to_string(),
            },
            "/mnt/c/Temp/sd-1/repo",
        )
        .expect("build plan");
        let cancellation = CancellationSignal::default();
        let cancellation_request = cancellation.clone();
        let cleanup_finished = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let cleanup_from_runner = cleanup_finished.clone();
        let run = run_wsl_acquisition_plan_with(
            session(true),
            plan,
            cancellation,
            move |_, _, _, _, cancellation| async move {
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

    #[tokio::test]
    async fn staged_source_keeps_host_repo_alive_until_result_is_dropped() {
        let temp = tempdir().expect("tempdir");
        let root = temp.path().to_path_buf();
        let staging = HostStagingDir::from_temp_dir_for_test(temp, "/mnt/c/Temp/sd-1");
        let host_repo = staging.host_repo_path().to_path_buf();
        let repo_for_command = host_repo.clone();

        let staged = stage_wsl_source_with_staging(
            session(true),
            WslAcquisitionSource::Local {
                native_path: "/home/alice/code/skills".to_string(),
            },
            CancellationSignal::default(),
            staging,
            move |_, _, _, _, _| async move {
                fs::create_dir_all(&repo_for_command).expect("create staged repo");
                Ok(())
            },
        )
        .await
        .expect("stage source");

        assert_eq!(staged.host_repo_path(), host_repo.as_path());
        assert!(host_repo.is_dir());
        drop(staged);
        assert!(!root.exists());
    }

    #[tokio::test]
    async fn staging_fails_when_wsl_command_does_not_materialize_repo() {
        let temp = tempdir().expect("tempdir");
        let staging = HostStagingDir::from_temp_dir_for_test(temp, "/mnt/c/Temp/sd-1");

        let error = stage_wsl_source_with_staging(
            session(true),
            WslAcquisitionSource::Local {
                native_path: "/home/alice/code/skills".to_string(),
            },
            CancellationSignal::default(),
            staging,
            |_, _, _, _, _| async move { Ok(()) },
        )
        .await
        .expect_err("missing staged repo must fail");

        assert!(error.to_string().contains("did not create"));
    }

    #[tokio::test]
    async fn staged_source_maps_discovered_host_path_back_into_wsl_staging() {
        let temp = tempdir().expect("tempdir");
        let staging = HostStagingDir::from_temp_dir_for_test(temp, "/mnt/c/Temp/sd-1");
        let host_repo = staging.host_repo_path().to_path_buf();
        let repo_for_command = host_repo.clone();
        let staged = stage_wsl_source_with_staging(
            session(true),
            WslAcquisitionSource::Local {
                native_path: "/home/alice/code/skills".to_string(),
            },
            CancellationSignal::default(),
            staging,
            move |_, _, _, _, _| async move {
                fs::create_dir_all(repo_for_command.join("plugins/toolkit"))
                    .expect("create staged skill");
                Ok(())
            },
        )
        .await
        .expect("stage source");

        assert_eq!(
            staged
                .linux_path_for_host_path(&host_repo.join("plugins/toolkit"))
                .expect("map staged path"),
            "/mnt/c/Temp/sd-1/repo/plugins/toolkit"
        );
        assert!(staged
            .linux_path_for_host_path(&host_repo.join("../outside"))
            .is_err());
    }
}

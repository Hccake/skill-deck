use std::collections::HashMap;
use std::fmt;
use std::path::{Component, Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tokio::io::AsyncReadExt;
use tokio::process::Command;

use environment_engine::payload::source_metadata_fingerprint;
#[cfg(target_os = "linux")]
use environment_engine::source_inventory::{
    scan_source_with_cancel, SourceEntryError as EngineEntryError,
    SourceEntryKind as EngineEntryKind, SourceInventoryRequest, SourceRoot,
    SourceScanMode as EngineScanMode,
};
#[cfg(target_os = "linux")]
use environment_protocol::{SourceEntry, SourceEntryErrorCode, SourceEntryKind};
use environment_protocol::{SourceScanRequest, SourceScanResponse};

const GIT_STDERR_LIMIT: usize = 256 * 1024;
const GIT_STDOUT_LIMIT: usize = 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitSourceOptions {
    pub url: String,
    pub git_ref: Option<String>,
    pub proxy: Option<String>,
    pub deadline: Duration,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenedSource {
    pub id: u64,
    pub root: PathBuf,
    pub revision: Option<String>,
}

#[derive(Debug)]
pub enum SourceError {
    InvalidManagedBase,
    InvalidLocalSource,
    InvalidRelativePath,
    InvalidInventory,
    MissingSource,
    GitUnavailable {
        message: String,
    },
    GitFailed {
        exit_code: Option<i32>,
        stderr: String,
    },
    DeadlineExceeded,
    Cancelled,
    Io(std::io::Error),
}

#[cfg(target_os = "linux")]
pub fn scan_source<F>(
    manager: &SourceManager,
    request: SourceScanRequest,
    is_cancelled: F,
) -> Result<SourceScanResponse, SourceError>
where
    F: Fn() -> bool,
{
    use std::ffi::OsString;
    use std::os::unix::ffi::{OsStrExt, OsStringExt};

    let roots = request
        .roots
        .into_iter()
        .map(|root| {
            let relative = PathBuf::from(OsString::from_vec(root.relative_path));
            manager
                .resolve(request.source_id, &relative)
                .map(|path| SourceRoot {
                    path,
                    stat_only: root.stat_only,
                })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let response = scan_source_with_cancel(
        &SourceInventoryRequest {
            roots,
            mode: match request.mode {
                environment_protocol::SourceScanMode::Recursive => EngineScanMode::Recursive,
                environment_protocol::SourceScanMode::PriorityDirectories => {
                    EngineScanMode::PriorityDirectories
                }
            },
            per_file_limit: request.per_file_limit,
            aggregate_limit: request.aggregate_limit,
        },
        is_cancelled,
    )
    .map_err(|_| SourceError::InvalidInventory)?;
    Ok(SourceScanResponse {
        entries: response
            .entries
            .into_iter()
            .map(|entry| SourceEntry {
                root_index: entry.root_index,
                relative_path: entry.relative_path.as_os_str().as_bytes().to_vec(),
                kind: match entry.kind {
                    EngineEntryKind::Missing => SourceEntryKind::Missing,
                    EngineEntryKind::File => SourceEntryKind::File,
                    EngineEntryKind::Directory => SourceEntryKind::Directory,
                    EngineEntryKind::Symlink => SourceEntryKind::Symlink,
                    EngineEntryKind::Other => SourceEntryKind::Other,
                },
                link_target: entry
                    .link_target
                    .map(|target| target.as_os_str().as_bytes().to_vec()),
                content_bytes: entry.content_bytes,
                truncated: entry.truncated,
                error_code: entry.error.map(|error| match error {
                    EngineEntryError::PathUnavailable => SourceEntryErrorCode::PathUnavailable,
                    EngineEntryError::ReadFailed => SourceEntryErrorCode::ReadFailed,
                    EngineEntryError::ReadLinkFailed => SourceEntryErrorCode::ReadLinkFailed,
                }),
            })
            .collect(),
        total_content_bytes: response.total_content_bytes,
    })
}

#[cfg(not(target_os = "linux"))]
pub fn scan_source<F>(
    _manager: &SourceManager,
    _request: SourceScanRequest,
    _is_cancelled: F,
) -> Result<SourceScanResponse, SourceError>
where
    F: Fn() -> bool,
{
    Err(SourceError::InvalidInventory)
}

impl fmt::Display for SourceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for SourceError {}

impl From<std::io::Error> for SourceError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

struct SourceRecord {
    root: PathBuf,
    cleanup_root: Option<PathBuf>,
    revision: Option<String>,
}

pub struct SourceManager {
    managed_base: PathBuf,
    next_id: u64,
    sources: HashMap<u64, SourceRecord>,
}

impl SourceManager {
    pub fn new(managed_base: PathBuf) -> Result<Self, SourceError> {
        if !managed_base.is_absolute() {
            return Err(SourceError::InvalidManagedBase);
        }
        std::fs::create_dir_all(&managed_base)?;
        let managed_base = std::fs::canonicalize(managed_base)?;
        if !managed_base.is_dir() {
            return Err(SourceError::InvalidManagedBase);
        }
        Ok(Self {
            managed_base,
            next_id: 1,
            sources: HashMap::new(),
        })
    }

    pub fn open_local(&mut self, path: &str) -> Result<OpenedSource, SourceError> {
        let path = Path::new(path);
        if !path.is_absolute() {
            return Err(SourceError::InvalidLocalSource);
        }
        let root = std::fs::canonicalize(path).map_err(|_| SourceError::InvalidLocalSource)?;
        if !root.is_dir() {
            return Err(SourceError::InvalidLocalSource);
        }
        Ok(self.insert(root, None, None))
    }

    pub async fn acquire_git(
        &mut self,
        options: GitSourceOptions,
        cancelled: Arc<AtomicBool>,
    ) -> Result<OpenedSource, SourceError> {
        if options.url.is_empty() || options.deadline.is_zero() {
            return Err(SourceError::GitFailed {
                exit_code: None,
                stderr: "invalid Git source request".to_string(),
            });
        }
        if cancelled.load(Ordering::Acquire) {
            return Err(SourceError::Cancelled);
        }
        let id = self.allocate_id()?;
        let managed_root = self.managed_base.join(format!(
            "skill-deck-discovery-worker-{}-{id}",
            std::process::id()
        ));
        if std::fs::symlink_metadata(&managed_root).is_ok() {
            return Err(SourceError::InvalidManagedBase);
        }
        std::fs::create_dir(&managed_root)?;
        std::fs::write(managed_root.join(".skill-deck-owner"), b"1\n")?;
        let repository = managed_root.join("repo");
        let clone_result = run_git(
            git_clone_arguments(&options, &repository),
            options.proxy.as_deref(),
            options.deadline,
            Arc::clone(&cancelled),
        )
        .await;
        if let Err(error) = clone_result {
            let _ = std::fs::remove_dir_all(&managed_root);
            return Err(error);
        }
        let revision = run_git(
            vec![
                "-C".to_string(),
                repository.to_string_lossy().into_owned(),
                "rev-parse".to_string(),
                "--verify".to_string(),
                "HEAD".to_string(),
            ],
            None,
            options.deadline,
            cancelled,
        )
        .await
        .and_then(|output| parse_revision(&output.stdout));
        let revision = match revision {
            Ok(revision) => revision,
            Err(error) => {
                let _ = std::fs::remove_dir_all(&managed_root);
                return Err(error);
            }
        };
        let opened = OpenedSource {
            id,
            root: repository.clone(),
            revision: Some(revision.clone()),
        };
        self.sources.insert(
            id,
            SourceRecord {
                root: repository,
                cleanup_root: Some(managed_root),
                revision: Some(revision),
            },
        );
        Ok(opened)
    }

    pub fn root(&self, source_id: u64) -> Result<&Path, SourceError> {
        self.sources
            .get(&source_id)
            .map(|source| source.root.as_path())
            .ok_or(SourceError::MissingSource)
    }

    pub fn revision(&self, source_id: u64) -> Result<Option<&str>, SourceError> {
        self.sources
            .get(&source_id)
            .map(|source| source.revision.as_deref())
            .ok_or(SourceError::MissingSource)
    }

    pub fn fingerprint(&self, source_id: u64, relative_path: &Path) -> Result<String, SourceError> {
        let path = self.resolve(source_id, relative_path)?;
        source_metadata_fingerprint(&path).map_err(|_| SourceError::InvalidLocalSource)
    }

    pub async fn tree_revision(
        &self,
        source_id: u64,
        relative_path: &Path,
        deadline: Duration,
        cancelled: Arc<AtomicBool>,
    ) -> Result<String, SourceError> {
        let root = self.root(source_id)?;
        let relative = normalized_git_path(relative_path)?;
        let revision_spec = if relative.is_empty() {
            "HEAD^{tree}".to_string()
        } else {
            format!("HEAD:{relative}")
        };
        let output = run_git(
            vec![
                "-C".to_string(),
                root.to_string_lossy().into_owned(),
                "rev-parse".to_string(),
                "--verify".to_string(),
                revision_spec,
            ],
            None,
            deadline,
            cancelled,
        )
        .await?;
        parse_revision(&output.stdout)
    }

    pub fn resolve(&self, source_id: u64, relative_path: &Path) -> Result<PathBuf, SourceError> {
        if relative_path.is_absolute()
            || relative_path
                .components()
                .any(|component| !matches!(component, Component::Normal(_) | Component::CurDir))
        {
            return Err(SourceError::InvalidRelativePath);
        }
        Ok(self.root(source_id)?.join(relative_path))
    }

    pub fn release(&mut self, source_id: u64) -> Result<(), SourceError> {
        let source = self
            .sources
            .remove(&source_id)
            .ok_or(SourceError::MissingSource)?;
        if let Some(cleanup_root) = source.cleanup_root {
            std::fs::remove_dir_all(cleanup_root)?;
        }
        Ok(())
    }

    fn insert(
        &mut self,
        root: PathBuf,
        cleanup_root: Option<PathBuf>,
        revision: Option<String>,
    ) -> OpenedSource {
        let id = self.allocate_id().expect("source handle space exhausted");
        self.sources.insert(
            id,
            SourceRecord {
                root: root.clone(),
                cleanup_root,
                revision: revision.clone(),
            },
        );
        OpenedSource { id, root, revision }
    }

    fn allocate_id(&mut self) -> Result<u64, SourceError> {
        let id = self.next_id;
        self.next_id = self
            .next_id
            .checked_add(1)
            .ok_or(SourceError::InvalidManagedBase)?;
        Ok(id)
    }
}

pub async fn probe_git(
    options: GitSourceOptions,
    cancelled: Arc<AtomicBool>,
) -> Result<String, SourceError> {
    if options.url.is_empty() || options.deadline.is_zero() {
        return Err(SourceError::GitFailed {
            exit_code: None,
            stderr: "invalid Git probe request".to_string(),
        });
    }
    let output = run_git(
        vec![
            "ls-remote".to_string(),
            "--exit-code".to_string(),
            "--".to_string(),
            options.url,
            "HEAD".to_string(),
        ],
        options.proxy.as_deref(),
        options.deadline,
        cancelled,
    )
    .await?;
    let revision = output
        .stdout
        .split(|byte| byte.is_ascii_whitespace())
        .next()
        .unwrap_or_default();
    parse_revision(revision)
}

impl Drop for SourceManager {
    fn drop(&mut self) {
        for source in self.sources.values_mut() {
            if let Some(cleanup_root) = source.cleanup_root.take() {
                let _ = std::fs::remove_dir_all(cleanup_root);
            }
        }
    }
}

struct GitOutput {
    stdout: Vec<u8>,
}

fn git_clone_arguments(options: &GitSourceOptions, repository: &Path) -> Vec<String> {
    let mut arguments = vec![
        "clone".to_string(),
        "--depth".to_string(),
        "1".to_string(),
        "--progress".to_string(),
    ];
    if let Some(git_ref) = &options.git_ref {
        arguments.extend(["--branch".to_string(), git_ref.clone()]);
    }
    arguments.extend([
        "--".to_string(),
        options.url.clone(),
        repository.to_string_lossy().into_owned(),
    ]);
    arguments
}

fn normalized_git_path(path: &Path) -> Result<String, SourceError> {
    if path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_) | Component::CurDir))
    {
        return Err(SourceError::InvalidRelativePath);
    }
    let mut components = Vec::new();
    for component in path.components() {
        match component {
            Component::Normal(value) => {
                components.push(value.to_str().ok_or(SourceError::InvalidRelativePath)?)
            }
            Component::CurDir => {}
            _ => return Err(SourceError::InvalidRelativePath),
        }
    }
    Ok(components.join("/"))
}

#[allow(
    clippy::disallowed_methods,
    reason = "Worker crate 独立管理 Linux Git 进程组，不依赖 app crate 的进程 helper"
)]
async fn run_git(
    arguments: Vec<String>,
    proxy: Option<&str>,
    deadline: Duration,
    cancelled: Arc<AtomicBool>,
) -> Result<GitOutput, SourceError> {
    let mut command = Command::new("git");
    command
        .env("LC_ALL", "C")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_ASKPASS", "")
        .env("SSH_ASKPASS", "")
        .env("GIT_ALLOW_PROTOCOL", "https:http:ssh:git:file")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(proxy) = proxy {
        command.args(["-c", &format!("http.proxy={proxy}")]);
    }
    command.args(arguments);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.as_std_mut().process_group(0);
    }
    let mut child = command
        .spawn()
        .map_err(|error| SourceError::GitUnavailable {
            message: error.to_string(),
        })?;
    let process_group_id = child.id().and_then(|id| i32::try_from(id).ok());
    let stdout = child.stdout.take().ok_or_else(|| SourceError::GitFailed {
        exit_code: None,
        stderr: "Git stdout is unavailable".to_string(),
    })?;
    let stderr = child.stderr.take().ok_or_else(|| SourceError::GitFailed {
        exit_code: None,
        stderr: "Git stderr is unavailable".to_string(),
    })?;
    let stdout_task = tokio::spawn(read_prefix(stdout, GIT_STDOUT_LIMIT));
    let stderr_task = tokio::spawn(read_tail(stderr, GIT_STDERR_LIMIT));
    let mut wait = Box::pin(child.wait());
    let mut deadline_sleep = Box::pin(tokio::time::sleep(deadline));
    let status = loop {
        tokio::select! {
            result = &mut wait => break result?,
            _ = &mut deadline_sleep => {
                kill_process_group(process_group_id);
                let _ = wait.await;
                return Err(SourceError::DeadlineExceeded);
            }
            _ = tokio::time::sleep(Duration::from_millis(25)) => {
                if cancelled.load(Ordering::Acquire) {
                    kill_process_group(process_group_id);
                    let _ = wait.await;
                    return Err(SourceError::Cancelled);
                }
            }
        }
    };
    let stdout = stdout_task.await.map_err(|_| SourceError::GitFailed {
        exit_code: status.code(),
        stderr: "failed to capture Git stdout".to_string(),
    })??;
    let stderr = stderr_task.await.map_err(|_| SourceError::GitFailed {
        exit_code: status.code(),
        stderr: "failed to capture Git stderr".to_string(),
    })??;
    if !status.success() {
        return Err(SourceError::GitFailed {
            exit_code: status.code(),
            stderr: String::from_utf8_lossy(&stderr).into_owned(),
        });
    }
    Ok(GitOutput { stdout })
}

async fn read_prefix<R>(mut reader: R, limit: usize) -> Result<Vec<u8>, std::io::Error>
where
    R: tokio::io::AsyncRead + Unpin,
{
    let mut output = Vec::new();
    let mut buffer = [0_u8; 8192];
    loop {
        let read = reader.read(&mut buffer).await?;
        if read == 0 {
            return Ok(output);
        }
        let remaining = limit.saturating_sub(output.len());
        output.extend_from_slice(&buffer[..read.min(remaining)]);
    }
}

async fn read_tail<R>(mut reader: R, limit: usize) -> Result<Vec<u8>, std::io::Error>
where
    R: tokio::io::AsyncRead + Unpin,
{
    let mut output = Vec::new();
    let mut buffer = [0_u8; 8192];
    loop {
        let read = reader.read(&mut buffer).await?;
        if read == 0 {
            return Ok(output);
        }
        output.extend_from_slice(&buffer[..read]);
        if output.len() > limit {
            output.drain(..output.len() - limit);
        }
    }
}

fn parse_revision(bytes: &[u8]) -> Result<String, SourceError> {
    let revision = std::str::from_utf8(bytes)
        .map_err(|_| SourceError::GitFailed {
            exit_code: None,
            stderr: "Git revision is not UTF-8".to_string(),
        })?
        .trim();
    if matches!(revision.len(), 40 | 64) && revision.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Ok(revision.to_ascii_lowercase())
    } else {
        Err(SourceError::GitFailed {
            exit_code: None,
            stderr: "Git returned an invalid revision".to_string(),
        })
    }
}

#[cfg(unix)]
fn kill_process_group(process_group_id: Option<i32>) {
    if let Some(process_group_id) = process_group_id {
        let _ = unsafe { libc::kill(-process_group_id, libc::SIGKILL) };
    }
}

#[cfg(not(unix))]
fn kill_process_group(_process_group_id: Option<i32>) {}

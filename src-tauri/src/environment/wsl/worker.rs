use std::collections::{HashMap, HashSet, VecDeque};
use std::path::Path;
#[cfg(target_os = "windows")]
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use environment_protocol::{
    codec, decode, spawn_writer, Envelope, Message, PathKind, ProtocolWriter, WireRecord,
    MAX_PAYLOAD_CHUNK_BYTES, MAX_RESPONSE_TRANSFER_BYTES,
};
use futures_util::StreamExt;
use serde::Deserialize;
use sha2::{Digest, Sha256};
#[cfg(target_os = "windows")]
use tokio::io::AsyncReadExt;
use tokio::io::{AsyncRead, AsyncWrite};
#[cfg(target_os = "windows")]
use tokio::process::Child;
use tokio::sync::{oneshot, watch};
use tokio::task::AbortHandle;
use tokio::time::{sleep, Duration};
use tokio_util::codec::FramedRead;

use crate::core::mutation::CancellationSignal;
use crate::environment::types::EnvironmentRef;
#[cfg(target_os = "windows")]
use crate::environment::wsl::protocol::{
    WslCommandRequest, WslCommandRunner, DEFAULT_WSL_STDERR_LIMIT, DEFAULT_WSL_STDOUT_LIMIT,
};
use crate::error::AppError;

const TOMBSTONE_LIMIT: usize = 256;
const WORKER_TARGET: &str = "x86_64-unknown-linux-musl";
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);
#[cfg(target_os = "windows")]
const WORKER_START_TIMEOUT: Duration = Duration::from_secs(15);
#[cfg(target_os = "windows")]
const WORKER_STDERR_LIMIT: usize = 64 * 1024;
const BOOTSTRAP_SCRIPT: &str = r#"set -eu
expected=$1
target="$HOME/.skill-deck/runtime/wsl-worker/current/worker"
directory=${target%/*}
temporary="$target.tmp.$$"
mkdir -p "$directory"
umask 077
if [ -e "$target" ] || [ -L "$target" ]; then
  if [ ! -f "$target" ] || [ -L "$target" ]; then
    printf '%s\n' 'WSL worker target is not a regular file' >&2
    exit 71
  fi
  actual=$(sha256sum "$target" | awk '{print $1}')
  if [ "sha256:$actual" = "$expected" ]; then
    cat >/dev/null
    chmod 700 "$target"
    exit 0
  fi
fi
trap 'rm -f "$temporary"' EXIT HUP INT TERM
cat > "$temporary"
actual=$(sha256sum "$temporary" | awk '{print $1}')
if [ "sha256:$actual" != "$expected" ]; then
  printf '%s\n' 'WSL worker digest mismatch' >&2
  exit 70
fi
chmod 700 "$temporary"
if [ -e "$target" ] || [ -L "$target" ]; then
  if [ ! -f "$target" ] || [ -L "$target" ]; then
    printf '%s\n' 'WSL worker target changed type during deployment' >&2
    exit 71
  fi
fi
mv -f "$temporary" "$target"
trap - EXIT HUP INT TERM
"#;

#[derive(Debug)]
struct WorkerArtifact {
    bytes: Vec<u8>,
    build_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct WorkerManifest {
    build_id: String,
    sha256: String,
    target: String,
}

impl WorkerArtifact {
    fn load_from(directory: &Path) -> Result<Self, String> {
        let bytes = std::fs::read(directory.join("worker")).map_err(|error| error.to_string())?;
        let manifest: WorkerManifest = serde_json::from_slice(
            &std::fs::read(directory.join("manifest.json")).map_err(|error| error.to_string())?,
        )
        .map_err(|error| error.to_string())?;
        let actual = format!("sha256:{:x}", Sha256::digest(&bytes));
        if manifest.target != WORKER_TARGET {
            return Err(format!(
                "unsupported WSL worker target: {}",
                manifest.target
            ));
        }
        if manifest.build_id != manifest.sha256 || manifest.sha256 != actual {
            return Err("WSL worker bytes do not match the manifest".to_string());
        }
        Ok(Self {
            bytes,
            build_id: manifest.build_id,
        })
    }
}

#[derive(Clone)]
pub(super) struct WorkerSession {
    inner: Arc<WorkerSessionInner>,
}

struct WorkerSessionInner {
    distro_name: String,
    next_request_id: AtomicU64,
    routes: Arc<Mutex<ResponseRoutes>>,
    writer: ProtocolWriter,
    closed: watch::Sender<bool>,
    reader_task: AbortHandle,
    writer_task: AbortHandle,
    #[cfg(target_os = "windows")]
    stderr_task: Option<AbortHandle>,
    #[cfg(target_os = "windows")]
    child: Mutex<Option<Child>>,
}

#[derive(Default)]
struct ResponseRoutes {
    pending: HashMap<u64, PendingResponse>,
    active_transfer: Option<IncomingTransfer>,
    tombstones: VecDeque<u64>,
    tombstone_set: HashSet<u64>,
}

struct PendingResponse {
    expected: ExpectedResponse,
    max_payload_bytes: usize,
    accepted_resource: Option<Arc<Mutex<Option<String>>>>,
    sender: oneshot::Sender<Result<RoutedResponse, AppError>>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ExpectedResponse {
    Control,
    Payload,
    Mutation,
}

#[derive(Debug)]
pub(super) struct MutationSessionError {
    pub error: AppError,
    pub accepted_resource_id: Option<String>,
}

enum RoutedResponse {
    Control(Message),
    Payload(Vec<u8>),
}

struct IncomingTransfer {
    transfer_id: u64,
    owner_request_id: u64,
    total_bytes: usize,
    expected_sha256: String,
    received_bytes: usize,
    hasher: Sha256,
    payload: Option<Vec<u8>>,
}

impl WorkerSession {
    fn from_io<R, W>(reader: R, writer: W, distro_name: String) -> Self
    where
        R: AsyncRead + Unpin + Send + 'static,
        W: AsyncWrite + Unpin + Send + 'static,
    {
        let routes = Arc::new(Mutex::new(ResponseRoutes::default()));
        let (closed, _) = watch::channel(false);
        let (writer, writer_task) = spawn_writer(writer);
        let reader_routes = Arc::clone(&routes);
        let reader_distro = distro_name.clone();
        let reader_closed = closed.clone();
        let reader_task = tokio::spawn(async move {
            let mut reader = FramedRead::new(reader, codec());
            while let Some(frame) = reader.next().await {
                let record = match frame
                    .map_err(|error| error.to_string())
                    .and_then(|bytes| decode(&bytes).map_err(|error| error.to_string()))
                {
                    Ok(record) => record,
                    Err(error) => {
                        fail_pending(&reader_routes, unavailable(&reader_distro, error));
                        let _ = reader_closed.send(true);
                        return;
                    }
                };
                if let Err(error) = route_record(&reader_routes, record) {
                    fail_pending(&reader_routes, unavailable(&reader_distro, error));
                    let _ = reader_closed.send(true);
                    return;
                }
            }
            fail_pending(
                &reader_routes,
                unavailable(&reader_distro, "worker closed its output"),
            );
            let _ = reader_closed.send(true);
        });
        Self {
            inner: Arc::new(WorkerSessionInner {
                distro_name,
                next_request_id: AtomicU64::new(1),
                routes,
                writer,
                closed,
                reader_task: reader_task.abort_handle(),
                writer_task: writer_task.abort_handle(),
                #[cfg(target_os = "windows")]
                stderr_task: None,
                #[cfg(target_os = "windows")]
                child: Mutex::new(None),
            }),
        }
    }

    #[cfg(target_os = "windows")]
    fn from_child(mut child: Child, distro_name: String) -> Result<Self, AppError> {
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| unavailable(&distro_name, "worker stdin was not piped"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| unavailable(&distro_name, "worker stdout was not piped"))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| unavailable(&distro_name, "worker stderr was not piped"))?;
        let stderr_distro = distro_name.clone();
        let stderr_task = tokio::spawn(async move {
            drain_worker_stderr(stderr, &stderr_distro).await;
        });
        let mut session = Self::from_io(stdout, stdin, distro_name);
        let inner = Arc::get_mut(&mut session.inner).expect("new worker session is uniquely owned");
        inner.stderr_task = Some(stderr_task.abort_handle());
        *inner.child.lock().expect("worker child lock poisoned") = Some(child);
        Ok(session)
    }

    async fn request(&self, message: Message, limit: Duration) -> Result<Message, AppError> {
        match self
            .request_response(message, limit, ExpectedResponse::Control, None)
            .await?
        {
            RoutedResponse::Control(message) => Ok(message),
            RoutedResponse::Payload(_) => Err(unavailable(
                &self.inner.distro_name,
                "worker returned a payload for a control request",
            )),
        }
    }

    pub(super) async fn request_control_with_cancellation(
        &self,
        message: Message,
        limit: Duration,
        cancellation: Option<CancellationSignal>,
    ) -> Result<Message, AppError> {
        match self
            .request_response(message, limit, ExpectedResponse::Control, cancellation)
            .await?
        {
            RoutedResponse::Control(message) => Ok(message),
            RoutedResponse::Payload(_) => Err(unavailable(
                &self.inner.distro_name,
                "worker returned a payload for a control request",
            )),
        }
    }

    pub(super) async fn request_payload(
        &self,
        message: Message,
        limit: Duration,
    ) -> Result<Vec<u8>, AppError> {
        match self
            .request_response(message, limit, ExpectedResponse::Payload, None)
            .await?
        {
            RoutedResponse::Payload(payload) => Ok(payload),
            RoutedResponse::Control(Message::Error { code, phase, .. }) => Err(
                payload_control_error(&self.inner.distro_name, &code, &phase),
            ),
            RoutedResponse::Control(_) => Err(unavailable(
                &self.inner.distro_name,
                "worker returned control for a payload request",
            )),
        }
    }

    pub(super) async fn request_payload_with_limit(
        &self,
        message: Message,
        limit: Duration,
        max_payload_bytes: usize,
        cancellation: Option<CancellationSignal>,
    ) -> Result<Vec<u8>, AppError> {
        match self
            .request_response_with_payload_limit(
                message,
                limit,
                ExpectedResponse::Payload,
                cancellation,
                max_payload_bytes,
            )
            .await?
        {
            RoutedResponse::Payload(payload) => Ok(payload),
            RoutedResponse::Control(Message::Error { code, phase, .. }) => Err(
                payload_control_error(&self.inner.distro_name, &code, &phase),
            ),
            RoutedResponse::Control(_) => Err(unavailable(
                &self.inner.distro_name,
                "worker returned control for a payload request",
            )),
        }
    }

    pub(super) async fn send_prepared_transfer(
        &self,
        transfer_id: u64,
        payload: &[u8],
        max_payload_bytes: usize,
        limit: Duration,
    ) -> Result<Message, AppError> {
        let request_id = self.inner.next_request_id.fetch_add(1, Ordering::Relaxed);
        let (response_tx, response_rx) = oneshot::channel();
        self.inner
            .routes
            .lock()
            .expect("worker response routes lock poisoned")
            .pending
            .insert(
                request_id,
                PendingResponse {
                    expected: ExpectedResponse::Control,
                    max_payload_bytes: 0,
                    accepted_resource: None,
                    sender: response_tx,
                },
            );
        if let Err(error) = self
            .inner
            .writer
            .send_transfer_with_limit(request_id, transfer_id, payload, max_payload_bytes)
            .await
        {
            self.inner
                .routes
                .lock()
                .expect("worker response routes lock poisoned")
                .pending
                .remove(&request_id);
            return Err(unavailable(&self.inner.distro_name, error.to_string()));
        }
        match tokio::time::timeout(limit, response_rx).await {
            Ok(Ok(Ok(RoutedResponse::Control(message)))) => Ok(message),
            Ok(Ok(Ok(RoutedResponse::Payload(_)))) => Err(unavailable(
                &self.inner.distro_name,
                "worker returned a payload for an inbound transfer",
            )),
            Ok(Ok(Err(error))) => Err(error),
            Ok(Err(_)) => Err(unavailable(
                &self.inner.distro_name,
                "worker response router stopped",
            )),
            Err(_) => {
                let cancelled = {
                    let mut routes = self
                        .inner
                        .routes
                        .lock()
                        .expect("worker response routes lock poisoned");
                    let cancelled = routes.cancel_request(request_id);
                    if cancelled {
                        routes.remember_tombstone(request_id);
                    }
                    cancelled
                };
                if cancelled {
                    let cancel_request_id =
                        self.inner.next_request_id.fetch_add(1, Ordering::Relaxed);
                    let _ = self
                        .inner
                        .writer
                        .send_control(WireRecord::Control(Envelope {
                            request_id: cancel_request_id,
                            message: Message::Cancel {
                                target_request_id: request_id,
                            },
                        }))
                        .await;
                }
                Err(AppError::WslCommandTimedOut)
            }
        }
    }

    pub(super) async fn send_prepared_mutation(
        &self,
        transfer_id: u64,
        payload: &[u8],
        cancellation: CancellationSignal,
        limit: Duration,
    ) -> Result<environment_protocol::MutationUnitOutcome, MutationSessionError> {
        let request_id = self.inner.next_request_id.fetch_add(1, Ordering::Relaxed);
        let accepted_resource = Arc::new(Mutex::new(None));
        let (response_tx, response_rx) = oneshot::channel();
        self.inner
            .routes
            .lock()
            .expect("worker response routes lock poisoned")
            .pending
            .insert(
                request_id,
                PendingResponse {
                    expected: ExpectedResponse::Mutation,
                    max_payload_bytes: environment_protocol::MAX_MUTATION_TRANSFER_BYTES,
                    accepted_resource: Some(Arc::clone(&accepted_resource)),
                    sender: response_tx,
                },
            );
        if let Err(error) = self
            .inner
            .writer
            .send_transfer_with_limit(
                request_id,
                transfer_id,
                payload,
                environment_protocol::MAX_MUTATION_TRANSFER_BYTES,
            )
            .await
        {
            self.inner
                .routes
                .lock()
                .expect("worker response routes lock poisoned")
                .pending
                .remove(&request_id);
            return Err(MutationSessionError {
                error: unavailable(&self.inner.distro_name, error.to_string()),
                accepted_resource_id: None,
            });
        }
        let deadline = tokio::time::Instant::now() + limit;
        let mut response_rx = response_rx;
        let mut cancel_sent = false;
        let waited = tokio::select! {
            response = &mut response_rx => Some(response),
            _ = tokio::time::sleep_until(deadline) => None,
            _ = cancellation.cancelled() => {
                cancel_sent = true;
                let cancel_request_id = self.inner.next_request_id.fetch_add(1, Ordering::Relaxed);
                let _ = self
                    .inner
                    .writer
                    .send_control(WireRecord::Control(Envelope {
                        request_id: cancel_request_id,
                        message: Message::Cancel {
                            target_request_id: request_id,
                        },
                    }))
                    .await;
                tokio::time::timeout_at(deadline, &mut response_rx)
                    .await
                    .ok()
            },
        };
        let result = match waited {
            Some(Ok(Ok(RoutedResponse::Payload(payload)))) => {
                environment_protocol::decode_payload(&payload).map_err(|error| {
                    AppError::ConfigurationCorrupted {
                        message: format!("invalid WSL Worker mutation response: {error}"),
                    }
                })
            }
            Some(Ok(Ok(RoutedResponse::Control(Message::Error { code, phase, .. })))) => {
                Err(AppError::ExecutionFailed {
                    message: format!("worker mutation failed during {phase}: {code}"),
                })
            }
            Some(Ok(Ok(RoutedResponse::Control(_)))) => Err(unavailable(
                &self.inner.distro_name,
                "worker returned control for a mutation request",
            )),
            Some(Ok(Err(error))) => Err(error),
            Some(Err(_)) => Err(unavailable(
                &self.inner.distro_name,
                "worker response router stopped",
            )),
            None => {
                let cancelled = {
                    let mut routes = self
                        .inner
                        .routes
                        .lock()
                        .expect("worker response routes lock poisoned");
                    let cancelled = routes.cancel_request(request_id);
                    if cancelled {
                        routes.remember_tombstone(request_id);
                    }
                    cancelled
                };
                if cancelled && !cancel_sent {
                    let cancel_request_id =
                        self.inner.next_request_id.fetch_add(1, Ordering::Relaxed);
                    let _ = self
                        .inner
                        .writer
                        .send_control(WireRecord::Control(Envelope {
                            request_id: cancel_request_id,
                            message: Message::Cancel {
                                target_request_id: request_id,
                            },
                        }))
                        .await;
                }
                Err(if cancellation.is_cancelled() {
                    AppError::MutationCancelled
                } else {
                    AppError::WslCommandTimedOut
                })
            }
        };
        result.map_err(|error| MutationSessionError {
            error,
            accepted_resource_id: accepted_resource
                .lock()
                .expect("worker mutation accepted state lock poisoned")
                .clone(),
        })
    }

    pub(super) async fn request_payload_with_cancellation(
        &self,
        message: Message,
        limit: Duration,
        cancellation: CancellationSignal,
    ) -> Result<Vec<u8>, AppError> {
        match self
            .request_response(
                message,
                limit,
                ExpectedResponse::Payload,
                Some(cancellation),
            )
            .await?
        {
            RoutedResponse::Payload(payload) => Ok(payload),
            RoutedResponse::Control(Message::Error { code, .. }) if code == "deadlineExceeded" => {
                Err(AppError::WslCommandTimedOut)
            }
            RoutedResponse::Control(Message::Error { code, phase, .. }) => {
                Err(AppError::ExecutionFailed {
                    message: format!("worker request failed during {phase}: {code}"),
                })
            }
            RoutedResponse::Control(_) => Err(unavailable(
                &self.inner.distro_name,
                "worker returned control for a payload request",
            )),
        }
    }

    async fn request_response(
        &self,
        message: Message,
        limit: Duration,
        expected: ExpectedResponse,
        cancellation: Option<CancellationSignal>,
    ) -> Result<RoutedResponse, AppError> {
        self.request_response_with_payload_limit(
            message,
            limit,
            expected,
            cancellation,
            MAX_RESPONSE_TRANSFER_BYTES,
        )
        .await
    }

    async fn request_response_with_payload_limit(
        &self,
        message: Message,
        limit: Duration,
        expected: ExpectedResponse,
        cancellation: Option<CancellationSignal>,
        max_payload_bytes: usize,
    ) -> Result<RoutedResponse, AppError> {
        let request_id = self.inner.next_request_id.fetch_add(1, Ordering::Relaxed);
        let (response_tx, response_rx) = oneshot::channel();
        self.inner
            .routes
            .lock()
            .expect("worker response routes lock poisoned")
            .pending
            .insert(
                request_id,
                PendingResponse {
                    expected,
                    max_payload_bytes,
                    accepted_resource: None,
                    sender: response_tx,
                },
            );
        if let Err(error) = self
            .inner
            .writer
            .send_control(WireRecord::Control(Envelope {
                request_id,
                message,
            }))
            .await
        {
            self.inner
                .routes
                .lock()
                .expect("worker response routes lock poisoned")
                .pending
                .remove(&request_id);
            return Err(unavailable(&self.inner.distro_name, error.to_string()));
        }

        enum WaitResult<T> {
            Response(T),
            TimedOut,
            Cancelled,
        }
        let waited = if let Some(cancellation) = cancellation {
            tokio::select! {
                response = response_rx => WaitResult::Response(response),
                _ = sleep(limit) => WaitResult::TimedOut,
                _ = cancellation.cancelled() => WaitResult::Cancelled,
            }
        } else {
            match tokio::time::timeout(limit, response_rx).await {
                Ok(response) => WaitResult::Response(response),
                Err(_) => WaitResult::TimedOut,
            }
        };
        match waited {
            WaitResult::Response(Ok(response)) => response,
            WaitResult::Response(Err(_)) => Err(unavailable(
                &self.inner.distro_name,
                "worker response router stopped",
            )),
            reason @ (WaitResult::TimedOut | WaitResult::Cancelled) => {
                let was_cancelled = matches!(reason, WaitResult::Cancelled);
                let cancelled = {
                    let mut routes = self
                        .inner
                        .routes
                        .lock()
                        .expect("worker response routes lock poisoned");
                    let cancelled = routes.cancel_request(request_id);
                    if cancelled {
                        routes.remember_tombstone(request_id);
                    }
                    cancelled
                };
                if cancelled {
                    let cancel_request_id =
                        self.inner.next_request_id.fetch_add(1, Ordering::Relaxed);
                    let _ = self
                        .inner
                        .writer
                        .send_control(WireRecord::Control(Envelope {
                            request_id: cancel_request_id,
                            message: Message::Cancel {
                                target_request_id: request_id,
                            },
                        }))
                        .await;
                }
                if was_cancelled {
                    Err(AppError::MutationCancelled)
                } else {
                    Err(AppError::WslCommandTimedOut)
                }
            }
        }
    }

    pub(super) fn closed_receiver(&self) -> watch::Receiver<bool> {
        self.inner.closed.subscribe()
    }

    async fn handshake(
        &self,
        expected: &super::WslSession,
        build_id: &str,
    ) -> Result<(), AppError> {
        match self
            .request(
                Message::Handshake {
                    build_id: build_id.to_string(),
                },
                HANDSHAKE_TIMEOUT,
            )
            .await?
        {
            Message::HandshakeResult {
                build_id: actual_build,
                distro,
                user,
                uid,
                home,
            } if actual_build == build_id
                && distro.eq_ignore_ascii_case(&expected.distro_name)
                && user == expected.user
                && uid == expected.uid
                && home == expected.home => {}
            Message::Error { code, phase, .. } => {
                return Err(unavailable(
                    &expected.distro_name,
                    format!("worker handshake failed during {phase}: {code}"),
                ));
            }
            _ => {
                return Err(unavailable(
                    &expected.distro_name,
                    "worker identity does not match the WSL session",
                ));
            }
        }

        match self
            .request(
                Message::ObservePath {
                    path: expected.home.clone(),
                },
                HANDSHAKE_TIMEOUT,
            )
            .await?
        {
            Message::PathObserved {
                kind: PathKind::Directory | PathKind::SymlinkDirectory,
            } => Ok(()),
            Message::Error { code, phase, .. } => Err(unavailable(
                &expected.distro_name,
                format!("worker HOME observation failed during {phase}: {code}"),
            )),
            _ => Err(unavailable(
                &expected.distro_name,
                "WSL HOME is not an accessible directory",
            )),
        }
    }
}

impl Drop for WorkerSessionInner {
    fn drop(&mut self) {
        let _ = self.closed.send(true);
        self.reader_task.abort();
        self.writer_task.abort();
        #[cfg(target_os = "windows")]
        {
            if let Some(task) = &self.stderr_task {
                task.abort();
            }
            if let Ok(mut child) = self.child.lock() {
                if let Some(child) = child.as_mut() {
                    let _ = child.start_kill();
                }
            }
        }
    }
}

impl ResponseRoutes {
    fn cancel_request(&mut self, request_id: u64) -> bool {
        let cancelled = self.pending.remove(&request_id).is_some();
        if cancelled {
            if let Some(transfer) = self
                .active_transfer
                .as_mut()
                .filter(|transfer| transfer.owner_request_id == request_id)
            {
                transfer.payload = None;
            }
        }
        cancelled
    }

    fn remember_tombstone(&mut self, request_id: u64) {
        if self.tombstone_set.insert(request_id) {
            self.tombstones.push_back(request_id);
        }
        while self.tombstones.len() > TOMBSTONE_LIMIT {
            if let Some(expired) = self.tombstones.pop_front() {
                self.tombstone_set.remove(&expired);
            }
        }
    }
}

fn route_record(routes: &Mutex<ResponseRoutes>, record: WireRecord) -> Result<(), String> {
    let mut routes = routes.lock().expect("worker response routes lock poisoned");
    match record {
        WireRecord::Control(envelope) => route_control(&mut routes, envelope),
        WireRecord::PayloadChunk { transfer_id, bytes } => {
            route_payload_chunk(&mut routes, transfer_id, bytes)
        }
    }
}

fn route_control(routes: &mut ResponseRoutes, envelope: Envelope) -> Result<(), String> {
    match envelope.message {
        Message::BeginTransfer {
            transfer_id,
            total_bytes,
            sha256,
            owner_request_id,
        } => begin_transfer(
            routes,
            envelope.request_id,
            transfer_id,
            total_bytes,
            sha256,
            owner_request_id,
        ),
        Message::TransferCompleted {
            transfer_id,
            total_bytes,
            sha256,
        } => complete_transfer(
            routes,
            envelope.request_id,
            transfer_id,
            total_bytes,
            sha256,
        ),
        Message::Progress { .. } => {
            if routes.pending.contains_key(&envelope.request_id)
                || routes.tombstone_set.contains(&envelope.request_id)
            {
                Ok(())
            } else {
                Err("worker progress has no owning request".to_string())
            }
        }
        message => route_control_response(routes, envelope.request_id, message),
    }
}

fn begin_transfer(
    routes: &mut ResponseRoutes,
    envelope_request_id: u64,
    transfer_id: u64,
    total_bytes: u64,
    sha256: String,
    owner_request_id: u64,
) -> Result<(), String> {
    if routes.active_transfer.is_some() || envelope_request_id != owner_request_id {
        return Err("invalid worker transfer declaration".to_string());
    }
    let payload = if let Some(pending) = routes.pending.get(&owner_request_id) {
        if !matches!(
            pending.expected,
            ExpectedResponse::Payload | ExpectedResponse::Mutation
        ) || total_bytes > pending.max_payload_bytes as u64
        {
            return Err("worker payload does not match the pending response type".to_string());
        }
        Some(Vec::with_capacity(total_bytes as usize))
    } else if routes.tombstone_set.contains(&owner_request_id) {
        None
    } else {
        return Err("worker transfer has no owning request".to_string());
    };
    routes.active_transfer = Some(IncomingTransfer {
        transfer_id,
        owner_request_id,
        total_bytes: total_bytes as usize,
        expected_sha256: sha256,
        received_bytes: 0,
        hasher: Sha256::new(),
        payload,
    });
    Ok(())
}

fn route_payload_chunk(
    routes: &mut ResponseRoutes,
    transfer_id: u64,
    bytes: Vec<u8>,
) -> Result<(), String> {
    let transfer = routes
        .active_transfer
        .as_mut()
        .ok_or_else(|| "worker payload has no active transfer".to_string())?;
    if transfer.transfer_id != transfer_id
        || bytes.len() > MAX_PAYLOAD_CHUNK_BYTES
        || transfer.received_bytes.saturating_add(bytes.len()) > transfer.total_bytes
    {
        return Err("worker payload chunk exceeds its transfer boundary".to_string());
    }
    transfer.received_bytes += bytes.len();
    transfer.hasher.update(&bytes);
    if let Some(payload) = transfer.payload.as_mut() {
        payload.extend_from_slice(&bytes);
    }
    Ok(())
}

fn complete_transfer(
    routes: &mut ResponseRoutes,
    envelope_request_id: u64,
    transfer_id: u64,
    total_bytes: u64,
    sha256: String,
) -> Result<(), String> {
    let transfer = routes
        .active_transfer
        .take()
        .ok_or_else(|| "worker completed a transfer that is not active".to_string())?;
    let actual_sha256 = format!("sha256:{:x}", transfer.hasher.finalize());
    if transfer.transfer_id != transfer_id
        || transfer.owner_request_id != envelope_request_id
        || transfer.total_bytes != total_bytes as usize
        || transfer.received_bytes != transfer.total_bytes
        || transfer.expected_sha256 != sha256
        || actual_sha256 != sha256
    {
        return Err("worker transfer completion does not match its declaration".to_string());
    }
    if let Some(payload) = transfer.payload {
        let pending = routes
            .pending
            .remove(&transfer.owner_request_id)
            .ok_or_else(|| "worker transfer owner is no longer pending".to_string())?;
        if !matches!(
            pending.expected,
            ExpectedResponse::Payload | ExpectedResponse::Mutation
        ) {
            return Err("worker payload does not match the pending response type".to_string());
        }
        let _ = pending.sender.send(Ok(RoutedResponse::Payload(payload)));
    } else {
        forget_tombstone(routes, transfer.owner_request_id);
    }
    Ok(())
}

fn route_control_response(
    routes: &mut ResponseRoutes,
    request_id: u64,
    message: Message,
) -> Result<(), String> {
    if let Message::MutationAccepted { resource_id } = &message {
        let pending = routes
            .pending
            .get(&request_id)
            .ok_or_else(|| "worker mutation acceptance has no owning request".to_string())?;
        if pending.expected != ExpectedResponse::Mutation {
            return Err(
                "worker mutation acceptance does not match its pending request".to_string(),
            );
        }
        let accepted = pending
            .accepted_resource
            .as_ref()
            .ok_or_else(|| "worker mutation acceptance state is unavailable".to_string())?;
        let mut accepted = accepted
            .lock()
            .map_err(|_| "worker mutation acceptance state is poisoned".to_string())?;
        if accepted.replace(resource_id.clone()).is_some() {
            return Err("worker accepted one mutation more than once".to_string());
        }
        return Ok(());
    }
    if routes
        .active_transfer
        .as_ref()
        .is_some_and(|transfer| transfer.owner_request_id == request_id)
    {
        return Err("worker control response interrupted an active transfer".to_string());
    }
    if routes.tombstone_set.contains(&request_id) {
        forget_tombstone(routes, request_id);
        return Ok(());
    }
    let pending = routes
        .pending
        .remove(&request_id)
        .ok_or_else(|| "worker response has no owning request".to_string())?;
    if pending.expected != ExpectedResponse::Control && !matches!(message, Message::Error { .. }) {
        return Err("worker control does not match the pending response type".to_string());
    }
    let _ = pending.sender.send(Ok(RoutedResponse::Control(message)));
    Ok(())
}

fn forget_tombstone(routes: &mut ResponseRoutes, request_id: u64) {
    routes.tombstone_set.remove(&request_id);
    routes
        .tombstones
        .retain(|tombstone| *tombstone != request_id);
}

fn fail_pending(routes: &Mutex<ResponseRoutes>, error: AppError) {
    let pending = {
        let mut routes = routes.lock().expect("worker response routes lock poisoned");
        routes.active_transfer = None;
        std::mem::take(&mut routes.pending)
    };
    for pending in pending.into_values() {
        let _ = pending.sender.send(Err(error.clone()));
    }
}

fn unavailable(distro_name: &str, message: impl Into<String>) -> AppError {
    AppError::EnvironmentUnavailable {
        environment: EnvironmentRef::Wsl {
            distro_name: distro_name.to_string(),
        },
        message: message.into(),
    }
}

fn payload_control_error(distro_name: &str, code: &str, phase: &str) -> AppError {
    match code {
        "deadlineExceeded" => AppError::WslCommandTimedOut,
        "pathUnavailable" if phase == "pathMapping" => AppError::CapabilityUnavailable {
            capability: "wslPathMapping".to_string(),
            path: None,
        },
        "libraryRecoveryIncomplete" => AppError::LibraryRecoveryIncomplete {
            environment: EnvironmentRef::Wsl {
                distro_name: distro_name.to_string(),
            },
            message: "WSL Skill Library recovery is incomplete".to_string(),
        },
        _ => AppError::ExecutionFailed {
            message: format!("worker request failed during {phase}: {code}"),
        },
    }
}

#[cfg(target_os = "windows")]
pub(super) async fn connect_worker(
    session: &super::WslSession,
    artifact_directory: &Path,
) -> Result<WorkerSession, AppError> {
    let artifact = WorkerArtifact::load_from(artifact_directory)
        .map_err(|message| unavailable(&session.distro_name, message))?;
    deploy_worker(session, &artifact).await?;

    let worker_path = worker_path(&session.home);
    let mut command = super::wsl_command();
    command
        .args([
            "--distribution",
            &session.distro_name,
            "--user",
            &session.user,
            "--exec",
            &worker_path,
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let child = command
        .spawn()
        .map_err(|error| unavailable(&session.distro_name, error.to_string()))?;
    let worker = WorkerSession::from_child(child, session.distro_name.clone())?;
    worker.handshake(session, &artifact.build_id).await?;
    Ok(worker)
}

#[cfg(not(target_os = "windows"))]
pub(super) async fn connect_worker(
    session: &super::WslSession,
    _artifact_directory: &Path,
) -> Result<WorkerSession, AppError> {
    Err(unavailable(
        &session.distro_name,
        "WSL worker is only available on Windows",
    ))
}

#[cfg(target_os = "windows")]
async fn deploy_worker(
    session: &super::WslSession,
    artifact: &WorkerArtifact,
) -> Result<(), AppError> {
    let output = WslCommandRunner::run(WslCommandRequest {
        session: session.clone(),
        script: BOOTSTRAP_SCRIPT,
        args: vec![artifact.build_id.clone()],
        stdin: artifact.bytes.clone(),
        timeout: WORKER_START_TIMEOUT,
        stdout_limit: DEFAULT_WSL_STDOUT_LIMIT,
        stderr_limit: DEFAULT_WSL_STDERR_LIMIT,
        cancellation: None,
    })
    .await?;
    if output.exit_code != Some(0) {
        return Err(unavailable(
            &session.distro_name,
            String::from_utf8_lossy(&output.stderr).trim().to_string(),
        ));
    }
    Ok(())
}

#[cfg(target_os = "windows")]
fn worker_path(home: &str) -> String {
    if home == "/" {
        "/.skill-deck/runtime/wsl-worker/current/worker".to_string()
    } else {
        format!(
            "{}/.skill-deck/runtime/wsl-worker/current/worker",
            home.trim_end_matches('/')
        )
    }
}

#[cfg(target_os = "windows")]
async fn drain_worker_stderr(mut stderr: tokio::process::ChildStderr, distro_name: &str) {
    let mut retained = Vec::new();
    let mut buffer = [0u8; 4096];
    loop {
        match stderr.read(&mut buffer).await {
            Ok(0) => break,
            Ok(read) if retained.len() < WORKER_STDERR_LIMIT => {
                let keep = read.min(WORKER_STDERR_LIMIT - retained.len());
                retained.extend_from_slice(&buffer[..keep]);
            }
            Ok(_) => {}
            Err(error) => {
                log::warn!("failed to drain WSL worker stderr for {distro_name}: {error}");
                return;
            }
        }
    }
    if !retained.is_empty() {
        log::warn!(
            "WSL worker stderr for {distro_name}: {}",
            String::from_utf8_lossy(&retained).trim()
        );
    }
}

#[cfg(test)]
#[allow(
    clippy::disallowed_methods,
    reason = "Worker bootstrap 测试需要直接运行受控的 POSIX shell"
)]
mod tests {
    use std::fs;
    #[cfg(unix)]
    use std::io::Write;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;
    #[cfg(unix)]
    use std::path::Path;
    #[cfg(unix)]
    use std::process::{Command, Stdio};

    use environment_protocol::{
        codec, decode, spawn_writer, Envelope, InspectionRequest, Message, WireRecord,
    };
    use futures_util::StreamExt;
    use sha2::{Digest, Sha256};
    use tokio::io::{duplex, split};
    use tokio::time::Duration;
    use tokio_util::codec::FramedRead;

    use super::{payload_control_error, WorkerSession};
    use crate::core::mutation::CancellationSignal;
    use crate::environment::wsl::WslSession;

    #[test]
    fn library_recovery_payload_error_preserves_its_typed_contract() {
        assert!(matches!(
            payload_control_error("Ubuntu", "libraryRecoveryIncomplete", "libraryRead"),
            crate::error::AppError::LibraryRecoveryIncomplete {
                environment: crate::environment::types::EnvironmentRef::Wsl { distro_name },
                ..
            } if distro_name == "Ubuntu"
        ));
    }

    #[test]
    fn path_mapping_payload_error_preserves_its_typed_contract() {
        assert_eq!(
            payload_control_error("Ubuntu", "pathUnavailable", "pathMapping"),
            crate::error::AppError::CapabilityUnavailable {
                capability: "wslPathMapping".to_string(),
                path: None,
            }
        );
    }

    #[test]
    fn artifact_loader_rejects_bytes_that_do_not_match_the_manifest() {
        let directory = tempfile::tempdir().unwrap();
        fs::write(directory.path().join("worker"), b"worker").unwrap();
        fs::write(
            directory.path().join("manifest.json"),
            r#"{
              "buildId": "sha256:87eba76e7f3164534045ba922e7770fb58bbd14ad732bbf5ba6f11cc56989e6e",
              "sha256": "sha256:87eba76e7f3164534045ba922e7770fb58bbd14ad732bbf5ba6f11cc56989e6e",
              "target": "x86_64-unknown-linux-musl"
            }"#,
        )
        .unwrap();

        let artifact = super::WorkerArtifact::load_from(directory.path()).unwrap();
        assert_eq!(artifact.bytes, b"worker");
        assert_eq!(
            artifact.build_id,
            "sha256:87eba76e7f3164534045ba922e7770fb58bbd14ad732bbf5ba6f11cc56989e6e"
        );

        fs::write(directory.path().join("worker"), b"damaged").unwrap();
        let error = super::WorkerArtifact::load_from(directory.path()).unwrap_err();
        assert!(error.contains("do not match"), "{error}");
    }

    #[test]
    fn artifact_loader_does_not_search_sibling_directories() {
        let root = tempfile::tempdir().unwrap();
        let sibling = root.path().join("sibling");
        fs::create_dir(&sibling).unwrap();
        fs::write(sibling.join("worker"), b"worker").unwrap();
        fs::write(
            sibling.join("manifest.json"),
            r#"{
              "buildId": "sha256:87eba76e7f3164534045ba922e7770fb58bbd14ad732bbf5ba6f11cc56989e6e",
              "sha256": "sha256:87eba76e7f3164534045ba922e7770fb58bbd14ad732bbf5ba6f11cc56989e6e",
              "target": "x86_64-unknown-linux-musl"
            }"#,
        )
        .unwrap();

        assert!(super::WorkerArtifact::load_from(&root.path().join("explicit")).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn bootstrap_script_verifies_and_atomically_installs_worker() {
        use std::os::unix::fs::MetadataExt;

        let home = tempfile::tempdir().unwrap();
        assert!(run_bootstrap(
            home.path(),
            b"worker",
            "sha256:87eba76e7f3164534045ba922e7770fb58bbd14ad732bbf5ba6f11cc56989e6e",
        )
        .success());

        let installed = home
            .path()
            .join(".skill-deck/runtime/wsl-worker/current/worker");
        assert_eq!(fs::read(&installed).unwrap(), b"worker");
        assert_eq!(
            fs::metadata(&installed).unwrap().permissions().mode() & 0o777,
            0o700
        );

        let inode = fs::metadata(&installed).unwrap().ino();
        assert!(run_bootstrap(
            home.path(),
            b"worker",
            "sha256:87eba76e7f3164534045ba922e7770fb58bbd14ad732bbf5ba6f11cc56989e6e",
        )
        .success());
        assert_eq!(fs::metadata(&installed).unwrap().ino(), inode);

        assert!(!run_bootstrap(
            home.path(),
            b"damaged",
            "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        )
        .success());
        assert_eq!(fs::read(&installed).unwrap(), b"worker");
        assert_eq!(
            fs::read_dir(installed.parent().unwrap()).unwrap().count(),
            1
        );
    }

    #[cfg(unix)]
    #[test]
    fn bootstrap_switches_worker_builds_in_both_directions() {
        let home = tempfile::tempdir().unwrap();
        let worker = home
            .path()
            .join(".skill-deck/runtime/wsl-worker/current/worker");
        for (bytes, digest) in [
            (
                b"worker-a".as_slice(),
                "sha256:6a65e237ae44c42895b5049031466616173469f513b00240e9db90df66906a49",
            ),
            (
                b"worker-b".as_slice(),
                "sha256:0482c4aea1af397e54200017866d778ce32e1daac473f2cd09a9159d2e69b6ef",
            ),
            (
                b"worker-a".as_slice(),
                "sha256:6a65e237ae44c42895b5049031466616173469f513b00240e9db90df66906a49",
            ),
        ] {
            assert!(run_bootstrap(home.path(), bytes, digest).success());
            assert_eq!(fs::read(&worker).unwrap(), bytes);
        }
    }

    #[cfg(unix)]
    #[test]
    fn bootstrap_rejects_non_file_targets_without_following_them() {
        use std::os::unix::fs::symlink;

        for target_kind in ["directory", "symlink"] {
            let home = tempfile::tempdir().unwrap();
            let directory = home.path().join(".skill-deck/runtime/wsl-worker/current");
            fs::create_dir_all(&directory).unwrap();
            let worker = directory.join("worker");
            if target_kind == "directory" {
                fs::create_dir(&worker).unwrap();
            } else {
                let external = home.path().join("external");
                fs::write(&external, b"external").unwrap();
                symlink(&external, &worker).unwrap();
            }

            assert!(!run_bootstrap(
                home.path(),
                b"worker",
                "sha256:87eba76e7f3164534045ba922e7770fb58bbd14ad732bbf5ba6f11cc56989e6e",
            )
            .success());
            assert!(fs::symlink_metadata(&worker).is_ok());
            assert_eq!(
                fs::read_dir(&directory)
                    .unwrap()
                    .filter_map(Result::ok)
                    .count(),
                1
            );
        }
    }

    #[cfg(unix)]
    fn run_bootstrap(home: &Path, bytes: &[u8], digest: &str) -> std::process::ExitStatus {
        let mut child = Command::new("/bin/sh")
            .args(["-c", super::BOOTSTRAP_SCRIPT, "--", digest])
            .env("HOME", home)
            .stdin(Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(bytes).unwrap();
        child.wait().unwrap()
    }

    #[tokio::test]
    async fn concurrent_requests_receive_responses_by_request_id() {
        let (client, server) = duplex(4096);
        let (client_reader, client_writer) = split(client);
        let session = WorkerSession::from_io(client_reader, client_writer, "Ubuntu".to_string());
        let server_task = tokio::spawn(async move {
            let (server_reader, server_writer) = split(server);
            let mut reader = FramedRead::new(server_reader, codec());
            let (writer, writer_task) = spawn_writer(server_writer);
            let first = next_envelope(&mut reader).await;
            let second = next_envelope(&mut reader).await;
            for request in [second, first] {
                writer
                    .send_control(WireRecord::Control(Envelope {
                        request_id: request.request_id,
                        message: Message::PathObserved {
                            kind: environment_protocol::PathKind::Directory,
                        },
                    }))
                    .await
                    .unwrap();
            }
            drop(writer);
            writer_task.await.unwrap().unwrap();
        });

        let first = session.request(
            Message::ObservePath {
                path: "/home/alice".to_string(),
            },
            Duration::from_secs(1),
        );
        let second = session.request(
            Message::ObservePath {
                path: "/tmp".to_string(),
            },
            Duration::from_secs(1),
        );
        let (first, second) = tokio::join!(first, second);

        assert!(matches!(first.unwrap(), Message::PathObserved { .. }));
        assert!(matches!(second.unwrap(), Message::PathObserved { .. }));
        server_task.await.unwrap();
    }

    #[tokio::test]
    async fn payload_response_is_bounded_reassembled_and_verified() {
        let (client, server) = duplex(4096);
        let (client_reader, client_writer) = split(client);
        let session = WorkerSession::from_io(client_reader, client_writer, "Ubuntu".to_string());
        let payload = vec![0x5a; 3073];
        let expected = payload.clone();
        let server_task = tokio::spawn(async move {
            let (server_reader, server_writer) = split(server);
            let mut reader = FramedRead::new(server_reader, codec());
            let (writer, writer_task) = spawn_writer(server_writer);
            let request = next_envelope(&mut reader).await;
            assert!(matches!(request.message, Message::InspectFilesystem { .. }));
            writer
                .send_transfer(request.request_id, 91, &payload)
                .await
                .unwrap();
            drop(writer);
            writer_task.await.unwrap().unwrap();
        });

        let actual = session
            .request_payload(inspection_request(), Duration::from_secs(1))
            .await
            .unwrap();

        assert_eq!(actual, expected);
        server_task.await.unwrap();
    }

    #[tokio::test]
    async fn payload_response_uses_the_pending_requests_specific_limit() {
        let (client, server) = duplex(4096);
        let (client_reader, client_writer) = split(client);
        let session = WorkerSession::from_io(client_reader, client_writer, "Ubuntu".to_string());
        let server_task = tokio::spawn(async move {
            let (server_reader, server_writer) = split(server);
            let mut reader = FramedRead::new(server_reader, codec());
            let (writer, writer_task) = spawn_writer(server_writer);
            let request = next_envelope(&mut reader).await;
            writer
                .send_transfer(request.request_id, 94, b"four")
                .await
                .unwrap();
            drop(writer);
            writer_task.await.unwrap().unwrap();
        });

        let error = session
            .request_payload_with_limit(inspection_request(), Duration::from_secs(1), 3, None)
            .await
            .unwrap_err();

        assert!(matches!(
            error,
            crate::error::AppError::EnvironmentUnavailable { .. }
        ));
        server_task.await.unwrap();
    }

    #[tokio::test]
    async fn prepared_inbound_transfer_uses_a_fresh_owner_request() {
        let (client, server) = duplex(4096);
        let (client_reader, client_writer) = split(client);
        let session = WorkerSession::from_io(client_reader, client_writer, "Ubuntu".to_string());
        let server_task = tokio::spawn(async move {
            let (server_reader, server_writer) = split(server);
            let mut reader = FramedRead::new(server_reader, codec());
            let (writer, writer_task) = spawn_writer(server_writer);
            let declaration = next_envelope(&mut reader).await;
            let owner_request_id = declaration.request_id;
            assert!(matches!(
                declaration.message,
                Message::BeginTransfer {
                    transfer_id: 700,
                    owner_request_id: owner,
                    ..
                } if owner == owner_request_id
            ));
            assert!(matches!(
                decode(&reader.next().await.unwrap().unwrap()).unwrap(),
                WireRecord::PayloadChunk {
                    transfer_id: 700,
                    ..
                }
            ));
            let completion = next_envelope(&mut reader).await;
            assert_eq!(completion.request_id, owner_request_id);
            writer
                .send_control(WireRecord::Control(Envelope {
                    request_id: owner_request_id,
                    message: Message::PayloadBlobUploaded {
                        upload_id: 7,
                        blob_id: "a".repeat(64),
                    },
                }))
                .await
                .unwrap();
            drop(writer);
            writer_task.await.unwrap().unwrap();
        });

        let response = session
            .send_prepared_transfer(700, b"blob", 16, Duration::from_secs(1))
            .await
            .unwrap();

        assert!(matches!(
            response,
            Message::PayloadBlobUploaded { upload_id: 7, .. }
        ));
        server_task.await.unwrap();
    }

    #[tokio::test]
    async fn mutation_acceptance_keeps_the_pending_route_until_terminal_payload() {
        let (client, server) = duplex(4096);
        let (client_reader, client_writer) = split(client);
        let session = WorkerSession::from_io(client_reader, client_writer, "Ubuntu".to_string());
        let server_task = tokio::spawn(async move {
            let (server_reader, server_writer) = split(server);
            let mut reader = FramedRead::new(server_reader, codec());
            let (writer, writer_task) = spawn_writer(server_writer);
            let declaration = next_envelope(&mut reader).await;
            let owner_request_id = declaration.request_id;
            assert!(matches!(declaration.message, Message::BeginTransfer { .. }));
            assert!(matches!(
                decode(&reader.next().await.unwrap().unwrap()).unwrap(),
                WireRecord::PayloadChunk { .. }
            ));
            let _completion = next_envelope(&mut reader).await;
            writer
                .send_control(WireRecord::Control(Envelope {
                    request_id: owner_request_id,
                    message: Message::MutationAccepted {
                        resource_id: "resource-1".to_string(),
                    },
                }))
                .await
                .unwrap();
            let outcome = environment_protocol::encode_payload(
                &environment_protocol::MutationUnitOutcome::Cancelled,
            )
            .unwrap();
            writer
                .send_transfer(owner_request_id, 701, &outcome)
                .await
                .unwrap();
            drop(writer);
            writer_task.await.unwrap().unwrap();
        });

        let outcome = session
            .send_prepared_mutation(
                700,
                b"request",
                CancellationSignal::default(),
                Duration::from_secs(1),
            )
            .await
            .map_err(|error| error.error)
            .unwrap();

        assert_eq!(
            outcome,
            environment_protocol::MutationUnitOutcome::Cancelled
        );
        server_task.await.unwrap();
    }

    #[tokio::test]
    async fn mutation_disconnect_reports_whether_the_worker_accepted_it() {
        for accepted in [false, true] {
            let (client, server) = duplex(4096);
            let (client_reader, client_writer) = split(client);
            let session =
                WorkerSession::from_io(client_reader, client_writer, "Ubuntu".to_string());
            let server_task = tokio::spawn(async move {
                let (server_reader, server_writer) = split(server);
                let mut reader = FramedRead::new(server_reader, codec());
                let (writer, writer_task) = spawn_writer(server_writer);
                let declaration = next_envelope(&mut reader).await;
                let owner_request_id = declaration.request_id;
                let _chunk = reader.next().await;
                let _completion = next_envelope(&mut reader).await;
                if accepted {
                    writer
                        .send_control(WireRecord::Control(Envelope {
                            request_id: owner_request_id,
                            message: Message::MutationAccepted {
                                resource_id: "resource-accepted".to_string(),
                            },
                        }))
                        .await
                        .unwrap();
                }
                drop(writer);
                writer_task.await.unwrap().unwrap();
            });

            let error = session
                .send_prepared_mutation(
                    710,
                    b"request",
                    CancellationSignal::default(),
                    Duration::from_secs(1),
                )
                .await
                .unwrap_err();

            assert_eq!(
                error.accepted_resource_id.as_deref(),
                accepted.then_some("resource-accepted")
            );
            server_task.await.unwrap();
        }
    }

    #[tokio::test]
    async fn mutation_cancellation_waits_for_the_worker_terminal_outcome() {
        let (client, server) = duplex(4096);
        let (client_reader, client_writer) = split(client);
        let session = WorkerSession::from_io(client_reader, client_writer, "Ubuntu".to_string());
        let server_task = tokio::spawn(async move {
            let (server_reader, server_writer) = split(server);
            let mut reader = FramedRead::new(server_reader, codec());
            let (writer, writer_task) = spawn_writer(server_writer);
            let declaration = next_envelope(&mut reader).await;
            let owner_request_id = declaration.request_id;
            let _chunk = reader.next().await;
            let _completion = next_envelope(&mut reader).await;
            writer
                .send_control(WireRecord::Control(Envelope {
                    request_id: owner_request_id,
                    message: Message::MutationAccepted {
                        resource_id: "resource-cancelled".to_string(),
                    },
                }))
                .await
                .unwrap();
            let cancel = next_envelope(&mut reader).await;
            assert!(matches!(
                cancel.message,
                Message::Cancel { target_request_id } if target_request_id == owner_request_id
            ));
            let outcome = environment_protocol::encode_payload(
                &environment_protocol::MutationUnitOutcome::Cancelled,
            )
            .unwrap();
            writer
                .send_transfer(owner_request_id, 712, &outcome)
                .await
                .unwrap();
            drop(writer);
            writer_task.await.unwrap().unwrap();
        });
        let cancellation = CancellationSignal::default();
        let cancellation_task = {
            let cancellation = cancellation.clone();
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_millis(20)).await;
                cancellation.cancel();
            })
        };

        let outcome = session
            .send_prepared_mutation(711, b"request", cancellation, Duration::from_secs(1))
            .await
            .map_err(|error| error.error)
            .unwrap();

        assert_eq!(
            outcome,
            environment_protocol::MutationUnitOutcome::Cancelled
        );
        cancellation_task.await.unwrap();
        server_task.await.unwrap();
    }

    #[tokio::test]
    async fn mismatched_payload_digest_invalidates_the_session() {
        let (client, server) = duplex(4096);
        let (client_reader, client_writer) = split(client);
        let session = WorkerSession::from_io(client_reader, client_writer, "Ubuntu".to_string());
        let mut closed = session.closed_receiver();
        let server_task = tokio::spawn(async move {
            let (server_reader, server_writer) = split(server);
            let mut reader = FramedRead::new(server_reader, codec());
            let (writer, writer_task) = spawn_writer(server_writer);
            let request = next_envelope(&mut reader).await;
            writer
                .send_binary(WireRecord::Control(Envelope {
                    request_id: request.request_id,
                    message: Message::BeginTransfer {
                        transfer_id: 92,
                        total_bytes: 3,
                        sha256: "sha256:incorrect".to_string(),
                        owner_request_id: request.request_id,
                    },
                }))
                .await
                .unwrap();
            writer
                .send_binary(WireRecord::PayloadChunk {
                    transfer_id: 92,
                    bytes: b"abc".to_vec(),
                })
                .await
                .unwrap();
            writer
                .send_binary_barrier(WireRecord::Control(Envelope {
                    request_id: request.request_id,
                    message: Message::TransferCompleted {
                        transfer_id: 92,
                        total_bytes: 3,
                        sha256: "sha256:incorrect".to_string(),
                    },
                }))
                .await
                .unwrap();
            drop(writer);
            writer_task.await.unwrap().unwrap();
        });

        let error = session
            .request_payload(inspection_request(), Duration::from_secs(1))
            .await
            .unwrap_err();

        assert!(matches!(
            error,
            crate::error::AppError::EnvironmentUnavailable { .. }
        ));
        tokio::time::timeout(Duration::from_secs(1), closed.changed())
            .await
            .unwrap()
            .unwrap();
        assert!(*closed.borrow());
        server_task.await.unwrap();
    }

    #[tokio::test]
    async fn timed_out_payload_transfer_is_drained_before_the_session_is_reused() {
        let (client, server) = duplex(4096);
        let (client_reader, client_writer) = split(client);
        let session = WorkerSession::from_io(client_reader, client_writer, "Ubuntu".to_string());
        let server_task = tokio::spawn(async move {
            let (server_reader, server_writer) = split(server);
            let mut reader = FramedRead::new(server_reader, codec());
            let (writer, writer_task) = spawn_writer(server_writer);
            let expired = next_envelope(&mut reader).await;
            let digest = format!("sha256:{:x}", Sha256::digest(b"abcdef"));
            writer
                .send_binary(WireRecord::Control(Envelope {
                    request_id: expired.request_id,
                    message: Message::BeginTransfer {
                        transfer_id: 93,
                        total_bytes: 6,
                        sha256: digest.clone(),
                        owner_request_id: expired.request_id,
                    },
                }))
                .await
                .unwrap();
            writer
                .send_binary(WireRecord::PayloadChunk {
                    transfer_id: 93,
                    bytes: b"abc".to_vec(),
                })
                .await
                .unwrap();
            tokio::time::sleep(Duration::from_millis(30)).await;
            let cancel = next_envelope(&mut reader).await;
            assert!(matches!(
                cancel.message,
                Message::Cancel { target_request_id } if target_request_id == expired.request_id
            ));
            writer
                .send_binary(WireRecord::PayloadChunk {
                    transfer_id: 93,
                    bytes: b"def".to_vec(),
                })
                .await
                .unwrap();
            writer
                .send_binary_barrier(WireRecord::Control(Envelope {
                    request_id: expired.request_id,
                    message: Message::TransferCompleted {
                        transfer_id: 93,
                        total_bytes: 6,
                        sha256: digest,
                    },
                }))
                .await
                .unwrap();
            let current = next_envelope(&mut reader).await;
            writer
                .send_control(WireRecord::Control(Envelope {
                    request_id: current.request_id,
                    message: Message::PathObserved {
                        kind: environment_protocol::PathKind::Directory,
                    },
                }))
                .await
                .unwrap();
            drop(writer);
            writer_task.await.unwrap().unwrap();
        });

        assert_eq!(
            session
                .request_payload(inspection_request(), Duration::from_millis(10))
                .await
                .unwrap_err(),
            crate::error::AppError::WslCommandTimedOut,
        );
        assert!(matches!(
            session
                .request(
                    Message::ObservePath {
                        path: "/current".to_string(),
                    },
                    Duration::from_secs(1),
                )
                .await
                .unwrap(),
            Message::PathObserved {
                kind: environment_protocol::PathKind::Directory,
            }
        ));
        server_task.await.unwrap();
    }

    #[tokio::test]
    async fn timed_out_request_is_cancelled_and_its_late_response_is_ignored() {
        let (client, server) = duplex(4096);
        let (client_reader, client_writer) = split(client);
        let session = WorkerSession::from_io(client_reader, client_writer, "Ubuntu".to_string());
        let server_task = tokio::spawn(async move {
            let (server_reader, server_writer) = split(server);
            let mut reader = FramedRead::new(server_reader, codec());
            let (writer, writer_task) = spawn_writer(server_writer);
            let expired = next_envelope(&mut reader).await;
            let cancel = next_envelope(&mut reader).await;
            assert!(matches!(
                cancel.message,
                Message::Cancel { target_request_id } if target_request_id == expired.request_id
            ));
            writer
                .send_control(WireRecord::Control(Envelope {
                    request_id: expired.request_id,
                    message: Message::PathObserved {
                        kind: environment_protocol::PathKind::Missing,
                    },
                }))
                .await
                .unwrap();
            let current = next_envelope(&mut reader).await;
            writer
                .send_control(WireRecord::Control(Envelope {
                    request_id: current.request_id,
                    message: Message::PathObserved {
                        kind: environment_protocol::PathKind::Directory,
                    },
                }))
                .await
                .unwrap();
            drop(writer);
            writer_task.await.unwrap().unwrap();
        });

        assert_eq!(
            session
                .request(
                    Message::ObservePath {
                        path: "/expired".to_string(),
                    },
                    Duration::from_millis(10),
                )
                .await
                .unwrap_err(),
            crate::error::AppError::WslCommandTimedOut,
        );
        assert!(matches!(
            session
                .request(
                    Message::ObservePath {
                        path: "/current".to_string(),
                    },
                    Duration::from_secs(1),
                )
                .await
                .unwrap(),
            Message::PathObserved {
                kind: environment_protocol::PathKind::Directory,
            }
        ));
        server_task.await.unwrap();
    }

    #[tokio::test]
    async fn handshake_confirms_identity_and_home_directory() {
        let (client, server) = duplex(4096);
        let (client_reader, client_writer) = split(client);
        let session = WorkerSession::from_io(client_reader, client_writer, "Ubuntu".to_string());
        let server_task = tokio::spawn(async move {
            let (server_reader, server_writer) = split(server);
            let mut reader = FramedRead::new(server_reader, codec());
            let (writer, writer_task) = spawn_writer(server_writer);
            let handshake = next_envelope(&mut reader).await;
            assert!(matches!(
                handshake.message,
                Message::Handshake { ref build_id } if build_id == "sha256:build"
            ));
            writer
                .send_control(WireRecord::Control(Envelope {
                    request_id: handshake.request_id,
                    message: Message::HandshakeResult {
                        build_id: "sha256:build".to_string(),
                        distro: "Ubuntu".to_string(),
                        user: "alice".to_string(),
                        uid: 1000,
                        home: "/home/alice".to_string(),
                    },
                }))
                .await
                .unwrap();
            let home = next_envelope(&mut reader).await;
            assert!(matches!(
                home.message,
                Message::ObservePath { ref path } if path == "/home/alice"
            ));
            writer
                .send_control(WireRecord::Control(Envelope {
                    request_id: home.request_id,
                    message: Message::PathObserved {
                        kind: environment_protocol::PathKind::Directory,
                    },
                }))
                .await
                .unwrap();
            drop(writer);
            writer_task.await.unwrap().unwrap();
        });
        let expected = WslSession {
            distro_name: "Ubuntu".to_string(),
            user: "alice".to_string(),
            uid: 1000,
            home: "/home/alice".to_string(),
            xdg_state_home: None,
            config_home: "/home/alice/.config".to_string(),
            environment: Default::default(),
            runtime_generation: 0,
        };

        session.handshake(&expected, "sha256:build").await.unwrap();
        server_task.await.unwrap();
    }

    #[tokio::test]
    async fn closing_worker_output_notifies_the_session_owner() {
        let (client, server) = duplex(4096);
        let (client_reader, client_writer) = split(client);
        let session = WorkerSession::from_io(client_reader, client_writer, "Ubuntu".to_string());
        let mut closed = session.closed_receiver();

        drop(server);

        tokio::time::timeout(Duration::from_secs(1), closed.changed())
            .await
            .unwrap()
            .unwrap();
        assert!(*closed.borrow());
    }

    async fn next_envelope<R>(
        reader: &mut FramedRead<R, tokio_util::codec::LengthDelimitedCodec>,
    ) -> Envelope
    where
        R: tokio::io::AsyncRead + Unpin,
    {
        match decode(&reader.next().await.unwrap().unwrap()).unwrap() {
            WireRecord::Control(envelope) => envelope,
            WireRecord::PayloadChunk { .. } => panic!("unexpected payload chunk"),
        }
    }

    fn inspection_request() -> Message {
        Message::InspectFilesystem {
            request: InspectionRequest {
                roots: Vec::new(),
                per_file_limit: 1,
                aggregate_limit: 1,
                deadline_millis: 1_000,
            },
        }
    }
}

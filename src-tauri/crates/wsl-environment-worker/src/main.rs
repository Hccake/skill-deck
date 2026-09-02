use std::collections::hash_map::Entry;
use std::collections::{HashMap, VecDeque};
use std::error::Error;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use environment_protocol::{
    codec, decode, decode_payload, encode_inspection_response, encode_payload, spawn_writer,
    Envelope, MapHostPathsRequest, MapHostPathsResponse, Message, ProtocolWriter, WireRecord,
    MAX_CONCURRENT_READ_REQUESTS, MAX_DOCUMENT_BYTES, MAX_INSPECTION_ROOTS,
    MAX_MUTATION_TRANSFER_BYTES, MAX_PAYLOAD_TRANSFER_BYTES, MAX_PENDING_READ_REQUESTS,
    MAX_REQUEST_DEADLINE_MILLIS,
};
use futures_util::StreamExt;
use sha2::Digest;
use tokio::task::JoinSet;
use tokio_util::codec::FramedRead;
use wsl_environment_worker::inbound_transfer::{
    InboundTransfer, TransferCompletion, TransferDeclaration,
};
use wsl_environment_worker::library::{LibraryError, LibraryManager};
use wsl_environment_worker::mutation::MutationManager;
use wsl_environment_worker::payload::{PayloadError, PayloadManager, PreparedPayloadFile};
use wsl_environment_worker::source::{
    probe_git, scan_source, GitSourceOptions, SourceError, SourceManager,
};
use wsl_environment_worker::{
    error_message, execute_directory_count, execute_directory_list, execute_document_read,
    execute_entry_facts, execute_inspection, execute_manifest, execute_map_windows_paths,
    execute_path_metadata, execute_path_observation, execute_projection, file_sha256, RequestError,
    WorkerIdentity, WorkerRuntime,
};

struct QueuedRequest {
    request_id: u64,
    message: Message,
    cancelled: Arc<AtomicBool>,
}

enum InboundAction {
    Blob {
        upload_id: u64,
        blob_id: String,
    },
    Manifest {
        upload_id: u64,
    },
    Mutation {
        resource_id: String,
    },
    Document {
        request: environment_protocol::DocumentWritePreparation,
        started: Instant,
    },
    Library {
        deadline_millis: u64,
        started: Instant,
    },
}

struct PreparedInbound {
    transfer_id: u64,
    total_bytes: u64,
    sha256: String,
    transfer_limit: u64,
    path: PathBuf,
    file: std::fs::File,
    action: InboundAction,
}

struct ActiveInbound {
    owner_request_id: u64,
    path: PathBuf,
    action: InboundAction,
    transfer: InboundTransfer,
}

struct LibraryExecution {
    deadline_millis: u64,
    started: Instant,
    path: PathBuf,
}

const MAX_MANIFEST_TRANSFER_BYTES: u64 = 8 * 1024 * 1024;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let executable = std::env::current_exe()?;
    let home = std::env::var("HOME").unwrap_or_default();
    let runtime = WorkerRuntime::new(
        file_sha256(&executable)?,
        WorkerIdentity {
            distro: std::env::var("WSL_DISTRO_NAME").unwrap_or_default(),
            user: std::env::var("USER").unwrap_or_default(),
            uid: effective_user_id(),
            home: home.clone(),
        },
    );
    let mut reader = FramedRead::new(tokio::io::stdin(), codec());
    let (writer, writer_task) = spawn_writer(tokio::io::stdout());
    let mut queue = VecDeque::new();
    let mut active = HashMap::<u64, Arc<AtomicBool>>::new();
    let mut tasks = JoinSet::new();
    let sources = Arc::new(tokio::sync::Mutex::new(SourceManager::new(PathBuf::from(
        "/tmp",
    ))?));
    let payloads = Arc::new(tokio::sync::Mutex::new(PayloadManager::new(
        PathBuf::from("/tmp"),
    )?));
    let mutations = Arc::new(tokio::sync::Mutex::new(MutationManager::new(
        PathBuf::from("/tmp"),
    )?));
    let libraries = Arc::new(tokio::sync::Mutex::new(LibraryManager::new(PathBuf::from(
        home,
    ))));
    let documents = Arc::new(tokio::sync::Mutex::new(()));
    let mut shutting_down = false;
    let mut prepared_inbound: Option<PreparedInbound> = None;
    let mut active_inbound: Option<ActiveInbound> = None;
    let mut next_inbound_transfer_id = 1_u64;

    loop {
        while tasks.len() < MAX_CONCURRENT_READ_REQUESTS {
            let Some(request) = queue.pop_front() else {
                break;
            };
            let task_writer = writer.clone();
            tasks.spawn(execute_business_request(
                request,
                task_writer,
                Arc::clone(&sources),
                Arc::clone(&payloads),
                Arc::clone(&mutations),
                Arc::clone(&libraries),
            ));
        }

        if shutting_down && tasks.is_empty() {
            break;
        }

        tokio::select! {
            completed = tasks.join_next(), if !tasks.is_empty() => {
                match completed {
                    Some(Ok(Ok(request_id))) => {
                        active.remove(&request_id);
                    }
                    Some(Ok(Err(error))) => return Err(error.into()),
                    Some(Err(error)) => return Err(error.into()),
                    None => {}
                }
            }
            frame = reader.next(), if !shutting_down => {
                let Some(frame) = frame else {
                    cancel_all(&active);
                    queue.clear();
                    discard_inbound(&mut prepared_inbound, &mut active_inbound).await;
                    shutting_down = true;
                    continue;
                };
                let record = decode(&frame?)?;
                if let WireRecord::PayloadChunk { transfer_id, bytes } = record {
                    let inbound = active_inbound
                        .as_mut()
                        .ok_or("worker received a payload without an owning request")?;
                    inbound.transfer.write_chunk(transfer_id, &bytes).await?;
                    continue;
                }
                let WireRecord::Control(envelope) = record else { unreachable!() };
                let request_id = envelope.request_id;
                match envelope.message {
                    message @ (Message::ObservePath { .. }
                    | Message::InspectFilesystem { .. }
                    | Message::InspectPaths { .. }
                    | Message::CountDirectoryEntries { .. }
                    | Message::ReadDocuments { .. }
                    | Message::ReadLibraryCatalog { .. }
                    | Message::ListChildDirectories { .. }
                    | Message::MapPathsToWindows { .. }
                    | Message::MapHostPaths { .. }
                    | Message::InspectEntries { .. }
                    | Message::ProjectTargets { .. }
                    | Message::BuildManifest { .. }
                    | Message::AcquireGitSource { .. }
                    | Message::OpenLocalSource { .. }
                    | Message::ReleaseSource { .. }
                    | Message::ScanSource { .. }
                    | Message::SourceFingerprint { .. }
                    | Message::SourceRevision { .. }
                    | Message::ProbeGit { .. }
                    | Message::AcquirePayloadFromSource { .. }
                    | Message::VerifyPayload { .. }
                    | Message::ReadPayloadBlob { .. }
                    | Message::RemovePayload { .. }
                    | Message::RemovePayloadSession { .. }
                    | Message::SweepPayloadOrphans { .. }
                    | Message::BeginPayloadUpload { .. }
                    | Message::AcknowledgeMutationUnit { .. }
                    | Message::ListMutationRecovery
                    | Message::CleanupMutationRecovery { .. }) => {
                        match active.entry(request_id) {
                            Entry::Occupied(_) => {
                                send_error(&writer, request_id, "duplicateRequest", "request").await?;
                            }
                            Entry::Vacant(_) if queue.len() >= MAX_PENDING_READ_REQUESTS => {
                                send_error(&writer, request_id, "workerBusy", "admission").await?;
                            }
                            Entry::Vacant(entry) => {
                                let cancelled = Arc::new(AtomicBool::new(false));
                                entry.insert(Arc::clone(&cancelled));
                                queue.push_back(QueuedRequest {
                                    request_id,
                                    message,
                                    cancelled,
                                });
                            }
                        }
                    }
                    Message::PrepareDocumentWrite { request: preparation } => {
                        if prepared_inbound.is_some() || active_inbound.is_some() {
                            send_error(&writer, request_id, "workerBusy", "inboundTransfer").await?;
                            continue;
                        }
                        if !preparation.path.starts_with('/')
                            || preparation.path.ends_with('/')
                            || preparation.total_bytes == 0
                            || preparation.total_bytes > MAX_DOCUMENT_BYTES as u64
                            || !valid_transfer_sha256(&preparation.sha256)
                            || preparation
                                .expected_revision
                                .as_deref()
                                .is_some_and(|revision| !valid_transfer_sha256(revision))
                            || preparation.deadline_millis == 0
                            || preparation.deadline_millis
                                > environment_protocol::MAX_REQUEST_DEADLINE_MILLIS
                        {
                            send_error(&writer, request_id, "invalidRequest", "documentWrite").await?;
                            continue;
                        }
                        let path = PathBuf::from(format!(
                            "/tmp/.skill-deck-document-request-{}-{}",
                            std::process::id(),
                            request_id
                        ));
                        let file = match std::fs::OpenOptions::new()
                            .write(true)
                            .create_new(true)
                            .open(&path)
                        {
                            Ok(file) => file,
                            Err(_) => {
                                send_error(&writer, request_id, "workerBusy", "documentWrite").await?;
                                continue;
                            }
                        };
                        #[cfg(unix)]
                        {
                            use std::os::unix::fs::PermissionsExt;
                            if std::fs::set_permissions(
                                &path,
                                std::fs::Permissions::from_mode(0o600),
                            )
                            .is_err()
                            {
                                drop(file);
                                let _ = std::fs::remove_file(&path);
                                send_error(&writer, request_id, "workerIoFailed", "documentWrite").await?;
                                continue;
                            }
                        }
                        let transfer_id = next_inbound_transfer_id;
                        next_inbound_transfer_id = next_inbound_transfer_id
                            .checked_add(1)
                            .ok_or("inbound transfer handle space exhausted")?;
                        prepared_inbound = Some(PreparedInbound {
                            transfer_id,
                            total_bytes: preparation.total_bytes,
                            sha256: preparation.sha256.clone(),
                            transfer_limit: MAX_DOCUMENT_BYTES as u64,
                            path,
                            file,
                            action: InboundAction::Document {
                                request: preparation,
                                started: Instant::now(),
                            },
                        });
                        writer
                            .send_control(WireRecord::Control(Envelope {
                                request_id,
                                message: Message::TransferReady { transfer_id },
                            }))
                            .await?;
                    }
                    Message::RemoveDocument { request } => {
                        if !request.path.starts_with('/')
                            || request.path.ends_with('/')
                            || request
                                .expected_revision
                                .as_deref()
                                .is_some_and(|revision| !valid_transfer_sha256(revision))
                            || request.deadline_millis == 0
                            || request.deadline_millis
                                > environment_protocol::MAX_REQUEST_DEADLINE_MILLIS
                        {
                            send_error(&writer, request_id, "invalidRequest", "documentRemove").await?;
                            continue;
                        }
                        let cancelled = Arc::new(AtomicBool::new(false));
                        if active.insert(request_id, Arc::clone(&cancelled)).is_some() {
                            send_error(&writer, request_id, "duplicateRequest", "documentRemove").await?;
                            continue;
                        }
                        tasks.spawn(execute_document_remove(
                            request_id,
                            request,
                            Instant::now(),
                            writer.clone(),
                            Arc::clone(&documents),
                            cancelled,
                        ));
                    }
                    Message::PrepareLibraryOperation { request: preparation } => {
                        if prepared_inbound.is_some() || active_inbound.is_some() {
                            send_error(&writer, request_id, "workerBusy", "inboundTransfer").await?;
                            continue;
                        }
                        if preparation.total_bytes == 0
                            || preparation.total_bytes > MAX_MUTATION_TRANSFER_BYTES as u64
                            || !valid_transfer_sha256(&preparation.sha256)
                            || preparation.deadline_millis == 0
                            || preparation.deadline_millis
                                > environment_protocol::MAX_REQUEST_DEADLINE_MILLIS
                        {
                            send_error(&writer, request_id, "invalidRequest", "library").await?;
                            continue;
                        }
                        let path = PathBuf::from(format!(
                            "/tmp/.skill-deck-library-request-{}-{}",
                            std::process::id(),
                            request_id
                        ));
                        let file = match std::fs::OpenOptions::new()
                            .write(true)
                            .create_new(true)
                            .open(&path)
                        {
                            Ok(file) => file,
                            Err(_) => {
                                send_error(&writer, request_id, "workerBusy", "library").await?;
                                continue;
                            }
                        };
                        #[cfg(unix)]
                        {
                            use std::os::unix::fs::PermissionsExt;
                            if std::fs::set_permissions(
                                &path,
                                std::fs::Permissions::from_mode(0o600),
                            )
                            .is_err()
                            {
                                drop(file);
                                let _ = std::fs::remove_file(&path);
                                send_error(&writer, request_id, "workerIoFailed", "library").await?;
                                continue;
                            }
                        }
                        let transfer_id = next_inbound_transfer_id;
                        next_inbound_transfer_id = next_inbound_transfer_id
                            .checked_add(1)
                            .ok_or("inbound transfer handle space exhausted")?;
                        prepared_inbound = Some(PreparedInbound {
                            transfer_id,
                            total_bytes: preparation.total_bytes,
                            sha256: preparation.sha256,
                            transfer_limit: MAX_MUTATION_TRANSFER_BYTES as u64,
                            path,
                            file,
                            action: InboundAction::Library {
                                deadline_millis: preparation.deadline_millis,
                                started: Instant::now(),
                            },
                        });
                        writer
                            .send_control(WireRecord::Control(Envelope {
                                request_id,
                                message: Message::TransferReady { transfer_id },
                            }))
                            .await?;
                    }
                    Message::UploadPayloadBlob {
                        upload_id,
                        blob_id,
                        total_bytes,
                        sha256,
                    } => {
                        if prepared_inbound.is_some() || active_inbound.is_some() {
                            send_error(&writer, request_id, "workerBusy", "inboundTransfer").await?;
                            continue;
                        }
                        if total_bytes > MAX_PAYLOAD_TRANSFER_BYTES as u64
                            || sha256 != format!("sha256:{blob_id}")
                        {
                            send_error(&writer, request_id, "invalidTransfer", "payloadUpload").await?;
                            continue;
                        }
                        match payloads.lock().await.prepare_blob(upload_id, &blob_id) {
                            Ok(PreparedPayloadFile { path, file }) => {
                                let transfer_id = next_inbound_transfer_id;
                                next_inbound_transfer_id = next_inbound_transfer_id
                                    .checked_add(1)
                                    .ok_or("inbound transfer handle space exhausted")?;
                                prepared_inbound = Some(PreparedInbound {
                                    transfer_id,
                                    total_bytes,
                                    sha256,
                                    transfer_limit: MAX_PAYLOAD_TRANSFER_BYTES as u64,
                                    path,
                                    file,
                                    action: InboundAction::Blob { upload_id, blob_id },
                                });
                                writer.send_control(WireRecord::Control(Envelope {
                                    request_id,
                                    message: Message::TransferReady { transfer_id },
                                })).await?;
                            }
                            Err(error) => {
                                send_payload_error(
                                    &writer,
                                    request_id,
                                    SourcePayloadError::Payload(error),
                                ).await?;
                            }
                        }
                    }
                    Message::FinalizePayloadUpload {
                        upload_id,
                        total_bytes,
                        sha256,
                    } => {
                        if prepared_inbound.is_some() || active_inbound.is_some() {
                            send_error(&writer, request_id, "workerBusy", "inboundTransfer").await?;
                            continue;
                        }
                        if total_bytes == 0 || total_bytes > MAX_MANIFEST_TRANSFER_BYTES {
                            send_error(&writer, request_id, "invalidTransfer", "payloadUpload").await?;
                            continue;
                        }
                        match payloads.lock().await.prepare_manifest(upload_id) {
                            Ok(PreparedPayloadFile { path, file }) => {
                                let transfer_id = next_inbound_transfer_id;
                                next_inbound_transfer_id = next_inbound_transfer_id
                                    .checked_add(1)
                                    .ok_or("inbound transfer handle space exhausted")?;
                                prepared_inbound = Some(PreparedInbound {
                                    transfer_id,
                                    total_bytes,
                                    sha256,
                                    transfer_limit: MAX_MANIFEST_TRANSFER_BYTES,
                                    path,
                                    file,
                                    action: InboundAction::Manifest { upload_id },
                                });
                                writer.send_control(WireRecord::Control(Envelope {
                                    request_id,
                                    message: Message::TransferReady { transfer_id },
                                })).await?;
                            }
                            Err(error) => {
                                send_payload_error(
                                    &writer,
                                    request_id,
                                    SourcePayloadError::Payload(error),
                                ).await?;
                            }
                        }
                    }
                    Message::PrepareMutationUnit {
                        resource_id,
                        total_bytes,
                        sha256,
                    } => {
                        if prepared_inbound.is_some() || active_inbound.is_some() {
                            send_error(&writer, request_id, "workerBusy", "inboundTransfer").await?;
                            continue;
                        }
                        if total_bytes == 0
                            || total_bytes > MAX_MUTATION_TRANSFER_BYTES as u64
                            || !valid_transfer_sha256(&sha256)
                            || resource_id.is_empty()
                            || !resource_id.bytes().all(|byte| {
                                byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_')
                            })
                        {
                            send_error(&writer, request_id, "invalidTransfer", "mutation").await?;
                            continue;
                        }
                        let path = PathBuf::from(format!(
                            "/tmp/.skill-deck-mutation-request-{resource_id}-{}",
                            std::process::id()
                        ));
                        let file = match std::fs::OpenOptions::new()
                            .write(true)
                            .create_new(true)
                            .open(&path)
                        {
                            Ok(file) => file,
                            Err(_) => {
                                send_error(&writer, request_id, "workerBusy", "mutation").await?;
                                continue;
                            }
                        };
                        #[cfg(unix)]
                        {
                            use std::os::unix::fs::PermissionsExt;
                            if std::fs::set_permissions(
                                &path,
                                std::fs::Permissions::from_mode(0o600),
                            )
                            .is_err()
                            {
                                drop(file);
                                let _ = std::fs::remove_file(&path);
                                send_error(
                                    &writer,
                                    request_id,
                                    "workerIoFailed",
                                    "mutation",
                                )
                                .await?;
                                continue;
                            }
                        }
                        let transfer_id = next_inbound_transfer_id;
                        next_inbound_transfer_id = next_inbound_transfer_id
                            .checked_add(1)
                            .ok_or("inbound transfer handle space exhausted")?;
                        prepared_inbound = Some(PreparedInbound {
                            transfer_id,
                            total_bytes,
                            sha256,
                            transfer_limit: MAX_MUTATION_TRANSFER_BYTES as u64,
                            path,
                            file,
                            action: InboundAction::Mutation { resource_id },
                        });
                        writer
                            .send_control(WireRecord::Control(Envelope {
                                request_id,
                                message: Message::TransferReady { transfer_id },
                            }))
                            .await?;
                    }
                    Message::BeginTransfer {
                        transfer_id,
                        total_bytes,
                        sha256,
                        owner_request_id,
                    } => {
                        let prepared = prepared_inbound
                            .take()
                            .ok_or("worker received an unprepared inbound transfer")?;
                        if request_id != owner_request_id
                            || transfer_id != prepared.transfer_id
                            || total_bytes != prepared.total_bytes
                            || sha256 != prepared.sha256
                        {
                            return Err("inbound transfer does not match its preparation".into());
                        }
                        let transfer = InboundTransfer::begin(
                            TransferDeclaration {
                                owner_request_id,
                                transfer_id,
                                total_bytes,
                                sha256,
                            },
                            prepared.transfer_limit,
                            tokio::fs::File::from_std(prepared.file),
                        )?;
                        active_inbound = Some(ActiveInbound {
                            owner_request_id,
                            path: prepared.path,
                            action: prepared.action,
                            transfer,
                        });
                    }
                    Message::TransferCompleted {
                        transfer_id,
                        total_bytes,
                        sha256,
                    } => {
                        let inbound = active_inbound
                            .take()
                            .ok_or("worker completed an inbound transfer that is not active")?;
                        let completed = inbound.transfer.complete(TransferCompletion {
                            owner_request_id: request_id,
                            transfer_id,
                            total_bytes,
                            sha256,
                        }).await?;
                        drop(completed.file);
                        if let InboundAction::Mutation { resource_id } = inbound.action {
                            let cancelled = Arc::new(AtomicBool::new(false));
                            if active.insert(request_id, Arc::clone(&cancelled)).is_some() {
                                return Err("duplicate mutation request".into());
                            }
                            tasks.spawn(execute_mutation_request(
                                request_id,
                                resource_id,
                                inbound.path,
                                writer.clone(),
                                Arc::clone(&payloads),
                                Arc::clone(&mutations),
                                cancelled,
                            ));
                        } else if let InboundAction::Document { request, started } = inbound.action {
                            let cancelled = Arc::new(AtomicBool::new(false));
                            if active.insert(request_id, Arc::clone(&cancelled)).is_some() {
                                return Err("duplicate document request".into());
                            }
                            tasks.spawn(execute_document_write(
                                request_id,
                                request,
                                started,
                                inbound.path,
                                writer.clone(),
                                Arc::clone(&documents),
                                cancelled,
                            ));
                        } else if let InboundAction::Library {
                            deadline_millis,
                            started,
                        } = inbound.action
                        {
                            let cancelled = Arc::new(AtomicBool::new(false));
                            if active.insert(request_id, Arc::clone(&cancelled)).is_some() {
                                return Err("duplicate Library request".into());
                            }
                            tasks.spawn(execute_library_operation(
                                request_id,
                                LibraryExecution {
                                    deadline_millis,
                                    started,
                                    path: inbound.path,
                                },
                                writer.clone(),
                                Arc::clone(&payloads),
                                Arc::clone(&libraries),
                                cancelled,
                            ));
                        } else {
                            complete_inbound_action(
                                &writer,
                                request_id,
                                Arc::clone(&payloads),
                                inbound.path,
                                inbound.action,
                            ).await?;
                        }
                    }
                    Message::Cancel { target_request_id } => {
                        if let Some(cancelled) = active.get(&target_request_id) {
                            cancelled.store(true, Ordering::Release);
                        }
                        if active_inbound
                            .as_ref()
                            .is_some_and(|inbound| inbound.owner_request_id == target_request_id)
                        {
                            let inbound = active_inbound.take().unwrap();
                            let _ = tokio::fs::remove_file(inbound.path).await;
                            match inbound.action {
                                InboundAction::Blob { upload_id, .. }
                                | InboundAction::Manifest { upload_id } => {
                                    payloads.lock().await.abort_upload(upload_id);
                                    send_error(&writer, target_request_id, "cancelled", "payloadUpload").await?;
                                }
                                InboundAction::Mutation { .. } => {
                                    send_error(&writer, target_request_id, "cancelled", "mutation").await?;
                                }
                                InboundAction::Document { .. } => {
                                    send_error(&writer, target_request_id, "cancelled", "documentWrite").await?;
                                }
                                InboundAction::Library { .. } => {
                                    send_error(&writer, target_request_id, "cancelled", "library").await?;
                                }
                            }
                        }
                    }
                    Message::Shutdown => {
                        cancel_all(&active);
                        queue.clear();
                        discard_inbound(&mut prepared_inbound, &mut active_inbound).await;
                        shutting_down = true;
                    }
                    message => {
                        let dispatch = runtime.dispatch(message);
                        if let Some(message) = dispatch.response {
                            writer
                                .send_control(WireRecord::Control(Envelope {
                                    request_id,
                                    message,
                                }))
                                .await?;
                        }
                        if dispatch.close {
                            cancel_all(&active);
                            queue.clear();
                            shutting_down = true;
                        }
                    }
                }
            }
        }
    }

    drop(writer);
    writer_task.await??;
    Ok(())
}

async fn execute_business_request(
    request: QueuedRequest,
    writer: ProtocolWriter,
    sources: Arc<tokio::sync::Mutex<SourceManager>>,
    payloads: Arc<tokio::sync::Mutex<PayloadManager>>,
    mutations: Arc<tokio::sync::Mutex<MutationManager>>,
    libraries: Arc<tokio::sync::Mutex<LibraryManager>>,
) -> Result<u64, String> {
    let request_id = request.request_id;
    match request.message {
        Message::ObservePath { path } => {
            let result = tokio::task::spawn_blocking(move || execute_path_observation(&path))
                .await
                .map_err(|error| error.to_string())?;
            if !request.cancelled.load(Ordering::Acquire) {
                let message = result
                    .map(|kind| Message::PathObserved { kind })
                    .unwrap_or_else(error_message);
                writer
                    .send_control(WireRecord::Control(Envelope {
                        request_id,
                        message,
                    }))
                    .await
                    .map_err(|error| error.to_string())?;
            }
        }
        Message::InspectFilesystem { request: intent } => {
            let deadline = Duration::from_millis(intent.deadline_millis);
            let cancelled = Arc::clone(&request.cancelled);
            let inspection_cancelled = Arc::clone(&cancelled);
            let task = tokio::task::spawn_blocking(move || {
                execute_inspection(intent, || inspection_cancelled.load(Ordering::Acquire))
            });
            let result = match tokio::time::timeout(deadline, task).await {
                Ok(joined) => joined.map_err(|error| error.to_string())?,
                Err(_) => {
                    cancelled.store(true, Ordering::Release);
                    Err(RequestError {
                        code: "deadlineExceeded",
                        phase: "inspection",
                    })
                }
            };
            let externally_cancelled = request.cancelled.load(Ordering::Acquire)
                && !matches!(
                    result,
                    Err(RequestError {
                        code: "deadlineExceeded",
                        ..
                    })
                );
            if externally_cancelled {
                return Ok(request_id);
            }
            match result {
                Ok(response) => {
                    let payload =
                        encode_inspection_response(&response).map_err(|error| error.to_string())?;
                    writer
                        .send_transfer(request_id, request_id, &payload)
                        .await
                        .map_err(|error| error.to_string())?;
                }
                Err(error) => {
                    writer
                        .send_control(WireRecord::Control(Envelope {
                            request_id,
                            message: error_message(error),
                        }))
                        .await
                        .map_err(|error| error.to_string())?;
                }
            }
        }
        Message::InspectPaths { request: intent } => {
            let deadline = Duration::from_millis(intent.deadline_millis);
            let cancelled = Arc::clone(&request.cancelled);
            let operation_cancelled = Arc::clone(&cancelled);
            let task = tokio::task::spawn_blocking(move || {
                execute_path_metadata(intent, || operation_cancelled.load(Ordering::Acquire))
            });
            let result = match tokio::time::timeout(deadline, task).await {
                Ok(joined) => joined.map_err(|error| error.to_string())?,
                Err(_) => {
                    cancelled.store(true, Ordering::Release);
                    Err(RequestError {
                        code: "deadlineExceeded",
                        phase: "pathMetadata",
                    })
                }
            };
            send_payload_result(request_id, result, request.cancelled, writer).await?;
        }
        Message::CountDirectoryEntries { request: intent } => {
            let deadline = Duration::from_millis(intent.deadline_millis);
            let cancelled = Arc::clone(&request.cancelled);
            let operation_cancelled = Arc::clone(&cancelled);
            let task = tokio::task::spawn_blocking(move || {
                execute_directory_count(intent, || operation_cancelled.load(Ordering::Acquire))
            });
            let result = match tokio::time::timeout(deadline, task).await {
                Ok(joined) => joined.map_err(|error| error.to_string())?,
                Err(_) => {
                    cancelled.store(true, Ordering::Release);
                    Err(RequestError {
                        code: "deadlineExceeded",
                        phase: "directoryCount",
                    })
                }
            };
            send_payload_result(request_id, result, request.cancelled, writer).await?;
        }
        Message::ReadDocuments { request: intent } => {
            let deadline = Duration::from_millis(intent.deadline_millis);
            let cancelled = Arc::clone(&request.cancelled);
            let operation_cancelled = Arc::clone(&cancelled);
            let task = tokio::task::spawn_blocking(move || {
                execute_document_read(intent, || operation_cancelled.load(Ordering::Acquire))
            });
            let result = match tokio::time::timeout(deadline, task).await {
                Ok(joined) => joined.map_err(|error| error.to_string())?,
                Err(_) => {
                    cancelled.store(true, Ordering::Release);
                    Err(RequestError {
                        code: "deadlineExceeded",
                        phase: "documentRead",
                    })
                }
            };
            send_payload_result(request_id, result, request.cancelled, writer).await?;
        }
        Message::ListChildDirectories { request: intent } => {
            let deadline = Duration::from_millis(intent.deadline_millis);
            let cancelled = Arc::clone(&request.cancelled);
            let operation_cancelled = Arc::clone(&cancelled);
            let task = tokio::task::spawn_blocking(move || {
                execute_directory_list(intent, || operation_cancelled.load(Ordering::Acquire))
            });
            let result = match tokio::time::timeout(deadline, task).await {
                Ok(joined) => joined.map_err(|error| error.to_string())?,
                Err(_) => {
                    cancelled.store(true, Ordering::Release);
                    Err(RequestError {
                        code: "deadlineExceeded",
                        phase: "directoryList",
                    })
                }
            };
            send_payload_result(request_id, result, request.cancelled, writer).await?;
        }
        Message::MapPathsToWindows { request: intent } => {
            let deadline = Duration::from_millis(intent.deadline_millis);
            let cancelled = Arc::clone(&request.cancelled);
            let operation_cancelled = Arc::clone(&cancelled);
            let task = tokio::task::spawn_blocking(move || {
                execute_map_windows_paths(intent, || operation_cancelled.load(Ordering::Acquire))
            });
            let result = match tokio::time::timeout(deadline, task).await {
                Ok(joined) => joined.map_err(|error| error.to_string())?,
                Err(_) => {
                    cancelled.store(true, Ordering::Release);
                    Err(RequestError {
                        code: "deadlineExceeded",
                        phase: "pathMapping",
                    })
                }
            };
            send_payload_result(request_id, result, request.cancelled, writer).await?;
        }
        Message::MapHostPaths { request: intent } => {
            let result = execute_map_host_paths(intent, Arc::clone(&request.cancelled)).await;
            send_payload_result(request_id, result, request.cancelled, writer).await?;
        }
        Message::InspectEntries { request: intent } => {
            let deadline = Duration::from_millis(intent.deadline_millis);
            let cancelled = Arc::clone(&request.cancelled);
            let operation_cancelled = Arc::clone(&cancelled);
            let task = tokio::task::spawn_blocking(move || {
                execute_entry_facts(intent, || operation_cancelled.load(Ordering::Acquire))
            });
            let result = timeout_result(deadline, task, &cancelled, "entryFacts").await?;
            send_payload_result(request_id, result, request.cancelled, writer).await?;
        }
        Message::ProjectTargets { request: intent } => {
            let deadline = Duration::from_millis(intent.deadline_millis);
            let cancelled = Arc::clone(&request.cancelled);
            let operation_cancelled = Arc::clone(&cancelled);
            let task = tokio::task::spawn_blocking(move || {
                execute_projection(intent, || operation_cancelled.load(Ordering::Acquire))
            });
            let result = timeout_result(deadline, task, &cancelled, "projection").await?;
            send_payload_result(request_id, result, request.cancelled, writer).await?;
        }
        Message::BuildManifest { request: intent } => {
            let deadline = Duration::from_millis(intent.deadline_millis);
            let cancelled = Arc::clone(&request.cancelled);
            let operation_cancelled = Arc::clone(&cancelled);
            let task = tokio::task::spawn_blocking(move || {
                execute_manifest(intent, || operation_cancelled.load(Ordering::Acquire))
            });
            let result = timeout_result(deadline, task, &cancelled, "manifest").await?;
            send_payload_result(request_id, result, request.cancelled, writer).await?;
        }
        Message::AcquireGitSource { request: intent } => {
            let result = sources
                .lock()
                .await
                .acquire_git(
                    GitSourceOptions {
                        url: intent.url,
                        git_ref: intent.git_ref,
                        proxy: intent.proxy,
                        deadline: Duration::from_millis(intent.deadline_millis),
                    },
                    Arc::clone(&request.cancelled),
                )
                .await;
            if request.cancelled.load(Ordering::Acquire) {
                return Ok(request_id);
            }
            send_source_result(
                &writer,
                request_id,
                result.map(|source| Message::SourceOpened {
                    source_id: source.id,
                    root: source.root.to_string_lossy().into_owned(),
                    revision: source.revision,
                }),
            )
            .await?;
        }
        Message::OpenLocalSource { request: intent } => {
            let result = sources.lock().await.open_local(&intent.path);
            send_source_result(
                &writer,
                request_id,
                result.map(|source| Message::SourceOpened {
                    source_id: source.id,
                    root: source.root.to_string_lossy().into_owned(),
                    revision: source.revision,
                }),
            )
            .await?;
        }
        Message::ReleaseSource { source_id } => {
            let result = sources
                .lock()
                .await
                .release(source_id)
                .map(|()| Message::SourceReleased { source_id });
            send_source_result(&writer, request_id, result).await?;
        }
        Message::ScanSource { request: intent } => {
            let deadline = Duration::from_millis(intent.deadline_millis);
            let cancelled = Arc::clone(&request.cancelled);
            let scan_cancelled = Arc::clone(&cancelled);
            let source_manager = Arc::clone(&sources);
            let task = tokio::task::spawn_blocking(move || {
                let manager = source_manager.blocking_lock();
                scan_source(&manager, intent, || scan_cancelled.load(Ordering::Acquire))
            });
            let result = match tokio::time::timeout(deadline, task).await {
                Ok(joined) => joined.map_err(|error| error.to_string())?,
                Err(_) => {
                    cancelled.store(true, Ordering::Release);
                    Err(SourceError::DeadlineExceeded)
                }
            };
            if request.cancelled.load(Ordering::Acquire)
                && !matches!(result, Err(SourceError::DeadlineExceeded))
            {
                return Ok(request_id);
            }
            match result {
                Ok(response) => {
                    let payload = encode_payload(&response).map_err(|error| error.to_string())?;
                    writer
                        .send_transfer(request_id, request_id, &payload)
                        .await
                        .map_err(|error| error.to_string())?;
                }
                Err(error) => send_source_error(&writer, request_id, error).await?,
            }
        }
        Message::SourceFingerprint {
            source_id,
            relative_path,
            deadline_millis,
        } => {
            let deadline = Duration::from_millis(deadline_millis);
            let source_manager = Arc::clone(&sources);
            let relative_path = posix_path(relative_path);
            let task = tokio::task::spawn_blocking(move || {
                source_manager
                    .blocking_lock()
                    .fingerprint(source_id, &relative_path)
            });
            let result = match tokio::time::timeout(deadline, task).await {
                Ok(joined) => joined.map_err(|error| error.to_string())?,
                Err(_) => Err(SourceError::DeadlineExceeded),
            };
            send_source_result(
                &writer,
                request_id,
                result.map(|fingerprint| Message::SourceFingerprintResult { fingerprint }),
            )
            .await?;
        }
        Message::SourceRevision {
            source_id,
            relative_path,
            deadline_millis,
        } => {
            let result = sources
                .lock()
                .await
                .tree_revision(
                    source_id,
                    &posix_path(relative_path),
                    Duration::from_millis(deadline_millis),
                    Arc::clone(&request.cancelled),
                )
                .await;
            if request.cancelled.load(Ordering::Acquire) {
                return Ok(request_id);
            }
            send_source_result(
                &writer,
                request_id,
                result.map(|revision| Message::SourceRevisionResult { revision }),
            )
            .await?;
        }
        Message::ProbeGit { request: intent } => {
            let result = probe_git(
                GitSourceOptions {
                    url: intent.url,
                    git_ref: None,
                    proxy: intent.proxy,
                    deadline: Duration::from_millis(intent.deadline_millis),
                },
                Arc::clone(&request.cancelled),
            )
            .await;
            if request.cancelled.load(Ordering::Acquire) {
                return Ok(request_id);
            }
            send_source_result(
                &writer,
                request_id,
                result.map(|revision| Message::GitProbed { revision }),
            )
            .await?;
        }
        Message::AcquirePayloadFromSource { request: intent } => {
            let deadline = Duration::from_millis(intent.deadline_millis);
            let cancelled = Arc::clone(&request.cancelled);
            let build_cancelled = Arc::clone(&cancelled);
            let source_manager = Arc::clone(&sources);
            let payload_manager = Arc::clone(&payloads);
            let relative_path = posix_path(intent.relative_path);
            let mut task = tokio::task::spawn_blocking(move || {
                let sources = source_manager.blocking_lock();
                let source_root = sources.resolve(intent.source_id, &relative_path)?;
                payload_manager
                    .blocking_lock()
                    .acquire_from_source_with_cancel(
                        &intent.session_id,
                        &intent.payload_name,
                        &source_root,
                        || build_cancelled.load(Ordering::Acquire),
                    )
                    .map_err(SourcePayloadError::Payload)
            });
            let result = match tokio::time::timeout(deadline, &mut task).await {
                Ok(joined) => joined.map_err(|error| error.to_string())?,
                Err(_) => {
                    cancelled.store(true, Ordering::Release);
                    let _ = task.await.map_err(|error| error.to_string())?;
                    Err(SourcePayloadError::DeadlineExceeded)
                }
            };
            if request.cancelled.load(Ordering::Acquire)
                && !matches!(result, Err(SourcePayloadError::DeadlineExceeded))
            {
                return Ok(request_id);
            }
            match result {
                Ok(payload) => {
                    let encoded = encode_payload(&payload.into_response())
                        .map_err(|error| error.to_string())?;
                    writer
                        .send_transfer(request_id, request_id, &encoded)
                        .await
                        .map_err(|error| error.to_string())?;
                }
                Err(error) => send_payload_error(&writer, request_id, error).await?,
            }
        }
        Message::VerifyPayload { request: intent } => {
            let payload_manager = Arc::clone(&payloads);
            let task = tokio::task::spawn_blocking(move || {
                payload_manager
                    .blocking_lock()
                    .verify(&intent.session_id, &intent.payload_name)
            });
            let result =
                match tokio::time::timeout(Duration::from_millis(intent.deadline_millis), task)
                    .await
                {
                    Ok(joined) => joined.map_err(|error| error.to_string())?,
                    Err(_) => Err(PayloadError::StalePayload),
                };
            match result {
                Ok(payload) => {
                    let encoded = encode_payload(&payload.map(|payload| payload.into_response()))
                        .map_err(|error| error.to_string())?;
                    writer
                        .send_transfer(request_id, request_id, &encoded)
                        .await
                        .map_err(|error| error.to_string())?;
                }
                Err(error) => {
                    send_payload_error(&writer, request_id, SourcePayloadError::Payload(error))
                        .await?
                }
            }
        }
        Message::ReadPayloadBlob {
            payload_id,
            blob_id,
            deadline_millis: _,
        } => {
            let file = payloads
                .lock()
                .await
                .read_blob(payload_id, &blob_id)
                .map_err(SourcePayloadError::Payload);
            match file {
                Ok(Some(file)) => {
                    let total_bytes = file.metadata().map_err(|error| error.to_string())?.len();
                    writer
                        .send_reader_transfer_with_limit(
                            request_id,
                            request_id,
                            tokio::fs::File::from_std(file),
                            total_bytes,
                            format!("sha256:{blob_id}"),
                            MAX_PAYLOAD_TRANSFER_BYTES,
                        )
                        .await
                        .map_err(|error| error.to_string())?;
                }
                Ok(None) => {
                    send_payload_error(&writer, request_id, SourcePayloadError::MissingPayload)
                        .await?
                }
                Err(error) => send_payload_error(&writer, request_id, error).await?,
            }
        }
        Message::ReadLibraryCatalog { deadline_millis } => {
            let result = if deadline_millis == 0
                || deadline_millis > environment_protocol::MAX_REQUEST_DEADLINE_MILLIS
            {
                Err(LibraryError::InvalidRequest)
            } else {
                let task =
                    tokio::task::spawn_blocking(move || libraries.blocking_lock().read_catalog());
                match tokio::time::timeout(Duration::from_millis(deadline_millis), task).await {
                    Ok(joined) => joined.map_err(|error| error.to_string())?,
                    Err(_) => {
                        request.cancelled.store(true, Ordering::Release);
                        send_error(&writer, request_id, "deadlineExceeded", "libraryRead")
                            .await
                            .map_err(|error| error.to_string())?;
                        return Ok(request_id);
                    }
                }
            };
            match result {
                Ok(response) if !request.cancelled.load(Ordering::Acquire) => {
                    let payload = encode_payload(&response).map_err(|error| error.to_string())?;
                    writer
                        .send_transfer(request_id, request_id, &payload)
                        .await
                        .map_err(|error| error.to_string())?;
                }
                Ok(_) => {}
                Err(error) => send_library_error(&writer, request_id, error, "libraryRead").await?,
            }
        }
        Message::RemovePayload {
            session_id,
            payload_name,
        } => {
            let result = payloads
                .lock()
                .await
                .remove(&session_id, &payload_name)
                .map(|()| Message::PayloadRemoved {
                    session_id,
                    payload_name,
                })
                .map_err(SourcePayloadError::Payload);
            send_payload_control_result(&writer, request_id, result).await?;
        }
        Message::RemovePayloadSession { session_id } => {
            let result = payloads
                .lock()
                .await
                .remove_session(&session_id)
                .map(|()| Message::PayloadSessionRemoved { session_id })
                .map_err(SourcePayloadError::Payload);
            send_payload_control_result(&writer, request_id, result).await?;
        }
        Message::SweepPayloadOrphans {
            protected_session_ids,
        } => {
            let result = payloads.lock().await.sweep_orphans(&protected_session_ids);
            match result {
                Ok(report) => {
                    let encoded = encode_payload(&report).map_err(|error| error.to_string())?;
                    writer
                        .send_transfer(request_id, request_id, &encoded)
                        .await
                        .map_err(|error| error.to_string())?;
                }
                Err(error) => {
                    send_payload_error(&writer, request_id, SourcePayloadError::Payload(error))
                        .await?
                }
            }
        }
        Message::BeginPayloadUpload {
            session_id,
            payload_name,
        } => {
            let result = payloads
                .lock()
                .await
                .begin_upload(&session_id, &payload_name)
                .map(|upload_id| Message::PayloadUploadBegun { upload_id })
                .map_err(SourcePayloadError::Payload);
            send_payload_control_result(&writer, request_id, result).await?;
        }
        Message::AcknowledgeMutationUnit { cleanup } => {
            let manager = mutations.lock_owned().await;
            let resource_id = cleanup.resource_id.clone();
            let result = tokio::task::spawn_blocking(move || manager.acknowledge(&cleanup))
                .await
                .map_err(|error| error.to_string())?;
            match result {
                Ok(()) => {
                    writer
                        .send_control(WireRecord::Control(Envelope {
                            request_id,
                            message: Message::MutationAcknowledged { resource_id },
                        }))
                        .await
                        .map_err(|error| error.to_string())?;
                }
                Err(error) => {
                    writer
                        .send_control(WireRecord::Control(Envelope {
                            request_id,
                            message: Message::Error {
                                code: "staleRecovery".to_string(),
                                phase: "mutationAck".to_string(),
                                parameters: vec![("message".to_string(), error.to_string())],
                            },
                        }))
                        .await
                        .map_err(|error| error.to_string())?;
                }
            }
        }
        Message::ListMutationRecovery => {
            let manager = mutations.lock_owned().await;
            let result = tokio::task::spawn_blocking(move || manager.recovery_store().list())
                .await
                .map_err(|error| error.to_string())?;
            match result {
                Ok(records) => {
                    let response = environment_protocol::MutationRecoveryList {
                        records: records
                            .into_iter()
                            .map(|record| environment_protocol::MutationRecoveryRecord {
                                resource_id: record.resource_id,
                                managed_root: record.managed_root.to_string_lossy().into_owned(),
                                state: if record.unsafe_root {
                                    environment_protocol::MutationRecoveryState::Unsafe
                                } else if record.marker_bytes.is_some() {
                                    environment_protocol::MutationRecoveryState::Present
                                } else {
                                    environment_protocol::MutationRecoveryState::Unreadable
                                },
                                marker_bytes: record.marker_bytes.unwrap_or_default(),
                            })
                            .collect(),
                    };
                    let payload = encode_payload(&response).map_err(|error| error.to_string())?;
                    writer
                        .send_transfer(request_id, request_id, &payload)
                        .await
                        .map_err(|error| error.to_string())?;
                }
                Err(error) => {
                    send_error(
                        &writer,
                        request_id,
                        "recoveryUnavailable",
                        "mutationRecovery",
                    )
                    .await
                    .map_err(|error| error.to_string())?;
                    let _ = error;
                }
            }
        }
        Message::CleanupMutationRecovery {
            resource_id,
            expected_marker_json,
            backups,
        } => {
            let manager = mutations.lock_owned().await;
            let result = tokio::task::spawn_blocking({
                let resource_id = resource_id.clone();
                move || {
                    manager.recovery_store().cleanup(
                        &resource_id,
                        &expected_marker_json,
                        &backups.into_iter().map(PathBuf::from).collect::<Vec<_>>(),
                    )
                }
            })
            .await
            .map_err(|error| error.to_string())?;
            match result {
                Ok(()) => {
                    writer
                        .send_control(WireRecord::Control(Envelope {
                            request_id,
                            message: Message::MutationRecoveryCleaned { resource_id },
                        }))
                        .await
                        .map_err(|error| error.to_string())?;
                }
                Err(error) => {
                    writer
                        .send_control(WireRecord::Control(Envelope {
                            request_id,
                            message: Message::Error {
                                code: "staleRecovery".to_string(),
                                phase: "mutationRecovery".to_string(),
                                parameters: vec![("message".to_string(), error.to_string())],
                            },
                        }))
                        .await
                        .map_err(|error| error.to_string())?;
                }
            }
        }
        _ => return Err("non-business message entered the worker queue".to_string()),
    }
    Ok(request_id)
}

async fn execute_map_host_paths(
    request: MapHostPathsRequest,
    cancelled: Arc<AtomicBool>,
) -> Result<MapHostPathsResponse, RequestError> {
    if request.paths.is_empty()
        || request.paths.len() > MAX_INSPECTION_ROOTS
        || request
            .paths
            .iter()
            .any(|path| path.is_empty() || path.contains('\0'))
        || request.deadline_millis == 0
        || request.deadline_millis > MAX_REQUEST_DEADLINE_MILLIS
    {
        return Err(RequestError {
            code: "invalidRequest",
            phase: "pathMapping",
        });
    }

    let mapping = async {
        let mut mapped = Vec::with_capacity(request.paths.len());
        for path in request.paths {
            if cancelled.load(Ordering::Acquire) {
                return Err(RequestError {
                    code: "cancelled",
                    phase: "pathMapping",
                });
            }
            mapped.push(map_host_path(&path, Arc::clone(&cancelled)).await?);
        }
        Ok(MapHostPathsResponse { mapped })
    };
    match tokio::time::timeout(Duration::from_millis(request.deadline_millis), mapping).await {
        Ok(result) => result,
        Err(_) => {
            cancelled.store(true, Ordering::Release);
            Err(RequestError {
                code: "deadlineExceeded",
                phase: "pathMapping",
            })
        }
    }
}

#[allow(
    clippy::disallowed_methods,
    reason = "Environment Worker 是独立 crate，必须直接构造受控的 wslpath 子进程"
)]
async fn map_host_path(path: &str, cancelled: Arc<AtomicBool>) -> Result<String, RequestError> {
    let mut command = tokio::process::Command::new("wslpath");
    command
        .args(["-u", "--", path])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    let output = tokio::select! {
        output = command.output() => output,
        _ = wait_until_cancelled(cancelled) => {
            return Err(RequestError {
                code: "cancelled",
                phase: "pathMapping",
            });
        }
    }
    .map_err(|_| RequestError {
        code: "pathUnavailable",
        phase: "pathMapping",
    })?;
    if !output.status.success() || output.stdout.len() > 16 * 1024 {
        return Err(RequestError {
            code: "pathUnavailable",
            phase: "pathMapping",
        });
    }
    String::from_utf8(output.stdout)
        .ok()
        .map(|path| path.trim_end_matches(['\r', '\n']).to_string())
        .filter(|path| path.starts_with('/') && !path.contains('\0'))
        .ok_or(RequestError {
            code: "pathUnavailable",
            phase: "pathMapping",
        })
}

async fn wait_until_cancelled(cancelled: Arc<AtomicBool>) {
    while !cancelled.load(Ordering::Acquire) {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

enum SourcePayloadError {
    Source(SourceError),
    Payload(PayloadError),
    MissingPayload,
    DeadlineExceeded,
}

async fn complete_inbound_action(
    writer: &ProtocolWriter,
    request_id: u64,
    payloads: Arc<tokio::sync::Mutex<PayloadManager>>,
    path: PathBuf,
    action: InboundAction,
) -> Result<(), String> {
    match action {
        InboundAction::Blob { upload_id, blob_id } => {
            let payload_manager = Arc::clone(&payloads);
            let committed_blob_id = blob_id.clone();
            let result = tokio::task::spawn_blocking(move || {
                payload_manager
                    .blocking_lock()
                    .commit_blob(upload_id, &committed_blob_id, path)
            })
            .await
            .map_err(|error| error.to_string())?;
            match result {
                Ok(()) => {
                    writer
                        .send_control(WireRecord::Control(Envelope {
                            request_id,
                            message: Message::PayloadBlobUploaded { upload_id, blob_id },
                        }))
                        .await
                        .map_err(|error| error.to_string())?;
                }
                Err(error) => {
                    payloads.lock().await.abort_upload(upload_id);
                    send_payload_error(writer, request_id, SourcePayloadError::Payload(error))
                        .await?;
                }
            }
        }
        InboundAction::Manifest { upload_id } => {
            let payload_manager = Arc::clone(&payloads);
            let result = tokio::task::spawn_blocking(move || {
                payload_manager
                    .blocking_lock()
                    .finalize_upload(upload_id, path)
            })
            .await
            .map_err(|error| error.to_string())?;
            match result {
                Ok(payload) => {
                    writer
                        .send_control(WireRecord::Control(Envelope {
                            request_id,
                            message: Message::PayloadUploadFinalized {
                                payload_id: payload.id,
                            },
                        }))
                        .await
                        .map_err(|error| error.to_string())?;
                }
                Err(error) => {
                    send_payload_error(writer, request_id, SourcePayloadError::Payload(error))
                        .await?;
                }
            }
        }
        InboundAction::Mutation { .. } => {
            return Err("mutation transfer entered payload completion".to_string());
        }
        InboundAction::Document { .. } => {
            return Err("document transfer entered payload completion".to_string());
        }
        InboundAction::Library { .. } => {
            return Err("Library transfer entered payload completion".to_string());
        }
    }
    Ok(())
}

async fn execute_mutation_request(
    request_id: u64,
    resource_id: String,
    path: PathBuf,
    writer: ProtocolWriter,
    payloads: Arc<tokio::sync::Mutex<PayloadManager>>,
    mutations: Arc<tokio::sync::Mutex<MutationManager>>,
    cancelled: Arc<AtomicBool>,
) -> Result<u64, String> {
    let bytes = tokio::fs::read(&path)
        .await
        .map_err(|error| error.to_string())?;
    let _ = tokio::fs::remove_file(path).await;
    let request: environment_protocol::MutationUnitRequest =
        environment_protocol::decode_payload(&bytes).map_err(|error| error.to_string())?;
    if request.resource_id != resource_id {
        return Err("mutation transfer resource does not match its request".to_string());
    }
    if cancelled.load(Ordering::Acquire) {
        send_mutation_outcome(
            &writer,
            request_id,
            &environment_protocol::MutationUnitOutcome::Cancelled,
        )
        .await?;
        return Ok(request_id);
    }
    let started = tokio::time::Instant::now();
    let deadline = Duration::from_millis(request.deadline_millis);
    let mutation_guard = tokio::select! {
        guard = mutations.lock_owned() => guard,
        _ = tokio::time::sleep(deadline) => {
            send_mutation_outcome(&writer, request_id, &mutation_deadline_outcome()).await?;
            return Ok(request_id);
        }
    };
    let deadline_elapsed = Arc::new(AtomicBool::new(false));
    let deadline_signal = Arc::clone(&deadline_elapsed);
    let deadline_task = tokio::spawn(async move {
        tokio::time::sleep(deadline.saturating_sub(started.elapsed())).await;
        deadline_signal.store(true, Ordering::Release);
    });
    let payload_manager = Arc::clone(&payloads);
    let accept_cancelled = Arc::clone(&cancelled);
    let accept_deadline = Arc::clone(&deadline_elapsed);
    let accepted = tokio::task::spawn_blocking(move || {
        let payloads = payload_manager.blocking_lock();
        match mutation_guard.accept(request, &payloads, || {
            accept_cancelled.load(Ordering::Acquire) || accept_deadline.load(Ordering::Acquire)
        }) {
            Ok(accepted) => Ok((mutation_guard, accepted)),
            Err(error) => Err((mutation_guard, error)),
        }
    })
    .await
    .map_err(|error| error.to_string())?;
    let (mutation_guard, accepted) = match accepted {
        Ok(value) => value,
        Err((_guard, error)) => {
            deadline_task.abort();
            let outcome = if deadline_elapsed.load(Ordering::Acquire) {
                mutation_deadline_outcome()
            } else {
                mutation_accept_error(error, cancelled.load(Ordering::Acquire))
            };
            send_mutation_outcome(&writer, request_id, &outcome).await?;
            return Ok(request_id);
        }
    };
    if MutationManager::requires_acceptance(&accepted) {
        writer
            .send_binary_barrier(WireRecord::Control(Envelope {
                request_id,
                message: Message::MutationAccepted {
                    resource_id: resource_id.clone(),
                },
            }))
            .await
            .map_err(|error| error.to_string())?;
    }
    let execute_cancelled = Arc::clone(&cancelled);
    let execute_deadline = Arc::clone(&deadline_elapsed);
    let outcome = tokio::task::spawn_blocking(move || {
        mutation_guard.execute(accepted, || {
            execute_cancelled.load(Ordering::Acquire) || execute_deadline.load(Ordering::Acquire)
        })
    })
    .await
    .map_err(|error| error.to_string())?
    .unwrap_or_else(
        |error| environment_protocol::MutationUnitOutcome::RecoveryRequired {
            resource_id,
            message: error.to_string(),
        },
    );
    deadline_task.abort();
    let outcome = if deadline_elapsed.load(Ordering::Acquire)
        && matches!(
            outcome,
            environment_protocol::MutationUnitOutcome::Cancelled
        ) {
        mutation_deadline_outcome()
    } else {
        outcome
    };
    send_mutation_outcome(&writer, request_id, &outcome).await?;
    Ok(request_id)
}

async fn execute_document_write(
    request_id: u64,
    preparation: environment_protocol::DocumentWritePreparation,
    started: Instant,
    path: PathBuf,
    writer: ProtocolWriter,
    gate: Arc<tokio::sync::Mutex<()>>,
    cancelled: Arc<AtomicBool>,
) -> Result<u64, String> {
    let bytes = match tokio::fs::read(&path).await {
        Ok(bytes) => bytes,
        Err(_) => {
            let _ = tokio::fs::remove_file(path).await;
            send_error(&writer, request_id, "documentWriteFailed", "documentWrite")
                .await
                .map_err(|error| error.to_string())?;
            return Ok(request_id);
        }
    };
    let _ = tokio::fs::remove_file(path).await;
    if cancelled.load(Ordering::Acquire) {
        send_error(&writer, request_id, "cancelled", "documentWrite")
            .await
            .map_err(|error| error.to_string())?;
        return Ok(request_id);
    }
    let Some(remaining) =
        Duration::from_millis(preparation.deadline_millis).checked_sub(started.elapsed())
    else {
        send_error(&writer, request_id, "deadlineExceeded", "documentWrite")
            .await
            .map_err(|error| error.to_string())?;
        return Ok(request_id);
    };
    let _guard = match tokio::time::timeout(remaining, gate.lock()).await {
        Ok(guard) => guard,
        Err(_) => {
            send_error(&writer, request_id, "deadlineExceeded", "documentWrite")
                .await
                .map_err(|error| error.to_string())?;
            return Ok(request_id);
        }
    };
    if cancelled.load(Ordering::Acquire) {
        send_error(&writer, request_id, "cancelled", "documentWrite")
            .await
            .map_err(|error| error.to_string())?;
        return Ok(request_id);
    }
    let expected_digest = format!("sha256:{:x}", sha2::Sha256::digest(&bytes));
    if expected_digest != preparation.sha256 {
        send_error(&writer, request_id, "invalidTransfer", "documentWrite")
            .await
            .map_err(|error| error.to_string())?;
        return Ok(request_id);
    }
    let result = tokio::task::spawn_blocking(move || {
        environment_engine::document::write_document_atomic(
            PathBuf::from(preparation.path).as_path(),
            preparation.expected_revision.as_deref(),
            &bytes,
        )
    })
    .await
    .map_err(|error| error.to_string())?;
    match result {
        Ok(revision) => {
            writer
                .send_control(WireRecord::Control(Envelope {
                    request_id,
                    message: Message::DocumentWritten { revision },
                }))
                .await
                .map_err(|error| error.to_string())?;
        }
        Err(environment_engine::document::DocumentWriteError::Conflict) => {
            send_error(&writer, request_id, "documentConflict", "documentWrite")
                .await
                .map_err(|error| error.to_string())?;
        }
        Err(environment_engine::document::DocumentWriteError::InvalidTarget) => {
            send_error(&writer, request_id, "invalidTarget", "documentWrite")
                .await
                .map_err(|error| error.to_string())?;
        }
        Err(error) => {
            let _ = error;
            send_error(&writer, request_id, "documentWriteFailed", "documentWrite")
                .await
                .map_err(|error| error.to_string())?;
        }
    }
    Ok(request_id)
}

async fn execute_library_operation(
    request_id: u64,
    execution: LibraryExecution,
    writer: ProtocolWriter,
    payloads: Arc<tokio::sync::Mutex<PayloadManager>>,
    libraries: Arc<tokio::sync::Mutex<LibraryManager>>,
    cancelled: Arc<AtomicBool>,
) -> Result<u64, String> {
    let bytes = match tokio::fs::read(&execution.path).await {
        Ok(bytes) => bytes,
        Err(_) => {
            let _ = tokio::fs::remove_file(execution.path).await;
            send_error(&writer, request_id, "libraryIo", "library")
                .await
                .map_err(|error| error.to_string())?;
            return Ok(request_id);
        }
    };
    let _ = tokio::fs::remove_file(execution.path).await;
    if cancelled.load(Ordering::Acquire) {
        send_error(&writer, request_id, "cancelled", "library")
            .await
            .map_err(|error| error.to_string())?;
        return Ok(request_id);
    }
    let request = match decode_payload::<environment_protocol::LibraryOperationRequest>(&bytes) {
        Ok(request) if request.deadline_millis == execution.deadline_millis => request,
        _ => {
            send_error(&writer, request_id, "invalidRequest", "library")
                .await
                .map_err(|error| error.to_string())?;
            return Ok(request_id);
        }
    };
    let Some(remaining) =
        Duration::from_millis(execution.deadline_millis).checked_sub(execution.started.elapsed())
    else {
        send_error(&writer, request_id, "deadlineExceeded", "library")
            .await
            .map_err(|error| error.to_string())?;
        return Ok(request_id);
    };
    let manager = match tokio::time::timeout(remaining, libraries.lock_owned()).await {
        Ok(manager) => manager,
        Err(_) => {
            send_error(&writer, request_id, "deadlineExceeded", "library")
                .await
                .map_err(|error| error.to_string())?;
            return Ok(request_id);
        }
    };
    if cancelled.load(Ordering::Acquire) {
        send_error(&writer, request_id, "cancelled", "library")
            .await
            .map_err(|error| error.to_string())?;
        return Ok(request_id);
    }
    let result = tokio::task::spawn_blocking(move || {
        let payloads = payloads.blocking_lock();
        manager.execute(request, &payloads)
    })
    .await
    .map_err(|error| error.to_string())?;
    match result {
        Ok(catalog_revision) => {
            writer
                .send_control(WireRecord::Control(Envelope {
                    request_id,
                    message: Message::LibraryOperationCompleted { catalog_revision },
                }))
                .await
                .map_err(|error| error.to_string())?;
        }
        Err(error) => send_library_error(&writer, request_id, error, "library").await?,
    }
    Ok(request_id)
}

async fn execute_document_remove(
    request_id: u64,
    request: environment_protocol::DocumentRemoveRequest,
    started: Instant,
    writer: ProtocolWriter,
    gate: Arc<tokio::sync::Mutex<()>>,
    cancelled: Arc<AtomicBool>,
) -> Result<u64, String> {
    let Some(remaining) =
        Duration::from_millis(request.deadline_millis).checked_sub(started.elapsed())
    else {
        send_error(&writer, request_id, "deadlineExceeded", "documentRemove")
            .await
            .map_err(|error| error.to_string())?;
        return Ok(request_id);
    };
    let _guard = match tokio::time::timeout(remaining, gate.lock()).await {
        Ok(guard) => guard,
        Err(_) => {
            send_error(&writer, request_id, "deadlineExceeded", "documentRemove")
                .await
                .map_err(|error| error.to_string())?;
            return Ok(request_id);
        }
    };
    if cancelled.load(Ordering::Acquire) {
        send_error(&writer, request_id, "cancelled", "documentRemove")
            .await
            .map_err(|error| error.to_string())?;
        return Ok(request_id);
    }
    let result = tokio::task::spawn_blocking(move || {
        environment_engine::document::remove_document_if_revision(
            PathBuf::from(request.path).as_path(),
            request.expected_revision.as_deref(),
        )
    })
    .await
    .map_err(|error| error.to_string())?;
    match result {
        Ok(()) => writer
            .send_control(WireRecord::Control(Envelope {
                request_id,
                message: Message::DocumentRemoved,
            }))
            .await
            .map_err(|error| error.to_string())?,
        Err(environment_engine::document::DocumentWriteError::Conflict) => {
            send_error(&writer, request_id, "documentConflict", "documentRemove")
                .await
                .map_err(|error| error.to_string())?
        }
        Err(environment_engine::document::DocumentWriteError::InvalidTarget) => {
            send_error(&writer, request_id, "invalidTarget", "documentRemove")
                .await
                .map_err(|error| error.to_string())?
        }
        Err(_) => send_error(&writer, request_id, "documentWriteFailed", "documentRemove")
            .await
            .map_err(|error| error.to_string())?,
    }
    Ok(request_id)
}

async fn send_library_error(
    writer: &ProtocolWriter,
    request_id: u64,
    error: LibraryError,
    phase: &'static str,
) -> Result<(), String> {
    let code = match error {
        LibraryError::InvalidRequest => "invalidRequest",
        LibraryError::StaleTarget => "staleTarget",
        LibraryError::StalePayload => "stalePayload",
        LibraryError::RecoveryIncomplete => "libraryRecoveryIncomplete",
        LibraryError::Io => "libraryIo",
    };
    send_error(writer, request_id, code, phase)
        .await
        .map_err(|error| error.to_string())
}

fn mutation_deadline_outcome() -> environment_protocol::MutationUnitOutcome {
    environment_protocol::MutationUnitOutcome::Failed {
        code: "deadlineExceeded".to_string(),
        phase: "mutation".to_string(),
        parameters: Vec::new(),
        message: "WSL mutation deadline exceeded".to_string(),
    }
}

async fn send_mutation_outcome(
    writer: &ProtocolWriter,
    request_id: u64,
    outcome: &environment_protocol::MutationUnitOutcome,
) -> Result<(), String> {
    let payload = encode_payload(outcome).map_err(|error| error.to_string())?;
    writer
        .send_transfer_with_limit(
            request_id,
            request_id,
            &payload,
            MAX_MUTATION_TRANSFER_BYTES,
        )
        .await
        .map_err(|error| error.to_string())
}

fn valid_transfer_sha256(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(|digest| {
        digest.len() == 64
            && digest
                .bytes()
                .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
    })
}

fn mutation_accept_error(
    error: wsl_environment_worker::mutation::WorkerMutationError,
    cancelled: bool,
) -> environment_protocol::MutationUnitOutcome {
    use environment_engine::linux_mutation::MutationError as EngineError;
    use wsl_environment_worker::mutation::WorkerMutationError;

    if cancelled || matches!(error, WorkerMutationError::Engine(EngineError::Cancelled)) {
        return environment_protocol::MutationUnitOutcome::Cancelled;
    }
    let code = match error {
        WorkerMutationError::Engine(EngineError::StaleTarget) => "staleTarget",
        WorkerMutationError::Payload => "stalePayload",
        _ => "invalidMutation",
    };
    environment_protocol::MutationUnitOutcome::Failed {
        code: code.to_string(),
        phase: "accept".to_string(),
        parameters: Vec::new(),
        message: error.to_string(),
    }
}

impl From<SourceError> for SourcePayloadError {
    fn from(error: SourceError) -> Self {
        Self::Source(error)
    }
}

async fn send_payload_control_result(
    writer: &ProtocolWriter,
    request_id: u64,
    result: Result<Message, SourcePayloadError>,
) -> Result<(), String> {
    match result {
        Ok(message) => writer
            .send_control(WireRecord::Control(Envelope {
                request_id,
                message,
            }))
            .await
            .map_err(|error| error.to_string()),
        Err(error) => send_payload_error(writer, request_id, error).await,
    }
}

async fn send_payload_error(
    writer: &ProtocolWriter,
    request_id: u64,
    error: SourcePayloadError,
) -> Result<(), String> {
    let (code, phase) = match error {
        SourcePayloadError::Source(SourceError::MissingSource) => ("staleSource", "payload"),
        SourcePayloadError::Source(_) => ("invalidSource", "payload"),
        SourcePayloadError::Payload(PayloadError::MissingPayload)
        | SourcePayloadError::MissingPayload => ("missingPayload", "payload"),
        SourcePayloadError::Payload(PayloadError::Engine(
            environment_engine::payload::PayloadError::Cancelled,
        )) => ("cancelled", "payload"),
        SourcePayloadError::DeadlineExceeded => ("deadlineExceeded", "payload"),
        SourcePayloadError::Payload(_) => ("stalePayload", "payload"),
    };
    writer
        .send_control(WireRecord::Control(Envelope {
            request_id,
            message: Message::Error {
                code: code.to_string(),
                phase: phase.to_string(),
                parameters: Vec::new(),
            },
        }))
        .await
        .map_err(|error| error.to_string())
}

async fn send_source_result(
    writer: &ProtocolWriter,
    request_id: u64,
    result: Result<Message, SourceError>,
) -> Result<(), String> {
    match result {
        Ok(message) => writer
            .send_control(WireRecord::Control(Envelope {
                request_id,
                message,
            }))
            .await
            .map_err(|error| error.to_string()),
        Err(error) => send_source_error(writer, request_id, error).await,
    }
}

async fn send_source_error(
    writer: &ProtocolWriter,
    request_id: u64,
    error: SourceError,
) -> Result<(), String> {
    let (code, phase, parameters) = match error {
        SourceError::GitUnavailable { message } => (
            "gitUnavailable",
            "git",
            vec![("message".to_string(), message)],
        ),
        SourceError::GitFailed { exit_code, stderr } => (
            "gitFailed",
            "git",
            vec![
                (
                    "exitCode".to_string(),
                    exit_code.map(|code| code.to_string()).unwrap_or_default(),
                ),
                ("stderr".to_string(), stderr),
            ],
        ),
        SourceError::DeadlineExceeded => ("deadlineExceeded", "source", Vec::new()),
        SourceError::Cancelled => ("cancelled", "source", Vec::new()),
        SourceError::MissingSource => ("staleSource", "source", Vec::new()),
        SourceError::InvalidLocalSource
        | SourceError::InvalidRelativePath
        | SourceError::InvalidInventory => ("invalidSource", "source", Vec::new()),
        SourceError::InvalidManagedBase | SourceError::Io(_) => ("sourceIo", "source", Vec::new()),
    };
    writer
        .send_control(WireRecord::Control(Envelope {
            request_id,
            message: Message::Error {
                code: code.to_string(),
                phase: phase.to_string(),
                parameters,
            },
        }))
        .await
        .map_err(|error| error.to_string())
}

#[cfg(unix)]
fn posix_path(bytes: Vec<u8>) -> PathBuf {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;

    PathBuf::from(OsString::from_vec(bytes))
}

#[cfg(not(unix))]
fn posix_path(bytes: Vec<u8>) -> PathBuf {
    PathBuf::from(String::from_utf8_lossy(&bytes).into_owned())
}

async fn timeout_result<T>(
    deadline: Duration,
    task: tokio::task::JoinHandle<Result<T, RequestError>>,
    cancelled: &Arc<AtomicBool>,
    phase: &'static str,
) -> Result<Result<T, RequestError>, String> {
    match tokio::time::timeout(deadline, task).await {
        Ok(joined) => joined.map_err(|error| error.to_string()),
        Err(_) => {
            cancelled.store(true, Ordering::Release);
            Ok(Err(RequestError {
                code: "deadlineExceeded",
                phase,
            }))
        }
    }
}

async fn send_payload_result<T>(
    request_id: u64,
    result: Result<T, RequestError>,
    cancelled: Arc<AtomicBool>,
    writer: ProtocolWriter,
) -> Result<(), String>
where
    T: serde::Serialize,
{
    let externally_cancelled = cancelled.load(Ordering::Acquire)
        && !matches!(
            result,
            Err(RequestError {
                code: "deadlineExceeded",
                ..
            })
        );
    if externally_cancelled {
        return Ok(());
    }
    match result {
        Ok(response) => {
            let payload = encode_payload(&response).map_err(|error| error.to_string())?;
            writer
                .send_transfer(request_id, request_id, &payload)
                .await
                .map_err(|error| error.to_string())
        }
        Err(error) => writer
            .send_control(WireRecord::Control(Envelope {
                request_id,
                message: error_message(error),
            }))
            .await
            .map_err(|error| error.to_string()),
    }
}

async fn send_error(
    writer: &ProtocolWriter,
    request_id: u64,
    code: &'static str,
    phase: &'static str,
) -> Result<(), environment_protocol::WriterError> {
    writer
        .send_control(WireRecord::Control(Envelope {
            request_id,
            message: error_message(RequestError { code, phase }),
        }))
        .await
}

async fn discard_inbound(
    prepared: &mut Option<PreparedInbound>,
    active: &mut Option<ActiveInbound>,
) {
    if let Some(prepared) = prepared.take() {
        drop(prepared.file);
        let _ = tokio::fs::remove_file(prepared.path).await;
    }
    if let Some(active) = active.take() {
        let _ = tokio::fs::remove_file(active.path).await;
    }
}

fn cancel_all(active: &HashMap<u64, Arc<AtomicBool>>) {
    for cancelled in active.values() {
        cancelled.store(true, Ordering::Release);
    }
}

#[cfg(unix)]
fn effective_user_id() -> u32 {
    unsafe extern "C" {
        fn geteuid() -> u32;
    }
    unsafe { geteuid() }
}

#[cfg(not(unix))]
fn effective_user_id() -> u32 {
    0
}

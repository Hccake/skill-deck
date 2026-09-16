use std::future::Future;
use std::time::Duration;

use sha2::{Digest, Sha256};

use crate::core::mutation::CancellationSignal;
use crate::environment::runtime::EntryFingerprint;
use crate::environment::wsl::WslWorkspace;
use crate::error::AppError;

const ENTRY_DEADLINE_MILLIS: u64 = 10_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PosixEntryKind {
    Missing,
    File,
    Directory,
    Symlink,
    BrokenLink,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PosixEntryState {
    pub index: u32,
    pub kind: PosixEntryKind,
    pub fingerprint: EntryFingerprint,
    pub link_target: Option<String>,
}

pub async fn inspect_entries(
    workspace: &WslWorkspace,
    paths: &[String],
    cancellation: Option<CancellationSignal>,
) -> Result<Vec<PosixEntryState>, AppError> {
    if paths.is_empty() || paths.iter().any(|path| !path.starts_with('/')) {
        return Err(AppError::Validation {
            field: Some("entry.paths".to_string()),
            message: "WSL entry inspection requires absolute paths".to_string(),
        });
    }
    inspect_entries_with(paths, cancellation, |request, cancellation| async move {
        #[cfg(target_os = "linux")]
        {
            let _ = workspace;
            linux_entry_response(&request.paths, cancellation.as_ref())
        }
        #[cfg(not(target_os = "linux"))]
        {
            let message = environment_protocol::Message::InspectEntries { request };
            match cancellation {
                Some(cancellation) => {
                    workspace
                        .request_worker_payload_with_cancellation(message, cancellation)
                        .await
                }
                None => workspace.request_worker_payload(message).await,
            }
        }
    })
    .await
}

async fn inspect_entries_with<Send, SendFuture>(
    paths: &[String],
    cancellation: Option<CancellationSignal>,
    mut send: Send,
) -> Result<Vec<PosixEntryState>, AppError>
where
    Send: FnMut(environment_protocol::EntryFactsRequest, Option<CancellationSignal>) -> SendFuture,
    SendFuture: Future<Output = Result<environment_protocol::EntryFactsResponse, AppError>>,
{
    let deadline = tokio::time::Instant::now() + Duration::from_millis(ENTRY_DEADLINE_MILLIS);
    let mut entries = Vec::with_capacity(paths.len());
    for (batch_index, batch) in paths
        .chunks(environment_protocol::MAX_INSPECTION_ROOTS)
        .enumerate()
    {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        let deadline_millis = u64::try_from(remaining.as_millis())
            .unwrap_or(environment_protocol::MAX_REQUEST_DEADLINE_MILLIS)
            .min(environment_protocol::MAX_REQUEST_DEADLINE_MILLIS);
        if deadline_millis == 0 {
            return Err(AppError::WslCommandTimedOut);
        }
        let response = tokio::time::timeout_at(
            deadline,
            send(
                environment_protocol::EntryFactsRequest {
                    paths: batch.to_vec(),
                    deadline_millis,
                },
                cancellation.clone(),
            ),
        )
        .await
        .map_err(|_| AppError::WslCommandTimedOut)??;
        if response.facts.len() != batch.len() {
            return Err(protocol_error());
        }
        let offset = batch_index * environment_protocol::MAX_INSPECTION_ROOTS;
        let mut batch_entries = response
            .facts
            .into_iter()
            .enumerate()
            .map(|(index, fact)| {
                let kind = match fact.kind {
                    environment_protocol::EntryFactKind::Missing => PosixEntryKind::Missing,
                    environment_protocol::EntryFactKind::File => PosixEntryKind::File,
                    environment_protocol::EntryFactKind::Directory => PosixEntryKind::Directory,
                    environment_protocol::EntryFactKind::Symlink => PosixEntryKind::Symlink,
                    environment_protocol::EntryFactKind::BrokenLink => PosixEntryKind::BrokenLink,
                    environment_protocol::EntryFactKind::Other => PosixEntryKind::Other,
                };
                let link_target = fact
                    .link_target
                    .map(String::from_utf8)
                    .transpose()
                    .map_err(|_| protocol_error())?;
                let fingerprint = if kind == PosixEntryKind::Missing {
                    if fact.metadata.is_some() || link_target.is_some() {
                        return Err(protocol_error());
                    }
                    EntryFingerprint("entry-v1-missing".to_string())
                } else {
                    let metadata = fact.metadata.ok_or_else(protocol_error)?;
                    let values = [
                        metadata.device.to_string(),
                        metadata.inode.to_string(),
                        format!("{:x}", metadata.mode),
                        metadata.size.to_string(),
                        metadata.mtime_seconds.to_string(),
                        format!("{:09}", metadata.mtime_nanos),
                    ];
                    let mut hasher = Sha256::new();
                    hasher.update(b"skill-deck-wsl-entry-v1\0");
                    for value in values {
                        hasher.update(value.as_bytes());
                        hasher.update([0]);
                    }
                    if let Some(target) = &link_target {
                        hasher.update(target.as_bytes());
                    }
                    EntryFingerprint(format!("entry-v1-{:x}", hasher.finalize()))
                };
                Ok(PosixEntryState {
                    index: (offset + index) as u32,
                    kind,
                    fingerprint,
                    link_target,
                })
            })
            .collect::<Result<Vec<_>, AppError>>()?;
        entries.append(&mut batch_entries);
    }
    Ok(entries)
}

#[cfg(target_os = "linux")]
fn linux_entry_response(
    paths: &[String],
    cancellation: Option<&CancellationSignal>,
) -> Result<environment_protocol::EntryFactsResponse, AppError> {
    use std::os::unix::ffi::OsStrExt;

    let response = environment_engine::entry::inspect_entries_with_cancel(
        &environment_engine::entry::EntryRequest {
            paths: paths.iter().map(Into::into).collect(),
        },
        || cancellation.is_some_and(CancellationSignal::is_cancelled),
    )
    .map_err(|error| AppError::ExecutionFailed {
        message: format!("Linux entry inspection failed: {error}"),
    })?;
    Ok(environment_protocol::EntryFactsResponse {
        facts: response
            .facts
            .into_iter()
            .map(|fact| environment_protocol::EntryFact {
                kind: match fact.kind {
                    environment_engine::entry::EntryKind::Missing => {
                        environment_protocol::EntryFactKind::Missing
                    }
                    environment_engine::entry::EntryKind::File => {
                        environment_protocol::EntryFactKind::File
                    }
                    environment_engine::entry::EntryKind::Directory => {
                        environment_protocol::EntryFactKind::Directory
                    }
                    environment_engine::entry::EntryKind::Symlink => {
                        environment_protocol::EntryFactKind::Symlink
                    }
                    environment_engine::entry::EntryKind::BrokenLink => {
                        environment_protocol::EntryFactKind::BrokenLink
                    }
                    environment_engine::entry::EntryKind::Other => {
                        environment_protocol::EntryFactKind::Other
                    }
                },
                metadata: fact
                    .metadata
                    .map(|metadata| environment_protocol::EntryMetadata {
                        device: metadata.device,
                        inode: metadata.inode,
                        mode: metadata.mode,
                        size: metadata.size,
                        mtime_seconds: metadata.mtime_seconds,
                        mtime_nanos: metadata.mtime_nanos,
                    }),
                link_target: fact
                    .link_target
                    .map(|target| target.as_os_str().as_bytes().to_vec()),
            })
            .collect(),
    })
}

fn protocol_error() -> AppError {
    AppError::ConfigurationCorrupted {
        message: "invalid WSL Worker entry response".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;

    #[tokio::test]
    async fn entry_inspection_batches_preserve_order_and_the_transport_limit() {
        let paths = (0..513)
            .map(|index| format!("/tmp/skill-{index}"))
            .collect::<Vec<_>>();
        let batch_sizes = Arc::new(Mutex::new(Vec::new()));
        let observed = batch_sizes.clone();
        let entries = inspect_entries_with(&paths, None, move |request, _| {
            observed.lock().unwrap().push(request.paths.len());
            async move {
                Ok(environment_protocol::EntryFactsResponse {
                    facts: request
                        .paths
                        .into_iter()
                        .map(|_| environment_protocol::EntryFact {
                            kind: environment_protocol::EntryFactKind::Missing,
                            metadata: None,
                            link_target: None,
                        })
                        .collect(),
                })
            }
        })
        .await
        .unwrap();

        assert_eq!(*batch_sizes.lock().unwrap(), vec![256, 256, 1]);
        assert_eq!(entries.len(), paths.len());
        assert_eq!(entries[256].index, 256);
        assert_eq!(entries[512].index, 512);
    }
}

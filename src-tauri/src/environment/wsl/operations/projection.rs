use std::future::Future;
use std::time::Duration;

use crate::core::mutation::CancellationSignal;
use crate::environment::wsl::WslWorkspace;
use crate::error::AppError;

const PROJECTION_DEADLINE_MILLIS: u64 = 10_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectedPosixTarget {
    pub index: u32,
    pub anchor_device: u64,
    pub anchor_inode: u64,
    pub physical_destination: String,
    pub relative_components: Vec<String>,
    pub storage_projection: String,
}

pub async fn project_targets(
    workspace: &WslWorkspace,
    destinations: &[String],
    cancellation: Option<CancellationSignal>,
) -> Result<Vec<ProjectedPosixTarget>, AppError> {
    if destinations.is_empty() || destinations.iter().any(|path| !path.starts_with('/')) {
        return Err(AppError::Validation {
            field: Some("projection.destinations".to_string()),
            message: "WSL target projection requires absolute destinations".to_string(),
        });
    }
    project_targets_with(
        destinations,
        cancellation,
        |request, cancellation| async move {
            let message = environment_protocol::Message::ProjectTargets { request };
            match cancellation {
                Some(cancellation) => {
                    workspace
                        .request_worker_payload_with_cancellation(message, cancellation)
                        .await
                }
                None => workspace.request_worker_payload(message).await,
            }
        },
    )
    .await
}

async fn project_targets_with<Send, SendFuture>(
    destinations: &[String],
    cancellation: Option<CancellationSignal>,
    mut send: Send,
) -> Result<Vec<ProjectedPosixTarget>, AppError>
where
    Send: FnMut(environment_protocol::ProjectionRequest, Option<CancellationSignal>) -> SendFuture,
    SendFuture: Future<Output = Result<environment_protocol::ProjectionResponse, AppError>>,
{
    let deadline = tokio::time::Instant::now() + Duration::from_millis(PROJECTION_DEADLINE_MILLIS);
    let mut projected = Vec::with_capacity(destinations.len());
    for (batch_index, batch) in destinations
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
                environment_protocol::ProjectionRequest {
                    destinations: batch.to_vec(),
                    deadline_millis,
                },
                cancellation.clone(),
            ),
        )
        .await
        .map_err(|_| AppError::WslCommandTimedOut)??;
        if response.targets.len() != batch.len() {
            return Err(protocol_error());
        }
        let offset = batch_index * environment_protocol::MAX_INSPECTION_ROOTS;
        let mut batch_targets = response
            .targets
            .into_iter()
            .enumerate()
            .map(|(index, target)| {
                let physical_destination =
                    String::from_utf8(target.physical_destination).map_err(|_| protocol_error())?;
                let relative_components = target
                    .relative_components
                    .into_iter()
                    .map(String::from_utf8)
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|_| protocol_error())?;
                if !physical_destination.starts_with('/')
                    || target.storage_projection.trim().is_empty()
                    || relative_components.is_empty()
                    || relative_components.iter().any(|component| {
                        component.is_empty()
                            || matches!(component.as_str(), "." | "..")
                            || component.contains('/')
                    })
                {
                    return Err(protocol_error());
                }
                Ok(ProjectedPosixTarget {
                    index: (offset + index) as u32,
                    anchor_device: target.anchor_device,
                    anchor_inode: target.anchor_inode,
                    physical_destination,
                    relative_components,
                    storage_projection: target.storage_projection,
                })
            })
            .collect::<Result<Vec<_>, AppError>>()?;
        projected.append(&mut batch_targets);
    }
    Ok(projected)
}

fn protocol_error() -> AppError {
    AppError::ConfigurationCorrupted {
        message: "invalid WSL Worker target projection response".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;

    #[tokio::test]
    async fn projection_batches_preserve_order_and_the_transport_limit() {
        let destinations = (0..513)
            .map(|index| format!("/tmp/skill-{index}"))
            .collect::<Vec<_>>();
        let batch_sizes = Arc::new(Mutex::new(Vec::new()));
        let observed = batch_sizes.clone();
        let projected = project_targets_with(&destinations, None, move |request, _| {
            observed.lock().unwrap().push(request.destinations.len());
            async move {
                Ok(environment_protocol::ProjectionResponse {
                    targets: request
                        .destinations
                        .into_iter()
                        .map(|destination| environment_protocol::ProjectedTarget {
                            anchor_device: 1,
                            anchor_inode: 2,
                            physical_destination: destination.as_bytes().to_vec(),
                            relative_components: vec![destination
                                .rsplit('/')
                                .next()
                                .unwrap()
                                .as_bytes()
                                .to_vec()],
                            storage_projection: r"\\wsl.localhost\Ubuntu\tmp".to_string(),
                        })
                        .collect(),
                })
            }
        })
        .await
        .unwrap();

        assert_eq!(*batch_sizes.lock().unwrap(), vec![256, 256, 1]);
        assert_eq!(projected.len(), destinations.len());
        assert_eq!(projected[256].index, 256);
        assert_eq!(projected[512].physical_destination, destinations[512]);
    }

    #[cfg(target_os = "windows")]
    #[tokio::test]
    #[ignore = "requires SKILL_DECK_TEST_WSL_DISTRO and a matching real WSL Worker"]
    async fn real_wsl_worker_projects_and_inspects_more_than_one_transport_batch() {
        use crate::environment::wsl::operations::entry::inspect_entries;
        use crate::environment::wsl::WslRuntime;

        let distro_name =
            std::env::var("SKILL_DECK_TEST_WSL_DISTRO").expect("set SKILL_DECK_TEST_WSL_DISTRO");
        let runtime = WslRuntime::for_wsl_test();
        runtime.connect(&distro_name).await.expect("connect Worker");
        let workspace = runtime.workspace(&distro_name).unwrap();
        let paths = (0..513)
            .map(|index| format!("/tmp/skill-deck-batch-{index}"))
            .collect::<Vec<_>>();

        let projected = project_targets(&workspace, &paths, None)
            .await
            .expect("project all paths");
        let entries = inspect_entries(&workspace, &paths, None)
            .await
            .expect("inspect all paths");

        assert_eq!(projected.len(), paths.len());
        assert_eq!(entries.len(), paths.len());
        assert_eq!(projected[512].index, 512);
        assert_eq!(entries[512].index, 512);
    }
}

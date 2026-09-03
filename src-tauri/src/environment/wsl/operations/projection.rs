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
    let message = environment_protocol::Message::ProjectTargets {
        request: environment_protocol::ProjectionRequest {
            destinations: destinations.to_vec(),
            deadline_millis: PROJECTION_DEADLINE_MILLIS,
        },
    };
    let response: environment_protocol::ProjectionResponse = match cancellation {
        Some(cancellation) => {
            workspace
                .request_worker_payload_with_cancellation(message, cancellation)
                .await?
        }
        None => workspace.request_worker_payload(message).await?,
    };
    if response.targets.len() != destinations.len() {
        return Err(protocol_error());
    }
    response
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
                index: index as u32,
                anchor_device: target.anchor_device,
                anchor_inode: target.anchor_inode,
                physical_destination,
                relative_components,
                storage_projection: target.storage_projection,
            })
        })
        .collect()
}

fn protocol_error() -> AppError {
    AppError::ConfigurationCorrupted {
        message: "invalid WSL Worker target projection response".to_string(),
    }
}

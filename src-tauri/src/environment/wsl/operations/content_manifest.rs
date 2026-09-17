use crate::core::mutation::CancellationSignal;
use crate::environment::content_manifest::{
    ContentManifest, ContentManifestRecord, ContentManifestTarget,
};
use crate::environment::runtime::ExecutionBackend;
use crate::environment::types::{normalized_wsl_distro_name, EnvironmentRef};
use crate::environment::wsl::WslWorkspace;
use crate::error::AppError;

const MANIFEST_DEADLINE_MILLIS: u64 = 60_000;

pub async fn inspect(
    workspace: &WslWorkspace,
    target: &ContentManifestTarget,
    cancellation: Option<CancellationSignal>,
) -> Result<ContentManifest, AppError> {
    let expected_environment = EnvironmentRef::Wsl {
        distro_name: workspace.distro_name().to_string(),
    };
    if target.location.environment != expected_environment
        || !matches!(
            &target.key.backend,
            ExecutionBackend::WslPosix { distro_name }
                if distro_name == &normalized_wsl_distro_name(workspace.distro_name())
        )
        || !target.location.native_path.starts_with('/')
    {
        return Err(AppError::StorageUnsupported {
            path: target.location.native_path.clone(),
        });
    }
    inspect_path(workspace, &target.location.native_path, cancellation).await
}

pub(crate) async fn inspect_path(
    workspace: &WslWorkspace,
    path: &str,
    cancellation: Option<CancellationSignal>,
) -> Result<ContentManifest, AppError> {
    #[cfg(target_os = "linux")]
    let _ = workspace;
    if !path.starts_with('/') {
        return Err(AppError::StorageUnsupported {
            path: path.to_string(),
        });
    }
    #[cfg(target_os = "linux")]
    let response = linux_manifest_response(path, cancellation.as_ref())?;
    #[cfg(not(target_os = "linux"))]
    let response: environment_protocol::ManifestResponse = {
        let message = environment_protocol::Message::BuildManifest {
            request: environment_protocol::ManifestRequest {
                root: path.to_string(),
                deadline_millis: MANIFEST_DEADLINE_MILLIS,
            },
        };
        match cancellation {
            Some(cancellation) => {
                workspace
                    .request_worker_payload_with_cancellation(message, cancellation)
                    .await?
            }
            None => workspace.request_worker_payload(message).await?,
        }
    };
    let records =
        response
            .records
            .into_iter()
            .map(|record| {
                let path = String::from_utf8(record.relative_path).map_err(|_| protocol_error())?;
                match record.kind {
                    environment_protocol::ManifestRecordKind::Directory
                        if record.digest.is_none()
                            && !record.executable
                            && record.symlink_target.is_none() =>
                    {
                        ContentManifestRecord::directory(path)
                    }
                    environment_protocol::ManifestRecordKind::File
                        if record.digest.is_some() && record.symlink_target.is_none() =>
                    {
                        ContentManifestRecord::file(
                            path,
                            record.digest.expect("checked file digest"),
                            record.executable,
                        )
                    }
                    environment_protocol::ManifestRecordKind::Symlink
                        if record.digest.is_none() && !record.executable =>
                    {
                        let target = record.symlink_target.ok_or_else(protocol_error).and_then(
                            |target| String::from_utf8(target).map_err(|_| protocol_error()),
                        )?;
                        ContentManifestRecord::symlink(path, target)
                    }
                    _ => return Err(protocol_error()),
                }
                .map_err(|_| protocol_error())
            })
            .collect::<Result<Vec<_>, _>>()?;
    ContentManifest::from_records(records)
}

#[cfg(target_os = "linux")]
fn linux_manifest_response(
    path: &str,
    cancellation: Option<&CancellationSignal>,
) -> Result<environment_protocol::ManifestResponse, AppError> {
    use std::os::unix::ffi::OsStrExt;

    let response = environment_engine::manifest::build_manifest_with_cancel(
        &environment_engine::manifest::ManifestRequest { root: path.into() },
        || cancellation.is_some_and(CancellationSignal::is_cancelled),
    )
    .map_err(|error| AppError::ExecutionFailed {
        message: format!("Linux content manifest failed: {error}"),
    })?;
    Ok(environment_protocol::ManifestResponse {
        records: response
            .records
            .into_iter()
            .map(|record| environment_protocol::ManifestRecord {
                relative_path: record.relative_path.as_os_str().as_bytes().to_vec(),
                kind: match record.kind {
                    environment_engine::manifest::ManifestKind::Directory => {
                        environment_protocol::ManifestRecordKind::Directory
                    }
                    environment_engine::manifest::ManifestKind::File => {
                        environment_protocol::ManifestRecordKind::File
                    }
                    environment_engine::manifest::ManifestKind::Symlink => {
                        environment_protocol::ManifestRecordKind::Symlink
                    }
                },
                digest: record.digest,
                executable: record.executable,
                symlink_target: record
                    .symlink_target
                    .map(|target| target.as_os_str().as_bytes().to_vec()),
            })
            .collect(),
    })
}

fn protocol_error() -> AppError {
    AppError::ConfigurationCorrupted {
        message: "invalid WSL Worker content manifest response".to_string(),
    }
}

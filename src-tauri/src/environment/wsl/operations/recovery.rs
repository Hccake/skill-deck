use tokio::time::Duration;

use crate::environment::recovery::{
    validate_recovery_marker, RecoveryFuture, RecoveryMarker, RecoveryMarkerLoad,
    RecoveryMarkerRef, RecoveryMarkerStore,
};
use crate::environment::types::{same_environment_identity, EnvironmentRef, ResourceLocator};
use crate::environment::wsl::WslSession;
use crate::environment::wsl_protocol::{
    wsl_operation, WslOperationDescriptor, WslOperationExecutor, WslOperationRequest,
    DEFAULT_WSL_STDERR_LIMIT, DEFAULT_WSL_STDOUT_LIMIT,
};
use crate::error::{AppError, RecoveryResourceId};

const ENUMERATE_SCRIPT: &str = include_str!("../scripts/recovery.sh");

const WRITE_MARKER_SCRIPT: &str = include_str!("../scripts/recovery.sh");

const REMOVE_MARKER_SCRIPT: &str = include_str!("../scripts/recovery.sh");

const CLEANUP_RECOVERY_SCRIPT: &str = include_str!("../scripts/recovery.sh");
const ENUMERATE_OPERATION: WslOperationDescriptor =
    wsl_operation("recovery", "enumerate", ENUMERATE_SCRIPT);
const WRITE_MARKER_OPERATION: WslOperationDescriptor =
    wsl_operation("recovery", "write-marker", WRITE_MARKER_SCRIPT);
const REMOVE_MARKER_OPERATION: WslOperationDescriptor =
    wsl_operation("recovery", "remove-marker", REMOVE_MARKER_SCRIPT);
const CLEANUP_RECOVERY_OPERATION: WslOperationDescriptor =
    wsl_operation("recovery", "cleanup", CLEANUP_RECOVERY_SCRIPT);

pub fn parse_enumeration(
    bytes: &[u8],
    environment: EnvironmentRef,
) -> Result<Vec<RecoveryMarkerLoad>, AppError> {
    let mut fields = bytes.split(|byte| *byte == 0);
    if text(fields.next())? != "1" {
        return Err(protocol_error());
    }
    let mut loads = Vec::new();
    while let Some(tag) = fields.next() {
        if tag.is_empty() {
            continue;
        }
        if text(Some(tag))? != "R" {
            return Err(protocol_error());
        }
        let root = text(fields.next())?.to_string();
        let status = text(fields.next())?;
        let content = fields.next().ok_or_else(protocol_error)?;
        let managed_root = ResourceLocator {
            environment: environment.clone(),
            native_path: root.clone(),
        };
        let parsed = (|| {
            if status != "present" {
                return Err(AppError::ConfigurationCorrupted {
                    message: format!("WSL recovery marker is {status}"),
                });
            }
            let marker: RecoveryMarker = serde_json::from_slice(content)?;
            validate_recovery_marker(&marker)?;
            let expected_id = root
                .rsplit('/')
                .next()
                .and_then(|name| name.strip_prefix("skill-deck-operation-"))
                .ok_or_else(protocol_error)?;
            if !same_environment_identity(&marker.environment, &environment)
                || marker.resource_id.as_str() != expected_id
            {
                return Err(AppError::ConfigurationCorrupted {
                    message: "WSL recovery marker does not match its managed root".to_string(),
                });
            }
            Ok(marker)
        })();
        match parsed {
            Ok(marker) => loads.push(RecoveryMarkerLoad::Valid {
                marker_ref: RecoveryMarkerRef {
                    resource_id: marker.resource_id.clone(),
                    environment: environment.clone(),
                    managed_root,
                },
                marker,
            }),
            Err(error) => loads.push(RecoveryMarkerLoad::Invalid {
                managed_root,
                error,
            }),
        }
    }
    Ok(loads)
}

fn text(field: Option<&[u8]>) -> Result<&str, AppError> {
    std::str::from_utf8(field.ok_or_else(protocol_error)?).map_err(|_| protocol_error())
}

fn protocol_error() -> AppError {
    AppError::ConfigurationCorrupted {
        message: "invalid WSL recovery enumeration response".to_string(),
    }
}

pub struct WslRecoveryMarkerStore {
    session: WslSession,
    namespace: String,
}

impl WslRecoveryMarkerStore {
    pub fn new(session: WslSession) -> Self {
        Self {
            session,
            namespace: "/tmp".to_string(),
        }
    }

    fn environment_ref(&self) -> EnvironmentRef {
        EnvironmentRef::Wsl {
            distro_name: self.session.distro_name.clone(),
        }
    }

    fn root(&self, id: &RecoveryResourceId) -> String {
        format!("{}/skill-deck-operation-{}", self.namespace, id.as_str())
    }

    fn marker_ref(&self, id: RecoveryResourceId) -> RecoveryMarkerRef {
        let environment = self.environment_ref();
        RecoveryMarkerRef {
            managed_root: ResourceLocator {
                environment: environment.clone(),
                native_path: self.root(&id),
            },
            resource_id: id,
            environment,
        }
    }

    fn verify_ref(&self, marker_ref: &RecoveryMarkerRef) -> Result<(), AppError> {
        let expected_environment = self.environment_ref();
        if !same_environment_identity(&marker_ref.environment, &expected_environment)
            || !same_environment_identity(
                &marker_ref.managed_root.environment,
                &expected_environment,
            )
            || marker_ref.managed_root.native_path != self.root(&marker_ref.resource_id)
        {
            return Err(AppError::UnsafePath {
                path: marker_ref.managed_root.native_path.clone(),
                reason: "WSL recovery root is outside the managed namespace".to_string(),
            });
        }
        Ok(())
    }

    async fn run(
        &self,
        operation: &WslOperationDescriptor,
        args: Vec<String>,
        stdin: Vec<u8>,
        stdout_limit: usize,
    ) -> Result<Vec<u8>, AppError> {
        let output = WslOperationExecutor::execute(
            operation,
            WslOperationRequest {
                session: self.session.clone(),
                args,
                stdin,
                timeout: Duration::from_secs(30),
                stdout_limit,
                stderr_limit: DEFAULT_WSL_STDERR_LIMIT,
                cancellation: None,
            },
        )
        .await?;
        Ok(output.stdout)
    }

    async fn write(
        &self,
        marker: &RecoveryMarker,
        mode: &str,
    ) -> Result<RecoveryMarkerRef, AppError> {
        validate_recovery_marker(marker)?;
        if !same_environment_identity(&marker.environment, &self.environment_ref()) {
            return Err(AppError::StaleEnvironment);
        }
        let response = self
            .run(
                &WRITE_MARKER_OPERATION,
                vec![
                    self.namespace.clone(),
                    marker.resource_id.as_str().to_string(),
                    mode.to_string(),
                ],
                serde_json::to_vec(marker)?,
                32,
            )
            .await?;
        parse_write_response(&response)?;
        Ok(self.marker_ref(marker.resource_id.clone()))
    }
}

impl RecoveryMarkerStore for WslRecoveryMarkerStore {
    fn environment(&self) -> EnvironmentRef {
        self.environment_ref()
    }

    fn validate_managed_root(&self, root: &ResourceLocator) -> Result<(), AppError> {
        let expected_environment = self.environment_ref();
        let Some(name) = root.native_path.strip_prefix("/tmp/skill-deck-operation-") else {
            return Err(AppError::UnsafePath {
                path: root.native_path.clone(),
                reason: "WSL recovery root is outside the managed namespace".to_string(),
            });
        };
        if !same_environment_identity(&root.environment, &expected_environment)
            || name.is_empty()
            || !name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
        {
            return Err(AppError::UnsafePath {
                path: root.native_path.clone(),
                reason: "WSL recovery root is outside the managed namespace".to_string(),
            });
        }
        Ok(())
    }

    fn create<'a>(
        &'a self,
        marker: &'a RecoveryMarker,
    ) -> RecoveryFuture<'a, Result<RecoveryMarkerRef, AppError>> {
        Box::pin(async move { self.write(marker, "create").await })
    }

    fn update<'a>(
        &'a self,
        marker_ref: &'a RecoveryMarkerRef,
        marker: &'a RecoveryMarker,
    ) -> RecoveryFuture<'a, Result<(), AppError>> {
        Box::pin(async move {
            self.verify_ref(marker_ref)?;
            if marker.resource_id != marker_ref.resource_id {
                return Err(AppError::StaleTarget);
            }
            self.write(marker, "update").await?;
            Ok(())
        })
    }

    fn enumerate<'a>(&'a self) -> RecoveryFuture<'a, Result<Vec<RecoveryMarkerLoad>, AppError>> {
        Box::pin(async move {
            let response = self
                .run(
                    &ENUMERATE_OPERATION,
                    vec![self.namespace.clone()],
                    Vec::new(),
                    DEFAULT_WSL_STDOUT_LIMIT,
                )
                .await?;
            parse_enumeration(&response, self.environment_ref())
        })
    }

    fn remove<'a>(
        &'a self,
        marker_ref: &'a RecoveryMarkerRef,
    ) -> RecoveryFuture<'a, Result<(), AppError>> {
        Box::pin(async move {
            self.verify_ref(marker_ref)?;
            let response = self
                .run(
                    &REMOVE_MARKER_OPERATION,
                    vec![
                        self.namespace.clone(),
                        marker_ref.resource_id.as_str().to_string(),
                    ],
                    Vec::new(),
                    32,
                )
                .await?;
            parse_write_response(&response)
        })
    }

    fn cleanup<'a>(
        &'a self,
        marker_ref: &'a RecoveryMarkerRef,
        marker: &'a RecoveryMarker,
    ) -> RecoveryFuture<'a, Result<(), AppError>> {
        Box::pin(async move {
            self.verify_ref(marker_ref)?;
            validate_recovery_marker(marker)?;
            if marker.resource_id != marker_ref.resource_id
                || !same_environment_identity(&marker.environment, &marker_ref.environment)
                || marker.kind != crate::environment::recovery::RecoveryMarkerKind::CleanupOnly
            {
                return Err(AppError::StaleTarget);
            }
            let mut args = vec![
                self.namespace.clone(),
                marker_ref.resource_id.as_str().to_string(),
            ];
            args.extend(
                marker
                    .entries
                    .iter()
                    .filter_map(|entry| entry.backup.as_ref())
                    .map(|backup| backup.native_path.clone()),
            );
            let response = self
                .run(
                    &CLEANUP_RECOVERY_OPERATION,
                    args,
                    serde_json::to_vec(marker)?,
                    32,
                )
                .await?;
            parse_write_response(&response)
        })
    }
}

fn parse_write_response(bytes: &[u8]) -> Result<(), AppError> {
    (bytes == b"1\0").then_some(()).ok_or_else(protocol_error)
}

#[cfg(all(test, target_os = "linux"))]
#[allow(
    clippy::disallowed_methods,
    reason = "recovery 协议测试需要直接执行被验证的 shell fixture"
)]
mod tests {
    use std::fs;
    use std::io::Write;
    use std::process::{Command, Stdio};

    use tempfile::tempdir;

    use super::*;
    use crate::environment::recovery::{
        RecoveryEntryPhase, RecoveryMarker, RecoveryMarkerEntry, RecoveryMarkerKind,
        RecoveryMarkerLoad, RECOVERY_MARKER_SCHEMA_VERSION,
    };
    use crate::environment::types::{EnvironmentRef, ResourceLocator};
    use crate::error::RecoveryResourceId;

    fn marker(environment: EnvironmentRef, id: &str) -> RecoveryMarker {
        RecoveryMarker {
            schema_version: RECOVERY_MARKER_SCHEMA_VERSION,
            resource_id: RecoveryResourceId::parse(id).unwrap(),
            kind: RecoveryMarkerKind::RecoveryRequired,
            environment: environment.clone(),
            operation_id: "operation-1".to_string(),
            unit_id: "unit-1".to_string(),
            created_at_epoch_ms: 1,
            entries: vec![RecoveryMarkerEntry {
                physical_target_digest: "target-1".to_string(),
                destination: ResourceLocator {
                    environment: environment.clone(),
                    native_path: "/home/alice/.agents/skills/demo".to_string(),
                },
                backup: Some(ResourceLocator {
                    environment,
                    native_path: "/home/alice/.agents/skills/.skill-deck-backup-demo".to_string(),
                }),
                expected_state: crate::environment::recovery::RecoveryExpectedEntryState::Present,
                original_fingerprint: "entry-v1-original".to_string(),
                phase: RecoveryEntryPhase::RestoreFailed,
            }],
        }
    }

    #[test]
    fn restart_enumeration_keeps_valid_marker_and_isolates_invalid_sibling() {
        let temp = tempdir().unwrap();
        let environment = EnvironmentRef::Wsl {
            distro_name: "Ubuntu".to_string(),
        };
        let valid_root = temp.path().join("skill-deck-operation-valid-id");
        let invalid_root = temp.path().join("skill-deck-operation-invalid-id");
        fs::create_dir(&valid_root).unwrap();
        fs::create_dir(&invalid_root).unwrap();
        fs::write(
            valid_root.join("recovery.json"),
            serde_json::to_vec(&marker(environment.clone(), "valid-id")).unwrap(),
        )
        .unwrap();
        fs::write(invalid_root.join("recovery.json"), b"{broken").unwrap();

        let output = Command::new("/bin/sh")
            .arg("-c")
            .arg(ENUMERATE_SCRIPT)
            .arg("--")
            .arg("enumerate")
            .arg(temp.path())
            .output()
            .unwrap();
        assert!(output.status.success());
        let loads = parse_enumeration(&output.stdout, environment).unwrap();

        assert_eq!(loads.len(), 2);
        assert_eq!(
            loads
                .iter()
                .filter(|load| matches!(load, RecoveryMarkerLoad::Valid { .. }))
                .count(),
            1
        );
        assert_eq!(
            loads
                .iter()
                .filter(|load| matches!(load, RecoveryMarkerLoad::Invalid { .. }))
                .count(),
            1
        );
    }

    #[test]
    fn marker_write_requires_matching_operation_owner_and_updates_atomically() {
        let temp = tempdir().unwrap();
        let environment = EnvironmentRef::Wsl {
            distro_name: "Ubuntu".to_string(),
        };
        let root = temp.path().join("skill-deck-operation-owned-id");
        let mut initial = marker(environment, "owned-id");

        let output = run_write(temp.path(), "owned-id", "create", &initial);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(output.stdout, b"1\0");
        assert_eq!(
            serde_json::from_slice::<RecoveryMarker>(
                &fs::read(root.join("recovery.json")).unwrap()
            )
            .unwrap(),
            initial
        );

        initial.kind = RecoveryMarkerKind::CleanupOnly;
        assert!(run_write(temp.path(), "owned-id", "update", &initial)
            .status
            .success());
        assert_eq!(
            serde_json::from_slice::<RecoveryMarker>(
                &fs::read(root.join("recovery.json")).unwrap()
            )
            .unwrap(),
            initial
        );

        fs::write(root.join(".skill-deck-owner"), b"1\nother-id\n").unwrap();
        assert!(!run_write(temp.path(), "owned-id", "update", &initial)
            .status
            .success());
    }

    #[test]
    fn marker_create_initializes_the_owned_operation_root() {
        let temp = tempdir().unwrap();
        let environment = EnvironmentRef::Wsl {
            distro_name: "Ubuntu".to_string(),
        };
        let value = marker(environment, "prepared-id");

        let output = run_write(temp.path(), "prepared-id", "create", &value);

        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let root = temp.path().join("skill-deck-operation-prepared-id");
        assert_eq!(
            fs::read(root.join(".skill-deck-owner")).unwrap(),
            b"1\nprepared-id\n"
        );
        assert_eq!(
            serde_json::from_slice::<RecoveryMarker>(
                &fs::read(root.join("recovery.json")).unwrap()
            )
            .unwrap(),
            value
        );
    }

    #[test]
    fn confirmed_cleanup_removes_owned_backups_and_the_managed_marker_root() {
        let temp = tempdir().unwrap();
        let environment = EnvironmentRef::Wsl {
            distro_name: "Ubuntu".to_string(),
        };
        let root = temp.path().join("skill-deck-operation-cleanup-id");
        fs::create_dir(&root).unwrap();
        fs::write(root.join(".skill-deck-owner"), b"1\ncleanup-id\n").unwrap();
        let backup = temp.path().join("targets/.skill-deck-backup-demo");
        fs::create_dir_all(&backup).unwrap();
        fs::write(backup.join("SKILL.md"), b"backup").unwrap();
        let destination = temp.path().join("targets/demo");
        let mut value = marker(environment, "cleanup-id");
        value.kind = RecoveryMarkerKind::CleanupOnly;
        value.entries[0].destination.native_path = destination.to_string_lossy().into_owned();
        value.entries[0].backup = Some(ResourceLocator {
            environment: value.environment.clone(),
            native_path: backup.to_string_lossy().into_owned(),
        });
        fs::write(
            root.join("recovery.json"),
            serde_json::to_vec(&value).unwrap(),
        )
        .unwrap();

        let mut child = Command::new("/bin/sh")
            .arg("-c")
            .arg(CLEANUP_RECOVERY_SCRIPT)
            .arg("--")
            .arg("cleanup")
            .arg(temp.path())
            .arg("cleanup-id")
            .arg(&backup)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(&serde_json::to_vec(&value).unwrap())
            .unwrap();
        let output = child.wait_with_output().unwrap();

        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(output.stdout, b"1\0");
        assert!(!backup.exists());
        assert!(!root.exists());
    }

    fn run_write(
        namespace: &std::path::Path,
        id: &str,
        mode: &str,
        marker: &RecoveryMarker,
    ) -> std::process::Output {
        let mut child = Command::new("/bin/sh")
            .arg("-c")
            .arg(WRITE_MARKER_SCRIPT)
            .arg("--")
            .arg("write-marker")
            .arg(namespace)
            .arg(id)
            .arg(mode)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(&serde_json::to_vec(marker).unwrap())
            .unwrap();
        child.wait_with_output().unwrap()
    }
}

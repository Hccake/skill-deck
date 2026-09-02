#![cfg(target_os = "linux")]
#![allow(
    clippy::disallowed_methods,
    reason = "Worker 集成测试需要直接启动当前 crate 构建的受控二进制"
)]

use std::process::Stdio;
use std::time::Duration;

use environment_protocol::{codec, decode, Envelope, Message, PathKind, WireRecord};
#[cfg(target_os = "linux")]
use environment_protocol::{
    decode_inspection_response, decode_payload, AcquirePayloadFromSourceRequest,
    DocumentWritePreparation, InspectionRequest, InspectionRoot, MapHostPathsRequest,
    MapHostPathsResponse, MutationEntry, MutationEntryAction, MutationUnitOutcome,
    MutationUnitRequest, OpenLocalSourceRequest, PayloadReadyResponse, VerifyPayloadRequest,
};
use futures_util::StreamExt;
use tokio::process::Command;
use tokio::time::timeout;
use tokio_util::codec::FramedRead;
use wsl_environment_worker::file_sha256;

#[tokio::test]
async fn worker_binary_handshakes_observes_home_and_shuts_down() {
    let binary = env!("CARGO_BIN_EXE_wsl-environment-worker");
    let build_id = file_sha256(std::path::Path::new(binary)).unwrap();
    let home = tempfile::tempdir().unwrap();
    let mut child = Command::new(binary)
        .env("WSL_DISTRO_NAME", "Ubuntu")
        .env("USER", "alice")
        .env("HOME", home.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let (writer, writer_task) = environment_protocol::spawn_writer(stdin);
    let mut reader = FramedRead::new(stdout, codec());

    writer
        .send_control(WireRecord::Control(Envelope {
            request_id: 1,
            message: Message::Handshake {
                build_id: build_id.clone(),
            },
        }))
        .await
        .unwrap();
    assert!(matches!(
        next_message(&mut reader).await,
        Envelope {
            request_id: 1,
            message: Message::HandshakeResult {
                build_id: actual,
                distro,
                user,
                home: actual_home,
                ..
            },
        } if actual == build_id
            && distro == "Ubuntu"
            && user == "alice"
            && actual_home == home.path().to_string_lossy()
    ));

    writer
        .send_control(WireRecord::Control(Envelope {
            request_id: 2,
            message: Message::ObservePath {
                path: home.path().to_string_lossy().into_owned(),
            },
        }))
        .await
        .unwrap();
    assert_eq!(
        next_message(&mut reader).await,
        Envelope {
            request_id: 2,
            message: Message::PathObserved {
                kind: PathKind::Directory,
            },
        }
    );

    writer
        .send_control(WireRecord::Control(Envelope {
            request_id: 3,
            message: Message::Shutdown,
        }))
        .await
        .unwrap();
    drop(writer);
    writer_task.await.unwrap().unwrap();
    assert!(timeout(Duration::from_secs(2), child.wait())
        .await
        .unwrap()
        .unwrap()
        .success());
}

#[tokio::test]
async fn worker_maps_host_paths_with_structured_wslpath_arguments() {
    use std::os::unix::fs::PermissionsExt;

    let binary = env!("CARGO_BIN_EXE_wsl-environment-worker");
    let build_id = file_sha256(std::path::Path::new(binary)).unwrap();
    let home = tempfile::tempdir().unwrap();
    let tools = tempfile::tempdir().unwrap();
    let wslpath = tools.path().join("wslpath");
    std::fs::write(
        &wslpath,
        r#"#!/bin/sh
[ "$1" = "-u" ] && [ "$2" = "--" ] || exit 64
case "$3" in
  'C:\Code\Skill Deck') printf '%s\n' '/mnt/c/Code/Skill Deck' ;;
  '\\server\share\项目') printf '%s\n' '/mnt/server/share/项目' ;;
  *) exit 65 ;;
esac
"#,
    )
    .unwrap();
    std::fs::set_permissions(&wslpath, std::fs::Permissions::from_mode(0o755)).unwrap();
    let mut child = Command::new(binary)
        .env("WSL_DISTRO_NAME", "Ubuntu")
        .env("USER", "alice")
        .env("HOME", home.path())
        .env("PATH", tools.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let (writer, writer_task) = environment_protocol::spawn_writer(stdin);
    let mut reader = FramedRead::new(stdout, codec());

    writer
        .send_control(WireRecord::Control(Envelope {
            request_id: 1,
            message: Message::Handshake { build_id },
        }))
        .await
        .unwrap();
    assert!(matches!(
        next_message(&mut reader).await.message,
        Message::HandshakeResult { .. }
    ));

    writer
        .send_control(WireRecord::Control(Envelope {
            request_id: 2,
            message: Message::MapHostPaths {
                request: MapHostPathsRequest {
                    paths: vec![
                        r"C:\Code\Skill Deck".to_string(),
                        r"\\server\share\项目".to_string(),
                    ],
                    deadline_millis: 5_000,
                },
            },
        }))
        .await
        .unwrap();
    let response: MapHostPathsResponse =
        decode_payload(&next_transfer(&mut reader, 2).await).unwrap();
    assert_eq!(
        response.mapped,
        ["/mnt/c/Code/Skill Deck", "/mnt/server/share/项目"]
    );

    writer
        .send_control(WireRecord::Control(Envelope {
            request_id: 3,
            message: Message::Shutdown,
        }))
        .await
        .unwrap();
    drop(writer);
    writer_task.await.unwrap().unwrap();
    assert!(timeout(Duration::from_secs(2), child.wait())
        .await
        .unwrap()
        .unwrap()
        .success());
}

#[tokio::test]
async fn host_path_mapping_deadline_reaps_wslpath() {
    use std::os::unix::fs::PermissionsExt;

    let binary = env!("CARGO_BIN_EXE_wsl-environment-worker");
    let build_id = file_sha256(std::path::Path::new(binary)).unwrap();
    let home = tempfile::tempdir().unwrap();
    let tools = tempfile::tempdir().unwrap();
    let pid_file = tools.path().join("wslpath.pid");
    let wslpath = tools.path().join("wslpath");
    std::fs::write(
        &wslpath,
        "#!/bin/sh\nprintf '%s\\n' \"$$\" > \"$SKILL_DECK_TEST_PID\"\nexec /bin/sleep 30\n",
    )
    .unwrap();
    std::fs::set_permissions(&wslpath, std::fs::Permissions::from_mode(0o755)).unwrap();
    let mut child = Command::new(binary)
        .env("WSL_DISTRO_NAME", "Ubuntu")
        .env("USER", "alice")
        .env("HOME", home.path())
        .env("PATH", tools.path())
        .env("SKILL_DECK_TEST_PID", &pid_file)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let (writer, writer_task) = environment_protocol::spawn_writer(stdin);
    let mut reader = FramedRead::new(stdout, codec());

    writer
        .send_control(WireRecord::Control(Envelope {
            request_id: 1,
            message: Message::Handshake { build_id },
        }))
        .await
        .unwrap();
    assert!(matches!(
        next_message(&mut reader).await.message,
        Message::HandshakeResult { .. }
    ));
    writer
        .send_control(WireRecord::Control(Envelope {
            request_id: 2,
            message: Message::MapHostPaths {
                request: MapHostPathsRequest {
                    paths: vec![r"C:\Code\slow".to_string()],
                    deadline_millis: 50,
                },
            },
        }))
        .await
        .unwrap();
    assert!(matches!(
        next_message(&mut reader).await,
        Envelope {
            request_id: 2,
            message: Message::Error { ref code, ref phase, .. },
        } if code == "deadlineExceeded" && phase == "pathMapping"
    ));

    let pid = std::fs::read_to_string(&pid_file)
        .unwrap()
        .trim()
        .parse::<u32>()
        .unwrap();
    for _ in 0..100 {
        if !std::path::Path::new(&format!("/proc/{pid}")).exists() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(!std::path::Path::new(&format!("/proc/{pid}")).exists());

    writer
        .send_control(WireRecord::Control(Envelope {
            request_id: 3,
            message: Message::Shutdown,
        }))
        .await
        .unwrap();
    drop(writer);
    writer_task.await.unwrap().unwrap();
    assert!(timeout(Duration::from_secs(2), child.wait())
        .await
        .unwrap()
        .unwrap()
        .success());
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn worker_streams_inspection_without_starving_control() {
    let binary = env!("CARGO_BIN_EXE_wsl-environment-worker");
    let build_id = file_sha256(std::path::Path::new(binary)).unwrap();
    let home = tempfile::tempdir().unwrap();
    let root = home.path().join("skills");
    for index in 0..128 {
        let skill = root.join(format!("skill-{index}"));
        std::fs::create_dir_all(&skill).unwrap();
        std::fs::write(skill.join("SKILL.md"), vec![b'x'; 32 * 1024]).unwrap();
    }
    let mut child = Command::new(binary)
        .env("WSL_DISTRO_NAME", "Ubuntu")
        .env("USER", "alice")
        .env("HOME", home.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let (writer, writer_task) = environment_protocol::spawn_writer(stdin);
    let mut reader = FramedRead::new(stdout, codec());

    writer
        .send_control(WireRecord::Control(Envelope {
            request_id: 1,
            message: Message::Handshake {
                build_id: build_id.clone(),
            },
        }))
        .await
        .unwrap();
    assert!(matches!(
        next_message(&mut reader).await.message,
        Message::HandshakeResult { .. }
    ));

    writer
        .send_control(WireRecord::Control(Envelope {
            request_id: 2,
            message: Message::InspectFilesystem {
                request: InspectionRequest {
                    roots: vec![InspectionRoot {
                        path: root.to_string_lossy().into_owned(),
                        stat_only: false,
                    }],
                    per_file_limit: 256 * 1024,
                    aggregate_limit: 8 * 1024 * 1024,
                    deadline_millis: 5_000,
                },
            },
        }))
        .await
        .unwrap();
    writer
        .send_control(WireRecord::Control(Envelope {
            request_id: 3,
            message: Message::ObservePath {
                path: home.path().to_string_lossy().into_owned(),
            },
        }))
        .await
        .unwrap();

    let mut payload = Vec::new();
    let mut record_index = 0usize;
    let mut observed_index = None;
    let mut completed_index = None;
    while observed_index.is_none() || completed_index.is_none() {
        let record = decode(&reader.next().await.unwrap().unwrap()).unwrap();
        match record {
            WireRecord::Control(Envelope {
                request_id: 3,
                message: Message::PathObserved { kind },
            }) => {
                assert_eq!(kind, PathKind::Directory);
                observed_index = Some(record_index);
            }
            WireRecord::Control(Envelope {
                request_id: 2,
                message: Message::BeginTransfer { .. },
            }) => {}
            WireRecord::PayloadChunk {
                transfer_id: 2,
                bytes,
            } => payload.extend_from_slice(&bytes),
            WireRecord::Control(Envelope {
                request_id: 2,
                message: Message::TransferCompleted { .. },
            }) => completed_index = Some(record_index),
            other => panic!("unexpected worker record: {other:?}"),
        }
        record_index += 1;
    }

    assert!(observed_index.unwrap() < completed_index.unwrap());
    let response = decode_inspection_response(&payload).unwrap();
    assert_eq!(response.total_content_bytes, 4 * 1024 * 1024);
    assert_eq!(response.facts.len(), 257);

    writer
        .send_control(WireRecord::Control(Envelope {
            request_id: 4,
            message: Message::Shutdown,
        }))
        .await
        .unwrap();
    drop(writer);
    writer_task.await.unwrap().unwrap();
    assert!(timeout(Duration::from_secs(2), child.wait())
        .await
        .unwrap()
        .unwrap()
        .success());
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn worker_completes_source_payload_and_host_upload_lifecycles_over_stdio() {
    use sha2::{Digest, Sha256};

    let binary = env!("CARGO_BIN_EXE_wsl-environment-worker");
    let build_id = file_sha256(std::path::Path::new(binary)).unwrap();
    let home = tempfile::tempdir().unwrap();
    let source = home.path().join("source");
    std::fs::create_dir(&source).unwrap();
    std::fs::write(source.join("SKILL.md"), b"demo").unwrap();
    let session_id = format!("stdio-{}", std::process::id());
    let uploaded_session_id = format!("stdio-upload-{}", std::process::id());
    let mut child = Command::new(binary)
        .env("WSL_DISTRO_NAME", "Ubuntu")
        .env("USER", "alice")
        .env("HOME", home.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let (writer, writer_task) = environment_protocol::spawn_writer(stdin);
    let mut reader = FramedRead::new(stdout, codec());

    writer
        .send_control(WireRecord::Control(Envelope {
            request_id: 1,
            message: Message::Handshake { build_id },
        }))
        .await
        .unwrap();
    assert!(matches!(
        next_message(&mut reader).await.message,
        Message::HandshakeResult { .. }
    ));
    writer
        .send_control(WireRecord::Control(Envelope {
            request_id: 2,
            message: Message::OpenLocalSource {
                request: OpenLocalSourceRequest {
                    path: source.to_string_lossy().into_owned(),
                },
            },
        }))
        .await
        .unwrap();
    let source_id = match next_message(&mut reader).await.message {
        Message::SourceOpened { source_id, .. } => source_id,
        message => panic!("unexpected source response: {message:?}"),
    };
    writer
        .send_control(WireRecord::Control(Envelope {
            request_id: 3,
            message: Message::AcquirePayloadFromSource {
                request: AcquirePayloadFromSourceRequest {
                    source_id,
                    relative_path: Vec::new(),
                    session_id: session_id.clone(),
                    payload_name: "payload-stdio".to_string(),
                    deadline_millis: 30_000,
                },
            },
        }))
        .await
        .unwrap();
    let acquired: PayloadReadyResponse =
        decode_payload(&next_transfer(&mut reader, 3).await).unwrap();
    let blob_id = acquired
        .manifest
        .entries
        .iter()
        .find_map(|entry| entry.blob_id.clone())
        .unwrap();
    writer
        .send_control(WireRecord::Control(Envelope {
            request_id: 4,
            message: Message::ReadPayloadBlob {
                payload_id: acquired.payload_id,
                blob_id: blob_id.clone(),
                deadline_millis: 30_000,
            },
        }))
        .await
        .unwrap();
    assert_eq!(next_transfer(&mut reader, 4).await, b"demo");

    writer
        .send_control(WireRecord::Control(Envelope {
            request_id: 5,
            message: Message::BeginPayloadUpload {
                session_id: uploaded_session_id.clone(),
                payload_name: "payload-stdio".to_string(),
            },
        }))
        .await
        .unwrap();
    let upload_id = match next_message(&mut reader).await.message {
        Message::PayloadUploadBegun { upload_id } => upload_id,
        message => panic!("unexpected upload response: {message:?}"),
    };
    writer
        .send_control(WireRecord::Control(Envelope {
            request_id: 6,
            message: Message::UploadPayloadBlob {
                upload_id,
                blob_id: blob_id.clone(),
                total_bytes: 4,
                sha256: format!("sha256:{blob_id}"),
            },
        }))
        .await
        .unwrap();
    let blob_transfer_id = match next_message(&mut reader).await.message {
        Message::TransferReady { transfer_id } => transfer_id,
        message => panic!("unexpected transfer response: {message:?}"),
    };
    writer
        .send_transfer(7, blob_transfer_id, b"demo")
        .await
        .unwrap();
    assert!(matches!(
        next_message(&mut reader).await.message,
        Message::PayloadBlobUploaded { .. }
    ));
    let manifest = serde_json::to_vec(&acquired.manifest).unwrap();
    let manifest_sha = format!("sha256:{:x}", Sha256::digest(&manifest));
    writer
        .send_control(WireRecord::Control(Envelope {
            request_id: 8,
            message: Message::FinalizePayloadUpload {
                upload_id,
                total_bytes: manifest.len() as u64,
                sha256: manifest_sha,
            },
        }))
        .await
        .unwrap();
    let manifest_transfer_id = match next_message(&mut reader).await.message {
        Message::TransferReady { transfer_id } => transfer_id,
        message => panic!("unexpected manifest response: {message:?}"),
    };
    writer
        .send_transfer(9, manifest_transfer_id, &manifest)
        .await
        .unwrap();
    assert!(matches!(
        next_message(&mut reader).await.message,
        Message::PayloadUploadFinalized { .. }
    ));
    writer
        .send_control(WireRecord::Control(Envelope {
            request_id: 10,
            message: Message::VerifyPayload {
                request: VerifyPayloadRequest {
                    session_id: uploaded_session_id.clone(),
                    payload_name: "payload-stdio".to_string(),
                    deadline_millis: 30_000,
                },
            },
        }))
        .await
        .unwrap();
    let verified: Option<PayloadReadyResponse> =
        decode_payload(&next_transfer(&mut reader, 10).await).unwrap();
    assert_eq!(verified.unwrap().manifest, acquired.manifest);

    for (request_id, cleanup_session) in [(11, session_id), (12, uploaded_session_id)] {
        writer
            .send_control(WireRecord::Control(Envelope {
                request_id,
                message: Message::RemovePayloadSession {
                    session_id: cleanup_session,
                },
            }))
            .await
            .unwrap();
        assert!(matches!(
            next_message(&mut reader).await.message,
            Message::PayloadSessionRemoved { .. }
        ));
    }
    writer
        .send_control(WireRecord::Control(Envelope {
            request_id: 13,
            message: Message::ReleaseSource { source_id },
        }))
        .await
        .unwrap();
    assert!(matches!(
        next_message(&mut reader).await.message,
        Message::SourceReleased { .. }
    ));
    writer
        .send_control(WireRecord::Control(Envelope {
            request_id: 14,
            message: Message::Shutdown,
        }))
        .await
        .unwrap();
    drop(writer);
    writer_task.await.unwrap().unwrap();
    assert!(timeout(Duration::from_secs(2), child.wait())
        .await
        .unwrap()
        .unwrap()
        .success());
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn worker_executes_one_accepted_mutation_and_acknowledges_cleanup_over_stdio() {
    use environment_engine::linux_mutation::{
        content_hash_path, fingerprint_path, parent_identity,
    };
    use sha2::{Digest, Sha256};

    let binary = env!("CARGO_BIN_EXE_wsl-environment-worker");
    let build_id = file_sha256(std::path::Path::new(binary)).unwrap();
    let home = tempfile::tempdir().unwrap();
    let source = home.path().join("source");
    std::fs::create_dir(&source).unwrap();
    std::fs::write(source.join("SKILL.md"), b"new").unwrap();
    let destination = home.path().join("targets/demo");
    std::fs::create_dir_all(&destination).unwrap();
    std::fs::write(destination.join("SKILL.md"), b"old").unwrap();
    let resource_id = format!(
        "{:x}",
        Sha256::digest(destination.as_os_str().as_encoded_bytes())
    );
    let backup = destination
        .parent()
        .unwrap()
        .join(format!(".skill-deck-backup-{resource_id}-000000"));
    let session_id = format!("stdio-mutation-{}", std::process::id());
    let mut child = Command::new(binary)
        .env("WSL_DISTRO_NAME", "Ubuntu")
        .env("USER", "alice")
        .env("HOME", home.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let (writer, writer_task) = environment_protocol::spawn_writer(stdin);
    let mut reader = FramedRead::new(stdout, codec());

    writer
        .send_control(WireRecord::Control(Envelope {
            request_id: 1,
            message: Message::Handshake { build_id },
        }))
        .await
        .unwrap();
    assert!(matches!(
        next_message(&mut reader).await.message,
        Message::HandshakeResult { .. }
    ));
    writer
        .send_control(WireRecord::Control(Envelope {
            request_id: 2,
            message: Message::OpenLocalSource {
                request: OpenLocalSourceRequest {
                    path: source.to_string_lossy().into_owned(),
                },
            },
        }))
        .await
        .unwrap();
    let source_id = match next_message(&mut reader).await.message {
        Message::SourceOpened { source_id, .. } => source_id,
        message => panic!("unexpected source response: {message:?}"),
    };
    writer
        .send_control(WireRecord::Control(Envelope {
            request_id: 3,
            message: Message::AcquirePayloadFromSource {
                request: AcquirePayloadFromSourceRequest {
                    source_id,
                    relative_path: Vec::new(),
                    session_id: session_id.clone(),
                    payload_name: "payload-mutation".to_string(),
                    deadline_millis: 30_000,
                },
            },
        }))
        .await
        .unwrap();
    let payload: PayloadReadyResponse =
        decode_payload(&next_transfer(&mut reader, 3).await).unwrap();
    let anchor = parent_identity(destination.parent().unwrap()).unwrap();
    let fingerprint = fingerprint_path(&destination).unwrap();
    let marker = serde_json::to_vec(&serde_json::json!({
        "schemaVersion": 2,
        "resourceId": resource_id,
        "kind": "inProgress",
        "environment": { "kind": "wsl", "distroName": "Ubuntu" },
        "operationId": "stdio-operation",
        "unitId": "stdio-unit",
        "subject": {
            "operationKind": "install",
            "skillName": "demo",
            "context": {
                "environment": { "kind": "wsl", "distroName": "Ubuntu" },
                "scope": { "kind": "global" }
            }
        },
        "createdAtEpochMs": 1,
        "entries": [{
            "physicalTargetDigest": "stdio-target",
            "destination": {
                "environment": { "kind": "wsl", "distroName": "Ubuntu" },
                "nativePath": destination
            },
            "backup": {
                "environment": { "kind": "wsl", "distroName": "Ubuntu" },
                "nativePath": backup
            },
            "expectedState": "present",
            "originalFingerprint": fingerprint,
            "phase": "staged"
        }]
    }))
    .unwrap();
    let request = MutationUnitRequest {
        resource_id: resource_id.clone(),
        operation_id: "stdio-operation".to_string(),
        unit_id: "stdio-unit".to_string(),
        initial_marker_json: marker,
        entries: vec![MutationEntry {
            destination: destination.to_string_lossy().into_owned(),
            expected_anchor_device: anchor.device,
            expected_anchor_inode: anchor.inode,
            expected_fingerprint: fingerprint,
            expected_content_hash: Some(content_hash_path(&destination).unwrap()),
            action: MutationEntryAction::Materialize {
                payload_id: payload.payload_id,
            },
        }],
        lock: None,
        deadline_millis: 30_000,
    };
    let request_bytes = environment_protocol::encode_payload(&request).unwrap();
    let request_sha = format!("sha256:{:x}", Sha256::digest(&request_bytes));
    writer
        .send_control(WireRecord::Control(Envelope {
            request_id: 4,
            message: Message::PrepareMutationUnit {
                resource_id: resource_id.clone(),
                total_bytes: request_bytes.len() as u64,
                sha256: request_sha,
            },
        }))
        .await
        .unwrap();
    let transfer_id = match next_message(&mut reader).await.message {
        Message::TransferReady { transfer_id } => transfer_id,
        message => panic!("unexpected mutation preparation response: {message:?}"),
    };
    writer
        .send_transfer(4, transfer_id, &request_bytes)
        .await
        .unwrap();
    assert_eq!(
        next_message(&mut reader).await,
        Envelope {
            request_id: 4,
            message: Message::MutationAccepted {
                resource_id: resource_id.clone(),
            },
        }
    );
    let outcome: MutationUnitOutcome =
        decode_payload(&next_transfer(&mut reader, 4).await).unwrap();
    let cleanup = match outcome {
        MutationUnitOutcome::Succeeded {
            cleanup: Some(cleanup),
            ..
        } => cleanup,
        outcome => panic!("unexpected mutation outcome: {outcome:?}"),
    };
    assert_eq!(std::fs::read(destination.join("SKILL.md")).unwrap(), b"new");
    assert!(backup.is_dir());

    writer
        .send_control(WireRecord::Control(Envelope {
            request_id: 5,
            message: Message::AcknowledgeMutationUnit {
                cleanup: cleanup.clone(),
            },
        }))
        .await
        .unwrap();
    assert_eq!(
        next_message(&mut reader).await,
        Envelope {
            request_id: 5,
            message: Message::MutationAcknowledged {
                resource_id: resource_id.clone(),
            },
        }
    );
    assert!(!backup.exists());
    assert!(!std::path::Path::new("/tmp")
        .join(format!("skill-deck-operation-{resource_id}"))
        .exists());

    writer
        .send_control(WireRecord::Control(Envelope {
            request_id: 6,
            message: Message::RemovePayloadSession { session_id },
        }))
        .await
        .unwrap();
    assert!(matches!(
        next_message(&mut reader).await.message,
        Message::PayloadSessionRemoved { .. }
    ));
    writer
        .send_control(WireRecord::Control(Envelope {
            request_id: 7,
            message: Message::ReleaseSource { source_id },
        }))
        .await
        .unwrap();
    assert!(matches!(
        next_message(&mut reader).await.message,
        Message::SourceReleased { .. }
    ));
    writer
        .send_control(WireRecord::Control(Envelope {
            request_id: 8,
            message: Message::Shutdown,
        }))
        .await
        .unwrap();
    drop(writer);
    writer_task.await.unwrap().unwrap();
    assert!(timeout(Duration::from_secs(2), child.wait())
        .await
        .unwrap()
        .unwrap()
        .success());
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn worker_writes_one_document_atomically_over_stdio() {
    use sha2::{Digest, Sha256};

    let binary = env!("CARGO_BIN_EXE_wsl-environment-worker");
    let build_id = file_sha256(std::path::Path::new(binary)).unwrap();
    let home = tempfile::tempdir().unwrap();
    let path = home.path().join(".skill-deck/projects.json");
    let bytes = br#"{"projects":[]}"#;
    let digest = format!("sha256:{:x}", Sha256::digest(bytes));
    let mut child = Command::new(binary)
        .env("WSL_DISTRO_NAME", "Ubuntu")
        .env("USER", "alice")
        .env("HOME", home.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let worker_pid = child.id().expect("worker process ID");
    let stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let (writer, writer_task) = environment_protocol::spawn_writer(stdin);
    let mut reader = FramedRead::new(stdout, codec());
    writer
        .send_control(WireRecord::Control(Envelope {
            request_id: 1,
            message: Message::Handshake { build_id },
        }))
        .await
        .unwrap();
    assert!(matches!(
        next_message(&mut reader).await.message,
        Message::HandshakeResult { .. }
    ));
    writer
        .send_control(WireRecord::Control(Envelope {
            request_id: 2,
            message: Message::PrepareDocumentWrite {
                request: DocumentWritePreparation {
                    path: path.to_string_lossy().into_owned(),
                    expected_revision: None,
                    total_bytes: bytes.len() as u64,
                    sha256: digest.clone(),
                    deadline_millis: 30_000,
                },
            },
        }))
        .await
        .unwrap();
    let transfer_id = match next_message(&mut reader).await.message {
        Message::TransferReady { transfer_id } => transfer_id,
        message => panic!("unexpected document preparation response: {message:?}"),
    };
    writer.send_transfer(2, transfer_id, bytes).await.unwrap();
    let revision = match next_message(&mut reader).await.message {
        Message::DocumentWritten { revision } => revision,
        message => panic!("unexpected document write response: {message:?}"),
    };
    assert_eq!(revision, digest);
    assert_eq!(std::fs::read(&path).unwrap(), bytes);

    std::fs::write(&path, b"external").unwrap();
    let replacement = br#"{"projects":["demo"]}"#;
    let replacement_digest = format!("sha256:{:x}", Sha256::digest(replacement));
    writer
        .send_control(WireRecord::Control(Envelope {
            request_id: 3,
            message: Message::PrepareDocumentWrite {
                request: DocumentWritePreparation {
                    path: path.to_string_lossy().into_owned(),
                    expected_revision: Some(revision),
                    total_bytes: replacement.len() as u64,
                    sha256: replacement_digest,
                    deadline_millis: 30_000,
                },
            },
        }))
        .await
        .unwrap();
    let transfer_id = match next_message(&mut reader).await.message {
        Message::TransferReady { transfer_id } => transfer_id,
        message => panic!("unexpected document preparation response: {message:?}"),
    };
    writer
        .send_transfer(3, transfer_id, replacement)
        .await
        .unwrap();
    match next_message(&mut reader).await.message {
        Message::Error { code, phase, .. } => {
            assert_eq!(code, "documentConflict");
            assert_eq!(phase, "documentWrite");
        }
        message => panic!("unexpected document conflict response: {message:?}"),
    }
    assert_eq!(std::fs::read(&path).unwrap(), b"external");
    assert_eq!(
        std::fs::read_dir(path.parent().unwrap()).unwrap().count(),
        1
    );
    writer
        .send_control(WireRecord::Control(Envelope {
            request_id: 4,
            message: Message::RemoveDocument {
                request: environment_protocol::DocumentRemoveRequest {
                    path: path.to_string_lossy().into_owned(),
                    expected_revision: Some(format!("sha256:{:x}", Sha256::digest(b"external"))),
                    deadline_millis: 30_000,
                },
            },
        }))
        .await
        .unwrap();
    assert!(matches!(
        next_message(&mut reader).await.message,
        Message::DocumentRemoved
    ));
    assert!(!path.exists());

    let abandoned_path = home.path().join(".skill-deck/abandoned.json");
    writer
        .send_control(WireRecord::Control(Envelope {
            request_id: 5,
            message: Message::PrepareDocumentWrite {
                request: DocumentWritePreparation {
                    path: abandoned_path.to_string_lossy().into_owned(),
                    expected_revision: None,
                    total_bytes: bytes.len() as u64,
                    sha256: digest,
                    deadline_millis: 30_000,
                },
            },
        }))
        .await
        .unwrap();
    assert!(matches!(
        next_message(&mut reader).await.message,
        Message::TransferReady { .. }
    ));

    writer
        .send_control(WireRecord::Control(Envelope {
            request_id: 6,
            message: Message::Shutdown,
        }))
        .await
        .unwrap();
    drop(writer);
    writer_task.await.unwrap().unwrap();
    assert!(timeout(Duration::from_secs(2), child.wait())
        .await
        .unwrap()
        .unwrap()
        .success());
    assert!(!abandoned_path.exists());
    for request_id in [2_u64, 3, 5] {
        assert!(!std::path::PathBuf::from(format!(
            "/tmp/.skill-deck-document-request-{worker_pid}-{request_id}"
        ))
        .exists());
    }
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn worker_executes_one_library_catalog_operation_over_stdio() {
    use sha2::{Digest, Sha256};

    let binary = env!("CARGO_BIN_EXE_wsl-environment-worker");
    let build_id = file_sha256(std::path::Path::new(binary)).unwrap();
    let home = tempfile::tempdir().unwrap();
    let catalog = br#"{"schemaVersion":1,"libraries":[]}"#.to_vec();
    let request = environment_protocol::LibraryOperationRequest {
        operation_id: "save-catalog-1".to_string(),
        expected_catalog_revision: None,
        catalog_bytes: catalog.clone(),
        action: environment_protocol::LibraryOperationAction::SaveCatalog {
            library_ids: Vec::new(),
        },
        deadline_millis: 30_000,
    };
    let payload = environment_protocol::encode_payload(&request).unwrap();
    let digest = format!("sha256:{:x}", Sha256::digest(&payload));
    let mut child = Command::new(binary)
        .env("WSL_DISTRO_NAME", "Ubuntu")
        .env("USER", "alice")
        .env("HOME", home.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let (writer, writer_task) = environment_protocol::spawn_writer(stdin);
    let mut reader = FramedRead::new(stdout, codec());
    writer
        .send_control(WireRecord::Control(Envelope {
            request_id: 1,
            message: Message::Handshake { build_id },
        }))
        .await
        .unwrap();
    assert!(matches!(
        next_message(&mut reader).await.message,
        Message::HandshakeResult { .. }
    ));
    writer
        .send_control(WireRecord::Control(Envelope {
            request_id: 2,
            message: Message::PrepareLibraryOperation {
                request: environment_protocol::LibraryOperationPreparation {
                    total_bytes: payload.len() as u64,
                    sha256: digest,
                    deadline_millis: 30_000,
                },
            },
        }))
        .await
        .unwrap();
    let transfer_id = match next_message(&mut reader).await.message {
        Message::TransferReady { transfer_id } => transfer_id,
        message => panic!("unexpected Library preparation response: {message:?}"),
    };
    writer
        .send_transfer(2, transfer_id, &payload)
        .await
        .unwrap();
    let revision = match next_message(&mut reader).await.message {
        Message::LibraryOperationCompleted { catalog_revision } => catalog_revision,
        message => panic!("unexpected Library operation response: {message:?}"),
    };
    writer
        .send_control(WireRecord::Control(Envelope {
            request_id: 3,
            message: Message::ReadLibraryCatalog {
                deadline_millis: 30_000,
            },
        }))
        .await
        .unwrap();
    let response: environment_protocol::LibraryCatalogResponse =
        environment_protocol::decode_payload(&next_transfer(&mut reader, 3).await).unwrap();
    assert!(response.present);
    assert_eq!(response.bytes, catalog);
    assert_eq!(response.revision, Some(revision));

    writer
        .send_control(WireRecord::Control(Envelope {
            request_id: 4,
            message: Message::Shutdown,
        }))
        .await
        .unwrap();
    drop(writer);
    writer_task.await.unwrap().unwrap();
    assert!(timeout(Duration::from_secs(2), child.wait())
        .await
        .unwrap()
        .unwrap()
        .success());
}

#[cfg(target_os = "linux")]
async fn next_transfer<R>(
    reader: &mut FramedRead<R, tokio_util::codec::LengthDelimitedCodec>,
    owner_request_id: u64,
) -> Vec<u8>
where
    R: tokio::io::AsyncRead + Unpin,
{
    let mut payload = Vec::new();
    loop {
        match decode(&reader.next().await.unwrap().unwrap()).unwrap() {
            WireRecord::Control(Envelope {
                request_id,
                message: Message::BeginTransfer { .. },
            }) => assert_eq!(request_id, owner_request_id),
            WireRecord::PayloadChunk { bytes, .. } => payload.extend(bytes),
            WireRecord::Control(Envelope {
                request_id,
                message: Message::TransferCompleted { .. },
            }) => {
                assert_eq!(request_id, owner_request_id);
                return payload;
            }
            record => panic!("unexpected transfer record: {record:?}"),
        }
    }
}

async fn next_message<R>(
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

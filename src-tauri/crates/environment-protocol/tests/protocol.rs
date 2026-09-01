use std::time::Duration;

use bytes::{Bytes, BytesMut};
use environment_protocol::{
    codec, decode, decode_inspection_response, encode, encode_inspection_response, spawn_writer,
    AcquirePayloadFromSourceRequest, DecodeError, DocumentWritePreparation, Envelope,
    InspectionEntryKind, InspectionErrorCode, InspectionFact, InspectionResponse,
    MapHostPathsRequest, MapHostPathsResponse, Message, OpenLocalSourceRequest, PathKind,
    SourceScanMode, SourceScanRequest, SourceScanRoot, WireRecord, MAX_FRAME_BYTES,
    MAX_PAYLOAD_CHUNK_BYTES,
};
use futures_util::StreamExt;
use sha2::Digest;
use tokio_util::codec::{Decoder, Encoder, FramedRead};

#[test]
fn strict_postcard_wire_format_round_trips_known_literals() {
    let shutdown = WireRecord::Control(Envelope {
        request_id: 7,
        message: Message::Shutdown,
    });
    assert_eq!(encode(&shutdown).unwrap(), [0, 7, 7]);
    assert_eq!(decode(&[0, 7, 7]).unwrap(), shutdown);

    let chunk = WireRecord::PayloadChunk {
        transfer_id: 42,
        bytes: vec![0, 255, 1],
    };
    assert_eq!(encode(&chunk).unwrap(), [1, 42, 3, 0, 255, 1]);
    assert_eq!(decode(&[1, 42, 3, 0, 255, 1]).unwrap(), chunk);
}

#[test]
fn strict_decoder_rejects_truncated_trailing_and_oversized_frames() {
    let encoded = encode(&WireRecord::Control(Envelope {
        request_id: 1,
        message: Message::Shutdown,
    }))
    .unwrap();

    assert!(matches!(
        decode(&encoded[..encoded.len() - 1]),
        Err(DecodeError::Postcard(_))
    ));

    let mut trailing = encoded;
    trailing.push(0);
    assert_eq!(
        decode(&trailing),
        Err(DecodeError::TrailingBytes { count: 1 })
    );

    let mut framed = BytesMut::new();
    framed.extend_from_slice(&((MAX_FRAME_BYTES + 1) as u32).to_be_bytes());
    framed.resize(MAX_FRAME_BYTES + 5, 0);
    assert!(codec().decode(&mut framed).is_err());
}

#[test]
fn path_observation_messages_round_trip_without_changing_existing_literals() {
    let request = WireRecord::Control(Envelope {
        request_id: 8,
        message: Message::ObservePath {
            path: "/home/alice".to_string(),
        },
    });
    assert_eq!(decode(&encode(&request).unwrap()).unwrap(), request);

    let response = WireRecord::Control(Envelope {
        request_id: 8,
        message: Message::PathObserved {
            kind: PathKind::Directory,
        },
    });
    assert_eq!(decode(&encode(&response).unwrap()).unwrap(), response);

    assert_eq!(
        encode(&WireRecord::Control(Envelope {
            request_id: 7,
            message: Message::Shutdown,
        }))
        .unwrap(),
        [0, 7, 7]
    );
}

#[test]
fn host_path_mapping_messages_round_trip_in_input_order() {
    let request = WireRecord::Control(Envelope {
        request_id: 9,
        message: Message::MapHostPaths {
            request: MapHostPathsRequest {
                paths: vec![
                    r"C:\Code\Skill Deck".to_string(),
                    r"\\server\share\项目".to_string(),
                ],
                deadline_millis: 10_000,
            },
        },
    });
    assert_eq!(decode(&encode(&request).unwrap()).unwrap(), request);

    let response = MapHostPathsResponse {
        mapped: vec![
            "/mnt/c/Code/Skill Deck".to_string(),
            "/mnt/server/share/项目".to_string(),
        ],
    };
    let encoded = environment_protocol::encode_payload(&response).unwrap();
    assert_eq!(
        environment_protocol::decode_payload::<MapHostPathsResponse>(&encoded).unwrap(),
        response
    );
}

#[test]
fn inspection_payload_round_trips_raw_posix_path_bytes() {
    let response = InspectionResponse {
        facts: vec![InspectionFact {
            root_index: 2,
            relative_path: vec![b's', b'k', 0x80],
            kind: InspectionEntryKind::Symlink,
            resolved_target: Some(vec![b'.', b'.', b'/', 0xff]),
            content_bytes: vec![0, 255, 1],
            truncated: true,
            error_code: Some(InspectionErrorCode::ReadLinkFailed),
        }],
        total_content_bytes: 3,
    };

    let encoded = encode_inspection_response(&response).unwrap();
    assert_eq!(decode_inspection_response(&encoded).unwrap(), response);

    let mut trailing = encoded;
    trailing.push(0);
    assert!(matches!(
        decode_inspection_response(&trailing),
        Err(DecodeError::TrailingBytes { count: 1 })
    ));
}

#[test]
fn source_requests_keep_paths_relative_to_the_owning_handle() {
    let open = WireRecord::Control(Envelope {
        request_id: 51,
        message: Message::OpenLocalSource {
            request: OpenLocalSourceRequest {
                path: "/home/alice/skills".to_string(),
            },
        },
    });
    assert_eq!(decode(&encode(&open).unwrap()).unwrap(), open);

    let scan = WireRecord::Control(Envelope {
        request_id: 52,
        message: Message::ScanSource {
            request: SourceScanRequest {
                source_id: 9,
                roots: vec![SourceScanRoot {
                    relative_path: vec![b's', b'k', 0x80],
                    stat_only: false,
                }],
                mode: SourceScanMode::Recursive,
                per_file_limit: 1024,
                aggregate_limit: 4096,
                deadline_millis: 30_000,
            },
        },
    });
    assert_eq!(decode(&encode(&scan).unwrap()).unwrap(), scan);
}

#[test]
fn payload_requests_bind_storage_to_a_source_handle_and_managed_key() {
    let request = WireRecord::Control(Envelope {
        request_id: 61,
        message: Message::AcquirePayloadFromSource {
            request: AcquirePayloadFromSourceRequest {
                source_id: 9,
                relative_path: b"skills/demo".to_vec(),
                session_id: "session-1".to_string(),
                payload_name: "payload-demo".to_string(),
                deadline_millis: 60_000,
            },
        },
    });
    assert_eq!(decode(&encode(&request).unwrap()).unwrap(), request);

    let upload = WireRecord::Control(Envelope {
        request_id: 62,
        message: Message::UploadPayloadBlob {
            upload_id: 4,
            blob_id: "a".repeat(64),
            total_bytes: 128,
            sha256: format!("sha256:{}", "a".repeat(64)),
        },
    });
    assert_eq!(decode(&encode(&upload).unwrap()).unwrap(), upload);
}

#[test]
fn document_write_messages_round_trip_revision_and_transfer_evidence() {
    let request = WireRecord::Control(Envelope {
        request_id: 63,
        message: Message::PrepareDocumentWrite {
            request: DocumentWritePreparation {
                path: "/home/alice/.skill-deck/projects.json".to_string(),
                expected_revision: Some(format!("sha256:{}", "a".repeat(64))),
                total_bytes: 128,
                sha256: format!("sha256:{}", "b".repeat(64)),
                deadline_millis: 30_000,
            },
        },
    });
    assert_eq!(decode(&encode(&request).unwrap()).unwrap(), request);

    let response = WireRecord::Control(Envelope {
        request_id: 63,
        message: Message::DocumentWritten {
            revision: format!("sha256:{}", "b".repeat(64)),
        },
    });
    assert_eq!(decode(&encode(&response).unwrap()).unwrap(), response);

    let remove = WireRecord::Control(Envelope {
        request_id: 64,
        message: Message::RemoveDocument {
            request: environment_protocol::DocumentRemoveRequest {
                path: "/home/alice/.skill-deck/application.json".to_string(),
                expected_revision: Some(format!("sha256:{}", "b".repeat(64))),
                deadline_millis: 30_000,
            },
        },
    });
    assert_eq!(decode(&encode(&remove).unwrap()).unwrap(), remove);
}

#[test]
fn mutation_request_round_trips_one_unit_with_payload_handle_and_lock_evidence() {
    let request = environment_protocol::MutationUnitRequest {
        resource_id: "a".repeat(64),
        operation_id: "operation-1".to_string(),
        unit_id: "unit-1".to_string(),
        initial_marker_json: br#"{"kind":"inProgress"}"#.to_vec(),
        entries: vec![environment_protocol::MutationEntry {
            destination: "/home/alice/.agents/skills/demo".to_string(),
            expected_anchor_device: 1,
            expected_anchor_inode: 2,
            expected_fingerprint: "entry-v1-missing".to_string(),
            expected_content_hash: None,
            action: environment_protocol::MutationEntryAction::Materialize { payload_id: 9 },
        }],
        lock: Some(environment_protocol::MutationLock {
            target: "/home/alice/.agents/skills-lock.json".to_string(),
            legacy_target: None,
            schema: environment_protocol::MutationLockSchema::Global,
            entry: environment_protocol::MutationLockEntry::Remove {
                key: "demo".to_string(),
            },
            root_replacements_json: Default::default(),
            expected_entries_json: std::collections::BTreeMap::from([(
                "demo".to_string(),
                Some(br#"{"source":"old"}"#.to_vec()),
            )]),
            expected_roots_json: Default::default(),
        }),
        deadline_millis: 60_000,
    };

    let encoded = environment_protocol::encode_payload(&request).unwrap();
    assert_eq!(
        environment_protocol::decode_payload::<environment_protocol::MutationUnitRequest>(&encoded)
            .unwrap(),
        request
    );
}

#[test]
fn library_operation_round_trips_opaque_catalog_and_payload_handle() {
    let request = environment_protocol::LibraryOperationRequest {
        operation_id: "library-operation-1".to_string(),
        expected_catalog_revision: Some(format!("sha256:{}", "a".repeat(64))),
        catalog_bytes: br#"{"schemaVersion":1}"#.to_vec(),
        action: environment_protocol::LibraryOperationAction::CommitMember {
            library_id: "library-1".to_string(),
            skill_name: "demo".to_string(),
            expected_anchor_device: 11,
            expected_anchor_inode: 22,
            expected_fingerprint: "entry-v1-missing".to_string(),
            expected_content_hash: None,
            mutation: environment_protocol::LibraryMemberAction::Upsert { payload_id: 9 },
        },
        deadline_millis: 60_000,
    };
    let encoded = environment_protocol::encode_payload(&request).unwrap();
    assert_eq!(
        environment_protocol::decode_payload::<environment_protocol::LibraryOperationRequest>(
            &encoded,
        )
        .unwrap(),
        request
    );

    let read = WireRecord::Control(Envelope {
        request_id: 70,
        message: Message::ReadLibraryCatalog {
            deadline_millis: 30_000,
        },
    });
    assert_eq!(decode(&encode(&read).unwrap()).unwrap(), read);
}

#[tokio::test]
async fn one_writer_preserves_binary_order_and_acks_written_barriers() {
    let (writer_side, reader_side) = tokio::io::duplex(128);
    let (writer, writer_task) = spawn_writer(writer_side);
    let mut reader = FramedRead::new(reader_side, codec());

    writer
        .send_binary(WireRecord::PayloadChunk {
            transfer_id: 9,
            bytes: vec![1; 1024],
        })
        .await
        .unwrap();
    writer
        .send_control(WireRecord::Control(Envelope {
            request_id: 3,
            message: Message::Progress {
                current: 1,
                total: 2,
            },
        }))
        .await
        .unwrap();

    let barrier_writer = writer.clone();
    let barrier = tokio::spawn(async move {
        barrier_writer
            .send_binary_barrier(WireRecord::Control(Envelope {
                request_id: 9,
                message: Message::TransferCompleted {
                    transfer_id: 9,
                    total_bytes: 1024,
                    sha256: "digest".to_string(),
                },
            }))
            .await
    });

    tokio::time::sleep(Duration::from_millis(20)).await;
    assert!(!barrier.is_finished());

    let mut records = Vec::new();
    while records.len() < 3 {
        let frame = reader.next().await.unwrap().unwrap();
        records.push(decode(&frame).unwrap());
    }
    barrier.await.unwrap().unwrap();

    let chunk_index = records
        .iter()
        .position(|record| matches!(record, WireRecord::PayloadChunk { transfer_id: 9, .. }))
        .unwrap();
    let completion_index = records
        .iter()
        .position(|record| {
            matches!(
                record,
                WireRecord::Control(Envelope {
                    message: Message::TransferCompleted { transfer_id: 9, .. },
                    ..
                })
            )
        })
        .unwrap();
    assert!(chunk_index < completion_index);
    drop(writer);
    writer_task.await.unwrap().unwrap();
}

#[tokio::test]
async fn one_transfer_is_chunked_and_bound_to_its_owner_request() {
    let (writer_side, reader_side) = tokio::io::duplex(MAX_PAYLOAD_CHUNK_BYTES * 3);
    let (writer, writer_task) = spawn_writer(writer_side);
    let mut reader = FramedRead::new(reader_side, codec());
    let payload = vec![0x5a; MAX_PAYLOAD_CHUNK_BYTES * 2 + 7];

    writer.send_transfer(41, 73, &payload).await.unwrap();
    drop(writer);

    let mut records = Vec::new();
    while let Some(frame) = reader.next().await {
        records.push(decode(&frame.unwrap()).unwrap());
    }
    writer_task.await.unwrap().unwrap();

    assert!(matches!(
        &records[0],
        WireRecord::Control(Envelope {
            request_id: 41,
            message: Message::BeginTransfer {
                transfer_id: 73,
                owner_request_id: 41,
                total_bytes,
                ..
            },
        }) if *total_bytes == payload.len() as u64
    ));
    let chunks = records[1..records.len() - 1]
        .iter()
        .map(|record| match record {
            WireRecord::PayloadChunk {
                transfer_id: 73,
                bytes,
            } => bytes.as_slice(),
            other => panic!("unexpected transfer record: {other:?}"),
        })
        .collect::<Vec<_>>();
    assert_eq!(chunks.len(), 3);
    assert!(chunks
        .iter()
        .all(|chunk| chunk.len() <= MAX_PAYLOAD_CHUNK_BYTES));
    assert_eq!(chunks.concat(), payload);
    assert!(matches!(
        records.last().unwrap(),
        WireRecord::Control(Envelope {
            request_id: 41,
            message: Message::TransferCompleted {
                transfer_id: 73,
                total_bytes,
                ..
            },
        }) if *total_bytes == payload.len() as u64
    ));
}

#[tokio::test]
async fn transfer_writer_enforces_the_callers_boundary() {
    let (writer_side, _reader_side) = tokio::io::duplex(128);
    let (writer, writer_task) = spawn_writer(writer_side);

    let error = writer
        .send_transfer_with_limit(1, 1, &[0; 16], 8)
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        environment_protocol::WriterError::TransferTooLarge
    ));
    drop(writer);
    writer_task.await.unwrap().unwrap();
}

#[tokio::test]
async fn reader_transfer_streams_a_declared_blob_without_changing_the_wire_shape() {
    let (writer_side, reader_side) = tokio::io::duplex(4096);
    let (writer, writer_task) = spawn_writer(writer_side);
    let mut reader = FramedRead::new(reader_side, codec());
    let content = b"streamed-blob";
    let digest = format!("sha256:{:x}", sha2::Sha256::digest(content));

    writer
        .send_reader_transfer_with_limit(
            81,
            82,
            std::io::Cursor::new(content),
            content.len() as u64,
            digest,
            1024,
        )
        .await
        .unwrap();
    drop(writer);

    let mut records = Vec::new();
    while let Some(frame) = reader.next().await {
        records.push(decode(&frame.unwrap()).unwrap());
    }
    writer_task.await.unwrap().unwrap();
    assert!(matches!(
        records.as_slice(),
        [
            WireRecord::Control(Envelope {
                message: Message::BeginTransfer { transfer_id: 82, .. },
                ..
            }),
            WireRecord::PayloadChunk { transfer_id: 82, bytes },
            WireRecord::Control(Envelope {
                message: Message::TransferCompleted { transfer_id: 82, .. },
                ..
            })
        ] if bytes == content
    ));
}

#[tokio::test]
async fn continuous_control_messages_do_not_starve_binary_records() {
    let (writer_side, reader_side) = tokio::io::duplex(4);
    let (writer, writer_task) = spawn_writer(writer_side);
    let mut reader = FramedRead::new(reader_side, codec());

    writer
        .send_control(WireRecord::Control(Envelope {
            request_id: 1,
            message: Message::Progress {
                current: 1,
                total: 10,
            },
        }))
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(20)).await;
    for request_id in 2..=10 {
        writer
            .send_control(WireRecord::Control(Envelope {
                request_id,
                message: Message::Progress {
                    current: request_id as u32,
                    total: 10,
                },
            }))
            .await
            .unwrap();
    }
    writer
        .send_binary(WireRecord::PayloadChunk {
            transfer_id: 99,
            bytes: vec![9],
        })
        .await
        .unwrap();

    let first = decode(&reader.next().await.unwrap().unwrap()).unwrap();
    let second = decode(&reader.next().await.unwrap().unwrap()).unwrap();
    assert!(matches!(first, WireRecord::Control(_)));
    assert!(matches!(
        second,
        WireRecord::PayloadChunk {
            transfer_id: 99,
            ..
        }
    ));

    drop(writer);
    while reader.next().await.is_some() {}
    writer_task.await.unwrap().unwrap();
}

#[test]
fn frame_encoder_accepts_the_largest_valid_record() {
    let mut codec = codec();
    let mut output = BytesMut::new();
    codec
        .encode(Bytes::from(vec![0; MAX_FRAME_BYTES]), &mut output)
        .unwrap();
    assert_eq!(output.len(), MAX_FRAME_BYTES + 4);
}

use std::io::Write;

#[cfg(target_os = "linux")]
use environment_protocol::{InspectionEntryKind, InspectionRequest, InspectionRoot};
use environment_protocol::{Message, PathKind};
#[cfg(target_os = "linux")]
use wsl_environment_worker::execute_inspection;
use wsl_environment_worker::{file_sha256, Dispatch, WorkerIdentity, WorkerRuntime};

fn identity(home: &str) -> WorkerIdentity {
    WorkerIdentity {
        distro: "Ubuntu".to_string(),
        user: "alice".to_string(),
        uid: 1000,
        home: home.to_string(),
    }
}

#[test]
fn handshake_accepts_only_the_running_binary_build() {
    let temp = tempfile::tempdir().unwrap();
    let runtime = WorkerRuntime::new(
        "sha256:worker-v1".to_string(),
        identity(temp.path().to_str().unwrap()),
    );

    assert_eq!(
        runtime.dispatch(Message::Handshake {
            build_id: "sha256:worker-v1".to_string(),
        }),
        Dispatch {
            response: Some(Message::HandshakeResult {
                build_id: "sha256:worker-v1".to_string(),
                distro: "Ubuntu".to_string(),
                user: "alice".to_string(),
                uid: 1000,
                home: temp.path().to_string_lossy().into_owned(),
            }),
            close: false,
        }
    );

    assert_eq!(
        runtime.dispatch(Message::Handshake {
            build_id: "sha256:other".to_string(),
        }),
        Dispatch {
            response: Some(Message::Error {
                code: "buildMismatch".to_string(),
                phase: "handshake".to_string(),
                parameters: Vec::new(),
            }),
            close: true,
        }
    );
}

#[test]
fn observe_path_reports_directory_file_missing_and_symlink() {
    let temp = tempfile::tempdir().unwrap();
    let file = temp.path().join("file");
    std::fs::write(&file, b"content").unwrap();
    let runtime = WorkerRuntime::new("build".to_string(), identity(temp.path().to_str().unwrap()));

    for (path, expected) in [
        (temp.path().to_path_buf(), PathKind::Directory),
        (file, PathKind::File),
        (temp.path().join("missing"), PathKind::Missing),
    ] {
        assert_eq!(
            runtime.dispatch(Message::ObservePath {
                path: path.to_string_lossy().into_owned(),
            }),
            Dispatch {
                response: Some(Message::PathObserved { kind: expected }),
                close: false,
            }
        );
    }

    #[cfg(unix)]
    {
        let link = temp.path().join("link");
        std::os::unix::fs::symlink(temp.path(), &link).unwrap();
        assert_eq!(
            runtime.dispatch(Message::ObservePath {
                path: link.to_string_lossy().into_owned(),
            }),
            Dispatch {
                response: Some(Message::PathObserved {
                    kind: PathKind::SymlinkDirectory,
                }),
                close: false,
            }
        );
    }
}

#[test]
fn self_hash_is_the_sha256_of_the_exact_binary_bytes() {
    let mut file = tempfile::NamedTempFile::new().unwrap();
    file.write_all(b"worker-bytes").unwrap();
    assert_eq!(
        file_sha256(file.path()).unwrap(),
        "sha256:11b87fcc63c88aff5a8568038519a02d7db3551e1432836b8d60bbf6eb6a7b38"
    );
}

#[cfg(target_os = "linux")]
#[test]
fn inspection_adapter_executes_the_shared_engine_and_returns_raw_path_bytes() {
    use std::os::unix::ffi::OsStringExt;

    let temp = tempfile::tempdir().unwrap();
    let raw_name = std::ffi::OsString::from_vec(vec![b's', b'k', 0x80]);
    std::fs::write(temp.path().join(&raw_name), b"payload").unwrap();

    let response = execute_inspection(
        InspectionRequest {
            roots: vec![InspectionRoot {
                path: temp.path().to_string_lossy().into_owned(),
                stat_only: false,
            }],
            per_file_limit: 16,
            aggregate_limit: 16,
            deadline_millis: 1_000,
        },
        || false,
    )
    .unwrap();

    assert!(response.facts.iter().any(|fact| {
        fact.relative_path == vec![b's', b'k', 0x80] && fact.kind == InspectionEntryKind::File
    }));
}

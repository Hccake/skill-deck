#![cfg(target_os = "linux")]

use environment_engine::linux_mutation::fingerprint_path;
use environment_engine::projection::{project_targets, ProjectionRequest};
use environment_protocol::LibraryMemberAction;
use environment_protocol::{LibraryOperationAction, LibraryOperationRequest};
use wsl_environment_worker::library::LibraryManager;
use wsl_environment_worker::payload::PayloadManager;

#[test]
fn manager_saves_and_reads_the_catalog_under_its_home() {
    let home = tempfile::tempdir().unwrap();
    let manager = LibraryManager::new(home.path().to_path_buf());
    let payloads = PayloadManager::new(home.path().join("payloads")).unwrap();
    let bytes = br#"{"schemaVersion":1,"libraries":[]}"#.to_vec();

    let revision = manager
        .execute(
            LibraryOperationRequest {
                operation_id: "save-1".to_string(),
                expected_catalog_revision: None,
                catalog_bytes: bytes.clone(),
                action: LibraryOperationAction::SaveCatalog {
                    library_ids: Vec::new(),
                },
                deadline_millis: 30_000,
            },
            &payloads,
        )
        .unwrap();
    let catalog = manager.read_catalog().unwrap();

    assert_eq!(catalog.bytes, bytes);
    assert!(catalog.present);
    assert_eq!(catalog.revision, Some(revision));
    assert!(home
        .path()
        .join(".skill-deck/skill-libraries/catalog.json")
        .is_file());
}

#[test]
fn manager_commits_a_member_from_an_existing_payload_handle() {
    let home = tempfile::tempdir().unwrap();
    let source = home.path().join("source");
    std::fs::create_dir(&source).unwrap();
    std::fs::write(source.join("SKILL.md"), b"member").unwrap();
    let manager = LibraryManager::new(home.path().to_path_buf());
    let mut payloads = PayloadManager::new(home.path().join("payloads")).unwrap();
    let payload = payloads
        .acquire_from_source("library", "payload-demo", &source)
        .unwrap();
    let first = br#"{"schemaVersion":1,"libraries":[{"id":"lib-1","skills":[]}]}"#.to_vec();
    let catalog_revision = manager
        .execute(
            LibraryOperationRequest {
                operation_id: "save-1".to_string(),
                expected_catalog_revision: None,
                catalog_bytes: first,
                action: LibraryOperationAction::SaveCatalog {
                    library_ids: vec!["lib-1".to_string()],
                },
                deadline_millis: 30_000,
            },
            &payloads,
        )
        .unwrap();
    let destination = home
        .path()
        .join(".skill-deck/skill-libraries/libraries/lib-1/skills/demo");
    let target = project_targets(&ProjectionRequest {
        destinations: vec![destination.clone()],
    })
    .unwrap()
    .targets
    .pop()
    .unwrap();
    let second = br#"{"schemaVersion":1,"libraries":[{"id":"lib-1","skills":["demo"]}]}"#.to_vec();

    manager
        .execute(
            LibraryOperationRequest {
                operation_id: "member-1".to_string(),
                expected_catalog_revision: Some(catalog_revision),
                catalog_bytes: second.clone(),
                action: LibraryOperationAction::CommitMember {
                    library_id: "lib-1".to_string(),
                    skill_name: "demo".to_string(),
                    expected_anchor_device: target.anchor_device,
                    expected_anchor_inode: target.anchor_inode,
                    expected_fingerprint: fingerprint_path(&destination).unwrap(),
                    expected_content_hash: None,
                    mutation: LibraryMemberAction::Upsert {
                        payload_id: payload.id,
                    },
                },
                deadline_millis: 30_000,
            },
            &payloads,
        )
        .unwrap();

    assert_eq!(
        std::fs::read(destination.join("SKILL.md")).unwrap(),
        b"member"
    );
    assert_eq!(manager.read_catalog().unwrap().bytes, second);
}

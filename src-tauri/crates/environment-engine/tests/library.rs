#![cfg(target_os = "linux")]

use std::path::Path;

use environment_engine::library::{
    commit, read_catalog, write_catalog, CatalogWrite, ContentAction, LibraryCommit, LibraryError,
    TargetExpectation,
};
use environment_engine::linux_mutation::fingerprint_path;
use environment_engine::projection::{project_targets, ProjectionRequest};

#[test]
fn member_upsert_commits_content_and_catalog_as_one_intent() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("skill-libraries");
    let destination = root.join("libraries/lib-1/skills/demo");
    std::fs::create_dir_all(destination.parent().unwrap()).unwrap();
    let payload = payload_fixture(temp.path(), b"new content");
    let catalog = br#"{"schemaVersion":1,"libraries":[{"id":"lib-1"}]}"#.to_vec();

    commit(LibraryCommit {
        root: root.clone(),
        operation_id: "operation-1".to_string(),
        destination: destination.clone(),
        expected_target: target_expectation(&destination),
        content: ContentAction::Upsert {
            payload_root: payload,
        },
        catalog: CatalogWrite {
            expected_revision: None,
            bytes: catalog.clone(),
        },
    })
    .unwrap();

    assert_eq!(
        std::fs::read(destination.join("SKILL.md")).unwrap(),
        b"new content"
    );
    assert_eq!(read_catalog(&root).unwrap().bytes, Some(catalog));
    assert!(!root.join(".transactions/operation-1").exists());
}

#[test]
fn member_commit_accepts_a_managed_root_reached_through_a_symlinked_parent() {
    let temp = tempfile::tempdir().unwrap();
    let physical_home = temp.path().join("physical-home");
    std::fs::create_dir(&physical_home).unwrap();
    let alias_home = temp.path().join("home");
    std::os::unix::fs::symlink(&physical_home, &alias_home).unwrap();
    let root = alias_home.join(".skill-deck/skill-libraries");
    let destination = root.join("libraries/lib-1/skills/demo");
    std::fs::create_dir_all(destination.parent().unwrap()).unwrap();
    let payload = payload_fixture(temp.path(), b"linked home");

    commit(LibraryCommit {
        root: root.clone(),
        operation_id: "linked-home".to_string(),
        destination: destination.clone(),
        expected_target: target_expectation(&destination),
        content: ContentAction::Upsert {
            payload_root: payload,
        },
        catalog: CatalogWrite {
            expected_revision: None,
            bytes: br#"{"schemaVersion":1}"#.to_vec(),
        },
    })
    .unwrap();

    assert_eq!(
        std::fs::read(destination.join("SKILL.md")).unwrap(),
        b"linked home"
    );
}

#[test]
fn catalog_write_is_conditional_and_creates_declared_library_roots() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("skill-libraries");
    let first = br#"{"schemaVersion":1,"libraries":[]}"#.to_vec();
    let revision = write_catalog(
        &root,
        &[],
        CatalogWrite {
            expected_revision: None,
            bytes: first,
        },
    )
    .unwrap();
    let second = br#"{"schemaVersion":1,"libraries":[{"id":"lib-1"}]}"#.to_vec();

    write_catalog(
        &root,
        &["lib-1".to_string()],
        CatalogWrite {
            expected_revision: Some(revision),
            bytes: second.clone(),
        },
    )
    .unwrap();
    assert!(root.join("libraries/lib-1/skills").is_dir());
    assert_eq!(read_catalog(&root).unwrap().bytes, Some(second.clone()));

    assert!(matches!(
        write_catalog(
            &root,
            &[],
            CatalogWrite {
                expected_revision: Some("sha256:wrong".to_string()),
                bytes: b"stale".to_vec(),
            },
        ),
        Err(LibraryError::StaleTarget)
    ));
    assert_eq!(read_catalog(&root).unwrap().bytes, Some(second));
}

#[test]
fn delete_commit_removes_the_destination_and_updates_catalog() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("skill-libraries");
    let destination = root.join("libraries/lib-1/skills/demo");
    std::fs::create_dir_all(&destination).unwrap();
    std::fs::write(destination.join("SKILL.md"), b"old").unwrap();
    let first = br#"{"schemaVersion":1,"libraries":[{"id":"lib-1","skills":["demo"]}]}"#.to_vec();
    let revision = write_catalog(
        &root,
        &["lib-1".to_string()],
        CatalogWrite {
            expected_revision: None,
            bytes: first,
        },
    )
    .unwrap();
    let second = br#"{"schemaVersion":1,"libraries":[{"id":"lib-1","skills":[]}]}"#.to_vec();

    commit(LibraryCommit {
        root: root.clone(),
        operation_id: "delete-1".to_string(),
        destination: destination.clone(),
        expected_target: target_expectation(&destination),
        content: ContentAction::Delete,
        catalog: CatalogWrite {
            expected_revision: Some(revision),
            bytes: second.clone(),
        },
    })
    .unwrap();

    assert!(!destination.exists());
    assert_eq!(read_catalog(&root).unwrap().bytes, Some(second));
    assert!(!root.join(".transactions/delete-1").exists());
}

#[test]
fn catalog_read_recovers_an_activated_legacy_wsl_transaction() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("skill-libraries");
    let destination = root.join("libraries/lib-1/skills/demo");
    let transaction = root.join(".transactions/interrupted");
    std::fs::create_dir_all(&destination).unwrap();
    std::fs::write(destination.join("SKILL.md"), b"new").unwrap();
    std::fs::create_dir_all(transaction.join("backup")).unwrap();
    std::fs::write(transaction.join("backup/SKILL.md"), b"old").unwrap();
    std::fs::write(
        transaction.join("destination"),
        destination.as_os_str().as_encoded_bytes(),
    )
    .unwrap();
    std::fs::write(transaction.join("desired-presence"), b"1").unwrap();
    std::fs::write(transaction.join("phase"), b"activated").unwrap();
    let catalog = br#"{"schemaVersion":1,"libraries":[]}"#.to_vec();
    std::fs::write(root.join("catalog.json"), &catalog).unwrap();

    assert_eq!(read_catalog(&root).unwrap().bytes, Some(catalog));
    assert_eq!(std::fs::read(destination.join("SKILL.md")).unwrap(), b"old");
    assert!(!transaction.exists());
}

#[test]
fn catalog_read_keeps_legacy_prepared_content_when_catalog_hash_matches() {
    use sha2::{Digest, Sha256};

    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("skill-libraries");
    let destination = root.join("libraries/lib-1/skills/demo");
    let transaction = root.join(".transactions/committed");
    std::fs::create_dir_all(&destination).unwrap();
    std::fs::write(destination.join("SKILL.md"), b"new").unwrap();
    std::fs::create_dir_all(transaction.join("backup")).unwrap();
    std::fs::write(transaction.join("backup/SKILL.md"), b"old").unwrap();
    std::fs::write(
        transaction.join("destination"),
        destination.as_os_str().as_encoded_bytes(),
    )
    .unwrap();
    std::fs::write(transaction.join("desired-presence"), b"1").unwrap();
    std::fs::write(transaction.join("phase"), b"catalogPrepared").unwrap();
    let catalog = br#"{"schemaVersion":1,"libraries":[{"id":"lib-1"}]}"#.to_vec();
    std::fs::write(
        transaction.join("expected-catalog-hash"),
        format!("{:x}", Sha256::digest(&catalog)),
    )
    .unwrap();
    std::fs::write(root.join("catalog.json"), &catalog).unwrap();

    read_catalog(&root).unwrap();

    assert_eq!(std::fs::read(destination.join("SKILL.md")).unwrap(), b"new");
    assert!(!transaction.exists());
}

fn target_expectation(destination: &Path) -> TargetExpectation {
    let target = project_targets(&ProjectionRequest {
        destinations: vec![destination.to_path_buf()],
    })
    .unwrap()
    .targets
    .pop()
    .unwrap();
    TargetExpectation {
        parent: environment_engine::linux_mutation::ParentIdentity {
            device: target.anchor_device,
            inode: target.anchor_inode,
        },
        fingerprint: fingerprint_path(destination).unwrap(),
        content_hash: None,
    }
}

fn payload_fixture(parent: &Path, content: &[u8]) -> std::path::PathBuf {
    let source = parent.join("source");
    let payload = parent.join("payload");
    std::fs::create_dir(&source).unwrap();
    std::fs::write(source.join("SKILL.md"), content).unwrap();
    environment_engine::payload::build_payload(&source, &payload).unwrap();
    payload
}

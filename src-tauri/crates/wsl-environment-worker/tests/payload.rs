#![cfg(target_os = "linux")]

use std::io::Read;

use wsl_environment_worker::payload::PayloadManager;

#[test]
fn payload_manager_builds_verifies_reads_and_removes_worker_owned_payloads() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("source");
    std::fs::create_dir_all(source.join("scripts")).unwrap();
    std::fs::write(source.join("SKILL.md"), b"demo").unwrap();
    std::fs::write(source.join("scripts/run.sh"), b"#!/bin/sh\n").unwrap();
    let mut manager = PayloadManager::new(temp.path().to_path_buf()).unwrap();

    let acquired = manager
        .acquire_from_source("session-1", "payload-demo", &source)
        .unwrap();
    assert!(acquired.root.join("manifest.json").is_file());
    let verified = manager
        .verify("session-1", "payload-demo")
        .unwrap()
        .unwrap();
    assert_ne!(verified.id, acquired.id);
    assert_eq!(verified.manifest, acquired.manifest);

    let blob_id = acquired
        .manifest
        .entries
        .iter()
        .find_map(|entry| entry.blob_id.as_deref())
        .unwrap();
    let mut blob = manager.read_blob(acquired.id, blob_id).unwrap().unwrap();
    let mut content = Vec::new();
    blob.read_to_end(&mut content).unwrap();
    assert!(content == b"demo" || content == b"#!/bin/sh\n");

    let root = acquired.root;
    manager.remove("session-1", "payload-demo").unwrap();
    assert!(!root.exists());
    assert!(manager.read_blob(acquired.id, blob_id).is_err());
}

#[test]
fn payload_manager_only_sweeps_valid_unprotected_owned_sessions() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("source");
    std::fs::create_dir(&source).unwrap();
    std::fs::write(source.join("SKILL.md"), b"demo").unwrap();
    let mut manager = PayloadManager::new(temp.path().to_path_buf()).unwrap();
    manager
        .acquire_from_source("protected", "payload-demo", &source)
        .unwrap();
    manager
        .acquire_from_source("orphan", "payload-demo", &source)
        .unwrap();
    std::fs::create_dir(temp.path().join("skill-deck-source-foreign")).unwrap();

    let report = manager.sweep_orphans(&["protected".to_string()]).unwrap();

    assert_eq!(report.removed_sessions, 1);
    assert_eq!(report.protected_sessions, 1);
    assert!(report.cleanup_blocked);
    assert!(temp.path().join("skill-deck-source-protected").is_dir());
    assert!(!temp.path().join("skill-deck-source-orphan").exists());
    assert!(temp.path().join("skill-deck-source-foreign").is_dir());
}

#[test]
fn payload_upload_is_staged_and_published_only_after_exact_manifest_validation() {
    use sha2::{Digest, Sha256};

    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("source");
    std::fs::create_dir(&source).unwrap();
    std::fs::write(source.join("SKILL.md"), b"demo").unwrap();
    let mut manager = PayloadManager::new(temp.path().to_path_buf()).unwrap();
    let source_payload = manager
        .acquire_from_source("source", "payload-demo", &source)
        .unwrap();
    let manifest = source_payload.manifest.clone();
    manager.remove_session("source").unwrap();

    let upload_id = manager.begin_upload("uploaded", "payload-demo").unwrap();
    let blob_id = format!("{:x}", Sha256::digest(b"demo"));
    let prepared = manager.prepare_blob(upload_id, &blob_id).unwrap();
    std::fs::write(&prepared.path, b"demo").unwrap();
    manager
        .commit_blob(upload_id, &blob_id, prepared.path)
        .unwrap();
    let manifest_file = manager.prepare_manifest(upload_id).unwrap();
    std::fs::write(&manifest_file.path, serde_json::to_vec(&manifest).unwrap()).unwrap();
    let uploaded = manager
        .finalize_upload(upload_id, manifest_file.path)
        .unwrap();

    assert_eq!(uploaded.manifest, manifest);
    assert!(uploaded.root.join("manifest.json").is_file());

    let abandoned = manager.begin_upload("abandoned", "payload-demo").unwrap();
    manager.remove_session("abandoned").unwrap();
    assert!(manager.prepare_manifest(abandoned).is_err());
}

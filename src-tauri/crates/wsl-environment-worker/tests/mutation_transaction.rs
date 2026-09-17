#![cfg(target_os = "linux")]

use environment_engine::linux_mutation::{
    content_hash_path, fingerprint_path, parent_identity, MutationError,
};
use environment_protocol::{
    MutationEntry, MutationEntryAction, MutationLock, MutationLockEntry, MutationLockSchema,
    MutationUnitOutcome, MutationUnitRequest,
};
use wsl_environment_worker::mutation::MutationManager;
use wsl_environment_worker::mutation::WorkerMutationError;
use wsl_environment_worker::payload::PayloadManager;

#[test]
fn accepted_transaction_commits_directory_and_lock_before_exact_ack_cleanup() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("source");
    std::fs::create_dir(&source).unwrap();
    std::fs::write(source.join("SKILL.md"), b"new").unwrap();
    let mut payloads = PayloadManager::new(temp.path().to_path_buf()).unwrap();
    let payload = payloads
        .acquire_from_source("mutation", "payload-demo", &source)
        .unwrap();
    let destination = temp.path().join("targets/demo");
    std::fs::create_dir_all(&destination).unwrap();
    std::fs::write(destination.join("SKILL.md"), b"old").unwrap();
    let lock_path = temp.path().join("skills-lock.json");
    std::fs::write(
        &lock_path,
        br#"{"version":3,"skills":{"demo":{"source":"old"}}}"#,
    )
    .unwrap();
    let resource_id = "b".repeat(64);
    let backup = destination
        .parent()
        .unwrap()
        .join(format!(".skill-deck-backup-{resource_id}-000000"));
    let marker = serde_json::to_vec_pretty(&serde_json::json!({
        "schemaVersion": 2,
        "resourceId": resource_id,
        "kind": "inProgress",
        "environment": { "kind": "wsl", "distroName": "Ubuntu" },
        "operationId": "operation-1",
        "unitId": "unit-1",
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
            "physicalTargetDigest": "target-1",
            "destination": {
                "environment": { "kind": "wsl", "distroName": "Ubuntu" },
                "nativePath": destination
            },
            "backup": {
                "environment": { "kind": "wsl", "distroName": "Ubuntu" },
                "nativePath": backup
            },
            "expectedState": "present",
            "originalFingerprint": fingerprint_path(&destination).unwrap(),
            "phase": "staged"
        }]
    }))
    .unwrap();
    let request = MutationUnitRequest {
        resource_id: resource_id.clone(),
        operation_id: "operation-1".to_string(),
        unit_id: "unit-1".to_string(),
        initial_marker_json: marker,
        entries: vec![MutationEntry {
            destination: destination.to_string_lossy().into_owned(),
            expected_anchor_device: parent_identity(destination.parent().unwrap())
                .unwrap()
                .device,
            expected_anchor_inode: parent_identity(destination.parent().unwrap())
                .unwrap()
                .inode,
            expected_fingerprint: fingerprint_path(&destination).unwrap(),
            expected_content_hash: None,
            action: MutationEntryAction::Materialize {
                payload_id: payload.id,
            },
        }],
        lock: Some(MutationLock {
            target: lock_path.to_string_lossy().into_owned(),
            legacy_target: None,
            schema: MutationLockSchema::Global,
            entry: MutationLockEntry::Replace {
                key: "demo".to_string(),
                replacement_json: br#"{"source":"new"}"#.to_vec(),
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
    let manager = MutationManager::new(temp.path().to_path_buf()).unwrap();

    let accepted = manager.accept(request, &payloads, || false).unwrap();
    assert_eq!(std::fs::read(destination.join("SKILL.md")).unwrap(), b"old");
    assert!(temp
        .path()
        .join(format!("skill-deck-operation-{resource_id}/recovery.json"))
        .is_file());

    let outcome = manager.execute(accepted, || false).unwrap();
    let cleanup = match outcome {
        MutationUnitOutcome::Succeeded {
            cleanup: Some(cleanup),
            ..
        } => cleanup,
        outcome => panic!("unexpected transaction outcome: {outcome:?}"),
    };
    assert_eq!(std::fs::read(destination.join("SKILL.md")).unwrap(), b"new");
    let lock: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&lock_path).unwrap()).unwrap();
    assert_eq!(lock["skills"]["demo"]["source"], "new");
    assert!(backup.is_dir());

    manager.acknowledge(&cleanup).unwrap();
    assert!(!backup.exists());
    assert!(!temp
        .path()
        .join(format!("skill-deck-operation-{resource_id}"))
        .exists());
}

#[test]
fn lock_conflict_restores_the_directory_and_keeps_typed_conflict_evidence() {
    let temp = tempfile::tempdir().unwrap();
    let payloads = PayloadManager::new(temp.path().to_path_buf()).unwrap();
    let destination = temp.path().join("targets/demo");
    std::fs::create_dir_all(&destination).unwrap();
    std::fs::write(destination.join("SKILL.md"), b"old").unwrap();
    let lock_path = temp.path().join("skills-lock.json");
    std::fs::write(
        &lock_path,
        br#"{"version":3,"skills":{"demo":{"source":"changed"}}}"#,
    )
    .unwrap();
    let resource_id = "c".repeat(64);
    let backup = destination
        .parent()
        .unwrap()
        .join(format!(".skill-deck-backup-{resource_id}-000000"));
    let fingerprint = fingerprint_path(&destination).unwrap();
    let marker = serde_json::to_vec_pretty(&serde_json::json!({
        "schemaVersion": 2,
        "resourceId": resource_id,
        "kind": "inProgress",
        "environment": { "kind": "wsl", "distroName": "Ubuntu" },
        "operationId": "operation-2",
        "unitId": "unit-2",
        "subject": {
            "operationKind": "remove",
            "skillName": "demo",
            "context": {
                "environment": { "kind": "wsl", "distroName": "Ubuntu" },
                "scope": { "kind": "global" }
            }
        },
        "createdAtEpochMs": 1,
        "entries": [{
            "physicalTargetDigest": "target-1",
            "destination": {
                "environment": { "kind": "wsl", "distroName": "Ubuntu" },
                "nativePath": destination
            },
            "backup": {
                "environment": { "kind": "wsl", "distroName": "Ubuntu" },
                "nativePath": backup
            },
            "expectedState": "missing",
            "originalFingerprint": fingerprint,
            "phase": "staged"
        }]
    }))
    .unwrap();
    let anchor = parent_identity(destination.parent().unwrap()).unwrap();
    let request = MutationUnitRequest {
        resource_id: resource_id.clone(),
        operation_id: "operation-2".to_string(),
        unit_id: "unit-2".to_string(),
        initial_marker_json: marker,
        entries: vec![MutationEntry {
            destination: destination.to_string_lossy().into_owned(),
            expected_anchor_device: anchor.device,
            expected_anchor_inode: anchor.inode,
            expected_fingerprint: fingerprint,
            expected_content_hash: None,
            action: MutationEntryAction::Remove,
        }],
        lock: Some(MutationLock {
            target: lock_path.to_string_lossy().into_owned(),
            legacy_target: None,
            schema: MutationLockSchema::Global,
            entry: MutationLockEntry::Remove {
                key: "demo".to_string(),
            },
            root_replacements_json: Default::default(),
            expected_entries_json: std::collections::BTreeMap::from([(
                "demo".to_string(),
                Some(br#"{"source":"expected"}"#.to_vec()),
            )]),
            expected_roots_json: Default::default(),
        }),
        deadline_millis: 60_000,
    };
    let manager = MutationManager::new(temp.path().to_path_buf()).unwrap();

    let accepted = manager.accept(request, &payloads, || false).unwrap();
    let outcome = manager.execute(accepted, || false).unwrap();

    assert!(matches!(
        outcome,
        MutationUnitOutcome::Failed {
            ref code,
            ref parameters,
            ..
        } if code == "lockConflictSkill"
            && parameters == &vec![("skillName".to_string(), "demo".to_string())]
    ));
    assert_eq!(std::fs::read(destination.join("SKILL.md")).unwrap(), b"old");
    assert!(!backup.exists());
    assert!(!temp
        .path()
        .join(format!("skill-deck-operation-{resource_id}"))
        .exists());
}

#[test]
fn authoritative_accept_preserves_stale_target_classification() {
    let temp = tempfile::tempdir().unwrap();
    let payloads = PayloadManager::new(temp.path().to_path_buf()).unwrap();
    let destination = temp.path().join("targets/demo");
    std::fs::create_dir_all(&destination).unwrap();
    let anchor = parent_identity(destination.parent().unwrap()).unwrap();
    let expected = fingerprint_path(&destination).unwrap();
    let expected_content = content_hash_path(&destination).unwrap();
    std::fs::write(destination.join("changed"), b"changed").unwrap();
    let request = MutationUnitRequest {
        resource_id: "d".repeat(64),
        operation_id: "operation-3".to_string(),
        unit_id: "unit-3".to_string(),
        initial_marker_json: Vec::new(),
        entries: vec![MutationEntry {
            destination: destination.to_string_lossy().into_owned(),
            expected_anchor_device: anchor.device,
            expected_anchor_inode: anchor.inode,
            expected_fingerprint: expected,
            expected_content_hash: Some(expected_content),
            action: MutationEntryAction::Keep,
        }],
        lock: None,
        deadline_millis: 60_000,
    };
    let manager = MutationManager::new(temp.path().to_path_buf()).unwrap();

    assert!(matches!(
        manager.accept(request, &payloads, || false),
        Err(WorkerMutationError::Engine(MutationError::StaleTarget))
    ));
}

#[test]
fn lock_only_transaction_is_accepted_and_keeps_recovery_evidence_until_ack() {
    let temp = tempfile::tempdir().unwrap();
    let payloads = PayloadManager::new(temp.path().to_path_buf()).unwrap();
    let destination = temp.path().join("targets/demo");
    std::fs::create_dir_all(&destination).unwrap();
    std::fs::write(destination.join("SKILL.md"), b"same").unwrap();
    let lock_path = temp.path().join("skills-lock.json");
    std::fs::write(
        &lock_path,
        br#"{"version":3,"skills":{"demo":{"source":"old"}}}"#,
    )
    .unwrap();
    let resource_id = "e".repeat(64);
    let fingerprint = fingerprint_path(&destination).unwrap();
    let marker = serde_json::to_vec(&serde_json::json!({
        "schemaVersion": 2,
        "resourceId": resource_id,
        "kind": "inProgress",
        "environment": { "kind": "wsl", "distroName": "Ubuntu" },
        "operationId": "operation-4",
        "unitId": "unit-4",
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
            "physicalTargetDigest": "target-1",
            "destination": {
                "environment": { "kind": "wsl", "distroName": "Ubuntu" },
                "nativePath": destination
            },
            "backup": null,
            "expectedState": "present",
            "originalFingerprint": fingerprint,
            "phase": "staged"
        }]
    }))
    .unwrap();
    let anchor = parent_identity(destination.parent().unwrap()).unwrap();
    let request = MutationUnitRequest {
        resource_id: resource_id.clone(),
        operation_id: "operation-4".to_string(),
        unit_id: "unit-4".to_string(),
        initial_marker_json: marker,
        entries: vec![MutationEntry {
            destination: destination.to_string_lossy().into_owned(),
            expected_anchor_device: anchor.device,
            expected_anchor_inode: anchor.inode,
            expected_fingerprint: fingerprint,
            expected_content_hash: Some(content_hash_path(&destination).unwrap()),
            action: MutationEntryAction::Keep,
        }],
        lock: Some(MutationLock {
            target: lock_path.to_string_lossy().into_owned(),
            legacy_target: None,
            schema: MutationLockSchema::Global,
            entry: MutationLockEntry::Replace {
                key: "demo".to_string(),
                replacement_json: br#"{"source":"new"}"#.to_vec(),
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
    let manager = MutationManager::new(temp.path().to_path_buf()).unwrap();

    let accepted = manager.accept(request, &payloads, || false).unwrap();
    assert!(MutationManager::requires_acceptance(&accepted));
    let outcome = manager.execute(accepted, || false).unwrap();
    let cleanup = match outcome {
        MutationUnitOutcome::Succeeded {
            cleanup: Some(cleanup),
            ..
        } => cleanup,
        outcome => panic!("unexpected transaction outcome: {outcome:?}"),
    };
    let lock: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&lock_path).unwrap()).unwrap();
    assert_eq!(lock["skills"]["demo"]["source"], "new");
    assert!(temp
        .path()
        .join(format!("skill-deck-operation-{resource_id}/recovery.json"))
        .is_file());

    manager.acknowledge(&cleanup).unwrap();
    assert!(!temp
        .path()
        .join(format!("skill-deck-operation-{resource_id}"))
        .exists());
}

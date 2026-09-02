#![cfg(target_os = "linux")]

use wsl_environment_worker::mutation::{MutationRecoveryError, MutationRecoveryStore};

#[test]
fn recovery_store_lists_old_markers_and_cleans_only_an_exact_marker() {
    let temp = tempfile::tempdir().unwrap();
    let store = MutationRecoveryStore::new(temp.path().to_path_buf()).unwrap();
    let resource_id = "a".repeat(64);
    let root = temp
        .path()
        .join(format!("skill-deck-operation-{resource_id}"));
    let backup = temp
        .path()
        .join(format!(".skill-deck-backup-{resource_id}-000000"));
    let stage = temp
        .path()
        .join(format!(".skill-deck-stage-{resource_id}-000000"));
    std::fs::create_dir(&root).unwrap();
    std::fs::write(
        root.join(".skill-deck-owner"),
        format!("1\n{resource_id}\n"),
    )
    .unwrap();
    let destination = temp.path().join("demo");
    let marker = serde_json::to_vec(&serde_json::json!({
        "schemaVersion": 2,
        "kind": "cleanupOnly",
        "entries": [{
            "destination": { "nativePath": destination },
            "backup": { "nativePath": backup }
        }]
    }))
    .unwrap();
    std::fs::write(root.join("recovery.json"), &marker).unwrap();
    std::fs::create_dir(&backup).unwrap();
    std::fs::create_dir(&stage).unwrap();

    let listed = store.list().unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].resource_id, resource_id);
    assert_eq!(listed[0].marker_bytes.as_deref(), Some(marker.as_slice()));

    assert_eq!(
        store
            .cleanup(&resource_id, b"different", std::slice::from_ref(&backup))
            .unwrap_err(),
        MutationRecoveryError::StaleMarker
    );
    assert!(root.is_dir());
    assert!(backup.is_dir());
    assert!(stage.is_dir());

    store
        .cleanup(&resource_id, &marker, std::slice::from_ref(&backup))
        .unwrap();
    assert!(!root.exists());
    assert!(!backup.exists());
    assert!(!stage.exists());
}

#[test]
fn recovery_store_rejects_a_backup_outside_its_target_parent() {
    let temp = tempfile::tempdir().unwrap();
    let store = MutationRecoveryStore::new(temp.path().to_path_buf()).unwrap();
    let resource_id = "b".repeat(64);
    let root = temp
        .path()
        .join(format!("skill-deck-operation-{resource_id}"));
    let destination = temp.path().join("targets/demo");
    let backup = temp
        .path()
        .join(format!("elsewhere/.skill-deck-backup-{resource_id}-000000"));
    std::fs::create_dir(&root).unwrap();
    std::fs::write(
        root.join(".skill-deck-owner"),
        format!("1\n{resource_id}\n"),
    )
    .unwrap();
    let marker = serde_json::to_vec(&serde_json::json!({
        "entries": [{
            "destination": { "nativePath": destination },
            "backup": { "nativePath": backup }
        }]
    }))
    .unwrap();
    std::fs::write(root.join("recovery.json"), &marker).unwrap();

    assert_eq!(
        store
            .cleanup(&resource_id, &marker, std::slice::from_ref(&backup))
            .unwrap_err(),
        MutationRecoveryError::UnsafeRoot
    );
    assert!(root.is_dir());
}

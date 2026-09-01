#![cfg(target_os = "linux")]

use environment_engine::linux_mutation::{
    content_hash_path, fingerprint_path, parent_identity, EntryAction, EntryIntent, MutationError,
    StagedMutation,
};
use environment_engine::payload::build_payload;

#[test]
fn replace_rechecks_swaps_verifies_and_restores_one_linux_entry_set() {
    let temp = tempfile::tempdir().unwrap();
    let destination = temp.path().join("targets/demo");
    std::fs::create_dir_all(&destination).unwrap();
    std::fs::write(destination.join("SKILL.md"), b"old").unwrap();
    let source = temp.path().join("source");
    std::fs::create_dir(&source).unwrap();
    std::fs::write(source.join("SKILL.md"), b"new").unwrap();
    let payload = temp.path().join("payload");
    build_payload(&source, &payload).unwrap();
    let intent = EntryIntent {
        destination: destination.clone(),
        expected_parent: parent_identity(destination.parent().unwrap()).unwrap(),
        expected_fingerprint: fingerprint_path(&destination).unwrap(),
        expected_content_hash: None,
        action: EntryAction::Materialize {
            payload_root: payload,
        },
    };

    let mut staged = StagedMutation::stage("operation-1", vec![intent], || false).unwrap();
    staged.recheck(|| false).unwrap();
    staged.swap(|| false).unwrap();
    staged.verify(|| false).unwrap();
    assert_eq!(std::fs::read(destination.join("SKILL.md")).unwrap(), b"new");
    staged.restore().unwrap();
    assert_eq!(std::fs::read(destination.join("SKILL.md")).unwrap(), b"old");
    staged.cleanup().unwrap();
}

#[test]
fn materialize_keeps_an_unselected_agent_with_a_missing_root_absent() {
    let temp = tempfile::tempdir().unwrap();
    let scope = temp.path().join("scope");
    std::fs::create_dir(&scope).unwrap();
    let canonical = scope.join(".agents/skills/demo");
    let unselected = scope.join(".opencode/skills/demo");
    let source = temp.path().join("source");
    std::fs::create_dir(&source).unwrap();
    std::fs::write(source.join("SKILL.md"), b"new").unwrap();
    let payload = temp.path().join("payload");
    build_payload(&source, &payload).unwrap();
    let anchor = parent_identity(&scope).unwrap();
    let intents = vec![
        EntryIntent {
            destination: canonical.clone(),
            expected_parent: anchor,
            expected_fingerprint: fingerprint_path(&canonical).unwrap(),
            expected_content_hash: None,
            action: EntryAction::Materialize {
                payload_root: payload,
            },
        },
        EntryIntent {
            destination: unselected.clone(),
            expected_parent: anchor,
            expected_fingerprint: fingerprint_path(&unselected).unwrap(),
            expected_content_hash: None,
            action: EntryAction::Keep,
        },
    ];

    let mut staged = StagedMutation::stage("operation-keep", intents, || false).unwrap();

    assert!(!unselected.parent().unwrap().exists());
    staged.swap(|| false).unwrap();
    staged.verify(|| false).unwrap();
    assert_eq!(std::fs::read(canonical.join("SKILL.md")).unwrap(), b"new");
    assert!(!unselected.parent().unwrap().exists());
    staged.restore().unwrap();
    staged.cleanup().unwrap();
}

#[test]
fn materialize_creates_a_selected_agent_root_and_installs_its_symlink() {
    let temp = tempfile::tempdir().unwrap();
    let scope = temp.path().join("scope");
    std::fs::create_dir(&scope).unwrap();
    let canonical = scope.join(".agents/skills/demo");
    let selected = scope.join(".claude/skills/demo");
    let source = temp.path().join("source");
    std::fs::create_dir(&source).unwrap();
    std::fs::write(source.join("SKILL.md"), b"new").unwrap();
    let payload = temp.path().join("payload");
    build_payload(&source, &payload).unwrap();
    let anchor = parent_identity(&scope).unwrap();
    let intents = vec![
        EntryIntent {
            destination: canonical.clone(),
            expected_parent: anchor,
            expected_fingerprint: fingerprint_path(&canonical).unwrap(),
            expected_content_hash: None,
            action: EntryAction::Materialize {
                payload_root: payload,
            },
        },
        EntryIntent {
            destination: selected.clone(),
            expected_parent: anchor,
            expected_fingerprint: fingerprint_path(&selected).unwrap(),
            expected_content_hash: None,
            action: EntryAction::Symlink {
                target: std::path::PathBuf::from("../../.agents/skills/demo"),
            },
        },
    ];

    let mut staged = StagedMutation::stage("operation-symlink", intents, || false).unwrap();

    assert!(selected.parent().unwrap().is_dir());
    staged.swap(|| false).unwrap();
    staged.verify(|| false).unwrap();
    assert_eq!(
        std::fs::read_link(&selected).unwrap(),
        std::path::Path::new("../../.agents/skills/demo")
    );
    staged.restore().unwrap();
    staged.cleanup().unwrap();
}

#[test]
fn keep_rejects_a_missing_target_created_after_stage() {
    let temp = tempfile::tempdir().unwrap();
    let scope = temp.path().join("scope");
    std::fs::create_dir(&scope).unwrap();
    let destination = scope.join(".opencode/skills/demo");
    let intent = EntryIntent {
        destination: destination.clone(),
        expected_parent: parent_identity(&scope).unwrap(),
        expected_fingerprint: fingerprint_path(&destination).unwrap(),
        expected_content_hash: None,
        action: EntryAction::Keep,
    };
    let mut staged = StagedMutation::stage("operation-stale-keep", vec![intent], || false).unwrap();
    std::fs::create_dir_all(&destination).unwrap();

    assert_eq!(
        staged.swap(|| false).unwrap_err(),
        MutationError::StaleTarget
    );
    staged.cleanup().unwrap();
}

#[test]
fn a_later_stage_failure_cleans_every_prepared_entry() {
    let temp = tempfile::tempdir().unwrap();
    let parent = temp.path().join("targets");
    std::fs::create_dir(&parent).unwrap();
    let first = parent.join("first");
    let second = parent.join("second");
    let source = temp.path().join("source");
    std::fs::create_dir(&source).unwrap();
    std::fs::write(source.join("SKILL.md"), b"new").unwrap();
    let payload = temp.path().join("payload");
    build_payload(&source, &payload).unwrap();
    let anchor = parent_identity(&parent).unwrap();
    let intents = vec![
        EntryIntent {
            destination: first,
            expected_parent: anchor,
            expected_fingerprint: "entry-v1-missing".to_string(),
            expected_content_hash: None,
            action: EntryAction::Materialize {
                payload_root: payload,
            },
        },
        EntryIntent {
            destination: second,
            expected_parent: anchor,
            expected_fingerprint: "entry-v1-missing".to_string(),
            expected_content_hash: None,
            action: EntryAction::Materialize {
                payload_root: temp.path().join("missing-payload"),
            },
        },
    ];

    assert_eq!(
        StagedMutation::stage("operation-cleanup", intents, || false).unwrap_err(),
        MutationError::InvalidPayload
    );
    assert!(!parent
        .join(".skill-deck-stage-operation-cleanup-000000")
        .exists());
    assert!(!parent
        .join(".skill-deck-stage-operation-cleanup-000001")
        .exists());
}

#[test]
fn staging_never_cleans_a_preexisting_operation_path() {
    let temp = tempfile::tempdir().unwrap();
    let parent = temp.path().join("targets");
    std::fs::create_dir(&parent).unwrap();
    let destination = parent.join("demo");
    let existing_stage = parent.join(".skill-deck-stage-operation-existing-000000");
    std::fs::create_dir(&existing_stage).unwrap();
    std::fs::write(existing_stage.join("evidence"), b"keep").unwrap();
    let intent = EntryIntent {
        destination,
        expected_parent: parent_identity(&parent).unwrap(),
        expected_fingerprint: "entry-v1-missing".to_string(),
        expected_content_hash: None,
        action: EntryAction::Remove,
    };

    assert_eq!(
        StagedMutation::stage("operation-existing", vec![intent], || false).unwrap_err(),
        MutationError::StaleTarget
    );
    assert_eq!(
        std::fs::read(existing_stage.join("evidence")).unwrap(),
        b"keep"
    );
}

#[test]
fn stage_only_cleanup_preserves_backup_evidence() {
    let temp = tempfile::tempdir().unwrap();
    let parent = temp.path().join("targets");
    std::fs::create_dir(&parent).unwrap();
    let destination = parent.join("demo");
    let source = temp.path().join("source");
    std::fs::create_dir(&source).unwrap();
    std::fs::write(source.join("SKILL.md"), b"new").unwrap();
    let payload = temp.path().join("payload");
    build_payload(&source, &payload).unwrap();
    let intent = EntryIntent {
        destination,
        expected_parent: parent_identity(&parent).unwrap(),
        expected_fingerprint: "entry-v1-missing".to_string(),
        expected_content_hash: None,
        action: EntryAction::Materialize {
            payload_root: payload,
        },
    };
    let mut staged = StagedMutation::stage("operation-stage-only", vec![intent], || false).unwrap();
    let stage = parent.join(".skill-deck-stage-operation-stage-only-000000");
    let backup = parent.join(".skill-deck-backup-operation-stage-only-000000");
    std::fs::create_dir(&backup).unwrap();
    std::fs::write(backup.join("evidence"), b"old").unwrap();

    assert!(staged.cleanup_stages().is_empty());

    assert!(!stage.exists());
    assert_eq!(std::fs::read(backup.join("evidence")).unwrap(), b"old");
}

#[test]
fn remove_keeps_a_missing_target_parent_absent() {
    let temp = tempfile::tempdir().unwrap();
    let scope = temp.path().join("scope");
    std::fs::create_dir(&scope).unwrap();
    let destination = scope.join(".opencode/skills/demo");
    let intent = EntryIntent {
        destination: destination.clone(),
        expected_parent: parent_identity(&scope).unwrap(),
        expected_fingerprint: fingerprint_path(&destination).unwrap(),
        expected_content_hash: None,
        action: EntryAction::Remove,
    };

    let mut staged = StagedMutation::stage("operation-remove", vec![intent], || false).unwrap();

    assert!(!destination.parent().unwrap().exists());
    staged.swap(|| false).unwrap();
    staged.verify(|| false).unwrap();
    assert!(!destination.parent().unwrap().exists());
    staged.restore().unwrap();
    staged.cleanup().unwrap();
}

#[test]
fn replace_rejects_a_target_changed_after_stage() {
    let temp = tempfile::tempdir().unwrap();
    let destination = temp.path().join("targets/demo");
    std::fs::create_dir_all(&destination).unwrap();
    std::fs::write(destination.join("SKILL.md"), b"old").unwrap();
    let source = temp.path().join("source");
    std::fs::create_dir(&source).unwrap();
    std::fs::write(source.join("SKILL.md"), b"new").unwrap();
    let payload = temp.path().join("payload");
    build_payload(&source, &payload).unwrap();
    let intent = EntryIntent {
        destination: destination.clone(),
        expected_parent: parent_identity(destination.parent().unwrap()).unwrap(),
        expected_fingerprint: fingerprint_path(&destination).unwrap(),
        expected_content_hash: Some(content_hash_path(&destination).unwrap()),
        action: EntryAction::Materialize {
            payload_root: payload,
        },
    };
    let mut staged = StagedMutation::stage("operation-2", vec![intent], || false).unwrap();
    std::fs::write(destination.join("external"), b"changed").unwrap();

    assert_eq!(
        staged.swap(|| false).unwrap_err(),
        MutationError::StaleTarget
    );
    staged.cleanup().unwrap();
}

#[test]
fn replace_rejects_existing_file_content_changed_after_stage() {
    let temp = tempfile::tempdir().unwrap();
    let destination = temp.path().join("targets/demo");
    std::fs::create_dir_all(&destination).unwrap();
    std::fs::write(destination.join("SKILL.md"), b"old").unwrap();
    let source = temp.path().join("source");
    std::fs::create_dir(&source).unwrap();
    std::fs::write(source.join("SKILL.md"), b"new").unwrap();
    let payload = temp.path().join("payload");
    build_payload(&source, &payload).unwrap();
    let intent = EntryIntent {
        destination: destination.clone(),
        expected_parent: parent_identity(destination.parent().unwrap()).unwrap(),
        expected_fingerprint: fingerprint_path(&destination).unwrap(),
        expected_content_hash: Some(content_hash_path(&destination).unwrap()),
        action: EntryAction::Materialize {
            payload_root: payload,
        },
    };
    let mut staged = StagedMutation::stage("operation-3", vec![intent], || false).unwrap();
    std::fs::write(destination.join("SKILL.md"), b"locally changed").unwrap();

    assert_eq!(
        staged.swap(|| false).unwrap_err(),
        MutationError::StaleTarget
    );
    staged.cleanup().unwrap();
}

#[test]
fn cancellation_is_delayed_after_the_entry_set_starts_swapping() {
    let temp = tempfile::tempdir().unwrap();
    let destinations = [
        temp.path().join("targets/one"),
        temp.path().join("targets/two"),
    ];
    let intents = destinations
        .iter()
        .map(|destination| {
            std::fs::create_dir_all(destination).unwrap();
            std::fs::write(destination.join("SKILL.md"), b"old").unwrap();
            EntryIntent {
                destination: destination.clone(),
                expected_parent: parent_identity(destination.parent().unwrap()).unwrap(),
                expected_fingerprint: fingerprint_path(destination).unwrap(),
                expected_content_hash: None,
                action: EntryAction::Remove,
            }
        })
        .collect();
    let mut staged = StagedMutation::stage("operation-4", intents, || false).unwrap();
    let checks = std::cell::Cell::new(0_u32);

    staged
        .swap(|| {
            checks.set(checks.get() + 1);
            checks.get() >= 4
        })
        .unwrap();

    assert!(destinations.iter().all(|destination| !destination.exists()));
    staged.verify(|| false).unwrap();
    staged.restore().unwrap();
    assert!(destinations.iter().all(|destination| destination.is_dir()));
    staged.cleanup().unwrap();
}

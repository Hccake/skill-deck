use std::collections::BTreeMap;

use environment_engine::lock::{apply, EntryMutation, LockError, LockMutation, LockSchema};
use serde_json::json;

#[test]
fn conditional_lock_apply_preserves_unknown_fields_and_returns_new_evidence() {
    let current = serde_json::to_vec(&json!({
        "version": 3,
        "skills": {
            "demo": {
                "source": "old",
                "skillFolderHash": "old-hash",
                "futureEntry": { "keep": true }
            },
            "other": { "source": "untouched" }
        },
        "futureRoot": [1, 2, 3]
    }))
    .unwrap();
    let mutation = LockMutation {
        schema: LockSchema::Global,
        entry: EntryMutation::Replace {
            key: "demo".to_string(),
            replacement: json!({
                "source": "new",
                "skillFolderHash": "new-hash"
            }),
        },
        root_replacements: BTreeMap::new(),
        expected_entries: BTreeMap::from([(
            "demo".to_string(),
            Some(json!({
                "source": "old",
                "skillFolderHash": "old-hash",
                "futureEntry": { "keep": true }
            })),
        )]),
        expected_roots: BTreeMap::new(),
    };

    let applied = apply(Some(&current), None, &mutation).unwrap();
    let value: serde_json::Value = serde_json::from_slice(&applied.bytes).unwrap();

    assert_eq!(value["skills"]["demo"]["source"], "new");
    assert_eq!(value["skills"]["demo"]["futureEntry"]["keep"], true);
    assert_eq!(value["skills"]["other"]["source"], "untouched");
    assert_eq!(value["futureRoot"], json!([1, 2, 3]));
    assert_eq!(
        applied.receipt.entries["demo"],
        Some(value["skills"]["demo"].clone())
    );
}

#[test]
fn conditional_lock_apply_rejects_changed_selected_entry() {
    let current = serde_json::to_vec(&json!({
        "version": 1,
        "skills": { "demo": { "source": "changed" } }
    }))
    .unwrap();
    let mutation = LockMutation {
        schema: LockSchema::Project,
        entry: EntryMutation::Remove {
            key: "demo".to_string(),
        },
        root_replacements: BTreeMap::new(),
        expected_entries: BTreeMap::from([(
            "demo".to_string(),
            Some(json!({ "source": "expected" })),
        )]),
        expected_roots: BTreeMap::new(),
    };

    assert_eq!(
        apply(Some(&current), None, &mutation).unwrap_err(),
        LockError::EntryConflict {
            key: "demo".to_string()
        }
    );
}

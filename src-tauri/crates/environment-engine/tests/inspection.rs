#![cfg(target_os = "linux")]

use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::os::unix::fs::symlink;

use environment_engine::inspection::{
    inspect, inspect_with_cancel, EntryKind, ErrorCode, InspectionError, InspectionRequest,
    InspectionRoot,
};

#[test]
fn inspects_direct_children_and_bounded_skill_documents() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path().join("skills");
    std::fs::create_dir_all(root.join("Alpha")).expect("Alpha directory");
    std::fs::create_dir_all(root.join("beta")).expect("beta directory");
    std::fs::write(root.join("Alpha/SKILL.md"), b"123456").expect("Alpha document");
    std::fs::write(root.join("beta/SKILL.md"), b"abcdef").expect("beta document");

    let snapshot = inspect(&InspectionRequest {
        roots: vec![InspectionRoot {
            path: root,
            stat_only: false,
        }],
        per_file_limit: 4,
        aggregate_limit: 6,
    })
    .expect("inspection");

    assert_eq!(snapshot.total_content_bytes, 6);
    let alpha = snapshot
        .facts
        .iter()
        .find(|fact| fact.relative_path.as_path() == std::path::Path::new("Alpha/SKILL.md"))
        .expect("Alpha fact");
    assert_eq!(alpha.kind, EntryKind::File);
    assert_eq!(alpha.content_bytes, b"1234");
    assert!(alpha.truncated);

    let beta = snapshot
        .facts
        .iter()
        .find(|fact| fact.relative_path.as_path() == std::path::Path::new("beta/SKILL.md"))
        .expect("beta fact");
    assert_eq!(beta.content_bytes, b"ab");
    assert!(beta.truncated);
}

#[test]
fn stat_only_root_does_not_enumerate_or_consume_content_budget() {
    let temp = tempfile::tempdir().expect("tempdir");
    std::fs::create_dir_all(temp.path().join("toolkit")).expect("toolkit directory");
    std::fs::write(temp.path().join("toolkit/SKILL.md"), b"document").expect("document");

    let snapshot = inspect(&InspectionRequest {
        roots: vec![InspectionRoot {
            path: temp.path().to_path_buf(),
            stat_only: true,
        }],
        per_file_limit: 16,
        aggregate_limit: 16,
    })
    .expect("inspection");

    assert_eq!(snapshot.facts.len(), 1);
    assert_eq!(snapshot.facts[0].kind, EntryKind::Directory);
    assert_eq!(snapshot.total_content_bytes, 0);
}

#[test]
fn follows_a_child_directory_symlink_only_for_its_skill_document() {
    let temp = tempfile::tempdir().expect("tempdir");
    let canonical = temp.path().join("canonical/toolkit");
    let root = temp.path().join("skills");
    std::fs::create_dir_all(&canonical).expect("canonical directory");
    std::fs::create_dir_all(&root).expect("skills directory");
    std::fs::write(canonical.join("SKILL.md"), b"document").expect("document");
    symlink(&canonical, root.join("toolkit")).expect("directory symlink");
    symlink(temp.path().join("missing"), root.join("broken")).expect("broken symlink");

    let snapshot = inspect(&InspectionRequest {
        roots: vec![InspectionRoot {
            path: root,
            stat_only: false,
        }],
        per_file_limit: 16,
        aggregate_limit: 16,
    })
    .expect("inspection");

    let linked = snapshot
        .facts
        .iter()
        .find(|fact| fact.relative_path.as_path() == std::path::Path::new("toolkit"))
        .expect("linked directory");
    assert_eq!(linked.kind, EntryKind::Symlink);
    assert_eq!(linked.resolved_target.as_deref(), Some(canonical.as_path()));
    assert!(snapshot
        .facts
        .iter()
        .any(|fact| fact.relative_path.as_path() == std::path::Path::new("toolkit/SKILL.md")));
    assert!(!snapshot
        .facts
        .iter()
        .any(|fact| fact.relative_path.as_path() == std::path::Path::new("broken/SKILL.md")));
}

#[test]
fn preserves_non_utf8_relative_paths_without_lossy_conversion() {
    let temp = tempfile::tempdir().expect("tempdir");
    let raw_name = std::ffi::OsString::from_vec(vec![b's', b'k', 0x80]);
    std::fs::write(temp.path().join(&raw_name), b"payload").expect("non-UTF-8 entry");

    let snapshot = inspect(&InspectionRequest {
        roots: vec![InspectionRoot {
            path: temp.path().to_path_buf(),
            stat_only: false,
        }],
        per_file_limit: 16,
        aggregate_limit: 16,
    })
    .expect("inspection");

    let fact = snapshot
        .facts
        .iter()
        .find(|fact| fact.relative_path.as_os_str().as_bytes() == raw_name.as_bytes())
        .expect("raw path fact");
    assert_eq!(fact.kind, EntryKind::File);
}

#[test]
fn isolates_missing_and_unreadable_roots() {
    let temp = tempfile::tempdir().expect("tempdir");
    let missing = temp.path().join("missing");
    let file = temp.path().join("file");
    std::fs::write(&file, b"content").expect("file");

    let snapshot = inspect(&InspectionRequest {
        roots: vec![
            InspectionRoot {
                path: missing,
                stat_only: false,
            },
            InspectionRoot {
                path: file,
                stat_only: false,
            },
        ],
        per_file_limit: 16,
        aggregate_limit: 32,
    })
    .expect("inspection");

    assert_eq!(snapshot.facts[0].kind, EntryKind::Missing);
    assert_eq!(snapshot.facts[0].error_code, None);
    assert_eq!(snapshot.facts[1].kind, EntryKind::File);
    assert_ne!(
        snapshot.facts[1].error_code,
        Some(ErrorCode::PathUnavailable)
    );
}

#[test]
fn cooperative_cancellation_stops_between_filesystem_entries() {
    let temp = tempfile::tempdir().expect("tempdir");
    for index in 0..32 {
        std::fs::create_dir_all(temp.path().join(format!("skill-{index}")))
            .expect("skill directory");
    }
    let checks = std::cell::Cell::new(0usize);

    let error = inspect_with_cancel(
        &InspectionRequest {
            roots: vec![InspectionRoot {
                path: temp.path().to_path_buf(),
                stat_only: false,
            }],
            per_file_limit: 16,
            aggregate_limit: 16,
        },
        || {
            checks.set(checks.get() + 1);
            checks.get() > 2
        },
    )
    .unwrap_err();

    assert_eq!(error, InspectionError::Cancelled);
}

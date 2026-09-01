#![cfg(target_os = "linux")]

use std::os::unix::fs::{symlink, PermissionsExt};

use environment_engine::entry::{inspect_entries, EntryKind, EntryRequest};
use environment_engine::manifest::{build_manifest, ManifestKind, ManifestRequest};
use environment_engine::projection::{project_targets, ProjectionRequest};

#[test]
fn entry_facts_do_not_follow_the_final_symlink() {
    let temp = tempfile::tempdir().unwrap();
    let target = temp.path().join("target");
    let link = temp.path().join("link");
    std::fs::write(&target, b"first").unwrap();
    symlink(&target, &link).unwrap();

    let before = inspect_entries(&EntryRequest {
        paths: vec![link.clone()],
    })
    .unwrap();
    std::fs::write(&target, b"changed target").unwrap();
    let after = inspect_entries(&EntryRequest { paths: vec![link] }).unwrap();

    assert_eq!(before.facts[0].kind, EntryKind::Symlink);
    assert_eq!(before.facts[0], after.facts[0]);
}

#[test]
fn projection_resolves_existing_ancestors_before_appending_components() {
    let temp = tempfile::tempdir().unwrap();
    let physical = temp.path().join("physical");
    let logical = temp.path().join("logical");
    std::fs::create_dir(&physical).unwrap();
    symlink(&physical, &logical).unwrap();
    let destination = logical.join("skills/demo");

    let response = project_targets(&ProjectionRequest {
        destinations: vec![destination.clone()],
    })
    .unwrap();

    assert_eq!(
        response.targets[0].physical_destination,
        physical.join("skills/demo")
    );
    assert_eq!(
        response.targets[0].relative_components,
        vec![
            std::path::PathBuf::from("skills"),
            std::path::PathBuf::from("demo"),
        ]
    );
    assert!(!destination.exists());
}

#[test]
fn manifest_captures_digest_executable_directories_and_symlinks() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("skill");
    std::fs::create_dir_all(root.join("empty")).unwrap();
    std::fs::write(root.join("run.sh"), b"#!/bin/sh\n").unwrap();
    let mut permissions = std::fs::metadata(root.join("run.sh"))
        .unwrap()
        .permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(root.join("run.sh"), permissions).unwrap();
    symlink("run.sh", root.join("current")).unwrap();

    let response = build_manifest(&ManifestRequest { root }).unwrap();

    assert!(response.records.iter().any(|record| {
        record.relative_path.as_path() == std::path::Path::new("run.sh")
            && record.kind == ManifestKind::File
            && record.executable
    }));
    assert!(response.records.iter().any(|record| {
        record.relative_path.as_path() == std::path::Path::new("current")
            && record.kind == ManifestKind::Symlink
            && record.symlink_target.as_deref() == Some(std::path::Path::new("run.sh"))
    }));
}

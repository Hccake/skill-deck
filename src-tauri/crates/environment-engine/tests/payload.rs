#![cfg(target_os = "linux")]

use std::os::unix::fs::{symlink, PermissionsExt};

use environment_engine::payload::{
    build_payload, source_metadata_fingerprint, verify_payload, PayloadEntryKind, PayloadError,
};

#[test]
fn payload_build_preserves_linux_content_semantics_without_loading_duplicate_blobs() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("source");
    let payload_root = temp.path().join("payload");
    std::fs::create_dir_all(source.join("scripts")).unwrap();
    std::fs::create_dir_all(source.join("assets")).unwrap();
    std::fs::create_dir_all(source.join(".git")).unwrap();
    std::fs::write(source.join("SKILL.md"), b"skill").unwrap();
    std::fs::write(source.join("scripts/run.sh"), b"#!/bin/sh\n").unwrap();
    std::fs::write(source.join("assets/copy.txt"), b"skill").unwrap();
    std::fs::write(source.join("metadata.json"), b"excluded").unwrap();
    std::fs::write(source.join(".git/config"), b"excluded").unwrap();
    std::fs::set_permissions(
        source.join("scripts/run.sh"),
        std::fs::Permissions::from_mode(0o755),
    )
    .unwrap();

    let built = build_payload(&source, &payload_root).unwrap();

    assert_eq!(built.total_bytes, 15);
    assert_eq!(built.manifest.payload_id, built.manifest.payload_root_hash);
    assert_eq!(
        std::fs::read_dir(payload_root.join("blobs"))
            .unwrap()
            .count(),
        2
    );
    assert!(built.manifest.entries.iter().any(|entry| {
        entry.relative_path == "scripts/run.sh"
            && entry.kind == PayloadEntryKind::File
            && entry.executable
    }));
    assert!(!built.manifest.entries.iter().any(
        |entry| entry.relative_path.contains(".git") || entry.relative_path == "metadata.json"
    ));
    assert_eq!(verify_payload(&payload_root).unwrap(), built.manifest);
}

#[test]
fn payload_build_rejects_links_outside_the_selected_source() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("source");
    std::fs::create_dir(&source).unwrap();
    std::fs::write(temp.path().join("outside"), b"outside").unwrap();
    symlink(temp.path().join("outside"), source.join("linked")).unwrap();

    assert!(matches!(
        build_payload(&source, &temp.path().join("payload")),
        Err(PayloadError::UnsafeSourceLink { .. })
    ));
}

#[test]
fn source_fingerprint_tracks_linux_metadata_and_rejects_external_links() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("source");
    std::fs::create_dir(&source).unwrap();
    std::fs::write(source.join("SKILL.md"), b"demo").unwrap();
    let before = source_metadata_fingerprint(&source).unwrap();

    let mut permissions = std::fs::metadata(source.join("SKILL.md"))
        .unwrap()
        .permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(source.join("SKILL.md"), permissions).unwrap();
    let after = source_metadata_fingerprint(&source).unwrap();
    assert_ne!(before, after);

    std::fs::write(temp.path().join("outside"), b"outside").unwrap();
    symlink(temp.path().join("outside"), source.join("external")).unwrap();
    assert!(matches!(
        source_metadata_fingerprint(&source),
        Err(PayloadError::UnsafeSourceLink { .. })
    ));
}

#[test]
fn cancelled_payload_build_removes_its_partial_destination() {
    use environment_engine::payload::build_payload_with_cancel;

    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("source");
    let payload = temp.path().join("payload");
    std::fs::create_dir(&source).unwrap();
    std::fs::write(source.join("SKILL.md"), vec![7_u8; 128 * 1024]).unwrap();

    let checks = std::cell::Cell::new(0_u32);
    let result = build_payload_with_cancel(&source, &payload, || {
        checks.set(checks.get() + 1);
        checks.get() > 2
    });

    assert!(matches!(result, Err(PayloadError::Cancelled)));
    assert!(!payload.exists());
}

#[test]
fn payload_build_rejects_non_utf8_manifest_paths() {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;

    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("source");
    std::fs::create_dir(&source).unwrap();
    std::fs::write(
        source.join(OsString::from_vec(vec![b's', 0xff])),
        b"content",
    )
    .unwrap();

    assert!(matches!(
        build_payload(&source, &temp.path().join("payload")),
        Err(PayloadError::InvalidSource)
    ));
}

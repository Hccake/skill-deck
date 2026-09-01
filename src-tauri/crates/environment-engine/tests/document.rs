#![cfg(target_os = "linux")]

use environment_engine::document::{
    read_documents, remove_document_if_revision, write_document_atomic, DocumentQuery,
    DocumentRequest, DocumentState, DocumentWriteError,
};
use sha2::Digest;

#[test]
fn conditional_document_write_replaces_a_file_and_returns_its_revision() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("state/projects.json");

    let bytes = br#"{"projects":[]}"#;
    let revision = write_document_atomic(&path, None, bytes).unwrap();
    let expected = format!("sha256:{:x}", sha2::Sha256::digest(bytes));
    assert_eq!(revision, expected);
    assert_eq!(std::fs::read(&path).unwrap(), bytes);

    let replacement = br#"{"projects":["demo"]}"#;
    let replacement_revision = write_document_atomic(&path, Some(&revision), replacement).unwrap();
    assert_eq!(
        replacement_revision,
        format!("sha256:{:x}", sha2::Sha256::digest(replacement))
    );
    assert_eq!(std::fs::read(&path).unwrap(), replacement);
}

#[test]
fn conditional_document_remove_preserves_a_changed_target() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("applications/project.json");
    let revision = write_document_atomic(&path, None, b"current").unwrap();

    assert_eq!(
        remove_document_if_revision(&path, Some("sha256:wrong")).unwrap_err(),
        DocumentWriteError::Conflict
    );
    assert_eq!(std::fs::read(&path).unwrap(), b"current");

    remove_document_if_revision(&path, Some(&revision)).unwrap();
    assert!(!path.exists());
}

#[test]
fn conditional_document_write_rejects_a_changed_file_without_overwriting_it() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("state/projects.json");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, b"old").unwrap();

    assert_eq!(
        write_document_atomic(&path, Some("sha256:wrong"), b"new").unwrap_err(),
        DocumentWriteError::Conflict
    );
    assert_eq!(std::fs::read(&path).unwrap(), b"old");
    assert_eq!(
        std::fs::read_dir(path.parent().unwrap()).unwrap().count(),
        1
    );
}

#[test]
fn conditional_document_write_rejects_directory_and_symlink_targets() {
    let temp = tempfile::tempdir().unwrap();
    let directory = temp.path().join("state");
    std::fs::create_dir(&directory).unwrap();
    assert_eq!(
        write_document_atomic(&directory, None, b"new").unwrap_err(),
        DocumentWriteError::InvalidTarget
    );
    let target = temp.path().join("target");
    std::fs::write(&target, b"old").unwrap();
    let link = temp.path().join("link");
    std::os::unix::fs::symlink(&target, &link).unwrap();
    assert_eq!(
        write_document_atomic(&link, None, b"new").unwrap_err(),
        DocumentWriteError::InvalidTarget
    );
}

#[test]
fn optional_documents_are_bounded_and_isolated() {
    let temp = tempfile::tempdir().unwrap();
    let file = temp.path().join("document");
    let directory = temp.path().join("directory");
    std::fs::write(&file, b"abcdef").unwrap();
    std::fs::create_dir(&directory).unwrap();

    let response = read_documents(&DocumentRequest {
        queries: vec![
            DocumentQuery {
                path: file,
                limit: 4,
            },
            DocumentQuery {
                path: temp.path().join("missing"),
                limit: 4,
            },
            DocumentQuery {
                path: directory,
                limit: 4,
            },
        ],
        aggregate_limit: 8,
    })
    .unwrap();

    assert_eq!(
        response.facts[0].state,
        DocumentState::Bytes(b"abcd".to_vec())
    );
    assert!(response.facts[0].truncated);
    assert_eq!(response.facts[1].state, DocumentState::Missing);
    assert_eq!(response.facts[2].state, DocumentState::NotFile);
    assert_eq!(response.total_content_bytes, 4);
}

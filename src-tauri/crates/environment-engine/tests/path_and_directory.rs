#![cfg(target_os = "linux")]

use std::os::unix::fs::symlink;

use environment_engine::directory::{
    count_entries, list_child_directories, DirectoryCountRequest, DirectoryListRequest,
};
use environment_engine::path::{inspect_paths, ContentState, PathKind, PathQuery, PathRequest};

#[test]
fn path_metadata_classifies_links_and_reads_bounded_content() {
    let temp = tempfile::tempdir().unwrap();
    let directory = temp.path().join("directory");
    let file = temp.path().join("package.json");
    std::fs::create_dir(&directory).unwrap();
    std::fs::write(&file, b"abcdef").unwrap();
    symlink(&directory, temp.path().join("directory-link")).unwrap();
    symlink(&file, temp.path().join("file-link")).unwrap();
    symlink(temp.path().join("missing"), temp.path().join("broken")).unwrap();

    let response = inspect_paths(&PathRequest {
        queries: vec![
            PathQuery {
                path: directory,
                content_limit: None,
            },
            PathQuery {
                path: temp.path().join("directory-link"),
                content_limit: None,
            },
            PathQuery {
                path: temp.path().join("file-link"),
                content_limit: Some(4),
            },
            PathQuery {
                path: temp.path().join("broken"),
                content_limit: None,
            },
        ],
        aggregate_content_limit: 8,
    })
    .unwrap();

    assert_eq!(response.facts[0].kind, PathKind::Directory);
    assert_eq!(response.facts[1].kind, PathKind::SymlinkDirectory);
    assert_eq!(response.facts[2].kind, PathKind::SymlinkOther);
    assert_eq!(
        response.facts[2].content,
        ContentState::Bytes(b"abcd".to_vec())
    );
    assert!(response.facts[2].content_truncated);
    assert_eq!(response.facts[3].kind, PathKind::BrokenLink);
}

#[test]
fn directory_count_isolates_missing_paths_and_caps_entries() {
    let temp = tempfile::tempdir().unwrap();
    let directory = temp.path().join("skills");
    std::fs::create_dir(&directory).unwrap();
    for index in 0..4 {
        std::fs::write(directory.join(format!("skill-{index}")), b"skill").unwrap();
    }

    let response = count_entries(&DirectoryCountRequest {
        paths: vec![directory, temp.path().join("missing")],
        limit: 3,
    })
    .unwrap();

    assert_eq!(response.facts[0].observed_count, Some(3));
    assert!(response.facts[0].truncated);
    assert_eq!(response.facts[1].observed_count, None);
    assert!(!response.facts[1].truncated);
}

#[test]
fn child_directory_listing_is_sorted_and_bounded() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::create_dir(temp.path().join("beta")).unwrap();
    std::fs::create_dir(temp.path().join("Alpha")).unwrap();
    std::fs::write(temp.path().join("file"), b"file").unwrap();

    let response = list_child_directories(&DirectoryListRequest {
        path: temp.path().to_path_buf(),
        limit: 1,
    })
    .unwrap();

    assert_eq!(response.names, vec![std::path::PathBuf::from("Alpha")]);
    assert!(response.truncated);
}

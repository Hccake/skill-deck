#![cfg(target_os = "linux")]
#![allow(
    clippy::disallowed_methods,
    reason = "Worker Git 集成测试需要直接创建受控的本地 Git fixture"
)]

use std::process::Command;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Duration;

use environment_protocol::{SourceScanMode, SourceScanRequest, SourceScanRoot};
use wsl_environment_worker::source::scan_source;
use wsl_environment_worker::source::{GitSourceOptions, SourceManager};

fn git(cwd: &std::path::Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .current_dir(cwd)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().to_string()
}

#[tokio::test]
async fn source_manager_owns_git_clones_but_not_opened_local_directories() {
    let temp = tempfile::tempdir().unwrap();
    let repository = temp.path().join("repository");
    std::fs::create_dir(&repository).unwrap();
    git(&repository, &["init", "-b", "main"]);
    git(&repository, &["config", "user.email", "test@example.com"]);
    git(&repository, &["config", "user.name", "Skill Deck Test"]);
    std::fs::write(repository.join("SKILL.md"), b"demo").unwrap();
    std::fs::create_dir_all(repository.join("skills/demo")).unwrap();
    std::fs::write(repository.join("skills/demo/SKILL.md"), b"nested").unwrap();
    git(&repository, &["add", "."]);
    git(&repository, &["commit", "-m", "fixture"]);
    let expected_revision = git(&repository, &["rev-parse", "HEAD"]);
    let expected_skill_revision = git(&repository, &["rev-parse", "HEAD:skills/demo"]);

    let managed_base = temp.path().join("managed");
    std::fs::create_dir(&managed_base).unwrap();
    let mut manager = SourceManager::new(managed_base).unwrap();
    let local = manager.open_local(repository.to_str().unwrap()).unwrap();
    let git_source = manager
        .acquire_git(
            GitSourceOptions {
                url: repository.to_string_lossy().into_owned(),
                git_ref: None,
                proxy: None,
                deadline: Duration::from_secs(30),
            },
            Arc::new(AtomicBool::new(false)),
        )
        .await
        .unwrap();

    assert_eq!(
        git_source.revision.as_deref(),
        Some(expected_revision.as_str())
    );
    let tree_revision = manager
        .tree_revision(
            git_source.id,
            std::path::Path::new(""),
            Duration::from_secs(30),
            Arc::new(AtomicBool::new(false)),
        )
        .await
        .unwrap();
    assert_eq!(tree_revision.len(), expected_revision.len());
    let skill_revision = manager
        .tree_revision(
            git_source.id,
            std::path::Path::new("skills/demo"),
            Duration::from_secs(30),
            Arc::new(AtomicBool::new(false)),
        )
        .await
        .unwrap();
    assert_eq!(skill_revision, expected_skill_revision);
    assert!(manager
        .root(git_source.id)
        .unwrap()
        .join("SKILL.md")
        .is_file());
    let managed_root = manager.root(git_source.id).unwrap().to_path_buf();
    manager.release(git_source.id).unwrap();
    assert!(!managed_root.exists());
    manager.release(local.id).unwrap();
    assert!(repository.join("SKILL.md").is_file());
}

#[test]
fn source_relative_paths_cannot_escape_the_opened_root() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("source");
    std::fs::create_dir(&root).unwrap();
    let mut manager = SourceManager::new(temp.path().join("managed")).unwrap();
    let source = manager.open_local(root.to_str().unwrap()).unwrap();

    assert!(manager
        .resolve(source.id, std::path::Path::new("skills/demo"))
        .is_ok());
    assert!(manager
        .resolve(source.id, std::path::Path::new("../outside"))
        .is_err());
    assert!(manager
        .resolve(source.id, std::path::Path::new("/absolute"))
        .is_err());
}

#[test]
fn source_scan_projects_relative_wire_paths_through_the_handle() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("source");
    std::fs::create_dir_all(root.join("skills/demo")).unwrap();
    std::fs::write(root.join("skills/demo/SKILL.md"), b"demo").unwrap();
    let mut manager = SourceManager::new(temp.path().join("managed")).unwrap();
    let source = manager.open_local(root.to_str().unwrap()).unwrap();

    let response = scan_source(
        &manager,
        SourceScanRequest {
            source_id: source.id,
            roots: vec![SourceScanRoot {
                relative_path: b"skills".to_vec(),
                stat_only: false,
            }],
            mode: SourceScanMode::Recursive,
            per_file_limit: 1024,
            aggregate_limit: 4096,
            deadline_millis: 30_000,
        },
        || false,
    )
    .unwrap();

    assert!(response
        .entries
        .iter()
        .any(|entry| entry.relative_path == b"demo/SKILL.md"));
}

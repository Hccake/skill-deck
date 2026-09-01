#![cfg(target_os = "linux")]

use environment_engine::source_inventory::{
    scan_source, SourceEntryKind, SourceInventoryRequest, SourceRoot, SourceScanMode,
};

#[test]
fn recursive_inventory_reads_only_source_documents_and_prunes_dependency_trees() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("repo");
    std::fs::create_dir_all(root.join("skills/demo")).unwrap();
    std::fs::create_dir_all(root.join("skills/linked")).unwrap();
    std::fs::create_dir_all(root.join("node_modules/ignored")).unwrap();
    std::fs::create_dir_all(root.join(".claude-plugin")).unwrap();
    std::fs::write(root.join("skills/demo/SKILL.md"), b"demo").unwrap();
    std::fs::write(root.join("linked-document"), b"linked-demo").unwrap();
    std::os::unix::fs::symlink(
        root.join("linked-document"),
        root.join("skills/linked/SKILL.md"),
    )
    .unwrap();
    std::fs::write(root.join("node_modules/ignored/SKILL.md"), b"ignored").unwrap();
    std::fs::write(root.join(".claude-plugin/plugin.json"), b"plugin").unwrap();
    std::fs::write(root.join("skills-lock.json"), b"lock").unwrap();
    std::fs::write(root.join("ordinary.txt"), b"ordinary").unwrap();

    let response = scan_source(&SourceInventoryRequest {
        roots: vec![SourceRoot {
            path: root,
            stat_only: false,
        }],
        mode: SourceScanMode::Recursive,
        per_file_limit: 16,
        aggregate_limit: 64,
    })
    .unwrap();

    let documents = response
        .entries
        .iter()
        .filter(|entry| !entry.relative_path.as_os_str().is_empty())
        .map(|entry| {
            (
                entry.relative_path.to_string_lossy().into_owned(),
                entry.content_bytes.clone(),
            )
        })
        .collect::<std::collections::BTreeMap<_, _>>();
    assert_eq!(documents["skills/demo/SKILL.md"], b"demo");
    assert_eq!(documents["skills/linked/SKILL.md"], b"linked-demo");
    assert_eq!(documents[".claude-plugin/plugin.json"], b"plugin");
    assert_eq!(documents["skills-lock.json"], b"lock");
    assert!(!documents.contains_key("node_modules/ignored/SKILL.md"));
    assert!(!documents.contains_key("ordinary.txt"));
    assert_eq!(response.entries[0].kind, SourceEntryKind::Directory);
}

#[test]
fn priority_inventory_reads_only_direct_child_skill_documents() {
    use std::os::unix::fs::symlink;

    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("catalog");
    let linked_source = temp.path().join("linked-source");
    std::fs::create_dir_all(root.join("direct/scripts")).unwrap();
    std::fs::create_dir_all(root.join("category/nested")).unwrap();
    std::fs::create_dir_all(&linked_source).unwrap();
    std::fs::write(root.join("direct/skill.MD"), b"direct").unwrap();
    std::fs::write(root.join("direct/scripts/SKILL.md"), b"too-deep").unwrap();
    std::fs::write(root.join("category/nested/SKILL.md"), b"nested").unwrap();
    std::fs::write(linked_source.join("SKILL.md"), b"linked").unwrap();
    symlink(linked_source, root.join("linked")).unwrap();

    let response = scan_source(&SourceInventoryRequest {
        roots: vec![SourceRoot {
            path: root,
            stat_only: false,
        }],
        mode: SourceScanMode::PriorityDirectories,
        per_file_limit: 16,
        aggregate_limit: 64,
    })
    .unwrap();

    let relative_paths = response
        .entries
        .iter()
        .map(|entry| entry.relative_path.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    assert_eq!(relative_paths, vec!["", "direct/skill.MD"]);
}

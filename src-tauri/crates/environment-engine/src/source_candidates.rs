use crate::source_inventory::{
    SourceEntry, SourceEntryKind, SourceInventoryError, SourceInventoryRequest,
    SourceInventoryResponse,
};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

const MAX_ENTRIES: usize = 100_000;

fn invalid(_: impl std::fmt::Debug) -> SourceInventoryError {
    SourceInventoryError::InvalidRequest
}

fn is_link(metadata: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    {
        metadata.file_type().is_symlink()
    }
}

pub fn scan_update_candidates(
    request: &SourceInventoryRequest,
    is_cancelled: impl Fn() -> bool,
) -> Result<SourceInventoryResponse, SourceInventoryError> {
    if request.roots.len() != 1
        || request.roots[0].stat_only
        || request.per_file_limit == 0
        || request.aggregate_limit < request.per_file_limit
    {
        return Err(SourceInventoryError::InvalidRequest);
    }
    let root = &request.roots[0].path;
    crate::directory::plain_descendant_directory(root, Path::new("")).map_err(invalid)?;
    let mut pending = vec![PathBuf::new()];
    let mut entries = Vec::new();
    let mut total = 0u32;
    let mut visited = 0usize;
    while let Some(relative) = pending.pop() {
        if is_cancelled() {
            return Err(SourceInventoryError::Cancelled);
        }
        let directory = root.join(&relative);
        // Git 子模块和嵌套仓库不属于当前来源快照的普通目录。
        if !relative.as_os_str().is_empty() && directory.join(".git").exists() {
            continue;
        }
        for child in fs::read_dir(&directory).map_err(invalid)? {
            if is_cancelled() {
                return Err(SourceInventoryError::Cancelled);
            }
            visited += 1;
            if visited > MAX_ENTRIES {
                return Err(SourceInventoryError::InvalidRequest);
            }
            let child = child.map_err(invalid)?;
            let name = child.file_name();
            let name = name
                .to_str()
                .filter(|name| !name.contains('\\'))
                .ok_or(SourceInventoryError::InvalidRequest)?;
            let metadata = fs::symlink_metadata(child.path()).map_err(invalid)?;
            if metadata.is_dir() {
                if !is_link(&metadata)
                    && !matches!(
                        name,
                        ".git"
                            | "node_modules"
                            | "dist"
                            | "build"
                            | "__pycache__"
                            | "__pypackages__"
                    )
                {
                    pending.push(relative.join(name));
                }
                continue;
            }
            let relative_path = relative.join(name);
            let relevant = name.eq_ignore_ascii_case("SKILL.md")
                || relative_path == Path::new("skills-lock.json")
                || relative_path == Path::new(".claude-plugin/marketplace.json")
                || relative_path == Path::new(".claude-plugin/plugin.json");
            if !relevant {
                continue;
            }
            if !metadata.is_file() || is_link(&metadata) {
                return Err(SourceInventoryError::InvalidRequest);
            }
            let allowance = request
                .per_file_limit
                .min(request.aggregate_limit.saturating_sub(total));
            let mut content_bytes = Vec::new();
            fs::File::open(child.path())
                .map_err(invalid)?
                .take(u64::from(allowance) + 1)
                .read_to_end(&mut content_bytes)
                .map_err(invalid)?;
            if content_bytes.len() > allowance as usize {
                return Err(SourceInventoryError::InvalidRequest);
            }
            total += content_bytes.len() as u32;
            entries.push(SourceEntry {
                root_index: 0,
                relative_path,
                kind: SourceEntryKind::File,
                link_target: None,
                content_bytes,
                truncated: false,
                error: None,
            });
        }
    }
    entries.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
    Ok(SourceInventoryResponse {
        entries,
        total_content_bytes: total,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source_inventory::{SourceRoot, SourceScanMode};
    use std::fs;
    use std::path::Path;

    fn request(root: &Path) -> SourceInventoryRequest {
        SourceInventoryRequest {
            roots: vec![SourceRoot {
                path: root.to_path_buf(),
                stat_only: false,
            }],
            mode: SourceScanMode::Recursive,
            per_file_limit: 1024,
            aggregate_limit: 8192,
        }
    }

    fn write(root: &Path, path: &str, content: &[u8]) {
        let path = root.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, content).unwrap();
    }

    #[test]
    fn candidate_scan_keeps_deep_and_nested_same_name_documents() {
        let root = tempfile::tempdir().unwrap();
        write(root.path(), "SKILL.md", b"root");
        write(root.path(), "a/b/c/d/e/f/g/h/SKILL.md", b"same name");
        write(root.path(), "tools/SKILL.md", b"same name");
        write(root.path(), "node_modules/ignored/SKILL.md", b"ignored");
        let scan = scan_update_candidates(&request(root.path()), || false).unwrap();
        assert_eq!(scan.entries.len(), 3);
        assert!(scan
            .entries
            .iter()
            .any(|entry| entry.relative_path == Path::new("a/b/c/d/e/f/g/h/SKILL.md")));
        assert_eq!(
            scan.entries
                .iter()
                .filter(|entry| entry.content_bytes == b"same name")
                .count(),
            2
        );
    }

    #[test]
    fn candidate_scan_never_returns_a_partial_catalog_when_limits_are_hit() {
        let root = tempfile::tempdir().unwrap();
        write(root.path(), "one/SKILL.md", &[b'x'; 1025]);
        assert_eq!(
            scan_update_candidates(&request(root.path()), || false),
            Err(SourceInventoryError::InvalidRequest)
        );
        write(root.path(), "one/SKILL.md", &[b'x'; 800]);
        write(root.path(), "two/SKILL.md", &[b'y'; 800]);
        let mut request = request(root.path());
        request.aggregate_limit = 1024;
        assert_eq!(
            scan_update_candidates(&request, || false),
            Err(SourceInventoryError::InvalidRequest)
        );
    }

    #[test]
    fn candidate_scan_propagates_cancellation() {
        let root = tempfile::tempdir().unwrap();
        assert_eq!(
            scan_update_candidates(&request(root.path()), || true),
            Err(SourceInventoryError::Cancelled)
        );
    }

    #[cfg(unix)]
    #[test]
    fn candidate_scan_excludes_directory_links_and_rejects_document_links() {
        let root = tempfile::tempdir().unwrap();
        let external = tempfile::tempdir().unwrap();
        write(external.path(), "SKILL.md", b"external");
        std::os::unix::fs::symlink(external.path(), root.path().join("linked")).unwrap();
        assert!(scan_update_candidates(&request(root.path()), || false)
            .unwrap()
            .entries
            .is_empty());
        std::os::unix::fs::symlink(
            external.path().join("SKILL.md"),
            root.path().join("SKILL.md"),
        )
        .unwrap();
        assert_eq!(
            scan_update_candidates(&request(root.path()), || false),
            Err(SourceInventoryError::InvalidRequest)
        );
    }
}

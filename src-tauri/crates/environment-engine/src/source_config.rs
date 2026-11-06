//! 来源声明的容器路径及有界文档采集，供 Native 与 Worker 共用。
use std::collections::BTreeMap;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use crate::source_inventory::SourceInventoryError;

pub const CONFIG_FILE: &str = "skills.sh.json";
const CONFIG_LIMIT: u64 = 64 * 1024;
const ENTRY_LIMIT: usize = 100_000;

pub fn parse_paths(document: &str) -> Vec<String> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(document) else {
        return Vec::new();
    };
    let Some(paths) = value.get("paths").and_then(serde_json::Value::as_array) else {
        return Vec::new();
    };
    let mut result = Vec::new();
    for value in paths {
        let Some(path) = value
            .as_str()
            .filter(|path| path.starts_with("./") && !path.contains(['\\', '\0']))
        else {
            continue;
        };
        let mut parts = Vec::new();
        let mut invalid = false;
        for part in path.split('/') {
            match part {
                "" | "." => {}
                ".." => {
                    if parts.pop().is_none() {
                        invalid = true;
                        break;
                    }
                }
                _ => parts.push(part),
            }
        }
        if invalid || parts.is_empty() || parts[0].ends_with(':') {
            continue;
        }
        let normalized = parts.join("/");
        if !result.contains(&normalized) {
            result.push(normalized);
        }
    }
    result
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

pub fn read_document(root: &Path) -> std::io::Result<Option<String>> {
    let path = root.join(CONFIG_FILE);
    let metadata = match fs::symlink_metadata(&path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        value => value?,
    };
    if !metadata.is_file() || is_link(&metadata) {
        return Err(std::io::ErrorKind::InvalidData.into());
    }
    let mut bytes = Vec::new();
    fs::File::open(path)?
        .take(CONFIG_LIMIT + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > CONFIG_LIMIT {
        return Err(std::io::ErrorKind::InvalidData.into());
    }
    // 无效配置按未声明处理；读取失败和超限仍是可见错误。
    Ok(String::from_utf8(bytes).ok())
}

pub fn collect_documents(
    root: &Path,
    document: &str,
    per_file_limit: u32,
    aggregate_limit: u32,
    is_cancelled: impl Fn() -> bool,
) -> Result<BTreeMap<PathBuf, Vec<u8>>, SourceInventoryError> {
    let invalid = |_| SourceInventoryError::InvalidRequest;
    let physical_root = fs::canonicalize(root).map_err(invalid)?;
    let root = physical_root.as_path();
    let mut documents = BTreeMap::new();
    let mut total = 0usize;
    let mut visited = 0usize;
    for container in parse_paths(document) {
        let container = PathBuf::from(container);
        match crate::directory::plain_descendant_directory(root, &container) {
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(_) => return Err(SourceInventoryError::InvalidRequest),
        }
        let mut pending = vec![(container, 0usize)];
        while let Some((relative, depth)) = pending.pop() {
            if is_cancelled() {
                return Err(SourceInventoryError::Cancelled);
            }
            let directory =
                crate::directory::plain_descendant_directory(root, &relative).map_err(invalid)?;
            for child in fs::read_dir(directory).map_err(invalid)? {
                if is_cancelled() {
                    return Err(SourceInventoryError::Cancelled);
                }
                visited += 1;
                if visited > ENTRY_LIMIT {
                    return Err(SourceInventoryError::InvalidRequest);
                }
                let child = child.map_err(invalid)?;
                let name = child.file_name();
                let path = relative.join(&name);
                let metadata = fs::symlink_metadata(child.path()).map_err(invalid)?;
                if is_link(&metadata) {
                    continue;
                }
                if metadata.is_dir() {
                    if depth < 3
                        && !matches!(
                            name.to_str(),
                            Some(
                                ".git"
                                    | "node_modules"
                                    | "dist"
                                    | "build"
                                    | "__pycache__"
                                    | "__pypackages__"
                            )
                        )
                    {
                        pending.push((path, depth + 1));
                    }
                } else if depth > 0
                    && metadata.is_file()
                    && name
                        .to_str()
                        .is_some_and(|name| name.eq_ignore_ascii_case("SKILL.md"))
                    && !documents.contains_key(&path)
                {
                    let allowance = (per_file_limit as usize)
                        .min((aggregate_limit as usize).saturating_sub(total));
                    let mut bytes = Vec::new();
                    fs::File::open(child.path())
                        .map_err(invalid)?
                        .take(allowance as u64 + 1)
                        .read_to_end(&mut bytes)
                        .map_err(invalid)?;
                    if bytes.len() > allowance {
                        return Err(SourceInventoryError::InvalidRequest);
                    }
                    total += bytes.len();
                    documents.insert(path, bytes);
                }
            }
        }
    }
    Ok(documents)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    #[test]
    fn declared_containers_and_configuration_cannot_follow_external_links() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(outside.path(), root.path().join("linked")).unwrap();
        assert_eq!(
            collect_documents(root.path(), r#"{"paths":["./linked"]}"#, 1024, 4096, || {
                false
            }),
            Err(SourceInventoryError::InvalidRequest)
        );
        fs::write(outside.path().join("config.json"), r#"{"paths":[]}"#).unwrap();
        std::os::unix::fs::symlink(
            outside.path().join("config.json"),
            root.path().join(CONFIG_FILE),
        )
        .unwrap();
        assert!(read_document(root.path()).is_err());
    }
    #[test]
    fn declared_paths_are_posix_relative_containers() {
        assert_eq!(
            parse_paths(
                r#"{"paths":["./tools/../skills/","./skills","../escape","/absolute","./a/../../escape","./C:/escape","./x\\y","./",false]}"#
            ),
            ["skills"]
        );
        for invalid in ["not json", "[]", r#"{"paths":"./skills"}"#] {
            assert!(parse_paths(invalid).is_empty());
        }
    }
    #[test]
    fn configuration_limits_and_collection_limits_are_enforced() {
        let root = tempfile::tempdir().unwrap();
        fs::write(
            root.path().join(CONFIG_FILE),
            vec![b' '; CONFIG_LIMIT as usize + 1],
        )
        .unwrap();
        assert!(read_document(root.path()).is_err());
        fs::create_dir_all(root.path().join("tools/review")).unwrap();
        fs::write(root.path().join("tools/review/SKILL.md"), b"too long").unwrap();
        assert_eq!(
            collect_documents(root.path(), r#"{"paths":["./tools"]}"#, 3, 3, || false),
            Err(SourceInventoryError::InvalidRequest)
        );
        assert_eq!(
            collect_documents(root.path(), r#"{"paths":["./tools"]}"#, 100, 100, || true),
            Err(SourceInventoryError::Cancelled)
        );
    }
}

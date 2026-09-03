use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use crate::environment::wsl::WslWorkspace;
use crate::error::AppError;

pub const MAX_OBSERVED_SKILL_ENTRIES: u32 = 10_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectoryInspection {
    pub observed_skill_count: Option<u32>,
    pub observed_skill_count_truncated: bool,
}

pub async fn inspect_native(paths: &[String]) -> BTreeMap<String, DirectoryInspection> {
    unique_paths(paths)
        .into_iter()
        .map(|path| {
            let inspection = inspect_native_path(&path);
            (path, inspection)
        })
        .collect()
}

pub async fn inspect_wsl(
    workspace: &WslWorkspace,
    paths: &[String],
) -> Result<BTreeMap<String, DirectoryInspection>, AppError> {
    let paths = unique_paths(paths);
    if paths.is_empty() {
        return Ok(BTreeMap::new());
    }
    Ok(workspace
        .count_directory_entries(paths.clone(), MAX_OBSERVED_SKILL_ENTRIES)
        .await?
        .into_iter()
        .map(|fact| {
            (
                fact.path,
                DirectoryInspection {
                    observed_skill_count: fact.observed_count,
                    observed_skill_count_truncated: fact.truncated,
                },
            )
        })
        .collect())
}

fn unique_paths(paths: &[String]) -> Vec<String> {
    paths
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn bounded_directory_count(
    entries: impl Iterator<Item = std::io::Result<()>>,
) -> Option<(u32, bool)> {
    let mut entry_count = 0;
    for entry in entries.take((MAX_OBSERVED_SKILL_ENTRIES + 1) as usize) {
        entry.ok()?;
        entry_count += 1;
    }
    Some((
        entry_count.min(MAX_OBSERVED_SKILL_ENTRIES),
        entry_count > MAX_OBSERVED_SKILL_ENTRIES,
    ))
}

fn inspect_native_path(path: &str) -> DirectoryInspection {
    let Ok(entries) = fs::read_dir(Path::new(path)) else {
        return DirectoryInspection {
            observed_skill_count: None,
            observed_skill_count_truncated: false,
        };
    };
    let Some((observed_skill_count, observed_skill_count_truncated)) =
        bounded_directory_count(entries.map(|entry| entry.map(|_| ())))
    else {
        return DirectoryInspection {
            observed_skill_count: None,
            observed_skill_count_truncated: false,
        };
    };
    DirectoryInspection {
        observed_skill_count: Some(observed_skill_count),
        observed_skill_count_truncated,
    }
}

#[cfg(test)]
mod tests {
    use super::{bounded_directory_count, inspect_native};

    #[tokio::test]
    async fn native_inspection_deduplicates_paths_and_caps_observed_entries() {
        let temp = tempfile::tempdir().expect("tempdir");
        let skills = temp.path().join("skills");
        std::fs::create_dir_all(&skills).expect("create skills directory");
        for index in 0..10_001 {
            std::fs::write(skills.join(format!("skill-{index}")), "skill")
                .expect("write skill entry");
        }
        let missing = temp.path().join("missing").to_string_lossy().to_string();
        let path = skills.to_string_lossy().to_string();

        let inspected = inspect_native(&[path.clone(), path.clone(), missing.clone()]).await;

        assert_eq!(inspected.len(), 2);
        assert_eq!(inspected[&path].observed_skill_count, Some(10_000));
        assert!(inspected[&path].observed_skill_count_truncated);
        assert_eq!(inspected[&missing].observed_skill_count, None);
    }

    #[test]
    fn native_bounded_count_reports_an_entry_error_as_unavailable() {
        let entries = vec![
            Ok(()),
            Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "entry cannot be read",
            )),
        ];

        assert_eq!(bounded_directory_count(entries.into_iter()), None);
    }
}

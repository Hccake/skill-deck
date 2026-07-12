use std::fs;
use std::path::Path;

use crate::environment::service::{EnvironmentSnapshot, InspectRequest, SkillEntrySnapshot};
use crate::error::AppError;

pub fn inspect_host_context(request: &InspectRequest) -> Result<EnvironmentSnapshot, AppError> {
    let root = Path::new(&request.context.skill_root.native_path);
    if !root.is_dir() {
        return Ok(EnvironmentSnapshot { skills: Vec::new() });
    }
    let mut skills = fs::read_dir(root)?
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let path = entry.path();
            if !path.is_dir() || !path.join("SKILL.md").is_file() {
                return None;
            }
            Some(SkillEntrySnapshot {
                name: entry.file_name().to_string_lossy().to_string(),
                canonical_path: path.to_string_lossy().to_string(),
            })
        })
        .collect::<Vec<_>>();
    skills.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(EnvironmentSnapshot { skills })
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::inspect_host_context;
    use crate::environment::service::{InspectRequest, ResolvedContext};
    use crate::environment::types::{ContextRef, ContextScope, EnvironmentRef, ResourceLocator};

    #[test]
    fn host_inspect_lists_only_direct_skill_directories_with_skill_md() {
        let temp = tempdir().expect("tempdir");
        let skill_root = temp.path().join("skills");
        fs::create_dir_all(skill_root.join("toolkit")).expect("create skill");
        fs::write(skill_root.join("toolkit/SKILL.md"), "# Toolkit").expect("write skill");
        fs::create_dir_all(skill_root.join("not-a-skill")).expect("create other dir");

        let snapshot = inspect_host_context(&InspectRequest {
            context: ResolvedContext {
                context: ContextRef {
                    environment: EnvironmentRef::Host,
                    scope: ContextScope::Global,
                },
                project: None,
                home: ResourceLocator {
                    environment: EnvironmentRef::Host,
                    native_path: temp.path().to_string_lossy().to_string(),
                },
                skill_root: ResourceLocator {
                    environment: EnvironmentRef::Host,
                    native_path: skill_root.to_string_lossy().to_string(),
                },
                lock: ResourceLocator {
                    environment: EnvironmentRef::Host,
                    native_path: temp.path().join("lock.json").to_string_lossy().to_string(),
                },
            },
        })
        .expect("inspect host");

        assert_eq!(snapshot.skills.len(), 1);
        assert_eq!(snapshot.skills[0].name, "toolkit");
    }
}

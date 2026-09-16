use std::{fs, io::Read, path::Path};

#[cfg(target_os = "linux")]
use environment_engine::inspection::{
    self as engine_inspection, EntryKind as EngineEntryKind, ErrorCode as EngineErrorCode,
    InspectionRequest, InspectionRoot,
};

use crate::environment::inspection::{
    FilesystemEntryKind, FilesystemInspector, InspectionFuture, MetadataFuture,
    RawFilesystemSnapshot, RawPathFact, RawSkillMetadata, ReadPlan, ReadRootPurpose,
    SkillMetadataSource,
};
#[cfg(not(target_os = "linux"))]
use crate::environment::native::tree::{inspect_entry_no_follow, NativeEntryKind};
use crate::environment::types::EnvironmentRef;
use crate::error::AppError;

pub struct NativeInspector {
    environment: EnvironmentRef,
}

impl NativeInspector {
    pub fn new(environment: EnvironmentRef) -> Self {
        Self { environment }
    }
}

impl FilesystemInspector for NativeInspector {
    fn environment(&self) -> EnvironmentRef {
        self.environment.clone()
    }

    fn inspect<'a>(
        &'a self,
        plan: &'a ReadPlan,
    ) -> InspectionFuture<'a, Result<RawFilesystemSnapshot, AppError>> {
        let environment = self.environment.clone();
        let plan = plan.clone();
        Box::pin(async move {
            if environment != EnvironmentRef::Native
                || plan.context.environment != EnvironmentRef::Native
            {
                return Err(AppError::StorageUnsupported {
                    path: "nativeInspector".to_string(),
                });
            }
            tokio::task::spawn_blocking(move || inspect_native(&plan))
                .await
                .map_err(|error| AppError::ExecutionFailed {
                    message: format!("native inspection task failed: {error}"),
                })?
        })
    }
}

impl SkillMetadataSource for NativeInspector {
    fn read<'a>(
        &'a self,
        locators: &'a [crate::environment::types::ResourceLocator],
        per_file_limit: u32,
    ) -> MetadataFuture<'a, Result<Vec<RawSkillMetadata>, AppError>> {
        let environment = self.environment.clone();
        let locators = locators.to_vec();
        Box::pin(async move {
            if environment != EnvironmentRef::Native
                || per_file_limit == 0
                || locators
                    .iter()
                    .any(|locator| locator.environment != EnvironmentRef::Native)
            {
                return Err(AppError::StorageUnsupported {
                    path: "nativeSkillMetadata".to_string(),
                });
            }
            tokio::task::spawn_blocking(move || read_native_metadata(&locators, per_file_limit))
                .await
                .map_err(|error| AppError::ExecutionFailed {
                    message: format!("native metadata task failed: {error}"),
                })?
        })
    }
}

fn read_native_metadata(
    locators: &[crate::environment::types::ResourceLocator],
    per_file_limit: u32,
) -> Result<Vec<RawSkillMetadata>, AppError> {
    Ok(locators
        .iter()
        .map(|locator| {
            let path = Path::new(&locator.native_path);
            let mut bytes = Vec::new();
            let mut truncated = false;
            let mut error_code = None;
            match fs::File::open(path) {
                Ok(file) => {
                    if file
                        .take(per_file_limit as u64)
                        .read_to_end(&mut bytes)
                        .is_err()
                    {
                        bytes.clear();
                        error_code = Some("readFailed".to_string());
                    } else {
                        truncated = fs::metadata(path)
                            .map(|metadata| metadata.len() > bytes.len() as u64)
                            .unwrap_or(false);
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    error_code = Some("missing".to_string());
                }
                Err(_) => error_code = Some("readFailed".to_string()),
            }
            RawSkillMetadata {
                locator: locator.clone(),
                bytes,
                truncated,
                error_code,
            }
        })
        .collect())
}

#[cfg(target_os = "linux")]
fn inspect_native(plan: &ReadPlan) -> Result<RawFilesystemSnapshot, AppError> {
    let snapshot = engine_inspection::inspect(&InspectionRequest {
        read_content: false,
        roots: plan
            .roots
            .iter()
            .map(|root| InspectionRoot {
                path: root.locator.native_path.clone().into(),
                stat_only: root.purposes.len() == 1
                    && root.purposes.contains(&ReadRootPurpose::Context),
            })
            .collect(),
        per_file_limit: plan.per_file_limit,
        aggregate_limit: plan.aggregate_limit,
    })
    .map_err(|error| AppError::ExecutionFailed {
        message: format!("native inspection failed: {error}"),
    })?;

    Ok(RawFilesystemSnapshot {
        environment: EnvironmentRef::Native,
        facts: snapshot
            .facts
            .into_iter()
            .map(|fact| RawPathFact {
                root_index: fact.root_index,
                relative_path: fact.relative_path.to_string_lossy().into_owned(),
                kind: match fact.kind {
                    EngineEntryKind::Missing => FilesystemEntryKind::Missing,
                    EngineEntryKind::File => FilesystemEntryKind::File,
                    EngineEntryKind::Directory => FilesystemEntryKind::Directory,
                    EngineEntryKind::Symlink => FilesystemEntryKind::Symlink,
                    EngineEntryKind::Other => FilesystemEntryKind::Other,
                },
                resolved_target: fact
                    .resolved_target
                    .map(|target| target.to_string_lossy().into_owned()),
                fingerprint: fact
                    .fingerprint
                    .map(crate::environment::runtime::EntryFingerprint),
                frontmatter_bytes: fact.content_bytes,
                truncated: fact.truncated,
                error_code: fact.error_code.map(|code| match code {
                    EngineErrorCode::PathUnavailable => "pathUnavailable".to_string(),
                    EngineErrorCode::ReadFailed => "readFailed".to_string(),
                    EngineErrorCode::ReadLinkFailed => "readLinkFailed".to_string(),
                }),
            })
            .collect(),
        total_content_bytes: snapshot.total_content_bytes,
    })
}

#[cfg(not(target_os = "linux"))]
fn inspect_native(plan: &ReadPlan) -> Result<RawFilesystemSnapshot, AppError> {
    let mut facts = Vec::new();
    let mut total = 0usize;
    for (root_index, root) in plan.roots.iter().enumerate() {
        let path = Path::new(&root.locator.native_path);
        facts.push(inspect_path(
            path,
            root_index as u32,
            String::new(),
            false,
            plan,
            &mut total,
        ));
        if root.purposes.len() == 1 && root.purposes.contains(&ReadRootPurpose::Context) {
            continue;
        }
        if !matches!(inspect_entry_no_follow(path), Ok(entry) if entry.kind == NativeEntryKind::Directory)
        {
            continue;
        }
        let entries = match fs::read_dir(path) {
            Ok(entries) => entries,
            Err(_) => {
                if let Some(root_fact) = facts.last_mut() {
                    root_fact.error_code = Some("pathUnavailable".to_string());
                }
                continue;
            }
        };
        let mut children = entries.filter_map(Result::ok).collect::<Vec<_>>();
        children.sort_by_key(fs::DirEntry::file_name);
        for child in children {
            let relative = child.file_name().to_string_lossy().into_owned();
            let child_path = child.path();
            let child_kind = inspect_entry_no_follow(&child_path)
                .ok()
                .map(|entry| entry.kind);
            facts.push(inspect_path(
                &child_path,
                root_index as u32,
                relative.clone(),
                false,
                plan,
                &mut total,
            ));
            if matches!(
                child_kind,
                Some(
                    NativeEntryKind::Directory
                        | NativeEntryKind::Symlink
                        | NativeEntryKind::ReparsePoint
                )
            ) {
                let skill = child_path.join("SKILL.md");
                if fs::symlink_metadata(&skill).is_ok() {
                    facts.push(inspect_path(
                        &skill,
                        root_index as u32,
                        format!("{relative}/SKILL.md"),
                        false,
                        plan,
                        &mut total,
                    ));
                }
            }
        }
    }
    Ok(RawFilesystemSnapshot {
        environment: EnvironmentRef::Native,
        facts,
        total_content_bytes: total as u32,
    })
}

#[cfg(not(target_os = "linux"))]
fn inspect_path(
    path: &Path,
    root_index: u32,
    relative_path: String,
    read_content: bool,
    plan: &ReadPlan,
    total: &mut usize,
) -> RawPathFact {
    let inspected = match inspect_entry_no_follow(path) {
        Ok(inspected) => inspected,
        Err(_) => {
            return RawPathFact {
                root_index,
                relative_path,
                kind: FilesystemEntryKind::Other,
                resolved_target: None,
                fingerprint: None,
                frontmatter_bytes: Vec::new(),
                truncated: false,
                error_code: Some("pathUnavailable".to_string()),
            };
        }
    };
    let kind = match inspected.kind {
        NativeEntryKind::Missing => FilesystemEntryKind::Missing,
        NativeEntryKind::File => FilesystemEntryKind::File,
        NativeEntryKind::Directory => FilesystemEntryKind::Directory,
        NativeEntryKind::Symlink => FilesystemEntryKind::Symlink,
        NativeEntryKind::ReparsePoint => FilesystemEntryKind::ReparsePoint,
        NativeEntryKind::Other => FilesystemEntryKind::Other,
    };
    let mut content = Vec::new();
    let mut truncated = false;
    let mut error_code = None;
    if kind == FilesystemEntryKind::File
        && read_content
        && (relative_path == "SKILL.md" || relative_path.ends_with("/SKILL.md"))
    {
        let remaining = (plan.aggregate_limit as usize).saturating_sub(*total);
        let limit = remaining.min(plan.per_file_limit as usize);
        match fs::File::open(path) {
            Ok(file) => {
                if file.take(limit as u64).read_to_end(&mut content).is_err() {
                    content.clear();
                    error_code = Some("readFailed".to_string());
                } else {
                    truncated = fs::metadata(path)
                        .map(|metadata| metadata.len() > content.len() as u64)
                        .unwrap_or(false);
                    *total += content.len();
                }
            }
            Err(_) => error_code = Some("readFailed".to_string()),
        }
    }
    RawPathFact {
        root_index,
        relative_path,
        kind,
        resolved_target: inspected
            .link_target
            .map(|target| target.to_string_lossy().into_owned()),
        fingerprint: Some(inspected.fingerprint),
        frontmatter_bytes: content,
        truncated,
        error_code,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::environment::inspection::{FilesystemInspector, ReadPlanBuilder, ReadRootPurpose};
    use crate::environment::runtime::ContextSnapshotRevision;
    use crate::environment::types::{ResourceLocator, SkillLocation, SkillLocationRef};

    #[tokio::test]
    async fn context_root_is_stat_only_and_does_not_consume_skill_content_budget() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join("unrelated")).unwrap();
        std::fs::write(
            temp.path().join("unrelated/SKILL.md"),
            b"---\nname: unrelated\ndescription: Unrelated\n---\n",
        )
        .unwrap();
        let context = SkillLocationRef {
            environment: EnvironmentRef::Native,
            scope: SkillLocation::Global,
        };
        let mut builder = ReadPlanBuilder::new(
            context,
            "registry-v1",
            "environment-v1",
            ContextSnapshotRevision::parse("context-v1-stat-only").unwrap(),
        );
        builder
            .add_root(
                ResourceLocator {
                    environment: EnvironmentRef::Native,
                    native_path: temp.path().to_string_lossy().into_owned(),
                },
                ReadRootPurpose::Context,
                None,
            )
            .unwrap();

        let plan = builder.build().unwrap();
        let inspector = NativeInspector::new(EnvironmentRef::Native);
        let snapshot = inspector.inspect(&plan).await.unwrap();

        assert_eq!(snapshot.facts.len(), 1);
        assert_eq!(snapshot.total_content_bytes, 0);
        assert_eq!(snapshot.facts[0].kind, FilesystemEntryKind::Directory);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn reads_skill_document_through_a_direct_child_directory_symlink() {
        use std::os::unix::fs::symlink;

        let temp = tempfile::tempdir().unwrap();
        let canonical = temp.path().join("canonical/toolkit");
        let agent_root = temp.path().join("agent-skills");
        std::fs::create_dir_all(&canonical).unwrap();
        std::fs::create_dir_all(&agent_root).unwrap();
        let document = b"---\nname: toolkit\ndescription: Toolkit\n---\n";
        std::fs::write(canonical.join("SKILL.md"), document).unwrap();
        symlink(&canonical, agent_root.join("toolkit")).unwrap();

        let context = SkillLocationRef {
            environment: EnvironmentRef::Native,
            scope: SkillLocation::Global,
        };
        let mut builder = ReadPlanBuilder::new(
            context,
            "registry-v1",
            "environment-v1",
            ContextSnapshotRevision::parse("context-v1-directory-link").unwrap(),
        );
        builder
            .add_root(
                ResourceLocator {
                    environment: EnvironmentRef::Native,
                    native_path: agent_root.to_string_lossy().into_owned(),
                },
                ReadRootPurpose::Private,
                None,
            )
            .unwrap();

        let plan = builder.build().unwrap();
        let inspector = NativeInspector::new(EnvironmentRef::Native);
        let snapshot = inspector.inspect(&plan).await.unwrap();

        let linked_directory = snapshot
            .facts
            .iter()
            .find(|fact| fact.relative_path == "toolkit")
            .expect("linked Skill directory");
        assert_eq!(linked_directory.kind, FilesystemEntryKind::Symlink);
        let skill_document = snapshot
            .facts
            .iter()
            .find(|fact| fact.relative_path == "toolkit/SKILL.md")
            .expect("Skill document through directory link");
        assert_eq!(skill_document.kind, FilesystemEntryKind::File);
        assert!(skill_document.frontmatter_bytes.is_empty());
        assert!(skill_document.fingerprint.is_some());

        let locator = ResourceLocator {
            environment: EnvironmentRef::Native,
            native_path: agent_root
                .join("toolkit/SKILL.md")
                .to_string_lossy()
                .into_owned(),
        };
        let metadata = inspector
            .read(std::slice::from_ref(&locator), plan.per_file_limit)
            .await
            .unwrap();
        assert_eq!(metadata[0].locator, locator);
        assert_eq!(metadata[0].bytes, document);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn broken_child_directory_symlink_does_not_produce_a_skill_document() {
        use std::os::unix::fs::symlink;

        let temp = tempfile::tempdir().unwrap();
        let agent_root = temp.path().join("agent-skills");
        std::fs::create_dir_all(&agent_root).unwrap();
        symlink(temp.path().join("missing"), agent_root.join("toolkit")).unwrap();
        let context = SkillLocationRef {
            environment: EnvironmentRef::Native,
            scope: SkillLocation::Global,
        };
        let mut builder = ReadPlanBuilder::new(
            context,
            "registry-v1",
            "environment-v1",
            ContextSnapshotRevision::parse("context-v1-broken-directory-link").unwrap(),
        );
        builder
            .add_root(
                ResourceLocator {
                    environment: EnvironmentRef::Native,
                    native_path: agent_root.to_string_lossy().into_owned(),
                },
                ReadRootPurpose::Private,
                None,
            )
            .unwrap();

        let snapshot = NativeInspector::new(EnvironmentRef::Native)
            .inspect(&builder.build().unwrap())
            .await
            .unwrap();

        assert!(snapshot.facts.iter().any(|fact| {
            fact.relative_path == "toolkit" && fact.kind == FilesystemEntryKind::Symlink
        }));
        assert!(!snapshot
            .facts
            .iter()
            .any(|fact| fact.relative_path == "toolkit/SKILL.md"));
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn reads_skill_document_through_a_direct_child_junction() {
        let temp = tempfile::tempdir().unwrap();
        let canonical = temp.path().join("canonical/toolkit");
        let agent_root = temp.path().join("agent-skills");
        std::fs::create_dir_all(&canonical).unwrap();
        std::fs::create_dir_all(&agent_root).unwrap();
        let document = b"---\nname: toolkit\ndescription: Toolkit\n---\n";
        std::fs::write(canonical.join("SKILL.md"), document).unwrap();
        junction::create(&canonical, agent_root.join("toolkit")).unwrap();
        let context = SkillLocationRef {
            environment: EnvironmentRef::Native,
            scope: SkillLocation::Global,
        };
        let mut builder = ReadPlanBuilder::new(
            context,
            "registry-v1",
            "environment-v1",
            ContextSnapshotRevision::parse("context-v1-directory-junction").unwrap(),
        );
        builder
            .add_root(
                ResourceLocator {
                    environment: EnvironmentRef::Native,
                    native_path: agent_root.to_string_lossy().into_owned(),
                },
                ReadRootPurpose::Private,
                None,
            )
            .unwrap();

        let snapshot = NativeInspector::new(EnvironmentRef::Native)
            .inspect(&builder.build().unwrap())
            .await
            .unwrap();

        assert!(snapshot.facts.iter().any(|fact| {
            fact.relative_path == "toolkit"
                && matches!(
                    fact.kind,
                    FilesystemEntryKind::Symlink | FilesystemEntryKind::ReparsePoint
                )
        }));
        let skill_document = snapshot
            .facts
            .iter()
            .find(|fact| fact.relative_path == "toolkit/SKILL.md")
            .expect("Skill document through junction");
        assert_eq!(skill_document.frontmatter_bytes, document);
    }
}

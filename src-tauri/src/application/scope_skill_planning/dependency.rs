use crate::application::installed_skill_resolver::InstalledSkillResolver;
use crate::application::planning_facts::ScopePlanningSnapshot;
use crate::environment::planning::{TargetEntryKind, TargetFactResolver};
use crate::environment::types::{EnvironmentRef, ResourceLocator};
use crate::error::AppError;

/// 按已安装记录定位依赖包。父级 node_modules 可以提供提升后的依赖，包目录可以是 workspace 链接。
pub(super) async fn package_root<T: TargetFactResolver + ?Sized>(
    skill_name: &str,
    facts: &ScopePlanningSnapshot,
    targets: &T,
) -> Result<Option<ResourceLocator>, AppError> {
    let Some(project) = &facts.resolved_context.project else {
        return Ok(None);
    };
    let installed = InstalledSkillResolver::resolve(skill_name, &facts.lock_document)?;
    let record = facts.lock_document.entry_snapshot(&installed.lock_key);
    let Some(record) = record
        .value()
        .filter(|record| record["sourceType"] == "node_modules")
    else {
        return Ok(None);
    };
    let Some(package) = record["source"]
        .as_str()
        .filter(|source| valid_package_name(source))
    else {
        return Ok(None);
    };
    let context = &facts.resolved_context.context;
    let project = ResourceLocator {
        environment: context.environment.clone(),
        native_path: project.native_path.clone(),
    };
    // 投影子文件时会解析全部父级链接，得到项目的物理位置；不要求 package.json 存在。
    let project_file = resolve_one(targets, context, &project.join_child("package.json")).await?;
    let mut ancestor = parent(&project_file.destination);
    while let Some(root) = ancestor {
        let package_path = package
            .split('/')
            .fold(root.join_child("node_modules"), |root, part| {
                root.join_child(part)
            });
        let package_fact = resolve_one(targets, context, &package_path).await?;
        if matches!(
            package_fact.entry_kind,
            TargetEntryKind::Directory | TargetEntryKind::Symlink | TargetEntryKind::Junction
        ) {
            let file =
                resolve_one(targets, context, &package_path.join_child("package.json")).await?;
            return Ok(parent(&file.destination));
        }
        ancestor = parent(&root);
    }
    Ok(None)
}

async fn resolve_one<T: TargetFactResolver + ?Sized>(
    targets: &T,
    context: &crate::environment::types::SkillLocationRef,
    path: &ResourceLocator,
) -> Result<crate::environment::planning::ResolvedTargetFact, AppError> {
    let mut resolved = targets
        .resolve(context, std::slice::from_ref(path), None)
        .await?;
    if resolved.len() != 1 {
        return Err(AppError::StaleTarget);
    }
    Ok(resolved.remove(0))
}

fn parent(path: &ResourceLocator) -> Option<ResourceLocator> {
    let native_path = match &path.environment {
        EnvironmentRef::Native => std::path::Path::new(&path.native_path)
            .parent()?
            .to_string_lossy()
            .into_owned(),
        EnvironmentRef::Wsl { .. } => {
            let (parent, _) = path.native_path.trim_end_matches('/').rsplit_once('/')?;
            if parent.is_empty() {
                "/".into()
            } else {
                parent.into()
            }
        }
    };
    Some(ResourceLocator {
        environment: path.environment.clone(),
        native_path,
    })
}

fn valid_package_name(source: &str) -> bool {
    let parts = source.split('/').collect::<Vec<_>>();
    let valid_part = |part: &str| {
        !part.is_empty()
            && part != "."
            && part != ".."
            && part
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"-._~".contains(&c))
    };
    match parts.as_slice() {
        [name] => valid_part(name),
        [scope, name] => scope.strip_prefix('@').is_some_and(valid_part) && valid_part(name),
        _ => false,
    }
}

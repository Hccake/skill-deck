//! 删除核心模块
//!
//! 完全复刻 CLI remove.ts 的删除逻辑：
//! 1. 遍历 target agents，删除各 agent 目录下的 skill（对应 CLI remove.ts:152-168）
//! 2. 删除 canonical 目录（对应 CLI remove.ts:170-171）
//! 3. 更新 lock file（仅 Global scope）（对应 CLI remove.ts:173-178）
//!
//! 与 CLI 的差异：
//! - agent 检测 fallback：CLI 用 `Object.keys(agents)` 全部 agents，Rust 用 `AgentType::all()` 枚举迭代（等价）
//! - 路径安全检查：CLI 有独立 `isPathSafe()` 函数，Rust 的 `sanitize_name()` 已移除路径穿越字符，无需二次检查
//! - 错误收集：CLI 用 `results` 数组收集批量结果，GUI 是单个删除返回 `RemoveResult`

use crate::core::agents::AgentType;
use crate::core::paths::canonical_skills_dir;
use crate::core::skill::sanitize_name;
use crate::core::local_lock::remove_skill_from_local_lock;
use crate::core::skill_lock::{get_skill_from_lock, remove_skill_from_lock};
use crate::error::AppError;
use crate::models::{RemoveResult, Scope};
use std::fs;
use std::path::PathBuf;

/// 删除 skill
///
/// 对应 CLI: remove.ts 第 150-179 行的核心循环
/// GUI 增强：支持 partial removal（仅删除指定 agents 的 symlink，不删 canonical 和 lock）
///
/// # Arguments
/// * `skill_name` - 要删除的 skill 名称
/// * `scope` - 删除范围（Global/Project）
/// * `project_path` - Project scope 时的项目路径
/// * `full_removal` - 是否完全删除（true = 删除一切，false = 仅删除指定 agents 的 symlink）
/// * `target_agents` - 部分移除时指定的 agent 列表（None = 自动检测）
///
/// # Returns
/// * `RemoveResult` - 删除结果
pub fn remove_skill(
    skill_name: &str,
    scope: &Scope,
    project_path: Option<&str>,
    full_removal: bool,
    target_agents: Option<&[AgentType]>,
) -> Result<RemoveResult, AppError> {
    let is_global = matches!(scope, Scope::Global);
    let cwd = project_path.unwrap_or(".");
    let sanitized_name = sanitize_name(skill_name);

    // 1. 确定要操作的 agents
    let agents_to_remove: Vec<AgentType> = if let Some(specified) = target_agents {
        specified.to_vec()
    } else {
        // 自动检测（向后兼容原始行为）
        let detected = AgentType::detect_installed();
        if detected.is_empty() {
            AgentType::all().collect::<Vec<_>>()
        } else {
            detected
        }
    };

    let mut removed_paths = Vec::new();

    // 2. 遍历 agents 删除 skill 目录
    // 对应 CLI: remove.ts:152-168
    for agent in &agents_to_remove {
        let config = agent.config();

        // 计算 agent 目录下的 skill 路径
        // 对应 CLI: installer.ts:367-389 getInstallPath()
        let skill_path = if is_global {
            match &config.global_skills_dir {
                Some(global_dir) => global_dir.join(&sanitized_name),
                // agent 不支持 global 安装，跳过
                None => continue,
            }
        } else {
            PathBuf::from(cwd)
                .join(&config.skills_dir)
                .join(&sanitized_name)
        };

        // 删除 agent 目录下的 skill（可能是 symlink 或实体目录）
        // 对应 CLI: remove.ts:156-167
        // Rust 优化：使用 symlink_metadata() 判断 symlink 存在性（不 follow），
        // 比 CLI 的 lstat().catch(() => null) 更明确语义
        if let Err(e) = remove_path(&skill_path) {
            // 对应 CLI: remove.ts:162-166
            // 单个 agent 删除失败不影响整体流程，仅 warn
            log::warn!(
                "Could not remove skill from {}: {}",
                config.display_name,
                e
            );
        } else if skill_path.exists() || skill_path.symlink_metadata().is_ok() {
            // 路径存在但删除后仍然存在，说明删除失败
        } else {
            removed_paths.push(skill_path.to_string_lossy().to_string());
        }
    }

    // 3. 完全删除模式：清理 canonical 目录 + lock file
    let (source, source_type) = if full_removal {
        // 删除 canonical 目录（带共享保护）
        let canonical_path = canonical_skills_dir(is_global, cwd).join(&sanitized_name);
        let should_remove_canonical = if is_global {
            let still_used = AgentType::all().any(|agent| {
                if agents_to_remove.contains(&agent) {
                    return false;
                }
                let config = agent.config();
                if let Some(global_dir) = &config.global_skills_dir {
                    let agent_skill_path = global_dir.join(&sanitized_name);
                    agent_skill_path.symlink_metadata().is_ok()
                } else {
                    false
                }
            });
            !still_used
        } else {
            let still_used = AgentType::all().any(|agent| {
                if agents_to_remove.contains(&agent) {
                    return false;
                }
                let config = agent.config();
                let agent_skill_path = PathBuf::from(cwd)
                    .join(&config.skills_dir)
                    .join(&sanitized_name);
                agent_skill_path.symlink_metadata().is_ok()
            });
            !still_used
        };

        if should_remove_canonical {
            let _ = remove_path(&canonical_path);
        }

        // 更新 lock file
        if is_global {
            let lock_entry = get_skill_from_lock(skill_name).ok().flatten();
            let effective_source = lock_entry
                .as_ref()
                .map(|e| e.source.clone())
                .unwrap_or_else(|| "local".to_string());
            let effective_source_type = lock_entry
                .as_ref()
                .map(|e| e.source_type.clone())
                .unwrap_or_else(|| "local".to_string());
            let _ = remove_skill_from_lock(skill_name);
            (Some(effective_source), Some(effective_source_type))
        } else {
            if let Some(project_dir) = project_path {
                let local_lock = crate::core::local_lock::read_local_lock(project_dir).ok();
                let lock_entry = local_lock.and_then(|l| l.skills.get(skill_name).cloned());
                let effective_source = lock_entry.as_ref().map(|e| e.source.clone());
                let effective_source_type = lock_entry.as_ref().map(|e| e.source_type.clone());
                let _ = remove_skill_from_local_lock(skill_name, project_dir);
                (effective_source, effective_source_type)
            } else {
                (None, None)
            }
        }
    } else {
        // 部分移除：不删 canonical、不更新 lock
        (None, None)
    };

    Ok(RemoveResult {
        skill_name: skill_name.to_string(),
        success: true,
        removed_paths,
        source,
        source_type,
        error: None,
    })
}

/// 删除路径（目录或 symlink）
///
/// 对应 CLI: remove.ts:156-161
/// ```js
/// const stats = await lstat(skillPath).catch(() => null);
/// if (stats) {
///   await rm(skillPath, { recursive: true, force: true });
/// }
/// ```
///
/// Rust 优化：使用 symlink_metadata() 代替 lstat()，语义相同但更 Rust-idiomatic
fn remove_path(path: &PathBuf) -> Result<(), AppError> {
    // 检查路径是否存在（包括 symlink 本身，不 follow）
    match path.symlink_metadata() {
        Ok(metadata) => {
            if metadata.is_dir() {
                // 实体目录：递归删除
                fs::remove_dir_all(path)?;
            } else {
                // 文件或 symlink：直接删除
                // 注：Windows junction 也可能被 symlink_metadata 视为 directory，
                // 此时 remove_dir_all 会处理
                fs::remove_file(path).or_else(|_| fs::remove_dir_all(path))?;
            }
            Ok(())
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            // 路径不存在，无需操作（对应 CLI 的 force: true）
            Ok(())
        }
        Err(e) => Err(AppError::Io {
            message: e.to_string(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_remove_path_directory() {
        let temp = tempdir().unwrap();
        let dir = temp.path().join("test-skill");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("SKILL.md"), "# Test").unwrap();

        assert!(dir.exists());
        remove_path(&dir).unwrap();
        assert!(!dir.exists());
    }

    #[test]
    fn test_remove_path_nonexistent() {
        let temp = tempdir().unwrap();
        let path = temp.path().join("nonexistent");

        // 不存在的路径不应报错（对应 CLI 的 force: true）
        let result = remove_path(&path);
        assert!(result.is_ok());
    }

    #[test]
    fn test_remove_path_file() {
        let temp = tempdir().unwrap();
        let file = temp.path().join("test-file");
        fs::write(&file, "content").unwrap();

        assert!(file.exists());
        remove_path(&file).unwrap();
        assert!(!file.exists());
    }

    #[test]
    fn test_remove_path_nested_directory() {
        let temp = tempdir().unwrap();
        let dir = temp.path().join("test-skill");
        let sub_dir = dir.join("scripts");
        fs::create_dir_all(&sub_dir).unwrap();
        fs::write(dir.join("SKILL.md"), "# Test").unwrap();
        fs::write(sub_dir.join("helper.py"), "# Python").unwrap();

        assert!(dir.exists());
        remove_path(&dir).unwrap();
        assert!(!dir.exists());
    }

    #[cfg(unix)]
    #[test]
    fn test_remove_path_symlink() {
        use std::os::unix::fs::symlink;

        let temp = tempdir().unwrap();
        let target = temp.path().join("target-skill");
        let link = temp.path().join("link-skill");

        fs::create_dir_all(&target).unwrap();
        fs::write(target.join("SKILL.md"), "# Test").unwrap();
        symlink(&target, &link).unwrap();

        assert!(link.symlink_metadata().is_ok());
        remove_path(&link).unwrap();
        // symlink 应被删除
        assert!(link.symlink_metadata().is_err());
        // 目标目录不受影响
        assert!(target.exists());
    }
}

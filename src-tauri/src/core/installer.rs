//! 安装核心模块
//!
//! 功能：
//! - 复制文件到 canonical 目录
//! - 创建 symlink/junction 到各 agent 目录
//! - 处理 fallback 到 copy 模式
//!
//! 与 CLI installer.ts 行为一致

use crate::core::agents::AgentType;
use crate::core::paths::canonical_skills_dir;
use crate::core::skill::sanitize_name;
use crate::error::AppError;
use crate::models::{InstallMode, InstallResult, Scope};
use std::fs;
use std::path::{Path, PathBuf};

/// 复制时排除的文件（与 CLI 一致）
const EXCLUDE_FILES: &[&str] = &["README.md", "metadata.json"];

/// 复制时排除的目录（与 CLI 一致）
const EXCLUDE_DIRS: &[&str] = &[".git"];

/// 安装 skill 到指定 agent
///
/// # Arguments
/// * `skill_path` - skill 源目录路径
/// * `skill_name` - skill 名称
/// * `agent` - 目标 agent 类型
/// * `scope` - 安装范围（Global/Project）
/// * `project_path` - Project scope 时的项目路径
/// * `mode` - 安装模式（Symlink/Copy）
///
/// # Returns
/// * `InstallResult` - 安装结果（成功或失败信息）
pub fn install_skill_for_agent(
    skill_path: &Path,
    skill_name: &str,
    agent: &AgentType,
    scope: &Scope,
    project_path: Option<&str>,
    mode: &InstallMode,
) -> InstallResult {
    let is_global = matches!(scope, Scope::Global);
    let cwd = project_path.unwrap_or(".");
    let sanitized_name = sanitize_name(skill_name);

    // 检查 agent 是否支持 global 安装
    let config = agent.config();
    if is_global && config.global_skills_dir.is_none() {
        return InstallResult {
            skill_name: skill_name.to_string(),
            agent: agent.to_string(),
            success: false,
            path: PathBuf::new(),
            canonical_path: None,
            mode: mode.clone(),
            symlink_failed: false,
            error: Some(format!(
                "{} does not support global skill installation",
                config.display_name
            )),
        };
    }

    let result = match mode {
        InstallMode::Symlink => {
            install_with_symlink(skill_path, &sanitized_name, agent, is_global, cwd)
        }
        InstallMode::Copy => install_with_copy(skill_path, &sanitized_name, agent, is_global, cwd),
    };

    match result {
        Ok((path, canonical_path, symlink_failed)) => InstallResult {
            skill_name: skill_name.to_string(),
            agent: agent.to_string(),
            success: true,
            path,
            canonical_path,
            mode: if symlink_failed {
                InstallMode::Copy
            } else {
                mode.clone()
            },
            symlink_failed,
            error: None,
        },
        Err(e) => InstallResult {
            skill_name: skill_name.to_string(),
            agent: agent.to_string(),
            success: false,
            path: PathBuf::new(),
            canonical_path: None,
            mode: mode.clone(),
            symlink_failed: false,
            error: Some(e.to_string()),
        },
    }
}

/// Symlink 模式安装
fn install_with_symlink(
    skill_path: &Path,
    skill_name: &str,
    agent: &AgentType,
    is_global: bool,
    cwd: &str,
) -> Result<(PathBuf, Option<PathBuf>, bool), AppError> {
    // 1. 确定 canonical 目录
    let canonical_base = canonical_skills_dir(is_global, cwd);
    let canonical_dir = canonical_base.join(skill_name);

    // 2. 复制到 canonical 目录
    clean_and_create_directory(&canonical_dir)?;
    copy_skill_files(skill_path, &canonical_dir)?;

    // 3. 对于 Universal Agent 的 global 安装，跳过 symlink（已在 canonical 目录）
    if is_global && agent.is_universal() {
        return Ok((canonical_dir.clone(), Some(canonical_dir), false));
    }

    // 4. 获取 agent 目录
    let config = agent.config();
    let agent_base = if is_global {
        config.global_skills_dir.clone().unwrap()
    } else {
        PathBuf::from(cwd).join(&config.skills_dir)
    };
    let agent_dir = agent_base.join(skill_name);

    // 5. 创建 symlink
    let symlink_failed = match create_symlink(&canonical_dir, &agent_dir) {
        Ok(_) => false,
        Err(_) => {
            // Symlink 失败，fallback 到 copy
            clean_and_create_directory(&agent_dir)?;
            copy_skill_files(skill_path, &agent_dir)?;
            true
        }
    };

    Ok((agent_dir, Some(canonical_dir), symlink_failed))
}

/// Copy 模式安装
fn install_with_copy(
    skill_path: &Path,
    skill_name: &str,
    agent: &AgentType,
    is_global: bool,
    cwd: &str,
) -> Result<(PathBuf, Option<PathBuf>, bool), AppError> {
    let config = agent.config();
    let agent_base = if is_global {
        config.global_skills_dir.clone().unwrap()
    } else {
        PathBuf::from(cwd).join(&config.skills_dir)
    };
    let agent_dir = agent_base.join(skill_name);

    clean_and_create_directory(&agent_dir)?;
    copy_skill_files(skill_path, &agent_dir)?;

    Ok((agent_dir, None, false))
}

/// 清理并创建目录（与 CLI cleanAndCreateDirectory 一致）
fn clean_and_create_directory(path: &Path) -> Result<(), AppError> {
    // 尝试删除现有目录/文件
    if path.exists() || path.symlink_metadata().is_ok() {
        let _ = fs::remove_dir_all(path);
        let _ = fs::remove_file(path);
    }

    // 创建目录
    fs::create_dir_all(path)
        .map_err(|e| AppError::InstallFailed { message: format!("Failed to create dir: {}", e) })?;

    Ok(())
}

/// 复制 skill 文件（排除特定文件，与 CLI copyDirectory 一致）
fn copy_skill_files(src: &Path, dst: &Path) -> Result<(), AppError> {
    // 确保目标目录存在
    fs::create_dir_all(dst)
        .map_err(|e| AppError::InstallFailed { message: format!("Failed to create dir: {}", e) })?;

    // 遍历源目录
    let entries = fs::read_dir(src)
        .map_err(|e| AppError::InstallFailed { message: format!("Failed to read dir: {}", e) })?;

    for entry in entries.filter_map(|e| e.ok()) {
        let path = entry.path();
        let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");

        // 跳过排除的文件
        if EXCLUDE_FILES.contains(&file_name) {
            continue;
        }
        // 跳过 _ 开头的文件/目录
        if file_name.starts_with('_') {
            continue;
        }

        let dst_path = dst.join(file_name);

        if path.is_dir() {
            // 跳过排除的目录
            if EXCLUDE_DIRS.contains(&file_name) {
                continue;
            }
            // 递归复制目录
            copy_skill_files(&path, &dst_path)?;
        } else {
            // 复制文件（解引用 symlink）
            fs::copy(&path, &dst_path)
                .map_err(|e| AppError::InstallFailed { message: format!("Failed to copy file: {}", e) })?;
        }
    }

    Ok(())
}

/// 创建 symlink（跨平台，与 CLI createSymlink 一致）
fn create_symlink(target: &Path, link: &Path) -> Result<(), AppError> {
    // 确保父目录存在
    if let Some(parent) = link.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| AppError::InstallFailed { message: format!("Failed to create parent dir: {}", e) })?;
    }

    // 检查目标和链接是否相同
    let resolved_target = target.canonicalize().unwrap_or_else(|_| target.to_path_buf());
    let resolved_link_parent = link
        .parent()
        .and_then(|p| p.canonicalize().ok())
        .unwrap_or_else(|| link.parent().unwrap_or(Path::new(".")).to_path_buf());
    let resolved_link = resolved_link_parent.join(link.file_name().unwrap_or_default());

    if resolved_target == resolved_link {
        // 相同路径，无需创建 symlink
        return Ok(());
    }

    // 如果已存在，先删除
    if link.exists() || link.symlink_metadata().is_ok() {
        if link.is_dir() {
            fs::remove_dir_all(link).ok();
        } else {
            fs::remove_file(link).ok();
        }
    }

    // 计算相对路径
    let relative_target = pathdiff::diff_paths(&resolved_target, &resolved_link_parent)
        .ok_or_else(|| AppError::InstallFailed { message: "Failed to compute relative path".to_string() })?;

    // 创建 symlink
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&relative_target, link)
            .map_err(|e| AppError::InstallFailed(format!("Failed to create symlink: {}", e)))?;
    }

    #[cfg(windows)]
    {
        // Windows 优先尝试 junction（不需要管理员权限）
        if let Err(_) = junction::create(&resolved_target, link) {
            // Junction 失败，尝试 symlink_dir
            std::os::windows::fs::symlink_dir(&relative_target, link)
                .map_err(|e| AppError::InstallFailed { message: format!("Failed to create symlink: {}", e) })?;
        }
    }

    Ok(())
}

/// 检查 skill 是否已安装在指定 agent
pub fn is_skill_installed(
    skill_name: &str,
    agent: &AgentType,
    scope: &Scope,
    project_path: Option<&str>,
) -> bool {
    let is_global = matches!(scope, Scope::Global);
    let cwd = project_path.unwrap_or(".");
    let sanitized_name = sanitize_name(skill_name);

    let config = agent.config();

    // 检查 agent 是否支持 global 安装
    if is_global && config.global_skills_dir.is_none() {
        return false;
    }

    let agent_base = if is_global {
        config.global_skills_dir.clone().unwrap()
    } else {
        PathBuf::from(cwd).join(&config.skills_dir)
    };

    let skill_dir = agent_base.join(&sanitized_name);
    skill_dir.exists()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_copy_skill_files_basic() {
        let src = tempdir().unwrap();
        let dst = tempdir().unwrap();

        // 创建源文件
        fs::write(src.path().join("SKILL.md"), "# Test").unwrap();
        fs::write(src.path().join("config.json"), "{}").unwrap();

        copy_skill_files(src.path(), dst.path()).unwrap();

        assert!(dst.path().join("SKILL.md").exists());
        assert!(dst.path().join("config.json").exists());
    }

    #[test]
    fn test_copy_skill_files_excludes() {
        let src = tempdir().unwrap();
        let dst = tempdir().unwrap();

        // 创建源文件（包括应被排除的）
        fs::write(src.path().join("SKILL.md"), "# Test").unwrap();
        fs::write(src.path().join("README.md"), "# README").unwrap();
        fs::write(src.path().join("metadata.json"), "{}").unwrap();
        fs::write(src.path().join("_internal.md"), "internal").unwrap();
        fs::create_dir(src.path().join(".git")).unwrap();
        fs::write(src.path().join(".git/config"), "git config").unwrap();

        copy_skill_files(src.path(), dst.path()).unwrap();

        // SKILL.md 应该被复制
        assert!(dst.path().join("SKILL.md").exists());
        // 这些应该被排除
        assert!(!dst.path().join("README.md").exists());
        assert!(!dst.path().join("metadata.json").exists());
        assert!(!dst.path().join("_internal.md").exists());
        assert!(!dst.path().join(".git").exists());
    }

    #[test]
    fn test_copy_skill_files_recursive() {
        let src = tempdir().unwrap();
        let dst = tempdir().unwrap();

        // 创建嵌套目录结构
        fs::create_dir(src.path().join("scripts")).unwrap();
        fs::write(src.path().join("SKILL.md"), "# Test").unwrap();
        fs::write(src.path().join("scripts/helper.py"), "# Python").unwrap();

        copy_skill_files(src.path(), dst.path()).unwrap();

        assert!(dst.path().join("SKILL.md").exists());
        assert!(dst.path().join("scripts/helper.py").exists());
    }

    #[test]
    fn test_clean_and_create_directory() {
        let temp = tempdir().unwrap();
        let dir = temp.path().join("test-dir");

        // 首次创建
        clean_and_create_directory(&dir).unwrap();
        assert!(dir.exists());

        // 添加文件
        fs::write(dir.join("file.txt"), "content").unwrap();

        // 再次调用应该清理并重建
        clean_and_create_directory(&dir).unwrap();
        assert!(dir.exists());
        assert!(!dir.join("file.txt").exists());
    }
}

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

/// Per-agent install result (shared between install and update flows)
#[derive(Debug, Clone)]
pub struct PerAgentInstallResult {
    pub agent: String,
    pub success: bool,
    pub error: Option<String>,
    pub duration_ms: Option<u32>,
    pub symlink_failed: bool,
    pub path: PathBuf,
    pub canonical_path: Option<PathBuf>,
    pub mode: InstallMode,
}

/// 复制时排除的文件（与 CLI 一致）
const EXCLUDE_FILES: &[&str] = &["metadata.json"];

/// 复制时排除的目录（与 CLI 一致）
const EXCLUDE_DIRS: &[&str] = &[".git", "__pycache__", "__pypackages__"];

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
    let canonical_dir = write_canonical_skill(skill_path, skill_name, is_global, cwd)?;

    // 3. 创建 symlink（Universal Agent global 安装跳过）
    symlink_canonical_to_agent(&canonical_dir, skill_name, agent, is_global, cwd)
}

/// 从已有的 canonical 目录创建 symlink 到 agent 目录（不复制 canonical）
///
/// 与 `install_with_symlink` 共享 "resolve agent dir + create symlink" 逻辑。
fn link_from_canonical(
    canonical_dir: &Path,
    skill_name: &str,
    agent: &AgentType,
    is_global: bool,
    cwd: &str,
) -> Result<(PathBuf, Option<PathBuf>, bool), AppError> {
    symlink_canonical_to_agent(canonical_dir, skill_name, agent, is_global, cwd)
}

/// 共享核心：从 canonical dir 创建 symlink 到 agent dir
fn symlink_canonical_to_agent(
    canonical_dir: &Path,
    skill_name: &str,
    agent: &AgentType,
    is_global: bool,
    cwd: &str,
) -> Result<(PathBuf, Option<PathBuf>, bool), AppError> {
    symlink_canonical_to_agent_inner(canonical_dir, skill_name, agent, is_global, cwd, true)
}

fn symlink_canonical_to_agent_inner(
    canonical_dir: &Path,
    skill_name: &str,
    agent: &AgentType,
    is_global: bool,
    cwd: &str,
    fallback_to_copy: bool,
) -> Result<(PathBuf, Option<PathBuf>, bool), AppError> {
    let agent_dir = resolve_agent_skill_dir(agent, skill_name, is_global, cwd)?;

    // 创建 symlink
    let symlink_failed = match create_symlink(canonical_dir, &agent_dir) {
        Ok(_) => false,
        Err(_) if fallback_to_copy => {
            // Symlink 失败，fallback 到 copy
            clean_and_create_directory(&agent_dir)?;
            copy_skill_files(canonical_dir, &agent_dir)?;
            true
        }
        Err(e) => return Err(e),
    };

    Ok((agent_dir, Some(canonical_dir.to_path_buf()), symlink_failed))
}

/// Copy 模式安装
fn install_with_copy(
    skill_path: &Path,
    skill_name: &str,
    agent: &AgentType,
    is_global: bool,
    cwd: &str,
) -> Result<(PathBuf, Option<PathBuf>, bool), AppError> {
    let canonical_dir = write_canonical_skill(skill_path, skill_name, is_global, cwd)?;
    copy_canonical_to_agent(&canonical_dir, skill_name, agent, is_global, cwd)
}

fn resolve_agent_base_dir(
    agent: &AgentType,
    is_global: bool,
    cwd: &str,
) -> Result<PathBuf, AppError> {
    if agent.is_universal() {
        return Ok(canonical_skills_dir(is_global, cwd));
    }

    let config = agent.config();
    if is_global {
        config
            .global_skills_dir
            .clone()
            .ok_or_else(|| AppError::InstallFailed {
                message: format!(
                    "{} does not support global skill installation",
                    config.display_name
                ),
            })
    } else {
        Ok(PathBuf::from(cwd).join(config.skills_dir))
    }
}

fn resolve_agent_skill_dir(
    agent: &AgentType,
    skill_name: &str,
    is_global: bool,
    cwd: &str,
) -> Result<PathBuf, AppError> {
    Ok(resolve_agent_base_dir(agent, is_global, cwd)?.join(skill_name))
}

fn write_canonical_skill(
    skill_path: &Path,
    skill_name: &str,
    is_global: bool,
    cwd: &str,
) -> Result<PathBuf, AppError> {
    let canonical_dir = canonical_skills_dir(is_global, cwd).join(skill_name);
    clean_and_create_directory(&canonical_dir)?;
    copy_skill_files(skill_path, &canonical_dir)?;
    Ok(canonical_dir)
}

fn copy_canonical_to_agent(
    canonical_dir: &Path,
    skill_name: &str,
    agent: &AgentType,
    is_global: bool,
    cwd: &str,
) -> Result<(PathBuf, Option<PathBuf>, bool), AppError> {
    let agent_dir = resolve_agent_skill_dir(agent, skill_name, is_global, cwd)?;
    let resolved_canonical = canonical_dir
        .canonicalize()
        .unwrap_or_else(|_| canonical_dir.to_path_buf());
    let resolved_agent = agent_dir
        .canonicalize()
        .unwrap_or_else(|_| agent_dir.clone());

    if resolved_canonical == resolved_agent {
        return Ok((
            canonical_dir.to_path_buf(),
            Some(canonical_dir.to_path_buf()),
            false,
        ));
    }

    clean_and_create_directory(&agent_dir)?;
    copy_skill_files(canonical_dir, &agent_dir)?;

    Ok((agent_dir, Some(canonical_dir.to_path_buf()), false))
}

/// 清理并创建目录（与 CLI cleanAndCreateDirectory 一致）
fn clean_and_create_directory(path: &Path) -> Result<(), AppError> {
    // 尝试删除现有目录/文件
    if path.exists() || path.symlink_metadata().is_ok() {
        let _ = fs::remove_dir_all(path);
        let _ = fs::remove_file(path);
    }

    // 创建目录
    fs::create_dir_all(path).map_err(|e| AppError::InstallFailed {
        message: format!("Failed to create dir: {}", e),
    })?;

    Ok(())
}

/// 复制 skill 文件（排除特定文件，与 CLI copyDirectory 一致）
fn copy_skill_files(src: &Path, dst: &Path) -> Result<(), AppError> {
    // 确保目标目录存在
    fs::create_dir_all(dst).map_err(|e| AppError::InstallFailed {
        message: format!("Failed to create dir: {}", e),
    })?;

    // 遍历源目录
    let entries = fs::read_dir(src).map_err(|e| AppError::InstallFailed {
        message: format!("Failed to read dir: {}", e),
    })?;

    for entry in entries.filter_map(|e| e.ok()) {
        let path = entry.path();
        let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");

        // 跳过排除的文件
        if EXCLUDE_FILES.contains(&file_name) {
            continue;
        }
        // 仅跳过 dotfile / dotdir（CLI 不再排除 underscore 文件）
        if file_name.starts_with('.') {
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
            // 复制文件（解引用 symlink）。broken symlink 直接跳过，不中断整个安装。
            if let Err(e) = fs::copy(&path, &dst_path) {
                let is_broken_symlink = e.kind() == std::io::ErrorKind::NotFound
                    && path
                        .symlink_metadata()
                        .map(|m| m.file_type().is_symlink())
                        .unwrap_or(false);

                if !is_broken_symlink {
                    return Err(AppError::InstallFailed {
                        message: format!("Failed to copy file: {}", e),
                    });
                }
            }
        }
    }

    Ok(())
}

/// 创建 symlink（跨平台，与 CLI createSymlink 一致）
fn create_symlink(target: &Path, link: &Path) -> Result<(), AppError> {
    // 确保父目录存在
    if let Some(parent) = link.parent() {
        fs::create_dir_all(parent).map_err(|e| AppError::InstallFailed {
            message: format!("Failed to create parent dir: {}", e),
        })?;
    }

    // 检查目标和链接是否相同
    let resolved_target = target
        .canonicalize()
        .unwrap_or_else(|_| target.to_path_buf());
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
        .ok_or_else(|| AppError::InstallFailed {
            message: "Failed to compute relative path".to_string(),
        })?;

    // 创建 symlink
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&relative_target, link).map_err(|e| {
            AppError::InstallFailed {
                message: format!("Failed to create symlink: {}", e),
            }
        })?;
    }

    #[cfg(windows)]
    {
        // Windows 优先尝试 junction（不需要管理员权限）
        if let Err(_) = junction::create(&resolved_target, link) {
            // Junction 失败，尝试 symlink_dir
            std::os::windows::fs::symlink_dir(&relative_target, link).map_err(|e| {
                AppError::InstallFailed {
                    message: format!("Failed to create symlink: {}", e),
                }
            })?;
        }
    }

    Ok(())
}

/// 为已安装的 skill 创建到新 agent 的 symlink（不重新复制 canonical dir）
///
/// 与 `install_skill_for_agent` 的区别：跳过 copy-to-canonical 步骤，
/// 仅创建从已有 canonical dir 到 agent dir 的 symlink。
/// 用于 manage_agents 命令（为已有 skill 添加 agent 支持）。
pub fn link_skill_for_agent(
    canonical_dir: &Path,
    skill_name: &str,
    agent: &AgentType,
    scope: &Scope,
    project_path: Option<&str>,
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
            mode: InstallMode::Symlink,
            symlink_failed: false,
            error: Some(format!(
                "{} does not support global skill installation",
                config.display_name
            )),
        };
    }

    match link_from_canonical(canonical_dir, &sanitized_name, agent, is_global, cwd) {
        Ok((path, canonical_path, symlink_failed)) => InstallResult {
            skill_name: skill_name.to_string(),
            agent: agent.to_string(),
            success: true,
            path,
            canonical_path,
            mode: if symlink_failed {
                InstallMode::Copy
            } else {
                InstallMode::Symlink
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
            mode: InstallMode::Symlink,
            symlink_failed: false,
            error: Some(e.to_string()),
        },
    }
}

/// 为已安装的 skill 创建到新 agent 的 symlink，失败时不降级为 copy。
///
/// 用于 manage_agents 命令：用户显式选择 symlink 时，失败需要暴露给前端。
pub fn link_skill_for_agent_without_fallback(
    canonical_dir: &Path,
    skill_name: &str,
    agent: &AgentType,
    scope: &Scope,
    project_path: Option<&str>,
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
            mode: InstallMode::Symlink,
            symlink_failed: false,
            error: Some(format!(
                "{} does not support global skill installation",
                config.display_name
            )),
        };
    }

    match symlink_canonical_to_agent_inner(
        canonical_dir,
        &sanitized_name,
        agent,
        is_global,
        cwd,
        false,
    ) {
        Ok((path, canonical_path, symlink_failed)) => InstallResult {
            skill_name: skill_name.to_string(),
            agent: agent.to_string(),
            success: true,
            path,
            canonical_path,
            mode: if symlink_failed {
                InstallMode::Copy
            } else {
                InstallMode::Symlink
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
            mode: InstallMode::Symlink,
            symlink_failed: false,
            error: Some(e.to_string()),
        },
    }
}

/// 为已安装的 skill 复制一份到新 agent 目录（不重写 canonical）。
pub fn copy_skill_for_agent(
    canonical_dir: &Path,
    skill_name: &str,
    agent: &AgentType,
    scope: &Scope,
    project_path: Option<&str>,
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
            mode: InstallMode::Copy,
            symlink_failed: false,
            error: Some(format!(
                "{} does not support global skill installation",
                config.display_name
            )),
        };
    }

    match copy_canonical_to_agent(canonical_dir, &sanitized_name, agent, is_global, cwd) {
        Ok((path, canonical_path, symlink_failed)) => InstallResult {
            skill_name: skill_name.to_string(),
            agent: agent.to_string(),
            success: true,
            path,
            canonical_path,
            mode: InstallMode::Copy,
            symlink_failed,
            error: None,
        },
        Err(e) => InstallResult {
            skill_name: skill_name.to_string(),
            agent: agent.to_string(),
            success: false,
            path: PathBuf::new(),
            canonical_path: None,
            mode: InstallMode::Copy,
            symlink_failed: false,
            error: Some(e.to_string()),
        },
    }
}

/// Install a single skill to multiple agents, returning per-agent results.
///
/// Shared core function used by both install and update commands.
pub fn install_skill_to_agents(
    skill_path: &Path,
    skill_name: &str,
    agents: &[AgentType],
    scope: &Scope,
    project_path: Option<&str>,
    mode: &InstallMode,
) -> Vec<PerAgentInstallResult> {
    let mut results = Vec::with_capacity(agents.len());

    for agent in agents {
        let started = std::time::Instant::now();
        let result =
            install_skill_for_agent(skill_path, skill_name, agent, scope, project_path, mode);

        let elapsed = started.elapsed().as_millis();
        let duration_ms = if elapsed > u32::MAX as u128 {
            u32::MAX
        } else {
            elapsed as u32
        };

        results.push(PerAgentInstallResult {
            agent: agent.to_string(),
            success: result.success,
            error: result.error,
            duration_ms: Some(duration_ms),
            symlink_failed: result.symlink_failed,
            path: result.path,
            canonical_path: result.canonical_path,
            mode: result.mode,
        });
    }

    results
}

/// Install a single skill to multiple agents, preserving each agent's mode.
///
/// Used by update flows where existing agents may be mixed symlink/copy installs.
pub fn install_skill_to_agents_with_modes(
    skill_path: &Path,
    skill_name: &str,
    agents: &[(AgentType, InstallMode)],
    scope: &Scope,
    project_path: Option<&str>,
) -> Vec<PerAgentInstallResult> {
    let is_global = matches!(scope, Scope::Global);
    let cwd = project_path.unwrap_or(".");
    let sanitized_name = sanitize_name(skill_name);

    let canonical_dir = match write_canonical_skill(skill_path, &sanitized_name, is_global, cwd) {
        Ok(path) => path,
        Err(err) => {
            return agents
                .iter()
                .map(|(agent, mode)| PerAgentInstallResult {
                    agent: agent.to_string(),
                    success: false,
                    error: Some(err.to_string()),
                    duration_ms: None,
                    symlink_failed: false,
                    path: PathBuf::new(),
                    canonical_path: None,
                    mode: mode.clone(),
                })
                .collect();
        }
    };

    agents
        .iter()
        .map(|(agent, mode)| {
            let started = std::time::Instant::now();
            let result = match mode {
                InstallMode::Symlink => link_skill_for_agent(
                    &canonical_dir,
                    &sanitized_name,
                    agent,
                    scope,
                    project_path,
                ),
                InstallMode::Copy => copy_skill_for_agent(
                    &canonical_dir,
                    &sanitized_name,
                    agent,
                    scope,
                    project_path,
                ),
            };
            let elapsed = started.elapsed().as_millis();
            let duration_ms = if elapsed > u32::MAX as u128 {
                u32::MAX
            } else {
                elapsed as u32
            };

            PerAgentInstallResult {
                agent: agent.to_string(),
                success: result.success,
                error: result.error,
                duration_ms: Some(duration_ms),
                symlink_failed: result.symlink_failed,
                path: result.path,
                canonical_path: result.canonical_path,
                mode: result.mode,
            }
        })
        .collect()
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

    let skill_dir = match resolve_agent_skill_dir(agent, &sanitized_name, is_global, cwd) {
        Ok(path) => path,
        Err(_) => return false,
    };
    skill_dir.exists()
}

/// Detect which agents actually have a skill installed by scanning the file system.
///
/// Used by the update command to determine which agents to update,
/// instead of maintaining metadata in lock files.
pub fn detect_installed_agents_for_skill(
    skill_name: &str,
    scope: &Scope,
    project_path: Option<&str>,
) -> Vec<AgentType> {
    let is_global = matches!(scope, Scope::Global);
    let cwd = project_path.unwrap_or(".");
    let sanitized_name = sanitize_name(skill_name);

    // Always scan all agent types — checking ~40 paths via symlink_metadata() is negligible,
    // and this catches orphaned agent directories (e.g., user uninstalled Cursor but .cursor/rules still exists).
    let candidates = AgentType::all();

    let mut installed = Vec::new();
    for agent in candidates {
        let skill_path = match resolve_agent_skill_dir(&agent, &sanitized_name, is_global, cwd) {
            Ok(path) => path,
            Err(_) => continue,
        };

        // Use symlink_metadata to detect even broken symlinks
        if skill_path.symlink_metadata().is_ok() {
            installed.push(agent);
        }
    }

    installed
}

/// Detect whether a skill was installed via symlink/junction or copy
/// by examining the actual file system state.
pub fn detect_install_mode(
    skill_name: &str,
    agent: &AgentType,
    scope: &Scope,
    project_path: Option<&str>,
) -> InstallMode {
    let is_global = matches!(scope, Scope::Global);
    let cwd = project_path.unwrap_or(".");
    let sanitized_name = sanitize_name(skill_name);
    let skill_path = match resolve_agent_skill_dir(agent, &sanitized_name, is_global, cwd) {
        Ok(path) => path,
        Err(_) => return InstallMode::Symlink, // default
    };

    let is_symlink = skill_path
        .symlink_metadata()
        .map(|m| {
            let symlink = m.file_type().is_symlink();

            #[cfg(windows)]
            let symlink = symlink || {
                use std::os::windows::fs::MetadataExt;
                // Junction = directory + reparse point (0x400)
                m.file_type().is_dir() && m.file_attributes() & 0x400 != 0
            };

            symlink
        })
        .unwrap_or(false);

    if is_symlink {
        InstallMode::Symlink
    } else {
        InstallMode::Copy
    }
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
        fs::write(src.path().join(".env"), "secret").unwrap();
        fs::create_dir(src.path().join(".git")).unwrap();
        fs::write(src.path().join(".git/config"), "git config").unwrap();
        fs::create_dir(src.path().join("__pycache__")).unwrap();
        fs::write(src.path().join("__pycache__/module.pyc"), "pyc").unwrap();
        fs::create_dir(src.path().join("__pypackages__")).unwrap();
        fs::write(src.path().join("__pypackages__/lock"), "pkg").unwrap();

        copy_skill_files(src.path(), dst.path()).unwrap();

        // SKILL.md 应该被复制
        assert!(dst.path().join("SKILL.md").exists());
        // README.md 现在会被保留（CLI v1.4.1 变更）
        assert!(dst.path().join("README.md").exists());
        // underscore 文件现在应该被保留
        assert!(dst.path().join("_internal.md").exists());
        // 这些应该被排除
        assert!(!dst.path().join("metadata.json").exists());
        assert!(!dst.path().join(".env").exists());
        assert!(!dst.path().join(".git").exists());
        assert!(!dst.path().join("__pycache__").exists());
        assert!(!dst.path().join("__pypackages__").exists());
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

    #[test]
    fn test_install_skill_to_agents_returns_per_agent_results() {
        let src = tempdir().unwrap();
        fs::write(src.path().join("SKILL.md"), "# Test").unwrap();

        // Empty agents list returns empty results
        let agents = vec![];
        let results = install_skill_to_agents(
            src.path(),
            "test-skill",
            &agents,
            &Scope::Global,
            None,
            &InstallMode::Copy,
        );
        assert!(results.is_empty());
    }

    #[test]
    fn test_copy_install_writes_canonical_and_agent_copy() {
        let temp = tempdir().unwrap();
        let project_path = temp.path().to_string_lossy().to_string();
        let src = temp.path().join("source-skill");
        fs::create_dir_all(&src).unwrap();
        fs::write(
            src.join("SKILL.md"),
            "---\nname: copy-skill\ndescription: test\n---\n",
        )
        .unwrap();

        let result = install_skill_for_agent(
            &src,
            "copy-skill",
            &AgentType::ClaudeCode,
            &Scope::Project,
            Some(&project_path),
            &InstallMode::Copy,
        );

        assert!(
            result.success,
            "copy install should succeed: {:?}",
            result.error
        );

        let canonical = temp
            .path()
            .join(".agents")
            .join("skills")
            .join("copy-skill");
        let agent_copy = temp
            .path()
            .join(".claude")
            .join("skills")
            .join("copy-skill");

        assert!(
            canonical.join("SKILL.md").exists(),
            "canonical must be written"
        );
        assert!(
            agent_copy.join("SKILL.md").exists(),
            "agent copy must be written"
        );
        assert_eq!(result.canonical_path.as_deref(), Some(canonical.as_path()));
    }

    #[test]
    fn test_copy_install_same_path_universal_preserves_canonical() {
        let temp = tempdir().unwrap();
        let project_path = temp.path().to_string_lossy().to_string();
        let src = temp.path().join("source-skill");
        fs::create_dir_all(&src).unwrap();
        fs::write(
            src.join("SKILL.md"),
            "---\nname: universal-copy\ndescription: test\n---\n",
        )
        .unwrap();

        let result = install_skill_for_agent(
            &src,
            "universal-copy",
            &AgentType::Cursor,
            &Scope::Project,
            Some(&project_path),
            &InstallMode::Copy,
        );

        assert!(
            result.success,
            "same-path copy install should succeed: {:?}",
            result.error
        );
        let canonical = temp
            .path()
            .join(".agents")
            .join("skills")
            .join("universal-copy");
        assert!(canonical.join("SKILL.md").exists());
        assert_eq!(result.path, canonical);
        assert_eq!(result.canonical_path.as_deref(), Some(canonical.as_path()));
    }

    #[test]
    fn test_detect_installed_agents_empty_for_nonexistent_skill() {
        let results =
            detect_installed_agents_for_skill("nonexistent-skill-xyz-12345", &Scope::Global, None);
        assert!(results.is_empty());
    }

    #[test]
    fn test_link_skill_for_agent_creates_symlink_from_canonical() {
        let temp = tempdir().unwrap();
        let project_path = temp.path().to_string_lossy().to_string();

        // 创建 canonical dir（模拟已安装的 skill）
        let canonical_dir = temp
            .path()
            .join(".agents")
            .join("skills")
            .join("test-skill");
        fs::create_dir_all(&canonical_dir).unwrap();
        fs::write(canonical_dir.join("SKILL.md"), "# Test Skill").unwrap();
        fs::write(canonical_dir.join("config.json"), "{}").unwrap();

        // 用一个 non-universal agent 测试
        let agent = AgentType::Cursor;
        let result = link_skill_for_agent(
            &canonical_dir,
            "test-skill",
            &agent,
            &Scope::Project,
            Some(&project_path),
        );

        assert!(result.success, "link should succeed: {:?}", result.error);
        assert!(result.canonical_path.is_some());
        // canonical dir 应该保持不变
        assert!(
            canonical_dir.join("SKILL.md").exists(),
            "canonical dir should not be destroyed"
        );
        assert!(canonical_dir.join("config.json").exists());
    }

    #[test]
    fn test_link_skill_for_agent_does_not_destroy_canonical() {
        // 这是之前 install_skill_for_agent 的自毁 bug 的回归测试
        let temp = tempdir().unwrap();
        let project_path = temp.path().to_string_lossy().to_string();

        let canonical_dir = temp.path().join(".agents").join("skills").join("my-skill");
        fs::create_dir_all(&canonical_dir).unwrap();
        let content = "---\nname: my-skill\n---\n# My Skill Content";
        fs::write(canonical_dir.join("SKILL.md"), content).unwrap();

        let agent = AgentType::Cline;
        let _ = link_skill_for_agent(
            &canonical_dir,
            "my-skill",
            &agent,
            &Scope::Project,
            Some(&project_path),
        );

        // 关键断言：canonical dir 内容完好
        let read_content = fs::read_to_string(canonical_dir.join("SKILL.md")).unwrap();
        assert_eq!(
            read_content, content,
            "canonical dir content must survive linking"
        );
    }

    #[test]
    fn test_install_skill_to_agents_with_modes_preserves_mixed_modes() {
        let temp = tempdir().unwrap();
        let project_path = temp.path().to_string_lossy().to_string();
        let src = temp.path().join("source-skill");
        fs::create_dir_all(&src).unwrap();
        fs::write(
            src.join("SKILL.md"),
            "---\nname: mixed-skill\ndescription: test\n---\n",
        )
        .unwrap();

        let targets = vec![
            (AgentType::ClaudeCode, InstallMode::Copy),
            (AgentType::Cursor, InstallMode::Symlink),
        ];

        let results = install_skill_to_agents_with_modes(
            &src,
            "mixed-skill",
            &targets,
            &Scope::Project,
            Some(&project_path),
        );

        assert!(results.iter().all(|r| r.success), "results: {:?}", results);
        assert!(temp
            .path()
            .join(".agents")
            .join("skills")
            .join("mixed-skill")
            .join("SKILL.md")
            .exists());
        assert!(temp
            .path()
            .join(".claude")
            .join("skills")
            .join("mixed-skill")
            .join("SKILL.md")
            .exists());
    }
}

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
use crate::models::{InstallMode, InstallResult, InstallResultCategory, Scope};
use std::fs;
use std::path::{Path, PathBuf};

/// Per-agent install result (shared between install and update flows)
#[derive(Debug, Clone)]
pub struct PerAgentInstallResult {
    pub agent: String,
    pub success: bool,
    pub skipped: bool,
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
            target_id: None,
            subagent: None,
            success: false,
            path: PathBuf::new(),
            canonical_path: None,
            mode: mode.clone(),
            symlink_failed: false,
            skipped: false,
            error: Some(format!(
                "{} does not support global skill installation",
                config.display_name
            )),
            category: InstallResultCategory::PrivateAdapted,
        };
    }

    let result = match mode {
        InstallMode::Symlink => {
            install_with_symlink(skill_path, &sanitized_name, agent, is_global, cwd)
        }
        InstallMode::Copy => install_with_copy(skill_path, &sanitized_name, agent, is_global, cwd),
    };

    match result {
        Ok((path, canonical_path, symlink_failed, skipped)) => InstallResult {
            skill_name: skill_name.to_string(),
            agent: agent.to_string(),
            target_id: None,
            subagent: None,
            success: true,
            path,
            canonical_path,
            mode: if symlink_failed {
                InstallMode::Copy
            } else {
                mode.clone()
            },
            symlink_failed,
            skipped,
            error: None,
            category: InstallResultCategory::PrivateAdapted,
        },
        Err(e) => InstallResult {
            skill_name: skill_name.to_string(),
            agent: agent.to_string(),
            target_id: None,
            subagent: None,
            success: false,
            path: PathBuf::new(),
            canonical_path: None,
            mode: mode.clone(),
            symlink_failed: false,
            skipped: false,
            error: Some(e.to_string()),
            category: InstallResultCategory::PrivateAdapted,
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
) -> Result<(PathBuf, Option<PathBuf>, bool, bool), AppError> {
    let canonical_dir = write_canonical_skill(skill_path, skill_name, is_global, cwd)?;

    // 3. 创建 symlink（自动应用的 global agent 跳过）
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
) -> Result<(PathBuf, Option<PathBuf>, bool, bool), AppError> {
    symlink_canonical_to_agent(canonical_dir, skill_name, agent, is_global, cwd)
}

/// 共享核心：从 canonical dir 创建 symlink 到 agent dir
fn symlink_canonical_to_agent(
    canonical_dir: &Path,
    skill_name: &str,
    agent: &AgentType,
    is_global: bool,
    cwd: &str,
) -> Result<(PathBuf, Option<PathBuf>, bool, bool), AppError> {
    symlink_canonical_to_agent_inner(canonical_dir, skill_name, agent, is_global, cwd, true, true)
}

fn symlink_canonical_to_agent_inner(
    canonical_dir: &Path,
    skill_name: &str,
    agent: &AgentType,
    is_global: bool,
    cwd: &str,
    fallback_to_copy: bool,
    skip_missing_project_agent_root: bool,
) -> Result<(PathBuf, Option<PathBuf>, bool, bool), AppError> {
    if skip_missing_project_agent_root && should_skip_project_agent_symlink(agent, is_global, cwd) {
        return Ok((
            canonical_dir.to_path_buf(),
            Some(canonical_dir.to_path_buf()),
            false,
            true,
        ));
    }

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

    Ok((
        agent_dir,
        Some(canonical_dir.to_path_buf()),
        symlink_failed,
        false,
    ))
}

/// Copy 模式安装
fn install_with_copy(
    skill_path: &Path,
    skill_name: &str,
    agent: &AgentType,
    is_global: bool,
    cwd: &str,
) -> Result<(PathBuf, Option<PathBuf>, bool, bool), AppError> {
    let canonical_dir = write_canonical_skill(skill_path, skill_name, is_global, cwd)?;
    copy_canonical_to_agent(&canonical_dir, skill_name, agent, is_global, cwd)
}

fn should_skip_project_agent_symlink(agent: &AgentType, is_global: bool, cwd: &str) -> bool {
    if is_global || agent.is_automatic_for_scope(false, cwd) {
        return false;
    }

    let config = agent.config();
    let Some(root) = config.skills_dir.split('/').next() else {
        return false;
    };

    !PathBuf::from(cwd).join(root).exists()
}

fn resolve_agent_base_dir(
    agent: &AgentType,
    is_global: bool,
    cwd: &str,
) -> Result<PathBuf, AppError> {
    if agent.is_automatic_for_scope(is_global, cwd) {
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

pub fn resolve_private_agent_skill_dir(
    agent: &AgentType,
    skill_name: &str,
    is_global: bool,
    cwd: &str,
) -> Result<PathBuf, AppError> {
    let availability =
        crate::core::agent_availability::availability_for_agent(*agent, is_global, cwd);
    let Some(private_path) = availability.private_path else {
        return Err(AppError::InstallFailed {
            message: format!(
                "{} does not have a separate private skill directory",
                agent.config().display_name
            ),
        });
    };

    Ok(PathBuf::from(private_path).join(skill_name))
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
) -> Result<(PathBuf, Option<PathBuf>, bool, bool), AppError> {
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
            false,
        ));
    }

    clean_and_create_directory(&agent_dir)?;
    copy_skill_files(canonical_dir, &agent_dir)?;

    Ok((agent_dir, Some(canonical_dir.to_path_buf()), false, false))
}

fn copy_canonical_to_private_agent(
    canonical_dir: &Path,
    skill_name: &str,
    agent: &AgentType,
    is_global: bool,
    cwd: &str,
) -> Result<(PathBuf, Option<PathBuf>, bool, bool), AppError> {
    let agent_dir = resolve_private_agent_skill_dir(agent, skill_name, is_global, cwd)?;
    clean_and_create_directory(&agent_dir)?;
    copy_skill_files(canonical_dir, &agent_dir)?;

    Ok((agent_dir, Some(canonical_dir.to_path_buf()), false, false))
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

fn copy_skill_files_for_agent(
    src: &Path,
    dst: &Path,
    agent: Option<AgentType>,
) -> Result<(), AppError> {
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
        let dst_path = dst.join(file_name);

        if path.is_dir() {
            // 跳过排除的目录
            if EXCLUDE_DIRS.contains(&file_name) {
                continue;
            }
            // 递归复制目录
            copy_skill_files_for_agent(&path, &dst_path, agent)?;
        } else {
            if agent == Some(AgentType::Eve) && file_name.eq_ignore_ascii_case("SKILL.md") {
                let raw = fs::read_to_string(&path).map_err(|e| AppError::InstallFailed {
                    message: format!("Failed to read file: {}", e),
                })?;
                let normalized = crate::core::eve::normalize_eve_skill_md(&raw);
                fs::write(&dst_path, normalized).map_err(|e| AppError::InstallFailed {
                    message: format!("Failed to write file: {}", e),
                })?;
                continue;
            }

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

/// 复制 skill 文件（排除特定文件，与 CLI copyDirectory 一致）
pub(crate) fn copy_skill_files(src: &Path, dst: &Path) -> Result<(), AppError> {
    copy_skill_files_for_agent(src, dst, None)
}

pub fn install_skill_for_eve_target(
    skill_path: &Path,
    skill_name: &str,
    project_path: &str,
    subagent: Option<&str>,
) -> InstallResult {
    let sanitized_name = sanitize_name(skill_name);
    let base_dir = crate::core::eve::eve_skills_dir_for_target(project_path, subagent);
    let target_dir = base_dir.join(&sanitized_name);
    let target_id = crate::core::eve::eve_target_id(subagent);

    if crate::core::eve::paths_overlap(skill_path, &target_dir) {
        return InstallResult {
            skill_name: skill_name.to_string(),
            agent: "eve".to_string(),
            target_id: Some(target_id),
            subagent: subagent.map(ToOwned::to_owned),
            success: true,
            path: target_dir,
            canonical_path: None,
            mode: InstallMode::Copy,
            symlink_failed: false,
            skipped: true,
            error: Some("source-overlaps-target".to_string()),
            category: InstallResultCategory::Skipped,
        };
    }

    let result = (|| -> Result<(), AppError> {
        clean_and_create_directory(&target_dir)?;
        copy_skill_files_for_agent(skill_path, &target_dir, Some(AgentType::Eve))
    })();

    match result {
        Ok(()) => InstallResult {
            skill_name: skill_name.to_string(),
            agent: "eve".to_string(),
            target_id: Some(target_id),
            subagent: subagent.map(ToOwned::to_owned),
            success: true,
            path: target_dir,
            canonical_path: None,
            mode: InstallMode::Copy,
            symlink_failed: false,
            skipped: false,
            error: None,
            category: InstallResultCategory::PrivateAdapted,
        },
        Err(error) => InstallResult {
            skill_name: skill_name.to_string(),
            agent: "eve".to_string(),
            target_id: Some(target_id),
            subagent: subagent.map(ToOwned::to_owned),
            success: false,
            path: target_dir,
            canonical_path: None,
            mode: InstallMode::Copy,
            symlink_failed: false,
            skipped: false,
            error: Some(error.to_string()),
            category: InstallResultCategory::Failed,
        },
    }
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
        if junction::create(&resolved_target, link).is_err() {
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
            target_id: None,
            subagent: None,
            success: false,
            path: PathBuf::new(),
            canonical_path: None,
            mode: InstallMode::Symlink,
            symlink_failed: false,
            skipped: false,
            error: Some(format!(
                "{} does not support global skill installation",
                config.display_name
            )),
            category: InstallResultCategory::PrivateAdapted,
        };
    }

    match link_from_canonical(canonical_dir, &sanitized_name, agent, is_global, cwd) {
        Ok((path, canonical_path, symlink_failed, skipped)) => InstallResult {
            skill_name: skill_name.to_string(),
            agent: agent.to_string(),
            target_id: None,
            subagent: None,
            success: true,
            path,
            canonical_path,
            mode: if symlink_failed {
                InstallMode::Copy
            } else {
                InstallMode::Symlink
            },
            symlink_failed,
            skipped,
            error: None,
            category: InstallResultCategory::PrivateAdapted,
        },
        Err(e) => InstallResult {
            skill_name: skill_name.to_string(),
            agent: agent.to_string(),
            target_id: None,
            subagent: None,
            success: false,
            path: PathBuf::new(),
            canonical_path: None,
            mode: InstallMode::Symlink,
            symlink_failed: false,
            skipped: false,
            error: Some(e.to_string()),
            category: InstallResultCategory::PrivateAdapted,
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
            target_id: None,
            subagent: None,
            success: false,
            path: PathBuf::new(),
            canonical_path: None,
            mode: InstallMode::Symlink,
            symlink_failed: false,
            skipped: false,
            error: Some(format!(
                "{} does not support global skill installation",
                config.display_name
            )),
            category: InstallResultCategory::PrivateAdapted,
        };
    }

    match symlink_canonical_to_agent_inner(
        canonical_dir,
        &sanitized_name,
        agent,
        is_global,
        cwd,
        false,
        false,
    ) {
        Ok((path, canonical_path, symlink_failed, skipped)) => InstallResult {
            skill_name: skill_name.to_string(),
            agent: agent.to_string(),
            target_id: None,
            subagent: None,
            success: true,
            path,
            canonical_path,
            mode: if symlink_failed {
                InstallMode::Copy
            } else {
                InstallMode::Symlink
            },
            symlink_failed,
            skipped,
            error: None,
            category: InstallResultCategory::PrivateAdapted,
        },
        Err(e) => InstallResult {
            skill_name: skill_name.to_string(),
            agent: agent.to_string(),
            target_id: None,
            subagent: None,
            success: false,
            path: PathBuf::new(),
            canonical_path: None,
            mode: InstallMode::Symlink,
            symlink_failed: false,
            skipped: false,
            error: Some(e.to_string()),
            category: InstallResultCategory::PrivateAdapted,
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
            target_id: None,
            subagent: None,
            success: false,
            path: PathBuf::new(),
            canonical_path: None,
            mode: InstallMode::Copy,
            symlink_failed: false,
            skipped: false,
            error: Some(format!(
                "{} does not support global skill installation",
                config.display_name
            )),
            category: InstallResultCategory::PrivateAdapted,
        };
    }

    match copy_canonical_to_agent(canonical_dir, &sanitized_name, agent, is_global, cwd) {
        Ok((path, canonical_path, symlink_failed, skipped)) => InstallResult {
            skill_name: skill_name.to_string(),
            agent: agent.to_string(),
            target_id: None,
            subagent: None,
            success: true,
            path,
            canonical_path,
            mode: InstallMode::Copy,
            symlink_failed,
            skipped,
            error: None,
            category: InstallResultCategory::PrivateAdapted,
        },
        Err(e) => InstallResult {
            skill_name: skill_name.to_string(),
            agent: agent.to_string(),
            target_id: None,
            subagent: None,
            success: false,
            path: PathBuf::new(),
            canonical_path: None,
            mode: InstallMode::Copy,
            symlink_failed: false,
            skipped: false,
            error: Some(e.to_string()),
            category: InstallResultCategory::PrivateAdapted,
        },
    }
}

pub fn copy_skill_for_agent_private(
    canonical_dir: &Path,
    skill_name: &str,
    agent: &AgentType,
    scope: &Scope,
    project_path: Option<&str>,
) -> InstallResult {
    let is_global = matches!(scope, Scope::Global);
    let cwd = project_path.unwrap_or(".");
    let sanitized_name = sanitize_name(skill_name);

    match copy_canonical_to_private_agent(canonical_dir, &sanitized_name, agent, is_global, cwd) {
        Ok((path, canonical_path, symlink_failed, skipped)) => InstallResult {
            skill_name: skill_name.to_string(),
            agent: agent.to_string(),
            target_id: None,
            subagent: None,
            success: true,
            path,
            canonical_path,
            mode: InstallMode::Copy,
            symlink_failed,
            skipped,
            error: None,
            category: InstallResultCategory::PrivateCopy,
        },
        Err(e) => InstallResult {
            skill_name: skill_name.to_string(),
            agent: agent.to_string(),
            target_id: None,
            subagent: None,
            success: false,
            path: PathBuf::new(),
            canonical_path: None,
            mode: InstallMode::Copy,
            symlink_failed: false,
            skipped: false,
            error: Some(e.to_string()),
            category: InstallResultCategory::PrivateCopy,
        },
    }
}

fn per_agent_result_from_install(
    result: InstallResult,
    duration_ms: Option<u32>,
) -> PerAgentInstallResult {
    PerAgentInstallResult {
        agent: result.agent,
        success: result.success,
        skipped: result.skipped,
        error: result.error,
        duration_ms,
        symlink_failed: result.symlink_failed,
        path: result.path,
        canonical_path: result.canonical_path,
        mode: result.mode,
    }
}

fn duration_ms(duration: std::time::Duration) -> u32 {
    let elapsed = duration.as_millis();
    if elapsed > u32::MAX as u128 {
        u32::MAX
    } else {
        elapsed as u32
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
            skipped: result.skipped,
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

pub fn install_skill_to_agent_groups(
    skill_path: &Path,
    skill_name: &str,
    private_required_agents: &[AgentType],
    private_copy_agents: &[AgentType],
    scope: &Scope,
    project_path: Option<&str>,
    mode: &InstallMode,
    include_canonical_result: bool,
) -> Vec<PerAgentInstallResult> {
    let is_global = matches!(scope, Scope::Global);
    let cwd = project_path.unwrap_or(".");
    let sanitized_name = sanitize_name(skill_name);
    let all_agents: Vec<AgentType> = private_required_agents
        .iter()
        .chain(private_copy_agents.iter())
        .copied()
        .collect();

    let canonical_dir = match write_canonical_skill(skill_path, &sanitized_name, is_global, cwd) {
        Ok(path) => path,
        Err(err) => {
            return all_agents
                .iter()
                .map(|agent| PerAgentInstallResult {
                    agent: agent.to_string(),
                    success: false,
                    skipped: false,
                    error: Some(err.to_string()),
                    duration_ms: None,
                    symlink_failed: false,
                    path: PathBuf::new(),
                    canonical_path: None,
                    mode: if private_copy_agents.contains(agent) {
                        InstallMode::Copy
                    } else {
                        mode.clone()
                    },
                })
                .collect();
        }
    };

    let mut results = Vec::with_capacity(all_agents.len());
    if include_canonical_result {
        results.push(PerAgentInstallResult {
            agent: "__canonical__".to_string(),
            success: true,
            skipped: false,
            error: None,
            duration_ms: None,
            symlink_failed: false,
            path: canonical_dir.clone(),
            canonical_path: Some(canonical_dir.clone()),
            mode: mode.clone(),
        });
    }

    for agent in private_required_agents {
        let started = std::time::Instant::now();
        let result = match mode {
            InstallMode::Symlink => {
                link_skill_for_agent(&canonical_dir, &sanitized_name, agent, scope, project_path)
            }
            InstallMode::Copy => {
                copy_skill_for_agent(&canonical_dir, &sanitized_name, agent, scope, project_path)
            }
        };
        results.push(per_agent_result_from_install(
            result,
            Some(duration_ms(started.elapsed())),
        ));
    }

    for agent in private_copy_agents {
        let started = std::time::Instant::now();
        let result = copy_skill_for_agent_private(
            &canonical_dir,
            &sanitized_name,
            agent,
            scope,
            project_path,
        );
        results.push(per_agent_result_from_install(
            result,
            Some(duration_ms(started.elapsed())),
        ));
    }

    results
}

pub fn install_skill_to_agent_groups_with_modes(
    skill_path: &Path,
    skill_name: &str,
    private_required_agents: &[(AgentType, InstallMode)],
    private_copy_agents: &[AgentType],
    scope: &Scope,
    project_path: Option<&str>,
    include_canonical_result: bool,
) -> Vec<PerAgentInstallResult> {
    let is_global = matches!(scope, Scope::Global);
    let cwd = project_path.unwrap_or(".");
    let sanitized_name = sanitize_name(skill_name);
    let all_agent_count = private_required_agents.len() + private_copy_agents.len();

    let canonical_dir = match write_canonical_skill(skill_path, &sanitized_name, is_global, cwd) {
        Ok(path) => path,
        Err(err) => {
            let mut results = Vec::with_capacity(all_agent_count);
            for (agent, mode) in private_required_agents {
                results.push(PerAgentInstallResult {
                    agent: agent.to_string(),
                    success: false,
                    skipped: false,
                    error: Some(err.to_string()),
                    duration_ms: None,
                    symlink_failed: false,
                    path: PathBuf::new(),
                    canonical_path: None,
                    mode: mode.clone(),
                });
            }
            for agent in private_copy_agents {
                results.push(PerAgentInstallResult {
                    agent: agent.to_string(),
                    success: false,
                    skipped: false,
                    error: Some(err.to_string()),
                    duration_ms: None,
                    symlink_failed: false,
                    path: PathBuf::new(),
                    canonical_path: None,
                    mode: InstallMode::Copy,
                });
            }
            return results;
        }
    };

    let mut results = Vec::with_capacity(all_agent_count);
    if include_canonical_result {
        results.push(PerAgentInstallResult {
            agent: "__canonical__".to_string(),
            success: true,
            skipped: false,
            error: None,
            duration_ms: None,
            symlink_failed: false,
            path: canonical_dir.clone(),
            canonical_path: Some(canonical_dir.clone()),
            mode: InstallMode::Copy,
        });
    }

    for (agent, mode) in private_required_agents {
        let started = std::time::Instant::now();
        let result = match mode {
            InstallMode::Symlink => {
                link_skill_for_agent(&canonical_dir, &sanitized_name, agent, scope, project_path)
            }
            InstallMode::Copy => {
                copy_skill_for_agent(&canonical_dir, &sanitized_name, agent, scope, project_path)
            }
        };
        results.push(per_agent_result_from_install(
            result,
            Some(duration_ms(started.elapsed())),
        ));
    }

    for agent in private_copy_agents {
        let started = std::time::Instant::now();
        let result = copy_skill_for_agent_private(
            &canonical_dir,
            &sanitized_name,
            agent,
            scope,
            project_path,
        );
        results.push(per_agent_result_from_install(
            result,
            Some(duration_ms(started.elapsed())),
        ));
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
                    skipped: false,
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
                skipped: result.skipped,
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

pub fn is_private_copy_installed(
    skill_name: &str,
    agent: &AgentType,
    scope: &Scope,
    project_path: Option<&str>,
) -> bool {
    let is_global = matches!(scope, Scope::Global);
    let cwd = project_path.unwrap_or(".");
    let sanitized_name = sanitize_name(skill_name);

    let skill_dir = match resolve_private_agent_skill_dir(agent, &sanitized_name, is_global, cwd) {
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
        fs::create_dir(src.path().join(".rules")).unwrap();
        fs::write(src.path().join(".rules/config.md"), "rules").unwrap();
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
        // dotfiles / dotdirs 应该被保留，除非是显式排除项
        assert!(dst.path().join(".env").exists());
        assert!(dst.path().join(".rules/config.md").exists());
        // 这些应该被排除
        assert!(!dst.path().join("metadata.json").exists());
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
    fn test_copy_install_same_path_automatic_agent_preserves_canonical() {
        let temp = tempdir().unwrap();
        let project_path = temp.path().to_string_lossy().to_string();
        let src = temp.path().join("source-skill");
        fs::create_dir_all(&src).unwrap();
        fs::write(
            src.join("SKILL.md"),
            "---\nname: shared-copy\ndescription: test\n---\n",
        )
        .unwrap();

        let result = install_skill_for_agent(
            &src,
            "shared-copy",
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
            .join("shared-copy");
        assert!(canonical.join("SKILL.md").exists());
        assert_eq!(result.path, canonical);
        assert_eq!(result.canonical_path.as_deref(), Some(canonical.as_path()));
    }

    #[test]
    fn test_project_automatic_agent_resolves_to_project_canonical() {
        let temp = tempdir().unwrap();
        let cwd = temp.path().to_string_lossy().to_string();

        let base = resolve_agent_base_dir(&AgentType::Antigravity, false, &cwd).unwrap();

        assert_eq!(base, temp.path().join(".agents").join("skills"));
    }

    #[test]
    fn test_global_antigravity_resolves_to_agent_specific_global_dir() {
        let base = resolve_agent_base_dir(&AgentType::Antigravity, true, ".").unwrap();
        let base_str = base.to_string_lossy();

        assert!(base_str.contains(".gemini"));
        assert!(base_str.contains("antigravity"));
        assert!(!base_str.ends_with(".agents/skills"));
    }

    #[test]
    fn test_global_shared_compatible_private_copy_resolves_private_dir() {
        let path =
            resolve_private_agent_skill_dir(&AgentType::Firebender, "demo", true, ".").unwrap();
        let path_str = path.to_string_lossy();

        assert!(path_str.contains(".firebender"));
        assert!(path_str.ends_with("skills/demo"));
        assert!(!path_str.ends_with(".agents/skills/demo"));
    }

    #[test]
    fn test_private_copy_writes_private_project_dir() {
        let temp = tempdir().unwrap();
        let project_path = temp.path().to_string_lossy().to_string();
        let canonical_dir = temp.path().join(".agents").join("skills").join("demo");
        fs::create_dir_all(&canonical_dir).unwrap();
        fs::write(canonical_dir.join("SKILL.md"), "# Demo").unwrap();

        let result = copy_skill_for_agent_private(
            &canonical_dir,
            "demo",
            &AgentType::ClaudeCode,
            &Scope::Project,
            Some(&project_path),
        );

        let private_dir = temp.path().join(".claude").join("skills").join("demo");
        assert!(
            result.success,
            "private copy should succeed: {:?}",
            result.error
        );
        assert_eq!(result.path, private_dir);
        assert_eq!(
            result.canonical_path.as_deref(),
            Some(canonical_dir.as_path())
        );
        assert!(private_dir.join("SKILL.md").exists());
        assert!(canonical_dir.join("SKILL.md").exists());
    }

    #[test]
    fn test_project_symlink_install_skips_separate_agent_when_root_missing() {
        let temp = tempdir().unwrap();
        let project_path = temp.path().to_string_lossy().to_string();
        let src = temp.path().join("source-skill");
        fs::create_dir_all(&src).unwrap();
        fs::write(
            src.join("SKILL.md"),
            "---\nname: cursor-skill\ndescription: test\n---\n",
        )
        .unwrap();

        let result = install_skill_for_agent(
            &src,
            "cursor-skill",
            &AgentType::Windsurf,
            &Scope::Project,
            Some(&project_path),
            &InstallMode::Symlink,
        );

        let canonical = temp
            .path()
            .join(".agents")
            .join("skills")
            .join("cursor-skill");

        assert!(
            result.success,
            "canonical install should succeed: {:?}",
            result.error
        );
        assert!(
            result.skipped,
            "missing project agent root should be skipped"
        );
        assert_eq!(result.path, canonical);
        assert_eq!(result.canonical_path.as_deref(), Some(canonical.as_path()));
        assert!(canonical.join("SKILL.md").exists());
        assert!(
            !temp.path().join(".windsurf").exists(),
            "project install should not create unused agent roots"
        );
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

        // 用一个独立目录 agent 测试
        let agent = AgentType::Cursor;
        fs::create_dir_all(temp.path().join(".cursor")).unwrap();
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
        fs::create_dir_all(temp.path().join(".cursor")).unwrap();

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

    #[test]
    fn test_eve_native_install_writes_to_root_agent_and_strips_name() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join("agent")).unwrap();
        let source = temp.path().join("source/demo");
        std::fs::create_dir_all(&source).unwrap();
        std::fs::write(
            source.join("SKILL.md"),
            "---\nname: demo\ndescription: Demo\n---\n# Demo\n",
        )
        .unwrap();

        let result =
            install_skill_for_eve_target(&source, "demo", &temp.path().to_string_lossy(), None);

        assert!(result.success);
        assert_eq!(result.target_id.as_deref(), Some("eve:root"));
        let installed = temp.path().join("agent/skills/demo/SKILL.md");
        let content = std::fs::read_to_string(installed).unwrap();
        assert!(!content.contains("name:"));
        assert!(content.contains("description: Demo"));
    }

    #[test]
    fn test_eve_native_install_skips_when_source_overlaps_target() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("agent/skills/demo");
        std::fs::create_dir_all(&source).unwrap();
        std::fs::write(
            source.join("SKILL.md"),
            "---\nname: demo\ndescription: Demo\n---\n# Demo\n",
        )
        .unwrap();
        std::fs::write(source.join("extra.txt"), "keep").unwrap();

        let result =
            install_skill_for_eve_target(&source, "demo", &temp.path().to_string_lossy(), None);

        assert!(result.success);
        assert!(result.skipped);
        assert_eq!(result.error.as_deref(), Some("source-overlaps-target"));
        assert!(source.join("extra.txt").exists());
    }
}

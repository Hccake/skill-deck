//! Skills 发现模块
//!
//! 功能：
//! - 扫描目录查找 SKILL.md 文件
//! - 解析 frontmatter 获取 skill 信息
//! - 支持 internal skills 过滤
//!
//! 与 CLI skills.ts 行为一致

use crate::core::skill::parse_skill_md;
use crate::error::AppError;
use crate::models::AvailableSkill;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

/// 发现时跳过的目录（与 CLI 一致）
const SKIP_DIRS: &[&str] = &["node_modules", ".git", "dist", "build", "__pycache__"];

/// 最大递归深度（与 CLI 一致）
const MAX_DEPTH: usize = 5;

/// 发现选项
#[derive(Debug, Default)]
pub struct DiscoverOptions {
    /// 是否包含 internal skills
    pub include_internal: bool,
    /// 是否进行深度递归搜索（即使已找到 skills）
    pub full_depth: bool,
}

/// 发现的 Skill 信息
#[derive(Debug, Clone)]
pub struct DiscoveredSkill {
    pub name: String,
    pub description: String,
    pub path: PathBuf,
    pub relative_path: String,
    pub is_internal: bool,
}

impl From<DiscoveredSkill> for AvailableSkill {
    fn from(skill: DiscoveredSkill) -> Self {
        AvailableSkill {
            name: skill.name,
            description: skill.description,
            relative_path: skill.relative_path,
        }
    }
}

/// 发现目录中的所有 skills
///
/// # Arguments
/// * `base_path` - 搜索根目录
/// * `subpath` - 可选的子路径
/// * `options` - 发现选项
///
/// # 行为（与 CLI 一致）
/// 1. 如果 searchPath 本身有 SKILL.md，添加它（除非 fullDepth，否则立即返回）
/// 2. 搜索优先目录（skills/, .claude/skills/ 等）
/// 3. 如果未找到或 fullDepth=true，进行递归搜索
/// 4. 使用 seenNames 去重
pub fn discover_skills(
    base_path: &Path,
    subpath: Option<&str>,
    options: DiscoverOptions,
) -> Result<Vec<DiscoveredSkill>, AppError> {
    let search_path = match subpath {
        Some(sub) => base_path.join(sub),
        None => base_path.to_path_buf(),
    };

    if !search_path.exists() {
        return Err(AppError::PathNotFound {
            path: search_path.display().to_string(),
        });
    }

    let mut skills = Vec::new();
    let mut seen_names: HashSet<String> = HashSet::new();

    // 1. 检查 searchPath 本身是否是 skill
    let skill_md = search_path.join("SKILL.md");
    if skill_md.exists() {
        if let Some(skill) = try_parse_skill(&skill_md, base_path, &options)? {
            seen_names.insert(skill.name.clone());
            skills.push(skill);

            // 如果不是 fullDepth 模式，直接返回
            if !options.full_depth {
                return Ok(skills);
            }
        }
    }

    // 2. 搜索优先目录
    let priority_dirs = get_priority_search_dirs(&search_path);
    for priority_dir in priority_dirs {
        if priority_dir.exists() {
            discover_in_dir(&priority_dir, base_path, &options, &mut skills, &mut seen_names)?;
        }
    }

    // 3. 如果未找到或启用 fullDepth，进行递归搜索
    if skills.is_empty() || options.full_depth {
        discover_recursive(&search_path, base_path, &options, &mut skills, &mut seen_names)?;
    }

    Ok(skills)
}

/// 获取优先搜索目录列表（与 CLI 一致）
fn get_priority_search_dirs(search_path: &Path) -> Vec<PathBuf> {
    vec![
        search_path.to_path_buf(),
        search_path.join("skills"),
        search_path.join("skills/.curated"),
        search_path.join("skills/.experimental"),
        search_path.join("skills/.system"),
        search_path.join(".agent/skills"),
        search_path.join(".agents/skills"),
        search_path.join(".claude/skills"),
        search_path.join(".cline/skills"),
        search_path.join(".codebuddy/skills"),
        search_path.join(".codex/skills"),
        search_path.join(".commandcode/skills"),
        search_path.join(".continue/skills"),
        search_path.join(".cursor/skills"),
        search_path.join(".github/skills"),
        search_path.join(".goose/skills"),
        search_path.join(".iflow/skills"),
        search_path.join(".junie/skills"),
        search_path.join(".kilocode/skills"),
        search_path.join(".kiro/skills"),
        search_path.join(".mux/skills"),
        search_path.join(".neovate/skills"),
        search_path.join(".opencode/skills"),
        search_path.join(".openhands/skills"),
        search_path.join(".pi/skills"),
        search_path.join(".qoder/skills"),
        search_path.join(".roo/skills"),
        search_path.join(".trae/skills"),
        search_path.join(".windsurf/skills"),
        search_path.join(".zencoder/skills"),
    ]
}

/// 在目录中发现 skills（搜索直接子目录）
fn discover_in_dir(
    dir: &Path,
    root: &Path,
    options: &DiscoverOptions,
    skills: &mut Vec<DiscoveredSkill>,
    seen_names: &mut HashSet<String>,
) -> Result<(), AppError> {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return Ok(()), // 目录不存在或无权限
    };

    for entry in entries.filter_map(|e| e.ok()) {
        let path = entry.path();
        if path.is_dir() {
            let skill_md = path.join("SKILL.md");
            if skill_md.exists() {
                if let Some(skill) = try_parse_skill(&skill_md, root, options)? {
                    if !seen_names.contains(&skill.name) {
                        seen_names.insert(skill.name.clone());
                        skills.push(skill);
                    }
                }
            }
        }
    }

    Ok(())
}

/// 递归发现 skills
fn discover_recursive(
    dir: &Path,
    root: &Path,
    options: &DiscoverOptions,
    skills: &mut Vec<DiscoveredSkill>,
    seen_names: &mut HashSet<String>,
) -> Result<(), AppError> {
    let walker = WalkDir::new(dir)
        .max_depth(MAX_DEPTH)
        .follow_links(true)
        .into_iter()
        .filter_entry(|e| {
            let name = e.file_name().to_str().unwrap_or("");
            // 跳过排除目录
            if e.file_type().is_dir() && SKIP_DIRS.contains(&name) {
                return false;
            }
            true
        });

    for entry in walker.filter_map(|e| e.ok()) {
        let path = entry.path();
        if path.is_file() {
            if let Some(file_name) = path.file_name() {
                if file_name.to_str() == Some("SKILL.md") {
                    if let Some(skill) = try_parse_skill(path, root, options)? {
                        if !seen_names.contains(&skill.name) {
                            seen_names.insert(skill.name.clone());
                            skills.push(skill);
                        }
                    }
                }
            }
        }
    }

    Ok(())
}

/// 检查是否应该安装 internal skills（与 CLI 一致）
fn should_install_internal_skills() -> bool {
    std::env::var("INSTALL_INTERNAL_SKILLS")
        .map(|v| v == "1" || v.to_lowercase() == "true")
        .unwrap_or(false)
}

/// 尝试解析 SKILL.md 文件
fn try_parse_skill(
    skill_md: &Path,
    root: &Path,
    options: &DiscoverOptions,
) -> Result<Option<DiscoveredSkill>, AppError> {
    // 使用 skill.rs 中的 parse_skill_md 函数
    let parsed = match parse_skill_md(skill_md) {
        Ok(p) => p,
        Err(_) => return Ok(None), // 解析失败，跳过
    };

    // 检查是否是 internal skill
    let is_internal = parsed
        .metadata
        .as_ref()
        .map(|m| m.internal)
        .unwrap_or(false);

    // 如果是 internal 且未启用 include_internal 且环境变量未设置，跳过
    if is_internal && !options.include_internal && !should_install_internal_skills() {
        return Ok(None);
    }

    // 计算相对路径
    let skill_dir = skill_md.parent().unwrap_or(skill_md);
    let relative_path = skill_dir
        .strip_prefix(root)
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|_| skill_dir.to_string_lossy().to_string());

    // 使用 SKILL.md 路径格式
    let relative_skill_path = if relative_path.is_empty() {
        "SKILL.md".to_string()
    } else {
        format!("{}/SKILL.md", relative_path)
    };

    Ok(Some(DiscoveredSkill {
        name: parsed.name,
        description: parsed.description,
        path: skill_dir.to_path_buf(),
        relative_path: relative_skill_path,
        is_internal,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_discover_skills_in_simple_dir() {
        let temp = tempdir().unwrap();
        let skill_dir = temp.path().join("my-skill");
        fs::create_dir_all(&skill_dir).unwrap();

        let skill_md = skill_dir.join("SKILL.md");
        fs::write(
            &skill_md,
            "---\nname: test-skill\ndescription: A test skill\n---\nContent",
        )
        .unwrap();

        let options = DiscoverOptions::default();
        let skills = discover_skills(temp.path(), None, options).unwrap();

        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].name, "test-skill");
        assert_eq!(skills[0].description, "A test skill");
    }

    #[test]
    fn test_discover_skills_in_skills_subdir() {
        let temp = tempdir().unwrap();
        let skill_dir = temp.path().join("skills/my-skill");
        fs::create_dir_all(&skill_dir).unwrap();

        let skill_md = skill_dir.join("SKILL.md");
        fs::write(&skill_md, "---\nname: nested-skill\ndescription: Nested\n---\n").unwrap();

        let options = DiscoverOptions::default();
        let skills = discover_skills(temp.path(), None, options).unwrap();

        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].name, "nested-skill");
    }

    #[test]
    fn test_skip_internal_skills_by_default() {
        let temp = tempdir().unwrap();
        let skill_dir = temp.path().join("internal-skill");
        fs::create_dir_all(&skill_dir).unwrap();

        let skill_md = skill_dir.join("SKILL.md");
        fs::write(
            &skill_md,
            "---\nname: internal\ndescription: Internal skill\nmetadata:\n  internal: true\n---\n",
        )
        .unwrap();

        let options = DiscoverOptions::default();
        let skills = discover_skills(temp.path(), None, options).unwrap();

        assert_eq!(skills.len(), 0);
    }

    #[test]
    fn test_include_internal_skills_with_option() {
        let temp = tempdir().unwrap();
        let skill_dir = temp.path().join("internal-skill");
        fs::create_dir_all(&skill_dir).unwrap();

        let skill_md = skill_dir.join("SKILL.md");
        fs::write(
            &skill_md,
            "---\nname: internal\ndescription: Internal skill\nmetadata:\n  internal: true\n---\n",
        )
        .unwrap();

        let options = DiscoverOptions {
            include_internal: true,
            ..Default::default()
        };
        let skills = discover_skills(temp.path(), None, options).unwrap();

        assert_eq!(skills.len(), 1);
        assert!(skills[0].is_internal);
    }

    #[test]
    fn test_deduplicate_skills_by_name() {
        let temp = tempdir().unwrap();

        // 创建两个同名 skill
        let skill_dir1 = temp.path().join("skill1");
        let skill_dir2 = temp.path().join("skill2");
        fs::create_dir_all(&skill_dir1).unwrap();
        fs::create_dir_all(&skill_dir2).unwrap();

        fs::write(
            skill_dir1.join("SKILL.md"),
            "---\nname: same-name\ndescription: First\n---\n",
        )
        .unwrap();
        fs::write(
            skill_dir2.join("SKILL.md"),
            "---\nname: same-name\ndescription: Second\n---\n",
        )
        .unwrap();

        let options = DiscoverOptions::default();
        let skills = discover_skills(temp.path(), None, options).unwrap();

        // 应该只有一个（第一个找到的）
        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].name, "same-name");
    }

    #[test]
    fn test_direct_skill_path() {
        let temp = tempdir().unwrap();
        let skill_md = temp.path().join("SKILL.md");
        fs::write(
            &skill_md,
            "---\nname: direct-skill\ndescription: Direct\n---\n",
        )
        .unwrap();

        let options = DiscoverOptions::default();
        let skills = discover_skills(temp.path(), None, options).unwrap();

        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].name, "direct-skill");
    }

    #[test]
    fn test_skip_missing_fields() {
        let temp = tempdir().unwrap();
        let skill_dir = temp.path().join("incomplete-skill");
        fs::create_dir_all(&skill_dir).unwrap();

        // 缺少 description
        let skill_md = skill_dir.join("SKILL.md");
        fs::write(&skill_md, "---\nname: incomplete\ndescription: \"\"\n---\n").unwrap();

        let options = DiscoverOptions::default();
        let skills = discover_skills(temp.path(), None, options).unwrap();

        assert_eq!(skills.len(), 0);
    }
}

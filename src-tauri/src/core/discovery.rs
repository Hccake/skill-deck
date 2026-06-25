//! Skills 发现模块
//!
//! 功能：
//! - 扫描目录查找 SKILL.md 文件
//! - 解析 frontmatter 获取 skill 信息
//! - 支持 internal skills 过滤
//!
//! 与 CLI skills.ts 行为一致

use crate::core::skill::{parse_skill_md, sanitize_name};
use crate::core::skill_paths::{find_skill_md_case_insensitive, relative_skill_path};
use crate::error::AppError;
use crate::models::AvailableSkill;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

/// 发现时跳过的目录（与 CLI 一致）
const SKIP_DIRS: &[&str] = &["node_modules", ".git", "dist", "build", "__pycache__"];

/// 最大递归深度（与 CLI 一致）
const MAX_DEPTH: usize = 5;

const CLI_AGENT_PROJECT_SKILL_DIRS: &[&str] = &[
    ".agents/skills",
    ".claude/skills",
    ".cline/skills",
    ".codebuddy/skills",
    ".codex/skills",
    ".commandcode/skills",
    ".continue/skills",
    ".github/skills",
    ".goose/skills",
    ".iflow/skills",
    ".junie/skills",
    ".kilocode/skills",
    ".kiro/skills",
    ".mux/skills",
    ".neovate/skills",
    ".opencode/skills",
    ".openhands/skills",
    ".pi/skills",
    ".qoder/skills",
    ".roo/skills",
    ".trae/skills",
    ".windsurf/skills",
    ".zencoder/skills",
];

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
    pub install_dir_name: String,
    pub description: String,
    pub path: PathBuf,
    pub relative_path: String,
    /// 所属 plugin 名称（来自 .claude-plugin/ manifest）
    pub plugin_name: Option<String>,
}

struct PrioritySearchDir {
    path: PathBuf,
    bounded_depth_two: bool,
    filter_locked_agent_project_skills: bool,
}

impl From<DiscoveredSkill> for AvailableSkill {
    fn from(skill: DiscoveredSkill) -> Self {
        AvailableSkill {
            name: skill.name,
            install_dir_name: skill.install_dir_name,
            description: skill.description,
            relative_path: skill.relative_path,
            plugin_name: skill.plugin_name,
            well_known_version: None,
            well_known_entry_type: None,
            artifact_url_host: None,
            digest_verified: None,
            trust_reason: None,
        }
    }
}

/// Lexically normalize a path by resolving `.` and `..` without filesystem access
fn lexical_normalize(path: &Path) -> PathBuf {
    let mut components = Vec::new();
    for component in path.components() {
        match component {
            std::path::Component::ParentDir => {
                if !components.is_empty() {
                    components.pop();
                }
            }
            std::path::Component::CurDir => {}
            c => components.push(c),
        }
    }
    components.iter().collect()
}

/// 校验 resolved subpath 不逃逸 base_path（第二层防护）
/// 与 CLI isSubpathSafe() 行为一致
pub fn is_subpath_safe(base_path: &Path, subpath: &str) -> bool {
    let base = match base_path.canonicalize() {
        Ok(p) => p,
        Err(_) => base_path.to_path_buf(),
    };
    let target = base.join(subpath);
    let resolved = match target.canonicalize() {
        Ok(p) => p,
        Err(_) => lexical_normalize(&target),
    };
    resolved.starts_with(&base)
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

    // 校验 subpath 不逃逸 base_path（防止路径遍历）
    if let Some(sub) = subpath {
        if !is_subpath_safe(base_path, sub) {
            return Err(AppError::InvalidSource {
                value: format!(
                    "Invalid subpath: \"{}\" resolves outside the repository directory",
                    sub
                ),
            });
        }
    }

    if !search_path.exists() {
        return Err(AppError::PathNotFound {
            path: search_path.display().to_string(),
        });
    }

    // 获取 plugin 分组映射
    let plugin_groupings = crate::core::plugin_manifest::get_plugin_groupings(&search_path);

    let mut skills = Vec::new();
    let mut seen_names: HashSet<String> = HashSet::new();
    let locked_project_skill_names = get_locked_project_skill_names(base_path);

    // 1. 检查 searchPath 本身是否是 skill
    if let Some(skill_md) = find_skill_md_case_insensitive(&search_path) {
        push_skill_if_new(
            &skill_md,
            base_path,
            &options,
            Some(&locked_project_skill_names),
            &mut skills,
            &mut seen_names,
        )?;

        // 如果不是 fullDepth 模式，直接返回
        if !options.full_depth {
            return Ok(skills);
        }
    }

    // 2. 搜索优先目录
    let priority_dirs = get_priority_search_dir_specs(&search_path);
    for priority_dir in priority_dirs {
        if priority_dir.path.exists() {
            discover_in_dir(
                &priority_dir.path,
                base_path,
                &options,
                priority_dir.bounded_depth_two,
                if priority_dir.filter_locked_agent_project_skills {
                    Some(&locked_project_skill_names)
                } else if is_in_cli_agent_project_skill_dir(base_path, &priority_dir.path) {
                    Some(&locked_project_skill_names)
                } else {
                    None
                },
                &mut skills,
                &mut seen_names,
            )?;
        }
    }

    // 3. 启用 fullDepth 时进行递归搜索
    if options.full_depth {
        discover_recursive(
            &search_path,
            base_path,
            &options,
            &locked_project_skill_names,
            &mut skills,
            &mut seen_names,
        )?;
    }

    // 为 skills 填充 plugin_name
    for skill in &mut skills {
        let normalized = crate::core::plugin_manifest::normalize_path(&skill.path);
        if let Some(name) = plugin_groupings.get(&normalized) {
            skill.plugin_name = Some(name.clone());
        }
    }

    Ok(skills)
}

/// 获取优先搜索目录列表（与 CLI 一致）
fn get_priority_search_dirs(search_path: &Path) -> Vec<PathBuf> {
    get_priority_search_dir_specs(search_path)
        .into_iter()
        .map(|spec| spec.path)
        .collect()
}

fn get_priority_search_dir_specs(search_path: &Path) -> Vec<PrioritySearchDir> {
    let mut dirs = vec![
        PrioritySearchDir {
            path: search_path.to_path_buf(),
            bounded_depth_two: false,
            filter_locked_agent_project_skills: false,
        },
        PrioritySearchDir {
            path: search_path.join("skills"),
            bounded_depth_two: true,
            filter_locked_agent_project_skills: false,
        },
        PrioritySearchDir {
            path: search_path.join("skills/.curated"),
            bounded_depth_two: true,
            filter_locked_agent_project_skills: false,
        },
        PrioritySearchDir {
            path: search_path.join("skills/.experimental"),
            bounded_depth_two: true,
            filter_locked_agent_project_skills: false,
        },
        PrioritySearchDir {
            path: search_path.join("skills/.system"),
            bounded_depth_two: true,
            filter_locked_agent_project_skills: false,
        },
    ];

    dirs.extend(
        CLI_AGENT_PROJECT_SKILL_DIRS
            .iter()
            .map(|dir| PrioritySearchDir {
                path: search_path.join(dir),
                bounded_depth_two: true,
                filter_locked_agent_project_skills: true,
            }),
    );

    dirs
}

/// 在目录中发现 skills（搜索直接子目录）
fn discover_in_dir(
    dir: &Path,
    root: &Path,
    options: &DiscoverOptions,
    bounded_depth_two: bool,
    locked_project_skill_names: Option<&HashSet<String>>,
    skills: &mut Vec<DiscoveredSkill>,
    seen_names: &mut HashSet<String>,
) -> Result<(), AppError> {
    for path in read_child_dirs(dir) {
        if let Some(skill_md) = find_skill_md_case_insensitive(&path) {
            push_skill_if_new(
                &skill_md,
                root,
                options,
                locked_project_skill_names,
                skills,
                seen_names,
            )?;
            continue;
        }

        if bounded_depth_two {
            for nested_path in read_child_dirs(&path) {
                if let Some(skill_md) = find_skill_md_case_insensitive(&nested_path) {
                    push_skill_if_new(
                        &skill_md,
                        root,
                        options,
                        locked_project_skill_names,
                        skills,
                        seen_names,
                    )?;
                }
            }
        }
    }

    Ok(())
}

fn read_child_dirs(dir: &Path) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = match std::fs::read_dir(dir) {
        Ok(entries) => entries
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.path())
            .filter(|path| {
                path.is_dir()
                    && path
                        .file_name()
                        .and_then(|name| name.to_str())
                        .is_none_or(|name| !SKIP_DIRS.contains(&name))
            })
            .collect(),
        Err(_) => Vec::new(),
    };

    dirs.sort();
    dirs
}

fn push_skill_if_new(
    skill_md: &Path,
    root: &Path,
    options: &DiscoverOptions,
    locked_project_skill_names: Option<&HashSet<String>>,
    skills: &mut Vec<DiscoveredSkill>,
    seen_names: &mut HashSet<String>,
) -> Result<(), AppError> {
    if let Some(skill) = try_parse_skill(skill_md, root, options)? {
        if is_in_cli_agent_project_skill_dir(root, &skill.path)
            && locked_project_skill_names
                .is_some_and(|locked| is_locked_project_skill(&skill, locked))
        {
            return Ok(());
        }

        if !seen_names.contains(&skill.name) {
            seen_names.insert(skill.name.clone());
            skills.push(skill);
        }
    }

    Ok(())
}

/// 递归发现 skills
fn discover_recursive(
    dir: &Path,
    root: &Path,
    options: &DiscoverOptions,
    locked_project_skill_names: &HashSet<String>,
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
                if file_name
                    .to_str()
                    .is_some_and(|name| name.eq_ignore_ascii_case("SKILL.md"))
                {
                    if let Some(skill) = try_parse_skill(path, root, options)? {
                        if is_in_cli_agent_project_skill_dir(root, &skill.path)
                            && is_locked_project_skill(&skill, locked_project_skill_names)
                        {
                            continue;
                        }

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

fn get_locked_project_skill_names(base_path: &Path) -> HashSet<String> {
    let lock_path = base_path.join("skills-lock.json");
    let Ok(content) = std::fs::read_to_string(lock_path) else {
        return HashSet::new();
    };
    let Ok(lock) = serde_json::from_str::<serde_json::Value>(&content) else {
        return HashSet::new();
    };

    lock.get("skills")
        .and_then(|skills| skills.as_object())
        .into_iter()
        .flat_map(|skills| skills.keys())
        .map(|name| normalize_skill_name_for_lock_match(name))
        .collect()
}

fn normalize_skill_name_for_lock_match(name: &str) -> String {
    name.to_lowercase()
        .split(|c: char| c.is_whitespace() || c == '_')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

fn is_locked_project_skill(skill: &DiscoveredSkill, locked_names: &HashSet<String>) -> bool {
    locked_names.contains(&normalize_skill_name_for_lock_match(&skill.name))
        || skill
            .path
            .file_name()
            .and_then(|name| name.to_str())
            .map(|name| locked_names.contains(&normalize_skill_name_for_lock_match(name)))
            .unwrap_or(false)
}

fn is_in_cli_agent_project_skill_dir(root: &Path, skill_dir: &Path) -> bool {
    let Ok(relative_path) = skill_dir.strip_prefix(root) else {
        return false;
    };

    CLI_AGENT_PROJECT_SKILL_DIRS
        .iter()
        .any(|agent_dir| relative_path.starts_with(agent_dir))
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
    let relative_skill_path = relative_skill_path(root, skill_md);

    let install_dir_name = sanitize_name(&parsed.name);

    Ok(Some(DiscoveredSkill {
        name: parsed.name,
        install_dir_name,
        description: parsed.description,
        path: skill_dir.to_path_buf(),
        relative_path: relative_skill_path,
        plugin_name: None,
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
    fn test_discover_skills_exposes_install_dir_name() {
        let temp = tempdir().unwrap();
        let skill_dir = temp.path().join("localized-skill");
        fs::create_dir_all(&skill_dir).unwrap();

        fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: 张雪峰-skill\ndescription: Localized\n---\n",
        )
        .unwrap();

        let skills = discover_skills(temp.path(), None, DiscoverOptions::default()).unwrap();
        let available = AvailableSkill::from(skills.into_iter().next().unwrap());
        let value = serde_json::to_value(available).unwrap();

        assert_eq!(
            value
                .get("installDirName")
                .and_then(serde_json::Value::as_str),
            Some("skill")
        );
    }

    #[test]
    fn test_discover_skills_in_skills_subdir() {
        let temp = tempdir().unwrap();
        let skill_dir = temp.path().join("skills/my-skill");
        fs::create_dir_all(&skill_dir).unwrap();

        let skill_md = skill_dir.join("SKILL.md");
        fs::write(
            &skill_md,
            "---\nname: nested-skill\ndescription: Nested\n---\n",
        )
        .unwrap();

        let options = DiscoverOptions::default();
        let skills = discover_skills(temp.path(), None, options).unwrap();

        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].name, "nested-skill");
    }

    #[test]
    fn test_discover_skills_finds_depth_two_catalog_skill_by_default() {
        let temp = tempdir().unwrap();
        let skill_dir = temp.path().join("skills/product/demo");
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: catalog-demo\ndescription: Demo\n---\n",
        )
        .unwrap();

        let skills = discover_skills(temp.path(), None, DiscoverOptions::default()).unwrap();

        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].name, "catalog-demo");
        assert_eq!(skills[0].relative_path, "skills/product/demo/SKILL.md");
    }

    #[test]
    fn test_discover_skills_depth_one_skill_shadows_nested_skill() {
        let temp = tempdir().unwrap();
        fs::create_dir_all(temp.path().join("skills/demo/inner")).unwrap();
        fs::write(
            temp.path().join("skills/demo/SKILL.md"),
            "---\nname: demo\ndescription: Demo\n---\n",
        )
        .unwrap();
        fs::write(
            temp.path().join("skills/demo/inner/SKILL.md"),
            "---\nname: inner\ndescription: Inner\n---\n",
        )
        .unwrap();

        let skills = discover_skills(temp.path(), None, DiscoverOptions::default()).unwrap();

        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].name, "demo");
    }

    #[test]
    fn test_discover_skills_does_not_scan_examples_depth_two_by_default() {
        let temp = tempdir().unwrap();
        let skill_dir = temp.path().join("examples/product/demo");
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: example-demo\ndescription: Demo\n---\n",
        )
        .unwrap();

        let skills = discover_skills(temp.path(), None, DiscoverOptions::default()).unwrap();

        assert!(skills.is_empty());
    }

    #[test]
    fn test_discover_skills_skips_locked_agent_project_skill_by_frontmatter_name() {
        let temp = tempdir().unwrap();
        fs::write(
            temp.path().join("skills-lock.json"),
            r#"{"version":1,"skills":{"demo-skill":{"source":"owner/repo","sourceType":"github","skillPath":"skills/demo/SKILL.md","computedHash":"abc"}}}"#,
        )
        .unwrap();
        let skill_dir = temp.path().join(".agents/skills/demo-folder");
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: demo skill\ndescription: Demo\n---\n",
        )
        .unwrap();

        let skills = discover_skills(temp.path(), None, DiscoverOptions::default()).unwrap();

        assert!(skills.is_empty());
    }

    #[test]
    fn test_discover_skills_skips_locked_agent_project_skill_by_directory_name() {
        let temp = tempdir().unwrap();
        fs::write(
            temp.path().join("skills-lock.json"),
            r#"{"version":1,"skills":{"demo-skill":{"source":"owner/repo","sourceType":"github","skillPath":"skills/demo/SKILL.md","computedHash":"abc"}}}"#,
        )
        .unwrap();
        let skill_dir = temp.path().join(".agents/skills/demo_skill");
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: different-name\ndescription: Demo\n---\n",
        )
        .unwrap();

        let skills = discover_skills(temp.path(), None, DiscoverOptions::default()).unwrap();

        assert!(skills.is_empty());
    }

    #[test]
    fn test_discover_skills_keeps_unlocked_agent_project_skill() {
        let temp = tempdir().unwrap();
        fs::write(
            temp.path().join("skills-lock.json"),
            r#"{"version":1,"skills":{}}"#,
        )
        .unwrap();
        let skill_dir = temp.path().join(".agents/skills/new-skill");
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: new-skill\ndescription: Demo\n---\n",
        )
        .unwrap();

        let skills = discover_skills(temp.path(), None, DiscoverOptions::default()).unwrap();

        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].name, "new-skill");
    }

    #[test]
    fn test_discover_skills_does_not_filter_from_legacy_project_lock() {
        let temp = tempdir().unwrap();
        fs::create_dir_all(temp.path().join(".agents")).unwrap();
        fs::write(
            temp.path().join(".agents/.skill-lock.json"),
            r#"{"version":1,"skills":{"demo-skill":{"source":"owner/repo","sourceType":"github","skillPath":"skills/demo/SKILL.md","computedHash":"abc"}}}"#,
        )
        .unwrap();
        let skill_dir = temp.path().join(".agents/skills/demo-skill");
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: demo-skill\ndescription: Demo\n---\n",
        )
        .unwrap();

        let skills = discover_skills(temp.path(), None, DiscoverOptions::default()).unwrap();

        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].name, "demo-skill");
    }

    #[test]
    fn test_discover_skills_skips_locked_direct_agent_skill_subpath() {
        let temp = tempdir().unwrap();
        fs::write(
            temp.path().join("skills-lock.json"),
            r#"{"version":1,"skills":{"demo-skill":{"source":"owner/repo","sourceType":"github","skillPath":"skills/demo/SKILL.md","computedHash":"abc"}}}"#,
        )
        .unwrap();
        let skill_dir = temp.path().join(".agents/skills/demo-skill");
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: demo-skill\ndescription: Demo\n---\n",
        )
        .unwrap();

        let skills = discover_skills(
            temp.path(),
            Some(".agents/skills/demo-skill"),
            DiscoverOptions::default(),
        )
        .unwrap();

        assert!(skills.is_empty());
    }

    #[test]
    fn test_discover_skills_skips_locked_agent_skills_subpath_children() {
        let temp = tempdir().unwrap();
        fs::write(
            temp.path().join("skills-lock.json"),
            r#"{"version":1,"skills":{"demo-skill":{"source":"owner/repo","sourceType":"github","skillPath":"skills/demo/SKILL.md","computedHash":"abc"}}}"#,
        )
        .unwrap();
        let skill_dir = temp.path().join(".agents/skills/demo-skill");
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: demo-skill\ndescription: Demo\n---\n",
        )
        .unwrap();

        let skills = discover_skills(
            temp.path(),
            Some(".agents/skills"),
            DiscoverOptions::default(),
        )
        .unwrap();

        assert!(skills.is_empty());
    }

    #[test]
    fn test_priority_search_dirs_match_cli_1_5_9_agent_dirs() {
        let temp = tempdir().unwrap();
        let dirs = get_priority_search_dirs(temp.path());
        let expected = [
            ".agents/skills",
            ".claude/skills",
            ".cline/skills",
            ".codebuddy/skills",
            ".codex/skills",
            ".commandcode/skills",
            ".continue/skills",
            ".github/skills",
            ".goose/skills",
            ".iflow/skills",
            ".junie/skills",
            ".kilocode/skills",
            ".kiro/skills",
            ".mux/skills",
            ".neovate/skills",
            ".opencode/skills",
            ".openhands/skills",
            ".pi/skills",
            ".qoder/skills",
            ".roo/skills",
            ".trae/skills",
            ".windsurf/skills",
            ".zencoder/skills",
        ];

        for dir in expected {
            assert!(
                dirs.contains(&temp.path().join(dir)),
                "priority dirs should include {dir}"
            );
        }

        let excluded = [
            ".aider-desk/skills",
            ".codeartsdoer/skills",
            ".codemaker/skills",
            ".codestudio/skills",
            ".cursor/skills",
            ".devin/skills",
            ".forge/skills",
            ".hermes/skills",
            ".rovodev/skills",
            ".tabnine/agent/skills",
        ];

        for dir in excluded {
            assert!(
                !dirs.contains(&temp.path().join(dir)),
                "priority dirs should not include {dir}"
            );
        }
    }

    #[test]
    fn test_discover_skills_preserves_actual_skill_md_casing() {
        let temp = tempdir().unwrap();
        let skill_dir = temp.path().join("skills/lowercase");
        fs::create_dir_all(&skill_dir).unwrap();

        fs::write(
            skill_dir.join("skill.md"),
            "---\nname: lowercase-skill\ndescription: Lowercase\n---\n",
        )
        .unwrap();

        let options = DiscoverOptions::default();
        let skills = discover_skills(temp.path(), None, options).unwrap();

        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].name, "lowercase-skill");
        assert_eq!(skills[0].relative_path, "skills/lowercase/skill.md");
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
        assert_eq!(skills[0].description, "Internal skill");
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
    fn test_is_subpath_safe_within_base() {
        let temp = tempdir().unwrap();
        assert!(is_subpath_safe(temp.path(), "skills"));
        assert!(is_subpath_safe(temp.path(), "a/b/c"));
    }

    #[test]
    fn test_is_subpath_safe_escape() {
        let temp = tempdir().unwrap();
        assert!(!is_subpath_safe(temp.path(), ".."));
        assert!(!is_subpath_safe(temp.path(), "../etc"));
        assert!(!is_subpath_safe(temp.path(), "../../etc/passwd"));
    }

    #[test]
    fn test_is_subpath_safe_edge_base_itself() {
        let temp = tempdir().unwrap();
        assert!(is_subpath_safe(temp.path(), "."));
    }

    #[test]
    fn test_discover_skills_rejects_unsafe_subpath() {
        let temp = tempdir().unwrap();
        let options = DiscoverOptions::default();
        let result = discover_skills(temp.path(), Some("../../"), options);
        assert!(result.is_err());
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

//! Skills 发现模块
//!
//! 功能：
//! - 扫描目录查找 SKILL.md 文件
//! - 解析 frontmatter 获取 skill 信息
//! - 支持 internal skills 过滤
//!
//! 与 CLI skills.ts 行为一致

use crate::core::builtin_agent_catalog::cli_project_discovery_dirs;
use crate::core::plugin_manifest::{
    get_relative_plugin_groupings, get_relative_plugin_search_dirs,
};
use crate::core::skill::{parse_skill_md_content, sanitize_name};
use crate::core::skill_paths::find_skill_md_case_insensitive;
use crate::error::AppError;
use crate::models::AvailableSkill;
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

/// 发现时跳过的目录（与 CLI 一致）
const SKIP_DIRS: &[&str] = &["node_modules", ".git", "dist", "build", "__pycache__"];

/// 最大递归深度（与 CLI 一致）
const MAX_DEPTH: usize = 5;

/// 已知 Skill 容器的默认搜索深度（与 CLI 一致）
const DEFAULT_SKILL_CONTAINER_DEPTH: usize = 3;

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
    pub relative_path: String,
    /// 所属 plugin 名称（来自 .claude-plugin/ manifest）
    pub plugin_name: Option<String>,
    pub(crate) internal: bool,
}

/// Environment collector 读取的 discovery 文档。
///
/// 路径始终相对于 search root；selector 不访问 filesystem。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveryDocument {
    pub relative_path: String,
    pub content: Vec<u8>,
}

/// Native 与 WSL 共用的 Environment-neutral discovery 输入。
#[derive(Debug, Clone)]
pub struct DiscoveryInventory {
    pub search_prefix: PathBuf,
    pub skill_documents: Vec<DiscoveryDocument>,
    pub marketplace_document: Option<String>,
    pub plugin_document: Option<String>,
    pub local_lock_document: Option<String>,
}

struct PrioritySearchDir {
    path: PathBuf,
    max_depth: usize,
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
    let inventory = collect_discovery_inventory(base_path, subpath)?;
    select_discovered_skills(&inventory, options)
}

/// 从 Native filesystem 一次性采集 selector 所需事实。
pub fn collect_discovery_inventory(
    base_path: &Path,
    subpath: Option<&str>,
) -> Result<DiscoveryInventory, AppError> {
    if let Some(subpath) = subpath {
        if !is_subpath_safe(base_path, subpath) {
            return Err(AppError::InvalidSource {
                value: format!(
                    "Invalid subpath: \"{subpath}\" resolves outside the repository directory"
                ),
            });
        }
    }

    let search_prefix = subpath.map(PathBuf::from).unwrap_or_default();
    let search_path = base_path.join(&search_prefix);
    if !search_path.exists() {
        return Err(AppError::PathNotFound {
            path: search_path.display().to_string(),
        });
    }

    let marketplace_document =
        std::fs::read_to_string(search_path.join(".claude-plugin/marketplace.json")).ok();
    let plugin_document =
        std::fs::read_to_string(search_path.join(".claude-plugin/plugin.json")).ok();
    let local_lock_document = std::fs::read_to_string(base_path.join("skills-lock.json")).ok();
    let mut documents = BTreeMap::new();

    let walker = WalkDir::new(&search_path)
        .max_depth(MAX_DEPTH + 1)
        .follow_links(false)
        .sort_by_file_name()
        .into_iter()
        .filter_entry(|entry| {
            !entry.file_type().is_dir()
                || entry
                    .file_name()
                    .to_str()
                    .is_none_or(|name| !SKIP_DIRS.contains(&name))
        });
    for entry in walker.filter_map(Result::ok) {
        let path = entry.path();
        let is_readable_file =
            entry.file_type().is_file() || (entry.file_type().is_symlink() && path.is_file());
        if !is_readable_file
            || !path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.eq_ignore_ascii_case("SKILL.md"))
        {
            continue;
        }
        insert_native_document(&search_path, path, &mut documents);
    }

    // Plugin manifest 可声明超过 recursive fallback 深度的路径；这些目录按
    // CLI 的 priority-dir 规则额外采集一层。
    for relative_dir in
        get_relative_plugin_search_dirs(marketplace_document.as_deref(), plugin_document.as_deref())
    {
        let directory = search_path.join(relative_dir);
        let Ok(entries) = std::fs::read_dir(directory) else {
            continue;
        };
        let mut child_dirs = entries
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                std::fs::symlink_metadata(path)
                    .map(|metadata| metadata.file_type().is_dir())
                    .unwrap_or(false)
            })
            .collect::<Vec<_>>();
        child_dirs.sort();
        for child in child_dirs {
            if let Some(skill_md) = find_skill_md_case_insensitive(&child) {
                insert_native_document(&search_path, &skill_md, &mut documents);
            }
        }
    }

    Ok(DiscoveryInventory {
        search_prefix,
        skill_documents: documents.into_values().collect(),
        marketplace_document,
        plugin_document,
        local_lock_document,
    })
}

fn insert_native_document(
    search_path: &Path,
    path: &Path,
    documents: &mut BTreeMap<String, DiscoveryDocument>,
) {
    let Ok(relative) = path.strip_prefix(search_path) else {
        return;
    };
    let Ok(content) = std::fs::read(path) else {
        return;
    };
    let relative_path = relative.to_string_lossy().replace('\\', "/");
    documents.insert(
        relative_path.clone(),
        DiscoveryDocument {
            relative_path,
            content,
        },
    );
}

/// 在不访问 filesystem 的情况下应用 skills CLI discovery 规则。
pub fn select_discovered_skills(
    inventory: &DiscoveryInventory,
    options: DiscoverOptions,
) -> Result<Vec<DiscoveredSkill>, AppError> {
    DiscoverySelector::new(inventory, options).select()
}

fn get_priority_search_dir_specs(search_path: &Path) -> Vec<PrioritySearchDir> {
    let mut dirs = vec![
        PrioritySearchDir {
            path: search_path.to_path_buf(),
            max_depth: 1,
        },
        PrioritySearchDir {
            path: search_path.join("skills"),
            max_depth: DEFAULT_SKILL_CONTAINER_DEPTH,
        },
        PrioritySearchDir {
            path: search_path.join("skills/.curated"),
            max_depth: DEFAULT_SKILL_CONTAINER_DEPTH,
        },
        PrioritySearchDir {
            path: search_path.join("skills/.experimental"),
            max_depth: DEFAULT_SKILL_CONTAINER_DEPTH,
        },
        PrioritySearchDir {
            path: search_path.join("skills/.system"),
            max_depth: DEFAULT_SKILL_CONTAINER_DEPTH,
        },
    ];

    dirs.extend(
        cli_project_discovery_dirs()
            .iter()
            .map(|dir| PrioritySearchDir {
                path: search_path.join(dir),
                max_depth: DEFAULT_SKILL_CONTAINER_DEPTH,
            }),
    );

    dirs
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CandidateResult {
    Missing,
    Present,
    Added,
}

struct DiscoverySelector<'a> {
    inventory: &'a DiscoveryInventory,
    options: DiscoverOptions,
    documents: BTreeMap<PathBuf, &'a DiscoveryDocument>,
    plugin_groupings: std::collections::HashMap<PathBuf, String>,
    plugin_search_dirs: Vec<PathBuf>,
    locked_names: HashSet<String>,
    seen_names: HashSet<String>,
    skills: Vec<DiscoveredSkill>,
}

impl<'a> DiscoverySelector<'a> {
    fn new(inventory: &'a DiscoveryInventory, options: DiscoverOptions) -> Self {
        let mut documents = BTreeMap::new();
        for document in &inventory.skill_documents {
            let relative = PathBuf::from(&document.relative_path);
            let Some(file_name) = relative.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            if !file_name.eq_ignore_ascii_case("SKILL.md") || !is_safe_relative_path(&relative) {
                continue;
            }
            let directory = relative.parent().unwrap_or_else(|| Path::new(""));
            let replace = documents
                .get(directory)
                .is_some_and(|existing: &&DiscoveryDocument| {
                    existing.relative_path.rsplit('/').next() != Some("SKILL.md")
                        && file_name == "SKILL.md"
                });
            if replace || !documents.contains_key(directory) {
                documents.insert(directory.to_path_buf(), document);
            }
        }
        let plugin_groupings = get_relative_plugin_groupings(
            inventory.marketplace_document.as_deref(),
            inventory.plugin_document.as_deref(),
        );
        let plugin_search_dirs = get_relative_plugin_search_dirs(
            inventory.marketplace_document.as_deref(),
            inventory.plugin_document.as_deref(),
        );
        let locked_names = locked_project_skill_names(inventory.local_lock_document.as_deref());
        Self {
            inventory,
            options,
            documents,
            plugin_groupings,
            plugin_search_dirs,
            locked_names,
            seen_names: HashSet::new(),
            skills: Vec::new(),
        }
    }

    fn select(mut self) -> Result<Vec<DiscoveredSkill>, AppError> {
        if self.try_add(Path::new("")) == CandidateResult::Added && !self.options.full_depth {
            return Ok(self.skills);
        }

        let mut priority_dirs = get_priority_search_dir_specs(Path::new(""));
        priority_dirs.extend(
            self.plugin_search_dirs
                .iter()
                .cloned()
                .map(|path| PrioritySearchDir { path, max_depth: 1 }),
        );
        for priority in priority_dirs {
            self.walk_priority_dir(&priority.path, priority.max_depth, 1);
        }

        if self.skills.is_empty() || self.options.full_depth {
            let fallback = self
                .documents
                .keys()
                .filter(|directory| {
                    directory.components().count() <= MAX_DEPTH
                        && !directory
                            .components()
                            .filter_map(|component| component.as_os_str().to_str())
                            .any(|name| SKIP_DIRS.contains(&name))
                })
                .cloned()
                .collect::<Vec<_>>();
            for directory in fallback {
                self.try_add(&directory);
            }
        }

        Ok(self.skills)
    }

    fn walk_priority_dir(&mut self, parent: &Path, max_depth: usize, depth: usize) {
        for child in self.immediate_child_dirs(parent) {
            if self.try_add(&child) != CandidateResult::Missing
                || depth >= max_depth
                || is_skipped_directory(&child)
            {
                continue;
            }
            self.walk_priority_dir(&child, max_depth, depth + 1);
        }
    }

    fn immediate_child_dirs(&self, parent: &Path) -> Vec<PathBuf> {
        let mut children = BTreeSet::new();
        for directory in self.documents.keys() {
            let Ok(relative) = directory.strip_prefix(parent) else {
                continue;
            };
            let mut components = relative.components();
            let Some(first) = components.next() else {
                continue;
            };
            children.insert(parent.join(first.as_os_str()));
        }
        children.into_iter().collect()
    }

    fn try_add(&mut self, directory: &Path) -> CandidateResult {
        let Some(document) = self.documents.get(directory).copied() else {
            return CandidateResult::Missing;
        };
        let Ok(content) = std::str::from_utf8(&document.content) else {
            return CandidateResult::Present;
        };
        let Ok(parsed) = parse_skill_md_content(content) else {
            return CandidateResult::Present;
        };
        let internal = parsed
            .metadata
            .as_ref()
            .is_some_and(|metadata| metadata.internal);
        if internal && !self.options.include_internal {
            return CandidateResult::Present;
        }

        let relative_directory = self.inventory.search_prefix.join(directory);
        if is_in_cli_agent_project_skill_dir(&relative_directory)
            && is_locked_project_skill(&parsed.name, &relative_directory, &self.locked_names)
        {
            return CandidateResult::Present;
        }
        if !self.seen_names.insert(parsed.name.clone()) {
            return CandidateResult::Present;
        }

        let relative_path = self
            .inventory
            .search_prefix
            .join(&document.relative_path)
            .to_string_lossy()
            .replace('\\', "/");
        self.skills.push(DiscoveredSkill {
            install_dir_name: sanitize_name(&parsed.name),
            name: parsed.name,
            description: parsed.description,
            relative_path,
            plugin_name: self.plugin_groupings.get(directory).cloned(),
            internal,
        });
        CandidateResult::Added
    }
}

fn locked_project_skill_names(document: Option<&str>) -> HashSet<String> {
    let Some(document) = document else {
        return HashSet::new();
    };
    let Ok(lock) = serde_json::from_str::<serde_json::Value>(document) else {
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

fn is_locked_project_skill(
    skill_name: &str,
    relative_directory: &Path,
    locked_names: &HashSet<String>,
) -> bool {
    locked_names.contains(&normalize_skill_name_for_lock_match(skill_name))
        || relative_directory
            .file_name()
            .and_then(|name| name.to_str())
            .map(|name| locked_names.contains(&normalize_skill_name_for_lock_match(name)))
            .unwrap_or(false)
}

fn is_in_cli_agent_project_skill_dir(relative_path: &Path) -> bool {
    cli_project_discovery_dirs()
        .iter()
        .any(|agent_dir| relative_path.starts_with(agent_dir))
}

fn is_skipped_directory(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| SKIP_DIRS.contains(&name))
}

fn is_safe_relative_path(path: &Path) -> bool {
    !path.is_absolute()
        && path.components().all(|component| {
            matches!(
                component,
                std::path::Component::Normal(_) | std::path::Component::CurDir
            )
        })
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
    fn test_discover_skills_honors_default_container_depth_three() {
        let temp = tempdir().unwrap();
        let shallow_dir = temp.path().join("skills/shallow");
        let skill_dir = temp.path().join("skills/specialized/database/demo");
        let too_deep_dir = temp.path().join("skills/too/deep/default/demo");
        fs::create_dir_all(&shallow_dir).unwrap();
        fs::create_dir_all(&skill_dir).unwrap();
        fs::create_dir_all(&too_deep_dir).unwrap();
        fs::write(
            shallow_dir.join("SKILL.md"),
            "---\nname: shallow-demo\ndescription: Shallow\n---\n",
        )
        .unwrap();
        fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: catalog-demo\ndescription: Demo\n---\n",
        )
        .unwrap();
        fs::write(
            too_deep_dir.join("SKILL.md"),
            "---\nname: deep-demo\ndescription: Deep\n---\n",
        )
        .unwrap();

        let skills = discover_skills(temp.path(), None, DiscoverOptions::default()).unwrap();

        assert_eq!(
            skills
                .iter()
                .map(|skill| (skill.name.as_str(), skill.relative_path.as_str()))
                .collect::<Vec<_>>(),
            vec![
                ("shallow-demo", "skills/shallow/SKILL.md"),
                ("catalog-demo", "skills/specialized/database/demo/SKILL.md"),
            ]
        );

        let mut full_depth = discover_skills(
            temp.path(),
            None,
            DiscoverOptions {
                full_depth: true,
                ..DiscoverOptions::default()
            },
        )
        .unwrap()
        .into_iter()
        .map(|skill| skill.name)
        .collect::<Vec<_>>();
        full_depth.sort();
        assert_eq!(
            full_depth,
            vec!["catalog-demo", "deep-demo", "shallow-demo"]
        );
    }

    #[test]
    fn priority_search_boundaries_match_cli() {
        let skill = |relative_path: &str, name: &str| DiscoveryDocument {
            relative_path: relative_path.to_string(),
            content: format!("---\nname: {name}\ndescription: Test skill\n---\n").into_bytes(),
        };
        let inventory = DiscoveryInventory {
            search_prefix: PathBuf::new(),
            skill_documents: vec![
                skill("direct/SKILL.md", "root-direct"),
                skill("examples/category/hidden/SKILL.md", "root-nested"),
                skill("plugins/catalog/direct/SKILL.md", "plugin-direct"),
                skill("plugins/catalog/category/hidden/SKILL.md", "plugin-nested"),
                skill("skills/node_modules/hidden/SKILL.md", "skipped"),
            ],
            marketplace_document: None,
            plugin_document: Some(
                r#"{"name":"demo","skills":["./plugins/catalog/direct"]}"#.to_string(),
            ),
            local_lock_document: None,
        };

        let skills = select_discovered_skills(&inventory, DiscoverOptions::default()).unwrap();

        assert_eq!(
            skills
                .iter()
                .map(|skill| skill.name.as_str())
                .collect::<Vec<_>>(),
            vec!["root-direct", "plugin-direct"]
        );
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
    fn invalid_skill_in_priority_container_stops_nested_discovery() {
        let temp = tempdir().unwrap();
        fs::create_dir_all(temp.path().join("skills/broken/inner")).unwrap();
        fs::create_dir_all(temp.path().join("skills/visible")).unwrap();
        fs::write(
            temp.path().join("skills/broken/SKILL.md"),
            "not frontmatter",
        )
        .unwrap();
        fs::write(
            temp.path().join("skills/broken/inner/SKILL.md"),
            "---\nname: hidden\ndescription: Hidden\n---\n",
        )
        .unwrap();
        fs::write(
            temp.path().join("skills/visible/SKILL.md"),
            "---\nname: visible\ndescription: Visible\n---\n",
        )
        .unwrap();

        let skills = discover_skills(temp.path(), None, DiscoverOptions::default()).unwrap();

        assert_eq!(
            skills
                .iter()
                .map(|skill| skill.name.as_str())
                .collect::<Vec<_>>(),
            vec!["visible"]
        );
    }

    #[test]
    fn test_discover_skills_falls_back_to_examples_when_priority_is_empty() {
        let temp = tempdir().unwrap();
        let skill_dir = temp.path().join("examples/product/demo");
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: example-demo\ndescription: Demo\n---\n",
        )
        .unwrap();

        let skills = discover_skills(temp.path(), None, DiscoverOptions::default()).unwrap();

        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].name, "example-demo");
    }

    #[test]
    fn invalid_root_skill_continues_with_recursive_fallback() {
        let temp = tempdir().unwrap();
        fs::write(temp.path().join("SKILL.md"), "not frontmatter").unwrap();
        let nested = temp.path().join("examples/catalog/demo");
        fs::create_dir_all(&nested).unwrap();
        fs::write(
            nested.join("SKILL.md"),
            "---\nname: fallback-demo\ndescription: Fallback demo\n---\n",
        )
        .unwrap();

        let skills = discover_skills(temp.path(), None, DiscoverOptions::default()).unwrap();

        assert_eq!(
            skills
                .iter()
                .map(|skill| skill.name.as_str())
                .collect::<Vec<_>>(),
            vec!["fallback-demo"]
        );
    }

    #[test]
    fn internal_root_skill_continues_with_recursive_fallback() {
        let temp = tempdir().unwrap();
        fs::write(
            temp.path().join("SKILL.md"),
            "---\nname: internal-root\ndescription: Internal\nmetadata:\n  internal: true\n---\n",
        )
        .unwrap();
        let nested = temp.path().join("examples/demo");
        fs::create_dir_all(&nested).unwrap();
        fs::write(
            nested.join("SKILL.md"),
            "---\nname: public-demo\ndescription: Public\n---\n",
        )
        .unwrap();

        let skills = discover_skills(temp.path(), None, DiscoverOptions::default()).unwrap();

        assert_eq!(
            skills
                .iter()
                .map(|skill| skill.name.as_str())
                .collect::<Vec<_>>(),
            vec!["public-demo"]
        );
    }

    #[test]
    fn recursive_fallback_uses_cli_directory_depth_boundary() {
        let temp = tempdir().unwrap();
        let depth_five = temp.path().join("one/two/three/four/five");
        let depth_six = depth_five.join("six");
        fs::create_dir_all(&depth_six).unwrap();
        fs::write(
            depth_five.join("SKILL.md"),
            "---\nname: depth-five\ndescription: Included\n---\n",
        )
        .unwrap();
        fs::write(
            depth_six.join("SKILL.md"),
            "---\nname: depth-six\ndescription: Excluded\n---\n",
        )
        .unwrap();

        let skills = discover_skills(temp.path(), None, DiscoverOptions::default()).unwrap();

        assert_eq!(
            skills
                .iter()
                .map(|skill| skill.name.as_str())
                .collect::<Vec<_>>(),
            vec!["depth-five"]
        );
    }

    #[cfg(unix)]
    #[test]
    fn recursive_fallback_does_not_follow_directory_symlinks() {
        use std::os::unix::fs::symlink;

        let temp = tempdir().unwrap();
        let outside = tempdir().unwrap();
        fs::create_dir_all(outside.path().join("demo")).unwrap();
        fs::write(
            outside.path().join("demo/SKILL.md"),
            "---\nname: linked-demo\ndescription: Linked\n---\n",
        )
        .unwrap();
        symlink(outside.path(), temp.path().join("linked-catalog")).unwrap();

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

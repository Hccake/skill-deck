//! Plugin Manifest 解析模块
//!
//! 对应 CLI: plugin-manifest.ts 的 getPluginGroupings() 函数
//!
//! 从 .claude-plugin/ 目录读取 marketplace.json 和 plugin.json，
//! 构建 skill 目录路径到 plugin 名称的映射。

use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};

/// marketplace.json 中的单个 plugin 条目
#[derive(serde::Deserialize)]
struct PluginManifestEntry {
    #[serde(default)]
    source: Option<serde_json::Value>,
    #[serde(default)]
    skills: Option<Vec<String>>,
    #[serde(default)]
    name: Option<String>,
}

/// marketplace.json 顶层结构
#[derive(serde::Deserialize)]
struct MarketplaceManifest {
    #[serde(default)]
    metadata: Option<MarketplaceMetadata>,
    #[serde(default)]
    plugins: Option<Vec<PluginManifestEntry>>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct MarketplaceMetadata {
    #[serde(default)]
    plugin_root: Option<String>,
}

/// plugin.json 顶层结构（单 plugin）
#[derive(serde::Deserialize)]
struct PluginManifest {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    skills: Option<Vec<String>>,
}

/// 验证路径是否以 "./" 开头（Claude Code 约定）
fn is_valid_relative_path(path: &str) -> bool {
    path.starts_with("./")
}

/// 验证 child 路径是否在 parent 之内（防止路径遍历攻击）
#[cfg(test)]
fn is_contained_in(child: &Path, parent: &Path) -> bool {
    match (child.canonicalize(), parent.canonicalize()) {
        (Ok(c), Ok(p)) => c.starts_with(&p),
        // 如果 canonicalize 失败（路径不存在），使用 normalize 比较
        _ => {
            let child_str = child.to_string_lossy().replace('\\', "/");
            let parent_str = parent.to_string_lossy().replace('\\', "/");
            child_str.starts_with(&parent_str)
        }
    }
}

/// 从 .claude-plugin/ 目录获取 plugin 分组映射
///
/// 对应 CLI: getPluginGroupings() (plugin-manifest.ts)
///
/// 返回 HashMap<PathBuf, String>：skill 目录绝对路径 → plugin 名称
#[cfg(test)]
pub fn get_plugin_groupings(base_path: &Path) -> HashMap<PathBuf, String> {
    let marketplace =
        std::fs::read_to_string(base_path.join(".claude-plugin/marketplace.json")).ok();
    let plugin = std::fs::read_to_string(base_path.join(".claude-plugin/plugin.json")).ok();
    get_relative_plugin_groupings(marketplace.as_deref(), plugin.as_deref())
        .into_iter()
        .filter_map(|(relative, name)| {
            let skill_dir = base_path.join(relative);
            is_contained_in(&skill_dir, base_path).then(|| (normalize_path(&skill_dir), name))
        })
        .collect()
}

/// 解析 plugin manifest，并返回相对于 Source root 的 Skill 路径映射。
///
/// 该函数不访问 filesystem，因此 Host 与 WSL discovery 可以共用同一套规则。
pub fn get_relative_plugin_groupings(
    marketplace_document: Option<&str>,
    plugin_document: Option<&str>,
) -> HashMap<PathBuf, String> {
    let mut groupings = HashMap::new();

    if let Some(manifest) = marketplace_document
        .and_then(|content| serde_json::from_str::<MarketplaceManifest>(content).ok())
    {
        let plugin_root_value = manifest
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.plugin_root.as_deref());
        let (plugin_root, plugin_root_valid) = match plugin_root_value {
            Some(path) => match safe_manifest_path(path) {
                Ok(path) => (Some(path), true),
                Err(()) => (None, false),
            },
            None => (None, true),
        };

        if plugin_root_valid {
            for plugin in manifest.plugins.unwrap_or_default() {
                let Some(plugin_name) = plugin.name else {
                    continue;
                };
                let source = match plugin.source {
                    Some(serde_json::Value::String(path)) => match safe_manifest_path(&path) {
                        Ok(path) => Some(path),
                        Err(()) => continue,
                    },
                    None => None,
                    Some(_) => continue,
                };
                let mut plugin_base = plugin_root.clone().unwrap_or_default();
                if let Some(source) = source {
                    plugin_base.push(source);
                }
                for skill in plugin.skills.unwrap_or_default() {
                    let Ok(skill) = safe_manifest_path(&skill) else {
                        continue;
                    };
                    groupings.insert(plugin_base.join(skill), plugin_name.clone());
                }
            }
        }
    }

    if let Some(manifest) =
        plugin_document.and_then(|content| serde_json::from_str::<PluginManifest>(content).ok())
    {
        if let (Some(name), Some(skills)) = (manifest.name, manifest.skills) {
            for skill in skills {
                if let Ok(skill) = safe_manifest_path(&skill) {
                    groupings.insert(skill, name.clone());
                }
            }
        }
    }

    groupings
}

/// 返回 plugin manifest 声明的相对搜索目录。
///
/// 与 skills CLI 一致：显式 Skill 路径转换为其 parent directory，
/// 同时为每个 local plugin base 增加约定的 `skills/` 目录。
pub fn get_relative_plugin_search_dirs(
    marketplace_document: Option<&str>,
    plugin_document: Option<&str>,
) -> Vec<PathBuf> {
    let mut search_dirs = Vec::new();

    let mut add_plugin = |plugin_base: PathBuf, skills: Option<Vec<String>>| {
        if let Some(skills) = skills {
            for skill in skills {
                let Ok(skill) = safe_manifest_path(&skill) else {
                    continue;
                };
                if let Some(parent) = plugin_base.join(skill).parent() {
                    search_dirs.push(parent.to_path_buf());
                }
            }
        }
        search_dirs.push(plugin_base.join("skills"));
    };

    if let Some(manifest) = marketplace_document
        .and_then(|content| serde_json::from_str::<MarketplaceManifest>(content).ok())
    {
        let plugin_root = match manifest
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.plugin_root.as_deref())
        {
            Some(path) => safe_manifest_path(path).ok(),
            None => Some(PathBuf::new()),
        };
        if let Some(plugin_root) = plugin_root {
            for plugin in manifest.plugins.unwrap_or_default() {
                let source = match plugin.source {
                    Some(serde_json::Value::String(path)) => match safe_manifest_path(&path) {
                        Ok(path) => path,
                        Err(()) => continue,
                    },
                    None => PathBuf::new(),
                    Some(_) => continue,
                };
                add_plugin(plugin_root.join(source), plugin.skills);
            }
        }
    }

    if let Some(manifest) =
        plugin_document.and_then(|content| serde_json::from_str::<PluginManifest>(content).ok())
    {
        add_plugin(PathBuf::new(), manifest.skills);
    }

    let mut seen = std::collections::HashSet::new();
    search_dirs.retain(|path| seen.insert(path.clone()));
    search_dirs
}

fn safe_manifest_path(path: &str) -> Result<PathBuf, ()> {
    if !is_valid_relative_path(path) {
        return Err(());
    }
    let mut normalized = PathBuf::new();
    for component in Path::new(path).components() {
        match component {
            Component::CurDir => {}
            Component::Normal(value) => normalized.push(value),
            _ => return Err(()),
        }
    }
    Ok(normalized)
}

/// 简单的路径规范化（不要求路径存在）
#[cfg(test)]
pub fn normalize_path(path: &Path) -> PathBuf {
    match path.canonicalize() {
        Ok(p) => p,
        Err(_) => {
            // 路径不存在时，手动拼接为绝对路径
            if path.is_absolute() {
                path.to_path_buf()
            } else {
                std::env::current_dir().unwrap_or_default().join(path)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_marketplace_json_groupings() {
        let temp = tempdir().unwrap();
        let base = temp.path();

        // 创建 .claude-plugin/marketplace.json
        let plugin_dir = base.join(".claude-plugin");
        fs::create_dir_all(&plugin_dir).unwrap();

        let manifest = r#"{
            "metadata": { "pluginRoot": "./" },
            "plugins": [
                {
                    "name": "doc-skills",
                    "source": "./docs",
                    "skills": ["./pdf-reader", "./md-tools"]
                }
            ]
        }"#;
        fs::write(plugin_dir.join("marketplace.json"), manifest).unwrap();

        // 创建对应的 skill 目录
        fs::create_dir_all(base.join("docs/pdf-reader")).unwrap();
        fs::create_dir_all(base.join("docs/md-tools")).unwrap();

        let groupings = get_plugin_groupings(base);
        assert_eq!(groupings.len(), 2);

        // 验证映射关系
        let pdf_path = normalize_path(&base.join("docs/pdf-reader"));
        let md_path = normalize_path(&base.join("docs/md-tools"));
        assert_eq!(groupings.get(&pdf_path), Some(&"doc-skills".to_string()));
        assert_eq!(groupings.get(&md_path), Some(&"doc-skills".to_string()));
    }

    #[test]
    fn test_plugin_json_groupings() {
        let temp = tempdir().unwrap();
        let base = temp.path();

        let plugin_dir = base.join(".claude-plugin");
        fs::create_dir_all(&plugin_dir).unwrap();

        let manifest = r#"{
            "name": "my-plugin",
            "skills": ["./skill-a", "./skill-b"]
        }"#;
        fs::write(plugin_dir.join("plugin.json"), manifest).unwrap();

        // 创建 skill 目录
        fs::create_dir_all(base.join("skill-a")).unwrap();
        fs::create_dir_all(base.join("skill-b")).unwrap();

        let groupings = get_plugin_groupings(base);
        assert_eq!(groupings.len(), 2);

        let a_path = normalize_path(&base.join("skill-a"));
        assert_eq!(groupings.get(&a_path), Some(&"my-plugin".to_string()));
    }

    #[test]
    fn test_no_manifest_returns_empty() {
        let temp = tempdir().unwrap();
        let groupings = get_plugin_groupings(temp.path());
        assert!(groupings.is_empty());
    }

    #[test]
    fn test_invalid_relative_paths_skipped() {
        let temp = tempdir().unwrap();
        let base = temp.path();

        let plugin_dir = base.join(".claude-plugin");
        fs::create_dir_all(&plugin_dir).unwrap();

        // 不以 "./" 开头的路径应被跳过
        let manifest = r#"{
            "name": "bad-plugin",
            "skills": ["../escape", "no-dot-slash"]
        }"#;
        fs::write(plugin_dir.join("plugin.json"), manifest).unwrap();

        let groupings = get_plugin_groupings(base);
        assert!(groupings.is_empty());
    }

    #[test]
    fn manifest_documents_produce_environment_independent_relative_groupings() {
        let marketplace = r#"{
            "metadata": { "pluginRoot": "./plugins" },
            "plugins": [{
                "name": "docs",
                "source": "./toolkit",
                "skills": ["./skills/pdf", "../outside"]
            }]
        }"#;
        let plugin = r#"{
            "name": "root-plugin",
            "skills": ["./skills/root"]
        }"#;

        let groupings = get_relative_plugin_groupings(Some(marketplace), Some(plugin));

        assert_eq!(
            groupings.get(&PathBuf::from("plugins/toolkit/skills/pdf")),
            Some(&"docs".to_string())
        );
        assert_eq!(
            groupings.get(&PathBuf::from("skills/root")),
            Some(&"root-plugin".to_string())
        );
        assert_eq!(groupings.len(), 2);
    }

    #[test]
    fn manifest_documents_produce_cli_priority_search_directories() {
        let marketplace = r#"{
            "metadata": { "pluginRoot": "./plugins" },
            "plugins": [{
                "source": "./toolkit",
                "skills": ["./catalog/pdf", "../outside"]
            }, {
                "source": { "source": "remote/repo" },
                "skills": ["./ignored"]
            }]
        }"#;
        let plugin = r#"{"skills":["./skills/root"]}"#;

        let search_dirs = get_relative_plugin_search_dirs(Some(marketplace), Some(plugin));

        assert_eq!(
            search_dirs,
            vec![
                PathBuf::from("plugins/toolkit/catalog"),
                PathBuf::from("plugins/toolkit/skills"),
                PathBuf::from("skills"),
            ]
        );
    }
}

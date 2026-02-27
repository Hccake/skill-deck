//! Plugin Manifest 解析模块
//!
//! 对应 CLI: plugin-manifest.ts 的 getPluginGroupings() 函数
//!
//! 从 .claude-plugin/ 目录读取 marketplace.json 和 plugin.json，
//! 构建 skill 目录路径到 plugin 名称的映射。

use std::collections::HashMap;
use std::path::{Path, PathBuf};

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
pub fn get_plugin_groupings(base_path: &Path) -> HashMap<PathBuf, String> {
    let mut groupings = HashMap::new();

    // 尝试 marketplace.json（多 plugin 目录）
    let marketplace_path = base_path.join(".claude-plugin/marketplace.json");
    if let Ok(content) = std::fs::read_to_string(&marketplace_path) {
        if let Ok(manifest) = serde_json::from_str::<MarketplaceManifest>(&content) {
            let plugin_root = manifest.metadata.as_ref().and_then(|m| m.plugin_root.clone());

            // 验证 pluginRoot 以 "./" 开头（如果提供）
            let valid_plugin_root = match &plugin_root {
                None => true,
                Some(pr) => is_valid_relative_path(pr),
            };

            if valid_plugin_root {
                for plugin in manifest.plugins.unwrap_or_default() {
                    let plugin_name = match &plugin.name {
                        Some(n) => n,
                        None => continue,
                    };

                    // 跳过远程来源（对象格式 {source, repo}），只处理本地字符串路径
                    let source_str = match &plugin.source {
                        Some(serde_json::Value::String(s)) => Some(s.as_str()),
                        None => None,
                        _ => continue, // 对象格式，跳过
                    };

                    // 验证 source 以 "./" 开头
                    if let Some(s) = source_str {
                        if !is_valid_relative_path(s) {
                            continue;
                        }
                    }

                    let plugin_base = match (&plugin_root, source_str) {
                        (Some(pr), Some(s)) => base_path.join(pr).join(s),
                        (Some(pr), None) => base_path.join(pr),
                        (None, Some(s)) => base_path.join(s),
                        (None, None) => base_path.to_path_buf(),
                    };

                    // 验证 plugin_base 在 base_path 内
                    if !is_contained_in(&plugin_base, base_path) {
                        continue;
                    }

                    for skill_path in plugin.skills.unwrap_or_default() {
                        if !is_valid_relative_path(&skill_path) {
                            continue;
                        }
                        let skill_dir = plugin_base.join(&skill_path);
                        if is_contained_in(&skill_dir, base_path) {
                            // 使用 normalize 后的绝对路径作为 key
                            let abs_path = normalize_path(&skill_dir);
                            groupings.insert(abs_path, plugin_name.clone());
                        }
                    }
                }
            }
        }
    }

    // 尝试 plugin.json（单 plugin）
    let plugin_path = base_path.join(".claude-plugin/plugin.json");
    if let Ok(content) = std::fs::read_to_string(&plugin_path) {
        if let Ok(manifest) = serde_json::from_str::<PluginManifest>(&content) {
            if let (Some(name), Some(skills)) = (manifest.name, manifest.skills) {
                for skill_path in skills {
                    if !is_valid_relative_path(&skill_path) {
                        continue;
                    }
                    let skill_dir = base_path.join(&skill_path);
                    if is_contained_in(&skill_dir, base_path) {
                        let abs_path = normalize_path(&skill_dir);
                        groupings.insert(abs_path, name.clone());
                    }
                }
            }
        }
    }

    groupings
}

/// 简单的路径规范化（不要求路径存在）
pub fn normalize_path(path: &Path) -> PathBuf {
    match path.canonicalize() {
        Ok(p) => p,
        Err(_) => {
            // 路径不存在时，手动拼接为绝对路径
            if path.is_absolute() {
                path.to_path_buf()
            } else {
                std::env::current_dir()
                    .unwrap_or_default()
                    .join(path)
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
}

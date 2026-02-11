// Skill 解析逻辑

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

use super::agents::AgentType;
use super::paths::canonical_skills_dir;
use super::skill_lock::{get_skill_from_lock, SkillLockEntry};
use crate::error::AppError;

/// Skill 元数据
/// 对应 CLI: Skill (types.ts:42-49)
#[derive(Debug, Clone, Deserialize)]
pub struct SkillMetadata {
    #[serde(default)]
    pub internal: bool,
}

/// SKILL.md frontmatter 结构
/// 对应 CLI: parseSkillMd 返回的数据结构
#[derive(Debug, Clone, Deserialize)]
pub struct SkillFrontmatter {
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub metadata: Option<SkillMetadata>,
}

/// 解析 SKILL.md 文件
/// 对应 CLI: parseSkillMd (skills.ts:28-58)
pub fn parse_skill_md(path: &Path) -> Result<SkillFrontmatter, AppError> {
    let content = std::fs::read_to_string(path)?;

    // 检查是否以 --- 开头
    if !content.starts_with("---") {
        return Err(AppError::InvalidSkillMd(
            "Missing frontmatter delimiter".to_string(),
        ));
    }

    // 找到第二个 ---
    let rest = &content[3..];
    let end_pos = rest.find("---").ok_or_else(|| {
        AppError::InvalidSkillMd("Unclosed frontmatter delimiter".to_string())
    })?;

    // 提取 YAML 部分（跳过开头的换行符）
    let yaml_content = rest[..end_pos].trim();

    // 解析 YAML
    let frontmatter: SkillFrontmatter = serde_yaml::from_str(yaml_content)?;

    // 验证必填字段
    if frontmatter.name.is_empty() {
        return Err(AppError::InvalidSkillMd("Missing name field".to_string()));
    }
    if frontmatter.description.is_empty() {
        return Err(AppError::InvalidSkillMd(
            "Missing description field".to_string(),
        ));
    }

    Ok(frontmatter)
}

/// Sanitize skill 名称
/// 对应 CLI: sanitizeName (installer.ts:39-54)
pub fn sanitize_name(name: &str) -> String {
    let sanitized: String = name
        .to_lowercase()
        .chars()
        .map(|c| {
            if c.is_ascii_lowercase() || c.is_ascii_digit() || c == '.' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect();

    // 合并连续的 -
    let mut result = String::new();
    let mut prev_hyphen = false;
    for c in sanitized.chars() {
        if c == '-' {
            if !prev_hyphen {
                result.push(c);
            }
            prev_hyphen = true;
        } else {
            result.push(c);
            prev_hyphen = false;
        }
    }

    // 去除首尾的 . 和 -
    let trimmed = result.trim_matches(|c| c == '.' || c == '-');

    if trimmed.is_empty() {
        "unnamed-skill".to_string()
    } else {
        trimmed.chars().take(255).collect()
    }
}

/// Skill 范围
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SkillScope {
    Global,
    Project,
}

/// 已安装的 Skill 信息
/// 对应 CLI: InstalledSkill (installer.ts:783-790)
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstalledSkill {
    pub name: String,
    pub description: String,
    pub path: String,
    pub canonical_path: String,
    pub scope: SkillScope,
    pub agents: Vec<AgentType>,
    // 来自 skill-lock.json 的元数据
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub installed_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub has_update: Option<bool>,
}

impl InstalledSkill {
    /// 从 lock entry 填充元数据
    pub fn with_lock_entry(mut self, entry: Option<&SkillLockEntry>) -> Self {
        if let Some(e) = entry {
            self.source = Some(e.source.clone());
            self.source_url = Some(e.source_url.clone());
            self.installed_at = Some(e.installed_at.clone());
            self.updated_at = Some(e.updated_at.clone());
        }
        self
    }
}

/// list_skills 返回结果
/// 包含 skills 列表和路径存在性信息
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListSkillsResult {
    pub skills: Vec<InstalledSkill>,
    /// 项目目录是否存在（project scope 时有意义，global 始终为 true）
    pub path_exists: bool,
}

/// 扫描目录信息
struct ScanScope {
    global: bool,
    path: std::path::PathBuf,
    agent_type: Option<AgentType>,
}

/// 列出已安装的 skills
/// 对应 CLI: listInstalledSkills (installer.ts:797-1007)
pub fn list_installed_skills(
    scope: Option<SkillScope>,
    cwd: &str,
) -> Result<Vec<InstalledSkill>, AppError> {
    let mut skills_map: HashMap<String, InstalledSkill> = HashMap::new();
    let mut scopes: Vec<ScanScope> = Vec::new();

    // 检测已安装的 agents
    let detected_agents = AgentType::detect_installed();

    // 确定要扫描的 scope 类型
    let scope_types: Vec<bool> = match scope {
        Some(SkillScope::Global) => vec![true],
        Some(SkillScope::Project) => vec![false],
        None => vec![false, true], // 默认扫描 project 和 global
    };

    // 构建扫描目录列表
    // 对应 CLI: installer.ts 第 843-859 行
    for is_global in &scope_types {
        // 添加 canonical 目录
        scopes.push(ScanScope {
            global: *is_global,
            path: canonical_skills_dir(*is_global, cwd),
            agent_type: None,
        });

        // 添加每个已安装 agent 的 skills 目录
        for agent_type in &detected_agents {
            let config = agent_type.config();

            // 跳过不支持 global 安装的 agent
            if *is_global && config.global_skills_dir.is_none() {
                continue;
            }

            let agent_dir = if *is_global {
                config.global_skills_dir.clone().unwrap()
            } else {
                std::path::PathBuf::from(cwd).join(config.skills_dir)
            };

            // 避免重复路径
            if !scopes.iter().any(|s| s.path == agent_dir && s.global == *is_global) {
                scopes.push(ScanScope {
                    global: *is_global,
                    path: agent_dir,
                    agent_type: Some(*agent_type),
                });
            }
        }
    }

    // 遍历每个扫描目录
    // 对应 CLI: installer.ts 第 861-1004 行
    for scope_info in &scopes {
        let entries = match std::fs::read_dir(&scope_info.path) {
            Ok(e) => e,
            Err(_) => continue, // 目录不存在，跳过
        };

        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();

            // 只处理目录
            if !path.is_dir() {
                continue;
            }

            let skill_md_path = path.join("SKILL.md");

            // 检查 SKILL.md 是否存在
            if !skill_md_path.exists() {
                continue;
            }

            // 解析 SKILL.md
            let frontmatter = match parse_skill_md(&skill_md_path) {
                Ok(f) => f,
                Err(_) => continue,
            };

            // 跳过 internal skills
            if frontmatter
                .metadata
                .as_ref()
                .map(|m| m.internal)
                .unwrap_or(false)
            {
                continue;
            }

            let scope_key = if scope_info.global { "global" } else { "project" };
            let skill_key = format!("{}:{}", scope_key, frontmatter.name);

            // 如果是 agent 特定目录，直接归属于该 agent
            if let Some(agent_type) = scope_info.agent_type {
                if let Some(existing) = skills_map.get_mut(&skill_key) {
                    if !existing.agents.contains(&agent_type) {
                        existing.agents.push(agent_type);
                    }
                } else {
                    let lock_entry = get_skill_from_lock(&frontmatter.name).ok().flatten();
                    let skill = InstalledSkill {
                        name: frontmatter.name.clone(),
                        description: frontmatter.description,
                        path: path.to_string_lossy().to_string(),
                        canonical_path: path.to_string_lossy().to_string(),
                        scope: if scope_info.global {
                            SkillScope::Global
                        } else {
                            SkillScope::Project
                        },
                        agents: vec![agent_type],
                        source: None,
                        source_url: None,
                        installed_at: None,
                        updated_at: None,
                        has_update: None,
                    }
                    .with_lock_entry(lock_entry.as_ref());

                    skills_map.insert(skill_key, skill);
                }
                continue;
            }

            // 对于 canonical 目录，检查哪些 agents 安装了这个 skill
            // 对应 CLI: installer.ts 第 911-980 行
            let sanitized_name = sanitize_name(&frontmatter.name);
            let dir_name = path.file_name().unwrap().to_string_lossy().to_string();
            let mut installed_agents: Vec<AgentType> = Vec::new();

            for agent_type in &detected_agents {
                let config = agent_type.config();

                if scope_info.global && config.global_skills_dir.is_none() {
                    continue;
                }

                let agent_base = if scope_info.global {
                    config.global_skills_dir.clone().unwrap()
                } else {
                    std::path::PathBuf::from(cwd).join(config.skills_dir)
                };

                // 尝试多种目录名匹配
                // 对应 CLI: installer.ts 第 925-947 行
                let possible_names: Vec<&str> = vec![
                    &dir_name,
                    &sanitized_name,
                ];

                let mut found = false;
                for possible_name in &possible_names {
                    let agent_skill_dir = agent_base.join(possible_name);
                    if agent_skill_dir.exists() {
                        found = true;
                        break;
                    }
                }

                // Fallback: 扫描目录并比对 SKILL.md 中的 name
                // 对应 CLI: installer.ts 第 951-975 行
                if !found {
                    if let Ok(agent_entries) = std::fs::read_dir(&agent_base) {
                        for agent_entry in agent_entries.filter_map(|e| e.ok()) {
                            let candidate_path = agent_entry.path();
                            if !candidate_path.is_dir() {
                                continue;
                            }

                            let candidate_skill_md = candidate_path.join("SKILL.md");
                            if let Ok(candidate_frontmatter) = parse_skill_md(&candidate_skill_md) {
                                if candidate_frontmatter.name == frontmatter.name {
                                    found = true;
                                    break;
                                }
                            }
                        }
                    }
                }

                if found {
                    installed_agents.push(*agent_type);
                }
            }

            // 更新或插入 skill
            if let Some(existing) = skills_map.get_mut(&skill_key) {
                for agent in installed_agents {
                    if !existing.agents.contains(&agent) {
                        existing.agents.push(agent);
                    }
                }
            } else {
                let lock_entry = get_skill_from_lock(&frontmatter.name).ok().flatten();
                let skill = InstalledSkill {
                    name: frontmatter.name,
                    description: frontmatter.description,
                    path: path.to_string_lossy().to_string(),
                    canonical_path: path.to_string_lossy().to_string(),
                    scope: if scope_info.global {
                        SkillScope::Global
                    } else {
                        SkillScope::Project
                    },
                    agents: installed_agents,
                    source: None,
                    source_url: None,
                    installed_at: None,
                    updated_at: None,
                    has_update: None,
                }
                .with_lock_entry(lock_entry.as_ref());

                skills_map.insert(skill_key, skill);
            }
        }
    }

    Ok(skills_map.into_values().collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_parse_valid_skill_md() {
        let content = r#"---
name: test-skill
description: A test skill
---

# Test Skill

Content here.
"#;
        let mut file = NamedTempFile::new().unwrap();
        file.write_all(content.as_bytes()).unwrap();

        let result = parse_skill_md(file.path()).unwrap();
        assert_eq!(result.name, "test-skill");
        assert_eq!(result.description, "A test skill");
    }

    #[test]
    fn test_parse_skill_md_with_metadata() {
        let content = r#"---
name: internal-skill
description: An internal skill
metadata:
  internal: true
---

Content.
"#;
        let mut file = NamedTempFile::new().unwrap();
        file.write_all(content.as_bytes()).unwrap();

        let result = parse_skill_md(file.path()).unwrap();
        assert_eq!(result.name, "internal-skill");
        assert!(result.metadata.unwrap().internal);
    }

    #[test]
    fn test_parse_missing_frontmatter() {
        let content = "# No frontmatter\n\nJust content.";
        let mut file = NamedTempFile::new().unwrap();
        file.write_all(content.as_bytes()).unwrap();

        let result = parse_skill_md(file.path());
        assert!(result.is_err());
    }

    #[test]
    fn test_sanitize_name_basic() {
        assert_eq!(sanitize_name("Hello World"), "hello-world");
        assert_eq!(sanitize_name("my_skill.v2"), "my_skill.v2");
        assert_eq!(sanitize_name("---test---"), "test");
        assert_eq!(sanitize_name("UPPERCASE"), "uppercase");
    }

    #[test]
    fn test_sanitize_name_special_chars() {
        assert_eq!(sanitize_name("skill@v1!"), "skill-v1");
        assert_eq!(sanitize_name("../path/traversal"), "path-traversal");
        assert_eq!(sanitize_name(""), "unnamed-skill");
    }

    #[test]
    fn test_sanitize_name_consecutive_hyphens() {
        assert_eq!(sanitize_name("a  b  c"), "a-b-c");
        assert_eq!(sanitize_name("a---b"), "a-b");
    }
}

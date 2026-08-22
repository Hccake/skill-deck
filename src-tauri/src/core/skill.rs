// Skill 解析逻辑

use super::agent_definition::AgentId;
use super::local_lock::LocalSkillLockEntry;
use super::skill_lock::SkillLockEntry;
use super::skill_paths::find_skill_md_case_insensitive;
use super::update_metadata::{
    derive_update_capability, normalize_global_lock_entry, normalize_local_lock_entry,
};
use crate::error::AppError;
use serde::{Deserialize, Serialize};
use specta::Type;

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

/// 解析已经由所属 Environment 读取的 SKILL.md 内容。
///
/// Discovery selector 通过该入口共享 Native 与 WSL 的 frontmatter 语义，
/// filesystem 访问仍由各自的 inventory collector 负责。
pub fn parse_skill_md_content(content: &str) -> Result<SkillFrontmatter, AppError> {
    // 检查是否以 --- 开头
    if !content.starts_with("---") {
        return Err(AppError::InvalidSkillMd {
            message: "Missing frontmatter delimiter".to_string(),
        });
    }

    // 找到第二个 ---
    let rest = &content[3..];
    let end_pos = rest.find("---").ok_or_else(|| AppError::InvalidSkillMd {
        message: "Unclosed frontmatter delimiter".to_string(),
    })?;

    // 提取 YAML 部分（跳过开头的换行符）
    let yaml_content = rest[..end_pos].trim();

    // 解析 YAML
    let frontmatter: SkillFrontmatter = serde_yaml::from_str(yaml_content)?;

    // 验证必填字段
    if frontmatter.name.is_empty() {
        return Err(AppError::InvalidSkillMd {
            message: "Missing name field".to_string(),
        });
    }
    if frontmatter.description.is_empty() {
        return Err(AppError::InvalidSkillMd {
            message: "Missing description field".to_string(),
        });
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

/// 已安装 Skill 的位置类型
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "lowercase")]
#[specta(rename_all = "lowercase")]
pub enum InstalledSkillLocation {
    Global,
    Project,
}

/// 已安装的 Skill 信息
/// 对应 CLI: InstalledSkill (installer.ts:783-790)
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct InstalledSkill {
    pub name: String,
    pub description: String,
    pub path: String,
    pub canonical_path: String,
    pub scope: InstalledSkillLocation,
    pub agents: Vec<AgentId>,
    /// 每次读取 Skill 时根据当前 runtime 和文件系统重新组装，不写入 skill-lock。
    /// 只包含当前已检测到并且实际能够读取该 Skill 的关联 Agent。
    pub associated_agents: Vec<AgentId>,
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
    /// 是否可直接执行更新
    #[serde(skip_serializing_if = "Option::is_none")]
    pub can_run_update: Option<bool>,
    /// 是否可自动检查更新
    #[serde(skip_serializing_if = "Option::is_none")]
    pub can_check_for_updates: Option<bool>,
    /// 更新能力缺失原因
    #[serde(skip_serializing_if = "Option::is_none")]
    pub update_reason: Option<String>,
    /// 所属 plugin 名称
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plugin_name: Option<String>,
    /// Git ref（branch/tag）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub git_ref: Option<String>,
    /// 来源仓库内的 Skill 相对路径，用于更新检查会话身份指纹
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skill_path: Option<String>,
    /// 默认可用 Agent 数量
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_available_agent_count: Option<u32>,
    /// 已独立适配 Agent 数量
    #[serde(skip_serializing_if = "Option::is_none")]
    pub private_adapted_agent_count: Option<u32>,
    /// 可清理的额外 Agent Skill 安装项数量（可能是链接或副本）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duplicate_copy_count: Option<u32>,
    /// 默认可用 Agents
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_available_agents: Option<Vec<AgentId>>,
    /// 需要/已有单独适配的 Agents
    #[serde(skip_serializing_if = "Option::is_none")]
    pub private_adapted_agents: Option<Vec<AgentId>>,
    /// 当前存在额外 Agent Skill 安装项的 Agents（可能是链接或副本）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duplicate_copy_agents: Option<Vec<AgentId>>,
    /// 当前只通过 Agent Skill 安装项使用的 Agents
    #[serde(skip_serializing_if = "Option::is_none")]
    pub private_only_agents: Option<Vec<AgentId>>,
    /// 需要额外保留到 Agent 专用 Skill 目录的默认可用 Agents
    #[serde(skip_serializing_if = "Option::is_none")]
    pub private_copy_agents: Option<Vec<AgentId>>,
}

impl InstalledSkill {
    /// 从 global lock entry 填充元数据
    pub fn with_lock_entry(mut self, entry: Option<&SkillLockEntry>) -> Self {
        if let Some(e) = entry {
            self.source = Some(e.source.clone());
            self.source_url = normalize_global_lock_entry(e).source_url;
            self.installed_at = Some(e.installed_at.clone());
            self.updated_at = Some(e.updated_at.clone());
            self.plugin_name = e.plugin_name.clone();
            self.git_ref = e.ref_name.clone();
            self.skill_path = e.skill_path.clone();

            let capability = derive_update_capability(&normalize_global_lock_entry(e));
            self.can_run_update = Some(capability.can_run_update);
            self.can_check_for_updates = Some(capability.can_check_for_updates);
            self.update_reason = capability.reason;
        }
        self
    }

    /// 从 local lock entry 填充元数据（项目级）
    pub fn with_local_lock_entry(mut self, entry: Option<&LocalSkillLockEntry>) -> Self {
        if let Some(e) = entry {
            let metadata = normalize_local_lock_entry(e);
            self.source = Some(e.source.clone());
            self.source_url = metadata.source_url.clone();
            self.plugin_name = e.plugin_name.clone();
            self.git_ref = e.ref_name.clone();
            self.skill_path = e.skill_path.clone();

            let capability = derive_update_capability(&metadata);
            self.can_run_update = Some(capability.can_run_update);
            self.can_check_for_updates = Some(capability.can_check_for_updates);
            self.update_reason = capability.reason;
        }
        self
    }
}

/// Read the markdown body of SKILL.md, stripping YAML frontmatter.
/// Takes the skill's canonical directory path.
pub fn read_skill_content(canonical_path: &str) -> Result<String, AppError> {
    let canonical_dir = std::path::Path::new(canonical_path);
    let skill_md = find_skill_md_case_insensitive(canonical_dir)
        .unwrap_or_else(|| canonical_dir.join("SKILL.md"));
    let content = std::fs::read_to_string(&skill_md).map_err(|_| AppError::PathNotFound {
        path: skill_md.to_string_lossy().to_string(),
    })?;

    Ok(skill_content_from_markdown(&content))
}

pub fn skill_content_from_markdown(content: &str) -> String {
    if let Some(stripped) = content.strip_prefix("---") {
        if let Some(end) = stripped.find("---") {
            let body_start = 3 + end + 3;
            return content[body_start..].trim_start_matches('\n').to_string();
        }
    }

    content.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_parse_valid_skill_md() {
        let content = r#"---
name: test-skill
description: A test skill
---

# Test Skill

Content here.
"#;
        let result = parse_skill_md_content(content).unwrap();
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
        let result = parse_skill_md_content(content).unwrap();
        assert_eq!(result.name, "internal-skill");
        assert!(result.metadata.unwrap().internal);
    }

    #[test]
    fn test_parse_missing_frontmatter() {
        let content = "# No frontmatter\n\nJust content.";
        let result = parse_skill_md_content(content);
        assert!(result.is_err());
    }

    #[test]
    fn skill_content_from_markdown_strips_frontmatter() {
        let content = "---\nname: toolkit\ndescription: Toolkit\n---\n\n# Toolkit\n";

        assert_eq!(skill_content_from_markdown(content), "# Toolkit\n");
        assert_eq!(skill_content_from_markdown("# Plain\n"), "# Plain\n");
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

    #[test]
    fn test_with_local_lock_entry_prefers_explicit_source_url() {
        let entry = LocalSkillLockEntry {
            source: "owner/private-repo".to_string(),
            ref_name: None,
            source_type: "github".to_string(),
            source_url: Some("git@github.com:owner/private-repo.git".to_string()),
            well_known_digest: None,
            computed_hash: "abc123".to_string(),
            remote_hash: None,
            skill_path: None,
            subagents: None,
            plugin_name: None,
        };
        let skill = InstalledSkill {
            name: "ssh-skill".to_string(),
            description: "SSH skill".to_string(),
            path: String::new(),
            canonical_path: String::new(),
            scope: InstalledSkillLocation::Project,
            agents: Vec::new(),
            associated_agents: Vec::new(),
            source: None,
            source_url: None,
            installed_at: None,
            updated_at: None,
            has_update: None,
            can_run_update: None,
            can_check_for_updates: None,
            update_reason: None,
            plugin_name: None,
            git_ref: None,
            skill_path: None,
            default_available_agent_count: None,
            private_adapted_agent_count: None,
            duplicate_copy_count: None,
            default_available_agents: None,
            private_adapted_agents: None,
            duplicate_copy_agents: None,
            private_only_agents: None,
            private_copy_agents: None,
        }
        .with_local_lock_entry(Some(&entry));

        assert_eq!(
            skill.source_url.as_deref(),
            Some("git@github.com:owner/private-repo.git")
        );
    }

    #[test]
    fn test_with_local_lock_entry_does_not_invent_source_url_for_empty_source() {
        let entry = LocalSkillLockEntry {
            source: String::new(),
            ref_name: None,
            source_type: "github".to_string(),
            source_url: None,
            well_known_digest: None,
            computed_hash: String::new(),
            remote_hash: Some("tree123".to_string()),
            skill_path: Some("skills/demo/SKILL.md".to_string()),
            subagents: None,
            plugin_name: None,
        };

        let skill = InstalledSkill {
            name: "demo".to_string(),
            description: "Demo".to_string(),
            path: String::new(),
            canonical_path: String::new(),
            scope: InstalledSkillLocation::Project,
            agents: Vec::new(),
            associated_agents: Vec::new(),
            source: None,
            source_url: None,
            installed_at: None,
            updated_at: None,
            has_update: None,
            can_run_update: None,
            can_check_for_updates: None,
            update_reason: None,
            plugin_name: None,
            git_ref: None,
            skill_path: None,
            default_available_agent_count: None,
            private_adapted_agent_count: None,
            duplicate_copy_count: None,
            default_available_agents: None,
            private_adapted_agents: None,
            duplicate_copy_agents: None,
            private_only_agents: None,
            private_copy_agents: None,
        }
        .with_local_lock_entry(Some(&entry));

        assert_eq!(skill.source_url, None);
        assert_eq!(skill.skill_path.as_deref(), Some("skills/demo/SKILL.md"));
    }

    #[test]
    fn test_installed_skill_runtime_update_capabilities_can_be_stored() {
        let skill = InstalledSkill {
            name: "demo".to_string(),
            description: "Demo".to_string(),
            path: String::new(),
            canonical_path: String::new(),
            scope: InstalledSkillLocation::Global,
            agents: Vec::new(),
            associated_agents: Vec::new(),
            source: None,
            source_url: None,
            installed_at: None,
            updated_at: None,
            has_update: Some(false),
            can_run_update: Some(true),
            can_check_for_updates: Some(false),
            update_reason: Some("missing-skill-path".to_string()),
            plugin_name: None,
            git_ref: None,
            skill_path: None,
            default_available_agent_count: None,
            private_adapted_agent_count: None,
            duplicate_copy_count: None,
            default_available_agents: None,
            private_adapted_agents: None,
            duplicate_copy_agents: None,
            private_only_agents: None,
            private_copy_agents: None,
        };

        assert_eq!(skill.can_run_update, Some(true));
        assert_eq!(skill.can_check_for_updates, Some(false));
        assert_eq!(skill.update_reason.as_deref(), Some("missing-skill-path"));
    }

    #[test]
    fn test_read_skill_content_returns_body_without_frontmatter() {
        let content =
            "---\nname: test\ndescription: A test\n---\n\n# Test Skill\n\nBody content here.\n";
        let dir = tempdir().unwrap();
        let skill_dir = dir.path().join("test-skill");
        fs::create_dir(&skill_dir).unwrap();
        fs::write(skill_dir.join("SKILL.md"), content).unwrap();

        let result = read_skill_content(&skill_dir.to_string_lossy()).unwrap();
        assert!(result.starts_with("# Test Skill"));
        assert!(result.contains("Body content here."));
        assert!(!result.contains("---"));
        assert!(!result.contains("name: test"));
    }

    #[test]
    fn test_read_skill_content_missing_skill_md() {
        let dir = tempdir().unwrap();
        let result = read_skill_content(&dir.path().to_string_lossy());
        assert!(result.is_err());
    }

    #[test]
    fn test_read_skill_content_no_frontmatter() {
        let dir = tempdir().unwrap();
        let skill_dir = dir.path().join("plain");
        fs::create_dir(&skill_dir).unwrap();
        fs::write(
            skill_dir.join("SKILL.md"),
            "# Just content\n\nNo frontmatter.",
        )
        .unwrap();

        let result = read_skill_content(&skill_dir.to_string_lossy()).unwrap();
        assert!(result.starts_with("# Just content"));
    }
}

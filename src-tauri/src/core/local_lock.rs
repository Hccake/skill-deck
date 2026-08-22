//! 项目级 `skills-lock.json` 条目模型。
//!
//! 文件读取与无损更新由 Scope 规划和 lock Module 负责；此处只维护与 CLI 共享的
//! `LocalSkillLockEntry` 以及 Skill Deck 使用的附加来源字段。

use serde::{Deserialize, Serialize};

/// Local Skill Lock 条目
/// 对应 CLI: LocalSkillLockEntry；共享字段必须保持 CLI 语义。
/// GUI 额外建模 source_url、remote_hash 和 plugin_name，用于增强检测和展示。
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalSkillLockEntry {
    /// 来源标识符 (owner/repo, npm 包名, 本地路径)
    pub source: String,
    /// Branch or tag ref used for installation
    #[serde(rename = "ref", skip_serializing_if = "Option::is_none")]
    pub ref_name: Option<String>,
    /// 来源类型 ("github", "local" 等)
    pub source_type: String,
    /// 原始来源 URL（GUI 扩展，用于 SSH/private repo 保真）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub well_known_digest: Option<String>,
    /// SHA-256 本地文件内容哈希
    pub computed_hash: String,

    /// GUI 扩展字段：远端/来源版本追踪 hash，用于项目级更新检测
    /// CLI project update 不依赖此字段；缺失时 GUI 应降级为可重装但不可提前检测。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remote_hash: Option<String>,

    /// CLI/GUI 共享字段：仓库内的 SKILL.md 相对路径
    /// CLI project update 依赖它构造定点 reinstall source。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skill_path: Option<String>,

    /// CLI/GUI 共享 Eve 扩展：该 skill 安装到的 Eve subagent targets。
    /// 空字符串表示 Eve root agent (`agent/skills`)。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subagents: Option<Vec<String>>,

    /// GUI 扩展字段：所属 plugin 名称
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plugin_name: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_remote_hash_skip_serialization() {
        let entry = LocalSkillLockEntry {
            source: "owner/repo".to_string(),
            ref_name: None,
            source_type: "github".to_string(),
            source_url: None,
            well_known_digest: None,
            computed_hash: "abc123".to_string(),
            remote_hash: None,
            skill_path: None,
            subagents: None,
            plugin_name: None,
        };
        let json = serde_json::to_string(&entry).unwrap();
        assert!(
            !json.contains("remoteHash"),
            "None remote_hash should not be serialized"
        );
        assert!(
            !json.contains("skillPath"),
            "None skill_path should not be serialized"
        );

        let entry_with_hash = LocalSkillLockEntry {
            remote_hash: Some("tree-sha".to_string()),
            skill_path: Some("skills/test/SKILL.md".to_string()),
            ..entry
        };
        let json = serde_json::to_string(&entry_with_hash).unwrap();
        assert!(
            json.contains("remoteHash"),
            "Some remote_hash should be serialized"
        );
        assert!(
            json.contains("skillPath"),
            "Some skill_path should be serialized"
        );
    }

    #[test]
    fn test_ref_name_serialization() {
        let entry = LocalSkillLockEntry {
            source: "owner/repo".to_string(),
            ref_name: Some("feature-branch".to_string()),
            source_type: "github".to_string(),
            source_url: None,
            well_known_digest: None,
            computed_hash: "abc123".to_string(),
            remote_hash: None,
            skill_path: None,
            subagents: None,
            plugin_name: None,
        };
        let json = serde_json::to_string(&entry).unwrap();
        assert!(json.contains(r#""ref":"feature-branch"#));

        let deserialized: LocalSkillLockEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.ref_name, Some("feature-branch".to_string()));

        let entry_no_ref = LocalSkillLockEntry {
            ref_name: None,
            ..entry.clone()
        };
        let json_no_ref = serde_json::to_string(&entry_no_ref).unwrap();
        assert!(!json_no_ref.contains("ref"));
    }

    #[test]
    fn test_cli_lock_format_compat() {
        let cli_json = r#"{
            "source": "owner/repo",
            "ref": "main",
            "sourceType": "github",
            "computedHash": "abc123"
        }"#;
        let entry: LocalSkillLockEntry = serde_json::from_str(cli_json).unwrap();
        assert_eq!(entry.ref_name, Some("main".to_string()));
        assert_eq!(entry.source_url, None);
        assert_eq!(entry.remote_hash, None);
    }

    #[test]
    fn test_local_lock_entry_round_trips_eve_subagents() {
        let json = r#"{
          "source": "vercel/eve",
          "ref": "main",
          "sourceType": "github",
          "computedHash": "abc",
          "skillPath": "SKILL.md",
          "subagents": ["", "research"]
        }"#;

        let entry: LocalSkillLockEntry = serde_json::from_str(json).unwrap();
        assert_eq!(
            entry.subagents,
            Some(vec!["".to_string(), "research".to_string()])
        );

        let written = serde_json::to_string(&entry).unwrap();
        assert!(written.contains("\"subagents\""));
    }
}

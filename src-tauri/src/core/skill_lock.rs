// .skill-lock.json 读取

use serde::{Deserialize, Serialize};

use super::paths::PATHS;

/// Skill Lock 条目
/// 对应 CLI: SkillLockEntry (skill-lock.ts:14-33)
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillLockEntry {
    /// 规范化的来源标识符 (e.g., "owner/repo")
    pub source: String,
    /// 来源类型 (e.g., "github", "mintlify", "local")
    pub source_type: String,
    /// 原始安装 URL
    #[serde(default)]
    pub source_url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_base_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub well_known_digest: Option<String>,
    /// Branch or tag ref used for installation
    #[serde(rename = "ref", skip_serializing_if = "Option::is_none")]
    pub ref_name: Option<String>,
    /// 仓库内的子路径
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skill_path: Option<String>,
    /// 来源版本追踪 hash。GitHub 来源通常是远端 tree SHA；
    /// 非 GitHub git 来源可能是安装来源目录的内容 hash。
    #[serde(default)]
    pub skill_folder_hash: String,
    /// 安装时间 (ISO 格式)
    #[serde(default)]
    pub installed_at: String,
    /// 更新时间 (ISO 格式)
    #[serde(default)]
    pub updated_at: String,
    /// 所属 plugin 名称
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plugin_name: Option<String>,
}

/// 获取 skill-lock.json 路径
/// 对应 CLI: getSkillLockPath (skill-lock.ts:61-63)
pub fn get_skill_lock_path() -> std::path::PathBuf {
    if let Ok(xdg_state_home) = std::env::var("XDG_STATE_HOME") {
        let trimmed = xdg_state_home.trim();
        if !trimmed.is_empty() {
            return std::path::PathBuf::from(trimmed)
                .join("skills")
                .join(".skill-lock.json");
        }
    }
    PATHS.home.join(".agents").join(".skill-lock.json")
}

#[cfg(test)]
mod tests {
    use super::*;
    use once_cell::sync::Lazy;
    use std::sync::Mutex;
    use tempfile::tempdir;

    static ENV_LOCK: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));

    #[test]
    fn test_get_skill_lock_path() {
        let _guard = ENV_LOCK.lock().unwrap();
        let original = std::env::var("XDG_STATE_HOME").ok();
        std::env::remove_var("XDG_STATE_HOME");

        let path = get_skill_lock_path();
        assert!(path.to_string_lossy().contains(".agents"));
        assert!(path.to_string_lossy().contains(".skill-lock.json"));

        if let Some(value) = original {
            std::env::set_var("XDG_STATE_HOME", value);
        }
    }

    #[test]
    fn test_get_skill_lock_path_uses_xdg_state_home_when_set() {
        let _guard = ENV_LOCK.lock().unwrap();
        let original = std::env::var("XDG_STATE_HOME").ok();
        let temp = tempdir().unwrap();
        let xdg_state_home = temp.path().join("state");
        std::env::set_var("XDG_STATE_HOME", &xdg_state_home);

        let path = get_skill_lock_path();
        assert_eq!(path, xdg_state_home.join("skills").join(".skill-lock.json"));

        if let Some(value) = original {
            std::env::set_var("XDG_STATE_HOME", value);
        } else {
            std::env::remove_var("XDG_STATE_HOME");
        }
    }

    #[test]
    fn test_deserialize_skill_lock_entry() {
        let json = r#"{
            "source": "owner/repo",
            "sourceType": "github",
            "sourceUrl": "https://github.com/owner/repo",
            "skillFolderHash": "abc123",
            "installedAt": "2024-01-01T00:00:00Z",
            "updatedAt": "2024-01-01T00:00:00Z"
        }"#;

        let entry: SkillLockEntry = serde_json::from_str(json).unwrap();
        assert_eq!(entry.source, "owner/repo");
        assert_eq!(entry.source_type, "github");
        assert!(entry.skill_path.is_none());
    }

    #[test]
    fn test_deserialize_skill_lock_entry_allows_missing_skill_folder_hash() {
        let json = r#"{
            "source": "owner/repo",
            "sourceType": "github",
            "sourceUrl": "https://github.com/owner/repo",
            "installedAt": "2024-01-01T00:00:00Z",
            "updatedAt": "2024-01-01T00:00:00Z"
        }"#;

        let entry: SkillLockEntry = serde_json::from_str(json).unwrap();
        assert_eq!(entry.source, "owner/repo");
        assert_eq!(entry.skill_folder_hash, "");
    }

    #[test]
    fn test_deserialize_skill_lock_entry_allows_missing_source_url() {
        let json = r#"{
            "source": "owner/repo",
            "sourceType": "github",
            "skillFolderHash": "abc123",
            "installedAt": "2024-01-01T00:00:00Z",
            "updatedAt": "2024-01-01T00:00:00Z"
        }"#;

        let entry: SkillLockEntry = serde_json::from_str(json).unwrap();
        assert_eq!(entry.source, "owner/repo");
        assert_eq!(entry.source_url, "");
    }

    #[test]
    fn test_deserialize_skill_lock_entry_allows_missing_installed_and_updated_at() {
        let json = r#"{
            "source": "owner/repo",
            "sourceType": "github",
            "sourceUrl": "https://github.com/owner/repo"
        }"#;

        let entry: SkillLockEntry = serde_json::from_str(json).unwrap();
        assert_eq!(entry.source, "owner/repo");
        assert_eq!(entry.installed_at, "");
        assert_eq!(entry.updated_at, "");
    }
}

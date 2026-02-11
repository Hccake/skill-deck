// .skill-lock.json 读取

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::paths::PATHS;
use crate::error::AppError;

/// Lock 文件版本号
/// 对应 CLI: CURRENT_VERSION = 3 (skill-lock.ts:9)
const CURRENT_VERSION: u32 = 3;

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
    pub source_url: String,
    /// 仓库内的子路径
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skill_path: Option<String>,
    /// GitHub tree SHA（用于更新检测）
    pub skill_folder_hash: String,
    /// 安装时间 (ISO 格式)
    pub installed_at: String,
    /// 更新时间 (ISO 格式)
    pub updated_at: String,
}

/// 已忽略的提示
/// 对应 CLI: DismissedPrompts (skill-lock.ts:38-41)
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DismissedPrompts {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub find_skills_prompt: Option<bool>,
}

/// Skill Lock 文件结构
/// 对应 CLI: SkillLockFile (skill-lock.ts:46-55)
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillLockFile {
    pub version: u32,
    pub skills: HashMap<String, SkillLockEntry>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dismissed: Option<DismissedPrompts>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_selected_agents: Option<Vec<String>>,
}

impl SkillLockFile {
    /// 创建空的 lock 文件
    /// 对应 CLI: createEmptyLockFile (skill-lock.ts:300-306)
    pub fn empty() -> Self {
        Self {
            version: CURRENT_VERSION,
            skills: HashMap::new(),
            dismissed: None,
            last_selected_agents: None,
        }
    }
}

/// 获取 skill-lock.json 路径
/// 对应 CLI: getSkillLockPath (skill-lock.ts:61-63)
pub fn get_skill_lock_path() -> std::path::PathBuf {
    PATHS.home.join(".agents").join(".skill-lock.json")
}

/// 读取 skill-lock.json
/// 对应 CLI: readSkillLock (skill-lock.ts:70-93)
pub fn read_skill_lock() -> Result<SkillLockFile, AppError> {
    let path = get_skill_lock_path();

    if !path.exists() {
        return Ok(SkillLockFile::empty());
    }

    let content = std::fs::read_to_string(&path)?;
    let lock: SkillLockFile = match serde_json::from_str(&content) {
        Ok(l) => l,
        Err(_) => return Ok(SkillLockFile::empty()),
    };

    // 版本检查：旧版本返回空（与 CLI 行为一致）
    // 对应 CLI: skill-lock.ts 第 84-86 行
    if lock.version < CURRENT_VERSION {
        return Ok(SkillLockFile::empty());
    }

    Ok(lock)
}

/// 获取指定 skill 的 lock 条目
/// 对应 CLI: getSkillFromLock (skill-lock.ts:263-266)
pub fn get_skill_from_lock(skill_name: &str) -> Result<Option<SkillLockEntry>, AppError> {
    let lock = read_skill_lock()?;
    Ok(lock.skills.get(skill_name).cloned())
}

/// 获取所有 locked skills
/// 对应 CLI: getAllLockedSkills (skill-lock.ts:271-274)
pub fn get_all_locked_skills() -> Result<HashMap<String, SkillLockEntry>, AppError> {
    let lock = read_skill_lock()?;
    Ok(lock.skills)
}

/// 写入 skill-lock.json
/// 对应 CLI: writeSkillLock (skill-lock.ts:99-108)
pub fn write_skill_lock(lock: &SkillLockFile) -> Result<(), AppError> {
    let lock_path = get_skill_lock_path();

    // 确保目录存在
    if let Some(parent) = lock_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    // 序列化为 pretty JSON 并写入
    let content = serde_json::to_string_pretty(lock)?;
    std::fs::write(&lock_path, content)?;

    Ok(())
}

/// 添加或更新 skill 到 lock 文件
/// 对应 CLI: addSkillToLock (skill-lock.ts:227-242)
pub fn add_skill_to_lock(
    skill_name: &str,
    source: &str,
    source_type: &str,
    source_url: &str,
    skill_path: Option<&str>,
    skill_folder_hash: &str,
) -> Result<(), AppError> {
    let mut lock = read_skill_lock().unwrap_or_else(|_| SkillLockFile::empty());

    let now = chrono::Utc::now().to_rfc3339();

    // 保留原有的 installed_at
    let installed_at = lock
        .skills
        .get(skill_name)
        .map(|e| e.installed_at.clone())
        .unwrap_or_else(|| now.clone());

    let entry = SkillLockEntry {
        source: source.to_string(),
        source_type: source_type.to_string(),
        source_url: source_url.to_string(),
        skill_path: skill_path.map(|s| s.to_string()),
        skill_folder_hash: skill_folder_hash.to_string(),
        installed_at,
        updated_at: now,
    };

    lock.skills.insert(skill_name.to_string(), entry);

    write_skill_lock(&lock)
}

/// 从 lock 文件移除 skill
/// 对应 CLI: removeSkillFromLock (skill-lock.ts:247-254)
pub fn remove_skill_from_lock(skill_name: &str) -> Result<bool, AppError> {
    let mut lock = read_skill_lock()?;

    if lock.skills.remove(skill_name).is_none() {
        return Ok(false);
    }

    write_skill_lock(&lock)?;
    Ok(true)
}

/// 保存最后选择的 agents
/// 对应 CLI: saveLastSelectedAgents (skill-lock.ts:282-287)
pub fn save_selected_agents(agents: &[String]) -> Result<(), AppError> {
    let mut lock = read_skill_lock().unwrap_or_else(|_| SkillLockFile::empty());
    lock.last_selected_agents = Some(agents.to_vec());
    write_skill_lock(&lock)
}

/// 获取最后选择的 agents
/// 对应 CLI: getLastSelectedAgents (skill-lock.ts:292-295)
pub fn get_last_selected_agents() -> Option<Vec<String>> {
    read_skill_lock()
        .ok()
        .and_then(|lock| lock.last_selected_agents)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_lock_file() {
        let lock = SkillLockFile::empty();
        assert_eq!(lock.version, CURRENT_VERSION);
        assert!(lock.skills.is_empty());
    }

    #[test]
    fn test_get_skill_lock_path() {
        let path = get_skill_lock_path();
        assert!(path.to_string_lossy().contains(".agents"));
        assert!(path.to_string_lossy().contains(".skill-lock.json"));
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
    fn test_deserialize_skill_lock_file() {
        let json = r#"{
            "version": 3,
            "skills": {
                "test-skill": {
                    "source": "owner/repo",
                    "sourceType": "github",
                    "sourceUrl": "https://github.com/owner/repo",
                    "skillFolderHash": "abc123",
                    "installedAt": "2024-01-01T00:00:00Z",
                    "updatedAt": "2024-01-01T00:00:00Z"
                }
            }
        }"#;

        let lock: SkillLockFile = serde_json::from_str(json).unwrap();
        assert_eq!(lock.version, 3);
        assert_eq!(lock.skills.len(), 1);
        assert!(lock.skills.contains_key("test-skill"));
    }

    #[test]
    fn test_serialize_skill_lock_file() {
        let lock = SkillLockFile::empty();
        let json = serde_json::to_string(&lock).unwrap();
        assert!(json.contains("\"version\":3"));
        assert!(json.contains("\"skills\":{}"));
        // 空的 Option 字段不应该被序列化
        assert!(!json.contains("dismissed"));
        assert!(!json.contains("lastSelectedAgents"));
    }
}

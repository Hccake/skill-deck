//! 项目级 skills-lock.json 管理
//!
//! 对应 CLI: local-lock.ts
//! 设计意图：
//! - 项目根目录 skills-lock.json，便于 git 版本控制
//! - SHA-256 本地文件哈希（非 GitHub tree SHA）
//! - BTreeMap 按 key 排序，最小化 git diff
//! - GUI 扩展字段 remote_hash 用于更新检测

use crate::error::AppError;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Local lock 文件版本号
/// 对应 CLI: CURRENT_VERSION = 1 (local-lock.ts:6)
const LOCAL_LOCK_VERSION: u32 = 1;

/// Local lock 文件名
const LOCAL_LOCK_FILENAME: &str = "skills-lock.json";

/// 旧版项目级 lock 路径（向后兼容读取）
const LEGACY_PROJECT_LOCK_PATH: &str = ".agents/.skill-lock.json";

/// Local Skill Lock 条目
/// 对应 CLI: LocalSkillLockEntry (local-lock.ts:8-12)
/// GUI 扩展了 remote_hash 和 skill_path 字段（CLI 会忽略未知字段）
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalSkillLockEntry {
    /// 来源标识符 (owner/repo, npm 包名, 本地路径)
    pub source: String,
    /// 来源类型 ("github", "local" 等)
    pub source_type: String,
    /// SHA-256 本地文件内容哈希
    pub computed_hash: String,

    /// GUI 扩展字段：GitHub tree SHA（用于更新检测）
    /// CLI 会忽略此字段
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remote_hash: Option<String>,

    /// GUI 扩展字段：仓库内的 skill 子路径（用于更新检测）
    /// CLI 会忽略此字段
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skill_path: Option<String>,
}

/// Local Skill Lock 文件
/// 对应 CLI: LocalSkillLockFile (local-lock.ts:14-17)
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalSkillLockFile {
    pub version: u32,
    pub skills: BTreeMap<String, LocalSkillLockEntry>,
}

impl LocalSkillLockFile {
    pub fn empty() -> Self {
        Self {
            version: LOCAL_LOCK_VERSION,
            skills: BTreeMap::new(),
        }
    }
}

/// 获取项目级 lock 文件路径
/// 优先使用新格式 skills-lock.json
fn get_local_lock_path(project_path: &str) -> PathBuf {
    PathBuf::from(project_path).join(LOCAL_LOCK_FILENAME)
}

/// 获取旧版项目级 lock 文件路径（向后兼容）
fn get_legacy_lock_path(project_path: &str) -> PathBuf {
    PathBuf::from(project_path).join(LEGACY_PROJECT_LOCK_PATH)
}

/// 读取项目级 lock 文件
/// 对应 CLI: readLocalLock (local-lock.ts:19-38)
///
/// 优先读取 skills-lock.json，不存在则回退读取 .agents/.skill-lock.json
pub fn read_local_lock(project_path: &str) -> Result<LocalSkillLockFile, AppError> {
    let new_path = get_local_lock_path(project_path);

    // 优先读新格式
    if new_path.exists() {
        let content = fs::read_to_string(&new_path)?;
        return match serde_json::from_str::<LocalSkillLockFile>(&content) {
            Ok(lock) if lock.version >= LOCAL_LOCK_VERSION => Ok(lock),
            _ => Ok(LocalSkillLockFile::empty()),
        };
    }

    // 回退读旧格式并转换
    let legacy_path = get_legacy_lock_path(project_path);
    if legacy_path.exists() {
        return read_and_convert_legacy_lock(&legacy_path);
    }

    Ok(LocalSkillLockFile::empty())
}

/// 读取旧版 lock 文件并转换为新格式
/// 旧版使用 SkillLockFile 格式（GitHub tree SHA），需要转换
fn read_and_convert_legacy_lock(path: &Path) -> Result<LocalSkillLockFile, AppError> {
    use crate::core::skill_lock::SkillLockFile;

    let content = fs::read_to_string(path)?;
    let old_lock: SkillLockFile = match serde_json::from_str(&content) {
        Ok(l) => l,
        Err(_) => return Ok(LocalSkillLockFile::empty()),
    };

    let mut new_lock = LocalSkillLockFile::empty();
    for (name, entry) in old_lock.skills {
        new_lock.skills.insert(
            name,
            LocalSkillLockEntry {
                source: entry.source,
                source_type: entry.source_type,
                computed_hash: String::new(), // 旧版没有 SHA-256，留空
                remote_hash: if entry.skill_folder_hash.is_empty() {
                    None
                } else {
                    Some(entry.skill_folder_hash)
                },
                skill_path: entry.skill_path,
            },
        );
    }

    Ok(new_lock)
}

/// 写入项目级 lock 文件
/// 对应 CLI: writeLocalLock (local-lock.ts:40-53)
///
/// - BTreeMap 自动按 key 排序
/// - 尾部添加换行符
pub fn write_local_lock(
    lock: &LocalSkillLockFile,
    project_path: &str,
) -> Result<(), AppError> {
    let lock_path = get_local_lock_path(project_path);
    if let Some(parent) = lock_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let content = serde_json::to_string_pretty(lock)? + "\n";
    fs::write(&lock_path, content)?;
    Ok(())
}

/// 添加 skill 到项目级 lock 文件
/// 对应 CLI: addSkillToLocalLock (local-lock.ts:55-68)
pub fn add_skill_to_local_lock(
    skill_name: &str,
    entry: LocalSkillLockEntry,
    project_path: &str,
) -> Result<(), AppError> {
    let mut lock = read_local_lock(project_path)?;
    lock.skills.insert(skill_name.to_string(), entry);
    write_local_lock(&lock, project_path)
}

/// 从项目级 lock 文件移除 skill
/// 对应 CLI: removeSkillFromLocalLock (local-lock.ts:70-79)
pub fn remove_skill_from_local_lock(
    skill_name: &str,
    project_path: &str,
) -> Result<bool, AppError> {
    let mut lock = read_local_lock(project_path)?;
    if lock.skills.remove(skill_name).is_none() {
        return Ok(false);
    }
    write_local_lock(&lock, project_path)?;
    Ok(true)
}

/// 计算 skill 文件夹的 SHA-256 哈希
/// 对应 CLI: computeSkillFolderHash (local-lock.ts:98-113)
///
/// 算法：
/// 1. 递归收集所有文件（跳过 .git, node_modules）
/// 2. 按相对路径排序
/// 3. 依次 hash(相对路径 + 文件内容)
pub fn compute_skill_folder_hash(skill_dir: &Path) -> Result<String, AppError> {
    let mut files: Vec<(String, Vec<u8>)> = Vec::new();
    collect_files(skill_dir, skill_dir, &mut files)?;

    // 按相对路径排序确保确定性
    files.sort_by(|a, b| a.0.cmp(&b.0));

    let mut hasher = Sha256::new();
    for (relative_path, content) in &files {
        hasher.update(relative_path.as_bytes());
        hasher.update(content);
    }

    Ok(format!("{:x}", hasher.finalize()))
}

/// 递归收集目录下所有文件
/// 对应 CLI: collectFiles (local-lock.ts:115-137)
fn collect_files(
    base_dir: &Path,
    current_dir: &Path,
    files: &mut Vec<(String, Vec<u8>)>,
) -> Result<(), AppError> {
    let entries = fs::read_dir(current_dir)?;

    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        let file_name = entry.file_name().to_string_lossy().to_string();

        // 跳过 .git 和 node_modules
        if file_name == ".git" || file_name == "node_modules" {
            continue;
        }

        if path.is_dir() {
            collect_files(base_dir, &path, files)?;
        } else {
            let relative = path
                .strip_prefix(base_dir)
                .unwrap_or(&path)
                .to_string_lossy()
                // 统一使用正斜杠，确保跨平台一致性
                .replace('\\', "/");
            let content = fs::read(&path)?;
            files.push((relative, content));
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_empty_local_lock() {
        let lock = LocalSkillLockFile::empty();
        assert_eq!(lock.version, LOCAL_LOCK_VERSION);
        assert!(lock.skills.is_empty());
    }

    #[test]
    fn test_local_lock_serialization_order() {
        let mut lock = LocalSkillLockFile::empty();
        lock.skills.insert(
            "z-skill".to_string(),
            LocalSkillLockEntry {
                source: "owner/z".to_string(),
                source_type: "github".to_string(),
                computed_hash: "hash-z".to_string(),
                remote_hash: None,
                skill_path: None,
            },
        );
        lock.skills.insert(
            "a-skill".to_string(),
            LocalSkillLockEntry {
                source: "owner/a".to_string(),
                source_type: "github".to_string(),
                computed_hash: "hash-a".to_string(),
                remote_hash: None,
                skill_path: None,
            },
        );

        let json = serde_json::to_string_pretty(&lock).unwrap();
        let a_pos = json.find("a-skill").unwrap();
        let z_pos = json.find("z-skill").unwrap();
        assert!(a_pos < z_pos, "Skills should be sorted alphabetically (BTreeMap)");
    }

    #[test]
    fn test_remote_hash_skip_serialization() {
        let entry = LocalSkillLockEntry {
            source: "owner/repo".to_string(),
            source_type: "github".to_string(),
            computed_hash: "abc123".to_string(),
            remote_hash: None,
            skill_path: None,
        };
        let json = serde_json::to_string(&entry).unwrap();
        assert!(!json.contains("remoteHash"), "None remote_hash should not be serialized");
        assert!(!json.contains("skillPath"), "None skill_path should not be serialized");

        let entry_with_hash = LocalSkillLockEntry {
            remote_hash: Some("tree-sha".to_string()),
            skill_path: Some("skills/test/SKILL.md".to_string()),
            ..entry
        };
        let json = serde_json::to_string(&entry_with_hash).unwrap();
        assert!(json.contains("remoteHash"), "Some remote_hash should be serialized");
        assert!(json.contains("skillPath"), "Some skill_path should be serialized");
    }

    #[test]
    fn test_compute_skill_folder_hash() {
        let temp = tempdir().unwrap();
        let skill_dir = temp.path().join("my-skill");
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(skill_dir.join("SKILL.md"), "---\nname: test\n---\n").unwrap();
        fs::write(skill_dir.join("prompt.md"), "Hello world").unwrap();

        let hash = compute_skill_folder_hash(&skill_dir).unwrap();
        assert_eq!(hash.len(), 64, "SHA-256 hex should be 64 chars");

        // 相同内容应产生相同哈希
        let hash2 = compute_skill_folder_hash(&skill_dir).unwrap();
        assert_eq!(hash, hash2, "Same content should produce same hash");
    }

    #[test]
    fn test_compute_hash_excludes_git() {
        let temp = tempdir().unwrap();
        let skill_dir = temp.path().join("my-skill");
        fs::create_dir_all(skill_dir.join(".git")).unwrap();
        fs::write(skill_dir.join(".git/config"), "git stuff").unwrap();
        fs::write(skill_dir.join("SKILL.md"), "---\nname: test\n---\n").unwrap();

        let hash_with_git = compute_skill_folder_hash(&skill_dir).unwrap();

        // 删除 .git 目录
        fs::remove_dir_all(skill_dir.join(".git")).unwrap();
        let hash_without_git = compute_skill_folder_hash(&skill_dir).unwrap();

        assert_eq!(hash_with_git, hash_without_git, ".git should be excluded");
    }

    #[test]
    fn test_read_write_local_lock() {
        let temp = tempdir().unwrap();
        let project_path = temp.path().to_string_lossy().to_string();

        let mut lock = LocalSkillLockFile::empty();
        lock.skills.insert(
            "test-skill".to_string(),
            LocalSkillLockEntry {
                source: "owner/repo".to_string(),
                source_type: "github".to_string(),
                computed_hash: "abc123".to_string(),
                remote_hash: Some("tree-sha".to_string()),
                skill_path: Some("skills/test/SKILL.md".to_string()),
            },
        );

        write_local_lock(&lock, &project_path).unwrap();

        // 验证文件存在
        let lock_path = get_local_lock_path(&project_path);
        assert!(lock_path.exists());

        // 验证尾部换行符
        let content = fs::read_to_string(&lock_path).unwrap();
        assert!(content.ends_with('\n'), "Should end with newline");

        // 读回
        let read_lock = read_local_lock(&project_path).unwrap();
        assert_eq!(read_lock.skills.len(), 1);
        assert!(read_lock.skills.contains_key("test-skill"));
        assert_eq!(
            read_lock.skills["test-skill"].remote_hash,
            Some("tree-sha".to_string())
        );
    }

    #[test]
    fn test_add_remove_local_lock() {
        let temp = tempdir().unwrap();
        let project_path = temp.path().to_string_lossy().to_string();

        // 添加
        add_skill_to_local_lock(
            "my-skill",
            LocalSkillLockEntry {
                source: "owner/repo".to_string(),
                source_type: "github".to_string(),
                computed_hash: "hash1".to_string(),
                remote_hash: None,
                skill_path: None,
            },
            &project_path,
        )
        .unwrap();

        let lock = read_local_lock(&project_path).unwrap();
        assert_eq!(lock.skills.len(), 1);

        // 移除
        let removed = remove_skill_from_local_lock("my-skill", &project_path).unwrap();
        assert!(removed);

        let lock = read_local_lock(&project_path).unwrap();
        assert!(lock.skills.is_empty());

        // 再次移除不存在的
        let removed = remove_skill_from_local_lock("my-skill", &project_path).unwrap();
        assert!(!removed);
    }
}

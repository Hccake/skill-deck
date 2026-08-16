// .skill-lock.json 读取

use serde::{Deserialize, Serialize};
use specta::Type;
#[cfg(test)]
use std::collections::HashMap;
use std::collections::HashSet;

use super::paths::PATHS;
use crate::core::agent_definition::{AgentDefinition, AgentId, AgentSource, ScopeDefinition};
use crate::core::agent_registry::AgentRegistrySnapshot;

/// Lock 文件版本号
/// 对应 CLI: CURRENT_VERSION = 3 (skill-lock.ts:9)
#[cfg(test)]
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

/// 已忽略的提示
/// 对应 CLI: DismissedPrompts (skill-lock.ts:38-41)
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg(test)]
pub struct DismissedPrompts {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub find_skills_prompt: Option<bool>,
}

/// GUI 使用的 scope-aware 默认安装目标
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct DefaultTargetAgents {
    pub global: Vec<String>,
    pub project: Vec<String>,
}

pub fn effective_default_target_agents(
    stored: &DefaultTargetAgents,
    snapshot: &AgentRegistrySnapshot,
) -> DefaultTargetAgents {
    DefaultTargetAgents {
        global: effective_scope_defaults(&stored.global, snapshot, |definition| &definition.global),
        project: effective_scope_defaults(&stored.project, snapshot, |definition| {
            &definition.project
        }),
    }
}

pub fn builtin_last_selected_projection(
    effective: &DefaultTargetAgents,
    snapshot: &AgentRegistrySnapshot,
) -> Vec<String> {
    let mut seen = HashSet::new();
    effective
        .global
        .iter()
        .chain(&effective.project)
        .filter(|id| seen.insert((*id).clone()))
        .filter(|id| {
            AgentId::parse((*id).clone())
                .ok()
                .and_then(|id| snapshot.active_definitions.get(&id))
                .is_some_and(|definition| definition.source == AgentSource::Builtin)
        })
        .cloned()
        .collect()
}

fn effective_scope_defaults(
    stored: &[String],
    snapshot: &AgentRegistrySnapshot,
    scope: impl Fn(&AgentDefinition) -> &ScopeDefinition,
) -> Vec<String> {
    let mut seen = HashSet::new();
    stored
        .iter()
        .filter(|id| seen.insert((*id).clone()))
        .filter(|id| {
            AgentId::parse((*id).clone())
                .ok()
                .and_then(|id| snapshot.active_definitions.get(&id))
                .map(&scope)
                .is_some_and(|scope| {
                    scope.enabled && !scope.reads_standard && scope.private_path.is_some()
                })
        })
        .cloned()
        .collect()
}

/// Skill Lock 文件结构
/// 对应 CLI: SkillLockFile (skill-lock.ts:46-55)
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg(test)]
pub struct SkillLockFile {
    pub version: u32,
    pub skills: HashMap<String, SkillLockEntry>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dismissed: Option<DismissedPrompts>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_selected_agents: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_target_agents: Option<DefaultTargetAgents>,
}

#[cfg(test)]
impl SkillLockFile {
    /// 创建空的 lock 文件
    /// 对应 CLI: createEmptyLockFile (skill-lock.ts:300-306)
    pub fn empty() -> Self {
        Self {
            version: CURRENT_VERSION,
            skills: HashMap::new(),
            dismissed: None,
            last_selected_agents: None,
            default_target_agents: None,
        }
    }
}

#[cfg(test)]
pub(crate) fn parse_skill_lock_file(content: &str) -> Result<SkillLockFile, serde_json::Error> {
    match serde_json::from_str::<SkillLockFile>(content) {
        Ok(lock) => Ok(lock),
        Err(original_error) => {
            let value = serde_json::from_str::<serde_json::Value>(content)?;
            let Some(version) = value
                .get("version")
                .and_then(serde_json::Value::as_u64)
                .map(|version| version as u32)
            else {
                return Err(original_error);
            };
            let dismissed = value
                .get("dismissed")
                .cloned()
                .and_then(|value| serde_json::from_value(value).ok());
            let last_selected_agents = value
                .get("lastSelectedAgents")
                .cloned()
                .and_then(|value| serde_json::from_value(value).ok());
            let default_target_agents = value
                .get("defaultTargetAgents")
                .cloned()
                .and_then(|value| serde_json::from_value(value).ok());
            let Some(skills) = value.get("skills").and_then(serde_json::Value::as_object) else {
                return Err(original_error);
            };

            let skills = skills
                .iter()
                .filter_map(|(name, entry)| {
                    serde_json::from_value::<SkillLockEntry>(entry.clone())
                        .ok()
                        .map(|entry| (name.clone(), entry))
                })
                .collect();

            Ok(SkillLockFile {
                version,
                skills,
                dismissed,
                last_selected_agents,
                default_target_agents,
            })
        }
    }
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

/// 获取指定 scope 的 skill-lock.json 路径
///
/// - Global (None): ~/.agents/.skill-lock.json
/// - Project (Some(path)): <project_path>/.agents/.skill-lock.json
#[cfg(test)]
pub fn get_scoped_lock_path(project_path: Option<&str>) -> std::path::PathBuf {
    match project_path {
        Some(path) => std::path::PathBuf::from(path)
            .join(".agents")
            .join(".skill-lock.json"),
        None => get_skill_lock_path(),
    }
}

/// 读取 skill-lock.json
/// 对应 CLI: readSkillLock (skill-lock.ts:70-93)
#[cfg(test)]
pub fn read_skill_lock() -> Result<SkillLockFile, crate::error::AppError> {
    let path = get_skill_lock_path();

    if !path.exists() {
        return Ok(SkillLockFile::empty());
    }

    let content = std::fs::read_to_string(&path)?;
    let lock: SkillLockFile = match parse_skill_lock_file(&content) {
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

/// 读取指定 scope 的 skill-lock.json
#[cfg(test)]
pub fn read_scoped_lock(
    project_path: Option<&str>,
) -> Result<SkillLockFile, crate::error::AppError> {
    let path = get_scoped_lock_path(project_path);
    if !path.exists() {
        return Ok(SkillLockFile::empty());
    }
    let content = std::fs::read_to_string(&path)?;
    let lock: SkillLockFile = match parse_skill_lock_file(&content) {
        Ok(l) => l,
        Err(_) => return Ok(SkillLockFile::empty()),
    };
    if lock.version < CURRENT_VERSION {
        return Ok(SkillLockFile::empty());
    }
    Ok(lock)
}

/// 获取指定 skill 的 lock 条目
/// 对应 CLI: getSkillFromLock (skill-lock.ts:263-266)
#[cfg(test)]
pub fn get_skill_from_lock(
    skill_name: &str,
) -> Result<Option<SkillLockEntry>, crate::error::AppError> {
    let lock = read_skill_lock()?;
    Ok(lock.skills.get(skill_name).cloned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::agent_definition::{
        AgentAdapter, AgentDefinition, AgentId, AgentSource, DetectionSpec, PathSpec,
        ScopeDefinition,
    };
    use crate::core::agent_registry::{AgentRegistry, AgentRegistrySnapshot};
    use once_cell::sync::Lazy;
    use std::collections::BTreeMap;
    use std::sync::Mutex;
    use tempfile::tempdir;

    static ENV_LOCK: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));

    fn scope(enabled: bool, reads_standard: bool, private_path: bool) -> ScopeDefinition {
        ScopeDefinition {
            enabled,
            reads_standard,
            private_path: private_path.then(|| PathSpec::home(".agent/skills")),
        }
    }

    fn definition(
        id: &str,
        source: AgentSource,
        global: ScopeDefinition,
        project: ScopeDefinition,
    ) -> AgentDefinition {
        AgentDefinition {
            id: AgentId::parse(id).unwrap(),
            display_name: id.to_string(),
            source,
            aliases: Vec::new(),
            global,
            project,
            detection: DetectionSpec::AnyPathExists {
                paths: vec![PathSpec::home(format!(".{id}"))],
            },
            legacy_paths: Vec::new(),
            adapter: AgentAdapter::Standard,
        }
    }

    fn registry_snapshot(definitions: Vec<AgentDefinition>) -> AgentRegistrySnapshot {
        AgentRegistrySnapshot {
            revision: "registry-revision".to_string(),
            active_definitions: definitions
                .into_iter()
                .map(|definition| (definition.id.clone(), definition))
                .collect::<BTreeMap<_, _>>(),
        }
    }

    #[test]
    fn effective_defaults_filter_missing_conflicting_disabled_and_non_private_ids_stably() {
        let private = scope(true, false, true);
        let standard = scope(true, true, false);
        let both = scope(true, true, true);
        let disabled = scope(false, false, true);
        let snapshot = registry_snapshot(vec![
            definition(
                "builtin-private",
                AgentSource::Builtin,
                private.clone(),
                standard.clone(),
            ),
            definition(
                "custom-private",
                AgentSource::Custom,
                private.clone(),
                private.clone(),
            ),
            definition(
                "private-to-both",
                AgentSource::Builtin,
                both,
                standard.clone(),
            ),
            definition(
                "disabled-private",
                AgentSource::Builtin,
                disabled,
                standard.clone(),
            ),
            definition(
                "not-detected-private",
                AgentSource::Builtin,
                private.clone(),
                standard.clone(),
            ),
            definition(
                "indeterminate-private",
                AgentSource::Custom,
                standard.clone(),
                private,
            ),
        ]);
        let stored = DefaultTargetAgents {
            global: vec![
                "builtin-private".to_string(),
                "deleted-agent".to_string(),
                "conflicting-custom".to_string(),
                "private-to-both".to_string(),
                "disabled-private".to_string(),
                "not-detected-private".to_string(),
                "builtin-private".to_string(),
                "custom-private".to_string(),
            ],
            project: vec![
                "indeterminate-private".to_string(),
                "custom-private".to_string(),
                "indeterminate-private".to_string(),
            ],
        };

        assert_eq!(
            effective_default_target_agents(&stored, &snapshot),
            DefaultTargetAgents {
                global: vec![
                    "builtin-private".to_string(),
                    "not-detected-private".to_string(),
                    "custom-private".to_string(),
                ],
                project: vec![
                    "indeterminate-private".to_string(),
                    "custom-private".to_string(),
                ],
            }
        );
    }

    #[test]
    fn builtin_projection_excludes_custom_and_preserves_first_effective_order() {
        let private = scope(true, false, true);
        let disabled = scope(false, false, true);
        let snapshot = registry_snapshot(vec![
            definition(
                "builtin-a",
                AgentSource::Builtin,
                private.clone(),
                private.clone(),
            ),
            definition(
                "builtin-b",
                AgentSource::Builtin,
                private.clone(),
                private.clone(),
            ),
            definition(
                "custom-private",
                AgentSource::Custom,
                private.clone(),
                private,
            ),
            definition(
                "disabled-builtin",
                AgentSource::Builtin,
                disabled.clone(),
                disabled,
            ),
        ]);
        let effective = DefaultTargetAgents {
            global: vec![
                "custom-private".to_string(),
                "builtin-b".to_string(),
                "builtin-a".to_string(),
            ],
            project: vec![
                "builtin-a".to_string(),
                "custom-private".to_string(),
                "builtin-b".to_string(),
            ],
        };

        assert_eq!(
            builtin_last_selected_projection(&effective, &snapshot),
            vec!["builtin-b".to_string(), "builtin-a".to_string()]
        );
    }

    #[test]
    fn test_empty_lock_file() {
        let lock = SkillLockFile::empty();
        assert_eq!(lock.version, CURRENT_VERSION);
        assert!(lock.skills.is_empty());
    }

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
    fn test_deserialize_skill_lock_file_keeps_entry_when_source_url_missing() {
        let json = r#"{
            "version": 3,
            "skills": {
                "test-skill": {
                    "source": "owner/repo",
                    "sourceType": "github",
                    "skillPath": "skills/test-skill/SKILL.md",
                    "skillFolderHash": "abc123",
                    "installedAt": "2024-01-01T00:00:00Z",
                    "updatedAt": "2024-01-01T00:00:00Z"
                }
            }
        }"#;

        let lock: SkillLockFile = serde_json::from_str(json).unwrap();
        let entry = lock.skills.get("test-skill").unwrap();

        assert_eq!(entry.source, "owner/repo");
        assert_eq!(entry.source_url, "");
    }

    #[test]
    fn test_deserialize_skill_lock_file_keeps_entry_when_installed_and_updated_at_missing() {
        let json = r#"{
            "version": 3,
            "skills": {
                "test-skill": {
                    "source": "owner/repo",
                    "sourceType": "github",
                    "sourceUrl": "https://github.com/owner/repo",
                    "skillFolderHash": "abc123"
                }
            }
        }"#;

        let lock: SkillLockFile = serde_json::from_str(json).unwrap();
        let entry = lock.skills.get("test-skill").unwrap();

        assert_eq!(entry.installed_at, "");
        assert_eq!(entry.updated_at, "");
    }

    #[test]
    fn test_parse_skill_lock_file_skips_invalid_entries_without_dropping_valid_entries() {
        let json = r#"{
            "version": 3,
            "skills": {
                "valid-skill": {
                    "source": "owner/repo",
                    "sourceType": "github",
                    "sourceUrl": "https://github.com/owner/repo",
                    "skillFolderHash": "abc123"
                },
                "invalid-skill": {
                    "sourceType": "github",
                    "sourceUrl": "https://github.com/owner/repo"
                }
            },
            "lastSelectedAgents": ["codex"],
            "defaultTargetAgents": {
                "global": ["codex"],
                "project": ["cursor"]
            }
        }"#;

        let lock = parse_skill_lock_file(json).unwrap();

        assert!(lock.skills.contains_key("valid-skill"));
        assert!(!lock.skills.contains_key("invalid-skill"));
        assert_eq!(lock.last_selected_agents, Some(vec!["codex".to_string()]));
        assert_eq!(
            lock.default_target_agents.as_ref().unwrap().project,
            vec!["cursor"]
        );
    }

    #[test]
    fn test_parse_skill_lock_file_rejects_invalid_version_even_when_entries_are_recoverable() {
        let json = r#"{
            "version": "3",
            "skills": {
                "valid-skill": {
                    "source": "owner/repo",
                    "sourceType": "github",
                    "sourceUrl": "https://github.com/owner/repo"
                },
                "invalid-skill": {
                    "sourceType": "github",
                    "sourceUrl": "https://github.com/owner/repo"
                }
            }
        }"#;

        assert!(parse_skill_lock_file(json).is_err());
    }

    #[test]
    fn test_read_scoped_lock_skips_invalid_entries_without_returning_empty_lock() {
        let temp = tempdir().unwrap();
        let project_path = temp.path().to_string_lossy().to_string();
        let lock_path = temp.path().join(".agents").join(".skill-lock.json");
        std::fs::create_dir_all(lock_path.parent().unwrap()).unwrap();
        std::fs::write(
            &lock_path,
            r#"{
  "version": 3,
  "skills": {
    "valid-skill": {
      "source": "owner/repo",
      "sourceType": "github",
      "sourceUrl": "https://github.com/owner/repo"
    },
    "invalid-skill": {
      "source": "owner/repo",
      "sourceUrl": "https://github.com/owner/repo"
    }
  }
}"#,
        )
        .unwrap();

        let lock = read_scoped_lock(Some(&project_path)).unwrap();

        assert!(lock.skills.contains_key("valid-skill"));
        assert!(!lock.skills.contains_key("invalid-skill"));
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

    #[test]
    fn test_deserialize_default_target_agents() {
        let json = r#"{
            "version": 3,
            "skills": {},
            "lastSelectedAgents": ["claude-code"],
            "defaultTargetAgents": {
                "global": ["cursor"],
                "project": ["opencode"]
            }
        }"#;

        let lock: SkillLockFile = serde_json::from_str(json).unwrap();
        let defaults = lock.default_target_agents.unwrap();

        assert_eq!(defaults.global, vec!["cursor"]);
        assert_eq!(defaults.project, vec!["opencode"]);
    }

    #[test]
    fn cli_agent_ids_are_preserved_while_effective_targets_use_the_registry() {
        let lock = parse_skill_lock_file(
            r#"{
                "version": 3,
                "skills": {},
                "lastSelectedAgents": [
                    "grok", "kimchi", "minimax-code", "zcode", "future-agent"
                ],
                "defaultTargetAgents": {
                    "global": ["minimax-code", "future-agent", "grok", "codebuddy"],
                    "project": ["zcode", "future-agent", "kimchi"]
                }
            }"#,
        )
        .unwrap();

        assert_eq!(
            lock.last_selected_agents,
            Some(vec![
                "grok".to_string(),
                "kimchi".to_string(),
                "minimax-code".to_string(),
                "zcode".to_string(),
                "future-agent".to_string(),
            ])
        );
        let stored = lock.default_target_agents.unwrap();
        assert!(stored.global.contains(&"future-agent".to_string()));
        assert!(stored.project.contains(&"future-agent".to_string()));

        let registry = AgentRegistry::new(Vec::new());
        assert_eq!(
            effective_default_target_agents(&stored, registry.snapshot()),
            DefaultTargetAgents {
                global: vec!["minimax-code".to_string(), "codebuddy".to_string()],
                project: vec!["zcode".to_string()],
            }
        );
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

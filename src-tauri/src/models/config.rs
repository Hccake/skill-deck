use serde::{Deserialize, Serialize};
use specta::Type;

fn default_git_clone_timeout_secs() -> u32 {
    120
}

/// Skill Deck 应用配置
/// 持久化到 ~/.skill-deck/config.json
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct SkillDeckConfig {
    /// 已保存的项目路径列表
    #[serde(default)]
    pub projects: Vec<String>,
    /// Git 仓库拉取超时（秒）
    #[serde(default = "default_git_clone_timeout_secs")]
    pub git_clone_timeout_secs: u32,
}

impl Default for SkillDeckConfig {
    fn default() -> Self {
        Self {
            projects: Vec::new(),
            git_clone_timeout_secs: default_git_clone_timeout_secs(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::SkillDeckConfig;

    #[test]
    fn test_default_config_includes_clone_timeout() {
        let config = SkillDeckConfig::default();
        assert_eq!(config.git_clone_timeout_secs, 120);
    }

    #[test]
    fn test_legacy_config_without_timeout_uses_default() {
        let config: SkillDeckConfig =
            serde_json::from_str(r#"{"projects":["/demo"]}"#).expect("config");

        assert_eq!(config.projects, vec!["/demo"]);
        assert_eq!(config.git_clone_timeout_secs, 120);
    }
}

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::environment::types::EnvironmentRef;

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
    /// 是否允许 Skill Deck 发现和使用 WSL Environment
    #[serde(default)]
    pub wsl_integration_enabled: bool,
    #[serde(default)]
    pub hidden_wsl_distros: Vec<String>,
    #[serde(default)]
    pub last_selected_environment: Option<EnvironmentRef>,
    #[serde(default)]
    pub last_connected_wsl_user_by_distro: std::collections::BTreeMap<String, String>,
}

impl Default for SkillDeckConfig {
    fn default() -> Self {
        Self {
            projects: Vec::new(),
            git_clone_timeout_secs: default_git_clone_timeout_secs(),
            wsl_integration_enabled: false,
            hidden_wsl_distros: Vec::new(),
            last_selected_environment: None,
            last_connected_wsl_user_by_distro: std::collections::BTreeMap::new(),
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
        assert!(!config.wsl_integration_enabled);
    }

    #[test]
    fn test_legacy_config_without_timeout_uses_default() {
        let config: SkillDeckConfig =
            serde_json::from_str(r#"{"projects":["/demo"]}"#).expect("config");

        assert_eq!(config.projects, vec!["/demo"]);
        assert_eq!(config.git_clone_timeout_secs, 120);
        assert!(!config.wsl_integration_enabled);
    }
}

use serde::{Deserialize, Serialize};

/// Skill Deck 应用配置
/// 持久化到 ~/.skill-deck/config.json
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SkillDeckConfig {
    /// 已保存的项目路径列表
    #[serde(default)]
    pub projects: Vec<String>,
}

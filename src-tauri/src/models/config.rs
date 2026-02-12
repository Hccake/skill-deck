use serde::{Deserialize, Serialize};
use specta::Type;

/// Skill Deck 应用配置
/// 持久化到 ~/.skill-deck/config.json
#[derive(Debug, Clone, Serialize, Deserialize, Default, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct SkillDeckConfig {
    /// 已保存的项目路径列表
    #[serde(default)]
    pub projects: Vec<String>,
}

use serde::{Deserialize, Serialize};
use specta::Type;

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct DiscoverSearchPayload {
    pub search_json: String,
    pub official_creators: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct DiscoverLeaderboardPayload {
    pub leaderboard_html: String,
    pub official_creators: Option<Vec<String>>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub enum DiscoverLeaderboardTab {
    Popular,
    Trending,
    Hot,
}

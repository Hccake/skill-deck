use tauri::State;

pub use crate::application::discovery::{
    DiscoverLeaderboardPayload, DiscoverLeaderboardTab, DiscoverSearchPayload,
};
use crate::error::AppError;
use crate::runtime::RuntimeServiceGraph;

#[tauri::command]
#[specta::specta]
pub async fn search_discover_skills(
    query: String,
    runtime: State<'_, RuntimeServiceGraph>,
) -> Result<DiscoverSearchPayload, AppError> {
    runtime.discovery().search(&query).await
}

#[tauri::command]
#[specta::specta]
pub async fn get_discover_leaderboard(
    tab: DiscoverLeaderboardTab,
    runtime: State<'_, RuntimeServiceGraph>,
) -> Result<DiscoverLeaderboardPayload, AppError> {
    runtime.discovery().leaderboard(tab).await
}

#[tauri::command]
#[specta::specta]
pub async fn get_discover_skill_detail(
    source: String,
    skill: String,
    runtime: State<'_, RuntimeServiceGraph>,
) -> Result<String, AppError> {
    runtime.discovery().detail(&source, &skill).await
}
